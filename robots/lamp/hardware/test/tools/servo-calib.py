#!/usr/bin/env python3
# servo-calib.py — the five STS3215 servos' own settings, which a lamp built
# without V1 HAL never had (lamp-220c, 28 Oct): V1 wrote them at every start
# (lerobot configure_motors: Maximum_Acceleration 254, P 16) and once by hand
# (hal.calibrate: each joint's zero and range). Without them the body's stand
# is refused (acceleration read back as 50, the servo's own maximum) and its
# poses don't fit: lamp-4ace's stand drove lamp-220c's elbow into its body.
# Everything here is in the servos' EEPROM and survives a reflash and a reset.
# Torque stays off throughout; nothing is driven.
#
#   servo-calib.py check               read only: exit 0 calibrated, 3 not
#                                      configured, 4 not calibrated
#   servo-calib.py configure           store max acceleration 254 and P 16
#   servo-calib.py sweep SECONDS       a person moves each joint end to end by
#                                      hand, from folded at rest back to it;
#                                      writes the plan, changes nothing
#   servo-calib.py apply               write the plan's offsets (all five) and
#                                      angle limits (yaw, roll, pitch)
#
# The plan matches a V1-calibrated lamp (lamp-3e83): folded at rest, the
# shoulder and elbow read 2047 and the head pitch 2010 (inside the body's
# home range, Limits::FOLDED.pitch_home); yaw and roll have the middle of
# their swept range at 2047. Limits sit MARGIN counts inside the sweep, so
# the servo is never sent into a hard stop. lamp-3e83 stands at 2630,2562
# and lamp-220c, calibrated this way, at 2629,2564.
import argparse, json, os, select, sys, termios, time

NAMES = {1: "base_yaw", 2: "base_pitch", 3: "elbow_pitch", 4: "wrist_roll", 5: "wrist_pitch"}
REST_TARGET = {2: 2047, 3: 2047, 5: 2010}  # the folded rest reads these
MID_TARGET = {1: 2047, 4: 2047}            # the swept range's middle reads these
LIMITED = (1, 4, 5)                         # angle limits, as on lamp-3e83
MARGIN = 30                                 # counts inside the sweep, ~3 degrees
MAX_ACCELERATION, P_COEFFICIENT = 254, 16   # V1's (lerobot configure_motors, hal_follower)
# Control table: EEPROM below 40 and at 85, RAM from 40.
MIN_LIMIT, MAX_LIMIT, P_REG, OFFSET, TORQUE, LOCK, POSITION, MAX_ACC = 9, 11, 21, 31, 40, 55, 56, 85


