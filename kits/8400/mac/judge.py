#!/usr/bin/env python3
"""el-8400 judge, on the Mac: python3 -I mac/judge.py results/<run>

Reads the rounds the rig kept (out/r1[, r1b], out/r2, out/r3, out/leg2) and the two
manifests, and prints each check with PASS/FAIL, then LEG1 and LEG2. The lines are matched
exactly as #8400's code and build 14976's code print them (lamp-update's `say` and eprintln
both print "lamp-update: <line>"; the round's outcome is its last stdout line):

  A (14976) round.rs:316-321  build N runs; lamp-update switches from its build <a12> to <b12>, which must confirm itself
  A (14976) round.rs:322-331  build N needs updater ...   (the climb: must NOT appear)
  B main.rs:1046              confirmed: lamp-update runs its new build
  B round.rs:1352-1354        build N, which the lamp runs, adds X: the updater that switched to it did not carry them (#7018)
  B round.rs:3513 (+ :3480)   started X, added by build N
  B round.rs:301              up to date: build N
  B round.rs:1329, :1367, :3623  are not added / was taken away (must NOT appear)
"""
import hashlib
import json
import os
import re
import sys

base = sys.argv[1]
out = os.path.join(base, "out")
env = {}
for line in open(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(base))), "stage", "STAGE.env")):
    if "=" in line:
        k, v = line.strip().split("=", 1)
        env[k] = v.strip('"')
B, A = int(env["B_SEQ"]), int(env["A_SEQ"])
mb = json.load(open(os.path.join(base, "chanB-manifest.json")))
ma = json.load(open(os.path.join(base, "chanA-manifest.json")))
sha = lambda m, n: [f["sha256"] for f in m["files"] if f["name"] == n][0]
a12, b12 = sha(ma, "lamp-update")[:12], sha(mb, "lamp-update")[:12]
npu_unit = [u["sha256"] for u in mb["units"] if u["lamp"] == "product" and u["unit"] == "lamp-npu.service"][0]
npu_bin = sha(mb, "lamp-npu")

results = {"1": [], "2": []}


def check(leg, ok, what):
    results[leg].append(ok)
    print("%s  leg %s  %s" % ("PASS" if ok else "FAIL", leg, what))


def read(name, f):
    p = os.path.join(out, name, f)
    return open(p).read() if os.path.exists(p) else None


def lines(name):
    text = read(name, "journal.txt") or ""
    return text.splitlines()


def index(ls, pattern):
    rx = re.compile(pattern)
    for i, l in enumerate(ls):
        if rx.search(l):
            return i
    return -1


def files(name):
    return read(name, "files.txt") or ""


def state(name):
    t = read(name, "state.json")
    return json.loads(t) if t else {}


def seq(s, key):
    v = s.get(key)
    return v.get("sequence") if isinstance(v, dict) else v


# ---- Leg 1 ----
r1 = lines("r1")
rounds1 = ["r1"] + (["r1b"] if os.path.exists(os.path.join(out, "r1b")) else [])
print("== leg 1: rounds %s (A %d -> B %d; lamp-update %s -> %s)" % (", ".join(rounds1), A, B, a12, b12))
i_self = index(r1, r"^lamp-update: build %d runs; lamp-update switches from its build %s to %s, which must confirm itself$" % (B, a12, b12))
check("1", i_self >= 0, "r1: A's updater switched the lamp to %d and self-switched to B's (A round.rs:316-321)" % B)
check("1", index(r1, r"needs updater") < 0, "r1: no climb line ('needs updater', A round.rs:322-331): A switched B itself")
add_round, i_add, i_conf, i_start, L = None, -1, -1, -1, []
for name in rounds1:
    L = lines(name)
    i_add = index(L, r"^lamp-update: build %d, which the lamp runs, adds (.+): the updater that switched to it did not carry them \(#7018\)$" % B)
    if i_add >= 0:
        add_round = name
        break
