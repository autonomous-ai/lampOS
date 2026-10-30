#!/usr/bin/env python3
# ring-check.py — the hardware team's ring check: the self-test's ten colours,
# each to be drawn within 4 s (the hardware lead, #3530, 27 Oct). lamp-selftest
# allows 1 s (DRAWN_WITHIN, crates/selftest/src/ring.rs) and is not
# configurable; on lamp-29e7 (7071) its ring failed three runs in a row with
# 2 to 7 of the colours late. factory-run.sh runs this when the ring is the
# self-test's only failed part, and its verdict decides the ring.
#
# The ring is busy for 10-15 s after lamp-selftest ends: on lamp-220c (28 Oct)
# whichever colours came first were late or never drawn, a different set each
# run, and every colour after them drawn in 0.02-0.04 s; every colour drew at
# once in some run. So one warm-up colour is asked for first, until drawn
# (WARMUP_S at most) and not judged; then the ten, each still within 4 s (the
# hardware lead, 28 Oct). A ring that never draws still fails.
#
# Run on a set-up lamp (lamp-voice owns the ring), as root:
#
#   ring-check.py               ten colours, 4 s each at most
#   ring-check.py --within 2    a stricter bound
#   ring-check.py --warmup 0    no warm-up
#
# Each colour is asked for on `light/test` exactly as lamp-selftest asks
# (type lamp_msgs/Light, level 3, asked again every second: the owner shows a
# test 2 s at most), and is seen on `light/drawn`, which the owner says only
# from its own write receipts. The bus is spoken directly (docs/bus.md): one
# datagram, a header line and the payload, to every subscriber socket in
# light/test/subs, the same bytes put in light/test/latest, and a pubs/ entry
# while it publishes; the image has no lamp-bus binary.
#
# Prints RECORD lines (RING, RING_DRAW_S, RING_WARMUP_S) for factory.env.
# Exit 0 every colour drawn in time, 1 some not, 3 none drawn (the privacy
# switch closed, or a turn owns the ring: not_checked), 2 could not run.
# Python 3 standard library only.

import argparse, json, os, secrets, socket, sys, time

COLOURS = ["yellow", "red", "green", "blue", "cyan", "purple", "orange", "pink", "white", "warm"]
LIGHT_TYPE = "lamp_msgs/Light"
LEVEL = 3                  # lamp-selftest's ring.rs LEVEL
ASK_EVERY_S = 1.0          # the owner shows a test 2 s at most (TEST_HOLDS_US)
HOLD_S = 0.3               # ring.rs HOLD: each drawn colour kept a moment
WARMUP_S = 15.0            # the warm-up colour asked for until drawn, not judged
WARMUP_COLOUR = "warm"     # the last of the ten, so the first judged differs from it
SOURCE = "hwtest"


class Bus:
    def __init__(self, root):
        self.root = root
        self.boot = secrets.token_hex(8)
        self.seq = 0
        self.mark = os.path.join(root, "light/test/pubs", f"{SOURCE}.{os.getpid()}.{self.boot}")

    def publish(self, topic, value):
        self.seq += 1
        header = {"topic": topic, "kind": "latest", "enc": "json", "type": LIGHT_TYPE, "source": SOURCE,
                  "boot": self.boot, "seq": self.seq, "at_us": time.monotonic_ns() // 1000,
                  "at_unix_ms": int(time.time() * 1000)}
        data = (json.dumps(header, separators=(",", ":")) + "\n" + json.dumps(value, separators=(",", ":"))).encode()
        tdir = os.path.join(self.root, topic)
        os.makedirs(os.path.join(tdir, "pubs"), exist_ok=True)
        open(self.mark, "a").close()
        os.utime(self.mark)
        sent = 0
        subs = os.path.join(tdir, "subs")
        for name in (os.listdir(subs) if os.path.isdir(subs) else []):
            if not name.endswith(".sock"):
                continue
            s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            try:
                s.sendto(data, os.path.join(subs, name))
                sent += 1
            except OSError:
                pass
            finally:
                s.close()
        tmp = os.path.join(tdir, f".latest.{SOURCE}.{os.getpid()}")
        with open(tmp, "wb") as f:
            f.write(data)
        os.chmod(tmp, 0o660)
        os.replace(tmp, os.path.join(tdir, "latest"))
        return sent

    def latest(self, topic):
        try:
            raw = open(os.path.join(self.root, topic, "latest"), "rb").read()
        except OSError:
            return None
        line, _, payload = raw.partition(b"\n")
        try:
            return json.loads(line), json.loads(payload) if payload.strip() else None
        except ValueError:
            return None

    def close(self):
        try:
            os.remove(self.mark)
        except OSError:
            pass


def main():
    ap = argparse.ArgumentParser(description="The ring's ten colours, each drawn within 4 s (see the header).")
    ap.add_argument("--within", type=float, default=4.0, help="seconds a colour may take to be drawn (4)")
    ap.add_argument("--warmup", type=float, default=WARMUP_S, help="seconds to wait for the warm-up colour (15; 0: none)")
    ap.add_argument("--root", default=os.environ.get("LAMP_BUS_ROOT", "/run/lampos-bus"))
    args = ap.parse_args()
    if not os.path.isdir(os.path.join(args.root, "light")):
        print(f"ring-check: no light topics under {args.root}: is the lamp set up and lamp-voice running?", file=sys.stderr)
        return 2
    bus = Bus(args.root)
    took, missed = {}, []

    def ask(colour, within):
        """Seconds until colour is drawn, asked again every second; None past within."""
        light = {"on": True, "colour": colour, "level": LEVEL}
        asked_ms = int(time.time() * 1000)
        start = last_ask = time.monotonic()
        if not bus.publish("light/test", light):
            raise LookupError("nobody listens on light/test (lamp-voice down?)")
        while time.monotonic() - start < within:
            seen = bus.latest("light/drawn")
            if seen and seen[0].get("at_unix_ms", 0) >= asked_ms and seen[1] == light:
                return time.monotonic() - start
            if time.monotonic() - last_ask >= ASK_EVERY_S:
                bus.publish("light/test", light)
                last_ask = time.monotonic()
            time.sleep(0.02)
        return None

    try:
        if args.warmup > 0:
            warm = ask(WARMUP_COLOUR, args.warmup)
            print(f"  warm-up ({WARMUP_COLOUR}) " + (f"drawn in {warm:.2f} s" if warm is not None else f"not drawn in {args.warmup:g} s"))
            print(f"RECORD RING_WARMUP_S={f'{warm:.2f}' if warm is not None else 'none'}")
            if warm is not None:
                time.sleep(HOLD_S)
        for colour in COLOURS:
            t = ask(colour, args.within)
            if t is not None:
                took[colour] = t
                print(f"  {colour:<7} drawn in {t:.2f} s")
                time.sleep(HOLD_S)
            else:
                missed.append(colour)
                print(f"  {colour:<7} NOT drawn within {args.within:g} s")
    except LookupError as e:
        print(f"ring-check: {e}", file=sys.stderr)
        return 2
    finally:
        bus.close()
    times = ",".join(f"{c}:{took[c]:.2f}" for c in COLOURS if c in took)
    print(f"RECORD RING_DRAW_S={times or 'none'}")
    if not took:
        print("RECORD RING=NOT_CHECKED: no colour drawn (privacy switch closed, or a turn owns the ring)")
        return 3
    if missed:
        print(f"RECORD RING=FAIL: not drawn within {args.within:g} s: {', '.join(missed)}")
        return 1
    print(f"RECORD RING=PASS: ten colours within {args.within:g} s (slowest {max(took.values()):.2f} s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
