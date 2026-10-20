#!/usr/bin/env python3
# routine-tools-test.py — local tests for ring-pixels.py and privacy-switch.py
# against a stand-in lamp-bus. No lamp, no bus, no sound: the stand-in is a
# small program that answers `lamp-bus latest|pub|tap` the way the real one
# prints, from a scenario in a temporary folder (the switch's position, which
# parts misbehave, what the ring draws).
#
#   python3 routine-tools-test.py            all cases (~2 min)
#   python3 routine-tools-test.py -k ring    only the ring's
#
# It shows the scripts' logic against the bus's shapes as lampOS declares
# them (header line + payload, controls/state, voice/privacy, ears/level,
# diagnostics, light/test, light/drawn). It does not show that a V2 lamp
# says them so: that needs a run on a lamp with a person.
import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
RING = HERE / "ring-pixels.py"
PRIVACY = HERE / "privacy-switch.py"

FAKE_BUS = r'''#!/usr/bin/env python3
import json, os, sys, time
D = os.environ["FAKE_BUS_DIR"]
def rd(name, default=None):
    try:
        with open(os.path.join(D, name)) as f:
            return json.load(f)
    except (OSError, ValueError):
        return default
def ms(): return int(time.time() * 1000)
def line(topic, kind, payload, at=None, source="fake"):
    h = {"topic": topic, "kind": kind, "enc": "json", "type": "x", "source": source,
         "boot": "0123456789abcdef", "seq": 1, "at_us": 0, "at_unix_ms": at or ms()}
    body = payload if isinstance(payload, str) else json.dumps(payload, separators=(",", ":"))
    return json.dumps(h, separators=(",", ":")) + " " + body
args = sys.argv[1:]
if args[:1] == ["--root"]:
    args = args[2:]
cfg = rd("scenario.json", {})
cmd = args[0]
if cmd == "pub":
    with open(os.path.join(D, "pubs.jsonl"), "a") as f:
        f.write(json.dumps({"at": ms(), "topic": args[1], "value": json.loads(args[3]),
                            "argv": args}) + "\n")
    print("delivered 1 dropped 0 removed 0"); sys.exit(0)
if cmd == "latest":
    topic = args[1]
    person = cfg.get("person", {"on": True, "colour": "warm", "level": 3})
    if topic == "light/state":
        if cfg.get("no_state"): sys.exit(1)
        print(line(topic, "latest", person)); sys.exit(0)
    if topic == "light/command":
        at = cfg.get("command_at")
        if at is None:
            print(line(topic, "latest", person, at=ms() - 600000)); sys.exit(0)
        if ms() >= at:
            print(line(topic, "latest", person, at=at)); sys.exit(0)
        print(line(topic, "latest", person, at=ms() - 600000)); sys.exit(0)
    if topic == "light/drawn":
        pubs = []
        try:
            with open(os.path.join(D, "pubs.jsonl")) as f:
                pubs = [json.loads(l) for l in f]
        except OSError:
            pass
        last = pubs[-1] if pubs else None
        missing = set(cfg.get("not_drawn", []))
        if last and ms() - last["at"] < 2000 and last["value"]["colour"] not in missing \
                and not cfg.get("draw_nothing"):
            print(line(topic, "latest", last["value"], at=last["at"] + 30)); sys.exit(0)
        print(line(topic, "latest", person)); sys.exit(0)
    sys.exit(1)
if cmd == "tap":
    topic = args[1]
    secs = float(args[args.index("--seconds") + 1]) if "--seconds" in args else 5
    end = time.time() + secs
    period = {"controls/state": 0.02, "voice/privacy": 0.05, "mic2/audio": 0.1,
              "ears/level": 0.1, "diagnostics": 0.25}[topic]
    nxt = 0
    while time.time() < end:
        sw = rd("switch.json", {"pos": "open", "at": 0})
        closed = sw["pos"] == "closed"
        since = time.time() - sw["at"]
        out = None
        if topic == "controls/state":
            if cfg.get("no_route"):
                out = line(topic, "latest", {"ready": True})
            else:
                out = line(topic, "latest", {"ready": True, "privacy": {"muted": closed}})
        elif topic == "voice/privacy":
            permitted = (not closed) and since > 0.06
            if closed and cfg.get("mic2_stays"):
                permitted = True
            out = line(topic, "latest", {"permitted": permitted, "physical_muted": closed})
        elif topic == "mic2/audio":
            if cfg.get("mic2_audio") and not closed and since > 0.1:
                out = line(topic, "event", "<3200 bytes>")
        elif topic == "ears/level":
            if (not closed and since > cfg.get("mic1_back_s", 0.2)) or (closed and cfg.get("mic1_stays")):
                out = line(topic, "latest", {"level_db": -50.0, "floor_db": -60.0, "own_voice": False})
        elif topic == "diagnostics":
            nxt += 1
            if nxt % 2:
                out = line(topic, "event", {"level": "ok", "name": "voice", "message": "", "values": []})
            else:
                if closed:
                    cam = "closed" if since > 0.8 else "open"
                else:
                    cam = "open" if since > cfg.get("camera_back_s", 0.5) else "closed"
                if cfg.get("no_eyes"):
                    cam = None
                if cam:
                    out = line(topic, "event", {"level": "ok", "name": "eyes", "message": "",
                                                "values": [{"key": "camera", "value": cam}]})
        if out:
            print(out, flush=True)
        time.sleep(period)
    sys.exit(0)
sys.exit(2)
'''


