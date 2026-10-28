#!/usr/bin/env python3
# Host test for servo-calib.py on stand-in servos: five STS3215 control tables
# behind a pty, answering reads and writes as the servos do (EEPROM writes
# kept only while unlocked, Present = Actual - Offset, the offset's sign in
# bit 11). No lamp. Checks that a lamp as lamp-220c came (offsets all 85, no
# limits, max acceleration 50) is "not configured", that configure stores
# 254 and 16 and leaves it "not calibrated", that a sweep by a stand-in hand
# plans the folded rest at 2047/2047/2010 with yaw and roll centred and
# limits 30 inside, that apply writes it and check then passes, that a joint
# barely moved refuses the plan, and that torque on refuses every write.
#
#   python3 servo-calib-test.py
import json, os, subprocess, sys, tempfile, threading, time, tty

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "servo-calib.py")


class Servos:
    """Five stand-in servos; actual[sid] is where each joint really is."""

    def __init__(self, actual, offset=85):
        self.mem = {}
        for sid in range(1, 6):
            m = bytearray(90)
            m[9:11] = (0).to_bytes(2, "little"); m[11:13] = (4095).to_bytes(2, "little")
            m[21] = 32; m[31:33] = offset.to_bytes(2, "little"); m[55] = 1; m[85] = 50
            self.mem[sid] = m
        self.actual = dict(actual)
        self.lock = threading.Lock()

    def present(self, sid):
        o = self.mem[sid][31] | self.mem[sid][32] << 8
        o = -(o & 0x7FF) if o & 0x800 else o
        return (self.actual[sid] - o) % 4096

    def serve(self, fd, stop):
        buf = b""
        while not stop.is_set():
            try:
                buf += os.read(fd, 256)
            except BlockingIOError:
                time.sleep(0.001); continue
            except OSError:
                return
            while True:
                i = buf.find(b"\xff\xff")
                if i < 0 or len(buf) < i + 4 or len(buf) < i + 4 + buf[i + 3]:
                    break
                n = buf[i + 3]; pkt = buf[i + 2:i + 4 + n]; buf = buf[i + 4 + n:]
                sid, ins, params = pkt[0], pkt[2], pkt[3:-1]
                if sid not in self.mem:
                    continue
                with self.lock:
                    m = self.mem[sid]
                    if ins == 0x02:
                        addr, k = params[0], params[1]
                        if addr == 56:
                            m[56:58] = self.present(sid).to_bytes(2, "little")
                        data = bytes(m[addr:addr + k])
                    else:
                        addr, data = params[0], params[1:]
                        eeprom = addr < 40 or addr == 85
                        if not eeprom or m[55] == 0:
                            m[addr:addr + len(data)] = data
                        data = b""
                body = [sid, len(data) + 2, 0, *data]
                os.write(fd, bytes([0xFF, 0xFF, *body, (~sum(body)) & 0xFF]))


def start(servos):
    master, slave = os.openpty()
    tty.setraw(slave)
    os.set_blocking(master, False)
    stop = threading.Event()
    t = threading.Thread(target=servos.serve, args=(master, stop), daemon=True)
    t.start()
    return os.ttyname(slave), lambda: (stop.set(), os.close(master), os.close(slave))


def tool(device, *args, plan):
    return subprocess.run([sys.executable, TOOL, *args, "--device", device, "--plan", plan],
                          capture_output=True, text=True, timeout=60)


# lamp-220c as it came: folded actuals giving 2418/2813 at offset 85, yaw 3560, roll 1179, pitch 160.
FOLDED = {1: 3645, 2: 2503, 3: 2898, 4: 1264, 5: 245}
failures = []


def expect(name, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + name + (f": {detail}" if detail and not ok else ""))
    if not ok:
        failures.append(name)


