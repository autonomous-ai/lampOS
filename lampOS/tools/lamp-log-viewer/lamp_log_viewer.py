#!/usr/bin/env python3
"""Lamp log viewer: a dev lamp's journal, live, in a browser on this laptop.

A DEV tool, not a product feature. Read only: it runs `journalctl -f` and, for the
Conversation tab, a read-only sqlite query over ssh, and writes nothing on the lamp.
It listens on 127.0.0.1 only. Run:

    python3 -I lamp_log_viewer.py --lamp orangepi@lamp-XXXX.local

then open http://127.0.0.1:8790/. See README.md.
"""
import argparse
import json
import re
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

PAGE = Path(__file__).with_name("index.html")
DEFAULT_PORT = 8790
DEFAULT_CACHE = Path.home() / ".cache" / "lamp-log-viewer"
LAMP_ARGUMENT = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.-]*@[A-Za-z0-9][A-Za-z0-9.-]*$")

# The conversation: lamp-memory's own store, opened read only (mode=ro), only exchanges still
# active (a forgotten one is never shown). Sent to the lamp on stdin, so no quoting crosses ssh.
CONVO_READ = r"""
import json, sqlite3, sys
since = int(sys.argv[1]); limit = int(sys.argv[2])
c = sqlite3.connect("file:/var/lib/lampos-memory/memory-days.sqlite3?mode=ro", uri=True)
rows = c.execute(
    "select at_unix_ms, person, heard, said from entries"
    " where kind = 'exchange' and active = 1 and at_unix_ms > ?"
    " order by at_unix_ms desc limit ?", (since, limit)).fetchall()
print(json.dumps([{"t": t, "who": p, "heard": h, "said": s} for t, p, h, s in reversed(rows)]))
"""

# Per request timings from lamp-voice's own journal lines.
R_READY = re.compile(r"request (\d+): speech to text kept ready in (\d+) ms")
R_HEARD = re.compile(r"request (\d+) heard (\d+) words, the last (\d+) ms after the line ended")
R_ROUND = re.compile(r"request (\d+) chat round 1 first event in (\d+) ms")
R_CHAT = re.compile(r"chat answer: its first words or call (\d+) ms and its end (\d+) ms after it was asked; (\d+) prompt tokens, (\d+) from the cache, (\d+) not; (\d+) completion tokens")
R_PIECE = re.compile(r"request (\d+) first piece to the voice (\d+) ms after the words were heard")
R_TTS = re.compile(r"request (\d+) ElevenLabs piece 1: (\d+) chars, sent (\d+) ms after the words were heard on an? (\w+) connection; its first audio byte (\d+) ms after it went")
R_AUDIO = re.compile(r"request (\d+) first answer audio (\d+) ms after the words were heard")

PAIR_SLACK_MS = 150  # how far (answer_at - round_at) may be from (end - first) for a pair
PAIR_WITHIN_MS = 120_000  # an answer line is looked for this long after a round's line


def parse_metrics(raw_lines):
    """Timings per request from `journalctl -o json` lines of lamp-voice; only requests that
    reached their first piece to the voice are returned. Pure: no I/O."""
    reqs: dict[int, dict] = {}
    answers = []  # (at_ms, match) of every chat answer line, paired after the scan
    for raw in raw_lines:
        try:
            e = json.loads(raw)
        except ValueError:
            continue
        m = e.get("MESSAGE", "")
        if not isinstance(m, str) or "lamp-voice" not in m:
            continue
        at = int(e.get("__REALTIME_TIMESTAMP", "0")) // 1000
        if g := R_READY.search(m):
            reqs.setdefault(int(g[1]), {})["stt_ready_ms"] = int(g[2])
        elif g := R_HEARD.search(m):
            r = reqs.setdefault(int(g[1]), {})
            r["words"] = int(g[2])
            r["stt_last_ms"] = int(g[3])
        elif g := R_ROUND.search(m):
            r = reqs.setdefault(int(g[1]), {})
            r["llm_first_event_ms"] = int(g[2])
            r["round_at"] = at
        elif g := R_CHAT.search(m):
            answers.append((at, g))
        elif g := R_PIECE.search(m):
            r = reqs.setdefault(int(g[1]), {})
            r["first_piece_ms"] = int(g[2])
            r["piece_at"] = at
        elif g := R_TTS.search(m):
            r = reqs.setdefault(int(g[1]), {})
            r["tts_chars"] = int(g[2])
            r["tts_conn"] = g[4]
            r["tts_first_byte_ms"] = int(g[5])
        elif g := R_AUDIO.search(m):
            reqs.setdefault(int(g[1]), {})["first_audio_ms"] = int(g[2])
    pair_answers(reqs, answers)
    return [dict(request=n, **{k: v for k, v in r.items() if k != "round_at"}) for n, r in reqs.items() if "piece_at" in r]


