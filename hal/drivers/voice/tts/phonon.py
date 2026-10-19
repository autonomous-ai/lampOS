"""Phonon TTS backend — Gradium's on-device model, synthesized on the robot.

The engine (`ptts`) lives in its own venv (/opt/phonon/.venv) and the checkpoint in
/opt/phonon/models/phonon; neither ships with HAL. HAL drives one long-lived worker
process (phonon_worker.py) so the model loads and warms up once, and a crash in the
engine cannot take HAL down. Model weights are installed by hand and never downloaded.
"""

import glob
import json
import logging
import os
import queue
import re
import subprocess
import threading
import time
from typing import Callable, Iterator, List, Optional, Tuple

from hal.drivers.voice.tts.backend import TTS_SAMPLE_RATE, TTSBackend

logger = logging.getLogger("hal.voice.tts")

PHONON_DIR = os.environ.get("HAL_PHONON_DIR", "/opt/phonon")
PHONON_PYTHON = os.environ.get("HAL_PHONON_PYTHON", os.path.join(PHONON_DIR, ".venv", "bin", "python"))
PHONON_MODEL_DIR = os.environ.get("HAL_PHONON_MODEL", os.path.join(PHONON_DIR, "models", "phonon"))
# Text normalization only (numbers, symbols): en, fr, de, es, pt or none.
PHONON_LANG = os.environ.get("HAL_PHONON_LANG", "en")
PHONON_QUANT = os.environ.get("HAL_PHONON_QUANT", "q8")
# CPUs the worker is pinned to, e.g. "6-7" or "6,7"; empty = no pinning. 6-7 are the
# A733's two Cortex-A76 cores. Any thread on a Cortex-A55 holds back every chunk:
# on lamp-a0ae all 8 cores ran at RTF 0.55-0.74 and needed a 0.5-0.85 s head start,
# 6-7 with one thread ran at RTF 0.49 with no head start needed. CPUs the board
# lacks are dropped.
PHONON_CPUS = os.environ.get("HAL_PHONON_CPUS", "6-7")
# Engine compute threads; 0 = the engine's default (all cores). One beat two on 6-7.
PHONON_THREADS = int(os.environ.get("HAL_PHONON_THREADS", "1") or 0)
# Head start before a sentence starts playing, as margin for a busy HAL. Pinned to
# 6-7, every chunk arrived before it was due, so 200 ms is enough.
PHONON_PREBUFFER_S = max(0.0, float(os.environ.get("HAL_PHONON_PREBUFFER_MS", "200") or 0) / 1000.0)

WORKER_SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "phonon_worker.py")

# Load + warm-up measured ~2 s on blue-lamp; leave room for a busy CPU.
_READY_TIMEOUT_S = 60.0
# Longest silence between two frames before the worker is treated as hung.
_FRAME_TIMEOUT_S = 30.0
_RESTART_BACKOFF_S = 10.0
# How often a waiting request checks whether the caller cancelled it.
_CANCEL_POLL_S = 0.1

_FRAME_HEADER = 9  # struct ">IBI": request id, kind, payload length
_READY, _PCM, _END, _ERROR = 0, 1, 2, 3

# Phonon interprets no audio tags, so every [direction] goes, not only known ones.
_AUDIO_TAG_RE = re.compile(r"\s*\[[^\[\]]*\]")


def list_voices(model_dir: str = PHONON_MODEL_DIR) -> List[str]:
    """Voice names the checkpoint registers, without loading it.

    Mirrors ptts' loader: files directly in voices/ or embeddings/, plus
    default-voice.safetensors as "default".
    """
    names = set()
    for sub in ("voices", "embeddings"):
        try:
            entries = os.listdir(os.path.join(model_dir, sub))
        except OSError:
            continue
        for f in entries:
            if f.endswith(".safetensors"):
                names.add(f[: -len(".safetensors")])
    if os.path.isfile(os.path.join(model_dir, "default-voice.safetensors")):
        names.add("default")
    return sorted(names)


