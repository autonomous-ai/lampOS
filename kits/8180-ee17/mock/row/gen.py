# Fixtures for row.sh's legs: journal chunks (j/<LEG>.<CLIP>[.k], "@+<s> <unit>[pid]: <line>") and lamp-script outputs
# (out/<script>[.<key>]). Every journal line is the source's own text (crates/{tasks,control,voice}, branch head + worktree).
import os, shutil, sys
FX = sys.argv[1]; shutil.rmtree(FX, ignore_errors=True)
V = "lamp-voice[812]: lamp-voice: "; V2 = "lamp-voice[812]: lamp-voice second: "; T = "lamp-tasks[905]: [lamp-tasks] "; C = "lamp-control[640]: lamp-control: "
CGU = "/sys/fs/cgroup/system.slice/lamp-tasks.service"
SETUP = T + "each run gets its own cgroup under %s: memory.max 512M, cpu.max 100000 100000, pids.max 72" % CGU
def pieces(req, n): return V2 + "request %d tts: 1 of 1 pieces began, 0 on a reused connection, %d chars; the first piece's first audio byte 310 ms after it went, the slowest 310 ms" % (req, n)
def say(who, n): return C + "%s: say (%d chars, exact): ok, to the voice, by say.sock" % (who, n)
def tryrun(id_, grants): return C + "run %s: a make's tried run, a token for [%s], its writes stubbed" % (id_, grants)
def tryline(code, said, passed=""): return T + "make try: scripts/main.py: exit %s, 30 s limit%s, said %s" % (code, passed, said)
KEPT = lambda ms: V + "a skill made here was kept in %d ms" % ms
SAIDON = lambda n, u: V + "a made skill is said on with its try (%d chars, %s)" % (n, "uses named" if u else "no uses named")
def started(r, sk): return T + "skill run %s: %s handed over, token minted in 3 ms" % (r, sk)
def token(r, sk, g): return C + "run %s of skill %s: a token for [%s], 30 s" % (r, sk, g)
def handover(r, ms): return T + "skill run %s: hand-over to script start %d ms" % (r, ms)
def ended(r, how, said): return T + "skill run %s: ended in 1200 ms, %s, said %s" % (r, how, said)
def made(nm, kind, grants, stub=0, uses=1, eng="openclaw", fm="openclaw"):
    return "MADE %s new on=true kind=%s grants=%s uses=%d result=1 answer=1 stub=%d prints=0 fm_with=%s idx_with=%s engine=%s made_by=lamp src=folder\nCARD %s name: %s | made_with: %s\n" % (nm, kind, grants, uses, stub, fm, eng, eng, nm, nm, fm)
def leaf(r, pm="72"): return "RUNLEAF +%s at 09:00:01.20 memory.max=536870912 memory.high=469762048 memory.swap.max=0 cpu.max=100000_100000 pids.max=%s procs=2 [python3,lamp-tasks]" % (r, pm)
def cg(runs, pe="0", pm="72", gone=True, serve="serve"):
    s = "CGROOT %s subtree=cpu,memory,pids kids=serve \nSERVELEAF pid=905 path=/system.slice/lamp-tasks.service/%s leaf=%s\n" % (CGU, serve, serve)
    for r in runs:
        s += leaf(r, pm) + "\n"
        if gone: s += "RUNLEAF -%s gone at 09:00:03.10 pids.events.max=%s\n" % (r, pe)
    return s + "SERVELEAF pid=905 path=/system.slice/lamp-tasks.service/%s leaf=%s\nCGEND left=%s kids=serve  subtree=cpu,memory,pids\n" % (serve, serve, "" if gone else " ".join(runs))
def fence(pid, admin="hidden", eg=False):
    s = "EGRESS-AT-START \nTRYPID %d under_lamp_tasks=yes python3 /var/lib/lampos-tasks/make/try-x/scripts/main.py\nADMIN %d admin_sock=%s ctl_dir=[run.sock ]\nNET %d links=lo, sockfds=2 fd5=%s\n" % (pid, pid, admin, pid, "socket:[88123]" if eg else "-")
    if eg: s += "EGRESS +lampos-make-try-task-9.sock at 09:00:02\nEGRESSCONN 1 at 09:00:02\n"
    return s + "FENCE-WATCH END 09:01:00\n"
