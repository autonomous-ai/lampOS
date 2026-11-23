#!/usr/bin/env python3
# Mock fixtures for the #8337 ROW kit (from the #8162 kit's; only its RUN cases kept, plus leg E's): fx/<case>/{case.env, expect, out/*, j/*, a/*}. Recorded-form lamp outputs
# (each line in the form the lamp scripts print it), journal chunks (j/<leg>.<event>[.k]: "@+<s> <text>", the text in
# the form the code at 4c663af40f logs it, see row/patterns.env) and readiness chunks (a/<leg>.<event>: "@+<s> <state>
# <why>"). An event is a lamp body run (gw, jam.kill, l7.go, ...), a clip played (play.<clip>) or a runner command
# (hold, unhold). Each case = the defaults + its own changes (None removes a file).
import os, sys, shutil

OUT = sys.argv[1]
REQ = "lamp-voice provider: request %d: the words heard"
REF = "lamp-voice provider: request %d: the lamp's own agent is %s: refused at once, nothing prepared"
AUD = "lamp-voice second: request %d first answer audio %d ms after the words were heard"
DONE = "tasks: one session: agent:main's turn is done; the queue goes on"
TAKEN = "tasks: harness reply: agent:main exit=0 took 2 s"
KEPTNEW = "tasks: one session: kept session new cccc2222 agent:main"
READYTOLD = "lamp-voice: telling news from the work handed over (agent-ready-1700000000000)"
TOLD = "lamp-voice: telling news from the work handed over (task-row8162-1)"
BROKE = "tasks: one session: row8162-1: its session broke (gateway closed); the task ends now, never run again"
REFHAND = "tasks: one session: row8162-2 refused the hand-over (no answer); the task ends now, and a fresh session is opened after (#8138)"
WATCH = "tasks: one session: the kept session agent:main's watch ended (daemon restarted); ensured and watched again"
GWBACK = "tasks: one session: OpenClaw's Gateway answers again; opened at once"
HELD = "tasks: one session: Harness's watch says it is held (held_approval); only the person can clear it, so no fresh session (#8138)"
CUTBACK = "lamp-update: lamp-tasks.service started again: OpenClaw's install ended while it was stopped for the gateway's restart (#8138)"
AGENTL = "tasks: agent: ready -> recovering"


def ch(*lines):
    return "".join("@+%s %s\n" % (t, x) for t, x in lines)


def answered(n):
    return ch((0.1, REQ % n), (0.6, AUD % (n, 700)), (1.5, DONE))


def refused(n, state):
    return ch((0.1, REQ % n), (0.2, REF % (n, state)), (0.4, AUD % (n, 800)))


