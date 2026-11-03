#!/usr/bin/env python3
# input-check.py — every input a person touches, in one window: the head pads,
# the button, the reset button and the privacy switch. 30 s to touch and press
# everything in any order, then 30 s hands off. The head pads are written into
# the pins file as steps.sh step 2 writes them (head_pads and head_rest, which
# lamp-controls reads, #711); every input's result is printed as RECORD lines.
#
# Every lamp has its head pads on two (or more) of chip 0's lines 37, 96, 97
# and 98, and which ones differs by unit: a hardware limitation (#1798, #2182).
# The button, the reset and the privacy switch are where the pins file says.
#
# Run on the lamp, as root:
#
#   input-check.py            30 s touch and press, 30 s hands off, write the pins
#   input-check.py --dry-run  the same reading, nothing written
#
# 1. Each pad candidate is read with a pull-up and with a pull-down: a line
#    that reads the same both ways is driven (a pad's chip), its level its
#    rest and so its polarity; a floating line follows the pull and is no pad.
# 2. Touch window: it opens at the person's first touch or press (the lamp
#    asks for one and waits up to --wait-seconds), so nobody races a clock;
#    each line's changes away from its rest are counted, with the longest held.
# 3. Quiet window: it opens once nothing has been touched for --settle-seconds
#    after the lamp says hands off (touches finishing late count as touches);
#    then any change on a pad is counted against it, and an input still
#    pressed at the end is stuck.
# A pad: a driven candidate touched twice or held 0.5 s, still when quiet, as
# steps.sh judges. Pass: two or more pads, the button and the reset each
# pressed, nothing stuck. The privacy switch is recorded, moved or not.
#
# lamp-controls is stopped while the lines are read (lamp-voice and lamp-ears
# before it, as steps.sh does), so a reset held here resets nothing; all are
# started again after, however this ends. The profile is kept as
# PINS.before-input-check-STAMP and the new pins renamed over it.
#
# Exit 0 pass (pins written), 1 fail (nothing written), 2 could not run.
# Python 3 standard library only.

import argparse, fcntl, json, os, struct, subprocess, sys, time

CANDIDATES = [37, 96, 97, 98]
INPUT, PULL_UP, PULL_DOWN = 1 << 2, 1 << 8, 1 << 9
SYSTEMCTL = os.environ.get("SYSTEMCTL", "systemctl")
APLAY = os.environ.get("APLAY", "aplay")


def iowr(nr, size):
    return (3 << 30) | (size << 16) | (0xB4 << 8) | nr


class Line:
    """One GPIO line as an input with a bias (kernel GPIO v2 chardev uAPI)."""

    def __init__(self, chip, line, pull):
        req = bytearray(592)
        struct.pack_into("<I", req, 0, line)
        req[256:256 + 12] = b"input-check\0"
        struct.pack_into("<Q", req, 288, INPUT | (PULL_UP if pull == "up" else PULL_DOWN))
        struct.pack_into("<I", req, 560, 1)
        self.chip = os.open(chip, os.O_RDWR)
        try:
            fcntl.ioctl(self.chip, iowr(0x07, 592), req)
        except OSError:
            os.close(self.chip)
            raise
        self.fd = struct.unpack_from("<i", req, 588)[0]

    def value(self):
        vals = bytearray(struct.pack("<QQ", 0, 1))
        fcntl.ioctl(self.fd, iowr(0x0E, 16), vals)
        return struct.unpack_from("<Q", vals, 0)[0] & 1

    def close(self):
        os.close(self.fd)
        os.close(self.chip)


def read_once(chip, line, pull):
    l = Line(chip, line, pull)
    try:
        time.sleep(0.005)
        return l.value()
    finally:
        l.close()


