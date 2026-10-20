#!/usr/bin/env python3
# ring-pixels.py — TC-RT-05 (`ring_pixels`): the whole ring white, red, green
# and blue, about 3 s each, for a person to look for dark or stuck pixels.
#
# Run on the lamp, as a user that may use the bus (root or orangepi):
#
#   ./ring-pixels.py                       the routine step (level 5, 3 s each)
#   ./ring-pixels.py --level 3 --hold 4    dimmer, longer
#   ./ring-pixels.py --dry-run             print what it would publish, change nothing
#
# On V2 the ring belongs to lamp-voice (the ring's owner), so nothing here
# writes SPI. Each colour is asked for on `light/test` through `lamp-bus pub`,
# the way lamp-selftest asks for its ten colours (lampOS/crates/selftest/src/
# ring.rs). The owner shows a test colour for 2 s at most (TEST_HOLDS_US,
# lampOS/crates/voice/src/ring_light.rs), so the colour is asked again every
# second to hold it for --hold seconds. What the ring actually drew is read on
# `light/drawn`, which the owner says only from its own write receipts. The
# person's light (`light/command`) is never written: the test lapses by
# itself within 2 s of the last ask and the ring shows the person's light
# again, which is checked on `light/drawn` against `light/state`.
#
# A light the person asks for during the run (`light/command`) ends it:
# `not_checked`. None of the colours drawn (the privacy switch is closed, or
# a turn owns the ring) is `not_checked`; some drawn and some not is `fail`.
#
# The person is then asked pass or fail, and on a fail where: pixel 0 is at
# the top and they count clockwise as a person facing the lamp sees them,
# 8 at three o'clock, 16 at six, 24 at nine (measured on lamp-4ace,
# lampOS/docs/ring-choreography.md). An unanswered question is `not_checked`,
# never a pass.
#
# Output: one JSON line on stdout for lamp-station's answers file,
#   {"check":"ring_pixels","verdict":"pass|fail|not_checked|interrupted",
#    "reason":"...","at":"<UTC, Z>"}
# Prompts and progress go to stderr; --evidence FILE adds one JSON object
# with what was asked, drawn and answered, with times.
#
# Exit: 0 = pass, 1 = fail, 2 = could not run (no lamp-bus, bad option),
#       3 = not_checked or interrupted (never a pass).
#
# Settings: LAMP_BUS_BIN (lamp-bus on PATH, else /usr/local/bin/lamp-bus);
# --root or LAMP_BUS_ROOT for the bus root (/run/lampos-bus).
# Python 3.11 standard library only.
import argparse
import datetime
import json
import os
import select
import shutil
import subprocess
import sys
import time

CHECK = "ring_pixels"
COLOURS = ["white", "red", "green", "blue"]
LIGHT_TYPE = "lamp_msgs/Light"
TEST_TOPIC = "light/test"
DRAWN_TOPIC = "light/drawn"
COMMAND_TOPIC = "light/command"
STATE_TOPIC = "light/state"
# The owner looks every 50 ms and draws every 25 ms: a colour not drawn
# within 1 s is not coming (selftest's DRAWN_WITHIN).
DRAWN_WITHIN_S = 1.0
# A test is shown 2 s at most; asked again well inside that.
ASK_EVERY_S = 1.0
# light/state is said once a second; older than this, the owner stopped.
STATE_FRESH_S = 3.0
# The test lapses 2 s after the last ask; the person's light is looked for
# on light/drawn until then and a little after.
GIVEN_BACK_WITHIN_S = 3.5
PIXEL_PLACES = ("pixel 0 is at the top, counting clockwise as you face the lamp: "
                "8 at three o'clock, 16 at six, 24 at nine")


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def now_ms():
    return int(time.time() * 1000)


def say(text):
    print(text, file=sys.stderr, flush=True)


class Interrupted(Exception):
    pass


