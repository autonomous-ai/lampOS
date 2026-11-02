#!/usr/bin/env python3
"""cmd-score.py RUN_DIR: the #4861 table from a cmd.sh run (plays.tsv, turn-times.json) and commands.txt.

Per trial: right (an expected device call, and for light_colour:COLOUR light/state reads that colour after),
wrong (a device call on a none line, or one not in expect), or missed (no device call on a command line,
held back included). Timing is over right command trials: from the end of the clip to the lamp's first
sound (voice/state speaking, Mac clock moved to the lamp's), and from the end-of-speech decision to the
first sound handed to the speaker (turn-times, the lamp's own records), joined by request.
"""
import csv
import json
import os
import sys


def pct(xs, q):
    xs = sorted(xs)
    return None if not xs else xs[min(len(xs) - 1, int(q * len(xs)))]


def fmt(x):
    return "—" if x is None else f"{x:.0f}"


def main(run):
    here = os.path.dirname(os.path.abspath(__file__))
    lines = {}
    for row in open(os.path.join(here, "commands.txt")):
        f = row.rstrip("\n").split("|")
        if row.startswith("#") or len(f) < 8:
            continue
        lines[f[0]] = {"class": f[3], "expect": f[7]}
    tt = {}
    p = os.path.join(run, "turn-times.json")
    if os.path.exists(p):
        for row in open(p):
            try:
                r = json.loads(row)
            except ValueError:
                continue
            if "turn" in r:
                tt[str(r["turn"])] = r
    trials = []
    for t in csv.DictReader(open(os.path.join(run, "plays.tsv")), delimiter="\t"):
        ln = lines.get(t["id"])
        if not ln:
            continue
        calls = [] if t["calls"] == "none" else t["calls"].split(",")
        want = [] if ln["expect"] == "none" else ln["expect"].split(",")
        names = [w.split(":")[0] for w in want]
        wrong = any(c not in names for c in calls)
        right = not wrong and any(c in names for c in calls)
        for w in want:
            if ":" in w and w.split(":")[0] in calls:
                try:
                    right = right and json.loads(t["light_after"])["colour"] == w.split(":")[1]
                except ValueError:
                    right = False
                wrong = wrong or not right
        if not want:
            right = not calls
        r = tt.get(t["request"], {})
        trials.append({**t, **ln, "right": right, "wrong": wrong,
                       "missed": bool(want) and not calls,
                       "clip": None if t["first_sound_ms"] == "none" else float(t["first_sound_ms"]),
                       "decision": r.get("first_sound_ms")})
    print("## Device commands on today's path (#4861)\n")
    print("| class | n | right | missed | wrong | clip end → first sound p50 / p90 ms | decision → first sound p50 / p90 ms |")
    print("|---|---|---|---|---|---|---|")
    for c in ["clear", "vague", "plain", "other"]:
        ts = [t for t in trials if t["class"] == c]
        if not ts:
            continue
        timed = [t for t in ts if t["right"]] if c in ("clear", "vague") else ts
        a = [t["clip"] for t in timed if t["clip"] is not None]
        d = [t["decision"] for t in timed if t["decision"] is not None]
        print(f"| {c} | {len(ts)} | {sum(t['right'] for t in ts)} | {sum(t['missed'] for t in ts)} | "
              f"{sum(t['wrong'] for t in ts)} | {fmt(pct(a, .5))} / {fmt(pct(a, .9))} (n {len(a)}) | "
              f"{fmt(pct(d, .5))} / {fmt(pct(d, .9))} (n {len(d)}) |")
    cmd = [t for t in trials if t["class"] in ("clear", "vague") and t["right"]]
    a = [t["clip"] for t in cmd if t["clip"] is not None]
    p50 = pct(a, .5)
    print(f"\nRight command trials, clip end → first sound: p50 {fmt(p50)} ms over {len(a)}. "
          f"The first builder's reopen bar (#4861): p50 over 2500 ms → "
          f"{'—' if p50 is None else ('over' if p50 > 2500 else 'not over')}.")
    print(f"Wrong actions: {sum(t['wrong'] for t in trials)} of {len(trials)} trials.")
    print("\n### Per trial\n\n| pass | id | class | expect | voice | calls | light after | clip→sound ms | decision→sound ms | result |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for t in trials:
        res = "wrong" if t["wrong"] else "right" if t["right"] else "missed"
        print(f"| {t['pass']} | {t['id']} | {t['class']} | {t['expect']} | {t['voice_seen']} | {t['calls']} | "
              f"{t['light_after']} | {fmt(t['clip'])} | {fmt(t['decision'])} | {res} |")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__.split("\n\n")[0])
    main(sys.argv[1])
