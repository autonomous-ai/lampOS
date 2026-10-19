"""Phonon synthesis worker — runs under the Phonon venv, not HAL's.

Usage: <phonon-venv>/bin/python phonon_worker.py --model DIR [--lang en] [--quant q8] [--threads N] [--cpus 6-7]

Kept free of `hal` imports: it runs with the interpreter that has `ptts` installed
(/opt/phonon/.venv by default), so HAL's Python and numpy stay untouched.

Protocol (stdin = JSON lines, stdout = binary frames, stderr = logs):
  in   {"id": 7, "text": "...", "voice": "Freya"}   synthesize
  in   {"cancel": 7}                                 stop request 7 early
  out  frame = struct ">IBI" (request id, kind, payload length) + payload
       kind 0 READY  JSON {"sample_rate", "voices"} once, id 0, after warm-up
       kind 1 PCM    int16 mono little-endian
       kind 2 END    request finished (also sent after a cancel)
       kind 3 ERROR  UTF-8 message, followed by END
"""

import argparse
import json
import os
import queue
import struct
import sys
import threading

FRAME = struct.Struct(">IBI")
READY, PCM, END, ERROR = 0, 1, 2, 3

_out_lock = threading.Lock()


def _send(req_id: int, kind: int, payload: bytes = b"") -> None:
    with _out_lock:
        sys.stdout.buffer.write(FRAME.pack(req_id, kind, len(payload)) + payload)
        sys.stdout.buffer.flush()


def _log(msg: str) -> None:
    print(f"[phonon-worker] {msg}", file=sys.stderr, flush=True)


def _parse_cpus(spec: str) -> set:
    """"6-7" or "0,6-7" -> {6, 7} / {0, 6, 7}; malformed parts are skipped."""
    cpus = set()
    for part in spec.split(","):
        lo, _, hi = part.strip().partition("-")
        try:
            cpus.update(range(int(lo), int(hi or lo) + 1))
        except ValueError:
            continue
    return cpus


def _pin(spec: str) -> None:
    """Pin this process to the CPUs in `spec` that exist here; no-op if none do."""
    if not spec or not hasattr(os, "sched_setaffinity"):
        return
    allowed = os.sched_getaffinity(0)
    cpus = _parse_cpus(spec) & allowed
    if not cpus:
        _log(f"cpus {spec!r} not on this board ({sorted(allowed)}), not pinning")
        return
    os.sched_setaffinity(0, cpus)
    _log(f"pinned to cpus {sorted(cpus)}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--lang", default="en")
    ap.add_argument("--quant", default="q8")
    ap.add_argument("--threads", type=int, default=0)
    ap.add_argument("--cpus", default="")
    args = ap.parse_args()

    # Before the engine loads, so every thread it starts inherits the mask.
    _pin(args.cpus)

    import numpy as np
    import ptts

    if args.threads > 0:
        ptts.set_num_threads(args.threads)
    tts = ptts.TTS(config=args.model, lang=args.lang, quant=args.quant)
    voices = list(tts.voices)
    # The first synthesis after load is several times slower; pay it before READY.
    with tts.stream("Hi.") as audio:
        for _ in audio:
            pass
    _log(f"ready: quant={tts.quant} sr={tts.sample_rate} voices={voices}")
    _send(0, READY, json.dumps({"sample_rate": tts.sample_rate, "voices": voices}).encode())

    jobs: "queue.Queue" = queue.Queue()
    cancelled: set = set()
    cancel_lock = threading.Lock()

    def _is_cancelled(req_id: int) -> bool:
        with cancel_lock:
            return req_id in cancelled

    def _synth_loop() -> None:
        while True:
            job = jobs.get()
            if job is None:
                return
            req_id = int(job["id"])
            voice = job.get("voice") or None
            if voice is not None and voice not in voices:
                voice = None
            try:
                if not _is_cancelled(req_id):
                    with tts.stream(job.get("text", ""), voice=voice) as audio:
                        for chunk in audio:
                            if _is_cancelled(req_id):
                                break
                            pcm = np.clip(chunk, -1.0, 1.0)
                            _send(req_id, PCM, (pcm * 32767.0).astype("<i2").tobytes())
            except Exception as e:  # report and keep serving
                _send(req_id, ERROR, str(e).encode("utf-8", "replace"))
            finally:
                with cancel_lock:
                    cancelled.discard(req_id)
                _send(req_id, END)

    threading.Thread(target=_synth_loop, daemon=True).start()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            _log(f"bad request line: {line[:80]!r}")
            continue
        if "cancel" in msg:
            with cancel_lock:
                cancelled.add(int(msg["cancel"]))
        elif "id" in msg:
            jobs.put(msg)
    jobs.put(None)
    return 0


if __name__ == "__main__":
    sys.exit(main())