class Bus:
    def __init__(self, device):
        self.fd = os.open(device, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        t = termios.tcgetattr(self.fd)
        t[0] = 0; t[1] = 0; t[2] = termios.CS8 | termios.CREAD | termios.CLOCAL; t[3] = 0
        baud = getattr(termios, "B1000000", None)  # absent on a Mac, where only the host test runs
        if baud is not None:
            t[4] = t[5] = baud
        termios.tcsetattr(self.fd, termios.TCSANOW, t)

    def xfer(self, sid, ins, params, n):
        body = [sid, len(params) + 2, ins, *params]
        termios.tcflush(self.fd, termios.TCIOFLUSH)
        os.write(self.fd, bytes([0xFF, 0xFF, *body, (~sum(body)) & 0xFF]))
        want, buf, end = 6 + n, b"", time.time() + 0.1
        while len(buf) < want and time.time() < end:
            if select.select([self.fd], [], [], 0.01)[0]:
                buf += os.read(self.fd, 64)
        i = buf.find(b"\xff\xff")
        if i < 0 or len(buf) < i + want or buf[i + 2] != sid:
            return None
        if buf[i + 4]:
            sys.exit(f"servo {sid}: error {buf[i + 4]:#x}")
        return buf[i + 5:i + 5 + n]

    def read(self, sid, addr, n=1):
        b = self.xfer(sid, 0x02, [addr, n], n)
        if b is None:
            return None
        return b[0] if n == 1 else b[0] | b[1] << 8

    def must(self, sid, addr, n=1):
        v = self.read(sid, addr, n)
        if v is None:
            sys.exit(f"servo {sid}: no answer reading {addr}")
        return v

    def write(self, sid, addr, *data):
        self.xfer(sid, 0x03, [addr, *data], 0)

    def eeprom(self, sid, writes):
        """Unlock, write each (addr, bytes), lock again."""
        self.write(sid, LOCK, 0)
        for addr, data in writes:
            self.write(sid, addr, *data)
        time.sleep(0.05)
        self.write(sid, LOCK, 1)
        time.sleep(0.05)


s11 = lambda v: -(v & 0x7FF) if v & 0x800 else v  # the offset's sign is bit 11
enc = lambda v: (0x800 | -v) if v < 0 else v
le = lambda v: [v & 0xFF, v >> 8]
wrap = lambda d: (d + 2048) % 4096 - 2048


def torque_off(bus):
    for sid in NAMES:
        if bus.must(sid, TORQUE) != 0:
            sys.exit(f"servo {sid}: torque is on (is lamp-body running?); refusing")


def state(bus, sid):
    return {"offset": s11(bus.must(sid, OFFSET, 2)), "limits": [bus.must(sid, MIN_LIMIT, 2), bus.must(sid, MAX_LIMIT, 2)],
            "max_acceleration": bus.must(sid, MAX_ACC), "p": bus.must(sid, P_REG), "position": bus.must(sid, POSITION, 2)}


def check(bus):
    st = {sid: state(bus, sid) for sid in NAMES}
    print(f"{'id':<3}{'joint':<13}{'offset':>7}{'limits':>12}{'max acc':>8}{'P':>4}{'reads':>7}")
    for sid, s in st.items():
        print(f"{sid:<3}{NAMES[sid]:<13}{s['offset']:>7}{s['limits'][0]:>6}-{s['limits'][1]:<5}{s['max_acceleration']:>8}{s['p']:>4}{s['position']:>7}")
    configured = all(s["max_acceleration"] == MAX_ACCELERATION and s["p"] == P_COEFFICIENT for s in st.values())
    # A hand calibration gives each joint its own zero and limits the joints
    # that have stops; servos never calibrated share one offset and none.
    calibrated = (len({s["offset"] for s in st.values()}) == len(st)
                  and all(st[sid]["limits"] != [0, 4095] for sid in LIMITED))
    verdict = "PASS" if configured and calibrated else ("not configured" if not configured else "not calibrated")
    print(f"RECORD SERVOS={verdict}")
    print(f"RECORD SERVO_OFFSETS={','.join(str(st[sid]['offset']) for sid in NAMES)}")
    return 0 if verdict == "PASS" else 3 if not configured else 4


def configure(bus):
    torque_off(bus)
    for sid in NAMES:
        before = (bus.must(sid, MAX_ACC), bus.must(sid, P_REG))
        if before != (MAX_ACCELERATION, P_COEFFICIENT):
            bus.eeprom(sid, [(MAX_ACC, [MAX_ACCELERATION]), (P_REG, [P_COEFFICIENT])])
        after = (bus.must(sid, MAX_ACC), bus.must(sid, P_REG))
        print(f"servo {sid}: max acceleration, P {before} -> {after}")
        if after != (MAX_ACCELERATION, P_COEFFICIENT):
            sys.exit(f"servo {sid}: did not keep max acceleration {MAX_ACCELERATION}, P {P_COEFFICIENT}")
    return 0


def sweep(bus, seconds, plan_path):
    torque_off(bus)
    off = {sid: s11(bus.must(sid, OFFSET, 2)) for sid in NAMES}
    # Actual = Present + Offset (mod 4096), unwrapped from the rest reading.
    rest = {sid: (bus.must(sid, POSITION, 2) + off[sid]) % 4096 for sid in NAMES}
    last, lo, hi, at = dict(rest), dict(rest), dict(rest), dict(rest)
    misses, end, shown = 0, time.time() + seconds, 0.0
    print(f"sweep {seconds:.0f} s: move each joint end to end by hand, then fold back to rest", flush=True)
    while time.time() < end:
        for sid in NAMES:
            p = bus.read(sid, POSITION, 2)
            if p is None:
                misses += 1
                continue
            a = (p + off[sid]) % 4096
            at[sid] += wrap(a - last[sid]); last[sid] = a
            lo[sid] = min(lo[sid], at[sid]); hi[sid] = max(hi[sid], at[sid])
        if time.time() - shown > 3:
            shown = time.time()
            print("  " + "  ".join(f"{sid}:{hi[sid] - lo[sid]:>4}" for sid in NAMES) + "  (counts swept)", flush=True)
    plan = {"made": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "misses": misses, "joints": {}}
    for sid, name in NAMES.items():
        if sid in REST_TARGET:
            rest_reads = REST_TARGET[sid]
        else:
            rest_reads = round(MID_TARGET[sid] - ((lo[sid] + hi[sid]) / 2 - rest[sid]))
        new_off = wrap(rest[sid] - rest_reads)
        swept = [rest_reads + lo[sid] - rest[sid], rest_reads + hi[sid] - rest[sid]]
        limits = [swept[0] + MARGIN, swept[1] - MARGIN] if sid in LIMITED else [0, 4095]
        # A limited joint's whole sweep must read inside one turn; an unlimited
        # one (shoulder, elbow) may sweep past it by hand, as on lamp-3e83.
        fits = abs(new_off) <= 2047 and (sid not in LIMITED or (0 <= swept[0] and swept[1] <= 4095 and limits[0] < rest_reads < limits[1]))
        plan["joints"][str(sid)] = {"name": name, "span": hi[sid] - lo[sid], "old_offset": off[sid], "new_offset": new_off,
                                    "rest_reads": rest_reads, "swept": swept, "limits": limits,
                                    "back_at_rest_by": at[sid] - rest[sid], "fits": fits}
    os.makedirs(os.path.dirname(plan_path) or ".", exist_ok=True)
    with open(plan_path, "w") as f:
        json.dump(plan, f, indent=1)
    print(f"\nplan ({misses} missed reads), in {plan_path}:")
    print(f"{'id':<3}{'joint':<13}{'span':>6}{'offset':>15}{'rest reads':>11}{'limits':>14}{'back':>6} fits")
    for sid, j in plan["joints"].items():
        print(f"{sid:<3}{j['name']:<13}{j['span']:>6}{j['old_offset']:>6} -> {j['new_offset']:<6}{j['rest_reads']:>9}"
              f"{j['limits'][0]:>7}-{j['limits'][1]:<6}{j['back_at_rest_by']:>5}  {j['fits']}")
    small = [j["name"] for j in plan["joints"].values() if j["span"] < 400]
    if small:
        print(f"not swept far enough: {', '.join(small)}; sweep again")
        return 1
    return 0 if all(j["fits"] for j in plan["joints"].values()) else 1


def apply(bus, plan_path):
    with open(plan_path) as f:
        plan = json.load(f)
    if any(not j["fits"] or j["span"] < 400 for j in plan["joints"].values()):
        sys.exit("the plan does not fit every joint; sweep again")
    torque_off(bus)
    failed = False
    for s, j in plan["joints"].items():
        sid = int(s)
        bus.eeprom(sid, [(OFFSET, le(enc(j["new_offset"]))), (MIN_LIMIT, le(j["limits"][0]) + le(j["limits"][1]))])
        st = state(bus, sid)
        ok = st["offset"] == j["new_offset"] and st["limits"] == j["limits"]
        failed |= not ok
        print(f"servo {sid} {j['name']}: offset {st['offset']} (want {j['new_offset']}), limits {st['limits'][0]}-{st['limits'][1]}"
              f" (want {j['limits'][0]}-{j['limits'][1]}), reads {st['position']} (folded: {j['rest_reads']}){'' if ok else '  NOT KEPT'}")
    return 1 if failed else 0


def main():
    ap = argparse.ArgumentParser(description="The servos' own settings: check, configure, sweep, apply.")
    ap.add_argument("mode", choices=["check", "configure", "sweep", "apply"])
    ap.add_argument("seconds", nargs="?", type=float, default=120, help="sweep: how long")
    ap.add_argument("--device", default="/dev/ttyACM0")
    ap.add_argument("--plan", default="/var/lib/lamp-test/servo-calib-plan.json")
    a = ap.parse_args()
    bus = Bus(a.device)
    if a.mode == "check":
        return check(bus)
    if a.mode == "configure":
        return configure(bus)
    if a.mode == "sweep":
        return sweep(bus, a.seconds, a.plan)
    return apply(bus, a.plan)


if __name__ == "__main__":
    sys.exit(main())
