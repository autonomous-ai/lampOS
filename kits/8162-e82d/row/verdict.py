#!/usr/bin/env python3
# Mac, read only. #8162 ROW: one leg's checks and verdict, from row/logs/<LEG>.* (legs.sh writes them).
#   python3 -I verdict.py <logs dir> <LEG>                 prints CHECK lines, NUM lines, then "LEG <L> PASS|FAIL|VOID: ..."
#   python3 -I verdict.py <logs dir> --report <build>      the ROW comment (bot form) from logs/verdicts.txt + round logs
# Patterns come from the environment (patterns.env, exported by legs.sh). Times: the journal (short-unix) and the AW
# lines are the lamp's clock; plays.log is the Mac's, moved onto the lamp's clock by logs/skew.txt ("SKEW <lamp - mac>").
import os, re, sys

L = sys.argv[1]
P = {k: os.environ[k] for k in os.environ if k.startswith("P_")}
NOTTAKE = ("off", "not_installed", "installing", "starting", "recovering", "error", "none")
# Bounds (the handoff's PASS bars, #8162 comment 7068425649); each can be set from the environment.
B = {k: float(os.environ.get(k, v)) for k, v in dict(
    L1_LEAVE=14, L1_LEAVE_TARGET=1, L3_LEAVE=2, AUDIO_MS=2000, HEALTH_READY=15, L2_END=14, L4_END=10,
    L8_BACK=12, L9_ERROR=1, HOLDMIN=180, HEARD_WITHIN=120, TOLD_WITHIN=300, ASK_WITHIN=30).items()}


def rd(name):
    p = os.path.join(L, name)
    return open(p, errors="replace").read().splitlines() if os.path.exists(p) else []


def num(s):
    try: return float(s)
    except Exception: return None


def report(build):
    head = os.environ.get("HEAD_SHA", "?"); lamp = os.environ.get("LAMP", "lamp-ee17")
    legs = os.environ.get("ROW_LEGS", "L0 L1 L2 L3 L4 L5 L6 L7 L8 L9").split()
    must = os.environ.get("ROW_MUST", "L0 L1 L2 L3 L4 L5 L7 L8").split()
    v = {}
    for l in rd("verdicts.txt"):
        m = re.match(r"LEG (L[0-9]) (PASS|FAIL|VOID): ?(.*)", l)
        if m: v[m.group(1)] = (m.group(2), m.group(3))
    steps = {"round 1": ("round1.txt", "^ROUND 1 OK"), "verify": ("verify.txt", "^VERIFY PASS"), "round 2": ("round2.txt", "^ROUND 2 OK")}
    bad = [s for s, (f, pat) in steps.items() if not any(re.search(pat, l) for l in rd(f))]
    failed = [g for g in legs if v.get(g, ("",))[0] == "FAIL"]
    missing = [g for g in must if v.get(g, ("",))[0] != "PASS" and g not in failed]
    back = {"putback": ("putback.txt", "^PUTBACK OK"), "hdown": ("hdown.txt", "^HDOWN OK"), "read": ("read.txt", "^READ TWICE same")}
    notback = [s for s, (f, pat) in back.items() if not any(re.search(pat, l) for l in rd(f))]
    if notback: print("REPORT NOT READY: the put-back is not finished (%s): finish it first" % " ".join(notback)); return 3
    if missing and not failed and not bad:
        print("REPORT NOT READY: no PASS yet for %s (VOID or not run): rerun them" % " ".join(missing)); return 3
    word = "FAIL" if (failed or bad) else "PASS"
    out = ["ROW #8162 %s on %s build %s: %s" % (head, lamp, build, word)]
    if word == "FAIL": out.append("Failed: %s" % ", ".join(bad + failed))
    out += ["", "```"]
    r1 = [l[2:] for l in rd("round1.txt") if l.startswith("J ")]
    first = [l for l in r1 if re.search("switched to build %s|up to date: build %s|switches to its build|confirmed: " % (build, build), l)]
    out += [re.sub(r"^[0-9.]+ ", "", l)[:200] for l in first[:6]] or ["(no round 1 end line in row/logs/round1.txt)"]
    for g in legs:
        keys = [l for l in rd("%s.keylines" % g)][:6]
        out.append("== %s %s" % (g, v.get(g, ("not run", ""))[0]))
        out += [k[:220] for k in keys]
    r2 = [l[2:] for l in rd("round2.txt") if l.startswith("J ") and re.search("switched to build|up to date: build|confirmed: ", l)]
    out += ["== round 2"] + [re.sub(r"^[0-9.]+ ", "", l)[:200] for l in r2[:4]]
    out += ["```", "", "| leg | verdict | refused (ms) | first audio (ms) | left ready (s) | task ended (s) | health -> ready (s) | fresh -> ready (s) | notes |",
            "|---|---|---|---|---|---|---|---|---|"]
    for g in legs:
        n = {}
        for l in rd("%s.verdict" % g):
            m = re.match(r"NUM (\S+)=(\S+)", l)
            if m: n[m.group(1)] = m.group(2)
        out.append("| %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (g, v.get(g, ("not run", ""))[0], n.get("refused_ms", ""), n.get("audio_ms", ""),
                   n.get("left_s", ""), n.get("ended_s", ""), n.get("health_ready_s", ""), n.get("fresh_ready_s", ""), v.get(g, ("", ""))[1][:90]))
    print("\n".join(out))
    return 0