BACK = ch((0.5, "recovering gateway_down"), (2.5, "starting session_starting"), (4, "ready -"))
DEF = {
    # lamp outputs, leg steps
    "out/mute.lamp.sh": "MUTE OFF OK (voice/mute muted=False)\n",
    "out/kept.lamp.sh.1": "KEPT_AGENT aaaa1111bbbb\nKEPT_MACHINE m1\nPANEPID 4242\n",
    "out/kept.lamp.sh.2": "KEPT_AGENT cccc2222dddd\nKEPT_MACHINE m1\nPANEPID 4343\n",
    "out/ask.lamp.sh": "ASK_ANSWER row8162-1 done: the answer came\n",
    "out/ask.lamp.sh.L2.1": "ASK_NOT_SENT row8162-9 one_session_stuck: the task ended, never run again\n",
    "out/gw.lamp.sh": "GW_PID_BEFORE 100\nGWR_AT @NOW@\nGW_ACTIVE active\nGW_HEALTH @NOW+3@\nGW_PID_AFTER 101\n",
    "out/jam.lamp.sh.kill": "JAM_PID 4242\nJAM_AT @NOW@\nKILL 4242\nGW_SAME yes\n",
    "out/jam.lamp.sh.stop": "JAM_PID 4242\nJAM_AT @NOW@\nSTOP 4242\nGW_SAME yes\n",
    "out/unfreeze.lamp.sh": "UNFREEZE CONT 4242\nUNFREEZE OK\n",
    "out/nostop.lamp.sh": "NOSTOP OK (none stopped)\n",
    "out/hd.lamp.sh": "HD_BEFORE 300\nHDR_AT @NOW@\nHD_BACK active 301\n",
    "out/l6.lamp.sh": "L6 READ only\nAGENT state=ready why=- engine=openclaw since_ms=1 seq=3\n",
    "out/l7.lamp.sh.go": "L7_AT @NOW@\nSTRIPPED 2 keys\nORDER tasks-stop-said 1\nORDER lamp-tasks-stopped 2\nORDER gw-started 3\nORDER health 4\nORDER lamp-tasks-started 5\nORDER start-said 6\nL7_ORDER PASS\nL7_GW_HEALTH_S 4\nGW_PID_AFTER 555\n",
    "out/l7.lamp.sh.cut": "L7_AT @NOW@\nCUT_AT @NOW+1@\nGAP lamp-tasks=inactive record=yes gw=active\n",
    "out/l7check.lamp.sh": "L7_CONFIG LIVE\n",
    "out/l7back.lamp.sh": "L7BACK OK\n",
    "out/l8back.lamp.sh": "ONESHOT_END @NOW-2@\nTASKS_BACK @NOW+1@\nCUTBACK_LINES 1\nRECORD gone\nGW health ok\nL8BACK OK\n",
    # run.sh's steps (cmd mode: ROUND*_CMD; round 1 = build 15600 with a self-switch, round 2 = stable's bytes at 15601)
    "out/base.lamp.sh": "SEQ installed=15543 max=15543\nUPDATER e988ff0a4d27\nLINE product\nHARNESS 0.6.75\nENGINE openclaw\nAGENT state=ready why=-\nFAILED0 run-rpc_pipefs.mount smartmontools.service \nEXESHA lamp-update=e988ff0a4d275afcae5cff5062df59133c838efd673b0573384b5adcc162f7f4 lamp-tasks=b93a1471db20d0372a824ee004ac16abba37d0857ca60732b6657dc84914a175 lamp-voice=c0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ff\nSPEAKER Speaker 35%\nT HAUTH via=cli logged_in=True offline=False\nPRIVACY permitted=True physical_muted=False age_s=2.0\nKEYPUB product\nBASE OK\n",
    "out/round.lamp.sh.mark": "MARK round 1 at @NOW@ build=15543 max=15543\n",
    "out/round.lamp.sh.go.1": "GO round 1 at @NOW@\n",
    "out/round.lamp.sh.go.2": "GO round 2 at @NOW@\n",
    "out/round.lamp.sh.read.1": "SEQ installed=15600 busy=''\nJ 1.0 lamp-update: switching to build 15600\nJ 2.0 lamp-update: lamp-update switches to its build 4c663af4; it must confirm itself\nJ 3.0 lamp-update: confirmed: lamp-update runs its new build\nJ 4.0 lamp-update: Harness 0.6.75 is moved to the pinned 0.6.78\nJ 5.0 lamp-update: up to date: build 15600\nUPDATER running=aaaa\n",
    "out/round.lamp.sh.read.2": "SEQ installed=15601 busy=''\nJ 1.0 lamp-update: switching to build 15601\nJ 2.0 lamp-update: switched to build 15601; every part is well\nUPDATER running=e988ff0a4d275afcae5cff5062df59133c838efd673b0573384b5adcc162f7f4\n",
    "out/verify.lamp.sh": "STATE 15600\nHARNESS 0.6.78\nAGENT state=ready engine=openclaw\nVERIFY PASS\n",
    "out/micon.lamp.sh": "MIC_BEFORE muted\nMUTE LIFTED OK after 2s\n",
    "out/micoff.lamp.sh": "MUTE BACK OK (muted again)\n",
    "out/stage.lamp.sh": "STAGED OK\n",
    "out/putback.lamp.sh": "STATE 15601\nPUTBACK OK\n",
    "out/hdown.lamp.sh": "HARNESS 0.6.75\nHDOWN OK\n",
    "out/read.lamp.sh": "STATE 15601 busy=''\nINSTALLED 15601 e1f55d813a521d34c99d50e3535a01cb03b40644\nHARNESS 0.6.75\nEXESHA lamp-update=e988ff0a4d275afcae5cff5062df59133c838efd673b0573384b5adcc162f7f4 lamp-tasks=b93a1471db20d0372a824ee004ac16abba37d0857ca60732b6657dc84914a175 lamp-voice=c0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ff\nT at @NOW@\nREAD OK\n",
    # readiness at each follower's start; the legs' chunks
    "a/start": ch((0, "ready -")),
    "j/L0.play.20-ask-a": answered(1),
    "j/L1.gw": ch((0.3, AGENTL), (3.2, GWBACK), (3.5, KEPTNEW), (4.5, READYTOLD)),
    "a/L1.gw": BACK,
    "j/L1.play.21-ask-b": refused(2, "recovering"),
    "j/L1.play.22-ask-c": answered(3),
    "j/L2.ask.1": ch((0.3, TAKEN)),
    "j/L2.gw": ch((1, BROKE), (1.5, TOLD), (3, KEPTNEW), (4.5, READYTOLD)),
    "a/L2.gw": BACK,
    "j/L2.play.23-ask-d": answered(4),
    "j/L3.jam.kill": ch((0.4, AGENTL), (2.5, KEPTNEW), (3.5, READYTOLD)),
    "a/L3.jam.kill": ch((0.4, "recovering pane_gone"), (2, "starting session_starting"), (3, "ready -")),
    "j/L3.play.24-ask-e": refused(5, "recovering"),
    "j/L4.play.26-ask-g": ch((0.1, REQ % 6), (1, REFHAND), (1.5, TOLD), (2.5, KEPTNEW), (3.5, READYTOLD)),
    "a/L4.play.26-ask-g": ch((1.2, "recovering session_stuck"), (2, "starting session_starting"), (3, "ready -")),
    "j/L5.hd": ch((3, WATCH)),
    "j/L5.play.20-ask-a": ch((0.1, REQ % 7), (0.6, AUD % (7, 900))),
    "a/L7.l7.go": ch((0.1, "installing -"), (0.5, "starting tasks_restarting"), (1, "ready -")),
    "a/L8.l7.cut": ch((0.5, "starting tasks_restarting"), (6, "ready -")),
    "j/L8.l7.cut": ch((5, CUTBACK)),
    "j/L8.play.21-ask-b": refused(8, "starting"),
    "j/L9.hold": ch((0.5, HELD)),
    "a/L9.hold": ch((0.8, "error held_approval")),
    "j/L9.play.23-ask-d": refused(9, "error"),
    "a/L9.unhold": ch((0.5, "starting session_starting"), (1, "ready -")),
    "j/L9.unhold": ch((1.5, READYTOLD)),
}
HOLD = {"HOLD_CMD": "roundcmd hold", "UNHOLD_CMD": "roundcmd unhold", "HOLDMIN": "1"}
RUN = dict(RUNSH="1", MIC_YES="yes", ROUND1_SEQ="15600", ROUND2_SEQ="15601", ROUND1_CMD="roundcmd round", ROUND2_CMD="roundcmd round", APP_L6="refused", **HOLD)
NEVERBACK = ch((0.5, "recovering gateway_down"))
C = {
    # --- each leg: PASS, FAIL (a verdict rule broken), VOID (the leg could not be run as meant)
    "L0.pass": ("PASS", {}, {}),
    "L0.fail-refused": ("FAIL", {}, {"j/L0.play.20-ask-a": ch((0.1, REQ % 1), (0.2, REF % (1, "starting")), (1.5, DONE))}),
    "L0.fail-socket": ("FAIL", {}, {"out/ask.lamp.sh.L0": "ASK_NOT_SENT row8162-1 one_session_not_started\n"}),
    "L0.void-muted": ("VOID", {}, {"out/mute.lamp.sh.L0": "MUTE ON muted=True reason=privacy\n"}),
    "L0.void-unheard": ("VOID", {}, {"j/L0.play.20-ask-a": ""}),
    "L0.void-notready": ("VOID", {}, {"a/L0.start": ch((0, "starting session_starting"))}),
    "L1.pass": ("PASS", {}, {}),
    "L1.fail-not-refused": ("FAIL", {}, {"j/L1.play.21-ask-b": ch((0.1, REQ % 2), (0.3, TAKEN), (2, DONE))}),
    "L1.fail-told-twice": ("FAIL", {}, {"j/L1.gw": ch((0.3, AGENTL), (3.2, GWBACK), (3.5, KEPTNEW), (4.5, READYTOLD), (5.5, READYTOLD))}),
    "L1.fail-not-fresh": ("FAIL", {}, {"j/L1.gw": ch((0.3, AGENTL), (3.2, GWBACK), (4.5, READYTOLD)), "out/kept.lamp.sh.L1.2": "KEPT_AGENT aaaa1111bbbb\nPANEPID 4242\n"}),
    "L1.fail-slow-leave": ("FAIL", {}, {"a/L1.gw": ch((16, "recovering gateway_down"), (17, "starting session_starting"), (18, "ready -"))}),
    "L1.fail-hold2": ("FAIL", {}, {"a/L1.gw": ch((0.5, "recovering gateway_down"), (4, "ready -"))}),
    "L1.void-budget": ("VOID", {"MOCK_STARTS_lamp_openclaw_gateway": "10"}, {}),
    "L2.pass": ("PASS", {}, {}),
    "L2.fail-late-end": ("FAIL", {}, {"j/L2.gw": ch((20, BROKE), (20.5, TOLD), (3, KEPTNEW), (4.5, READYTOLD))}),
    "L2.fail-run-again": ("FAIL", {}, {"j/L2.gw": ch((1, BROKE), (1.5, TOLD), (1.8, TAKEN), (3, KEPTNEW), (4.5, READYTOLD))}),
    "L2.fail-not-told": ("FAIL", {}, {"j/L2.gw": ch((1, BROKE), (3, KEPTNEW), (4.5, READYTOLD))}),
    "L2.fail-sent": ("FAIL", {}, {"out/ask.lamp.sh.L2.1": "ASK_ANSWER row8162-9 done\n"}),
    "L2.void-not-taken": ("VOID", {}, {"j/L2.ask.1": ""}),
    "L3.pass": ("PASS", {}, {}),
    "L3.fail-slow-leave": ("FAIL", {}, {"a/L3.jam.kill": ch((3, "recovering pane_gone"), (4, "starting session_starting"), (5, "ready -"))}),
    "L3.fail-told-none": ("FAIL", {}, {"j/L3.jam.kill": ch((0.4, AGENTL), (2.5, KEPTNEW))}),
    "L3.void-no-pane": ("VOID", {}, {"out/kept.lamp.sh.L3.1": "KEPT_AGENT aaaa1111bbbb\nPANEWAY none\nPANEPID none (2 openclaw processes in the pane tree; way cwd)\n"}),
    "L4.pass": ("PASS", {}, {}),
    "L4.fail-late-end": ("FAIL", {}, {"j/L4.play.26-ask-g": ch((0.1, REQ % 6), (15, REFHAND), (15.5, TOLD), (2.5, KEPTNEW), (3.5, READYTOLD))}),
    "L4.fail-not-resumed": ("FAIL", {}, {"out/unfreeze.lamp.sh": "UNFREEZE FAIL 4242 still T\n"}),
    "L4.void-refused": ("VOID", {}, {"out/jam.lamp.sh.L4.stop": "JAM REFUSED: no single pane pid\n"}),
    "L5.pass": ("PASS", {}, {}),
    "L5.fail-no-watch": ("FAIL", {}, {"j/L5.hd": ""}),
    "L5.fail-silent": ("FAIL", {}, {"j/L5.play.20-ask-a": ch((0.1, REQ % 7))}),
    "L5.void-no-restart": ("VOID", {}, {"out/hd.lamp.sh.L5": "HD STOP: not active\n"}),
    "L6.pass": ("PASS", {"APP_L6": "refused"}, {}),
    "L6.fail-started": ("FAIL", {"APP_L6": "started"}, {}),
    "L6.void-unsent": ("VOID", {}, {}),
    "L7.pass": ("PASS", {}, {}),
    "L7.fail-order": ("FAIL", {}, {"out/l7.lamp.sh.L7.go": DEF["out/l7.lamp.sh.go"].replace("L7_ORDER PASS", "L7_ORDER FAIL (gw-started before lamp-tasks-stopped)")}),
    "L7.fail-not-live": ("FAIL", {}, {"out/l7check.lamp.sh": "L7_CONFIG NOT LIVE: models.list lacks the provider\n"}),
    "L7.fail-installing": ("FAIL", {}, {"a/L7.l7.go": ch((0.1, "installing -"), (1, "ready -"), (2, "installing -"))}),
    "L7.void-stop": ("VOID", {}, {"out/l7.lamp.sh.L7.go": "L7 STOP: the engine is not openclaw\n"}),
    "L8.pass": ("PASS", {}, {}),
    "L8.fail-cutback-twice": ("FAIL", {}, {"j/L8.l7.cut": ch((5, CUTBACK), (5.5, CUTBACK))}),
    "L8.fail-slow-back": ("FAIL", {}, {"out/l8back.lamp.sh": DEF["out/l8back.lamp.sh"].replace("@NOW+1@", "@NOW+20@")}),
    "L8.fail-no-gap": ("FAIL", {}, {"out/l7.lamp.sh.L8.cut": "L7_AT @NOW@\nCUT_AT @NOW+1@\nGAP lamp-tasks=active record=no\n"}),
    "L8.void-stop": ("VOID", {}, {"out/l7.lamp.sh.L8.cut": "L8 STOP: the install did not reach the tasks stop\n"}),
    "L9.pass": ("PASS", HOLD, {}),
    "L9.fail-churn": ("FAIL", HOLD, {"j/L9.hold": ch((0.5, HELD), (2, READYTOLD))}),
    "L9.fail-ready-early": ("FAIL", HOLD, {"a/L9.hold": ch((0.8, "error held_approval"), (1.5, "ready -"))}),
    "L9.void-no-hold-cmd": ("VOID", {}, {}),
    "L9.void-not-held": ("VOID", HOLD, {"j/L9.hold": ""}),
    # --- run.sh itself
    "RUN.pass": ("PASS", RUN, {}),
    "RUN.pass-split": ("PASS", dict(RUN, SPLIT="1"), {}),
    "RUN.pass-channel": ("PASS", dict(RUN, ROUND1_CMD="", ROUND2_CMD="", CHANNEL="1"), {}),
    "RUN.until-skip": ("STOPPED", dict(RUN, UNTIL="micon", SKIP="hfetch", WANT="lock clips base round1 verify"), {}),
    "RUN.then-step": ("STOPPED", dict(RUN, FROM="lock", THEN="base", UNTIL="round1", WANT="lock base"), {}),
    "RUN.trap-on-fail": ("TRAPPED", dict(RUN, LEGS="L4 L5"), {"a/L4.play.26-ask-g": NEVERBACK, "out/unfreeze.lamp.sh": "UNFREEZE FAIL 4242 still T\n"}),
    "RUN.trap-on-ctrl-c": ("TRAPPED", dict(RUN, LEGS="L4 L5", MOCK_INT_ON="26-ask-g"), {}),
    "RUN.lock-renew-fails": ("LOCKSTOP", dict(RUN, MOCK_RENEW_FAIL_AFTER="9"), {}),
    "RUN.lock-not-taken": ("LOCKSTOP", dict(RUN, MOCK_LOCK_RC="1"), {}),
    "RUN.volume-over-70": ("VOLSTOP", dict(RUN, MOCK_MACVOL="80"), {}),
    "RUN.volume-rises-mid-legs": ("TRAPPED", dict(RUN, LEGS="L0 L1", MOCK_MACVOL_AFTER="8:85"), {}),
    "RUN.seq-not-higher": ("STOP-base", dict(RUN, ROUND1_SEQ="15543"), {}),
    "RUN.seq2-not-higher": ("STOP-base", dict(RUN, ROUND2_SEQ="15600"), {}),
    "RUN.round-never-ends": ("STOP-round1", dict(RUN, ROUNDWAIT="3"), {"out/round.lamp.sh.read.1": "SEQ installed=15543 busy='journal.json'\nJ 1.0 lamp-update: switching to build 15600\nUPDATER running=e988\n"}),
    "RUN.round-refused": ("STOP-round1", RUN, {"out/round.lamp.sh.read.1": "SEQ installed=15543 busy=''\nJ 1.0 lamp-update: build 15600 is older than build 15601: refused, nothing switched\nUPDATER running=e988\n"}),
    "RUN.round2-not-stable-updater": ("STOP-round2", RUN, {"out/round.lamp.sh.read.2": DEF["out/round.lamp.sh.read.2"].replace("running=e988ff0a4d275afc", "running=bbbb")}),
    "RUN.speaker-over-40": ("STOP-base", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("SPEAKER Speaker 35%", "SPEAKER Speaker 55%")}),
    "RUN.speaker-unread": ("STOP-base", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("SPEAKER Speaker 35%", "SPEAKER unread")}),
    "RUN.exesha-not-as-found": ("STOP-read", RUN, {"out/read.lamp.sh.2": DEF["out/read.lamp.sh"].replace("lamp-tasks=b93a", "lamp-tasks=dead")}),
    "RUN.harness-signed-out-base-ok": ("PASS", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("logged_in=True", "logged_in=False")}),
    "RUN.engine-not-openclaw": ("STOP-base", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("ENGINE openclaw", "ENGINE hermes")}),
    "RUN.mic-slide-closed": ("STOP-base", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("physical_muted=False", "physical_muted=True")}),
    "RUN.mic-slide-unread": ("PASS", RUN, {"out/base.lamp.sh": DEF["out/base.lamp.sh"].replace("physical_muted=False", "physical_muted=None")}),
    "RUN.no-mic-yes": ("STOP-micon", dict(RUN, MIC_YES=""), {}),
    "RUN.read-twice-differs": ("STOP-read", RUN, {"out/read.lamp.sh.2": DEF["out/read.lamp.sh"].replace("INSTALLED 15601", "INSTALLED 15602")}),
    "RUN.leg-fail-still-puts-back": ("FAILREPORT", dict(RUN, LEGS="L0 L1 L2 L3 L4 L5 L6 L7 L8 L9"), {"j/L1.play.21-ask-b": ch((0.1, REQ % 2), (0.3, TAKEN), (2, DONE))}),
}