class Bus:
    """lamp-bus from a shell, as lamp-selftest's library calls do it."""

    def __init__(self, binary, root, dry_run):
        self.binary, self.root, self.dry_run = binary, root, dry_run

    def _run(self, *args, timeout=5):
        cmd = [self.binary] + (["--root", self.root] if self.root else []) + list(args)
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

    def latest(self, topic):
        """(header, payload) of the topic's kept value, or None."""
        try:
            done = self._run("latest", topic)
        except (OSError, subprocess.TimeoutExpired):
            return None
        if done.returncode != 0:
            return None
        return parse_line(done.stdout.strip())

    def publish(self, topic, value):
        line = json.dumps(value, separators=(",", ":"))
        if self.dry_run:
            say(f"[dry-run] lamp-bus pub {topic} latest '{line}' --type {LIGHT_TYPE}")
            return True
        try:
            done = self._run("pub", topic, "latest", line, "--type", LIGHT_TYPE)
        except (OSError, subprocess.TimeoutExpired) as error:
            say(f"lamp-bus pub {topic}: {error}")
            return False
        if done.returncode != 0:
            say(f"lamp-bus pub {topic}: {done.stderr.strip()}")
        return done.returncode == 0


def parse_line(text):
    """A `lamp-bus latest|tap` line: the header's JSON, a space, the payload."""
    if not text:
        return None
    try:
        header, end = json.JSONDecoder().raw_decode(text)
        rest = text[end:].strip()
        payload = json.loads(rest) if rest and not rest.startswith("<") else None
    except ValueError:
        return None
    return header, payload


def asked_by_person(bus, since_ms):
    seen = bus.latest(COMMAND_TOPIC)
    return bool(seen) and seen[0].get("at_unix_ms", 0) >= since_ms


def drawn_since(bus, light, since_ms):
    seen = bus.latest(DRAWN_TOPIC)
    return bool(seen) and seen[0].get("at_unix_ms", 0) >= since_ms and seen[1] == light


_PENDING = bytearray()
_EOF = object()


def read_line(timeout_s):
    """One line from the person on stdin, read from the descriptor itself so
    that a second line typed ahead is not hidden in a buffer. None when
    nothing comes in time, _EOF when nobody is there to type."""
    deadline = time.monotonic() + timeout_s
    while b"\n" not in _PENDING:
        left = deadline - time.monotonic()
        if left <= 0:
            return None
        try:
            ready, _, _ = select.select([0], [], [], left)
        except (OSError, ValueError):
            return _EOF
        if not ready:
            return None
        chunk = os.read(0, 4096)
        if not chunk:
            if _PENDING:
                break
            return _EOF
        _PENDING.extend(chunk)
    end = _PENDING.find(b"\n")
    end = len(_PENDING) if end < 0 else end
    line = bytes(_PENDING[:end]).decode(errors="replace").strip()
    del _PENDING[:end + 1]
    return line


def ask(prompt, timeout_s):
    """One line from the person, or None when nobody answers in time."""
    say(prompt)
    line = read_line(timeout_s)
    return None if line is None or line is _EOF else line


def show_colours(bus, level, hold_s, began_ms, evidence):
    """Ask for each colour and hold it; return (drawn, missed)."""
    drawn, missed = [], []
    for colour in COLOURS:
        light = {"on": True, "colour": colour, "level": level}
        if asked_by_person(bus, began_ms):
            raise Interrupted
        asked_ms = now_ms()
        if not bus.publish(TEST_TOPIC, light):
            raise RuntimeError(f"could not publish {TEST_TOPIC}")
        say(f"  {colour} (level {level}) for {hold_s:g} s")
        start = time.monotonic()
        last_ask = start
        seen_at = None
        while time.monotonic() - start < hold_s:
            if asked_by_person(bus, began_ms):
                raise Interrupted
            if seen_at is None and (bus.dry_run or drawn_since(bus, light, asked_ms)):
                seen_at = now_ms()
            if seen_at is None and time.monotonic() - start > DRAWN_WITHIN_S:
                break
            if time.monotonic() - last_ask >= ASK_EVERY_S:
                bus.publish(TEST_TOPIC, light)
                last_ask = time.monotonic()
            time.sleep(0.05)
        (drawn if seen_at is not None else missed).append(colour)
        evidence["colours"].append({"colour": colour, "asked_ms": asked_ms,
                                    "drawn_ms": seen_at})
    return drawn, missed


def given_back(bus, person_light):
    """Wait for the ring to draw the person's light again."""
    if person_light is None:
        return None
    since = now_ms()
    deadline = time.monotonic() + GIVEN_BACK_WITHIN_S
    while time.monotonic() < deadline:
        if drawn_since(bus, person_light, since - 1000):
            return True
        time.sleep(0.1)
    return False


