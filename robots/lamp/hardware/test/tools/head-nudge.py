#!/usr/bin/env python3
# head-nudge.py — bring one servo to a target slowly, under a torque cap, and
# let it go. Bench use only. Writes RAM registers only (torque limit 48, goal
# 42, torque enable 40); never EEPROM. Always ends with torque off and the
# torque limit put back.
#
#   head-nudge.py --dry-run                 read and print the plan, write nothing
#   head-nudge.py --id 5 --target 2030      head pitch down to 2030
import argparse, os, select, sys, termios, time


def next_goal(goal, target, step):
    """One step from goal toward target, never past it."""
    if goal < target:
        return min(goal + step, target)
    return max(goal - step, target)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--device", default="/dev/ttyACM0")
    ap.add_argument("--id", type=int, default=5)
    ap.add_argument("--target", type=int, default=2030)
    ap.add_argument("--step", type=int, default=3, help="counts a step")
    ap.add_argument("--period", type=float, default=0.08, help="seconds a step")
    ap.add_argument("--cap", type=int, default=500, help="torque limit, 0-1000 (500 = 50%%)")
    ap.add_argument("--stop-load", type=int, default=450, help="stop at this load, 0-1000")
    ap.add_argument("--max-lag", type=int, default=40, help="stop when the joint lags the goal this far")
    ap.add_argument("--max-temp", type=int, default=55)
    ap.add_argument("--timeout", type=float, default=30)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    fd = os.open(a.device, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    t = termios.tcgetattr(fd); t[0] = 0; t[1] = 0; t[2] = termios.CS8 | termios.CREAD | termios.CLOCAL; t[3] = 0
    t[4] = t[5] = termios.B1000000; termios.tcsetattr(fd, termios.TCSANOW, t)

    def xfer(ins, params, n):
        body = [a.id, len(params) + 2, ins, *params]
        termios.tcflush(fd, termios.TCIOFLUSH)
        os.write(fd, bytes([0xFF, 0xFF, *body, (~sum(body)) & 0xFF]))
        buf = b""; end = time.time() + 0.03
        while time.time() < end and len(buf) < 6 + n:
            r, _, _ = select.select([fd], [], [], 0.005)
            if r:
                try: buf += os.read(fd, 64)
                except BlockingIOError: pass
        if len(buf) < 6 + n or buf[2] != a.id:
            raise IOError(f"no reply from servo {a.id} (ins {ins:#x})")
        return buf[5:5 + n]
    def read(addr, n): return xfer(0x02, [addr, n], n)
    def write(addr, data): xfer(0x03, [addr, *data], 0)
    def u16(b): return b[0] | b[1] << 8
    def w16(v): return [v & 0xFF, (v >> 8) & 0xFF]
    def state():
        pos = u16(read(56, 2)); ld = u16(read(60, 2)); temp = read(63, 1)[0]
        return pos, ld & 0x3FF, ("-" if ld & 0x400 else "+"), temp

    pos, load, sign, temp = state()
    torque = read(40, 1)[0]; cap0 = u16(read(48, 2)); lo, hi = u16(read(9, 2)), u16(read(11, 2))
    print(f"servo {a.id}: pos {pos}, load {load/10:.0f}%{sign}, temp {temp}C, torque {'ON' if torque else 'off'}, "
          f"torque limit {cap0}, EEPROM limits {lo}-{hi}")
    print(f"plan: cap torque at {a.cap/10:.0f}%, {pos} -> {a.target} in {a.step}-count steps every {a.period}s "
          f"(~{abs(pos-a.target)/a.step*a.period:.1f}s), stop at load {a.stop_load/10:.0f}% / lag {a.max_lag} / "
          f"{a.max_temp}C / {a.timeout:g}s, then torque off, limit back to {cap0}")
    if torque:
        print("ABORT: torque already on: something else holds this servo"); return 2
    if not lo <= a.target <= hi:
        print(f"ABORT: target {a.target} outside the servo's limits {lo}-{hi}"); return 2
    if a.dry_run:
        print("dry run: nothing written"); return 0

    why = "?"; start = time.time(); goal = pos; peak = 0; still_since = None; last = pos
    try:
        write(48, w16(a.cap)); write(42, w16(pos)); write(40, [1])
        while True:
            goal = next_goal(goal, a.target, a.step)
            write(42, w16(goal))
            time.sleep(a.period)
            pos, load, sign, temp = state(); peak = max(peak, load)
            print(f"  t={time.time()-start:4.1f}s goal {goal} pos {pos} load {load/10:4.1f}%{sign} temp {temp}C")
            if load >= a.stop_load: why = f"load {load/10:.0f}% reached the stop ({a.stop_load/10:.0f}%): it met something"; break
            if abs(goal - pos) > a.max_lag: why = f"lag {abs(goal-pos)} counts: it is not following"; break
            if temp >= a.max_temp: why = f"temperature {temp}C"; break
            if time.time() - start > a.timeout: why = "timeout"; break
            if goal == a.target and abs(pos - a.target) <= 4: why = f"reached {a.target}"; break
            # A servo under a small push holds a few counts short of its goal:
            # once the goal is in and the joint has not moved for 1 s, stop.
            if goal == a.target and abs(pos - last) <= 1:
                still_since = still_since or time.time()
                if time.time() - still_since >= 1.0:
                    why = f"settled at {pos}, {abs(pos - a.target)} short of {a.target}"; break
            else:
                still_since = None
            last = pos
    except Exception as e:
        why = f"error: {e}"
    finally:
        for attempt in range(5):
            try:
                write(40, [0]); write(48, w16(cap0)); break
            except Exception:
                time.sleep(0.02)
    print(f"stopped: {why}; peak load {peak/10:.0f}%; torque off, limit back to {cap0}")
    time.sleep(1.5)
    pos, load, sign, temp = state(); torque = read(40, 1)[0]
    print(f"after letting go: pos {pos}, torque {'ON' if torque else 'off'}, temp {temp}C")
    os.close(fd)
    return 0

if __name__ == "__main__":
    sys.exit(main())