# --- #8337: leg E's reads (engines.lamp.sh N=0 on stable, N=1 on the PR build) and the cases that run.sh keeps.
E0 = '{"default":"openclaw","hermes":{"state":"not_installed"},"openclaw":{"state":"installed","version":"2026.11.10"}}'
DEF["out/engines.lamp.sh.0"] = "ENGINES0 %s\nHLOOK0 not: Harness names no machine of its own (signed out?)\nHAUTH via=cli logged_in=False offline=False\n" % E0
DEF["out/engines.lamp.sh.1"] = "ENGINES1 %s\nHLOOK1 not: Harness names no machine of its own (signed out?)\nHAUTH via=cli logged_in=False offline=False\n" % E0
E1IN = '{"claude":{"state":"installed","version":"2.0.1"},"codex":{"state":"installed"},"default":"openclaw","hermes":{"state":"not_installed"},"openclaw":{"state":"installed","version":"2026.11.10"}}'
E1LOST = E1IN.replace('"claude":{"state":"installed","version":"2.0.1"}', '"claude":{"state":"not_installed"}')
KEEP = ("RUN.pass", "RUN.pass-channel", "RUN.until-skip", "RUN.then-step", "RUN.lock-renew-fails", "RUN.lock-not-taken",
        "RUN.volume-over-70", "RUN.seq-not-higher", "RUN.seq2-not-higher", "RUN.round-never-ends", "RUN.round-refused",
        "RUN.round2-not-stable-updater", "RUN.speaker-over-40", "RUN.speaker-unread", "RUN.exesha-not-as-found",
        "RUN.harness-signed-out-base-ok", "RUN.engine-not-openclaw", "RUN.read-twice-differs")
