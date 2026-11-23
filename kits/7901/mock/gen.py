# Fixtures for the #7901 kit's mock: journal chunks (j/<LEG>.<CLIP>[.k], "@+<s> <unit>[pid]: <line>") and lamp-script
# outputs (out/<script>[.<key>]), one folder per case with case.env and expect. Journal lines are the source's own
# text: #7901's head for what it journals now (make_skill_local.rs kept/not kept, control server.rs token/end,
# tasks skill_start.rs egress), origin/lamp-voice for the old net-beside-private refusals the legs must never see.
import os, shutil, sys
FX = sys.argv[1]; shutil.rmtree(FX, ignore_errors=True)
V = "lamp-voice[812]: lamp-voice: "; V2 = "lamp-voice[812]: lamp-voice second: "
C = "lamp-control[640]: lamp-control: "; T = "lamp-tasks[905]: [lamp-tasks] "; S = "lamp-skills[700]: lamp-skills: "
def pieces(req, n): return V2 + "request %d tts: 1 of 1 pieces began, 0 on a reused connection, %d chars; the first piece's first audio byte 310 ms after it went" % (req, n)
def asked(req): return V + "request %d: a skill to make: asked" % req
def kept(ms): return V + "a skill made here was kept in %d ms" % ms
def notkept(ms, why): return V + "a skill made here wasn't kept in %d ms: %s" % (ms, why)
def readback(req): return V + "request %d: a made skill read back: it waits for the person's yes" % req
def yesok(req): return V + "request %d: a made skill with code of its own is on, on the person's yes (#6307); its task approved" % req
def on(nm): return V + "a made skill is on the lamp: %s, sha256 ab12cd34" % nm
def heard(req): return V2 + "request %d heard 9 words" % req
def token(r, sk, g): return C + "run %s of skill %s: a token for [%s], 300 s" % (r, sk, g)
def notoken(r, sk, why): return C + "run %s of skill %s: no token made: %s" % (r, sk, why)
def ended(r, sk): return C + "run %s of skill %s: ended" % (r, sk)
def egress(r): return T + "skill start: run %s's egress ready in its fence: 80 ms after the run's start, 20 ms after its claim" % r
def j(lines): return "\n".join("@+%.1f %s" % (t, l) for t, l in lines) + "\n"
# the old refusals (origin/lamp-voice, before #7901)
NBP = "a skill that reaches the internet can't also use your linked services, the camera, the microphone, what the lamp says or its notes, or know whether someone is there: make it two skills"
OLD_NOTES = "the lamp refused its code: it reaches the internet and also keeps notes, and one skill can't do both: make it two skills"
OLD_CAMERA = "the lamp refused its code: it reaches the internet and also uses the camera, and one skill can't do both: make it two skills"
OLD_CLASS = V + "a skill made here: the model's cannot was not made a task for the lamp's agent: net_private (LAMP_UPLOAD_TO_AGENT)"
SENSING = V + "a skill made here: the model's cannot was not made a task for the lamp's agent: sensing (LAMP_UPLOAD_TO_AGENT)"
NF = "/var/lib/lampos-control/notes/n_65b35631ee38e6d9.json"
def coffee(g="net,speak,notes", rd="yes", nf="none", keys=0, sha="-"):
    return "NEW coffee-price on=True grants=%s reads_notes=%s sets_notes=yes notes=%s keys=%s sha=%s\n" % (g, rd, nf, keys, sha)
def desk(nm="desk-search", g="camera,net,speak"):
    return "NEW %s on=True grants=%s reads_notes=no sets_notes=no notes=none keys=0 sha=-\n" % (nm, g)
FCU = "/system.slice/lamp-tasks.service"
# The voice's kept task ask (skill_task_yes.rs ASKED_FILE, serde_json to_string: no spaces). SAYBACK_OLD is f162be0fcb's
# words for camera + net (no line on what can go out); SAYBACK_NEW adds the line (#7901's head generalises AIR_BESIDE_NET).
SAYBACK_OLD = ("Photo search hands one task to the lamp's agent: \u201cTake one camera picture and look up online what it shows.\u201d. "
               "Its script comes from your computer Mac, and it uses the camera and reaches the public internet. It runs only once you agree.")