def pair_answers(reqs, answers):
    """A chat answer line is written at the answer's end and the round's line at its first
    event, so (answer_at - round_at) equals (end - first) for the request's own answer. A side
    call (a summary, a title) ends at some other gap and pairs with no request."""
    for at, g in answers:
        first, end, completion = int(g[1]), int(g[2]), int(g[6])
        best, gap = None, PAIR_SLACK_MS
        for r in reqs.values():
            if "round_at" in r and "llm_ttft_ms" not in r:
                d = abs((at - r["round_at"]) - (end - first))
                if 0 <= at - r["round_at"] < PAIR_WITHIN_MS and d < gap:
                    best, gap = r, d
        if best is not None:
            best.update(llm_ttft_ms=first, llm_end_ms=end, prompt_tokens=int(g[3]), cached_tokens=int(g[4]),
                        completion_tokens=completion,
                        gen_tps=round(completion / ((end - first) / 1000), 1) if end > first else None)


def match_turn(turn_ms, rows, within_ms=300):
    """The request whose first piece to the voice was nearest to a conversation turn's time
    (the memory's turn time is that moment, within a few ms), or None past `within_ms`. This is
    the rule index.html applies in the browser (300 ms); it is kept here so a test holds it."""
    best = min(rows, key=lambda r: abs(r["piece_at"] - turn_ms), default=None)
    if best is not None and abs(best["piece_at"] - turn_ms) <= within_ms:
        return best
    return None