class Lamp:
    """A stand-in lamp: the fake lamp-bus and its scenario folder."""

    def __init__(self, scenario):
        self.dir = Path(tempfile.mkdtemp(prefix="fakebus-"))
        self.bus = self.dir / "lamp-bus"
        self.bus.write_text(FAKE_BUS)
        self.bus.chmod(0o755)
        (self.dir / "scenario.json").write_text(json.dumps(scenario))
        self.slide("open", at=0)

    def slide(self, pos, at=None):
        tmp = self.dir / "switch.tmp"
        tmp.write_text(json.dumps({"pos": pos, "at": time.time() if at is None else at}))
        tmp.replace(self.dir / "switch.json")

    def run(self, script, argv, feed=None, actions=()):
        """Run a script; `feed` lines are written to its stdin at times, and
        `actions` (seconds, callable) run on the side."""
        env = dict(os.environ, LAMP_BUS_BIN=str(self.bus), FAKE_BUS_DIR=str(self.dir))
        proc = subprocess.Popen([sys.executable, str(script)] + argv + ["--evidence", str(self.dir / "ev.jsonl")],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, env=env)
        t0 = time.monotonic()

        def drive():
            for at, what in sorted(list(actions) + [(a, l) for a, l in (feed or [])], key=lambda x: x[0]):
                time.sleep(max(0, at - (time.monotonic() - t0)))
                if callable(what):
                    what()
                else:
                    try:
                        proc.stdin.write(what + "\n")
                        proc.stdin.flush()
                    except (BrokenPipeError, ValueError):
                        return
        threading.Thread(target=drive, daemon=True).start()
        # Not communicate(): it would close stdin before the person "types".
        streams = {}
        readers = [threading.Thread(target=lambda n=n, f=f: streams.__setitem__(n, f.read()), daemon=True)
                   for n, f in (("out", proc.stdout), ("err", proc.stderr))]
        for r in readers:
            r.start()
        proc.wait(timeout=180)
        for r in readers:
            r.join(timeout=5)
        out, err = streams.get("out", ""), streams.get("err", "")
        for f in (proc.stdin, proc.stdout, proc.stderr):
            try:
                f.close()
            except OSError:
                pass  # the script ended before the person's last line
        lines = [l for l in out.splitlines() if l.strip()]
        self.stderr = err
        assert len(lines) == 1, (out, err)
        result = json.loads(lines[0])
        assert set(result) == {"check", "verdict", "reason", "at"}, result
        assert result["at"].endswith("Z") and len(result["at"]) == 20, result
        return proc.returncode, result

    def pubs(self):
        p = self.dir / "pubs.jsonl"
        return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []


class RingPixels(unittest.TestCase):
    ARGS = ["--hold", "1.5", "--answer-timeout", "3"]

    def test_ring_pass_asks_each_colour_on_light_test_and_never_writes_light_command(self):
        lamp = Lamp({})
        code, r = lamp.run(RING, self.ARGS, feed=[(9, "p")])
        self.assertEqual((code, r["verdict"], r["check"]), (0, "pass", "ring_pixels"), lamp.stderr)
        topics = {p["topic"] for p in lamp.pubs()}
        self.assertEqual(topics, {"light/test"})
        colours = [p["value"]["colour"] for p in lamp.pubs()]
        self.assertEqual(sorted(set(colours)), sorted(["white", "red", "green", "blue"]))
        # Held past the owner's 2 s by asking again: more asks than colours.
        self.assertGreater(len(colours), 4)
        self.assertTrue(all(p["argv"][-2:] == ["--type", "lamp_msgs/Light"] for p in lamp.pubs()))
        self.assertTrue(all(p["value"]["level"] == 5 and p["value"]["on"] for p in lamp.pubs()))

    def test_ring_fail_carries_the_pixel_place(self):
        lamp = Lamp({})
        code, r = lamp.run(RING, self.ARGS, feed=[(9, "f"), (9.5, "pixel 12 dark in red")])
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("pixel 12 dark in red", r["reason"])

    def test_ring_unanswered_is_not_checked(self):
        lamp = Lamp({})
        code, r = lamp.run(RING, self.ARGS)
        self.assertEqual((code, r["verdict"]), (3, "not_checked"), lamp.stderr)
        self.assertIn("did not answer", r["reason"])

    def test_ring_nothing_drawn_is_not_checked_and_asks_nobody(self):
        lamp = Lamp({"draw_nothing": True})
        code, r = lamp.run(RING, self.ARGS, feed=[(1, "p")])
        self.assertEqual((code, r["verdict"]), (3, "not_checked"), lamp.stderr)
        self.assertIn("privacy switch", r["reason"])

    def test_ring_a_colour_not_drawn_fails_naming_it(self):
        lamp = Lamp({"not_drawn": ["green"]})
        code, r = lamp.run(RING, self.ARGS, feed=[(9, "p")])
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("green", r["reason"])

    def test_ring_a_persons_light_request_ends_it(self):
        lamp = Lamp({"command_at": int(time.time() * 1000) + 2500})
        code, r = lamp.run(RING, self.ARGS, feed=[(9, "p")])
        self.assertEqual((code, r["verdict"]), (3, "not_checked"), lamp.stderr)
        self.assertIn("light/command", r["reason"])

    def test_ring_no_owner_is_not_checked(self):
        lamp = Lamp({"no_state": True})
        code, r = lamp.run(RING, self.ARGS)
        self.assertEqual((code, r["verdict"]), (3, "not_checked"))
        self.assertEqual(lamp.pubs(), [])

    def test_ring_dry_run_publishes_nothing(self):
        lamp = Lamp({})
        code, r = lamp.run(RING, self.ARGS + ["--dry-run"])
        self.assertEqual((code, r["verdict"]), (3, "not_checked"))
        self.assertEqual(lamp.pubs(), [])