SAYBACK_NEW = SAYBACK_OLD.replace("public internet.", "public internet, so what the camera sees can go to the internet.")
def asked_file(sb, skill="photo-search"):
    import json as _j
    return _j.dumps({"at_unix_ms": 1795000000000, "grants": ["camera", "net"], "note": "(Nobody has spoken. A skill on this lamp waits for the owner's yes before its task runs. Call nothing now.) " + sb,
                     "say_back": sb, "sha256": "ab" * 32, "skill": skill}, separators=(",", ":"), ensure_ascii=False)
def fence(pid=4501, admin="hidden", eg=True, cg="run-d1", seen=True):
    s = "EGRESS-AT-START \n"
    if seen:
        s += "RUNPID %d under_lamp_tasks=yes python3 /var/lib/lampOS/skills/desk-search/scripts/main.py\n" % pid
        s += "ADMIN %d admin_sock=%s ctl_dir=[run.sock ]\n" % (pid, admin)
        s += "NET %d links=lo, sockfds=2 fd5=%s\n" % (pid, "socket:[88123]" if eg else "-")
        s += "CG %d %s/%s unit=%s\n" % (pid, FCU, cg, FCU) if cg else ""
    if eg: s += "EGRESS +run-d1.sock at 09:00:02\nEGRESSCONN 1 at 09:00:02\n"
    return s + "FENCE-WATCH END 09:01:00\n"
