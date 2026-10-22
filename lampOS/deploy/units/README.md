# Units for every lamp

Templates of the systemd units a lamp runs, and of the target and slices
that make its parts one lamp ([keeper.md](../../docs/keeper.md)). A lamp's
own folder (`deploy/lamp-4ace/` is the worked example) is made from them:

1. Copy the units of the parts this lamp runs, and always `lampos.target`,
   `lampos-voice.slice` and `lampos-parts.slice`.
2. Replace every `@NAME@`. Each names something only that lamp knows:
   `@UNIT_ID@` its host name; `@LAMP_USER@`, `@MEMORY_USER@` and
   `@SURROUND_USER@` the users its parts run as; `@<PART>_SHA256@` the
   SHA-256 of the binary installed under that directory; `@SERVO_DEVICE@`,
   `@CAMERA_DEVICE@`, `@CAMERA_USB@` its devices; `@VOICE_OPTIONS@` its
   measured voice options; `@FAST_CPUS@` its
   fast processors; the rest are paths. Nothing measured on one lamp is
   right for another: the poses and the gaze are measured on each
   ([body.md](../../docs/body.md)). `grep -n @ *.service *.target *.slice`
   finds what is left.
3. Leave out what the lamp does not run. A part that is not there is not
   waited for.
4. Install, then `systemctl enable` each part to be in the lamp, and
   `systemctl enable lampos.target` for the lamp to start at power-up. Copied
   but not enabled, nothing starts.

`lamp-surround.service` is not part of `lampos.target`: it is what asks for
an update, and stopping the lamp must not stop it. The keeper's own unit
comes with `crates/keeper`, and is not part of the target for the same
reason. Nor is `lamp-update.service`, which restarts the parts when it
switches them to a new build ([updates.md](../../docs/updates.md)): its
`@UPDATE_CHANNEL_URL@` is the channel's folder and `@UPDATE_APPLY@` its
switch, `off` for its first minute on a lamp and `on` after.