class PrivacySwitch(unittest.TestCase):
    ARGS = ["--within", "2.5", "--answer-timeout", "6"]

    def closing_then_opening(self, lamp, close_at=4.0, open_at=10.0):
        return [(close_at, lambda: lamp.slide("closed")), (open_at, lambda: lamp.slide("open"))]

    def test_privacy_pass(self):
        lamp = Lamp({})
        code, r = lamp.run(PRIVACY, self.ARGS, actions=self.closing_then_opening(lamp))
        self.assertEqual((code, r["verdict"], r["check"]), (0, "pass", "privacy_switch"), lamp.stderr + r["reason"])
        ev = json.loads((lamp.dir / "ev.jsonl").read_text().splitlines()[-1])
        for phase in ("before", "closed", "after"):
            self.assertIn(phase, ev)
        self.assertEqual(ev["before"]["mic1"], "open")

    def test_privacy_pass_with_mic2_audio_on_the_bus(self):
        lamp = Lamp({"mic2_audio": True})
        code, r = lamp.run(PRIVACY, self.ARGS, actions=self.closing_then_opening(lamp))
        self.assertEqual((code, r["verdict"]), (0, "pass"), lamp.stderr)

    def test_privacy_a_room_mic_that_keeps_capturing_fails(self):
        lamp = Lamp({"mic1_stays": True})
        code, r = lamp.run(PRIVACY, self.ARGS, actions=self.closing_then_opening(lamp))
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("still on", r["reason"])
        self.assertIn("mic1", r["reason"])

    def test_privacy_a_voice_lease_that_stays_fails(self):
        lamp = Lamp({"mic2_stays": True})
        code, r = lamp.run(PRIVACY, self.ARGS, actions=self.closing_then_opening(lamp))
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("mic2", r["reason"])

    def test_privacy_a_camera_late_back_fails(self):
        lamp = Lamp({"camera_back_s": 4.0})
        code, r = lamp.run(PRIVACY, self.ARGS, actions=self.closing_then_opening(lamp))
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("not back", r["reason"])
        self.assertIn("camera", r["reason"])

    def test_privacy_nobody_closes_it_is_not_checked(self):
        lamp = Lamp({})
        code, r = lamp.run(PRIVACY, self.ARGS)
        self.assertEqual((code, r["verdict"]), (3, "not_checked"), lamp.stderr)
        self.assertIn("did not close", r["reason"])

    def test_privacy_enter_with_the_switch_unmoved_fails(self):
        lamp = Lamp({})
        code, r = lamp.run(PRIVACY, self.ARGS, feed=[(4, "")])
        self.assertEqual((code, r["verdict"]), (1, "fail"), lamp.stderr)
        self.assertIn("still reads it open", r["reason"])

    def test_privacy_without_the_route_is_not_checked(self):
        lamp = Lamp({"no_route": True})
        code, r = lamp.run(PRIVACY, self.ARGS)
        self.assertEqual((code, r["verdict"]), (3, "not_checked"))
        self.assertIn("LAMP_PRIVACY_CONTROLS", r["reason"])

    def test_privacy_without_eyes_is_not_checked(self):
        lamp = Lamp({"no_eyes": True})
        code, r = lamp.run(PRIVACY, self.ARGS)
        self.assertEqual((code, r["verdict"]), (3, "not_checked"))
        self.assertIn("lamp-eyes", r["reason"])


if __name__ == "__main__":
    unittest.main()
