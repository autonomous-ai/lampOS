# The product units

The units every lampOS lamp runs, the same on every lamp
([install.md](../../docs/install.md), task #811). Unlike
[`deploy/units/`](../units/README.md) they have no `@NAME@` to fill: each
part's binary is `/opt/lampOS/<part>/current/lamp-<part>`, `current` being
a link to the build's folder, so an update needs no edit of a unit.

What is one lamp's own is outside them:

- `/etc/lampOS/body/lamp.env`: `BODY_POSES="--gaze .. --stand ..
  --folded-arm .. --home .."`, measured on that lamp ([body.md](../../docs/body.md)).
  Required: the body does not start without it.
- `/etc/lampOS/zone.env`: `LAMP_ZONE=` (schedule) and `LAMP_UTC_OFFSET_MINUTES=`,
  from the system's zone, written again by setup with the person's zone
  (#820). Memory, Expression and skills follow `/etc/localtime` instead.
- A switch a lamp must have off: a drop-in,
  `/etc/systemd/system/lamp-<part>.service.d/<why>.conf`.

| File | What |
|---|---|
| `lampos.target` | The lamp. Starts only once setup's marker `/var/lib/lampos-setup/done` is there (#806) |
| `lampos-voice.slice`, `lampos-parts.slice` | As in `deploy/units/` |
| `lampos.conf` | `/etc/tmpfiles.d/lampos.conf`: the bus root and the servo and camera locks |
| `settings/` | What no build carries and every lamp shares: Expression's catalogue (from lamp-4ace's `expression-alive-name-speech`) and the controls' two files, installed in `/etc/lampOS/<part>/` |
| `lamp-<part>.service` | Each part, `PartOf=` and `WantedBy=lampos.target`, in keeper.md's order |
| `lamp-update.service` | Outside the target; `--apply on`; the channel on GitHub |
| `lamp-records.service`, `lamp-records.timer` | Hourly, outside the target |
| `lamp-selftest.service`, `lamp-selftest.timer` | The passive self-test (#812): the timer is in the target and waits for setup's marker; it runs the service 5 min after the lamp starts and every 6 h. The service has no `[Install]` |
| `lamp-setup.service` | First boot only: while the marker is missing and setup's settings exist (#806, #811) |
| `lampos-setup-done.path` | Starts `lampos.target` once the marker exists ([install.md](../../docs/install.md), "Setup at first boot, without the keeper") |
| `lamp-setup-reset.service` | In the target: the reset button held 5 s |

`lamp-update install` (docs/install.md) carries all of these inside it and puts them on a lamp.

Not here yet: the keeper's unit (asked of the fifth on #811), and
`lamp-surround`, which no lamp runs.

Checked with `systemd-analyze verify` (systemd 252, Debian bookworm, arm64,
in Docker on the development Mac), with an empty executable at each
`current/` path and the two users made: no complaint. The same check finds
an unknown key and a missing binary in a unit made wrong on purpose.
