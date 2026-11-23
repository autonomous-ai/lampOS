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

## Fixtures by the product's push install (legs C, H, P)

On the lamp, a hand install (`lamp-skills install`) of lemon-probe, slow-probe or fork-probe is refused: "do.task:
only a skill from the Skills Store or pushed by the owner's own computer may carry a script" (skills `task.rs`). So the
fixtures go in by the product's own push install, as the #8237 kit does (thirteen/kit-8237 `row/stage.sh`, ported as
`row/push.sh` + `row/probe.lamp.sh`):
- the Mac packs the fixture with the SDK's `lampos.push.pack` (this repo's `lampOS/sdk/python`, no pip, no pairing;
  `PUSH_SDK` overrides the path), with a run mark in SKILL.md, and copies `package.tar` to the lamp;
- the lamp body writes `/var/lib/lampos-control/push/{push.json, package.tar}` as lamp-control's door keeps a push
  (client `c_row8180`), lays a one-request `LAMP_SKILLS_ANY_ASKER=on` drop-in on `lamp-skills-manage@`, sends
  `{"install_pushed": SHA}` to `/run/lampos-skills-manage/manage.sock`, and removes the drop-in right after the answer.
  The skill lands in `/var/lib/lampOS/skills-dev`. Its name goes into `~/t8180/pushed.names`.
- **The network door (the Mac's pairing with lamp-control) is skipped**: say so on the ROW line. Everything from the
  helper on is the product's path.
- `unrow` sends `{"remove_pushed": {client, name}}` for every pushed name (drop-in laid and removed the same way),
  puts `task-yes.json` back as found, and prints `PASS pushed fixtures gone from ..., no ANY_ASKER drop-in` or a FAIL.
- Every lamp change goes into `~/changes.txt`.

## Speaker volume (read only)

`save` prints `VOLUME saved kept=<n> bus=<n> muted=<b>` (`/var/lib/lampos-output-caps/kept-volume` and the bus's
`speaker/volume`). `unrow` reads both again: `PASS volume ... = saved`, or `FAIL volume moved <saved> -> <now> (a
leg's own volume call; not changed back: the user decides)`, which makes `UNROW NOT OK`, so `release` waits for a
person. The kit never sets a volume.

## Leg B and a second make of the same ask

A second chat round may call make_skill again while the first make runs; lamp-voice then logs "a skill wasn't made
again: the same ask is being made already". MAKE skips that line (a NOTE) and waits for the first make's own kept or
not-kept line (mock `B.pass-again`).

New mocks: `U.pass-push-probe`, `U.pass-volume-same`, `U.pass-volume-moved`, `B.pass-again`.

## The asks (ee17, 23 Nov, 17:2x run)

- **Only a plain ask is matched in code.** The heard line must be the skill's name or one of its asks, with only
  greetings and "run / use / now" words around it (`voice/src/skill_named.rs`, `named_among`). Any other word sends
  the line to the model.
  - K's make names its skill ("make a skill called camera look …"), and its ask is "Hey Lamp, camera look.".
  - C's fixture is `lemon-probe`: "grant probe" was twice not matched, while "slow probe" in the same form was.
- **A pushed fixture's run asks the person's yes once.** "do.run: not handed over: waiting for the owner's yes".
  `ASKYES` answers it with YES2. A waiting run spends nothing and comes again only on the next ask, so once
  lamp-control logs "the owner's yes to <skill>'s task kept", the kit lets the lamp finish and asks again.
- **C is VOID, not FAIL,** when the model never ran the fixture: no grant was tested.
- **P, fork past the cap (19:1x run):** the fence's `prlimit --nproc=64` refuses forks before the run leaf's
  `pids.max` 72 (`skill_run.rs`), so `pids.events.max` stays 0 by design, and a run's stderr is never journaled.
  fork-probe tells its refusals by its exit code: 0 when some of its 120 forks were refused, 3 when none were.
  P1 passes on exit 0 or on a cgroup count. Run steps are one at a time across the lamp, and a clip during the run
  speaks over its say, so P2 asks the price only after the fork run ends: the lamp runs the next task after it.
- **H's yes-ask cut by a room sound** ("asked aloud: say: dropped (user_interrupted)") is kept as tried and asked
  again on the next call (`task.rs`, `ask_yes`). `ASKYES` waits for quiet and asks the clip once more.
