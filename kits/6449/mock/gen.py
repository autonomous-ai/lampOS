# Fixtures for the #6449 kit's mock: journal chunks (j/<LEG>.<CLIP>[.k], "@+<s> <unit>[pid]: <line>", k = the k-th
# play of that clip in that leg) and lamp-script outputs (out/<script>[.<key>]), one folder per case with case.env and
# expect; logs/ files replace the run's own (check.txt, on.txt). Journal lines are #6449's head's own text
# (second_provider.rs: the request, tools and hold lines).
import os, shutil, sys
FX = sys.argv[1]; shutil.rmtree(FX, ignore_errors=True)
V2 = "lamp-voice[812]: lamp-voice second: "
def heard(q): return V2 + "request %d heard 8 words" % q
def tools(q, calls): return V2 + "request %d: tools: remember_offered=y calls=%s prepared=n" % (q, calls)
def rnd(q, r): return V2 + "request %d chat round %d: called keep_rule" % (q, r)
WORDED = ", in other words: LAMP_ACTION_CLAIM_WORDED, a said limit no call carried"
def hold(q, r, worded=True):
    return V2 + ("request %d chat round %d: its reply claims a light kept action no call made (2 call(s) in the turn): "
                 "not said, asked again (LAMP_ACTION_CLAIM_CHECKED%s)" % (q, r, WORDED if worded else ""))
EXIT = "systemd[1]: lamp-voice.service: Main process exited, code=killed, status=6/ABRT"
def j(lines): return "\n".join("@+%.1f %s" % (t, l) for t, l in lines) + "\n"
def plain(q): return j([(0.5, heard(q)), (3.0, rnd(q, 1)), (4.0, tools(q, "keep_rule:2"))])
def held(q, worded=True, after=True):
    L = [(0.5, heard(q)), (3.0, rnd(q, 1)), (3.5, hold(q, 2, worded))]
    if after: L += [(5.0, V2 + "request %d chat round 3: called set_reminder" % q), (6.0, tools(q, "keep_rule:2,set_reminder:1"))]
    return j(L)
FRESH = "FRESH OK pid 812 after 4s\n"
def case(name, expect, tries, legs="W", env="", fresh=FRESH, logs=None):
    d = os.path.join(FX, name); os.makedirs(os.path.join(d, "j")); os.makedirs(os.path.join(d, "out"))
    open(os.path.join(d, "expect"), "w").write(expect + "\n")
    open(os.path.join(d, "case.env"), "w").write('LEGS="%s"\nexport TRIES=%d SETTLE=0 TURNWAIT=3 %s\n' % (legs, max(len(tries), 1), env))
    open(os.path.join(d, "out", "fresh.lamp.sh"), "w").write(fresh)
    for k, chunk in enumerate(tries, 1):
        if chunk is not None: open(os.path.join(d, "j", "W.GO159.%d" % k), "w").write(chunk)
    for f, body in (logs or {}).items():
        os.makedirs(os.path.join(d, "logs"), exist_ok=True); open(os.path.join(d, "logs", f), "w").write(body)
# W: seven fresh tries of GO-159's line
case("W.pass", "PASS", [plain(1), held(1), plain(1), plain(1), held(1), plain(1), plain(1)])
case("W.pass-one-hold", "PASS", [plain(1)] * 6 + [held(1)])
case("W.void-no-claim", "VOID", [plain(1)] * 7)
case("W.void-strict-only", "VOID", [plain(1), held(1, worded=False)] + [plain(1)] * 5)
case("W.void-unheard", "VOID", [plain(1), None, held(1)] + [plain(1)] * 4)
case("W.fail-dropped", "FAIL", [plain(1), held(1, after=False)] + [plain(1)] * 5)
case("W.fail-exit", "FAIL", [plain(1), held(1), j([(0.5, heard(1)), (2.0, EXIT)])] + [plain(1)] * 4)
case("W.fail-fresh", "FAIL", [plain(1)] * 7, fresh="FRESH FAIL (lamp-voice failed, WORDED '')\n")
# E: the switch and the build, from logs/on.txt and logs/check.txt
case("E.pass", "PASS", [], legs="E")
case("E.fail-switch-off", "FAIL", [], legs="E",
     logs={"on.txt": "ENV LAMP_ACTION_CLAIM_WORDED= LAMP_ACTION_CLAIM_CHECKED=on LAMP_NAME_NEAR=off (pid 812)\nSWITCH FAIL\n"})
case("E.fail-checked-off", "FAIL", [], legs="E",
     logs={"on.txt": "ENV LAMP_ACTION_CLAIM_WORDED=on LAMP_ACTION_CLAIM_CHECKED= LAMP_NAME_NEAR=off (pid 812)\nSWITCH FAIL\n"})
# a lamp-voice that isn't #6449's head's: check gives no VERIFY PASS, and row.sh runs no leg (REFUSED)
case("E.refused-wrong-build", "REFUSED", [], legs="E",
     logs={"check.txt": "FAIL lamp-voice runs cd34, not ab12\nVERIFY FAIL\nREADY\n"})
