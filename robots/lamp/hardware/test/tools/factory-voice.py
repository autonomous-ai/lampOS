#!/usr/bin/env python3
# factory-voice.py — the factory steps (lampOS/deploy/factory/steps.sh) with
# each step told to the person at the lamp on the lamp's own speaker, so the
# prompt reaches them as it is needed, not on a console across the bench
# (#3576; on #3530 the self-test's 20 s touch window closed four times before
# its console prompt reached the lamp).
#
# Run on the lamp, as root, at a console, in place of steps.sh itself:
#
#   factory-voice.py -- ./steps.sh --unit-id 3e83 --write --to 3
#   factory-voice.py -- ./steps.sh --unit-id 29e7 --write --from 4
#   factory-voice.py --strip -- ./steps.sh ...   the base strip is read (LAMP_CONTROLS_STRIP=on)
#   factory-voice.py --no-voice -- ./steps.sh ... no spoken prompts
#   factory-voice.py --dry-run -- ./steps.sh ...  print the prompts, play nothing
#
# steps.sh runs unchanged under a pseudo-terminal, so its "Ready? [y/N]" still
# has the console it needs, and what it prints and what the person types pass
# through as they are. Its exit status is this one's. The prompts follow what
# steps.sh prints (a stand, a refusal, a step done) and the person's yes to
# each question: the head pads' timing (11 s hands away, then pad one, pad
# two) starts at that yes, as discover-head does.
#
# Prompts: the factory_* WAVs made once on a Mac by make-prompts.sh, in
# --prompts (default /var/lib/lamp-test/prompts). A missing prompt is silence.
# They are played one at a time with `aplay -D device_speaker` at the lamp's
# own level, never over each other; this never touches the volume. Steps 1 to
# 3 run on a lamp not set up, where lamp-voice is not running and the speaker
# is free. Step 4 runs on a set-up lamp whose lamp-voice holds the speaker the
# whole time it runs (a continuous PCM clock, idle zeros between replies:
# crates/voice/src/playback.rs) and must keep running for the self-test. So,
# as routine-run.py does, before steps.sh starts a run that includes step 4,
# lamp-voice is stopped, the whole of the self-test is told up front (stand
# up, then touch the head and press the button for 20 s), and lamp-voice is
# started again and given --voice-settle seconds before steps.sh runs. During
# step 4 a prompt the speaker refuses is skipped and said once on the console.
#
# Python 3 standard library only.

import argparse
import fcntl
import os
import pty
import queue
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import threading
import time
import tty

# Environment overrides, for a host test with stand-ins (factory-voice-test.py).
APLAY = os.environ.get("APLAY", "aplay")
SYSTEMCTL = os.environ.get("SYSTEMCTL", "systemctl")
TIME_SCALE = float(os.environ.get("FACTORY_VOICE_TIME_SCALE", "1"))

# What steps.sh prints → the prompts to play, in order. A question also names
# what the person's yes to it starts (YES below).
LINES = [
    (r"the arm's stand: it rises", ["factory_stand_ready", "factory_say_ready"], "stand"),
    (r"the lamp is held, torque on: it is let go", ["factory_held", "factory_say_ready"], "held"),
    (r"standing at 30%", ["factory_stand_30"], None),
    (r"standing at 100%", ["factory_stand_100"], None),
    (r"this arm stands at shoulder", ["factory_stand_done"], None),
    (r"a head off its stop", ["factory_head_off"], None),
    (r"the head pads: for 40 s", ["factory_pads_ready", "factory_say_ready"], "pads"),
    (r"step 2 done", ["factory_pads_done"], None),
    (r"step 3 done", ["factory_key_done"], None),
    (r"the self-test: the lamp stands up", ["factory_selftest_ready", "factory_say_ready"], "selftest"),
    (r"standing the lamp up for the head's small move", ["factory_selftest_stand"], None),
    (r"touch the head and press the button now", ["TOUCH"], None),
    (r"the reset, now: the lamp forgets", ["factory_selftest_pass", "factory_reset_ready"], "reset"),
    (r"resetting: the lamp reboots", ["factory_resetting"], None),
    (r"FAILED: the self-test found a part that failed", ["factory_selftest_fail"], None),
    (r"factory: FAILED: (?!the self-test found)", ["factory_failed"], None),
    (r"steps \d to \d done \(write\)|all four steps done \(write\)", ["factory_all_done"], None),
]

# The person's yes to a question → (seconds after the yes, prompt).
YES = {
    "stand": [(0.0, "factory_stand_go")],
    # discover-head: 10 s of rest after a settle, so 11 s hands away (as the
    # question says), then each pad held about 2 s.
    "pads": [(0.0, "factory_pads_away"), (11.5, "factory_pads_1"), (15.5, "factory_pads_2"),
             (19.5, "factory_pads_more"), (30.0, "factory_pads_wait")],
    "selftest": [(0.0, "factory_selftest_wait")],
}


class Voice:
    """Plays prompts one at a time, in the order asked, on its own thread."""

    def __init__(self, args):
        self.args = args
        self.q = queue.Queue()
        self.timers = []
        self.refused = False
        threading.Thread(target=self._run, daemon=True).start()

    def say(self, key):
        if key == "TOUCH":
            key = "factory_touch_now_strip" if self.args.strip else "factory_touch_now"
        self.q.put(key)

    def later(self, plan):
        self.cancel()
        for delay, key in plan:
            t = threading.Timer(delay * TIME_SCALE, self.say, args=(key,))
            t.daemon = True
            t.start()
            self.timers.append(t)

    def cancel(self):
        for t in self.timers:
            t.cancel()
        self.timers = []

    def note(self, text):
        os.write(2, f"\r\n[factory-voice] {text}\r\n".encode())

    def _run(self):
        while True:
            key = self.q.get()
            path = os.path.join(self.args.prompts, key + ".wav")
            if self.args.no_voice or not os.path.exists(path):
                pass
            elif self.args.dry_run:
                self.note(f"would say {key}")
            else:
                r = subprocess.run([APLAY, "-q", "-D", self.args.speaker, path],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=20)
                if r.returncode != 0 and not self.refused:
                    self.refused = True
                    why = r.stderr.decode(errors="replace").strip().splitlines()[-1:] or ["?"]
                    self.note(f"the speaker refused a prompt ({why[0]}); prompts are skipped while it does")
            self.q.task_done()

    def drain(self, seconds):
        end = time.time() + seconds
        while self.q.unfinished_tasks and time.time() < end:
            time.sleep(0.1)