BASE = {
 # A: the coffee make, its yes, on
 "j/A.MAKEA": j([(1, asked(101)), (40, kept(39012)), (40.5, readback(102)), (41, pieces(102, 140))]),
 "j/A.YES": j([(1, yesok(103)), (1.5, on("coffee-price"))]),
 "out/state.lamp.sh.A": "STAGE A\n" + coffee(),
 # B: two runs of coffee-price; notes file after run 1, read back at run 2
 "j/B.RUN.1": j([(1, heard(111)), (1.2, egress("task-r1")), (1.3, token("task-r1", "coffee-price", "speak, notes")),
                 (4, ended("task-r1", "coffee-price")), (4.2, pieces(112, 60))]),
 "j/B.RUN.2": j([(1, heard(113)), (1.2, egress("task-r2")), (1.3, token("task-r2", "coffee-price", "speak, notes")),
                 (4, ended("task-r2", "coffee-price")), (4.2, pieces(114, 70))]),
 "out/state.lamp.sh.B1": "STAGE B1\n" + coffee(nf=NF, keys=1, sha="aaaa1111") + 'NOTESJSON coffee-price {"version":1,"notes":{"prices":[12.5]}}\n',
 "out/state.lamp.sh.B2": "STAGE B2\n" + coffee(nf=NF, keys=1, sha="bbbb2222") + 'NOTESJSON coffee-price {"version":1,"notes":{"prices":[12.5,12.9]}}\n',
 # C (flipped): the desk make is kept with net AND camera
 "out/state.lamp.sh.C0": "STAGE C0\n" + coffee(nf=NF, keys=1, sha="bbbb2222"),
 "j/C.MAKEC": j([(1, asked(121)), (45, kept(44100)), (45.5, readback(122)), (46, pieces(122, 150))]),
 "j/C.YES": j([(1, yesok(123)), (1.5, on("desk-search"))]),
 "out/state.lamp.sh.C": "STAGE C\n" + coffee(nf=NF, keys=1, sha="bbbb2222") + desk(),
 # D: the desk skill's run, watched from outside its fence
 "j/D.RUNC": j([(1, heard(131)), (1.2, egress("task-d1")), (1.3, token("task-d1", "desk-search", "speak, camera")),
                (5, ended("task-d1", "desk-search")), (5.2, pieces(132, 80))]),
 "out/fence.lamp.sh.D": fence(),
 # S: the pushed photo-search (camera + net): asked aloud with the can-go-out line, NO, asked again, YES, its run
 "out/push.photo-search": 'PUSHED {"asks": ["camera", "net"], "name": "photo-search", "on": true, "version": "1.0.0"}\n',
 "out/asked.lamp.sh.askS": "ASKED " + asked_file(SAYBACK_NEW) + "\nASK-WATCH END 09:10:00\n",
 "j/S.RUNS.1": j([(1, heard(141)), (1.5, S + "skill:photo-search: do.task: not handed over: waiting for the owner's yes (the lamp says it back and asks); asked aloud: say: spoken"),
                  (2, V + "a skill's task with no yes is said back unasked (2 grant(s))"), (3, V + "request 142: a skill's task said back unasked waits for the owner's yes"), (3.2, pieces(142, 230))]),
 "j/S.NO": j([(1, V + "request 143: the answer to photo-search's say-back was no: nothing kept, it stays off"), (1.5, pieces(143, 40))]),
 "j/S.USES": j([(1, heard(144)), (2, V + "request 144: a skill's task read back: it waits for the person's yes (2 grant(s))"), (2.2, pieces(144, 230))]),
 "j/S.YES": j([(1, C + "the owner's yes to photo-search's task kept (2 grant(s))"), (1.2, V + "request 145: a skill's task approved on the person's yes (#6305)"), (1.5, pieces(145, 50))]),
 "j/S.RUNS.2": j([(1, heard(146)), (1.2, S + "skill:photo-search: do.task: handed to lamp-tasks: run task-s1"),
                  (1.3, T + "skill task task-s1: granted net: its egress socket task-s1.sock made for the lamp's default agent (openclaw)"),
                  (1.4, token("task-s1", "photo-search", "camera")), (3, C + "c_9a1b2c3d (photo-search): notes.get: refused: no_grant, 0.1 ms"),
                  (5, ended("task-s1", "photo-search")), (5.2, pieces(147, 70))]),
 # K: the pushed key-probe: its yes, then its run refused for a key never added (tasks skill_task.rs run_keys)
 # (its install: push.sh's lamp side, push.lamp.sh, the manage helper's install_pushed; no pairing, no push.py)
 "out/push.lamp.sh.key-probe": 'HELPER {"done": "installed key-probe"}\nPUSHED {"done": "installed key-probe"}\nPUSH INSTALLED\n',
 "j/K.KEYUSES": j([(1, heard(151)), (2, V + "request 151: a skill's task read back: it waits for the person's yes (1 grant(s))"), (2.2, pieces(151, 200))]),
 "j/K.YES": j([(1, C + "the owner's yes to key-probe's task kept (1 grant(s))"), (1.5, pieces(152, 40))]),
 "j/K.KEYRUN": j([(1, heard(153)), (1.5, T + "skill task: key-probe not run: 1 key(s) not added in the app"), (2, pieces(154, 90))]),
 # run.sh's other steps
 "out/check.lamp.sh": "BIN lamp-voice running=1111 file=1111 (/opt/lampOS/voice/releases/x/lamp-voice) dropins=\nVERIFY PASS (all four parts = the given PR shas)\nREADY\n",
 "out/notes.lamp.sh.on": "NOTES lamp-voice LAMP_MADE_NOTES='on' active=active\nNOTES lamp-control LAMP_MADE_NOTES='on' active=active\nNOTES ON OK (restarted: lamp-voice lamp-control)\n",
 "out/micon.lamp.sh": "VOICE_MUTE muted=True reason=person age_s=1.0 seq=4\nMUTE LIFTED OK after 4s\n",
 "out/micoff.lamp.sh": "VOICE_MUTE muted=True reason=person age_s=1.0 seq=9\nMUTE BACK OK after 4s\n",
 "out/mute.lamp.sh": "MUTE OFF OK\n",
 "out/save.lamp.sh": "ENGINE openclaw\nBASELINE made: / skills 3 / notes files 1\nCLASH none\nNAMENEAR off (row drop-in)\nSAVED OK\n",
 "out/unrow.lamp.sh": "DROPINS removed; restarted: lamp-voice lamp-control\nSKILL made by the row, removed: coffee-price (exit 0: removed)\nSKILL made by the row, removed: desk-search (exit 0: removed)\nPASS skills = save's list\nPASS lamp-cloud active\nUNROW OK\n",
}
n = 0
def case(name, legs, expect, over=None, drop=(), env=None):
    global n; n += 1
    d = os.path.join(FX, name); fx = dict(BASE)
    for k in drop: fx.pop(k, None)
    fx.update(over or {})
    for k, v in fx.items():
        p = os.path.join(d, k); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(v)
    e = {"LEGS": legs}; e.update(env or {})
    open(os.path.join(d, "case.env"), "w").write("".join("%s=%s\n" % (k, "'%s'" % v) for k, v in e.items()))
    open(os.path.join(d, "expect"), "w").write(expect + "\n")
# ---- A
case("A.pass", "A", "PASS")
case("A.fail-oldrefusal", "A", "FAIL", {"j/A.MAKEA": j([(1, asked(101)), (20, S + 'grants: "net" beside "notes": ' + NBP), (21, notkept(20500, OLD_NOTES)), (22, pieces(102, 90))])},
     drop=["out/state.lamp.sh.A"])