def engine_installed() -> bool:
    """The venv python plus the ptts package in it (a half-finished install has only the venv)."""
    if not (os.path.isfile(PHONON_PYTHON) and os.access(PHONON_PYTHON, os.X_OK)):
        return False
    venv = os.path.dirname(os.path.dirname(PHONON_PYTHON))
    return bool(glob.glob(os.path.join(venv, "lib", "python3*", "site-packages", "ptts", "__init__.py")))


def model_installed(model_dir: str = PHONON_MODEL_DIR) -> bool:
    return os.path.isfile(os.path.join(model_dir, "config.json"))


def _strip_audio_tags(text: str) -> str:
    """Drop ElevenLabs-style tags such as [curious, warm]; Phonon would read them aloud."""
    return _AUDIO_TAG_RE.sub("", text).strip()


class _Worker:
    """One phonon_worker.py process; requests are served one at a time."""

    def __init__(self, model_dir: str, lang: str, quant: str, threads: int, cpus: str):
        self._argv = [
            PHONON_PYTHON, WORKER_SCRIPT,
            "--model", model_dir, "--lang", lang, "--quant", quant,
            "--threads", str(threads), "--cpus", cpus,
        ]
        self._proc: Optional[subprocess.Popen] = None
        self._frames: "queue.Queue" = queue.Queue()
        self._ready = threading.Event()
        self._start_lock = threading.Lock()
        self._request_lock = threading.Lock()
        self._next_id = 0
        self._last_start = 0.0
        self.sample_rate = TTS_SAMPLE_RATE
        self.voices: List[str] = []

    def _alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def ensure_started(self) -> bool:
        """Start the process if it is not running; return whether it is ready."""
        with self._start_lock:
            if self._alive():
                return self._ready.wait(_READY_TIMEOUT_S)
            if time.monotonic() - self._last_start < _RESTART_BACKOFF_S:
                return False
            self._last_start = time.monotonic()
            self._ready.clear()
            self._frames = queue.Queue()
            try:
                self._proc = subprocess.Popen(
                    self._argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
            except OSError as e:
                logger.warning("Phonon: cannot start worker: %s", e)
                self._proc = None
                return False
            proc, frames = self._proc, self._frames
            threading.Thread(target=self._read_frames, args=(proc, frames),
                             daemon=True, name="phonon-frames").start()
            threading.Thread(target=self._drain_stderr, args=(proc,),
                             daemon=True, name="phonon-stderr").start()
            logger.info("Phonon: worker started (pid=%s)", proc.pid)
        if self._ready.wait(_READY_TIMEOUT_S):
            return True
        logger.warning("Phonon: worker not ready after %.0fs", _READY_TIMEOUT_S)
        return False

    def _read_frames(self, proc: subprocess.Popen, frames: "queue.Queue") -> None:
        out = proc.stdout
        try:
            while True:
                header = out.read(_FRAME_HEADER)  # type: ignore[union-attr]
                if len(header) < _FRAME_HEADER:
                    break
                req_id = int.from_bytes(header[0:4], "big")
                kind = header[4]
                size = int.from_bytes(header[5:9], "big")
                payload = out.read(size) if size else b""  # type: ignore[union-attr]
                if kind == _READY:
                    info = json.loads(payload or b"{}")
                    self.sample_rate = int(info.get("sample_rate") or TTS_SAMPLE_RATE)
                    self.voices = list(info.get("voices") or [])
                    logger.info("Phonon: ready (rate=%dHz, voices=%s)", self.sample_rate, self.voices)
                    self._ready.set()
                    continue
                frames.put((req_id, kind, payload, time.monotonic()))
        except Exception as e:
            logger.warning("Phonon: frame reader stopped: %s", e)
        finally:
            frames.put(None)
            logger.warning("Phonon: worker exited (code=%s)", proc.poll())

    @staticmethod
    def _drain_stderr(proc: subprocess.Popen) -> None:
        # Unread stderr would fill the pipe and stall the worker mid-sentence.
        try:
            for line in proc.stderr:  # type: ignore[union-attr]
                logger.info("Phonon: %s", line.decode("utf-8", "replace").rstrip())
        except Exception:
            pass

    def _write(self, msg: dict) -> bool:
        try:
            self._proc.stdin.write((json.dumps(msg) + "\n").encode())  # type: ignore[union-attr]
            self._proc.stdin.flush()  # type: ignore[union-attr]
            return True
        except (OSError, ValueError, AttributeError) as e:
            logger.warning("Phonon: cannot reach worker: %s", e)
            return False

    def _wait_end(self, req_id: int, frames: "queue.Queue") -> None:
        """Discard frames until request `req_id` ends, then free the worker."""
        try:
            while True:
                item = frames.get(timeout=_FRAME_TIMEOUT_S)
                if item is None or (item[0] == req_id and item[1] == _END):
                    return
        except queue.Empty:
            logger.warning("Phonon: request %d did not finish after cancel", req_id)
        finally:
            self._request_lock.release()

    def synth(self, text: str, voice: str,
              cancelled: Optional[Callable[[], bool]] = None) -> Iterator[Tuple[float, bytes]]:
        """Yield (arrival time, PCM) per chunk; stops early once `cancelled()` is true."""
        if not self.ensure_started():
            return
        self._request_lock.acquire()
        frames = self._frames
        self._next_id += 1
        req_id = self._next_id
        finished = False
        try:
            if not self._write({"id": req_id, "text": text, "voice": voice}):
                finished = True
                return
            last_frame = time.monotonic()
            while True:
                if cancelled is not None and cancelled():
                    return
                try:
                    item = frames.get(timeout=_CANCEL_POLL_S)
                except queue.Empty:
                    if time.monotonic() - last_frame < _FRAME_TIMEOUT_S:
                        continue
                    logger.warning("Phonon: no audio for %.0fs, restarting worker", _FRAME_TIMEOUT_S)
                    self.kill()
                    finished = True
                    return
                if item is None:
                    finished = True
                    return
                rid, kind, payload, arrived = item
                if rid != req_id:
                    continue
                last_frame = arrived
                if kind == _PCM:
                    yield arrived, payload
                elif kind == _ERROR:
                    logger.warning("Phonon: synthesis failed: %s", payload.decode("utf-8", "replace"))
                elif kind == _END:
                    finished = True
                    return
        finally:
            if finished:
                self._request_lock.release()
            else:
                # Abandoned mid-utterance (barge-in): stop the worker's generation and
                # drain off-thread so the caller returns at once.
                self._write({"cancel": req_id})
                threading.Thread(target=self._wait_end, args=(req_id, frames),
                                 daemon=True, name="phonon-cancel").start()

    def kill(self) -> None:
        proc = self._proc
        if proc is not None and proc.poll() is None:
            proc.kill()


# One worker per model setup, shared by the live service and Test Voice previews,
# so a preview never loads a second copy of the model.
_workers: dict = {}
_workers_lock = threading.Lock()


def _shared_worker(model_dir: str, lang: str, quant: str, threads: int, cpus: str) -> _Worker:
    key = (model_dir, lang, quant, threads, cpus)
    with _workers_lock:
        worker = _workers.get(key)
        if worker is None:
            worker = _workers[key] = _Worker(model_dir, lang, quant, threads, cpus)
        return worker


class PhononTTSBackend(TTSBackend):
    """Local Phonon synthesis, streamed as raw PCM int16 at 24 kHz."""

    # Stop is honoured while a sentence is still buffering its head start.
    supports_synthesis_cancellation = True

    def __init__(self, model_dir: str = "", lang: str = "", quant: str = "",
                 threads: Optional[int] = None, prebuffer_s: Optional[float] = None,
                 cpus: Optional[str] = None):
        self._model_dir = model_dir or PHONON_MODEL_DIR
        self._prebuffer_s = PHONON_PREBUFFER_S if prebuffer_s is None else prebuffer_s
        self._worker = _shared_worker(
            self._model_dir, lang or PHONON_LANG, quant or PHONON_QUANT,
            PHONON_THREADS if threads is None else threads,
            PHONON_CPUS if cpus is None else cpus,
        )
        self._warned_speed = False
        if self.available:
            logger.info("Phonon TTS backend ready (model=%s)", self._model_dir)
            threading.Thread(target=self._worker.ensure_started, daemon=True,
                             name="phonon-prewarm").start()
        else:
            logger.warning(
                "Phonon TTS unavailable (python=%s exists=%s, model=%s exists=%s)",
                PHONON_PYTHON, engine_installed(), self._model_dir, model_installed(self._model_dir),
            )

    @property
    def available(self) -> bool:
        return engine_installed() and model_installed(self._model_dir) and os.path.isfile(WORKER_SCRIPT)

    @property
    def sample_rate(self) -> int:
        return self._worker.sample_rate

    @property
    def volume_boost(self) -> float:
        """1.0 like Piper: the model already outputs full-scale audio."""
        return 1.0

    def voices(self) -> List[str]:
        return self._worker.voices or list_voices(self._model_dir)

    def stream_pcm(
        self,
        text: str,
        voice: str,
        model: str,
        speed: float,
        instructions: Optional[str] = None,
        cancelled: Optional[Callable[[], bool]] = None,
    ) -> Iterator[bytes]:
        """Yield raw PCM int16 chunks, held back until the sentence has a head start.

        Chunks are buffered until they have been arriving for `prebuffer_s` (or the
        sentence ends, so short replies play whole), then released and streamed.
        """
        text = _strip_audio_tags(text or "")
        if not text or not self.available:
            return
        if speed and abs(speed - 1.0) > 0.01 and not self._warned_speed:
            # The model has no rate control yet; speak at its natural pace.
            self._warned_speed = True
            logger.info("Phonon: speed %.2f ignored (model has no rate control)", speed)
        known = self.voices()
        if voice and known and voice not in known:
            logger.warning("Phonon: voice %r not in model (%s), using the default", voice, known)
            voice = ""

        bytes_per_s = 2.0 * self.sample_rate
        pending: List[bytes] = []
        first_at: Optional[float] = None
        play_start: Optional[float] = None
        audio_s = 0.0
        worst_lead: Optional[float] = None
        for arrived, pcm in self._worker.synth(text, voice, cancelled):
            if first_at is None:
                first_at = arrived
            if play_start is None:
                pending.append(pcm)
                if arrived - first_at < self._prebuffer_s:
                    continue
                play_start = time.monotonic()
                for held in pending:
                    audio_s += len(held) / bytes_per_s
                    yield held
                pending = []
                continue
            # Where this chunk is due on the playback clock vs when it arrived;
            # negative = the speaker ran dry waiting for it.
            lead = play_start + audio_s - arrived
            worst_lead = lead if worst_lead is None else min(worst_lead, lead)
            audio_s += len(pcm) / bytes_per_s
            yield pcm
        if play_start is None:
            # Sentence finished inside the head start: play it whole.
            for held in pending:
                audio_s += len(held) / bytes_per_s
                yield held
        if first_at is not None:
            held_ms = ((play_start or time.monotonic()) - first_at) * 1000.0
            logger.info(
                "Phonon: %.2fs audio, held %.0f ms before playback, worst lead %s",
                audio_s, held_ms,
                "n/a (played whole)" if worst_lead is None
                else f"{worst_lead * 1000:+.0f} ms{' (GAP)' if worst_lead < 0 else ''}",
            )

