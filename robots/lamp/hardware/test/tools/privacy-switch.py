#!/usr/bin/env python3
# privacy-switch.py — TC-RT-06 (`privacy_switch`): the switch closes both
# microphones and the camera, and opening it brings all three back in 5 s.
#
# Run on the lamp, as a user that may use the bus (root or orangepi):
#
#   ./privacy-switch.py                    the routine step
#   ./privacy-switch.py --within 5         seconds allowed to close / reopen (5)
#   ./privacy-switch.py --dry-run          print what it would read, ask nothing
#
# Nothing here opens a microphone, the camera or the GPIO line: each is read
# on the bus, from the part that owns it, with `lamp-bus tap` (only listens):
#
#   switch   controls/state .privacy.muted   lamp-controls, chip 1 line 9
#            (lampOS/crates/msgs/src/controls.rs, ControlsState/PrivacySwitch;
#            lampOS/docs/privacy-switch.md). Needs LAMP_PRIVACY_CONTROLS=on,
#            the product's route; without it controls/state has no `privacy`
#            and the step is not_checked.
#   mic2     the voice microphone: mic2/audio blocks arriving (only with
#            LAMP_BUS=on and LAMP_BUS_AUDIO=on), or the voice's capture lease
#            voice/privacy .permitted, fresh (lampOS/crates/msgs/src/
#            privacy.rs, MicrophonePrivacy). Open = audio arrives or the
#            lease permits; silent = neither.
#   mic1     the room microphone: ears/level arriving, ten a second while
#            lamp-ears captures (lampOS/crates/msgs/src/ears.rs, RoomSound).
#            Under the privacy route Ears captures only under the voice's
#            lease, so a closed switch stops it.
#   camera   diagnostics from `eyes`, fact camera=open|closed|not opening|
#            failed, once a second (lampOS/docs/eyes.md, "When the camera is
#            open"); the eyes close the camera on the voice's no, or 1.5 s
#            after its last yes.
#
# Steps: read all four with the switch open (all open is expected; a part
# whose owner says nothing is not_checked). Ask the person to slide the
# switch closed; the taps run all the while, so the moment controls/state
# says muted is the start. Within --within s both microphones must be silent
# and the camera closed, and stay so. Ask to open it; from the moment
# controls/state says open, all three must be back within --within s.
#
# Times are from the bus's own stamps (at_unix_ms). A microphone counts as
# silent once nothing has come from it for 0.6 s (QUIET_MS), so a time to
# silent is late by up to that much; a time to back is not.
#
# The person's answer is the switch moving, or Enter. Nobody moving it in
# --answer-timeout s is not_checked, never a pass; Enter with the switch
# still read as before is fail (the switch's line did not follow).
#
# Output: one JSON line on stdout for lamp-station's answers file,
#   {"check":"privacy_switch","verdict":"pass|fail|not_checked|interrupted",
#    "reason":"...","at":"<UTC, Z>"}
# Prompts and progress go to stderr; --evidence FILE adds one JSON object
# with the three states before, closed and after, and the times.
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
import threading
import time

CHECK = "privacy_switch"
TOPICS = ["controls/state", "voice/privacy", "mic2/audio", "ears/level", "diagnostics"]
# controls/state comes every 20 ms on the route, voice/privacy every 10 ms,
# ears/level every 100 ms, the eyes' diagnostics once a second: a microphone
# with nothing for this long is silent, a lease older than this is gone.
QUIET_MS = 600
# A baseline is read over this long, so the eyes have said how they are.
BASELINE_S = 2.5
# After the switch is closed, it must stay closed this long once silent.
STAY_S = 1.0
PARTS = ["mic2", "mic1", "camera"]


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def now_ms():
    return int(time.time() * 1000)


def say(text):
    print(text, file=sys.stderr, flush=True)


def parse_line(text):
    """A `lamp-bus tap` line: the header's JSON, a space, the payload."""
    try:
        header, end = json.JSONDecoder().raw_decode(text)
        rest = text[end:].strip()
        payload = json.loads(rest) if rest and not rest.startswith("<") else None
    except ValueError:
        return None
    return header, payload


class Watch:
    """Taps every topic until stopped; keeps (at_unix_ms, topic, payload)."""

    def __init__(self, binary, root, seconds):
        self.messages, self.lock, self.procs = [], threading.Lock(), []
        base = [binary] + (["--root", root] if root else [])
        for topic in TOPICS:
            proc = subprocess.Popen(base + ["tap", topic, "--seconds", str(seconds)],
                                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                    text=True, bufsize=1)
            self.procs.append(proc)
            threading.Thread(target=self._read, args=(topic, proc), daemon=True).start()

    def _read(self, topic, proc):
        for line in proc.stdout:
            seen = parse_line(line.strip())
            if seen is None:
                continue
            header, payload = seen
            with self.lock:
                self.messages.append((header.get("at_unix_ms", 0), topic, payload))

    def since(self, since_ms):
        with self.lock:
            return sorted(m for m in self.messages if m[0] >= since_ms)

    def stop(self):
        for proc in self.procs:
            if proc.poll() is None:
                proc.terminate()
        for proc in self.procs:
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()


