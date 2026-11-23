# #8180 legs ROW on lamp-e82d

This kit runs the legs half of #8180's ROW from the runner's Mac. It does not install or round a build. You put
#8180's build on the lamp before you start, and you put the lamp back to stable after the kit ends (or let the optional
`putback` step run your own put-back command, below).

It ran on e82d twice (22 Nov, the second at 14:38). Its mocks: `mock/row/run-all.sh` runs 104 cases: every leg's
PASS, FAIL and VOID cases, plus run.sh itself, with stub ssh, scp, afplay and lock. There is no other Mac-only dry mode:
the mocks are the dry run, `RERUN.pass` and `RUN.pass-rerun` among them for the rerun's legs. Rendering the clips needs
the lamp's own ElevenLabs relay (over ssh), so it can't be dry-run on the Mac alone.

## Prerequisites
1. lamp-e82d runs #8180's CI ARM64 artifact, installed by your own /run drop-in. `check` prints each unit's running
   exe and drop-ins. If you set `PR_TASKS_SHA`, `PR_CONTROL_SHA`, `PR_SKILLS_SHA`, `PR_VOICE_SHA` and
   `PR_UPDATE_SHA` (the artifact's sha256), `check` also requires each running unit to be exactly that build.
2. lamp-e82d's default agent is OpenClaw, lamp-cloud is active, and the physical mic slide is open.
3. Your env file holds these names. `KIT_ENV=<path>` points at it. It is sourced, and never copied or printed.
   ```
   LAMP=lamp-e82d               # default
   LAMP_HOST=172.168.20.193
   LAMP_USER=orangepi           # default
   LAMP_SUDO_PASS=...           # the lamp's sudo password
   LAMP_SSH_PASS=...            # ssh password, used by sshpass -e; leave it out to use ssh keys
   LOCK_WHO=migrate             # default; LOCK_SESSION=row-8180 is the default
   KIT_LL=<path>/lamp-lock.sh   # optional. With none, no lock is taken or renewed.
   ```
4. The Mac needs bash, python3, sshpass, ffmpeg and ffprobe (for render), afplay and osascript. Its output volume
   must be 70 or lower. The kit reads that volume and never sets it. Put the Mac's speaker next to the lamp.
5. **Render first.** Only YES and YES2 come in the kit. `render` makes the other 14 clips, NAMED among them,
   through e82d's own ElevenLabs relay. It needs the relay's credential on the lamp and changes no lamp state.
   Every clip that starts a turn opens with "Hey Lamp," said as a call (the mock `U.pass-clips-call` checks each
   clip row.sh asks or makes with); only YES, an answer to the lamp's own question, does not.

## Commands
Run everything from `kits/8180-e82d/`. One command runs every step, in this order:
`KIT_ENV=<env> MIC_YES=yes ./run.sh`. To resume, name a step: `./run.sh <step>`. `./run.sh <step> <then>` runs
one step, then goes on at `<then>`. `UNTIL=<step>` stops before that step, and the EXIT trap still puts the mute back.

| step | what it runs | ~min | go on at |
|------|--------------|------|----------|
| lock | lamp-lock try, then a 60 s renew loop (only with KIT_LL) | 0 | `LOCK TAKEN` or `LOCK none` |
| render | `row/render.sh` (skips clips already rendered) | 3 | `RENDER OK` or `RENDER nothing to do` |
| check | `row/check.lamp.sh`: the build, #8180's cgroup, ready | 1 | `VERIFY PASS`, `READY` |
| micon | `row/micon.lamp.sh`: the mute is recorded and lifted, only with MIC_YES=yes (the lamp's person's yes for this ROW) | 1 | `MUTE LIFTED OK` or `MUTE NOT MUTED` |
| save | `row/mute.lamp.sh`, then `row/save.lamp.sh` (baselines; the test ledgers are cleared; `LINKED` Google mail) | 1 | `MUTE OFF OK`, `SAVED OK` |
| legs | `row/row.sh A B C D K H P N E` (or `LEGS=...`) | 55 | `VERDICT PASS` |
| tails | `row/row.sh F G` (reads the verdicts of A, B and D from logs/verdicts.env) | 1 | `VERDICT PASS` |
| boot | `row/boot7170.sh`: one reboot | 4-9 | `LEG 7170 PASS` |
| unrow | `row/unrow.lamp.sh`: drop-ins, the row's skills, made.json | 1 | `UNROW OK` |
| micoff | `row/micoff.lamp.sh`: the recorded mute put back | 1 | `MUTE BACK OK` |
| putback | optional, only with `PUTBACK_CMD`: your own put-back command, then `row/putback.lamp.sh` until it is installed | 0-20 | `PUTBACK OK` or `PUTBACK skipped` |
| release | the lock released (needs UNROW OK, MUTE BACK OK, and PUTBACK OK when PUTBACK_CMD is set) | 0 | `LOCK RELEASED` |

The whole run takes about 70-75 min. **A split rerun** (rows of 20 min or less): `LEGS="A B"`, then `KEEP_VERDICTS=1 LEGS="C D K"`, then `KEEP_VERDICTS=1 LEGS="H P G"`, each with `SKIP="tails boot"`. `save` clears `logs/verdicts.env` unless `KEEP_VERDICTS=1`, and G reads A, B and D from it, so runs 2 and 3 set it. After it, do your own put-back to stable, unless the putback step did it.

`LEGS="..."` picks the legs step's legs. `SKIP="<step> ..."` leaves steps out. **The rerun of the failed legs** (First,
22 Nov 14:39: A, B, C, D, K, H, G and P on the same build, 15510; 7170 stays with #7170), from a fresh lock:
```
KIT_ENV=<env> MIC_YES=yes LEGS="A B C D K H G P" SKIP="tails boot" ./run.sh
```
It runs lock, render (nothing to do when the clips are already rendered), check, micon, save, the eight legs, unrow,
micoff, putback (skipped without PUTBACK_CMD) and release. G runs inside the legs step and reads A and B of the same run.

**Put-back above a higher ROW sequence.** lamp-update refuses a sequence lower than or equal to the one installed
(lampOS docs/updates.md: "A corrective build needs a higher sequence"). When the ROW's sequence is above stable's, stable's own
envelope is refused, and stable's bytes must be re-signed at a higher sequence. The kit holds no key and signs nothing.
`PUTBACK_CMD='<your command>' PUTBACK_SEQ=<N>` runs your command on the Mac (it gets PUTBACK_SEQ and LAMP_HOST) after
micoff. The step first stops if N is not above the lamp's installed sequence, then waits up to `PUTBACK_WAIT` (1200 s)
for lamp-update to have installed N with no round under way.

On ANY exit (a STOP, a FAIL or Ctrl-C), the EXIT traps do two things, each once:
- they take leg N's switch off, if it was asked on;
- they put the recorded mute back, if micon lifted it.

They keep the lock. The kit never sets any volume and never stops lamp-cloud. GATE allows at most 10 starts of a unit
in 10 min: a start is refused at 9 or more. A make leg (A, B, D, K) can take up to 11 min.

How the legs wait (since 14:38's run):
- Each fresh-turn ask waits until the lamp's own last say has ended (HUSH: its pieces line plus chars/12 + 3 s, at most
  45 s), as a person would.
- If the voice holds a leg's ask back as said to nobody (`its answer is held back: nobody spoke to the lamp`, voice
  attention.rs), a leg that would FAIL is VOID: "ask not heard as a call". The product's rule (#8136,
  `LAMP_NAME_FACE_OR_WORDS`) stays on.
