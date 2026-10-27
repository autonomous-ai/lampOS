#!/usr/bin/env python3
# Host test for ring-check.py on a stand-in bus: a stand-in ring owner binds a
# datagram socket in light/test/subs, as lamp-voice subscribes, and after a
# chosen delay says the colour it was asked for on light/drawn. No lamp, no
# ring. Checks that a colour drawn after 2.5 s passes at the 4 s bound (and
# fails at lamp-selftest's 1 s), that a colour never drawn fails and names
# itself, that none drawn is not_checked, that the request is a valid bus
# message (header type lamp_msgs/Light, level 3), and that the pubs/ entry is
# removed after.
#
#   python3 ring-check-test.py
import json, os, socket, subprocess, sys, tempfile, threading, time

HERE = os.path.dirname(os.path.abspath(__file__))
RUNNER = os.path.join(HERE, "ring-check.py")


def owner(root, delays, seen, stop):
    """Stand-in lamp-voice: draw what light/test asks after delays[colour] (None: never)."""
    subs = os.path.join(root, "light/test/subs")
    os.makedirs(subs, exist_ok=True)
    os.makedirs(os.path.join(root, "light/drawn"), exist_ok=True)
    path = os.path.join(subs, "voice.1.1.sock")
    s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    s.bind(path)
    s.settimeout(0.1)
    first = {}
    while not stop.is_set():
        try:
            data = s.recv(65536)
            header, _, payload = data.partition(b"\n")
            h, light = json.loads(header), json.loads(payload)
            seen.append((h, light))
            first.setdefault(light["colour"], time.monotonic())
        except socket.timeout:
            pass
        for colour, t in list(first.items()):
            d = delays.get(colour)
            if d is not None and time.monotonic() - t >= d:
                light = {"on": True, "colour": colour, "level": 3}
                hdr = {"topic": "light/drawn", "kind": "latest", "enc": "json", "type": "lamp_msgs/Light",
                       "source": "voice", "boot": "0" * 16, "seq": 1, "at_us": 1,
                       "at_unix_ms": int(time.time() * 1000)}
                with open(os.path.join(root, "light/drawn/latest"), "w") as f:
                    f.write(json.dumps(hdr) + "\n" + json.dumps(light))
                del first[colour]
    s.close()


def run(delays, within=None):
    with tempfile.TemporaryDirectory(dir="/tmp") as root:  # a socket path is at most 104 bytes on macOS
        seen, stop = [], threading.Event()
        t = threading.Thread(target=owner, args=(root, delays, seen, stop), daemon=True)
        t.start()
        time.sleep(0.2)
        cmd = [sys.executable, RUNNER, "--root", root] + (["--within", str(within)] if within else [])
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        stop.set(); t.join(2)
        pubs = os.listdir(os.path.join(root, "light/test/pubs"))
        return p, seen, pubs


def check(name, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + name + (f"  ({detail})" if detail and not ok else ""))
    return ok


def main():
    colours = ["yellow", "red", "green", "blue", "cyan", "purple", "orange", "pink", "white", "warm"]
    results = []
    slow = {c: 0.1 for c in colours}
    slow["green"] = 2.5
    p, seen, pubs = run(slow)
    results.append(check("a colour drawn after 2.5 s passes at 4 s", p.returncode == 0 and "RING=PASS" in p.stdout,
                         p.stdout[-400:] + p.stderr[-200:]))
    results.append(check("each colour's time is recorded", "green:2." in p.stdout, p.stdout[-300:]))
    h, light = seen[0]
    results.append(check("the request is lamp-selftest's: lamp_msgs/Light on light/test, level 3",
                         h.get("type") == "lamp_msgs/Light" and h.get("topic") == "light/test"
                         and h.get("kind") == "latest" and light == {"on": True, "colour": "yellow", "level": 3},
                         str(seen[0])))
    results.append(check("the pubs/ entry is removed after", pubs == [], str(pubs)))
    p, _, _ = run(slow, within=1)
    results.append(check("the same lamp fails at lamp-selftest's 1 s", p.returncode == 1
                         and "RING=FAIL" in p.stdout and "green" in p.stdout, p.stdout[-300:]))
    never = {c: 0.1 for c in colours}
    never["pink"] = None
    p, _, _ = run(never)
    results.append(check("a colour never drawn fails and is named", p.returncode == 1
                         and "RING=FAIL: not drawn within 4 s: pink" in p.stdout, p.stdout[-300:]))
    p, _, _ = run({c: None for c in colours}, within=0.5)
    results.append(check("none drawn is not_checked", p.returncode == 3 and "RING=NOT_CHECKED" in p.stdout,
                         p.stdout[-300:]))
    print(f"{sum(results)}/{len(results)} passed")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