if len(sys.argv) > 3 and sys.argv[2] == "--report":
    sys.exit(report(sys.argv[3]))

LEG = sys.argv[2]
out = []; checks = []; keyl = []


def say(s):
    out.append(s); print(s)


skew = 0.0
for l in rd("skew.txt"):
    if l.startswith("SKEW "): skew = float(l.split()[1])

J = []  # (t, text), sorted by the lamp's time
for l in rd(LEG + ".journal"):
    t = num(l.split(" ", 1)[0])
    if t is not None: J.append((t, l))
J.sort(key=lambda x: x[0])
AW = []  # (t, state, why)
for l in rd(LEG + ".aw"):
    f = l.split()
    if len(f) >= 4 and f[0] == "AW" and num(f[1]) is not None: AW.append((float(f[1]), f[2], f[3]))
AW.sort(key=lambda x: x[0])
MARKS = rd(LEG + ".marks")
T_START = next((num(l.split()[1]) for l in MARKS if l.startswith("START ")), None) or -1e18
PLAYS = []  # (lamp t, clip) of this leg's run only (plays.log keeps every run's plays)
for l in rd("plays.log"):
    f = l.split()
    if len(f) >= 4 and f[2] == "PLAY" and ("leg=%s " % LEG) in l + " " and float(f[0]) + skew >= T_START - 1: PLAYS.append((float(f[0]) + skew, f[3]))


def mark(name, f=".marks"):
    for l in rd(LEG + f):
        if l.startswith(name + " "): return num(l.split()[1])


def has(f, pat):
    return any(re.search(pat, l) for l in rd(LEG + f))


def first(pat, after=-1e18, before=1e18, but=None):
    r = re.compile(pat); x = re.compile(but) if but else None
    for t, l in J:
        if after <= t <= before and r.search(l) and not (x and x.search(l)): return t, l
    return None, None


def count(pat, after=-1e18, before=1e18):
    r = re.compile(pat); return sum(1 for t, l in J if after <= t <= before and r.search(l))


def key(l):
    if l and l not in keyl: keyl.append(l)


def check(ok, label, detail):
    w = "PASS" if ok else ("FAIL" if ok is False else "NOTE")
    checks.append(w); say("CHECK %s %s %s: %s" % (LEG, w, label, detail)); return ok


def numl(k, v):
    if v is not None: say("NUM %s=%s" % (k, ("%.1f" % v) if isinstance(v, float) else v))


def leave(t0):
    for t, s, w in AW:
        if t >= t0 and s in NOTTAKE: return t, s, w
    return None, None, None


def ready_after(t0):
    for t, s, w in AW:
        if t >= t0 and s == "ready": return t
    return None


def series(t0, t1):
    seq = []
    for t, s, w in AW:
        if t0 <= t <= t1 and (not seq or seq[-1] != (s, w)): seq.append((s, w))
    return seq


def fmt(seq):
    return " -> ".join("%s%s" % (s, "" if w == "-" else "(%s)" % w) for s, w in seq) or "none"


def play(i):
    return PLAYS[i] if len(PLAYS) > i else (None, None)


def heard(tp):
    # The voice took the clip as an ask: a request line within ASK_WITHIN, and not held back as said to nobody.
    if tp is None: return "no clip played"
    if first(P["P_V_HELD"], tp, tp + B["ASK_WITHIN"])[0]: return "ask not heard as a call (held back: nobody spoke to the lamp)"
    if not first(P["P_V_REQ"], tp, tp + B["ASK_WITHIN"])[0]: return "the clip was not heard (no request line within %d s)" % B["ASK_WITHIN"]
    return None