class Watch:
    """Matches what steps.sh prints, across reads, each match once."""

    def __init__(self, voice):
        self.voice = voice
        self.text = ""
        self.lines = [(re.compile(p), keys, q) for p, keys, q in LINES]
        self.done = [0] * len(LINES)
        self.question = None

    def printed(self, chunk):
        self.text += chunk
        for i, (pattern, keys, question) in enumerate(self.lines):
            for m in pattern.finditer(self.text, self.done[i]):
                self.done[i] = m.end()
                if "FAILED" in pattern.pattern:
                    self.voice.cancel()
                for key in keys:
                    self.voice.say(key)
                if question:
                    self.question = question

    def answered(self, line):
        if self.question is None:
            return
        if line in ("y", "Y") and self.question in YES:
            self.voice.later(YES[self.question])
        self.question = None


def runs_step_4(command):
    """Whether steps.sh's --from/--to (1 and 4 when not given) include step 4."""
    def option(name, default):
        if name in command and command.index(name) + 1 < len(command):
            try:
                return int(command[command.index(name) + 1])
            except ValueError:
                return default
        return default
    return option("--from", 1) <= 4 <= option("--to", 4)


def voice_active():
    return subprocess.run([SYSTEMCTL, "is-active", "--quiet", "lamp-voice"]).returncode == 0


def brief_step_4(args, voice):
    """lamp-voice holds the speaker for the whole of step 4, so the self-test
    is told before it: lamp-voice stopped, the brief said, lamp-voice started
    again and settled, as routine-run.py does for its prompts."""
    key = "factory_selftest_brief_strip" if args.strip else "factory_selftest_brief"
    if args.dry_run:
        voice.note(f"would stop lamp-voice, say {key}, start lamp-voice and wait {args.voice_settle:g} s")
        return
    subprocess.run([SYSTEMCTL, "stop", "lamp-voice"], check=False)
    voice.say(key)
    voice.drain(30)
    subprocess.run([SYSTEMCTL, "start", "lamp-voice"], check=False)
    time.sleep(args.voice_settle * TIME_SCALE)


def main():
    ap = argparse.ArgumentParser(description="steps.sh with each step said on the lamp (see the header).")
    ap.add_argument("--prompts", default="/var/lib/lamp-test/prompts", help="folder of factory_*.wav prompts")
    ap.add_argument("--speaker", default="device_speaker", help="ALSA device for the prompts")
    ap.add_argument("--strip", action="store_true", help="the base strip is read: ask for a swipe too")
    ap.add_argument("--no-voice", action="store_true", help="no spoken prompts")
    ap.add_argument("--voice-settle", type=float, default=6.0,
                    help="seconds after starting lamp-voice again, before step 4")
    ap.add_argument("--dry-run", action="store_true", help="print each prompt, play nothing")
    ap.add_argument("command", nargs=argparse.REMAINDER, help="-- steps.sh and its options")
    args = ap.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        ap.error("give the command after --, e.g. -- ./steps.sh --unit-id ID --write --to 3")

    voice = Voice(args)
    watch = Watch(voice)
    if not args.no_voice and runs_step_4(command) and voice_active():
        brief_step_4(args, voice)
    pid, fd = pty.fork()
    if pid == 0:
        try:
            os.execvp(command[0], command)
        except OSError as e:
            os.write(2, f"factory-voice: cannot run {command[0]}: {e}\n".encode())
            os._exit(127)

    console = os.isatty(0)
    saved = termios.tcgetattr(0) if console else None
    if console:
        size = fcntl.ioctl(0, termios.TIOCGWINSZ, struct.pack("HHHH", 0, 0, 0, 0))
        fcntl.ioctl(fd, termios.TIOCSWINSZ, size)
        tty.setraw(0)
    # Ctrl-C reaches steps.sh through its terminal (raw here), so its own
    # trap eases a standing arm back; a signal to this process is passed on.
    for sig in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda s, _f: os.kill(pid, s))

    typed = b""
    inputs = [fd, 0]
    try:
        while True:
            try:
                ready, _, _ = select.select(inputs, [], [])
            except InterruptedError:
                continue
            if fd in ready:
                try:
                    data = os.read(fd, 4096)
                except OSError:
                    data = b""
                if not data:
                    break
                os.write(1, data)
                watch.printed(data.decode(errors="replace"))
            if 0 in ready:
                data = os.read(0, 1024)
                if not data:
                    inputs.remove(0)
                    continue
                os.write(fd, data)
                for b in data:
                    if b in (10, 13):
                        watch.answered(typed.decode(errors="replace").strip())
                        typed = b""
                    elif b in (8, 127):
                        typed = typed[:-1]
                    else:
                        typed += bytes([b])
    finally:
        if console:
            termios.tcsetattr(0, termios.TCSADRAIN, saved)
    _, status = os.waitpid(pid, 0)
    voice.drain(15)
    return os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8)


if __name__ == "__main__":
    sys.exit(main())