class Lamp:
    """The one lamp this process reads, and the ways to read it (all over ssh, read only)."""

    def __init__(self, target: str, cache_dir: Path):
        if not LAMP_ARGUMENT.match(target):
            raise ValueError("--lamp must look like user@host, e.g. orangepi@lamp-XXXX.local")
        self.target = target
        self.name = target.split("@", 1)[1].split(".", 1)[0]
        self.cache_dir = cache_dir
        self.cache = cache_dir / (re.sub(r"[^A-Za-z0-9_.-]", "_", target) + ".metrics.json")  # timings only, never words
        self.lock = threading.Lock()
        base = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "ServerAliveInterval=20",
                "-o", "ControlMaster=auto", "-o", "ControlPath=/tmp/lamp-log-viewer-%C", "-o", "ControlPersist=120"]
        self.ssh = base + ["--", target]

    def conversation(self, since: int, limit: int) -> bytes:
        cmd = self.ssh + [f"sudo -n python3 -I - {int(since)} {max(1, min(int(limit), 500))}"]
        out = subprocess.run(cmd, input=CONVO_READ, capture_output=True, text=True, timeout=20)
        if out.returncode != 0:
            return json.dumps({"error": (out.stderr.strip() or "read failed")[:300]}).encode()
        return out.stdout.strip().encode() or b"[]"

    def metrics_live(self, since_ms: int, until_ms: int) -> list:
        since = max(0, since_ms // 1000 - 90)
        until = until_ms // 1000 + 90
        cmd = self.ssh + [f"journalctl -u lamp-voice -o json --output-fields=MESSAGE,__REALTIME_TIMESTAMP --no-pager --since @{since} --until @{until}"]
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        return parse_metrics(out.stdout.splitlines())

    def cache_load(self) -> dict:
        try:
            return json.loads(self.cache.read_text())
        except (OSError, ValueError):
            return {}

    def cache_merge(self, rows: list) -> None:
        with self.lock:
            self.cache_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
            cache = self.cache_load()
            for r in rows:
                cache[str(r["piece_at"])] = r
            tmp = self.cache.with_suffix(".tmp")
            tmp.write_text(json.dumps(cache))
            tmp.replace(self.cache)

    def metrics(self, since_ms: int, until_ms: int) -> bytes:
        self.cache_merge(self.metrics_live(since_ms, until_ms))
        with self.lock:
            cache = self.cache_load()
        lo, hi = since_ms - 120000, until_ms + 120000
        return json.dumps([r for r in cache.values() if lo <= r["piece_at"] <= hi]).encode()

    def collect_forever(self) -> None:
        # The lamp's journal keeps under an hour (#7254), so the timings are copied here every minute.
        while True:
            now = int(time.time() * 1000)
            try:
                self.cache_merge(self.metrics_live(now - 15 * 60 * 1000, now))
            except Exception as error:  # a lamp away for a minute is not fatal
                print(f"collector: {error}", flush=True)
            time.sleep(60)

    def journal_cmd(self, back: int) -> list:
        back = max(0, min(back, 2000))
        return self.ssh + [f"journalctl -f -n {back} -o json --output-fields=MESSAGE,_SYSTEMD_UNIT,SYSLOG_IDENTIFIER,PRIORITY,__REALTIME_TIMESTAMP --no-pager"]


def stream_line(raw: str):
    """One `journalctl -o json` line as the page's {t, unit, p, m}, or None if it isn't JSON."""
    try:
        e = json.loads(raw)
    except ValueError:
        return None
    msg = e.get("MESSAGE", "")
    if isinstance(msg, list):  # journald sends non-UTF-8 messages as byte arrays
        msg = bytes(msg).decode("utf-8", "replace")
    return {
        "t": int(e.get("__REALTIME_TIMESTAMP", "0")) // 1000,
        "unit": (e.get("_SYSTEMD_UNIT") or e.get("SYSLOG_IDENTIFIER") or "?").removesuffix(".service"),
        "p": int(e.get("PRIORITY", "6")),
        "m": msg,
    }


def make_handler(lamp: Lamp, port: int):
    allowed_hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):  # keep the terminal quiet
            pass

        def send_json(self, body: bytes):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            # The page shows what people said to the lamp: a page on another site must not read it
            # through the laptop (DNS rebinding), so only this server's own host names are served.
            if self.headers.get("Host", "") not in allowed_hosts:
                self.send_error(403)
                return
            url = urlparse(self.path)
            qs = parse_qs(url.query)

            def number(name, default):
                return int(qs.get(name, [str(default)])[0] or default)

            try:
                if url.path == "/":
                    body = PAGE.read_bytes()
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html; charset=utf-8")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                elif url.path == "/info":
                    self.send_json(json.dumps({"lamp": lamp.name}).encode())
                elif url.path == "/metrics":
                    self.send_json(lamp.metrics(number("since", 0), number("until", 0)))
                elif url.path == "/conversation":
                    self.send_json(lamp.conversation(number("since", 0), number("limit", 100)))
                elif url.path == "/stream":
                    self.stream(number("back", 200))
                else:
                    self.send_error(404)
            except (ValueError, subprocess.TimeoutExpired) as error:
                self.send_json(json.dumps({"error": str(error)[:300]}).encode())
            except (BrokenPipeError, ConnectionResetError):
                pass

        def stream(self, back: int):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            proc = subprocess.Popen(lamp.journal_cmd(back), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
            try:
                for raw in proc.stdout:
                    line = stream_line(raw)
                    if line is None:
                        continue
                    self.wfile.write(b"data: " + json.dumps(line).encode() + b"\n\n")
                    self.wfile.flush()
                err = proc.stderr.read().strip()
                if err:
                    self.wfile.write(b"event: oops\ndata: " + json.dumps(err[:300]).encode() + b"\n\n")
            finally:
                proc.kill()

    return Handler


def main(argv=None):
    ap = argparse.ArgumentParser(description="A dev lamp's journal and conversation, read only, in a browser on 127.0.0.1.")
    ap.add_argument("--lamp", required=True, help="the lamp to read, as user@host (for example orangepi@lamp-XXXX.local)")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    ap.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE, help=f"where the timings cache is kept (default {DEFAULT_CACHE})")
    args = ap.parse_args(argv)
    try:
        lamp = Lamp(args.lamp, args.cache_dir)
    except ValueError as error:
        ap.error(str(error))
    server = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(lamp, args.port))
    threading.Thread(target=lamp.collect_forever, daemon=True).start()
    print(f"lamp log viewer: http://127.0.0.1:{args.port}/  (reading {lamp.target}, read only)", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