case("A.fail-nonotes", "A", "FAIL", {"out/state.lamp.sh.A": "STAGE A\n" + coffee(g="net,speak")})
case("A.fail-noton", "A", "FAIL", {"j/A.YES": j([(1, yesok(103))])})
case("A.void-unheard", "A", "VOID", drop=["j/A.MAKEA", "j/A.YES", "out/state.lamp.sh.A"])
case("A.void-nomakeend", "A", "VOID", {"j/A.MAKEA": j([(1, asked(101))])}, drop=["out/state.lamp.sh.A"])
# ---- B (after A)
case("B.pass", "A B", "PASS")
case("B.fail-tokennonotes", "A B", "FAIL", {"j/B.RUN.1": BASE["j/B.RUN.1"].replace("[speak, notes]", "[speak]")})
case("B.fail-notoken", "A B", "FAIL", {"j/B.RUN.1": j([(1, heard(111)), (1.3, notoken("task-r1", "coffee-price", "grants: net beside notes: " + NBP)), (2, pieces(112, 40))])})
case("B.fail-refused", "A B", "FAIL", {"j/B.RUN.2": BASE["j/B.RUN.2"].replace("@+4.0 ", "@+3.0 " + C + "task-r2: notes.get: refused: no_grant\n@+4.0 ", 1)})
case("B.fail-noread", "A B", "FAIL", {"out/state.lamp.sh.B2": BASE["out/state.lamp.sh.B2"].replace("reads_notes=yes", "reads_notes=no")})
case("B.fail-nonotesfile", "A B", "FAIL", {"out/state.lamp.sh.B1": "STAGE B1\n" + coffee()})
case("B.void-noskill", "B", "VOID")
case("B.void-unheard", "A B", "VOID", drop=["j/B.RUN.1", "j/B.RUN.2"])
# ---- C (flipped)
case("C.fail-asked", "C", "FAIL")   # kept with net+camera, but on the yes path: First's 16:06 line 2 wants nothing asked
case("C.fail-oldrefusal", "C", "FAIL", {"j/C.MAKEC": j([(1, asked(121)), (30, notkept(30000, OLD_CAMERA)), (31, pieces(122, 90))]),
     "out/state.lamp.sh.C": BASE["out/state.lamp.sh.C0"].replace("C0", "C")})
case("C.fail-oldclass", "C", "FAIL", {"j/C.MAKEC": j([(1, asked(121)), (20, OLD_CLASS), (21, notkept(20100, "the lamp's model said no made skill can do it")), (22, pieces(122, 120))]),
     "out/state.lamp.sh.C": BASE["out/state.lamp.sh.C0"].replace("C0", "C")})
case("C.fail-split", "C", "FAIL", {"out/state.lamp.sh.C": BASE["out/state.lamp.sh.C"].replace(desk(), desk("desk-look", "camera,speak") + desk("desk-search", "net,speak"))})
case("C.fail-splittext", "C", "FAIL", {"j/C.MAKEC": BASE["j/C.MAKEC"].replace("@+45.5 ", "@+44.0 " + V2 + "request 121: said: it can look or search, so make it two skills\n@+45.5 ", 1)})
case("C.fail-nocamera", "C", "FAIL", {"out/state.lamp.sh.C": BASE["out/state.lamp.sh.C"].replace(desk(), desk(g="net,speak"))})
case("C.void-nomakeend", "C", "VOID", {"j/C.MAKEC": j([(1, asked(121))])}, drop=["out/state.lamp.sh.C"])
case("C.void-sensing", "C", "VOID", {"j/C.MAKEC": j([(1, asked(121)), (20, SENSING), (21, notkept(20100, "the lamp's model said no made skill can do it")), (22, pieces(122, 120))]),
     "out/state.lamp.sh.C": BASE["out/state.lamp.sh.C0"].replace("C0", "C")})