`lamp-skills.service` runs the physical skills ([skills.md](../../docs/skills.md)):
each skill is a folder copied from `lampOS/skills/` into
`/var/lib/lampOS/skills/`, readable by `@LAMP_USER@`, and a change there
reloads within two seconds. lamp-4ace's folder has it; copied and enabled,
it greets with Welcome back. It speaks through the voice's say service,
which needs `LAMP_SAY=on` and `LAMP_SAY_GROUP=@LAMP_USER@` in the voice's
unit (#954); it is the lamp's only hello, the voice's own removed (#2825). It sets `LAMP_SKILLS_ASK=skip` until the Policy's gate exists
(#939): without it every skill is refused. It reads the lamp's zone from
`/etc/lampOS/zone.env` (`LAMP_UTC_OFFSET_MINUTES`; 420 on lamp-4ace) and
does not start without it: a skill's quiet hours (Stuffy air's night,
#1021) need the local time. The product's unit passes `--zone system`
instead, which keeps those hours across summer time (#1384).

`lamp-body.service` takes its poses as the product's unit does, as
`$BODY_POSES` from files, not from a placeholder: the confirmed ones in
`/etc/lampOS/body/lamp.env` (the factory's, after its watched stand) over the
seated ones in `/var/lib/lampos-body/measured.env`. It requires
`lamp-body-measure.service`, a oneshot for a lamp moved to V2 without the
factory's step (a V1 lamp, #2163). Where neither file is there, it reads the
lamp at rest with `lamp-body-measure --seated` as `@LAMP_USER@`, and writes
`measured.env` once: `--home`, the pitch window and `--torque on`, with no
stand, so the body holds and moves the head and base only. It always runs and
stays active (`RemainAfterExit`), so the keeper does not wait on it. A read it
refuses (torque on, a head outside its window) fails it, so the body does not
start, and the journal says why. It never turns torque off to get a read. A
lamp's own folder made from these templates needs one of the two files
([body.md](../../docs/body.md)).

`lamp-first-stand.service` gives a seated lamp its first stand, confirmed at
the lamp (#2396, [body.md](../../docs/body.md), "The first stand of a moved
lamp"):
- Once someone is near, the lamp asks for a touch through the voice's say
  service. This is why the generic voice unit sets `LAMP_SAY=on`.
  `lamp-setup`'s Wi-Fi, reset and `wifi_lost` lines speak through it too
  (below). The socket is root's only.
- A touch held 1 s is the yes. On a yes, the unit stops `lamp-body`, stands
  the arm as the factory's steps do, writes `/etc/lampOS/body/lamp.env`, and
  starts the body again.
- It runs as root, and runs the body and its read as `@LAMP_USER@`.
- With `lamp.env` there it does nothing. It stays active, whatever became of
  the stand.

`lamp-setup.service` and `lamp-setup-recover.service` give a moved lamp the
product's setup and its recovery (#2937, #3020, [setup.md](../../docs/setup.md),
"A lamp moved from V1"): the same commands, conditions and hardening as the
product's units. Recovery is off unless `LAMP_WIFI_RECOVERY=on`, and is a
moved lamp's only way to a new Wi-Fi. There is no `lamp-setup-reset.service`:
`lamp-controls.service` still runs without `--touch` and the pins carry no
`reset`, since a moved lamp's line 100 may be a head pad, which can't be
told from the button (the first builder on #2975). `lamp-keeper prepare`
writes setup's page, its reset plan and, with the plan, the marker, so the
move itself never opens a hotspot. Setup's other product units (the app's
Wi-Fi, settings and own-key changes after setup, and the marker's path
unit) are not here yet; a test in `crates/update/src/unit.rs` names each
with why.

Since 22 Oct, for lamps moved from V1, `lamp-voice.service` carries the
product unit's "who it answers" switches at the product's values (with
`LAMP_ROOM_FACTS`), and the bus they read the name and the faces from
(`LAMP_BUS`, `LAMP_BUS_AUDIO`); `lamp-eyes.service` publishes the faces there
(`LAMP_BUS=on`). A test in `crates/update/src/unit.rs` keeps the two units the
same, and fails on a switch `docs/attention.md` names that is in neither list. `LAMP_NAMES` is
left out: it needs the eyes' who socket, which the generic eyes unit does not
serve. `busy-near` judges a busy room as `busy` does until the package runs
`lamp-ears`. The second route (`LAMP_VOICE_ROUTE=second`) is left out too,
since nothing in this package writes `/etc/lampOS/voice/second.json`.

`lamp-body.service` is `Type=notify` with `WatchdogSec=5`: the body says
when it is ready and that its loop is alive. The others are `Type=simple`
until each calls the keeper's helper; then each becomes `Type=notify` with
its own `WatchdogSec`.

What the lamp-os package does with each binary is in
[`package-binaries.json`](../package-binaries.json): `core`, `optional`,
`provisioning` or `development`. A pull request that adds a binary adds it to
one list. CI runs `tools/package-binaries.sh` after the release build and fails
on a binary in none or in two, so the next lamp-os build never stops on it.

Binaries with no unit here: `lamp-turn-end` is a worker the voice starts
and watches itself; `lamp-bus` is a tool; `lamp-records` (its check and its
report) has no unit on any lamp yet; `lamp-audio` is a qualification bench;
`lamp-observer`, `lamp-voice-eval` and `lamp-sound-fixtures` run on a
development machine.

Checked with `systemd-analyze verify` (systemd 252, Debian bookworm) with
every `@NAME@` filled in: no complaint but the missing binaries.

## A pin takes its unit's flags

A unit that pins a binary by SHA-256 (`/opt/lampOS/<part>/<sha256>/`, in a
lamp's own folder) must name only flags that binary accepts: one it refuses
stops the part from starting. Before a pin or a unit's `ExecStart` changes,
run the pinned binary's `--help` and grep it for every `--flag` in the
`ExecStart`. A flag added in source waits, left out of the unit, until a
binary built with it is pinned (lamp-4ace's `--sight-from-look`, #2748).