def refused(tp, tready, label="the ask is refused at once"):
    if tp is None: return check(False, label, "no clip played")
    tr, lr = first(P["P_V_REFUSED"], tp, tp + B["ASK_WITHIN"])
    tk, lk = first(P["P_T_TAKEN"], tp, tready if tready else tp + 60)
    key(lr); numl("refused_ms", int((tr - tp) * 1000) if tr else None)
    ok = bool(tr) and not tk
    check(ok, label, "voice refusal %s; hand-over before ready %s" % ("+%.2f s after the clip" % (tr - tp) if tr else "none", "YES: " + lk[-90:] if tk else "none"))
    ta, la = first(P["P_V_AUDIO"], tp, tp + B["ASK_WITHIN"])
    ms = int(re.search(r"first answer audio ([0-9]+) ms", la).group(1)) if la else None
    numl("audio_ms", ms); key(la)
    check(ms is not None and ms <= B["AUDIO_MS"], "the problem line within %d ms" % B["AUDIO_MS"], "first answer audio %s ms" % ms)


def fresh(t0, tready):
    tn, ln = first(P["P_T_KEPTNEW"], t0)
    b = [l.split()[1] for l in rd(LEG + ".kept-before") if l.startswith("KEPT_AGENT ")]
    a = [l.split()[1] for l in rd(LEG + ".kept-after") if l.startswith("KEPT_AGENT ")]
    key(ln)
    if tn and tready: numl("fresh_ready_s", tready - tn)
    check(bool(tn) and bool(b) and bool(a) and b != a and a[0] != "none", "a fresh session",
          "kept new %s; kept agent %s -> %s" % ("+%.1f s" % (tn - t0) if tn else "none", (b or ["?"])[0][:8], (a or ["?"])[0][:8]))


def ready_again(tready, tp):
    if tready is None: return check(False, "'ready again' said once", "never ready again")
    n = count(P["P_V_READYTOLD"], tready, tready + B["TOLD_WITHIN"]); key(first(P["P_V_READYTOLD"], tready)[1])
    if n == 1: return check(True, "'ready again' said once", "told 1 time")
    if n == 0 and tp and tready - tp > B["HEARD_WITHIN"]:
        return check(None, "'ready again'", "not told: ready came %.0f s after the ask, past %d s with nobody seen (the presence gate)" % (tready - tp, B["HEARD_WITHIN"]))
    return check(False, "'ready again' said once", "told %d times" % n)


def hold2(t0, tready):
    # HOLD 2 on #8162: after a fault, recovering (or error) -> starting(session_starting) -> ready, never stuck.
    s = series(t0, tready if tready else 1e18)
    before = [x for x in s if x[0] != "ready" and x[0] != "working"]
    ok = bool(tready) and any(x[0] in ("recovering", "error") for x in before) and bool(before) and before[-1] == ("starting", "session_starting")
    check(ok, "HOLD 2: recovering -> starting(session_starting) -> ready", fmt(s))


def asked_after(f=".ask-after"):
    a = [l for l in rd(LEG + f) if l.startswith(("ASK_ANSWER", "ASK_NOT_SENT", "ASK_REFUSED", "ASK_NO_ANSWER", "ASK_ASKING"))]
    check(bool(a) and a[0].startswith("ASK_ANSWER"), "a socket ask after is answered", (a or ["no socket ask"])[0][:120])


def spoken_after(i, label="the next spoken ask is answered"):
    tp, cid = play(i)
    if tp is None: return check(False, label, "no clip played")
    td, ld = first(P["P_T_DONE"], tp); tr = first(P["P_V_REFUSED"], tp, tp + B["ASK_WITHIN"])[0]
    key(ld); check(bool(td) and not tr, label, "clip %s: %s%s" % (cid, "turn done +%.1f s" % (td - tp) if td else "no turn done", "; REFUSED" if tr else ""))


def back(tl):
    tready = ready_after(tl) if tl else None
    check(tready is not None, "tasks/agent back to ready", "+%.1f s after it left" % (tready - tl) if tready else "never back to ready (%s)" % fmt(series(tl or 0, 1e18)))
    return tready