def HANDED(r): return V2 + "request %d: the heard words named openclaw for the work: round 1 hands them over in code (LAMP_NAMED_AGENT_TAKES_LINE)" % r
ALLOW = "tasks: the OpenClaw Gateway's allow-list add for /usr/local/lib/lampos/skill-start succeeded in 9800 ms"
NOTSENT = "lamp-tasks[905]: tasks: the OpenClaw Gateway answered on 127.0.0.1:18789 but did not serve its approvals read (exec.approvals.get) before the readiness or run deadline; the hand-over is not sent"
def j(lines): return "\n".join("@+%.1f %s" % (t, l) for t, l in lines) + "\n"
PASS = {
 # A: days-left make (signed out), tried run passes, said-on line with no uses (grants speak only), then the run
 "j/A.DAYS": j([(1, V + "request 101: a skill to make: asked"), (18, tryrun("make-try-task-a1", "speak")), (19, say("lamp-tasks-try-a1", 24)),
                (20, tryline(0, 1)), (30, KEPT(29812)), (30.5, SAIDON(86, False)), (31, pieces(102, 86))]),
 "j/A.DAYSASK": j([(1, started("run-a2", "days-left")), (1.1, token("run-a2", "days-left", "speak")), (1.6, say("run-a2", 22)),
                   (1.7, handover("run-a2", 412)), (1.8, ended("run-a2", "its script exited 0", 1)), (2, pieces(103, 22))]),
 "out/made.lamp.sh.A": made("days-left", "run", "speak"),
 "out/fence.lamp.sh.A": fence(4101),
 # B: price make (net), said-on names uses, the run: hand-over, exit 0, first say < 6 s, no agent
 "j/B.PRICE": j([(1, V + "request 111: a skill to make: asked"), (18, tryrun("make-try-task-b1", "net, speak")), (19, say("lamp-tasks-try-b1", 30)),
                 (20, tryline(0, 1)), (32, KEPT(31007)), (32.5, SAIDON(118, True)), (33, pieces(112, 118))]),
 "j/B.PRICEASK": j([(0.9, started("run-b2", "price-check")), (0.95, T + "skill run run-b2: granted net: its egress socket run-b2.sock made, its fence relays to it alone"),
                    (1.0, token("run-b2", "price-check", "net, speak")), (1.8, say("run-b2", 30)), (1.9, handover("run-b2", 640)),
                    (2.0, ended("run-b2", "its script exited 0", 1)), (2.1, pieces(113, 30))]),
 "out/made.lamp.sh.B": made("price-check", "run", "net,speak"),
 "out/fence.lamp.sh.B": fence(4202, eg=True),
 "out/cgwatch.lamp.sh.B": cg(["run-b2"]),
 # C: grant-probe asks the camera with no grant
 "out/probe.lamp.sh": "PROBE INSTALLED\n",
 "j/C.PROBE": j([(1, started("run-c1", "grant-probe")), (1.2, token("run-c1", "grant-probe", "speak")),
                 (1.5, C + "run-c1: look (center): refused: no_grant"), (1.6, handover("run-c1", 300)), (1.8, ended("run-c1", "its script exited 0", 1))]),
 # D: mail task step
 "j/D.MAIL": j([(1, V + "request 121: a skill to make: asked"), (40, KEPT(39020)), (41, pieces(122, 60))]),
 "j/D.MAILASK": j([(1, T + "skill task task-d2: started on the lamp's default agent"), (21, T + "skill task task-d2: ended, Harness says done"), (22, pieces(123, 70))]),
 "out/made.lamp.sh.D": made("mail-brief", "task", "mail-read"),
 "out/fence.lamp.sh.D": fence(4303),
 "out/task.lamp.sh": "PROMPT task=task-d2 card=yes first=yes never_lamp_files=yes state=done\nSESSION /var/lib/lampos-tasks/s1.jsonl card_named=yes lamp_paths=0\n",
 # E: long make runs on, READY asked while making
 "out/runon.lamp.sh.on": "RUNON ON\n", "out/runon.lamp.sh.off": "RUNON OFF\n",
 "j/E.LONGMAKE": j([(1, V + "request 131: a skill to make: asked"), (62, V2 + "request 131: the make has no answer 60 s after its waiting line: it runs on, its one line is said, and the turn ends"), (63, pieces(132, 40))]),
 "j/E.READY": j([(1, V + "a make asked while a long make runs on: told so, nothing made"), (2, pieces(133, 50))]),
 # K: camera + speaker skill
 "j/K.LOOK": j([(1, V + "request 141: a skill to make: asked"), (18, tryrun("make-try-task-k1", "speak, camera")), (19, say("lamp-tasks-try-k1", 40)),
                (20, tryline(0, 1)), (30, KEPT(29100)), (30.5, SAIDON(104, True)), (31, pieces(142, 104))]),
 "j/K.LOOKASK": j([(1, started("run-k2", "desk-look")), (1.1, token("run-k2", "desk-look", "speak, camera")), (2, say("run-k2", 40)),
                   (2.1, handover("run-k2", 500)), (2.2, ended("run-k2", "its script exited 0", 1)), (2.3, pieces(143, 40))]),
 "out/made.lamp.sh.K": made("desk-look", "run", "camera,speak"),
 "out/fence.lamp.sh.K": fence(4404),
 # H: restart lamp-tasks mid slow-probe run
 "j/H.SLOW.1": j([(1, started("run-h1", "slow-probe")), (1.1, token("run-h1", "slow-probe", "speak"))]),
 "j/H.SLOW.2": j([(1, started("run-h2", "slow-probe")), (1.1, token("run-h2", "slow-probe", "speak")), (2, say("run-h2", 20)),
                  (21, handover("run-h2", 380)), (21.1, ended("run-h2", "its script exited 0", 1))]),
 "out/cgwatch.lamp.sh.H": cg(["run-h1"]),
 "out/restart-tasks.lamp.sh": "BEFORE pid=905 kids=serve run-h1 \nAFTER 3s pid=1777 cgroup=/system.slice/lamp-tasks.service/serve leaf=serve subtree=cpu,memory,pids kids=serve \nAFTER serve_procs=[1777 ] root_procs=[] mainpid_in_serve=yes\nJ " + SETUP.split(": ", 1)[1] + "\n",
 # P: fork-probe past pids.max while the price run answers
 "j/P.FORK": j([(1, started("run-p1", "fork-probe")), (1.1, token("run-p1", "fork-probe", "speak"))]),
 "j/P.PRICEASK": j([(0.9, started("run-p2", "price-check")), (1.0, token("run-p2", "price-check", "net, speak")), (1.8, say("run-p2", 30)),
                    (1.9, handover("run-p2", 700)), (2.0, ended("run-p2", "its script exited 0", 1)),
                    (3.0, "lamp-skills[700]: FORKPROBE started=64 refused=56"), (3.1, handover("run-p1", 350)), (3.2, ended("run-p1", "its script exited 0", 1))]),
 "out/cgwatch.lamp.sh.P": cg(["run-p1", "run-p2"], pe="56"),
 # W
 "out/wshop.lamp.sh.hash.1": "WSHOP 3f2a9c0d11aa22bb files=4\n", "out/wshop.lamp.sh.hash.2": "WSHOP 3f2a9c0d11aa22bb files=4\n",
 "out/wshop.lamp.sh.config": 'DENY ["skill_workshop"]\nCHECK (W1) tools.deny holds skill_workshop: PASS\nGATEWAY active since \'Sat 2026-11-21 09:00:00 +07\' restarts=0 exit78_lines=0\nCHECK (W2) OpenClaw accepts the file (gateway active, no exit 78 since start): PASS\n',
 # run.sh's own steps (RUN cases)
 "out/check.lamp.sh": "BIN lamp-tasks running=ab12 exe=/run/row-artifact/lamp-tasks dropins=/run/systemd/system/lamp-tasks.service.d/zz-artifact.conf\nCGROUP OK serve in its own leaf 'serve'\nJ " + SETUP.split(": ", 1)[1] + "\nVERIFY PASS\nREADY\n",
 "out/micon.lamp.sh": "MIC_BEFORE {\"muted\":true}\nVOICE_MUTE muted=False reason=None muted_by=None control=None age_s=0.5 seq=3\nMUTE LIFTED OK after 4s\n",
 "out/mute.lamp.sh": "MUTE OFF OK\n", "out/save.lamp.sh": "ENGINE openclaw\nBASE build 15322\nLINKED google gmail-app (files: gmail-app)\nNAMENEAR off (row drop-in)\nSAVED OK\n",
 "out/unrow.lamp.sh": "PASS LAMP_MAKE_MAY_RUN_ON not set\nPASS LAMP_NAMED_AGENT_TAKES_LINE not set\nPASS LAMP_NAME_NEAR back to the product's ('on')\nPASS lamp-cloud active\nUNROW OK\n",
 "out/micoff.lamp.sh": "RECORD_BACK {\"muted\":true}\nMUTE BACK OK after 4s\n",
 "out/idle.lamp.sh": "IDLE resting after 3.0s\n",
 "out/queue.lamp.sh": "QUEUE CLEAR (close loop lines in 20s: 0)\n",
 # ee17 (23 Nov): swap binds #8180's four parts over stable, restore takes them away (swap/*.lamp.sh)
 "out/swap.lamp.sh": "".join("INSTALLED %s 11aa (/opt/lampOS/%s/15543/%s)\n" % (b, b[5:], b) for b in ("lamp-tasks", "lamp-control", "lamp-skills", "lamp-voice"))
     + "".join("PASS %s runs %s 22bb\n" % (b, b) for b in ("lamp-tasks", "lamp-control", "lamp-skills", "lamp-voice")) + "GATEWAY restarted, listening after 6s\nSWAPPED OK\n",
 "out/restore.lamp.sh": "".join("PASS %s back on installed 11aa\n" % b for b in ("lamp-tasks", "lamp-control", "lamp-skills", "lamp-voice")) + "GATEWAY restarted, listening after 6s\nRESTORED OK\n",
 # N (#7269): off (as on stable) the voice answers; on, the voice's hand-over line (second_provider.rs, #7269)
 "out/named.lamp.sh.read": "NAMED env='' dropin=absent\nENGINE default=openclaw engines=openclaw,hermes\nENGINEFILE {\"version\":1,\"engine\":\"openclaw\"} engine=openclaw\nMUTE muted=False\nTASKMAX 5\n",
 "out/named.lamp.sh.task": "TASKMAX 5\nTASK - id=6 engine=openclaw state=running alias=lamp weather=yes words=9\nTASKS - new=1 above 5\n",
 "out/named.lamp.sh.on": "NAMED env='on' dropin=present worker_line_since_restart=1\nENGINE default=openclaw engines=openclaw,hermes\nMUTE muted=False\nNAMED ON\n",
 "out/named.lamp.sh.off": "NAMED env='' dropin=absent worker_line_since_restart=0\nENGINE default=openclaw engines=openclaw,hermes\nMUTE muted=False\nNAMED OFF\n",
 "j/N.NAMED.1": j([(2, pieces(201, 80))]),
 "j/N.NAMED.2": j([(1, HANDED(202)), (3, pieces(202, 60))]),
 # 7170: reboot, ssh back on a new boot, port listening, the D task skill's run asked, its script started
 "out/boot.lamp.sh.before": "BOOTID boot-1\nUPTIME 5000.10\nVOICE_MUTE muted=False reason=None muted_by=None control=None age_s=0.4 seq=9\nPRIVACY permitted=True physical_muted=False age_s=1.0\nRECORD muted=False\nGATEWAY active start_up=20.10\nVOICE_DROPINS \n",
 "out/boot.lamp.sh.reboot": "REBOOT ASKED (systemctl reboot in 3 s)\n",
 "cmd/bootid": "boot-2\n",
 "out/boot.lamp.sh.after": "BOOTID boot-2\nUPTIME 40.20\nCLOUD active\nVOICE_MUTE muted=False reason=None muted_by=None control=None age_s=0.6 seq=1\nPRIVACY permitted=True physical_muted=False age_s=0.6\nRECORD muted=False\nMUTEBOOT muted=False (bus, after 6s)\nVOICE active GATEWAY activating\n",
 "out/boot.lamp.sh.port": "PORT listening up=71.40 epoch=@NOW@ after_s=31\nGWSTART up=20.10\nVOICE active TASKS active\n",
 "out/boot.lamp.sh.times": "J 1795230150.200 lamp-tasks[905]: " + ALLOW + "\nJ 1795230150.900 lamp-tasks[905]: [lamp-tasks] skill start: run task-t1 claimed by the launcher (pid 2211)\nJ 1795230151.000 lamp-tasks[905]: [lamp-tasks] skill start: run task-t1's script started\nSTARTUP gateway unit start up=20.10, port listening up=71.40, approvals read answering up=150.20 (uptime s)\nSTARTUP unit->port 51.3 s, port->approvals 78.8 s, unit->approvals 130.1 s\n",
 "j/7170.MAILASK": j([(1, T + "skill task task-t1: started on the lamp's default agent"), (2, "lamp-tasks[905]: " + ALLOW),
                     (2.5, T + "skill start: run task-t1 claimed by the launcher (pid 2211)"), (2.6, T + "skill start: run task-t1's script started"),
                     (3, T + "skill task task-t1: ended, Harness says done, after 40 s; its agent: started (openclaw)")]),
}
def case(name, legs, tails="", over=None, drop=(), env=None, boot=False):
    d = os.path.join(FX, name); os.makedirs(d)
    f = dict(PASS); f.update(over or {})
    for k in drop: f.pop(k, None)
    for k, v in f.items():
        p = os.path.join(d, k); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(v)
    open(os.path.join(d, "case.env"), "w").write("LEGS='%s'\nTAILS='%s'\n%s" % (legs, tails, "".join("%s=%s\n" % kv for kv in (env or {}).items()) + ("BOOT=1\n" if boot else "")))
    exp = name.split(".")[-1].split("-")[0].upper()
    open(os.path.join(d, "expect"), "w").write(exp + "\n")
