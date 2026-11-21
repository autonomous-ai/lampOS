# #8180 legs ROW on lamp-e82d

This kit runs the legs half of #8180's ROW from the runner's Mac. It does not install, round or put back a build. You
put #8180's build on the lamp before you start, and you put the lamp back to stable after the kit ends.

Nothing here has run on a lamp yet. It has only been mocked: `mock/row/run-all.sh` runs 61 cases: every leg's PASS, FAIL and
VOID cases, plus run.sh itself, with stub ssh, scp, afplay and lock.

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
| save | `row/mute.lamp.sh`, then `row/save.lamp.sh` (baselines; the test ledgers are cleared) | 1 | `MUTE OFF OK`, `SAVED OK` |
| legs | `row/row.sh A B C D E K H P N` | 55 | `VERDICT PASS` |
| tails | `row/row.sh F G` (reads the verdicts of A, B and D from logs/verdicts.env) | 1 | `VERDICT PASS` |
| boot | `row/boot7170.sh`: one reboot | 4-9 | `LEG 7170 PASS` |
| unrow | `row/unrow.lamp.sh`: drop-ins, the row's skills, made.json | 1 | `UNROW OK` |
| micoff | `row/micoff.lamp.sh`: the recorded mute put back | 1 | `MUTE BACK OK` |
| release | the lock released (needs UNROW OK and MUTE BACK OK) | 0 | `LOCK RELEASED` |

The whole run takes about 70-75 min. After it, do your own put-back to stable.

On ANY exit (a STOP, a FAIL or Ctrl-C), the EXIT traps do two things, each once:
- they take leg N's switch off, if it was asked on;
- they put the recorded mute back, if micon lifted it.

They keep the lock. The kit never sets any volume and never stops lamp-cloud. GATE allows at most 10 starts of a unit
in 10 min: a start is refused at 9 or more. A make leg (A, B, D, K) can take up to 11 min.

## Each leg's pass lines (row/logs/row-*.txt, boot7170.txt)
- **A-E**: every make prints `MADELINE <"Made it, and I tried it: ..." / "Made <name>, and I tried it.">` from the
  journal, or `none (not in the journal)`. A: the card has 3 of 3 sections, no stub, made_with = the engine, and the
  try passed. B: `hand-over to script start N ms`, no agent session, and the first say <= 6 s after the clip.
  C: lamp-control `refused: no_grant`. D: the prompt names SKILL.md first, no lamp files are read, and the answer
  comes <= 45 s after the clip. E: the make runs on, READY is answered while it makes, and the switch is off again.
- **K**: CHECK (K1) made and kept, (K2) the try's token names look or hear, (K3) the run's token names look/hear and
  speak, (K4) the say was spoken, (K5) no yes asked.
- **H**: CHECK (H1) the old run-* leaf is gone, (H2) serve's new pid is in `serve`, memory/cpu/pids are in
  subtree_control and the unit root is empty, (H3) `each run gets its own cgroup under` is back with no
  `runs have no own cgroup` / `refused, no own cgroup` line, (H4) a fresh slow-probe run ends exit 0 with a say.
- **P**: CHECK (P1) refused at the cap (the leaf's pids.events max > 0, or the script's refused count), (P2) the
  price run answers (overlap reported).
- **N** (#7269): CHECK (N1) with the switch off (as on stable), NAMED is not handed to the agent in code, (N2) with
  it on, `lamp-voice second: request R: the heard words named <agent> for the work: round 1 hands them over in code
  (LAMP_NAMED_AGENT_TAKES_LINE)`, (N3) `NAMED OFF` again.
- **F**: CHECK (F1) admin.sock is hidden in every try, (F2) B's net try reaches out by its egress socket.
- **G**: CHECK (1a) the own-cgroup line, (1b) never "no own cgroup", (2) serve holds MainPID and the unit root is
  empty, (3) the run-* leaf is exactly 536870912 / 469762048 / 0 / "100000 100000" / 72 and is gone after, (4) the
  price run answers.
- **7170**: CHECK (7170-1) ssh is back after one reboot (within 300 s), (7170-2) port 18789 is listening (within
  300 s), (7170-3) D's task skill is asked within 30 s of the port, (7170-4) `[lamp-tasks] skill start: run R's
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
- `LOCK: not ours`: the lock was lost. Stop, and run no ssh.
- After any stop, the lock and its renew loop stay. `FORCE=1 ./run.sh release` drops them.