def finish():
    void = [l[5:] for l in MARKS if l.startswith("VOID ")]
    if void:
        verdict = ("VOID", void[0])
    elif "FAIL" in checks:
        unheard = [h for h in (heard(tp) for tp, c in PLAYS) if h]
        bad = [o.split(": ", 1)[0].split(" ", 3)[-1] for o in out if o.startswith("CHECK %s FAIL" % LEG)]
        verdict = ("VOID", unheard[0]) if unheard else ("FAIL", "; ".join(bad))
    else:
        verdict = ("PASS", "%d checks" % checks.count("PASS") + (", %d notes" % checks.count("NOTE") if "NOTE" in checks else ""))
    say("LEG %s %s: %s" % (LEG, verdict[0], verdict[1]))
    open(os.path.join(L, LEG + ".verdict"), "w").write("\n".join(out) + "\n")
    open(os.path.join(L, LEG + ".keylines"), "w").write("\n".join(re.sub(r"^([0-9.]+) \S+ ", r"\1 ", l) for l in keyl) + ("\n" if keyl else ""))
    sys.exit(0)


say("== %s  AW: %s" % (LEG, fmt(series(-1e18, 1e18))))
if any(l.startswith("VOID ") for l in MARKS): finish()

if LEG == "L0":
    check(bool(AW) and AW[0][1] in ("ready", "working"), "tasks/agent reads ready at the start", AW[0][1] if AW else "no AW line")
    spoken_after(0, "the spoken ask is taken and answered")
    tp = play(0)[0]; td = first(P["P_T_DONE"], tp or 0)[0]
    if tp and td: numl("done_s", td - tp)
    asked_after()
elif LEG in ("L1", "L3"):
    t0 = mark("GWR_AT", ".gw") if LEG == "L1" else mark("JAM_AT", ".jam")
    if t0 is None: say("VOID no break mark"); MARKS.append("VOID the break was not made"); finish()
    tl, sl, wl = leave(t0); bound = B["L1_LEAVE"] if LEG == "L1" else B["L3_LEAVE"]
    numl("left_s", tl - t0 if tl else None)
    extra = " (over the %.0f s target, within the bound)" % B["L1_LEAVE_TARGET"] if LEG == "L1" and tl and tl - t0 > B["L1_LEAVE_TARGET"] and tl - t0 <= bound else ""
    check(tl is not None and tl - t0 <= bound, "tasks/agent leaves ready within %.0f s" % bound, "%s(%s) at +%s s%s" % (sl, wl, "%.2f" % (tl - t0) if tl else "never", extra))
    key(first(P["P_AGENT"], t0)[1])
    tready = back(tl)
    refused(play(0)[0], tready)
    if LEG == "L1":
        th = mark("GW_HEALTH", ".gw"); numl("health_ready_s", tready - th if tready and th else None)
        check(tready is not None and th is not None and tready - th <= B["HEALTH_READY"], "the Gateway answers -> ready within %.0f s" % B["HEALTH_READY"],
              "%s s (health +%s s after the restart)" % ("%.1f" % (tready - th) if tready and th else "?", "%.1f" % (th - t0) if th else "none"))
        key(first(P["P_T_GWBACK"], t0)[1])
    fresh(t0, tready); hold2(t0, tready); ready_again(tready, play(0)[0])
    spoken_after(1) if LEG == "L1" else asked_after()
elif LEG == "L2":
    t0 = mark("GWR_AT", ".gw")
    if t0 is None: MARKS.append("VOID the gateway was not restarted mid-task"); finish()
    te, le = first("%s|%s|%s" % (P["P_T_BROKE"], P["P_T_REFUSEDHAND"], P["P_T_STUCK"]), t0)
    key(le); numl("ended_s", te - t0 if te else None)
    check(te is not None and te - t0 <= B["L2_END"], "the task in flight ends within %.0f s" % B["L2_END"], "+%s s: %s" % ("%.1f" % (te - t0) if te else "?", (le or "no end line")[-120:]))
    ns = [l for l in rd("L2.ask-long") if l.startswith("ASK_NOT_SENT")]
    check(bool(ns) and re.search(r"one_session_stuck|one_session_not_started", ns[0]) is not None, "the store ends it not_sent, one_session_stuck|not_started", (ns or ["no ASK_NOT_SENT in L2.ask-long"])[0][:140])
    tt, lt = first(P["P_V_TOLD"], te or t0, (te or t0) + 60, but=P["P_V_READYTOLD"]); key(lt)
    check(tt is not None, "told it was not done", lt[-100:] if lt else "no task news told within 60 s")
    tl = leave(t0)[0]; tready = back(tl)
    r0, r1 = mark("REDO_FROM"), mark("REDO_TO")
    redo = count(P["P_T_TAKEN"], te or t0, r1 or 1e18) if te else None
    check(redo == 0 and r1 is not None, "never run again", "hand-overs from its end to the end of the %s s watch: %s" % ("%.0f" % (r1 - r0) if r0 and r1 else "?", redo))
    fresh(t0, tready); hold2(t0, tready); ready_again(tready, t0)
    spoken_after(0)