# ---- D (after C)
case("D.pass", "C D", "PASS")
case("D.fail-adminvisible", "C D", "FAIL", {"out/fence.lamp.sh.D": fence(admin="visible")})
case("D.fail-nocamera", "C D", "FAIL", {"j/D.RUNC": BASE["j/D.RUNC"].replace("[speak, camera]", "[speak]")})
case("D.fail-noegress", "C D", "FAIL", {"out/fence.lamp.sh.D": fence(eg=False), "j/D.RUNC": BASE["j/D.RUNC"].replace("@+1.2 " + egress("task-d1") + "\n", "")})
case("D.fail-refused", "C D", "FAIL", {"j/D.RUNC": BASE["j/D.RUNC"].replace("@+5.0 ", "@+3.0 " + C + "task-d1: camera.snapshot: refused: no_grant\n@+5.0 ", 1)})
case("D.fail-cgroupserve", "C D", "FAIL", {"out/fence.lamp.sh.D": fence(cg="serve")})
case("D.void-nopid", "C D", "VOID", {"out/fence.lamp.sh.D": fence(seen=False, eg=False)})
case("D.void-otherskill", "C D", "VOID", {"j/D.RUNC": BASE["j/D.RUNC"].replace("desk-search", "coffee-price")})
case("D.void-noskill", "C D", "VOID", drop=["j/C.MAKEC", "j/C.YES", "out/state.lamp.sh.C"])
case("D.void-private", "C D", "VOID", {"j/D.RUNC": BASE["j/D.RUNC"].replace("@+5.0 ", "@+3.0 " + C + "task-d1: camera.snapshot: refused: private\n@+5.0 ", 1)})
# ---- run.sh end to end (lock .. release)
case("RUN.fail-stop-muteback", "A B C D S K", "FAIL", {"out/save.lamp.sh": "SAVE STOP: lamp-update round under way (journal.json): wait\n"}, env={"MIC_YES": "yes", "RUNSH": "1"})
case("RUN.fail-legfail-unrowed", "A B C D S K", "FAIL", {"j/C.MAKEC": j([(1, asked(121)), (30, notkept(30000, OLD_CAMERA)), (31, pieces(122, 90))])}, env={"MIC_YES": "yes", "RUNSH": "1"})
case("RUN.void-noyes", "A B C D S K", "VOID", env={"RUNSH": "1"})
case("RUN.void-checksha", "A B C D S K", "VOID", {"out/check.lamp.sh": "BIN lamp-voice running=2222 file=2222\nVERIFY FAIL: lamp-voice-not-the-given-PR-sha\nREADY\n"}, env={"MIC_YES": "yes", "RUNSH": "1"})
case("RUN.void-budget", "A B C D S K", "VOID", env={"MIC_YES": "yes", "RUNSH": "1", "MOCK_STARTS": "9"})
# ---- #8180's on-at-once make (LAMP_MADE_CODE_ON_AT_ONCE on in the product): no read-back, no yes; the try's say is
# answered at once (said 1) and said in the tried line, journaled by its length only (make_try.rs, made_skill.rs).
# Playing YES there is wrong: these cases' YES chunk holds the old refusal, so a YES played FAILs the leg.
def tryline(code=0, said=1): return T + "make try: scripts/main.py: exit %s, 30 s limit, said %s" % (code, said)
def atonce(task="kept"): return V + "a made skill with code of its own is on at once (LAMP_MADE_CODE_ON_AT_ONCE); its task %s" % task
def saidon(n=118, uses=True): return V + "a made skill is said on with its try (%d chars, %s)" % (n, "uses named" if uses else "no uses named")
YESBAD = j([(1, V + "request 199: a yes played in an at-once make: " + NBP)])
def made_at_once(req, ms, nm, task="kept", tl=None, so=None, on_=True):
    tl = tryline() if tl is None else tl; so = saidon() if so is None else so
    ls = [(1, asked(req))] + ([(20, tl)] if tl else []) + [(30, atonce(task))] + ([(30.2, on(nm))] if on_ else []) + ([(30.4, so)] if so else [])
    return j(ls + [(31, kept(ms)), (31.5, pieces(req + 1, 140))])