def main():
    ap = argparse.ArgumentParser(description="TC-RT-05 ring pixels (see the header).")
    ap.add_argument("--level", type=int, default=5, help="1 to 5 (5, the brightest)")
    ap.add_argument("--hold", type=float, default=3.0, help="seconds each colour (3)")
    ap.add_argument("--answer-timeout", type=float, default=120.0,
                    help="seconds to wait for the person (120); none is not_checked")
    ap.add_argument("--root", help="the bus root (else $LAMP_BUS_ROOT, else /run/lampos-bus)")
    ap.add_argument("--evidence", help="append one JSON object with times and answers")
    ap.add_argument("--dry-run", action="store_true", help="print the steps, change nothing")
    args = ap.parse_args()
    if not 1 <= args.level <= 5 or not 1.0 <= args.hold <= 30.0:
        ap.error("--level is 1 to 5 and --hold 1 to 30 s")

    evidence = {"check": CHECK, "started": utc_now(), "level": args.level,
                "hold_s": args.hold, "colours": [], "answer": None}

    def finish(verdict, reason, code):
        evidence.update(verdict=verdict, reason=reason, ended=utc_now())
        if args.evidence:
            with open(args.evidence, "a") as f:
                f.write(json.dumps(evidence) + "\n")
        print(json.dumps({"check": CHECK, "verdict": verdict, "reason": reason,
                          "at": utc_now()}), flush=True)
        return code

    binary = os.environ.get("LAMP_BUS_BIN") or shutil.which("lamp-bus") or "/usr/local/bin/lamp-bus"
    if not args.dry_run and not os.access(binary, os.X_OK):
        return finish("not_checked", f"lamp-bus not found ({binary}): not run on a lampOS lamp?", 2)
    bus = Bus(binary, args.root, args.dry_run)

    person_light = None
    if not args.dry_run:
        state = bus.latest(STATE_TOPIC)
        if state is None or state[1] is None:
            return finish("not_checked", "nothing on light/state: is lamp-voice (the ring's owner) running?", 3)
        age = (now_ms() - state[0].get("at_unix_ms", 0)) / 1000
        if age > STATE_FRESH_S:
            return finish("not_checked", f"light/state is {age:.1f} s old: the ring's owner stopped saying what it shows", 3)
        person_light = state[1]
        evidence["person_light"] = person_light

    say(f"Ring pixels: look at every one of the 32 pixels ({PIXEL_PLACES}).")
    repeats = 0
    try:
        while True:
            began_ms = now_ms()
            drawn, missed = show_colours(bus, args.level, args.hold, began_ms, evidence)
            back = None if args.dry_run else given_back(bus, person_light)
            evidence["given_back"] = back
            back_note = "" if back in (None, True) else \
                "; the ring did not show the person's light again within 3.5 s"
            if args.dry_run:
                return finish("not_checked", "dry run: nothing was shown", 3)
            if not drawn:
                return finish("not_checked", "the ring drew none of the colours: it does not draw "
                              "under the privacy switch or while a turn owns the lamp" + back_note, 3)
            if missed:
                return finish("fail", "not drawn within 1 s by the ring's owner: "
                              + ", ".join(missed) + back_note, 1)
            answer = ask("Every pixel lit, in every colour, none dark, stuck or wrong? "
                         "[p]ass / [f]ail / [r]epeat: ", args.answer_timeout)
            evidence["answer"] = answer
            if answer is None:
                return finish("not_checked", "the person did not answer" + back_note, 3)
            word = answer.lower()[:1]
            if word == "r" and repeats < 3:
                repeats += 1
                evidence["colours"] = []
                continue
            if word == "p":
                return finish("pass", "the person saw all 32 pixels in white, red, green and blue"
                              + back_note, 0)
            if word == "f":
                place = ask(f"Which pixel(s), in which colour(s)? ({PIXEL_PLACES}): ",
                            args.answer_timeout)
                evidence["place"] = place
                if not place:
                    return finish("fail", "the person saw a bad pixel; its place was not given" + back_note, 1)
                return finish("fail", f"the person saw: {place}" + back_note, 1)
            return finish("not_checked", f"no pass or fail in the answer {answer!r}" + back_note, 3)
    except Interrupted:
        return finish("not_checked", "a light was asked for on light/command during the check, "
                      "which ends it; it was left as asked", 3)
    except KeyboardInterrupt:
        return finish("interrupted", "stopped by the operator", 3)
    except RuntimeError as error:
        return finish("not_checked", str(error), 3)


if __name__ == "__main__":
    sys.exit(main())