elif LEG == "L4":
    t0 = mark("JAM_AT", ".jam"); tp = play(0)[0]
    if t0 is None: MARKS.append("VOID the pane was not stopped"); finish()
    te, le = first("%s|%s|%s|%s" % (P["P_T_REFUSEDHAND"], P["P_T_WARMING"], P["P_T_STUCK"], P["P_T_BROKE"]), tp or t0)
    tr, lr = first(P["P_V_REFUSED"], tp or t0, (tp or t0) + B["ASK_WITHIN"])
    key(le or lr); numl("ended_s", (te - tp) if te and tp else None)
    ok = (te is not None and tp is not None and te - tp <= B["L4_END"]) or (tr is not None)
    check(ok, "ended at the first refusal within %.0f s (or refused at once)" % B["L4_END"],
          "end line %s; voice refusal %s" % ("+%.1f s after the ask" % (te - tp) if te and tp else "none", "yes" if tr else "none"))
    tt = first(P["P_V_TOLD"], te or tp or t0, (te or tp or t0) + 60, but=P["P_V_READYTOLD"])[0]
    check(tt is not None or tr is not None, "told at once (not done, or the problem line)", "news %s; refusal %s" % ("told" if tt else "none", "yes" if tr else "none"))
    tl = leave(t0)[0]; tready = back(tl)
    check(has(".unfreeze", "^UNFREEZE OK"), "the stopped pid resumed (SIGCONT)", " ".join(rd("L4.unfreeze"))[:120] or "no unfreeze read")
    fresh(t0, tready); hold2(t0, tready); ready_again(tready, tp); asked_after()
elif LEG == "L5":
    t0 = mark("HDR_AT", ".hd"); tp = play(0)[0]
    if t0 is None: MARKS.append("VOID Harness's daemon was not restarted"); finish()
    tw, lw = first(P["P_T_WATCHAGAIN"], t0, t0 + 120); key(lw)
    check(tw is not None, "the watch is taken again", "+%s s: %s" % ("%.1f" % (tw - t0) if tw else "?", (lw or "no 'ensured and watched again' line")[-100:]))
    check(bool(AW) and AW[-1][1] == "ready", "readiness ends at ready", AW[-1][1] if AW else "no AW")
    ta = first("%s|%s|%s" % (P["P_V_AUDIO"], P["P_V_REFUSED"], P["P_T_DONE"]), tp or t0, (tp or t0) + 300)
    key(ta[1]); check(ta[0] is not None, "the ask during the restart is answered or refused, never silent", (ta[1] or "no answer audio, no refusal")[-110:])
    asked_after()
elif LEG == "L6":
    app = [l.split(None, 1)[1] for l in MARKS if l.startswith("APP ")]
    a = [l for l in rd("L6.read") if l.startswith("AGENT ")]
    key((a or [""])[0])
    if not app or app[0] not in ("refused", "started"):
        MARKS.append("VOID engines.install not sent (from the app only: APP_L6=refused|started after sending it); read only: %s" % ((a or ["no AGENT read"])[0])); finish()
    check(bool(a) and "state=ready " in a[0], "the agent was ready when the app asked", (a or ["no AGENT read"])[0])
    check(app[0] == "refused", "engines.install refused: 'the agent is installed and running'", "the app said: %s" % app[0])
elif LEG == "L7":
    ins = rd("L7.install"); t0 = mark("L7_AT", ".install")
    if any(l.startswith("L7 STOP") for l in ins) or t0 is None:
        MARKS.append("VOID " + next((l for l in ins if l.startswith("L7 STOP")), "the install was not asked")); finish()
    o = [l for l in ins if l.startswith(("ORDER", "L7_ORDER", "L7_GW_HEALTH_S"))]
    for l in o: key(l)
    check(has(".install", "^L7_ORDER PASS"), "the install's order: tasks stop said < lamp-tasks stopped < gateway started < health < lamp-tasks started < start said",
          next((l for l in ins if l.startswith("L7_ORDER")), "no L7_ORDER line"))
    h = [l for l in ins if l.startswith("L7_GW_HEALTH_S")]; numl("gw_health_s", h[0].split()[1] if h else None)
    c = [l for l in rd("L7.check") if l.startswith("L7_CONFIG")]; key((c or [""])[0])
    check(bool(c) and c[0] == "L7_CONFIG LIVE", "lamp-tasks' merge live with no second restart", (c or ["no L7_CONFIG line"])[0][:140])
    tr = ready_after(t0)
    late = [x for x in AW if tr and x[0] > tr and x[1] == "installing"]
    check(tr is not None and not late and AW[-1][1] == "ready", "'installing' cleared: ready after the install and never installing again", fmt(series(t0, 1e18)))
    asked_after()
