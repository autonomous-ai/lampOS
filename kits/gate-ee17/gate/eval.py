#!/usr/bin/env python3
# The ee17 promote gate's judge, on the Mac (no lamp): it reads what the lamp scripts printed and says PASS, FINDING
# or FAIL with each check's reason. Subcommands:
#   manifest <base> <seq>                    a release's lampos-manifest.json (envelope: base64 "manifest"): prints
#                                            "MANIFEST seq=<n> commit=<c> lamp-update=<sha256>"
#   after <round.txt> <facts.txt> <seq> <prev> <manifest sha> <prefix|-> [window]
#                                            the after-round checks (the first builder's 12:02 list, #7814)
#   peek <label> <idle.txt> <task.txt>       "PEEK <label> idle bytes=.. cpu=.. task bytes=.. cpu=.."
#   peekcmp <before.txt> <after.txt>         #8417: after's CPU not higher and after's bytes lower, in both minutes
import base64, json, re, sys, urllib.request

def manifest(base, seq):
    url = base.rstrip("/") + "/lampos-lamp-voice-%s/lampos-manifest.json" % seq
    try:
        env = json.loads(urllib.request.urlopen(url, timeout=30).read())
        m = json.loads(base64.b64decode(env["manifest"]))
    except Exception as e:
        print("MANIFEST unread %s: %s" % (url, e)); return 1
    up = [f.get("sha256") for f in m.get("files", []) if f.get("name") == "lamp-update"]
    print("MANIFEST seq=%s commit=%s lamp-update=%s" % (m.get("sequence"), str(m.get("commit", ""))[:12], up[0] if up else "none"))
    return 0

def kv(line):
    return dict(re.findall(r"(\w+)='([^']*)'", line) + re.findall(r"(\w+)=([^\s']+)", line))

def first(lines, tag):
    for l in lines:
        if l.startswith(tag + " "): return l[len(tag) + 1:]
    return None

def epoch(l):
    m = re.match(r"\s*([0-9]+(?:\.[0-9]+)?)\s", l); return float(m.group(1)) if m else None

KILL = re.compile(r"status=9/KILL|SIGKILL|Killing process|code=killed")
STOPTO = re.compile(r"State '?stop[a-z-]*'? timed out|Failed with result '?timeout'?|Stopping timed out|Processes still around after")
EXIT1 = re.compile(r"Main process exited, code=exited, status=1/FAILURE")

def after(rnd, facts, seq, prev, msha, prefix, window=96.0):
    R = open(rnd).read().splitlines(); F = open(facts).read().splitlines()
    out = []; res = []
    def ck(cid, ok, what):
        v = ok if ok in ("PASS", "FAIL", "FINDING") else ("PASS" if ok else "FAIL")
        res.append(v); out.append("CHECK %s %s %s" % (cid, v, what))
    st = kv(first(F, "STATE") or "")
    ck("installed", st.get("installed") == seq, "installed=%s (want %s)" % (st.get("installed"), seq))
    ck("previous", st.get("previous") == prev, "previous=%s (want %s)" % (st.get("previous"), prev))
    try: o = R[R.index("--- OUTPUT") + 1:R.index("--- END")]
    except ValueError: o = None
    ex = first(R, "EXIT")
    if o is None:
        ck("outcome", False, "no round output in %s" % rnd)
    else:
        ut = [l for l in o if re.search(r"(^|: )up to date: build %s$" % re.escape(seq), l)]
        ck("outcome", bool(ut), "'%s'" % (ut[-1] if ut else "no 'up to date: build %s' line; last: %s" % (seq, (o[-1] if o else "none")[:120])))
        bad = [l for l in o if "refused" in l or "putting build" in l]
        ck("no-refused", not bad, "no 'refused' or 'putting build' line" if not bad else "%d line(s): %s" % (len(bad), bad[0][:140]))
    ck("exit", ex == "0", "EXIT %s" % ex)
    u = first(F, "UPD") or "none"
    ok = msha not in ("", "none") and u == msha and (prefix in ("", "-") or u.startswith(prefix))
    ck("digest", ok, "lamp-update %s, manifest %s%s" % (u[:16], msha[:16], "" if prefix in ("", "-") else ", want %s..." % prefix))
    g = kv(first(F, "GW") or "")
    i = kv(first(F, "INODE") or "")
    ino_ok = i.get("host") not in (None, "none", "") and i.get("host") == i.get("gateway")
    ck("gw-inode", ino_ok, "control.sock inode host=%s gateway=%s (pid %s, %s mount ns)" % (i.get("host"), i.get("gateway"), i.get("pid"), i.get("ns")))
    ck("gw-active", g.get("active") == "active" and g.get("listening") == "yes", "active=%s listening=%s" % (g.get("active"), g.get("listening")))
    po = g.get("partof", ""); to = g.get("timeout", "")
    ck("gw-unit", "lamp-control.service" in po.split() and to == "2min 30s", "PartOf='%s' TimeoutStopUSec='%s' (want lamp-control.service, 2min 30s)" % (po, to))
    J = [l[4:] for l in F if l.startswith("GWJ ")]
    k = [l for l in J if KILL.search(l) or STOPTO.search(l)]
    ck("gw-nokill", not k, "no KILL, no stop timeout since the round's start" if not k else "%d: %s" % (len(k), k[0][:160]))
    ex1 = [epoch(l) for l in J if EXIT1.search(l)]
    W = [(float(a), b) for a, b in (l[4:].split()[:2] for l in F if l.startswith("GWW "))]
    now = float(first(F, "NOW") or 0)
    if not ex1:
        ck("gw-exit1", True, "no exit 1")
    elif len(ex1) > 1:
        ck("gw-exit1", False, "%d exits 1 (more than one is a FAIL)" % len(ex1))
    else:
        t = ex1[0]; up = [a for a, b in W if b == "LISTEN" and a > t]
        if up:
            d = up[0] - t
            ck("gw-exit1", "FINDING" if d <= window else "FAIL", "one exit 1, listening again %.0f s after it (%s %.0f s; #8429)" % (d, "within" if d <= window else "NOT within", window))
        elif not W and g.get("listening") == "yes" and now and now - t <= window:
            ck("gw-exit1", "FINDING", "one exit 1, listening at the read %.0f s after it (no watcher log; #8429)" % (now - t))
        else:
            ck("gw-exit1", False, "one exit 1 and no listen after it within %.0f s (watcher %s)" % (window, "read" if W else "log missing"))
    U = [l[5:] for l in F if l.startswith("UNIT ")]
    badu = []; off = []
    for l in U:
        name = l.split()[0]; a = kv(l)
        if a.get("active") == "active": continue
        if a.get("active") == "inactive" and a.get("type") == "oneshot" and a.get("result") == "success": continue
        # A part off by its own switch (lamp-addons, lamp-mood, lamp-play, ...: "Off, it exits at once" and stays
        # inactive, its unit's own words) ended with exit 0: inactive with Result=success is that, never a failure.
        if a.get("active") == "inactive" and a.get("result") == "success": off.append(name); continue
        badu.append("%s %s/%s" % (name, a.get("active"), a.get("result")))
    fl = [x for x in (first(F, "FAILED") or "").split() if x.startswith("lamp")]
    offs = "; off by their switch (exited 0): %s" % ", ".join(off) if off else ""
    ck("units", bool(U) and not badu and not fl, "%d units, all active or off by their switch%s" % (len(U), offs) if U and not badu and not fl else
       "not active: %s; failed: %s" % (", ".join(badu) or "none", " ".join(fl) or "none") if U else "no UNIT lines")
    print("\n".join(out))
    v = "FAIL" if "FAIL" in res else ("FINDING" if "FINDING" in res else "PASS")
    print("AFTER %s installed=%s previous=%s (%d checks: %d PASS, %d FINDING, %d FAIL)" % (v, st.get("installed"), st.get("previous"), len(res), res.count("PASS"), res.count("FINDING"), res.count("FAIL")))
    return 0