def switch_of(payload):
    """'closed', 'open', 'unknown' (a fault) or 'no_route' from controls/state."""
    if not isinstance(payload, dict):
        return None
    privacy = payload.get("privacy")
    if privacy is None:
        return "no_route"
    muted = privacy.get("muted")
    return "unknown" if muted is None else ("closed" if muted else "open")


def eyes_camera(payload):
    if not isinstance(payload, dict) or payload.get("name") != "eyes":
        return None
    for kv in payload.get("values", []):
        if kv.get("key") == "camera":
            return kv.get("value")
    return None


def state_at(messages, at_ms):
    """Each part's state at `at_ms`, from the messages up to it."""
    upto = [m for m in messages if m[0] <= at_ms]
    recent = [m for m in upto if m[0] > at_ms - QUIET_MS]
    switch = None
    for _, topic, payload in upto:
        if topic == "controls/state":
            switch = switch_of(payload) or switch
    audio = any(t == "mic2/audio" for _, t, _ in recent)
    lease = None
    for _, topic, payload in recent:
        if topic == "voice/privacy" and isinstance(payload, dict):
            lease = bool(payload.get("permitted"))
    mic2 = "open" if audio or lease else "silent"
    mic1 = "open" if any(t == "ears/level" for _, t, _ in recent) else "silent"
    camera = None
    for _, topic, payload in upto:
        if topic == "diagnostics":
            camera = eyes_camera(payload) or camera
    return {"switch": switch, "mic2": mic2, "mic2_audio": audio, "mic2_lease": lease,
            "mic1": mic1, "camera": camera or "unheard"}


def is_open(state, part):
    return state[part] == "open"


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


def first_switch(messages, want, since_ms):
    for at, topic, payload in messages:
        if at >= since_ms and topic == "controls/state" and switch_of(payload) == want:
            return at
    return None


def wait_for_person(watch, want, since_ms, timeout_s):
    """Wait for the switch to read `want` or for Enter. Returns
    ('moved', at_ms), ('enter', None) or ('none', None)."""
    deadline = time.monotonic() + timeout_s
    eof = False
    while time.monotonic() < deadline:
        at = first_switch(watch.since(since_ms), want, since_ms)
        if at is not None:
            return "moved", at
        line = read_line(0.1) if not eof else None
        if line is _EOF:
            # Nobody there to type: keep waiting for the switch only.
            eof = True
            continue
        if line is None:
            if eof:
                time.sleep(0.1)
            continue
        # Enter: give the reading a moment to follow.
        end = time.monotonic() + 1.0
        while time.monotonic() < end:
            at = first_switch(watch.since(since_ms), want, since_ms)
            if at is not None:
                return "moved", at
            time.sleep(0.05)
        return "enter", None
    return "none", None


def settle(watch, from_ms, parts, want_open, within_s):
    """Each part's first time at the wanted state after `from_ms`, judged by
    the state at every 50 ms step up to `within_s` later, and that it then
    stays there for STAY_S. Returns {part: ms after from_ms or None}."""
    end_ms = from_ms + int(within_s * 1000)
    # Let the window, and STAY_S after it, pass.
    while now_ms() < end_ms + int(STAY_S * 1000) + 200:
        time.sleep(0.05)
    messages = watch.since(from_ms - 5000)
    reached = {}
    for part in parts:
        got = None
        t = from_ms
        while t <= end_ms:
            if is_open(state_at(messages, t), part) == want_open:
                # It must hold from there for STAY_S (a flap is not a close).
                holds = all(is_open(state_at(messages, u), part) == want_open
                            for u in range(t, t + int(STAY_S * 1000) + 1, 100))
                if holds:
                    got = t - from_ms
                    break
            t += 50
        reached[part] = got
    return reached