elif LEG == "L8":
    cut = rd("L8.cut"); tc = mark("CUT_AT", ".cut")
    if any(l.startswith(("L8 STOP", "L7 STOP")) for l in cut) or tc is None:
        MARKS.append("VOID " + next((l for l in cut if "STOP" in l), "the install was not cut off")); finish()
    g = next((l for l in cut if l.startswith("GAP lamp-tasks=")), "")
    key(g); check(re.search(r"lamp-tasks=(inactive|failed) ", g) is not None and "record=yes" in g, "the gap: lamp-tasks stopped, its stop recorded", g or "no GAP line")
    t0 = mark("L7_AT", ".cut") or tc
    st = [x for x in AW if x[0] >= t0 and x[1] == "starting" and x[2] == "tasks_restarting"]
    check(bool(st), "tasks/agent reads starting(tasks_restarting) in the gap", fmt(series(t0, 1e18)))
    tp = play(0)[0]; tr, lr = first(P["P_V_REFUSED"], tp or tc, (tp or tc) + B["ASK_WITHIN"]); key(lr)
    numl("refused_ms", int((tr - tp) * 1000) if tr and tp else None)
    check(tr is not None, "the ask in the gap is refused at once", (lr or "no voice refusal")[-110:])
    bk = rd("L8.back"); oe = mark("ONESHOT_END", ".back"); tb = mark("TASKS_BACK", ".back")
    numl("back_s", tb - oe if oe and tb else None)
    check(oe is not None and tb is not None and 0 <= tb - oe <= B["L8_BACK"], "lamp-tasks back within %.0f s of the one-shot's end" % B["L8_BACK"], "%s s" % ("%.1f" % (tb - oe) if oe and tb else "?"))
    n = count(P["P_U_CUTBACK"], t0); key(first(P["P_U_CUTBACK"], t0)[1])
    check(n == 1, "'started again: OpenClaw's install ended while it was stopped' said once", "said %d times" % n)
    check(has(".back", "^RECORD gone") and has(".back", "^L8BACK OK"), "the record gone and the repair finished", next((l for l in bk if l.startswith(("L8BACK", "RECORD"))), "no L8BACK line"))
    tl = leave(t0)[0]; trd = ready_after(tl) if tl else None
    late = [x for x in AW if trd and x[0] > trd and x[1] == "installing"]
    check(trd is not None and not late and AW[-1][1] == "ready", "back to ready, not left at installing", fmt(series(t0, 1e18)))
    asked_after()
elif LEG == "L9":
    t0 = mark("HOLD_AT")
    th, lh = first(P["P_T_HELD"], t0 or 0); key(lh)
    if th is None: MARKS.append("VOID the hold was not made (no held line)"); finish()
    te = [x for x in AW if x[0] >= th - 0.5 and x[1] == "error" and x[2] in ("held_approval", "held_sign_in")]
    numl("error_s", te[0][0] - th if te else None)
    check(bool(te) and te[0][0] - th <= B["L9_ERROR"], "error(held_*) within %.0f s of the held line" % B["L9_ERROR"], "%s" % ("+%.2f s" % (te[0][0] - th) if te else fmt(series(th, 1e18))))
    refused(play(0)[0], None, "an ask during the hold is refused")
    tu = mark("UNHOLD_AT")
    check(tu is not None and tu - th >= B["HOLDMIN"], "held at least %.0f s" % B["HOLDMIN"], "%s s" % ("%.0f" % (tu - th) if tu else "?"))
    nn = count(P["P_T_KEPTNEW"], th, tu or 1e18); nt = count(P["P_V_READYTOLD"], th, tu or 1e18)
    check(nn == 0 and nt == 0, "no churn while held: no fresh session, no 'ready again'", "kept new %d, ready again told %d" % (nn, nt))
    tready = ready_after(th)
    check(tready is not None and tu is not None and tready >= tu, "ready only after the hold is cleared", "ready %s" % ("+%.1f s after the clear" % (tready - tu) if tready and tu else ("BEFORE the clear" if tready else "never")))
    ready_again(tready, None)
finish()
