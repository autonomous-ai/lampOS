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
   `@CAMERA_DEVICE@`, `@CAMERA_USB@` its devices; `@VOICE_OPTIONS@` and
   `@BODY_POSES@` its measured voice options and poses; `@FAST_CPUS@` its
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
unit (#954), and the voice's own hello (`LAMP_SPEAK_FIRST`) off, so only one
greets. It sets `LAMP_SKILLS_ASK=skip` until the Policy's gate exists
(#939): without it every skill is refused. It reads the lamp's zone from
`/etc/lampOS/zone.env` (`LAMP_UTC_OFFSET_MINUTES`; 420 on lamp-4ace) and
does not start without it: a skill's quiet hours (Stuffy air's night,
#1021) need the local time. The product's unit passes `--zone system`
instead, which keeps those hours across summer time (#1384).

`lamp-body.service` is `Type=notify` with `WatchdogSec=5`: the body says
when it is ready and that its loop is alive. The others are `Type=simple`
until each calls the keeper's helper; then each becomes `Type=notify` with
its own `WatchdogSec`.

Binaries with no unit here: `lamp-turn-end` is a worker the voice starts
and watches itself; `lamp-bus` is a tool; `lamp-records` (its check and its
report) has no unit on any lamp yet; `lamp-audio` is a qualification bench;
`lamp-observer`, `lamp-voice-eval` and `lamp-sound-fixtures` run on a
development machine.

Checked with `systemd-analyze verify` (systemd 252, Debian bookworm) with
every `@NAME@` filled in: no complaint but the missing binaries.