- Before every make (A, B, D, K, E), MAKEIDLE waits up to `IDLEWAIT` (600 s, read every 15 s) until no long make is
  running on. A make runs on when its journal says `it runs on`. It ends with `a long make was made`, `a long make made
  nothing`, or lamp-tasks' `skill make: the run ended with an answer` or `refused <code>:`. If a make is still running at
  the bound, or lamp-tasks says `skill make: refused busy, one is being made`, the leg is VOID: "a make still running".
  E runs last by default for the same reason.
- Every ask's wait for its run or answer is at least 180 s (`RUNWAIT`). D's task gets 240 s (`TASKWAIT`), and a make
  gets 660 s (`MAKEWAIT`). The first make after a round may wait on a cold kept OpenClaw session.
- The kept session's cold start is printed as `INFO leg <L> cold start (#8237, no verdict): ...`. It covers the
  `kept session new ... in N ms`, `slow chat send ...stage=startup` and `first words not taken` lines.

## Each leg's pass lines (row/logs/row-*.txt, boot7170.txt)
- **A-E**: every make prints `MADELINE <"Made it, and I tried it: ..." / "Made <name>, and I tried it.">` from the
  journal, or `none (not in the journal)`. A (a days-left-in-the-year skill: no account and no net, so it runs on a
  signed-out lamp): the card has 3 of 3 sections, no stub, made_with = the engine, the try passed having said at least
  one line, the skill is kept, and DAYSASK starts its own run, which ends exit 0 having said, with a say after it. B: `hand-over to script start N ms`, no agent session, and the first say <= 6 s after the clip.
  C: lamp-control `refused: no_grant`. D: the prompt names SKILL.md first, no lamp files are read, and the answer
  comes <= 45 s after the clip. D needs a linked Google account; VOID without one. `save` prints `LINKED google <codes>` from
  lamp-services' store (`/var/lib/lampos-services/gmail.json` or `gmail-app.json`, not expired and unrenewable): a mail
  make not kept is VOID only on `LINKED google none`; with a usable link it is a FAIL. The LEG line says which. E: the make runs on, READY is answered while it makes, and the switch is off again.
