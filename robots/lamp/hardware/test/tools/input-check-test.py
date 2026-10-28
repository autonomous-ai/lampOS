#!/usr/bin/env python3
# input-check-test.py — input-check.py on the host with stand-in GPIO lines:
# no lamp, no sound. Each case scripts what the person does over time and
# checks the verdict and when the quiet window opened.
#
#   python3 input-check-test.py

import importlib.util, json, os, sys, tempfile, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
os.environ["SYSTEMCTL"] = "true"
os.environ["APLAY"] = "true"
spec = importlib.util.spec_from_file_location("input_check", os.path.join(HERE, "input-check.py"))
ic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ic)

T0 = [0.0]
PRESSED = {}  # (chip, line) -> list of (from s, to s) away from rest
REST = {("/dev/gpiochip0", n): 0 for n in (37, 96, 97, 98)}  # pads rest low
REST.update({("/dev/gpiochip0", 99): 1, ("/dev/gpiochip0", 100): 1, ("/dev/gpiochip1", 9): 1})
PADS = (97, 98)  # driven; 37 and 96 float


def now():
    return time.time() - T0[0]


class FakeLine:
    def __init__(self, chip, line, pull):
        self.key = (chip, line)

    def value(self):
        rest = REST[self.key]
        return 1 - rest if any(a <= now() < b for a, b in PRESSED.get(self.key, [])) else rest

    def close(self):
        pass


def fake_read_once(chip, n, pull):
    return 0 if n in PADS else (1 if pull == "up" else 0)


ic.Line = FakeLine
ic.read_once = fake_read_once


def run(case, presses, argv, stand_done_at=None):
    PRESSED.clear()
    PRESSED.update({("/dev/gpiochip0", k): v for k, v in presses.items()})
    tmp = tempfile.mkdtemp()
    pins = os.path.join(tmp, "pins.json")
    json.dump({"chip": "/dev/gpiochip0", "buttons": [{"name": "primary", "line": 99}],
               "reset": {"line": 100}, "privacy": {"chip": "/dev/gpiochip1", "line": 9}}, open(pins, "w"))
    flag = os.path.join(tmp, "stand-done")
    args = ["input-check.py", "--pins", pins, "--wait-seconds", "3", "--touch-seconds", "1",
            "--quiet-seconds", "1", "--settle-seconds", "0.3", "--prompts", tmp] + argv
    args = [a.replace("FLAG", flag) for a in args]
    if stand_done_at is not None:
        threading.Timer(stand_done_at, lambda: open(flag, "w").close()).start()
    out = []
    real_print = print
    sys.argv = args
    T0[0] = time.time()
    import builtins
    builtins.print = lambda *a, **k: out.append(" ".join(str(x) for x in a))
    try:
        rc = ic.main()
    finally:
        builtins.print = real_print
    return rc, now(), out


passed = failed = 0


def check(name, ok, detail=""):
    global passed, failed
    if ok:
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name} {detail}")


everything = {97: [(0.2, 0.4), (0.6, 0.8)], 98: [(0.3, 0.5), (0.7, 0.9)], 99: [(0.4, 0.5)], 100: [(0.5, 0.6)]}

# 1. Alone (as before): pass, done in about touch + settle + quiet.
rc, took, out = run("alone", everything, [])
check("alone: pass", rc == 0, out[-3:])
check("alone: no waiting for a stand", took < 3.5, f"{took:.1f} s")

# 2. Beside a stand that ends at 2.5 s: pass, and hands off only after it.
rc, took, out = run("beside", everything, ["--quiet-after", "FLAG"], stand_done_at=2.5)
check("beside: pass", rc == 0, out[-3:])
check("beside: waited for the stand", took >= 2.5 + 0.3 + 1.0, f"{took:.1f} s")
check("beside: said so", any("hands off waits" in l for l in out))

# 3. Beside a stand, touches made while the arm still stands count.
late = {97: [(1.5, 1.7), (1.9, 2.1)], 98: [(1.6, 1.8), (2.0, 2.2)], 99: [(0.2, 0.3)], 100: [(1.4, 1.5)]}
rc, took, out = run("late", late, ["--quiet-after", "FLAG"], stand_done_at=2.5)
check("late touches during the stand count: pass", rc == 0, out[-4:])

# 4. A pad that changes once hands off has begun still fails it (the quiet check is kept).
noisy = dict(everything)
noisy[97] = everything[97] + [(3.5, 3.6)]  # hands off opens about 2.8-3.1 s, for 1 s
rc, took, out = run("noisy", noisy, ["--quiet-after", "FLAG"], stand_done_at=2.5)
check("a pad changing in the quiet window: fail", rc == 1, out[-4:])

# 5. The stand already done before the touch window ends: no extra wait.
rc, took, out = run("done-early", everything, ["--quiet-after", "FLAG"], stand_done_at=0.1)
check("stand done early: pass", rc == 0, out[-3:])
check("stand done early: no extra wait", took < 3.5, f"{took:.1f} s")

print(f"{passed + failed} cases, {passed} pass")
sys.exit(1 if failed else 0)
