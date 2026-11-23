# #8162 ROW on lamp-ee17

This kit runs #8162's ROW (head `4c663af40fd1928ce5ebb8fc20cca2d08d1604e6`, issue #8138) from this Mac, on the in-house lamp lamp-ee17 (the kit folder keeps its first name, `kits/8162-e82d/`). The
PR build goes on the lamp by a **real update round** of the lamp's own lamp-update, from stable to the CI build of the
head. It is not a /run drop-in. After the legs, a second real round puts stable's bytes back at a higher sequence, and
Harness goes back to stable's pin by hand. The kit holds no key and signs nothing: each round is either your own
command, or two channels you made and signed.

Its mocks: `mock/row/run-all.sh` runs 75 cases with stub ssh, scp, afplay, curl, osascript and lock, and recorded
lamp outputs. They cover every leg's PASS, FAIL and VOID cases, and run.sh itself: the step order, resume and split
runs, `UNTIL`, `SKIP` and then-step, the traps on a FAIL and on Ctrl-C, a lock renew that fails, a lock not taken,
the volume refusal (at the start and mid-legs), sequences that are not higher, a round that never ends or is refused,
a put-back read that differs, the lamp's speaker over 40% or unread, a unit's exe not as found at base, Harness signed out (still BASE OK), another default agent, and the mic slide closed. `mock/row/mutate.sh` breaks six verdict rules and checks a FAIL case catches each
one. The mocks are the dry run; there is no other Mac-only mode.

