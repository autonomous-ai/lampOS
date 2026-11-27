#!/usr/bin/env python3
# servo-calibrate.py — the factory run's spoken servo calibration (kyle-aus,
# 6 Nov: a replaced servo on lamp-a0ae). Torque stays off; a person moves the
# joints by hand while the lamp tells them what to do on its own speaker.
#
#   servo-calibrate.py check            exit 0 calibrated, 3 needs calibrating
#   servo-calibrate.py run [SECONDS]    spoken: 10 s warning, 5 s at rest, then
#                                       the joints moved, and the head pads and
#                                       buttons pressed, until the person
#                                       presses the reset button (SECONDS at
#                                       most, default 300); checked, then
#                                       written to the servos
#
# Every press while calibrating is counted and recorded (kyle-aus, 10 Nov):
# the head pads (chip 0's driven lines among 37, 96, 97, 98), the button, the
# privacy switch and the reset. A reset press ends the movement; if a joint
# has not yet moved far enough the lamp says so and keeps recording.
# lamp-controls and lamp-ears are stopped while the lines are read (as
# input-check.py does) and started again after.
#
# A servo needs calibrating when base yaw, wrist roll or head pitch still has
# the factory limits 0-4095 (new or replaced servos; every calibrated lamp has
# them narrowed). Offsets: the rest pose reads 2048 (head pitch 2010, inside
# the body's home window 1740-2030). Limits: the moved range +-20 for yaw, roll
# and pitch; shoulder and elbow keep 0-4095. Nothing is written unless the
# rest was still and every limited joint moved through most of its range.
# Prints RECORD KEY=value lines for factory-run.sh (check also records each
# servo's register 85 and firmware). Run as root, with
# lamp-body stopped (it owns the servo port otherwise).
import importlib.util, json, os, select, subprocess, sys, termios, time

PROMPTS = os.environ.get("PROMPTS", "/var/lib/lamp-test/prompts")
PORT = os.environ.get("SERVO_PORT", "/dev/ttyACM0")
NAMES = {1: "base yaw", 2: "shoulder", 3: "elbow", 4: "wrist roll", 5: "head pitch"}
TARGET = {1: 2048, 2: 2048, 3: 2048, 4: 2048, 5: 2010}
LIMITED = (1, 4, 5)
# Smallest moved range accepted (calibrated lamps move about yaw 2490, roll
# 2250, pitch 1700 counts).
MIN_MOVE = {1: 1800, 4: 1800, 5: 1200}
REST_SPREAD = 30
PINS = os.environ.get("PINS", "/etc/lampOS/controls/controls-pins.json")
# The presses counted while calibrating, as input-check.py --touch-from reads them.
TOUCH_FILE = os.environ.get("CAL_TOUCH_FILE", "/var/lib/lamp-test/cal-touch.json")
HERE = os.path.dirname(os.path.abspath(__file__))

fd = os.open(PORT, os.O_RDWR | os.O_NOCTTY)
t = termios.tcgetattr(fd); t[0] = 0; t[1] = 0; t[2] = termios.CS8 | termios.CREAD | termios.CLOCAL; t[3] = 0
t[4] = t[5] = termios.B1000000; termios.tcsetattr(fd, termios.TCSANOW, t)


def xfer(i, inst, params, n):
    pkt = [0xFF, 0xFF, i, len(params) + 2, inst] + params
    pkt.append((~sum(pkt[2:])) & 0xFF)
    termios.tcflush(fd, termios.TCIOFLUSH); os.write(fd, bytes(pkt))
    buf = b""; end = time.time() + 0.1
    while len(buf) < 6 + n and time.time() < end:
        if select.select([fd], [], [], 0.02)[0]:
            buf += os.read(fd, 64)
    k = buf.find(b"\xff\xff")
    if k < 0 or len(buf) < k + 6 + n:
        return None
    return buf[k + 5:k + 5 + n]


def read(i, addr, n):
    d = xfer(i, 2, [addr, n], n)
    if d is None:
        return None
    return d[0] | (d[1] << 8) if n == 2 else d[0]


def write(i, addr, vals):
    if xfer(i, 3, [addr] + vals, 0) is None:
        raise SystemExit(f"servo-calibrate: id {i}: no answer writing register {addr}")


def write16(i, addr, v): write(i, addr, [v & 0xFF, (v >> 8) & 0xFF])
def sm_get(v): return -(v & 0x7FF) if v & 0x800 else v
def sm_put(v): return (0x800 | (-v)) if v < 0 else v


def raw(i):
    p = read(i, 56, 2); o = read(i, 31, 2)
    return None if p is None or o is None else (p + sm_get(o)) % 4096


