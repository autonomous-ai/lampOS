#!/usr/bin/env python3
# Fixtures for the ee17 gate kit's mock, one folder per case (case.env, expect, files). Three kinds:
#   E.*  gate/eval.py after: a round's output + after.lamp.sh's facts -> PASS | FINDING | FAIL (each check both ways)
#   P.*  gate/eval.py peek / peekcmp: #8417's numbers -> PASS | FAIL (or the computed line)
#   R.*  ../run.sh end to end with stub ssh, lock, afplay, osascript: out/<lamp script>.<key>[.<n>] and j/<clip>
#        journal chunks -> PASS (GATE ... PASS) | FAIL (GATE ... FAIL) | STOP:<step>
# Lines are the source's own text: lamp-update's Outcome Display (round.rs), systemd's unit lines (#8429's raw).
import base64, json, os, shutil, sys
FX = sys.argv[1]; shutil.rmtree(FX, ignore_errors=True)
STABLE, CAND, NEXT = "15759", "15860", "15866"
SUPD = "b213d9041a88" + "1" * 52; CUPD = "e00e49d4785d" + "2" * 52; NUPD = CUPD                              # 15866 carries 15860's lamp-update (First, 12:29)
BAD = "f00f00f00f00" + "3" * 52              # a lamp-update no manifest names
T0 = 1795455800
def case(name, files, expect, env=""):
    d = os.path.join(FX, name); os.makedirs(d)
    for k, v in files.items():
        p = os.path.join(d, k); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(v)
    open(os.path.join(d, "expect"), "w").write(expect + "\n"); open(os.path.join(d, "case.env"), "w").write(env)
def rnd(seq, upd_before, exit_="0", lines=None, t0=T0):
    out = lines if lines is not None else [
        "lamp-update: build %s: fetching 9 files" % seq,
        "lamp-update: build %s runs; lamp-update switches from its build %s to %s, which must confirm itself" % (seq, upd_before[:12], "e00e49d4785d"),
        "lamp-update: confirmed: lamp-update runs its new build", "up to date: build %s" % seq]
    return "T0 %d\nWATCH started pid 4100\nUPD before %s (/opt/lampOS/update/%s/lamp-update)\nENV LAMP_UPDATE_SELF=on\nCMD lamp-update once\n--- OUTPUT\n%s\n--- END\nEXIT %s\nUPD after x\nSTATE installed=%s\n" % (
        t0, upd_before, upd_before, "\n".join(out), exit_, seq)
UNITS = ["lamp-voice.service", "lamp-skills.service", "lamp-control.service", "lamp-tasks.service", "lamp-cloud.service",
         "lamp-openclaw-gateway.service", "lamp-update.service"]
def facts(seq, prev, upd, active="active", listening="yes", partof="lamp-control.service", timeout="2min 30s",
          ino=("4711", "4711"), ns="own", gwj=None, gww=None, units=None, failed="", now=T0 + 400):
    s = "STATE installed=%s previous=%s refused=none busy=''\nUPD %s\n" % (seq, prev, upd)
    s += "GW active=%s listening=%s partof='%s' timeout='%s' nrestarts=0\n" % (active, listening, partof, timeout)
    s += "INODE host=%s gateway=%s pid=5120 ns=%s path=/run/lampos-control/control.sock\n" % (ino[0], ino[1], ns)
    for l in (gwj if gwj is not None else ["%d.10 lamp systemd[1]: Started lamp-openclaw-gateway.service - lampOS OpenClaw gateway." % (T0 + 30)]):
        s += "GWJ %s\n" % l
    for l in (gww if gww is not None else ["%d LISTEN" % T0]): s += "GWW %s\n" % l
    s += "NOW %d\n" % now
    for u in (units if units is not None else ["%s active=active sub=running result=success type=simple" % u for u in UNITS]):
        s += "UNIT %s\n" % u
    return s + "FAILED %s\n" % failed
def ecase(name, expect, r=None, f=None, seq=CAND, prev=STABLE, msha=CUPD, prefix="e00e49d4785d"):
    case(name, {"round.txt": r or rnd(CAND, SUPD), "facts.txt": f or facts(CAND, STABLE, CUPD)}, expect,
         "KIND=after\nARGS='%s %s %s %s'\n" % (seq, prev, msha, prefix))