## Prerequisites
1. **The stable of the moment.** lamp-ee17 is on stable: build **15543**, lamp-update sha256 `e988ff0a4d27...`,
   Harness pin **0.6.75** (ee17 is on it now; stable may move at about 11:30 on 23 Nov). If stable moves before you start, pass the new one:
   `STABLE_SEQ`, `STABLE_UPD_SHA`, `STABLE_TASKS_SHA`, `HV_STABLE`, and `HMETA=<its cli-<v>.metadata.json>` (from
   `lampOS/deploy/harness/` at the new stable's commit). `base` stops when the lamp is not on that stable. A lamp
   above stable with stable's own lamp-update (after an earlier put-back at a higher sequence) is allowed.
2. **The rounds.** Pick one way:
   - **cmd mode:** `ROUND1_CMD` and `ROUND2_CMD` are your own commands. Each one makes the lamp's lamp-update take a
     round: round 1 to the CI build of the head, round 2 to stable's bytes. The kit runs each with `bash -c` and passes
     `ROUND_N`, `ROUND_SEQ`, `LAMP_HOST` and `HEAD_SHA` in its environment. Any envelope or key stays inside your
     command.
   - **channel mode:** `CH1` and `CH2` are two channel folders you made and signed (each has `lampos-manifest.json`),
     and `TEST_PUB` is their public key file. The kit copies them to the lamp, points lamp-update's own product line
     at them with /run drop-ins (removed at `putback`), and restarts lamp-update.
   - `ROUND1_SEQ` is the sequence round 1 installs. `ROUND2_SEQ` is round 2's. A round takes only a higher sequence,
     and a corrective build needs a higher one: `base` stops unless ROUND1_SEQ is above the highest sequence the lamp
     has seen (installed, previous or refused), and ROUND2_SEQ is above ROUND1_SEQ.
   - Optional: `PR_UPDATE_SHA` and `PR_TASKS_SHA` (the CI artifact's sha256). With them, `verify` requires the
     running units to be exactly that build.
3. lamp-ee17's default agent is OpenClaw, lamp-cloud is active, and the physical mic slide is open.
4. Your env file holds these names. `KIT_ENV=<path>` points at it. It is sourced, and never copied or printed.
   ```
   LAMP=lamp-ee17               # default
   LAMP_HOST=lamp-ee17.local    # default: <LAMP>.local
   LAMP_USER=orangepi           # default
   LAMP_SUDO_PASS=...           # the lamp's sudo password
   LAMP_SSH_PASS=...            # optional; unset (the default) = ssh keys
   LOCK_WHO=thirteen            # default; LOCK_SESSION=row-8162 is the default
   KIT_LL=<path>/lamp-lock.sh   # called as `lamp-lock.sh lamp-ee17 try|renew|release ...`; with none, no lock
   SPK_CTL=...                  # optional: the amixer control base reads (default: card 0's first playback control)
   ROUND1_SEQ=... ROUND2_SEQ=... and ROUND1_CMD/ROUND2_CMD, or CH1/CH2/TEST_PUB (above)
   ```
5. The Mac needs bash, python3, curl, sshpass (only with LAMP_SSH_PASS), afplay and osascript. Its output volume must
   be 70 or lower. The kit reads that volume and never sets any volume, the Mac's or the lamp's. Put the Mac's speaker
   next to the lamp. The clips are pre-rendered and come in the kit (`row/clips/`, checked by `clips`); there is no
   live TTS.
6. Optional, from the app: leg L6 needs you to send engines.install from the app while the agent is ready, then set
   `APP_L6=refused` or `APP_L6=started` (what the app said). Leg L9 needs your own `HOLD_CMD` and `UNHOLD_CMD`, which
   make and clear a lasting Harness hold. Without them, L6 and L9 are VOID, and the ROW can still PASS.

## Commands
Run everything from `kits/8162-e82d/`. One command runs every step in order:
`KIT_ENV=<env> MIC_YES=yes ./run.sh`. To resume, name a step: `./run.sh <step>`. `./run.sh <step> <then>` runs one
step, then goes on at `<then>`. `UNTIL=<step>` stops before that step. `SKIP="<step> ..."` leaves steps out.
`LEGS="L0 L1"` picks the legs; `KEEP_VERDICTS=1` keeps the verdicts of earlier pieces.

| step | what it runs | ~min | go on at |
|------|--------------|------|----------|
| lock | lamp-lock try, then a 60 s renew loop (only with KIT_LL) | 0 | `LOCK TAKEN` or `LOCK none` |
| clips | the clips' sha256 (`row/clips/SHAS`) | 0 | `CLIPS OK` |
| hfetch | Harness `HV_STABLE`'s cli.js and notify.mjs, fetched on the Mac from the URLs in stable's pin file, sha256 and size checked | 1 | `HFETCH OK` |
| base | `row/base.lamp.sh`: on stable, its updater, Harness 0.6.75, OpenClaw, the units; each unit's running exe sha256 recorded (as found); the lamp's speaker volume read (amixer, never set): over 40% or unread stops; Harness's sign-in printed as a fact only (`T HAUTH`, from `harness auth status --json` as lampos; never a stop: the lamp's own OpenClaw runs while Harness is signed out, #6407), OpenClaw the default agent, the physical mic slide open (unread: noted, not judged), else `BASE FAIL <why>` before any change; the sequences checked | 1 | `BASE OK` |
| round1 | your round 1 (cmd or channel), then lamp-update's own end: `up to date: build <ROUND1_SEQ>` after `confirmed: lamp-update runs its new build` (a self-switch) | 3-15 | `ROUND 1 OK` |
| verify | `row/verify.lamp.sh` every 30 s, up to 15 min: build ROUND1_SEQ, the PR's updater (not stable's), Harness 0.6.78 and its record, nothing newly failed, tasks/agent ready with engine openclaw | 1-15 | `VERIFY PASS` |
| micon | `row/micon.lamp.sh`: the mute recorded and lifted, only with `MIC_YES=yes` (the lamp's person's yes for this ROW) | 1 | `MUTE LIFTED OK` or `MUTE NOT MUTED` |
| legs | `row/legs.sh L0 .. L9` (or `LEGS=...`) | 45 | `VERDICT PASS` (or VOID) |
| micoff | `row/micoff.lamp.sh`: the recorded mute put back | 1 | `MUTE BACK OK` |
| round2 | your round 2: stable's bytes at ROUND2_SEQ, from the PR build's own lamp-update; then stable's updater running again | 3-15 | `ROUND 2 OK` |
| putback | `row/nostop.lamp.sh`, `row/l7back.lamp.sh`, `row/putback.lamp.sh` (channel mode: the drop-ins removed, the product line's own round) | 1-2 | `NOSTOP OK`, `L7BACK OK`, `PUTBACK OK` |
| hdown | Harness back to 0.6.75 by hand (lamp-update never steps Harness down): the fetched files copied, installed, checked | 2 | `HDOWN OK` |
| read | `row/read.lamp.sh` twice, 60 s apart: the installed sequence and commit, stable's updater, the product line, no drop-ins, Harness 0.6.75, nothing newly failed; lamp-update, lamp-tasks and lamp-voice run the exes base recorded (the same stable bytes); both READ OK and the same | 2 | `READ TWICE same` |
| report | `row/verdict.py --report`: `row/logs/row-report.md` (the kit posts nothing) | 0 | the ROW line |
| release | the lock released, only after MUTE BACK OK, PUTBACK OK, HDOWN OK and READ TWICE same (`FORCE=1` releases anyway) | 0 | `LOCK RELEASED` |

## Split run: every piece 20 min or less
Every piece needs the same `KIT_ENV` and the lock (the lock step's renew loop keeps running between pieces; take it
again with `./run.sh lock <next step>` if it lapsed). Pieces 3-7 each lift the mute and put it back.

| piece | command | ~min |
|-------|---------|------|
| P1 | `UNTIL=verify ./run.sh` | 5-20 |
| P2 | `UNTIL=micon ./run.sh verify` | 1-16 |
| P3 | `MIC_YES=yes LEGS="L0 L1" UNTIL=round2 ./run.sh micon` | 9 |
| P4 | `MIC_YES=yes KEEP_VERDICTS=1 LEGS="L2 L3" UNTIL=round2 ./run.sh micon` | 13 |
| P5 | `MIC_YES=yes KEEP_VERDICTS=1 LEGS="L4 L5 L6" UNTIL=round2 ./run.sh micon` | 11 |
| P6 | `MIC_YES=yes KEEP_VERDICTS=1 LEGS="L7" UNTIL=round2 ./run.sh micon` | 9 |
| P7 | `MIC_YES=yes KEEP_VERDICTS=1 LEGS="L8 L9" UNTIL=round2 ./run.sh micon` | 14 |
| P8 | `UNTIL=hdown ./run.sh round2` | 5-17 |
| P9 | `./run.sh hdown` | 5 |

A leg to rerun: `MIC_YES=yes KEEP_VERDICTS=1 LEGS="L3" UNTIL=round2 ./run.sh micon`.

## Safety
- Lock: `KIT_LL` only. Every step, leg and wait renews it; a failed renew stops the run before any more ssh, and the
  renew loop's `RENEW FAILED` stops the next step.
- The EXIT traps (run.sh and legs.sh), on any exit, a FAIL, a STOP or Ctrl-C, each once: SIGCONT to a pid leg L4
  stopped (by number), the OpenClaw wrapper and config copies of L7/L8 put back, `UNHOLD_CMD` after L9's hold, and
  the recorded mute put back. With the lock lost they only say what is left to do.
- No volume is ever set. The lamp's speaker volume is only read, at `base`: over 40% stops the run. lamp-cloud is never stopped. No factory reset, no forget-all.
- `GATE`: no unit starts more than 10 times in any 10 min (the lamp's own journal counts). A leg without the budget is
  VOID, a step without it stops.
- Credentials come only from `KIT_ENV`; the sudo password goes only to sudo's stdin.

## The legs and what PASS means
A leg is VOID, not FAIL, when it could not run as meant: no start budget, the mic muted, the agent not ready at the
start, the clip not heard (no request line in 30 s, or held back as said to nobody), the break not made. If a fault
leg's agent never comes back to ready, the legs after it are not run (`STOP`, exit 3): look at the lamp.

| leg | the break | PASS when |
|-----|-----------|-----------|
| L0 | none | ready at the start; the spoken ask is taken and answered (turn done, not refused); a socket ask after is answered |
| L1 | the gateway restarted under an idle session | tasks/agent leaves ready within 14 s (the 1 s target is noted when missed); the ask is refused at once with no hand-over before ready, first answer audio within 2000 ms; the Gateway answers -> ready within 15 s; a fresh session (kept new, a new kept agent); HOLD 2: recovering -> starting(session_starting) -> ready; "ready again" told once; the next spoken ask answered |
| L2 | the gateway restarted mid-task (a long socket task, 8 s in) | the task ends within 14 s; the store ends it not_sent (one_session_stuck or not_started); told it was not done; never run again in 120 s; a fresh session; HOLD 2; ready again told once; the next ask answered |
| L3 | the kept pane's openclaw pid killed (by number) | leaves ready within 2 s; refused at once (audio within 2000 ms); a fresh session; HOLD 2; ready again once; a socket ask after answered |
| L4 | that pid stopped (SIGSTOP) | the ask ends within 10 s at the first refusal, or is refused at once; told at once; back to ready; the pid resumed (SIGCONT); a fresh session; HOLD 2; ready again once; a socket ask after answered |
| L5 | Harness's daemon restarted | the watch taken again within 120 s; readiness ends at ready; the ask during the restart answered or refused, never silent; a socket ask after answered |
| L6 | engines.install from the app | the agent was ready and the app said refused (`APP_L6=refused`); VOID when not sent |
| L7 | OpenClaw's install (first-install config) | the order: tasks stop said < lamp-tasks stopped < gateway started < health < lamp-tasks started < start said; lamp-tasks' merge live with no second restart; ready after, never installing again; a socket ask after answered |
| L8 | that install cut off between lamp-tasks' stop and start | the gap: lamp-tasks stopped and its stop recorded; tasks/agent starting(tasks_restarting); the ask refused at once; lamp-tasks back within 12 s of the one-shot's end; the cut-back line said once; the record gone; ready, not left at installing; a socket ask after answered |
| L9 | a lasting hold (`HOLD_CMD`) | error(held_*) within 1 s of the held line; the ask refused; held at least 180 s with no fresh session and no "ready again"; ready only after the clear; ready again once. VOID without HOLD_CMD |

The ROW is **PASS** when round 1, verify and round 2 are OK, L0-L5, L7 and L8 PASS, and no leg FAILs (L6 and L9 may
be VOID). It is **FAIL** when any of those fail. `report` refuses to write it before the put-back (putback, hdown,
read) is done, or while a required leg has no PASS yet (VOID or not run: rerun it).

## The ROW comment (bot form)
`row/logs/row-report.md` is the comment. Post it yourself. Its first line is exactly:
```
ROW #8162 4c663af40fd1928ce5ebb8fc20cca2d08d1604e6 on lamp-ee17 build <ROUND1_SEQ>: PASS
```
(or `: FAIL`, then a line `Failed: <steps and legs>`). Then a fenced journal block: round 1's own end lines first (so
its first 400 characters name `build <ROUND1_SEQ>`), each leg's key journal lines, round 2's end line. Then a table
of each leg's verdict and numbers (refused ms, first audio ms, left ready s, task ended s, health -> ready s, fresh ->
ready s).

## Logs
Everything goes to `row/logs/` (ignored by git): `times.log` (every step), `round1.txt`, `verify.txt`, `<leg>.*`
(journal, aw, marks, verdict, keylines), `verdicts.txt`, `legs-<names>.txt`, `putback.txt`, `hdown.txt`, `read1.txt`,
`read2.txt`, `read.txt`, `row-report.md`.