def main():
    ap = argparse.ArgumentParser(description="TC-RT-06 privacy switch (see the header).")
    ap.add_argument("--within", type=float, default=5.0,
                    help="seconds allowed for the parts to close and to come back (5)")
    ap.add_argument("--answer-timeout", type=float, default=120.0,
                    help="seconds to wait for the person to move the switch (120)")
    ap.add_argument("--root", help="the bus root (else $LAMP_BUS_ROOT, else /run/lampos-bus)")
    ap.add_argument("--evidence", help="append one JSON object with the states and times")
    ap.add_argument("--dry-run", action="store_true", help="print the steps, ask nothing")
    args = ap.parse_args()
    if not 1.0 <= args.within <= 30.0:
        ap.error("--within is 1 to 30 s")

    evidence = {"check": CHECK, "started": utc_now(), "within_s": args.within}

    def finish(verdict, reason, code):
        evidence.update(verdict=verdict, reason=reason, ended=utc_now())
        if args.evidence:
            with open(args.evidence, "a") as f:
                f.write(json.dumps(evidence) + "\n")
        print(json.dumps({"check": CHECK, "verdict": verdict, "reason": reason,
                          "at": utc_now()}), flush=True)
        return code

    binary = os.environ.get("LAMP_BUS_BIN") or shutil.which("lamp-bus") or "/usr/local/bin/lamp-bus"
    if args.dry_run:
        for topic in TOPICS:
            say(f"[dry-run] lamp-bus tap {topic} --seconds N   (listen only)")
        say("[dry-run] then: ask to close the switch, judge; ask to open it, judge")
        return finish("not_checked", "dry run: nothing was read", 3)
    if not os.access(binary, os.X_OK):
        return finish("not_checked", f"lamp-bus not found ({binary}): not run on a lampOS lamp?", 2)

    lasting = BASELINE_S + 2 * (args.answer_timeout + args.within + STAY_S + 3) + 10
    watch = Watch(binary, args.root, lasting)
    try:
        start_ms = now_ms()
        time.sleep(BASELINE_S)
        before = state_at(watch.since(start_ms - 5000), now_ms())
        evidence["before"] = dict(before, at=utc_now())
        say(f"Before: switch {before['switch']}, mic2 {before['mic2']}, "
            f"mic1 {before['mic1']}, camera {before['camera']}")
        if before["switch"] is None:
            return finish("not_checked", "nothing on controls/state: is lamp-controls running?", 3)
        if before["switch"] == "no_route":
            return finish("not_checked", "controls/state carries no switch reading: "
                          "LAMP_PRIVACY_CONTROLS is not on in Controls, Voice and Ears", 3)
        if before["switch"] == "unknown":
            return finish("fail", "controls/state says the switch cannot be read (a fault)", 1)
        if before["switch"] == "closed":
            say("The switch reads closed: slide it open first, then press Enter.")
            how, at = wait_for_person(watch, "open", now_ms() - 100, args.answer_timeout)
            if how != "moved":
                return finish("not_checked", "the switch was closed at the start and was not opened", 3)
            time.sleep(args.within + 0.5)
            before = state_at(watch.since(start_ms - 5000), now_ms())
            evidence["before"] = dict(before, at=utc_now())
        opened_before = [p for p in PARTS if is_open(before, p)]
        if before["camera"] == "unheard":
            return finish("not_checked", "the eyes said nothing on diagnostics: is lamp-eyes running?", 3)
        not_open = [p for p in PARTS if p not in opened_before]
        if not_open:
            return finish("not_checked", "with the switch open these were not open before the switch "
                          f"was touched: {', '.join(not_open)} (their own checks say why)", 3)

        # Close.
        asked_ms = now_ms()
        say("Slide the privacy switch CLOSED now (or press Enter once it is).")
        how, closed_ms = wait_for_person(watch, "closed", asked_ms, args.answer_timeout)
        if how == "none":
            return finish("not_checked", "the person did not close the switch", 3)
        if how == "enter":
            return finish("fail", "the person closed the switch; controls/state still reads it open", 1)
        say("Switch read closed; watching the microphones and the camera...")
        closed = settle(watch, closed_ms, PARTS, False, args.within)
        evidence["closed"] = dict(state_at(watch.since(start_ms - 5000), now_ms()),
                                  at=utc_now(), ms_to_silent=closed)
        say(f"Closed: ms to silent/closed {closed}")
        still = [p for p in PARTS if closed[p] is None]

        # Open again, whatever the close gave, so the lamp is left open.
        asked_ms = now_ms()
        say("Slide the privacy switch OPEN again now (or press Enter once it is).")
        how, opened_ms = wait_for_person(watch, "open", asked_ms, args.answer_timeout)
        if how == "none":
            return finish("not_checked", "the person did not open the switch again; "
                          "the lamp is left with the switch closed", 3)
        if how == "enter":
            return finish("fail", "the person opened the switch; controls/state still reads it closed", 1)
        say("Switch read open; watching for all three to come back...")
        back = settle(watch, opened_ms, PARTS, True, args.within)
        evidence["after"] = dict(state_at(watch.since(start_ms - 5000), now_ms()),
                                 at=utc_now(), ms_to_back=back)
        say(f"After: ms to back {back}")
        late = [p for p in PARTS if back[p] is None]

        if still or late:
            parts = []
            if still:
                parts.append(f"still on {args.within:g} s after closing: {', '.join(still)}")
            if late:
                parts.append(f"not back {args.within:g} s after opening: {', '.join(late)}")
            return finish("fail", "; ".join(parts), 1)
        slowest_close = max(closed.values()) / 1000
        slowest_back = max(back.values()) / 1000
        return finish("pass", f"closed: both microphones silent and the camera closed in "
                      f"{slowest_close:.1f} s; opened: all three back in {slowest_back:.1f} s", 0)
    except KeyboardInterrupt:
        return finish("interrupted", "stopped by the operator", 3)
    finally:
        watch.stop()


if __name__ == "__main__":
    sys.exit(main())