def hand(servos, moves, seconds):
    """A stand-in hand: each joint swept (lo, hi) from rest and back, one after another."""
    time.sleep(0.5)  # the lamp is folded and still while the tool reads its rest
    step = seconds / len(moves)
    for sid, (lo, hi) in moves.items():
        rest = FOLDED[sid]
        for target in (rest + lo, rest + hi, rest):
            begin = servos.actual[sid]
            for k in range(1, 21):
                with servos.lock:
                    servos.actual[sid] = (begin + (target - begin) * k // 20)
                time.sleep(step / 60)


with tempfile.TemporaryDirectory() as tmp:
    plan = os.path.join(tmp, "plan.json")
    servos = Servos(FOLDED)
    dev, close = start(servos)
    try:
        p = tool(dev, "check", plan=plan)
        expect("as it came: not configured (exit 3)", p.returncode == 3 and "RECORD SERVOS=not configured" in p.stdout, p.stdout + p.stderr)

        p = tool(dev, "configure", plan=plan)
        expect("configure stores 254 and 16", p.returncode == 0 and all(servos.mem[s][85] == 254 and servos.mem[s][21] == 16 for s in servos.mem), p.stdout + p.stderr)
        expect("configure locks the EEPROM again", all(servos.mem[s][55] == 1 for s in servos.mem))

        p = tool(dev, "check", plan=plan)
        expect("configured, not calibrated (exit 4)", p.returncode == 4 and "RECORD SERVOS=not calibrated" in p.stdout, p.stdout + p.stderr)

        # Ranges from lamp-220c's sweep: yaw and roll either side of rest, the arm only up, pitch both ways.
        moves = {1: (-1300, 1133), 2: (0, 2070), 3: (0, 2893), 4: (-1000, 1191), 5: (-689, 945)}
        h = threading.Thread(target=hand, args=(servos, moves, 4.0)); h.start()
        p = tool(dev, "sweep", "5", plan=plan); h.join()
        expect("sweep exits 0", p.returncode == 0, p.stdout + p.stderr)
        j = json.load(open(plan))["joints"] if os.path.exists(plan) else {}
        rest = {s: j[s]["rest_reads"] for s in j}
        expect("folded rest: shoulder and elbow 2047, pitch 2010", rest.get("2") == 2047 and rest.get("3") == 2047 and rest.get("5") == 2010, rest)
        mids = {s: sum(j[s]["swept"]) / 2 for s in ("1", "4")} if j else {}
        expect("yaw and roll centred at 2047", all(abs(m - 2047) <= 1 for m in mids.values()) and len(mids) == 2, mids)
        expect("limits 30 inside the sweep on yaw, roll and pitch",
               j and all(j[s]["limits"] == [j[s]["swept"][0] + 30, j[s]["swept"][1] - 30] for s in ("1", "4", "5")), j)
        expect("shoulder and elbow keep 0-4095", j and all(j[s]["limits"] == [0, 4095] for s in ("2", "3")), j)

        p = tool(dev, "apply", plan=plan)
        expect("apply exits 0", p.returncode == 0, p.stdout + p.stderr)
        expect("folded, the servos now read the plan", all(abs(servos.present(int(s)) - rest[s]) <= 1 for s in rest),
               ({s: servos.present(int(s)) for s in rest}, rest))
        p = tool(dev, "check", plan=plan)
        expect("calibrated: check passes (exit 0)", p.returncode == 0 and "RECORD SERVOS=PASS" in p.stdout, p.stdout + p.stderr)

        # Calibrated, but not as the fleet (lamp-9adf: folded shoulder and
        # elbow at 3151,3765): check refuses it.
        kept = {s: bytes(servos.mem[s][31:33]) for s in (2, 3)}
        for sid, reads in ((2, 3151), (3, 3765)):
            o = servos.actual[sid] - reads
            servos.mem[sid][31:33] = ((0x800 | -o) if o < 0 else o).to_bytes(2, "little")
        p = tool(dev, "check", plan=plan)
        expect("calibrated off the fleet (folded 3151,3765): not calibrated (exit 4)", p.returncode == 4
               and "not as the fleet" in p.stdout and "RECORD SERVOS=not calibrated" in p.stdout, p.stdout + p.stderr)
        for sid, b in kept.items():
            servos.mem[sid][31:33] = b

        # A joint barely moved: the plan is refused.
        h = threading.Thread(target=hand, args=(servos, {1: (-50, 50)}, 1.0)); h.start()
        p = tool(dev, "sweep", "2", plan=plan); h.join()
        expect("a joint barely moved: sweep exits 1", p.returncode == 1 and "not swept far enough" in p.stdout, p.stdout + p.stderr)
        before = bytes(servos.mem[1])
        p = tool(dev, "apply", plan=plan)
        expect("and apply refuses that plan", p.returncode != 0 and bytes(servos.mem[1]) == before, p.stdout + p.stderr)

        # Torque on: every write refused.
        servos.mem[3][40] = 1
        before = {s: bytes(m) for s, m in servos.mem.items()}
        for mode in ("configure", "sweep"):
            p = tool(dev, mode, *(["1"] if mode == "sweep" else []), plan=plan)
            expect(f"torque on: {mode} refused", p.returncode != 0 and "torque is on" in p.stderr, p.stdout + p.stderr)
        expect("torque on: nothing written", {s: bytes(m) for s, m in servos.mem.items()} == before)
    finally:
        close()

print(f"\n{'all passed' if not failures else f'{len(failures)} failed'}")
sys.exit(1 if failures else 0)