AA = {"j/A.MAKEA": made_at_once(101, 30500, "coffee-price"), "j/A.YES": YESBAD}
CA = {"j/C.MAKEC": made_at_once(121, 30500, "desk-search"), "j/C.YES": YESBAD}
def over(base, **kw): d = dict(base); d.update(kw); return d
case("A.pass-atonce", "A", "PASS", AA)
case("A.fail-atonce-tasknotkept", "A", "FAIL", over(AA, **{"j/A.MAKEA": made_at_once(101, 30500, "coffee-price", task="not kept")}))
case("A.fail-atonce-nosaidon", "A", "FAIL", over(AA, **{"j/A.MAKEA": made_at_once(101, 30500, "coffee-price", so="")}))
case("A.fail-atonce-nouses", "A", "FAIL", over(AA, **{"j/A.MAKEA": made_at_once(101, 30500, "coffee-price", so=saidon(90, False))}))
case("A.fail-atonce-trysaid0", "A", "FAIL", over(AA, **{"j/A.MAKEA": made_at_once(101, 30500, "coffee-price", tl=tryline(0, 0))}))
case("A.fail-atonce-noon", "A", "FAIL", over(AA, **{"j/A.MAKEA": made_at_once(101, 30500, "coffee-price", on_=False)}))
case("A.pass-yes-try", "A", "PASS", {"j/A.MAKEA": BASE["j/A.MAKEA"].replace("@+40.0 ", "@+20.0 " + tryline() + "\n@+40.0 ", 1)})
case("A.fail-yes-tryexit1", "A", "FAIL", {"j/A.MAKEA": BASE["j/A.MAKEA"].replace("@+40.0 ", "@+20.0 " + tryline(1, 0) + "\n@+40.0 ", 1)})
case("C.pass-atonce", "C", "PASS", CA)
case("C.fail-atonce-nosaidon", "C", "FAIL", over(CA, **{"j/C.MAKEC": made_at_once(121, 30500, "desk-search", so="")}))
case("D.pass-atonce", "C D", "PASS", CA)
# RUN.pass: A on the yes path, C at once (C6: nothing asked), S and K pushed
case("RUN.pass", "A B C D S K", "PASS", dict(CA), env={"MIC_YES": "yes", "RUNSH": "1"})
case("RUN.pass-atonce", "A B C D S K", "PASS", dict(AA, **CA), env={"MIC_YES": "yes", "RUNSH": "1"})
# ---- S: a pushed camera+net skill (First's 16:06 line 3; the Reviewer-lead's ask on #7901, 16:04)
SNO_KEPT = j([(1, V + "request 143: the answer to photo-search's say-back was no: nothing kept, it stays off"), (1.2, C + "the owner's yes to photo-search's task kept (2 grant(s))"), (1.5, pieces(143, 40))])
case("S.pass", "S", "PASS")
case("S.fail-keptonno", "S", "FAIL", {"j/S.NO": SNO_KEPT})
case("S.fail-noline", "S", "FAIL", {"out/asked.lamp.sh.askS": "ASKED " + asked_file(SAYBACK_OLD) + "\n"})
case("S.fail-token", "S", "FAIL", {"j/S.RUNS.2": BASE["j/S.RUNS.2"].replace("a token for [camera]", "a token for [speak, camera]")})
case("S.fail-noegress", "S", "FAIL", {"j/S.RUNS.2": "".join(l + "\n" for l in BASE["j/S.RUNS.2"].splitlines() if "egress" not in l)})
case("S.void-held", "S", "VOID", {"j/S.YES": j([(1, V2 + "request 145: its answer is held back: nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on")])})
case("S.void-nopush", "S", "VOID", drop=["out/push.photo-search"])
case("S.void-later", "S", "VOID", {"j/S.RUNS.1": j([(1, heard(141)), (1.5, S + "skill:photo-search: do.task: not handed over: waiting for the owner's yes (the lamp says it back and asks); asked aloud: say: refused (its read-back can't be said whole here (the owner isn't seen alone): kept for later)")])})
# ---- K: a pushed skill whose key was never added (First's 16:06 line 4: keys refused)
case("K.pass", "K", "PASS")
case("K.fail-token", "K", "FAIL", {"j/K.KEYRUN": j([(1, heard(153)), (1.4, token("task-k1", "key-probe", "speak")), (3, ended("task-k1", "key-probe")), (3.2, pieces(154, 30))])})
case("K.void-nopush", "K", "VOID", drop=["out/push.lamp.sh.key-probe"])
case("K.void-helperrefused", "K", "VOID", {"out/push.lamp.sh.key-probe": 'HELPER {"refused": "not installed: a bad package"}\nPUSH REFUSED {"refused": "not installed: a bad package"}\nPUSH NOT INSTALLED\n'})
# ---- kit helpers alone (unit.sh), the fixes ported from the #8180 e82d kit: engine(), the mute check, SSHR; Mac volume
for u in ("engine-env", "engine-voice", "engine-app", "engine-none", "mute-oldstamp", "mute-timed", "mute-recoff", "mute-live",
          "mute-liveoff", "ssh-retry", "ssh-sshpass", "ssh-giveup", "ssh-other", "macvol", "fixtures", "namenear", "push-k"):
    case("U." + u, "-", "PASS", env={"UNIT": u})
print("gen: %d cases in %s" % (n, FX))