EXIT1 = "lamp systemd[1]: lamp-openclaw-gateway.service: Main process exited, code=exited, status=1/FAILURE"
# ---- E: the after-round checks
ecase("E.pass", "PASS")
ecase("E.fail-installed", "FAIL", f=facts(STABLE, STABLE, CUPD))
ecase("E.fail-previous", "FAIL", f=facts(CAND, "15700", CUPD))
ecase("E.fail-exit1", "FAIL", r=rnd(CAND, SUPD, exit_="1"))
ecase("E.fail-refused", "FAIL", r=rnd(CAND, SUPD, lines=["refused, nothing switched: build 15860: lamp-voice did not start", "up to date: build %s" % CAND]))
ecase("E.fail-putting", "FAIL", r=rnd(CAND, SUPD, lines=["lamp-update: putting build 15860 back: lamp-voice not well", "up to date: build %s" % CAND]))
ecase("E.fail-outcome", "FAIL", r=rnd(CAND, SUPD, lines=["switched to build 15860; every part is well"]))
ecase("E.fail-nooutput", "FAIL", r="T0 %d\nEXIT 0\n" % T0)
ecase("E.fail-digest", "FAIL", f=facts(CAND, STABLE, BAD))
ecase("E.fail-digest-prefix", "FAIL", msha=BAD, f=facts(CAND, STABLE, BAD))
ecase("E.fail-digest-none", "FAIL", msha="none")
ecase("E.fail-inode", "FAIL", f=facts(CAND, STABLE, CUPD, ino=("4711", "4690")))
ecase("E.fail-inode-none", "FAIL", f=facts(CAND, STABLE, CUPD, ino=("4711", "none")))
ecase("E.pass-inode-hostns", "PASS", f=facts(CAND, STABLE, CUPD, ns="host"))
ecase("E.fail-gw-inactive", "FAIL", f=facts(CAND, STABLE, CUPD, active="activating"))
ecase("E.fail-gw-notlistening", "FAIL", f=facts(CAND, STABLE, CUPD, listening="no"))
ecase("E.fail-partof", "FAIL", f=facts(CAND, STABLE, CUPD, partof=""))
ecase("E.fail-timeout", "FAIL", f=facts(CAND, STABLE, CUPD, timeout="1min 30s"))
ecase("E.fail-kill", "FAIL", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 lamp systemd[1]: lamp-openclaw-gateway.service: Main process exited, code=killed, status=9/KILL" % (T0 + 40)]))
ecase("E.fail-stop-timeout", "FAIL", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 lamp systemd[1]: lamp-openclaw-gateway.service: State 'stop-sigterm' timed out. Killing." % (T0 + 40)]))
ecase("E.pass-benign-timeout-word", "PASS", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 lamp lamp-openclaw-gateway[1131527]: [gateway] Unable to read systemd stop timeout; system manager lamp-openclaw-gateway.service: systemctl show exited 1" % (T0 + 40)]))
ecase("E.finding-exit1-within", "FINDING", f=facts(CAND, STABLE, CUPD, gwj=["%d.04 %s" % (T0 + 98, EXIT1)], gww=["%d LISTEN" % T0, "%d DOWN" % (T0 + 85), "%d LISTEN" % (T0 + 135)]))
ecase("E.fail-exit1-late", "FAIL", f=facts(CAND, STABLE, CUPD, gwj=["%d.04 %s" % (T0 + 98, EXIT1)], gww=["%d LISTEN" % T0, "%d DOWN" % (T0 + 85), "%d LISTEN" % (T0 + 220)]))
ecase("E.fail-exit1-twice", "FAIL", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 %s" % (T0 + 98, EXIT1), "%d.0 %s" % (T0 + 130, EXIT1)], gww=["%d LISTEN" % T0, "%d LISTEN" % (T0 + 150)]))
ecase("E.fail-exit1-never", "FAIL", f=facts(CAND, STABLE, CUPD, listening="no", gwj=["%d.0 %s" % (T0 + 98, EXIT1)], gww=["%d LISTEN" % T0, "%d DOWN" % (T0 + 85)]))
ecase("E.finding-exit1-nowatch", "FINDING", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 %s" % (T0 + 98, EXIT1)], gww=[], now=T0 + 150))
ecase("E.fail-exit1-nowatch-old", "FAIL", f=facts(CAND, STABLE, CUPD, gwj=["%d.0 %s" % (T0 + 98, EXIT1)], gww=[], now=T0 + 400))
U = ["%s active=active sub=running result=success type=simple" % u for u in UNITS]
ecase("E.fail-unit-failed", "FAIL", f=facts(CAND, STABLE, CUPD, units=U[:2] + ["lamp-control.service active=failed sub=failed result=exit-code type=simple"] + U[3:]))
ecase("E.fail-unit-inactive", "FAIL", f=facts(CAND, STABLE, CUPD, units=U + ["lamp-region.service active=inactive sub=dead result=exit-code type=oneshot"]))
ecase("E.pass-unit-off-by-switch", "PASS", f=facts(CAND, STABLE, CUPD, units=U + ["lamp-mood.service active=inactive sub=dead result=success type=simple"]))
ecase("E.fail-unit-simple-crashed", "FAIL", f=facts(CAND, STABLE, CUPD, units=U + ["lamp-mood.service active=inactive sub=dead result=exit-code type=simple"]))
ecase("E.pass-unit-oneshot", "PASS", f=facts(CAND, STABLE, CUPD, units=U + ["lamp-region.service active=inactive sub=dead result=success type=oneshot"]))
ecase("E.fail-failed-list", "FAIL", f=facts(CAND, STABLE, CUPD, failed="lamp-update-back.service"))
ecase("E.fail-no-units", "FAIL", f=facts(CAND, STABLE, CUPD, units=[]))
ecase("E.pass-round2", "PASS", r=rnd(NEXT, CUPD), f=facts(NEXT, CAND, NUPD), seq=NEXT, prev=CAND, msha=NUPD, prefix="e00e49d4785d")
# ---- P: #8417's read
def pk(label, ib, ic, tb, tc): return "PEEK %s idle bytes=%d cpu=%.2f task bytes=%d cpu=%.2f\n" % (label, ib, ic, tb, tc)
def pcase(name, expect, b, a): case(name, {"before.txt": b, "after.txt": a}, expect, "KIND=peekcmp\n")
pcase("P.pass", "PASS", pk("before", 90000, 1.5, 240000, 3.2), pk("after", 30000, 1.1, 120000, 3.2))
pcase("P.fail-cpu-idle", "FAIL", pk("before", 90000, 1.5, 240000, 3.2), pk("after", 30000, 1.6, 120000, 3.0))
pcase("P.fail-cpu-task", "FAIL", pk("before", 90000, 1.5, 240000, 3.2), pk("after", 30000, 1.0, 120000, 3.3))
pcase("P.fail-bytes-idle", "FAIL", pk("before", 90000, 1.5, 240000, 3.2), pk("after", 90000, 1.0, 120000, 3.0))
pcase("P.fail-bytes-task", "FAIL", pk("before", 90000, 1.5, 240000, 3.2), pk("after", 30000, 1.0, 250000, 3.0))
pcase("P.fail-missing", "FAIL", pk("before", 90000, 1.5, 240000, 3.2), "")
def ms(label, b, t0, t1, ps="none", p2="777"): return "MEASURE %s bytes=%d ticks0=%d ticks1=%d clk=100 secs=60 pid=777 pid_after=%s pidstat=%s\n" % (label, b, t0, t1, p2, ps)
case("P.calc-proc", {"idle.txt": ms("idle", 5000, 100, 160), "task.txt": ms("task", 9000, 200, 500)}, "PEEK x idle bytes=5000 cpu=1.00 task bytes=9000 cpu=5.00", "KIND=peek\n")
case("P.calc-pidstat", {"idle.txt": ms("idle", 5000, 100, 160, ps="0.75"), "task.txt": ms("task", 9000, 200, 500, ps="4.90")}, "PEEK x idle bytes=5000 cpu=0.75 task bytes=9000 cpu=4.90", "KIND=peek\n")
# ---- R: run.sh end to end
def env_(m): return json.dumps({"format": "lampos-update/1", "key": "0" * 16, "manifest": base64.b64encode(json.dumps(m).encode()).decode(), "signature": "AA=="})
def man(seq, upd): return env_({"schema": 6, "channel": "lamp-voice", "sequence": int(seq), "commit": "c" * 40, "files_url": "x", "units": [],
                                "files": [{"name": "lamp-voice", "sha256": "9" * 64, "size": 1}, {"name": "lamp-update", "sha256": upd, "size": 1}]})
REL = {"rel/lampos-lamp-voice-%s/lampos-manifest.json" % s: man(s, u) for s, u in ((STABLE, SUPD), (CAND, CUPD), (NEXT, NUPD))}
def rd(inst, upd, prev="15700", ref="none", busy="", binds=(), dropins=()):
    s = "STATE installed=%s previous=%s refused=%s busy='%s'\nUPD %s\n" % (inst, prev, ref, busy, upd)
    return s + "".join("BIND %s /x\n" % b for b in binds) + "".join("DROPIN %s\n" % d for d in dropins)
R = "restore.lamp.sh.read."
PEEKOK = {"out/peek.lamp.sh.on": "PEEK ON OK monitor (drop-in was absent, lamp-peek was inactive); POST /in http=303, cookie kept (not printed)\n",
          "out/peek.lamp.sh.measure-idle.1": ms("idle", 90000, 100, 190), "out/peek.lamp.sh.measure-task.1": ms("task", 240000, 300, 500),
          "out/peek.lamp.sh.measure-idle.2": ms("idle", 30000, 100, 160), "out/peek.lamp.sh.measure-task.2": ms("task", 120000, 300, 480),
          "out/peek.lamp.sh.off": "PEEK BACK OK (drop-in absent, lamp-peek inactive, was inactive)\n",
          "out/peek.lamp.sh.check": "PEEK BACK OK (nothing laid by the gate)\n"}
V = "lamp-voice[812]: lamp-voice: "; V2 = "lamp-voice[812]: lamp-voice second: "
def pieces(req, n): return V2 + "request %d tts: 1 of 1 pieces began, 0 on a reused connection, %d chars" % (req, n)
JRN = {"j/TIME": "@+1 %srequest 201 heard 5 words\n@+2 %s\n" % (V2, pieces(201, 30)),
       "j/TASK": "@+1 %srequest 202 heard 9 words\n@+2 %srequest 202: a read of weather handed to the lamp's agent\n@+20 lamp-tasks[905]: [lamp-tasks] skill start: run task-w1's script started\n@+21 lamp-control[640]: lamp-control: run task-w1 of skill weather: a token for [speak], 300 s\n@+30 lamp-control[640]: lamp-control: run task-w1 of skill weather: ended\n" % (V2, V),
       "j/MAKE": "@+1 %srequest 203 heard 14 words\n@+40 %sa made skill with code of its own is on at once (LAMP_MADE_CODE_ON_AT_ONCE); its task kept\n@+40.2 %sa skill made here was kept in 39012 ms\n@+41 %sa made skill is on the lamp: days-left, sha256 ab12cd34\n" % (V2, V, V, V)}
def base(**over):
    f = dict(REL); f.update(PEEKOK); f.update(JRN)
    f.update({"out/vol.lamp.sh": "VOL kept=25 bus=25 muted=false\nMUTE live muted=False reason=user\n",
      "out/" + R + "1": rd(STABLE, SUPD, dropins=["/run/systemd/system/lamp-update.service.d/zz-row-7901-hold.conf"]),
      "out/restore.lamp.sh.clean": "REMOVED /run/systemd/system/lamp-update.service.d/zz-row-7901-hold.conf\nRESTARTED lamp-update.service\nCLEAN OK\n",
      "out/" + R + "2": rd(STABLE, SUPD), "out/" + R + "3": rd(STABLE, SUPD), "out/" + R + "4": rd(STABLE, SUPD),
      "out/round.lamp.sh.round1": rnd(CAND, SUPD), "out/after.lamp.sh.round1": facts(CAND, STABLE, CUPD),
      "out/" + R + "5": rd(CAND, CUPD, prev=STABLE),
      "out/round.lamp.sh.round2": rnd(NEXT, CUPD), "out/after.lamp.sh.round2": facts(NEXT, CAND, NUPD),
      "out/" + R + "6": rd(NEXT, NUPD, prev=CAND), "out/restore.lamp.sh.rewind": "REWOUND installed 15866 -> 15758\nREWOUND OK\n",
      "out/round.lamp.sh.putback-round": rnd(STABLE, NUPD), "out/" + R + "7": rd(STABLE, SUPD, prev="15758")})
    f.update(over); return {k: v for k, v in f.items() if v is not None}
RE = "KIND=run\nNEXT_SEQ=%s\nREWIND_YES=yes\n" % NEXT
case("R.pass", base(), "PASS", RE + "MUST_OUT='GATE 15860 on lamp-ee17: PASS'\nMUST_CALL='LAMP restore.lamp.sh key=rewind'\n")
case("R.pass-exit4-retry", base(**{"out/round.lamp.sh.round1.1": rnd(CAND, SUPD, exit_="4", lines=["not switched: no quiet moment came in 3600 s"]), "out/round.lamp.sh.round1.2": rnd(CAND, SUPD)}), "PASS", RE)
case("R.pass-finding", base(**{"out/after.lamp.sh.round2": facts(NEXT, CAND, NUPD, gwj=["%d.04 %s" % (T0 + 98, EXIT1)], gww=["%d LISTEN" % T0, "%d DOWN" % (T0 + 85), "%d LISTEN" % (T0 + 140)])}), "PASS",
     RE + "MUST_OUT='FINDING \\(#8429\\) after round 2: gw-exit1'\n")
case("R.pass-peek8417-fail", base(**{"out/peek.lamp.sh.measure-idle.2": ms("idle", 95000, 100, 250)}), "PASS", RE + "MUST_OUT='PEEK 8417 FAIL'\n")
case("R.fail-core-task", base(**{"j/TASK": "@+1 %srequest 202 heard 9 words\n@+2 %srequest 202: a read of weather handed to the lamp's agent\n" % (V2, V)}), "STOP:core", RE + "TASKWAIT=3\n")
case("R.stop-vol-high", base(**{"out/vol.lamp.sh": "VOL kept=40 bus=40 muted=false\nMUTE live muted=False reason=user\n"}), "STOP:restore", RE + "MUST_NOCALL='restore.lamp.sh'\n")
case("R.stop-vol-unread", base(**{"out/vol.lamp.sh": "VOL kept=none bus=none muted=none\nMUTE live muted=None reason=None\n"}), "STOP:restore", RE)
case("R.stop-binds", base(**{"out/" + R + "1": rd(STABLE, SUPD, binds=["/opt/lampOS/voice/abc/lamp-voice"])}), "STOP:restore", RE + "MUST_NOCALL='key=clean'\n")
case("R.stop-busy", base(**{"out/" + R + "1": rd(STABLE, SUPD, busy="journal.json")}), "STOP:restore", RE)
case("R.stop-refused", base(**{"out/" + R + "1": rd(STABLE, SUPD, ref="15860")}), "STOP:restore", RE)
case("R.stop-above-no-rewind", base(**{"out/" + R + "1": rd(CAND, CUPD)}), "STOP:restore", "KIND=run\nNEXT_SEQ=%s\nMUST_NOCALL='key=rewind'\n" % NEXT)
case("R.stop-restore-wrong-upd", base(**{"out/" + R + "3": rd(STABLE, CUPD)}), "STOP:restore", RE)
case("R.stop-round1-upd", base(**{"out/" + R + "4": rd(STABLE, BAD)}), "STOP:round1", RE + "MUST_NOCALL='round.lamp.sh'\n")
case("R.stop-cand-manifest", base(**{"rel/lampos-lamp-voice-15860/lampos-manifest.json": man(CAND, BAD)}), "STOP:round1", RE + "MUST_NOCALL='round.lamp.sh'\n")
case("R.stop-after1", base(**{"out/after.lamp.sh.round1": facts(STABLE, STABLE, SUPD)}), "STOP:after1", RE + "MUST_NOCALL='peek.lamp.sh'\n")
case("R.stop-peek-guard", base(**{"out/peek.lamp.sh.off": None, "out/peek.lamp.sh.off.1": "PEEK BACK NOT OK (drop-in present)\n", "out/peek.lamp.sh.off.2": "PEEK BACK OK (drop-in absent)\n"}),
     "STOP:peek-before-off", RE + "MUST_OUT='lamp-peek put back \\(PEEK BACK OK\\)'\nMUST_CALL='peek.lamp.sh key=off n=2'\n")
case("R.stop-next-unset", base(), "STOP:round2", "KIND=run\nNEXT_SEQ=\nREWIND_YES=yes\nMUST_NOCALL='round.lamp.sh key=round2'\n")
case("R.stop-after2", base(**{"out/after.lamp.sh.round2": facts(NEXT, CAND, BAD)}), "STOP:after2", RE)
case("R.stop-next-manifest-upd", base(**{"rel/lampos-lamp-voice-15866/lampos-manifest.json": man(NEXT, BAD)}), "STOP:round2", RE + "MUST_NOCALL='round.lamp.sh key=round2'\n")
case("R.pass-default-next", base(), "PASS", "KIND=run\nNEXT_DEFAULT=1\nREWIND_YES=yes\nMUST_OUT='15860 -> 15866'\n")