C = {k: v for k, v in C.items() if k in KEEP}
C["RUN.then-step"] = ("STOPPED", dict(RUN, FROM="lock", THEN="base", UNTIL="round1", WANT="lock base engines0"), {})
C["RUN.until-skip"] = ("STOPPED", dict(RUN, UNTIL="round2", SKIP="engines0", WANT="lock base round1 verify engines"), {})
C["RUN.e-signed-in-pass"] = ("PASS", RUN, {"out/engines.lamp.sh.0": "ENGINES0 %s\nHLOOK0 said\n" % E1IN, "out/engines.lamp.sh.1": "ENGINES1 %s\nHLOOK1 said\n" % E1IN})
C["RUN.e-installed-said-not"] = ("EFAIL", RUN, {"out/engines.lamp.sh.0": "ENGINES0 %s\nHLOOK0 said\n" % E1IN, "out/engines.lamp.sh.1": "ENGINES1 %s\nHLOOK1 said\n" % E1LOST})
C["RUN.e-openclaw-moved"] = ("EFAIL", RUN, {"out/engines.lamp.sh.1": DEF["out/engines.lamp.sh.1"].replace('"openclaw":{"state":"installed"', '"openclaw":{"state":"failed"')})
C["RUN.e-look-said-none"] = ("EFAIL", RUN, {"out/engines.lamp.sh.1": "ENGINES1 %s\nHLOOK1 said\n" % E0})
C["RUN.e-unread"] = ("EVOID", dict(RUN, ENGWAIT="2"), {"out/engines.lamp.sh.1": "ENGINES1 unreadable:FileNotFoundError\nHLOOK1 none\n"})
if os.path.isdir(OUT): shutil.rmtree(OUT)
for name, (exp, env, over) in C.items():
    d = os.path.join(OUT, name); files = dict(DEF); files.update(over)
    for k, v in files.items():
        if v is None: continue
        p = os.path.join(d, k); os.makedirs(os.path.dirname(p), exist_ok=True); open(p, "w").write(v)
    open(os.path.join(d, "expect"), "w").write(exp + "\n")
    leg = name.split(".")[0]
    env = dict(env); env.setdefault("LEGS", leg if leg != "RUN" else "L0 L1 L2 L3 L4 L5 L6 L7 L8 L9")
    open(os.path.join(d, "case.env"), "w").write("".join("%s='%s'\n" % kv for kv in env.items()))
print("GEN %d cases" % len(C))