class Voice:
    def __init__(self, args):
        self.args = args

    def say(self, key, wait=False):
        path = os.path.join(self.args.prompts, key + ".wav")
        if self.args.no_voice or not os.path.exists(path):
            return
        p = subprocess.Popen([APLAY, "-q", "-D", self.args.speaker, path],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if wait:
            p.wait(timeout=20)


def watch(inputs, seconds, every=0.01, cue=None, settle=None):
    """Poll every input for seconds. inputs: name -> (Line, rest level).
    Per input: completed pulses away from rest, the longest, every change,
    and whether it is away from rest at the end. cue: (at s, callable) once.
    settle: end early once every input has been at rest that many seconds."""
    out = {k: {"pulses": 0, "longest_s": 0.0, "changes": 0, "away_at_end": False} for k in inputs}
    away = {k: None for k in inputs}
    prev = {k: r for k, (_, r) in inputs.items()}
    start = last = time.time()
    while time.time() < start + seconds:
        now = time.time()
        if settle and now - last >= settle and all(a is None for a in away.values()):
            break
        if cue and now - start >= cue[0]:
            cue[1]()
            cue = None
        for k, (l, rest) in inputs.items():
            v = l.value()
            if v != prev[k]:
                last = now
                out[k]["changes"] += 1
                if v != rest:
                    away[k] = now
                elif away[k] is not None:
                    out[k]["pulses"] += 1
                    out[k]["longest_s"] = max(out[k]["longest_s"], now - away[k])
                    away[k] = None
                prev[k] = v
        time.sleep(every)
    for k in inputs:
        if away[k] is not None:
            out[k]["away_at_end"] = True
            out[k]["pulses"] += 1
            out[k]["longest_s"] = max(out[k]["longest_s"], time.time() - away[k])
    return out


def merged(a, b):
    """Two watches of the same inputs as one."""
    return {k: {"pulses": a[k]["pulses"] + b[k]["pulses"], "longest_s": max(a[k]["longest_s"], b[k]["longest_s"]),
                "changes": a[k]["changes"] + b[k]["changes"], "away_at_end": b[k]["away_at_end"]} for k in a}


def first_touch(inputs, seconds, every=0.01):
    """Wait for any input to leave its rest; True when one does."""
    end = time.time() + seconds
    while time.time() < end:
        for l, rest in inputs.values():
            if l.value() != rest:
                return True
        time.sleep(every)
    return False


def main():
    ap = argparse.ArgumentParser(description="Every input in one window (see the header).")
    ap.add_argument("--pins", default="/etc/lampOS/controls/controls-pins.json")
    ap.add_argument("--wait-seconds", type=float, default=300.0,
                    help="how long the lamp waits for the first touch that opens the touch window")
    ap.add_argument("--touch-seconds", type=float, default=30.0)
    ap.add_argument("--quiet-seconds", type=float, default=30.0)
    ap.add_argument("--settle-seconds", type=float, default=3.0,
                    help="after hands off, how long nothing is touched before the quiet window opens")
    ap.add_argument("--prompts", default="/var/lib/lamp-test/prompts")
    ap.add_argument("--speaker", default="device_speaker")
    ap.add_argument("--no-voice", action="store_true")
    ap.add_argument("--dry-run", action="store_true", help="read and judge, write nothing")
    ap.add_argument("--quiet-after", metavar="FILE",
                    help="run beside the arm's stand: the touch window stays open until FILE exists "
                         "(the stand done), so hands off never meets a moving arm; the prompts before it "
                         "are left to the stand, which speaks")
    ap.add_argument("--quiet-after-max-seconds", type=float, default=600.0)
    args = ap.parse_args()
    voice = Voice(args)
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    record = []

    try:
        profile = json.load(open(args.pins))
    except (OSError, ValueError) as e:
        print(f"input-check: cannot read {args.pins}: {e}", file=sys.stderr)
        return 2
    chip0 = profile.get("chip") or "/dev/gpiochip0"
    # The switches as the pins file names them: (key, chip, line, pull, rest).
    switches = []
    for b in profile.get("buttons", []):
        switches.append((f"BUTTON_{b['name'].upper()}", chip0, b["line"], b.get("pull", "up"),
                         1 if b.get("active_low", True) else 0))
    if profile.get("reset"):
        r = profile["reset"]
        switches.append(("RESET_BUTTON", chip0, r["line"], r.get("pull", "up"), 1 if r.get("active_low", True) else 0))
    if profile.get("privacy"):
        p = profile["privacy"]
        switches.append(("PRIVACY_SWITCH", p.get("chip", "/dev/gpiochip1"), p["line"], p.get("pull", "up"), None))

    stopped = [u for u in ("lamp-voice", "lamp-ears", "lamp-controls")
               if subprocess.run([SYSTEMCTL, "is-active", "--quiet", u]).returncode == 0]
    for u in stopped:
        subprocess.run([SYSTEMCTL, "stop", u], check=False)
    held = {}
    try:
        rest = {}
        for n in CANDIDATES:
            up, down = read_once(chip0, n, "up"), read_once(chip0, n, "down")
            print(f"pad line {n}: {'driven, rests ' + ('high' if up else 'low') if up == down else 'floating (no pad)'}")
            if up == down:
                rest[n] = up
        for n in rest:
            held[f"pad{n}"] = (Line(chip0, n, "up" if rest[n] else "down"), rest[n])
        for key, chip, line, pull, idle in switches:
            l = Line(chip, line, pull)
            # The privacy switch is a slide: its rest is wherever it is now.
            held[key] = (l, l.value() if idle is None else idle)
            print(f"{key.lower()}: {chip} line {line}, now {'at rest' if l.value() == held[key][1] else 'PRESSED'}")

        # Beside the stand the stand speaks; this one keeps quiet until hands off.
        early = (lambda key: None) if args.quiet_after else voice.say
        early("factory_inputs_wait")
        print(f"waiting up to {args.wait_seconds:g} s for the first touch or press", flush=True)
        if not first_touch(held, args.wait_seconds):
            started = False
        else:
            started = True
            early("factory_inputs_touch")
            print(f"touch: {args.touch_seconds:g} s from the first touch, touch each head pad, press the button "
                  f"and the reset briefly, any order", flush=True)
        if started:
            # The first touch is still under the hand: watch() starts from rest, so it counts.
            touch = watch(held, args.touch_seconds,
                          cue=(args.touch_seconds - 10, lambda: early("factory_padcheck_10")))
            if args.quiet_after and not os.path.exists(args.quiet_after):
                # The arm is still standing: what is touched meanwhile is a touch,
                # and hands off waits for the stand to finish.
                print("touch: the arm is still standing; hands off waits for it", flush=True)
                end = time.time() + args.quiet_after_max_seconds
                while not os.path.exists(args.quiet_after) and time.time() < end:
                    touch = merged(touch, watch(held, 0.2))
            voice.say("factory_padcheck_rest")
            # Touches finishing after "hands off" are touches: the quiet window
            # waits for nothing to be touched for --settle-seconds (at most 60 s).
            late = watch(held, 60, settle=args.settle_seconds)
            touch = merged(touch, late)
            print(f"quiet: {args.quiet_seconds:g} s, hands off (opened after {args.settle_seconds:g} s untouched)", flush=True)
            quiet = watch(held, args.quiet_seconds)
        else:
            touch = quiet = {k: {"pulses": 0, "longest_s": 0.0, "changes": 0, "away_at_end": False} for k in held}
            print(f"nobody touched the lamp within {args.wait_seconds:g} s", flush=True)
    finally:
        for l, _ in held.values():
            l.close()
        for u in reversed(stopped):
            subprocess.run([SYSTEMCTL, "start", u], check=False)

    path = f"/var/tmp/factory-input-check-{stamp}.json"
    try:
        json.dump({"stamp": stamp, "pad_rest": {str(n): rest[n] for n in rest},
                   "touch": touch, "quiet": quiet}, open(path, "w"), indent=1)
    except OSError:
        path = "(not kept)"

    pads, restless, problems = [], [], []
    for n in rest:
        t, q = touch[f"pad{n}"], quiet[f"pad{n}"]
        print(f"pad line {n}: touch {t['pulses']} pulses, longest {t['longest_s']:.1f} s; quiet {q['changes']} changes")
        if t["pulses"] >= 2 or t["longest_s"] >= 0.5:
            if q["changes"]:
                restless.append(n)
            else:
                pads.append({"line": n, "active_low": bool(rest[n]), "pull": "up" if rest[n] else "down",
                             "reads": "level" if t["longest_s"] >= 0.5 else "pulses"})
    for key, _, line, _, idle in switches:
        t, q = touch[key], quiet[key]
        stuck = q["away_at_end"]
        if key == "PRIVACY_SWITCH":
            state = f"moved {t['changes']}x" if t["changes"] else "not moved"
        else:
            state = "STUCK" if stuck else (f"ok ({t['pulses']} presses)" if t["pulses"] else "NOT PRESSED")
            if stuck or not t["pulses"]:
                problems.append(f"{key.lower()} {state.lower()}")
        print(f"{key.lower()} (line {line}): {state}")
        record.append(f"{key}={state}")
    if restless:
        problems.append(f"pad lines {restless} changed with hands off (#1764)")
    if len(pads) < 2:
        problems.append(f"head pads on {[p['line'] for p in pads]}, not two or more touched twice or held")
    lines = ",".join(str(p["line"]) for p in pads)
    record.insert(0, f"HEAD_PAD_LINES={lines or 'none'}")
    record.insert(1, "HEAD_PADS=" + ",".join(f"head-{i + 1}:{p['line']}:"
                  f"{'active-low' if p['active_low'] else 'active-high'}:{p['reads']}" for i, p in enumerate(pads)))
    record.append("BASE_STRIP=not read on this image (LAMP_CONTROLS_STRIP unset)")
    record.append(f"INPUTS={'PASS' if not problems else 'FAIL: ' + '; '.join(problems)}")
    for r in record:
        print(f"RECORD {r}")
    if problems:
        print(f"input-check: FAILED: {'; '.join(problems)}: nothing written (report: {path})", file=sys.stderr)
        voice.say("factory_inputs_fail", wait=True)
        return 1
    if args.dry_run:
        print(f"dry run: {args.pins} not written")
        return 0
    new = dict(profile)
    new["head_pads"] = [{"name": f"head-{i + 1}", "line": p["line"], "active_low": p["active_low"],
                         "pull": p["pull"]} for i, p in enumerate(pads)]
    new["head_rest"] = [{"name": f"head-{i + 1}", "rest": "high" if p["active_low"] else "low",
                         "reads": p["reads"]} for i, p in enumerate(pads)]
    tmp = f"{args.pins}.input-check-{stamp}"
    with open(tmp, "w") as f:
        json.dump(new, f, indent=2)
        f.write("\n")
    os.chmod(tmp, os.stat(args.pins).st_mode & 0o777)
    keep = f"{args.pins}.before-input-check-{stamp}"
    subprocess.run(["cp", "-p", args.pins, keep], check=True)
    os.replace(tmp, args.pins)
    subprocess.run([SYSTEMCTL, "restart", "lamp-controls"], check=False)
    print(f"head pads in {args.pins}; the profile's kept as {keep} (report: {path})")
    voice.say("factory_inputs_done", wait=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