if add_round:
    i_conf = index(L, r"^lamp-update: confirmed: lamp-update runs its new build$")
    added = re.search(r"adds (.+): the updater", L[i_add]).group(1).split(", ")
    i_start = index(L, r"^lamp-update: started lamp-npu\.service, added by build %d$" % B)
    print("     the #7018 line (%s): %s" % (add_round, L[i_add]))
    print("     added: %s" % ", ".join(added))
    for l in L:
        if re.match(r"^lamp-update: started \S+, added by build %d$" % B, l):
            print("     " + l)
    check("1", add_round != "r1" or (0 <= i_self < i_conf), "%s: B's updater confirmed itself (B main.rs:1046)%s" % (
        add_round, " after the self-switch" if add_round == "r1" else ""))
    check("1", "lamp-npu.service" in added and i_conf < i_add, "%s: 'build %d, which the lamp runs, adds ...lamp-npu.service...: ... (#7018)' after the confirm (B round.rs:1352-1354)" % (add_round, B))
    check("1", i_start > i_add, "%s: 'started lamp-npu.service, added by build %d' after it (B round.rs:3513, :3480)" % (add_round, B))
    last = [l for l in L if not l.startswith("lamp-update: ") and not l.startswith("EXIT ")]
    check("1", bool(last) and last[-1] == "up to date: build %d" % B and L[-1] == "EXIT 0", "%s: the round ends 'up to date: build %d', exit 0 (B round.rs:301)" % (add_round, B))
    bad = [l for l in L if re.search(r"was taken away|are not added|putting build|could not be started|^refused, nothing switched", l)]
    check("1", not bad, "%s: no taken-away, not-added, put-back, start-failed or refused line%s" % (add_round, (": " + bad[0]) if bad else ""))
    f = files(add_round)
    check("1", re.search(r"^%s /etc/systemd/system/lamp-npu\.service$" % npu_unit, f, re.M) is not None, "lamp-npu.service is B's unit byte for byte (sha256 %s)" % npu_unit[:12])
    check("1", "/etc/systemd/system/lampos.target.wants/lamp-npu.service -> " in f, "lamp-npu.service enabled: its lampos.target.wants link")
    check("1", re.search(r"^/opt/lampOS/npu/current -> \S*%s/?$" % npu_bin, f, re.M) is not None, "/opt/lampOS/npu/current names B's lamp-npu (%s)" % npu_bin[:12])
    upd = (read(add_round, "updater.after") or "").strip()
    check("1", upd.endswith(sha(mb, "lamp-update")), "update/current is B's lamp-update after the round")
    check("1", read(add_round, "self.json") is None, "no self.json left: B's switch of its own is confirmed")
    s = state(add_round)
    check("1", seq(s, "installed") == B and s.get("adds_tried") == B and seq(s, "previous") == A,
          "state.json: installed %s, previous %s, adds_tried %s (want %d, %d, %d)" % (seq(s, "installed"), seq(s, "previous"), s.get("adds_tried"), B, A, B))
    calls = read(add_round, "calls.txt") or ""
    check("1", re.search(r" systemctl start --no-block -- lamp-npu\.service$", calls, re.M) is not None,
          "the stand-in was asked 'systemctl start --no-block -- lamp-npu.service'")
else:
    check("1", False, "no round has 'build %d, which the lamp runs, adds ...' (B round.rs:1352-1354)" % B)

# ---- Leg 2 ----
print("== leg 2: lamp-npu.service deleted, two units edited, rounds r2 and r3")
leg2 = os.path.join(out, "leg2")
if not os.path.isdir(leg2) or not os.path.exists(os.path.join(out, "r3")):
    check("2", False, "leg 2 did not run (out/leg2, out/r3)")
else:
    edits = {}
    for tag in ("E0", "E1"):
        unit = open(os.path.join(leg2, tag)).read().strip()
        edited = [f for f in os.listdir(leg2) if f.startswith(tag + ".")][0]
        edits[unit] = hashlib.sha256(open(os.path.join(leg2, edited), "rb").read()).hexdigest()
        print("     %s: %s edited, sha256 %s" % (tag, unit, edits[unit][:12]))
    for name in ("r2", "r3"):
        L = lines(name)
        last = [l for l in L if not l.startswith("lamp-update: ") and not l.startswith("EXIT ")]
        check("2", bool(last) and last[-1] == "up to date: build %d" % B and L and L[-1] == "EXIT 0", "%s: 'up to date: build %d', exit 0" % (name, B))
        again = [l for l in L if re.search(r"which the lamp runs, adds|added by build|are not added|was taken away", l)]
        check("2", not again, "%s: nothing added or tried again%s" % (name, (": " + again[0]) if again else ""))
        f = files(name)
        check("2", "lamp-npu.service" not in f.split("## stand-in states")[0], "%s: lamp-npu.service and its wants link still gone" % name)
        for unit, h in edits.items():
            check("2", re.search(r"^%s /etc/systemd/system/%s$" % (h, re.escape(unit)), f, re.M) is not None, "%s: %s byte-identical to the owner's edit" % (name, unit))
        s = state(name)
        check("2", seq(s, "installed") == B and s.get("adds_tried") == B, "%s: state.json installed %s, adds_tried %s" % (name, seq(s, "installed"), s.get("adds_tried")))
        calls = read(name, "calls.txt") or ""
        touch = [l for l in calls.splitlines() if re.search(r"systemctl (start|restart|enable)\b.*(lamp-npu|%s)" % "|".join(map(re.escape, edits)), l)]
        check("2", not touch, "%s: no start, restart or enable of the deleted or edited units%s" % (name, (": " + touch[0]) if touch else ""))

for leg in ("1", "2"):
    print("LEG%s %s (%d checks)" % (leg, "PASS" if results[leg] and all(results[leg]) else "FAIL", len(results[leg])))
