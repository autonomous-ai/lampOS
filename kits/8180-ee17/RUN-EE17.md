# #8180 legs ROW on lamp-ee17 (stable, #8180's parts bound over it)

This is the e82d kit (`../8180-e82d`, at `1672a5fd25`) retargeted to lamp-ee17, the in-house lamp. It is the fallback
if e82d's run has no results by 10:30 (manager, 23 Nov). Everything about the legs, their waits, the clips, the
traps, the name drop-in and the split is as `../8180-e82d/RUN-E82D.md` describes. This file lists only what differs.

## What differs from e82d

- **The lamp stays on its installed stable build** (15543, or G once G is stable). Nobody installs #8180's build
  first. Instead, two new steps run:
  - **`swap`** (after render): `swap/swap.sh` binds #8180's four runtime parts read-only over the installed
    files: lamp-tasks, lamp-control, lamp-skills and lamp-voice. lamp-update's apply is held off while they're bound.
    Each unit starts once. lamp-update itself is not swapped, because #8180 changes only its tests.
  - **`restore`** (after unrow): `swap/restore.sh` unmounts the binds, removes the hold and the test directories,
    starts the four units on the installed build, and checks each one's sha is the installed one again.
- **`release`** needs `UNROW OK`, `RESTORED OK` and `MUTE BACK OK`.
- **The binds are gone at reboot.** The `boot` leg (#7170) therefore isn't run here: use `SKIP="tails boot"`, as in
  the split.
- **No put-back build.** The lamp never left stable, so `putback` is skipped (leave `PUTBACK_CMD` unset).
- **On any exit with the binds still on**, the EXIT trap says so: `./run.sh unrow restore`.
- `LAMP` defaults to `lamp-ee17`. Set `LAMP_HOST=lamp-ee17.local` and `LAMP_USER=orangepi` (key login; no
  `LAMP_SSH_PASS`). `KIT_LL` is `lamp-lock.sh`.

## ART and the shas

`ART=<folder>` holds `lamp-tasks`, `lamp-control`, `lamp-skills` and `lamp-voice` from #8180's head's CI ARM64
artifact (or `PKGS="lamp-tasks lamp-control lamp-skills lamp-voice" arm64-local/build-arm64.sh`), with their
`SHA256SUMS`. swap.sh refuses a binary whose sha isn't its SHA256SUMS line, or that isn't ARM64. Set `PR_TASKS_SHA`,
`PR_CONTROL_SHA`, `PR_SKILLS_SHA` and `PR_VOICE_SHA` to those sums, so that `check` requires each running unit to be
exactly that build.

## The split (rows of 20 min or less)

```
E="KIT_ENV=<env> MIC_YES=yes ART=<folder> PR_TASKS_SHA=.. PR_CONTROL_SHA=.. PR_SKILLS_SHA=.. PR_VOICE_SHA=.."
env $E LEGS="A B" SKIP="tails boot" ./run.sh
env $E KEEP_VERDICTS=1 LEGS="C D K" SKIP="tails boot" ./run.sh
env $E KEEP_VERDICTS=1 LEGS="H P G" SKIP="tails boot" ./run.sh
```

Each run is whole, from lock to release, as on e82d: it swaps, runs its legs, and puts everything back. Each of the four units
starts twice more per run (once at swap, once at restore), well inside GATE's 10 in 10 min.

## Mocks

`mock/row/run-all.sh` runs the e82d cases (with the voice-idle wait, `row/idle.lamp.sh`) plus these for ee17:
- `RUN.pass-swap`: the whole run, ending `RESTORED OK`.
- `RUN.void-badart`: one ART binary doesn't match its SHA256SUMS, so swap stops before any lift.
- `RUN.void-swapfail`: a unit isn't on the test build after the binds, so the run stops at swap.
- `RUN.fail-restore`: restore finds a bind it didn't make, so the run stops, puts the mute back and keeps the lock.
- `RUN.void-stalesock`: the gateway's namespace holds an older lamp-control socket than the host's (#8320), so the
  run stops at verify, before any lift.
- `U.pass-gateway-8320`: swap and restore both restart lamp-openclaw-gateway after lamp-control, and check fails on a
  stale socket.

## #8320 on stable

ee17's stable (15543) has no #8323. OpenClaw's gateway binds lamp-control's socket into its namespace once, when it
starts, and a skill run's fence inherits that namespace. The swap restarts lamp-control, so the old gateway would keep
a dead socket, and every skill-run leg would fail for a reason that isn't #8180. So:
- `swap` and `restore` restart lamp-openclaw-gateway after the four units start, and wait for its port (at most 90 s).
- `swap` also lays #8180's two unit lines as `/run` drop-ins, which a binary bind can't bring: lamp-tasks' `Delegate=memory cpu pids` (each run's own cgroup) and lamp-control's `LAMP_MADE_CODE_ON_AT_ONCE=on`. `restore` removes them and restarts the units.
- `check` compares the socket's inode on the host with the one in the gateway's namespace (`nsenter -m`). If they
  differ, it prints `VERIFY FAIL: control-sock-stale-in-gateway(#8320)`, and the run stops before any lift.

The ROW line says the gateway was restarted after the swap.