- **K**: CHECK (K1) made and kept, (K2) the try's token is exactly `[camera, speak]` (lamp-control: `a make's tried run,
  a token for [speak, camera], its writes stubbed`), (K3) the run's token is exactly `[camera, speak]`, any order,
  nothing more, (K4) the say was spoken, (K5) no yes asked. The camera's grant is `camera` and the mic's `microphone`;
  `look` and `hear` are presence and own-phrase grants.
- **H**: CHECK (H1) the old run-* leaf is gone, (H2) serve's new pid is in `serve`, memory/cpu/pids are in
  subtree_control and the unit root is empty, (H3) `each run gets its own cgroup under` is back with no
  `runs have no own cgroup` / `refused, no own cgroup` line, (H4) a fresh slow-probe run ends exit 0 with a say.
- **P**: CHECK (P1) refused at the cap (the leaf's pids.events max > 0, or the script's refused count), (P2) the
  price run answers (overlap reported). P needs the price skill B made and kept, not B's PASS. On 22 Nov at 14:38, B
  failed and P went VOID only for that reason.
- **N** (#7269): CHECK (N1) with the switch off (as on stable), NAMED is not handed to the agent in code, (N2) with
  it on, `lamp-voice second: request R: the heard words named <agent> for the work: round 1 hands them over in code
  (LAMP_NAMED_AGENT_TAKES_LINE)`, (N3) `NAMED OFF` again.
- **F**: CHECK (F1) admin.sock is hidden in every try, (F2) B's net try reaches out by its egress socket.
- **G**: CHECK (1a) the own-cgroup line, (1b) never "no own cgroup", (2) serve holds MainPID and the unit root is
  empty, (3) the run-* leaf is exactly 536870912 / 469762048 / 0 / "100000 100000" / 72 and is gone after, (4) the
  price run answers. If A or B is VOID and neither failed, G is VOID.
- **7170**: CHECK (7170-1) ssh is back after one reboot (within 300 s), (7170-2) port 18789 is listening (within
  300 s), (7170-3) D's task skill (or, with D VOID for lack of linked mail, leg A's made skill when it is a task step) is asked within 30 s of the port, (7170-4) `[lamp-tasks] skill start: run R's
  script started`. `STARTUP unit->port X s, port->approvals Y s, unit->approvals Z s` is in logs/boot-times.txt.
  "Approvals" is the time lamp-tasks' own `openclaw approvals get --gateway` first succeeded, marked by the
  allow-list add that follows it. After the reboot, the leg prints the mute it reads. A muted lamp is lifted again
  only through the same yes path, MIC_YES=yes. Without that yes, the leg is VOID.
- **W** (#8130): VOID for this ROW, because #8130 is not in #8180's artifact. It is not run.
- **Hermes pass**: none.

A leg that could not run (a fixture, the start budget, a switch) is VOID, not FAIL. VERDICT is PASS, VOID (none
failed) or FAIL.

## On a STOP
- `STOP at check`: the lamp is not on #8180's build, or it is not ready. Read row/logs/check.txt. Fix the drop-in,
  then `./run.sh check`.
- `STOP at micon`: MIC_YES is missing, or the physical slide is closed (that needs a hand). The kit lifts nothing.
- `STOP before <step>: the mute was put back`: run `./run.sh micon <step>`. It lifts the mute again on the same
  MIC_YES=yes, then goes on at `<step>`. Running save a second time stops by design: its baselines are kept.
- A STOP inside legs or tails: read row/logs/row-*.txt. To redo legs, start at `./run.sh legs`. Otherwise go on with
  `./run.sh unrow`. A failed leg never blocks the put-back.
- `LEG 7170 FAIL (ssh not back ...)`: the lamp did not come back from the reboot. Look at it. Once it is back, run
  `./run.sh unrow` (it continues with unrow, micoff and release).
- `WARN ... NOT off` or `WARN ... NOT back`: run `./run.sh unrow`, or `./run.sh micoff`. Keep the lock until you
  read `UNROW OK` and `MUTE BACK OK`.
- `STOP at putback`: PUTBACK_SEQ is not above the lamp's installed sequence, PUTBACK_CMD failed, or the sequence
  was not installed in time. Read row/logs/putback.txt, then `./run.sh putback`.
- `LOCK: not ours`: the lock was lost. Stop, and run no ssh.
- After any stop, the lock and its renew loop stay. `FORCE=1 ./run.sh release` drops them.