# name: <leg>.<expected>[-variant]   (the expected verdict of the LAST leg in LEGS TAILS)
case("A.pass", "A")
case("A.fail-stub-try", "A", over={"out/made.lamp.sh.A": made("days-left", "run", "speak", stub=1),
     "j/A.DAYS": PASS["j/A.DAYS"].replace("exit 0, 30 s limit, said 1", "exit 1, 30 s limit, said 0")})
case("A.fail-saidon-long", "A", over={"j/A.DAYS": PASS["j/A.DAYS"].replace("(86 chars, no uses named)", "(412 chars, no uses named)")})
case("A.fail-norun", "A", over={"j/A.DAYSASK": j([(2, pieces(103, 40))])})
case("A.fail-runexit", "A", over={"j/A.DAYSASK": PASS["j/A.DAYSASK"].replace("its script exited 0", "its script exited 1")})
case("A.fail-runsaid0", "A", over={"j/A.DAYSASK": PASS["j/A.DAYSASK"].replace("exited 0, said 1", "exited 0, said 0")})
case("A.fail-otherskill", "A", over={"j/A.DAYSASK": PASS["j/A.DAYSASK"].replace("days-left", "week-day")})
case("A.fail-nosay", "A", over={"j/A.DAYSASK": j([(1, started("run-a2", "days-left")), (1.1, token("run-a2", "days-left", "speak")), (1.7, handover("run-a2", 412)), (1.8, ended("run-a2", "its script exited 0", 1))])})
case("A.fail-trysaid0", "A", over={"j/A.DAYS": PASS["j/A.DAYS"].replace("exit 0, 30 s limit, said 1", "exit 0, 30 s limit, said 0")})
# the voice held an ask back as said to nobody (attention.rs NOT_SPOKEN_TO, provider_worker.rs / second_provider.rs's line)
HELD = lambda r: V + "provider: request %d: its answer is held back: nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on" % r
case("A.void-held", "A", over={"j/A.DAYSASK": j([(2, HELD(103))])})
case("B.void-held", "B", over={"j/B.PRICEASK": j([(2, HELD(113))])})
# a make still in flight: lamp-tasks refused the make busy; a long make running on past the bound; one that ends
BUSY = T + "skill make: refused busy, one is being made"
RUNON = "@NOW@ mock-lamp lamp-voice[812]: lamp-voice second: request 131: the make has no answer 60 s after its waiting line: it runs on, its one line is said, and the turn ends\n"
case("A.void-busy", "A", over={"j/A.DAYS": j([(1, BUSY), (2, V + "a skill wasn't made: one is being made")])})
case("K.void-inflight", "K", over={"cmd/makes": RUNON})
case("K.pass-inflight-ends", "K", over={"cmd/makes.1": RUNON, "cmd/makes": RUNON + "@NOW@ mock-lamp lamp-voice[812]: lamp-voice: a long make was made: kept off until its person hears it\n"}, env={"GREP": "'MAKEIDLE no make in flight after'"})
case("E.void-inflight", "E", over={"cmd/makes": RUNON})
# the kept session's cold start is an INFO line, no verdict (#8237)
case("D.pass-coldinfo", "D", over={"j/D.MAILASK": PASS["j/D.MAILASK"] + "@+22.5 lamp-tasks[905]: tasks: one session: kept session new agent:main (openclaw, ready) in 84883 ms\n"}, env={"GREP": "'^INFO leg D cold start \\(#8237, no verdict\\): .*kept session new .* in 84883 ms'"})
# an ask waits for the lamp's own say to end (its made line, 86 chars at +31 s)
case("A.pass-hush", "A", env={"GREP": "'HUSH the lamp.s say ends in'"})
case("A.fail-notkept", "A", over={"j/A.DAYS": j([(1, V + "request 101: a skill to make: asked"), (30, V + "a skill made here wasn't kept in 30100 ms: its try failed")])})
case("B.pass", "B")
# ee17 rest1 (23 Nov): a second chat round's make_skill for the same ask while the first make runs; lamp-voice's
# "wasn't made again" refusal comes before the first make's kept line and must not be read as B's make end.
case("B.pass-again", "B", over={"j/B.PRICE": PASS["j/B.PRICE"].replace("@+32.0 ", "@+25.0 " + V + "a skill wasn't made again: the same ask is being made already\n@+32.0 ", 1)})
case("B.fail-latency", "B", over={"j/B.PRICEASK": PASS["j/B.PRICEASK"].replace("@+1.8 ", "@+9.0 ")})
case("B.fail-agent", "B", over={"j/B.PRICEASK": PASS["j/B.PRICEASK"] + "@+2.5 " + T + "skill task task-b9: started on the lamp's default agent\n"})
case("B.fail-saidon-nouses", "B", over={"j/B.PRICE": PASS["j/B.PRICE"].replace("(118 chars, uses named)", "(96 chars, no uses named)")})
case("B.fail-oldform-nouses", "B", over={"j/B.PRICE": PASS["j/B.PRICE"].replace("a made skill is said on with its try (118 chars, uses named)", "a made skill is said on with its try: Made it, and I tried it: Bitcoin is at 61,000 dollars. Say 'remove it' to undo.")})
case("B.pass-oldform-uses", "B", over={"j/B.PRICE": PASS["j/B.PRICE"].replace("a made skill is said on with its try (118 chars, uses named)", "a made skill is said on with its try: Made it. It uses the internet, and I tried it: Bitcoin is at 61,000 dollars. Say 'remove it' to undo.")})
case("B.fail-nocgroup", "B", over={"j/B.PRICEASK": j([(0.9, started("run-b2", "price-check")), (1.0, T + "run run-b2: refused, no own cgroup: lamp-tasks serve did not set them up"),
     (1.2, ended("run-b2", "the run's own memory and CPU bounds could not be set, so it was not run", "unknown"))])})