def meas(path, label):
    for l in open(path).read().splitlines():
        if l.startswith("MEASURE %s " % label):
            a = kv(l)
            try:
                if a.get("pidstat", "none") != "none": cpu = float(a["pidstat"])
                else: cpu = (float(a["ticks1"]) - float(a["ticks0"])) / (float(a["clk"]) * float(a["secs"])) * 100
                return int(a["bytes"]), round(cpu, 2), a.get("pid") == a.get("pid_after")
            except Exception: return None
    return None

def peek(label, idle, task):
    i, t = meas(idle, "idle"), meas(task, "task")
    if not i or not t:
        print("PEEK %s unread (idle %s, task %s)" % (label, "ok" if i else "missing", "ok" if t else "missing")); return 1
    print("PEEK %s idle bytes=%d cpu=%.2f task bytes=%d cpu=%.2f" % (label, i[0], i[1], t[0], t[1]))
    if not (i[2] and t[2]): print("NOTE lamp-peek restarted during a minute: the CPU read is of two processes")
    return 0

def peekline(path):
    for l in open(path).read().splitlines():
        m = re.match(r"PEEK \w+ idle bytes=(\d+) cpu=([\d.]+) task bytes=(\d+) cpu=([\d.]+)", l)
        if m: return [float(x) for x in m.groups()]
    return None

def peekcmp(before, after):
    b, a = peekline(before), peekline(after)
    if not b or not a:
        print("PEEK 8417 FAIL: a read is missing (before %s, after %s)" % (bool(b), bool(a))); return 0
    why = []
    for n, (bb, bc, ab, ac) in (("idle", (b[0], b[1], a[0], a[1])), ("task", (b[2], b[3], a[2], a[3]))):
        if ac > bc: why.append("%s CPU higher (%.2f > %.2f)" % (n, ac, bc))
        if not ab < bb: why.append("%s bytes not lower (%d >= %d)" % (n, ab, bb))
    print("PEEK 8417 %s: idle bytes %d -> %d cpu %.2f -> %.2f; task bytes %d -> %d cpu %.2f -> %.2f%s" % (
        "FAIL" if why else "PASS", b[0], a[0], b[1], a[1], b[2], a[2], b[3], a[3], (": " + "; ".join(why)) if why else ""))
    return 0

if __name__ == "__main__":
    c, a = sys.argv[1], sys.argv[2:]
    if c == "manifest": sys.exit(manifest(*a))
    if c == "after": sys.exit(after(*a[:6], window=float(a[6]) if len(a) > 6 else 96.0))
    if c == "peek": sys.exit(peek(*a))
    if c == "peekcmp": sys.exit(peekcmp(*a))
    print("no such subcommand %s" % c); sys.exit(2)