player = None


def speak(key, wait=True):
    """One prompt at a time on the lamp's speaker; a missing prompt is silence."""
    global player
    path = f"{PROMPTS}/{key}.wav"
    print(f"servo-calibrate: say {key}", flush=True)
    if player is not None:
        player.wait()
    player = None
    if not os.path.exists(path):
        return
    player = subprocess.Popen(["aplay", "-q", "-D", "device_speaker", path],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if wait:
        player.wait(); player = None


def limits():
    return {i: (read(i, 9, 2), read(i, 11, 2)) for i in range(1, 6)}


def check():
    lim = limits()
    missing = [i for i, v in lim.items() if None in v]
    if missing:
        raise SystemExit(f"servo-calibrate: no answer from servo(s) {missing}")
    need = [NAMES[i] for i in LIMITED if lim[i] == (0, 4095)]
    print("servo-calibrate: limits " + ", ".join(f"{NAMES[i]} {a}-{b}" for i, (a, b) in lim.items()))
    # Kept for reference (#7946): each servo's maximum acceleration (register
    # 85, new servos ship 50 or 254) and its firmware (registers 0-1) and
    # servo version (3-4), so a batch can be traced later. Read only.
    def v(i, a):
        x = read(i, a, 1)
        return "?" if x is None else x
    print("RECORD SERVO_MAX_ACCEL=" + " ".join(f"{i}:{v(i, 85)}" for i in range(1, 6)))
    print("RECORD SERVO_FIRMWARE=" + " ".join(
        f"{i}:{v(i, 0)}.{v(i, 1)}/{v(i, 3)}.{v(i, 4)}" for i in range(1, 6)))
    if need:
        print(f"RECORD SERVO_CAL_NEEDED=yes ({', '.join(need)} at 0-4095)")
        return 3
    return 0


def open_inputs():
    """The head pads, buttons, reset and privacy switch as (Line, rest), by name."""
    spec = importlib.util.spec_from_file_location("input_check", os.path.join(HERE, "input-check.py"))
    ic = importlib.util.module_from_spec(spec); spec.loader.exec_module(ic)
    profile = json.load(open(PINS))
    chip0 = profile.get("chip") or "/dev/gpiochip0"
    inputs = {}
    for n in ic.CANDIDATES:
        up, down = ic.read_once(chip0, n, "up"), ic.read_once(chip0, n, "down")
        if up == down:                         # driven: a pad's chip, resting at that level
            inputs[f"pad{n}"] = (ic.Line(chip0, n, "up" if up else "down"), up)
    for btn in profile.get("buttons", []):
        inputs[f"BUTTON_{btn['name'].upper()}"] = (ic.Line(chip0, btn["line"], btn.get("pull", "up")), 1 if btn.get("active_low", True) else 0)
    if profile.get("reset"):
        r = profile["reset"]
        inputs["RESET_BUTTON"] = (ic.Line(chip0, r["line"], r.get("pull", "up")), 1 if r.get("active_low", True) else 0)
    if profile.get("privacy"):
        pv = profile["privacy"]
        line = ic.Line(pv.get("chip", "/dev/gpiochip1"), pv["line"], pv.get("pull", "up"))
        inputs["PRIVACY_SWITCH"] = (line, line.value())   # a slide: its rest is where it is now
    return inputs


def judge(rows):
    """Offsets and limits from the rest window and the moved range, and what is short."""
    rest = [r for at, r in rows if 1.5 <= at <= 4.5]
    if len(rest) < 10:
        return None, ["too few servo reads at rest"], []
    out = {}; problems = []; short = []
    for i in range(1, 6):
        vals = sorted(r[i - 1] for r in rest)
        mid = vals[len(vals) // 2]
        if max(abs((v - mid + 2048) % 4096 - 2048) for v in vals) > REST_SPREAD:
            problems.append(f"{NAMES[i]} not still at rest")
        off = mid - TARGET[i]
        off = off - 4096 if off > 2047 else off + 4096 if off < -2047 else off
        pos = [(r[i - 1] - mid + TARGET[i]) % 4096 for _, r in rows]
        lo, hi = min(pos), max(pos)
        if i in LIMITED:
            if hi - lo < MIN_MOVE[i]:
                short.append(f"{NAMES[i]} moved only {lo}-{hi}")
            lo, hi = max(0, lo - 20), min(4095, hi + 20)
        else:
            lo, hi = 0, 4095
        out[i] = (off, lo, hi)
    return out, problems, short


def run(secs):
    # The voice holds the speaker, and a busy speaker makes every prompt
    # silent (lamp-9224, 9 Nov); factory-run.sh starts it again at its end.
    subprocess.run(["systemctl", "stop", "lamp-voice"], check=False)
    held = [u for u in ("lamp-ears", "lamp-controls")
            if subprocess.run(["systemctl", "is-active", "--quiet", u]).returncode == 0]
    for u in held:
        subprocess.run(["systemctl", "stop", u], check=False)
    try:
        return calibrate(secs)
    finally:
        for u in held:
            subprocess.run(["systemctl", "start", u], check=False)


def calibrate(secs):
    inputs = open_inputs()
    # Per input, as input-check.py counts them: presses away from rest, every
    # change, the longest held and whether it is still away at the end.
    seen = {k: {"pulses": 0, "longest_s": 0.0, "changes": 0, "away_at_end": False} for k in inputs}
    since = {k: None for k in inputs}
    print("servo-calibrate: inputs " + ", ".join(inputs), flush=True)
    for i in range(1, 6):
        write(i, 40, [0])                      # torque off: the joints move by hand
    speak("factory_cal_needed")                # "...starts in ten seconds..."
    time.sleep(3)
    speak("factory_cal_rest", wait=False)      # "Hold still."
    rows = []; t0 = time.time(); moved_cue = False; last = 0; ended = "time"
    while (now := time.time() - t0) < secs:
        if now >= 5 and not moved_cue:
            speak("factory_cal_move", wait=False)   # move, touch, press; reset when done
            moved_cue = True
        row = [raw(i) for i in range(1, 6)]
        if None not in row:
            rows.append((now, row))
        reset_pressed = False
        for k, (line, rest) in inputs.items():
            on = line.value() != rest; c = seen[k]
            if on != c["away_at_end"]:
                c["changes"] += 1
                if on:
                    c["pulses"] += 1; since[k] = now
                    print(f"servo-calibrate: {now:5.1f} s {k.lower()} pressed ({c['pulses']})", flush=True)
                    if k == "RESET_BUTTON" and now >= 5:
                        reset_pressed = True
                else:
                    c["longest_s"] = max(c["longest_s"], round(now - since[k], 2))
            c["away_at_end"] = on
        if now - last > 5:
            print(f"servo-calibrate: {now:4.0f} s " + " ".join(f"{i}={v}" for i, v in enumerate(row, 1)), flush=True)
            last = now
        if reset_pressed:
            out, problems, short = judge(rows)
            if problems or not short:
                ended = "reset"; break
            print("servo-calibrate: reset pressed but " + "; ".join(short) + ": keep moving", flush=True)
            speak("factory_cal_more", wait=False)   # "Keep moving..."
    for k, c in seen.items():
        if c["away_at_end"] and since[k] is not None:
            c["longest_s"] = max(c["longest_s"], round(time.time() - t0 - since[k], 2))
    with open(TOUCH_FILE, "w") as f:
        json.dump(seen, f, indent=1)
    print("RECORD SERVO_CAL_INPUTS=" + " ".join(f"{k.lower()}:{c['pulses']}" for k, c in seen.items())
          + f" (ended by {ended})")
    for line, _ in inputs.values():
        line.close()
    speak("factory_cal_stop", wait=False)      # "Stop. Fold my arm down..."
    out, problems, short = judge(rows)
    if problems or short:
        return fail("; ".join(problems + short))
    for i, (off, lo, hi) in out.items():
        if read(i, 40, 1):
            return fail(f"{NAMES[i]}: torque came on")
        write(i, 55, [0]); write16(i, 31, sm_put(off)); write16(i, 9, lo); write16(i, 11, hi); write(i, 55, [1])
        time.sleep(0.05)
    back = {i: (sm_get(read(i, 31, 2)), read(i, 9, 2), read(i, 11, 2)) for i in range(1, 6)}
    if back != out:
        return fail(f"read back {back}, wrote {out}")
    speak("factory_cal_done")                  # "Calibration saved..."
    print("RECORD SERVO_CAL=" + " ".join(f"{i}:{o}/{a}-{b}" for i, (o, a, b) in out.items())
          + f" ({time.strftime('%F %T %z')})")
    return 0


def fail(why):
    print(f"RECORD SERVO_CAL=FAIL {why}")
    speak("factory_cal_fail")
    return 1


cmd = sys.argv[1] if len(sys.argv) > 1 else "check"
if cmd == "check":
    sys.exit(check())
if cmd == "run":
    sys.exit(run(float(sys.argv[2]) if len(sys.argv) > 2 else 300))
sys.exit(f"usage: {sys.argv[0]} check | run [SECONDS]")
