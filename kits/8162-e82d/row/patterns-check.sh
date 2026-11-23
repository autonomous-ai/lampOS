#!/bin/bash
# Mac, read only: each pattern's fixed text (patterns.env) still in the lampOS tree that was built.
# usage: ./patterns-check.sh <lampOS tree, e.g. a worktree's lampOS/ at the built head>. Prints ok|MISS per pattern,
# then PATTERNS OK | PATTERNS MISS <n>.
T=${1:?lampOS tree (.../lampOS)}; miss=0
chk(){ grep -qF -- "$2" "$T/crates/$3" && echo "ok   $1" || { echo "MISS $1: '$2' not in crates/$3"; miss=$((miss + 1)); }; }
chk P_AGENT 'tasks: agent: {} -> {}{}' tasks/src/agent.rs
chk P_T_REFUSEDHAND 'refused the hand-over ({why}); the task ends now, and a fresh session is opened after (#8138)' tasks/src/one_session.rs
chk P_T_WARMING 'refused the hand-over while warming up ({took:?}); the task ends now, the session is kept (#8138)' tasks/src/one_session.rs
chk P_T_BROKE 'its session broke ({why}); the task ends now, never run again' tasks/src/one_session.rs
chk P_T_STUCK 'is gone or stuck ({why}); closed, forgotten and replaced with a fresh session (#7906)' tasks/src/one_session.rs
chk P_T_KEPT 'tasks: one session: kept session {} {} ({}, {}){} in {} ms' tasks/src/one_session.rs
chk P_T_WATCHAGAIN "'s watch ended ({code}); ensured and watched again" tasks/src/one_session.rs
chk P_T_GWBACK "OpenClaw's Gateway answers again; opened at once" tasks/src/one_session.rs
chk P_T_HELD "Harness's watch says it is held ({why}); only the person can clear it, so no fresh session (#8138)" tasks/src/one_session.rs
chk P_T_DONE "'s turn is done; the queue goes on" tasks/src/one_session.rs
chk P_T_TAKEN 'tasks: harness {verb}:' tasks/src/harness.rs
chk P_T_TAKEN2 ' exit={exit} ms={ms} out={}' tasks/src/harness.rs
chk P_V_HELD 'its answer is held back: {why}' voice/src/second_provider.rs
chk P_V_HELD2 'nobody spoke to the lamp: no name' voice/src/attention.rs
chk P_V_REFUSED "the lamp's own agent is {:?}: refused at once, nothing prepared" voice/src/agent_tasks.rs
chk P_V_AUDIO 'first answer audio {first_ms} ms after the words were heard' voice/src/second_provider.rs
chk P_V_TOLD 'lamp-voice: telling news from the work handed over (' voice/src/coordinator.rs
chk P_V_READYTOLD 'agent-ready-{}' voice/src/task_news.rs
chk P_V_READYOFFER "the lamp's own agent is ready again after a refusal: told once at a quiet moment" voice/src/task_news.rs
chk PROBLEM_LINE "I'm having a problem right now; ask me again in a moment." voice/src/agent_ready.rs
chk LATER_LINE "I'm having a problem right now; ask me again later." voice/src/agent_ready.rs
chk UPDATING_LINE "I'm being updated right now; ask me again later." voice/src/agent_ready.rs
chk READY_AGAIN "I'm ready again; you can ask me now." voice/src/agent_ready.rs
chk P_U_TASKSSTOP '{TASKS_UNIT} stopped while {UNIT} restarts' update/src/openclaw_install.rs
chk P_U_HEALTH '{UNIT} answers health {} s after its restart' update/src/openclaw_install.rs
chk P_U_TASKSBACK '{TASKS_UNIT} started again' update/src/openclaw_install.rs
# #8263 moved the cut-back to harness_install::tasks_back, formatted from TasksStop::Gateway's words: the same line.
chk P_U_CUTBACK '{TASKS} started again: {what} ended while it was stopped for {for_what} (#8138)' update/src/harness_install.rs
chk P_U_CUTBACK_WORDS "TasksStop::Gateway => (\"OpenClaw's install\", \"the gateway's restart\")" update/src/harness_install.rs
chk P_U_END 'switched to build {sequence}; every part is well' update/src/round.rs
chk P_U_SELF 'lamp-update switches to its build {}; it must confirm itself' update/src/round.rs
chk P_U_CONFIRMED 'confirmed: lamp-update runs its new build' update/src/main.rs
chk P_U_HMOVED 'Harness {from} is moved to the pinned {want}' update/src/harness_install.rs
chk L6_REFUSAL 'the agent is installed and running' cloud/src/update.rs
chk HARNESS_PIN 'pub const VERSION: &str = "0.6.78";' update/src/harness_install/product.rs
[ $miss = 0 ] && echo "PATTERNS OK" || echo "PATTERNS MISS $miss"