case("C.pass", "C")
case("C.fail", "C", over={"j/C.PROBE": j([(1, started("run-c1", "grant-probe")), (1.6, handover("run-c1", 300)), (1.8, ended("run-c1", "its script exited 0", 1))])})
case("C.void", "C", over={"out/probe.lamp.sh": "PROBE install grant-probe exit 1: refused\nPROBE NOT INSTALLED\n"})
case("D.pass", "D")
NOTKEPT_D = {"j/D.MAIL": j([(1, V + "request 121: a skill to make: asked"), (40, V + "a skill made here wasn't kept in 39020 ms: its try failed")])}
case("D.void-nolink", "D", over=NOTKEPT_D, env={"LINKED": "'google none (files: gmail-app=expired)'"})
case("D.fail-notkept-linked", "D", over=NOTKEPT_D)
case("D.fail-lampfiles", "D", over={"out/task.lamp.sh": PASS["out/task.lamp.sh"].replace("lamp_paths=0", "lamp_paths=2")})
case("D.fail-slow", "D", over={"j/D.MAILASK": PASS["j/D.MAILASK"].replace("@+21.0", "@+70.0")})
case("E.pass", "E")
case("E.fail", "E", over={"j/E.LONGMAKE": j([(1, V + "request 131: a skill to make: asked"), (40, KEPT(39000))])})
case("E.void", "E", over={"j/E.READY": j([(0.5, V + "a long make was made: kept off until its person hears it"), (1, V + "a make asked while a long make runs on: told so, nothing made")])})
case("E.void-switch", "E", over={"out/runon.lamp.sh.on": "RUNON STILL 'off'\n"})
case("K.pass", "K")
case("K.fail-yes", "K", over={"j/K.LOOK": PASS["j/K.LOOK"] + "@+33 " + V + "a skill made here: read back: it waits for the person's yes\n"})
case("K.fail-notoken", "K", over={"j/K.LOOKASK": PASS["j/K.LOOKASK"].replace("[speak, camera]", "[speak]")})
# the lamp's 21 Nov line named "look" where the old check wanted it: the token's real names are camera/microphone
case("K.fail-oldname", "K", over={"j/K.LOOK": PASS["j/K.LOOK"].replace("[speak, camera]", "[speak, look]")})
case("K.fail-extragrant", "K", over={"j/K.LOOKASK": PASS["j/K.LOOKASK"].replace("[speak, camera]", "[speak, camera, microphone]")})
case("H.pass", "H")
case("H.fail-nocgroup", "H", over={"out/restart-tasks.lamp.sh": PASS["out/restart-tasks.lamp.sh"].replace("J " + SETUP.split(": ", 1)[1], "J [lamp-tasks] runs have no own cgroup: memory not handed down: Permission denied (lamp-tasks.service needs Delegate=memory cpu pids); every run step and make's try is refused")})
case("H.fail-leafleft", "H", over={"out/cgwatch.lamp.sh.H": cg(["run-h1"], gone=False)})
case("H.void", "H", over={"out/probe.lamp.sh": "PROBE NOT INSTALLED\n"})
case("P.pass", "B P")
case("P.fail", "B P", over={"out/cgwatch.lamp.sh.P": cg(["run-p1", "run-p2"], pe="0"), "j/P.PRICEASK": PASS["j/P.PRICEASK"].replace("refused=56", "refused=0")})
case("P.void", "P")
# P needs B's kept price skill, not B's PASS (e82d 14:38: B failed, so P went VOID)
case("P.pass-bfail", "B P", over={"j/B.PRICEASK": PASS["j/B.PRICEASK"].replace("@+1.8 ", "@+9.0 ")})
case("F.pass", "A B D", "F")
case("F.fail", "A B D", "F", over={"out/fence.lamp.sh.A": fence(4101, admin="visible")})
case("F.fail-noegress", "A B D", "F", over={"out/fence.lamp.sh.B": fence(4202), "j/B.PRICE": PASS["j/B.PRICE"]})
case("F.void", "A B D", "F", over={"out/fence.lamp.sh.A": "EGRESS-AT-START \nFENCE-WATCH END\n", "out/fence.lamp.sh.B": "EGRESS-AT-START \nFENCE-WATCH END\n", "out/fence.lamp.sh.D": "EGRESS-AT-START \nFENCE-WATCH END\n"})
case("G.pass", "A B", "G")
case("G.fail-uncapped", "A B", "G", over={"out/cgwatch.lamp.sh.B": cg(["run-b2"], pm="max")})
case("G.fail-serveroot", "A B", "G", over={"out/cgwatch.lamp.sh.B": cg(["run-b2"], serve="").replace("leaf=", "leaf=unit-root")})
case("G.void", "A B", "G", env={"MOCK_LAMPOS_TASKS": "40"})
# A or B not shown (VOID) and neither failed: G VOID, not FAIL
case("G.void-bvoid", "A B", "G", over={"j/B.PRICEASK": j([(2, HELD(113))])})
case("W.pass", " ", "W")
case("W.fail", " ", "W", over={"out/wshop.lamp.sh.hash.2": "WSHOP 99aa00bb11cc22dd files=5\n"})
case("W.fail-deny", " ", "W", over={"out/wshop.lamp.sh.config": PASS["out/wshop.lamp.sh.config"].replace("W1) tools.deny holds skill_workshop: PASS", "W1) tools.deny holds skill_workshop: FAIL")})
# RUN: ../../run.sh itself (LEGS/TAILS for its legs/tails steps), with the trap's put-back
case("RUN.pass", "A B D N", "F G", boot=True, env={"MIC_YES": "yes", "RUNSH": "1"})
case("RUN.fail-noboot-muteback", "A B D N", "F G", boot=True, env={"MIC_YES": "yes", "RUNSH": "1"}, over={"cmd/bootid": "boot-1\n"})
case("RUN.void-noyes", "A B D N", "F G", boot=True, env={"RUNSH": "1"})
case("ALL.pass", "A B C D E K H P N", "F G")
case("ALL.pass-elast", "A B C D K H P N E", "F G")
# the rerun of 22 Nov (First 14:39): the failed legs only, G among them, on the same build
case("RERUN.pass", "A B C D K H G P", "")
case("RUN.pass-rerun", "A B C D K H G P", "", env={"MIC_YES": "yes", "RUNSH": "1", "SKIP": "'tails boot'"})
# the optional put-back: stable re-signed above the lamp's sequence (15510 -> 15600), installed; a sequence not above it stops
PB = {"out/putback.lamp.sh.read.1": "SEQ installed=15510 busy=''\n", "out/putback.lamp.sh.read": "SEQ installed=15600 busy=''\n"}
case("RUN.pass-putback", "A B D N", "F G", boot=True, over=PB, env={"MIC_YES": "yes", "RUNSH": "1", "PUTBACK_CMD": "'putback-cmd'", "PUTBACK_SEQ": "15600", "GREP": "'^PUTBACK OK installed 15600 \\(was 15510\\)'"})
case("RUN.fail-putback-low", "A B D N", "F G", boot=True, over=PB, env={"MIC_YES": "yes", "RUNSH": "1", "PUTBACK_CMD": "'putback-cmd'", "PUTBACK_SEQ": "15500", "GREP": "'STOP at putback: PUTBACK_SEQ 15500 is not above'"})
# ee17: the swap's way in and out (swap/swap.sh, swap/restore.sh)
case("RUN.pass-swap", "A B D N", "F G", boot=True, env={"MIC_YES": "yes", "RUNSH": "1", "GREP": "'^RESTORED OK'"})
case("RUN.void-badart", "A B D N", "F G", boot=True, env={"MIC_YES": "yes", "RUNSH": "1", "BADART": "lamp-skills", "GREP": "'lamp-skills sha [0-9a-f]+ is not SHA256SUMS'"})
case("RUN.void-swapfail", "A B D N", "F G", boot=True, over={"out/swap.lamp.sh": "FAIL lamp-voice not on the test build (pid 0, )\nSWAP NOT OK\n"}, env={"MIC_YES": "yes", "RUNSH": "1", "GREP": "'STOP at swap'"})
case("RUN.fail-restore", "A B D N", "F G", boot=True, over={"out/restore.lamp.sh": "NOT OURS: /opt/lampOS/voice/15543/lamp-voice bound from /x: left\nRESTORE NOT OK\n"}, env={"MIC_YES": "yes", "RUNSH": "1", "GREP": "'still bound over the installed ones'"})
case("RUN.void-stalesock", "A B D N", "F G", boot=True, over={"out/check.lamp.sh": "CONTROLSOCK host=811 gateway=702 (gateway pid 990)\nVERIFY FAIL: control-sock-stale-in-gateway(#8320)\nREADY\n"}, env={"MIC_YES": "yes", "RUNSH": "1", "GREP": "'STOP at verify: no .\\^VERIFY PASS'"})
case("RUN.pass-noputback", "A B D N", "F G", boot=True, env={"MIC_YES": "yes", "RUNSH": "1", "GREP": "'PUTBACK skipped'"})
# run.sh's order: legs, then the tails in a second invocation
case("ALL.pass-split", "A B C D E K H P N", "", env={"TAILS2": "'F G'"})
case("G.pass-split", "A B", "", env={"TAILS2": "'F G'"})
case("G.fail-split-bfail", "A B", "", env={"TAILS2": "'F G'"}, over={"j/B.PRICEASK": PASS["j/B.PRICEASK"].replace("@+1.8 ", "@+9.0 ")})
case("N.pass", "N")
case("N.fail-offhanded", "N", over={"j/N.NAMED.1": j([(1, HANDED(201)), (2, pieces(201, 80))])})
case("N.fail-nohand", "N", over={"j/N.NAMED.2": j([(2, pieces(202, 60))])})
case("N.fail-stillon", "N", over={"out/named.lamp.sh.off": "NAMED env='on' dropin=present worker_line_since_restart=1\nNAMED STILL 'on' (drop-in present)\n"})
case("N.void", "N", over={"out/named.lamp.sh.on": "NAMED env='' dropin=absent worker_line_since_restart=0\nNAMED NOT ON ('')\n"})
case("7170.pass", "D", boot=True)
case("7170.fail-noport", "D", boot=True, over={"out/boot.lamp.sh.port": "PORT NONE in 300 s (lamp-openclaw-gateway activating, start_up=none)\nVOICE active TASKS active\n"})
case("7170.fail-notsent", "D", boot=True, over={"j/7170.MAILASK": j([(1, T + "skill task task-t1: started on the lamp's default agent"), (5, NOTSENT)])})
case("7170.fail-noboot", "D", boot=True, over={"cmd/bootid": "boot-1\n"})
case("7170.void", " ", boot=True)
# No linked mail (D VOID): the run asked is leg A's made skill when it is a task step, by its own clip.
case("7170.pass-from-A", "A", boot=True, over={"out/made.lamp.sh.A": made("days-left", "task", "speak"), "j/7170.DAYSASK": PASS["j/7170.MAILASK"]})
case("7170.void-A-is-a-run", "A", boot=True)
case("7170.void-muted", "D", boot=True, over={"out/boot.lamp.sh.after": PASS["out/boot.lamp.sh.after"].replace("MUTEBOOT muted=False", "MUTEBOOT muted=True")})
case("7170.pass-muted-yes", "D", boot=True, env={"MIC_YES": "yes"}, over={"out/boot.lamp.sh.after": PASS["out/boot.lamp.sh.after"].replace("MUTEBOOT muted=False", "MUTEBOOT muted=True"), "out/micon.lamp.sh": "RECORD_KEPT the person's record from this ROW's first micon stays\nMUTE LIFTED OK after 4s\n"})
# U: one kit helper alone (unit.sh), against a fake lamp tree: the engine without an app choice, an old-stamped
# mute, a refused ssh login
for u in ("engine-env", "engine-app", "engine-none", "mute-oldstamp", "mute-timed", "mute-live", "mute-liveoff", "ssh-retry", "ssh-giveup", "ssh-other", "fixtures", "clips-call", "linked-none", "linked-expired", "linked-ok", "vars-default", "keep-verdicts", "namenear", "idle-resting", "idle-turns", "idle-speaking", "idle-stale", "queue-clear", "queue-loop", "queue-stuck", "gateway-8320", "push-probe", "volume-same", "volume-moved"):
    case("U.pass-" + u, " ", env={"UNIT": u})
