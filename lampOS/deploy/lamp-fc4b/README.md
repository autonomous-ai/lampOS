# lamp-fc4b, heron's lamp, a product lamp on stable

lamp-fc4b is heron's ([LAMP.md](LAMP.md), [the role file](../../builders/heron.md)).
It is a product lamp: its units are the product's (`deploy/product/`),
written by `lamp-update` from the stable channel, and it updates itself. No
builder has installed anything on it by hand. This folder holds only what
install did not write, each file as it is on the lamp, and this record of
what the lamp ran when heron first read it.

Everything below was read over ssh on 2026-11-02 at 18:20 +07, with the
person's yes, and nothing was started, stopped, installed or moved. The
commands are at the end.

## What it runs (2026-11-02, 18:20 +07)

- **Build:** stable **9528** (lamp-voice `ba17e5438`), from
  `https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/`,
  channel `lamp-voice`, `apply=on`, `self=on`. `lamp-update` installed the
  new parts beside the old ones at 16:27, switched at 16:27:51 and wrote
  "switched to build 9528; every part is well" at 16:29:03. Before it the
  lamp ran 9497 ("up to date: build 9497", 15:06). The updater itself went
  from `2d0e63c02fd9` to `b68d8c343505` at 16:22, as 9528 needs updater 10.
- **Set up today:** `lampos.target` has been active since 14:59 +07, when
  `lampos-setup-done.path` fired; `/etc/lampOS/setup/backend.json`,
  `voice/credential` and `zone.env` are from 14:38 and `display-name` from
  14:59, so the lamp was set up (or set up again) with a phone this
  afternoon. The lamp had been up 3 h 58 min at 18:20. Zone
  `Asia/Ho_Chi_Minh` (+420 min).
- **The board:** kernel `5.15.147-sun60iw2`, 5,854 MB of memory (4,750 MB
  free), the disk 58 GB with 54 GB free (6 % used).
- **No V1 on it:** `systemctl list-units 'lamp*' 'hal*'` shows
  `hal.service` as `not-found`, and there is no `os-server`, `bootstrap` or
  `autonomous*` unit. There is nothing to go back to and nothing to back up
  before lampOS: this lamp was imaged with lampOS.
- **The one program on each bus:** `lamp-body.service` runs (the servos)
  and `lamp-eyes.service` with `lamp-eyes-camera.socket` (the camera).
  `fuser` on `/dev/ttyACM0` and `/dev/video0` was not run: it needs `sudo`,
  and `sudo -n` asks for a password on this lamp (below).

The parts under `/opt/lampOS/<part>/<sha256>/`, each `current` link as
read; the voice, body, cloud, ears, expression, eyes, name, setup, skills
and update folders keep one older build beside the current one:

| Part | `current` | Unit at 18:20 |
|---|---|---|
| actions | `ddf48372918e` | running |
| addons | `3f9cd047141a` | inactive |
| air | `427c8010b082` | running (`lamp-air-clock` exited, as its oneshot does) |
| body | `2d131ec06b48` | running |
| cloud | `438482a034c0` | running |
| controls | `34e941df30a4` | running |
| ears | `a4d287eb4b83` | running |
| expression | `75364a7c03d0` | running |
| eyes | `6d716e2ab46a` | running |
| health | `db304b49a95e` | running |
| memory | `9d607abd41e7` | running |
| mood | `ed6727b33fac` | inactive |
| name | `6a288db4e86c` | running |
| policy | `5c1b8cd0136c` | running |
| records | `677ba7e22936` | inactive; `lamp-records.timer` waiting (hourly) |
| schedule | `871b72fa1639` | running |
| selftest | `ecf9fc55f51d` | inactive; `lamp-selftest.timer` waiting |
| setup | `0279bf4771f7` | `lamp-setup` inactive (set up); `lamp-setup-reset` running; `lamp-setup-change.socket` and `lamp-setup-wifi.socket` listening |
| skills | `92ec2c241f8e` | running; `lamp-skills-manage.socket` listening |
| tasks | `2d77821ab8a4` | inactive |
| update | `b68d8c343505` | running; `lamp-update-back.timer` waiting |
| voice | `d3b4271d80e2` | running |

Also seen: `lamp-waking` inactive (the voice has the ring), `lamp-cmedia-speaker`,
`lamp-setup-recover` and `lamp-setup-wifi-restore` inactive; the slices
`lampos.slice`, `lampos-parts.slice` and `lampos-voice.slice` active. 42
units loaded in all. The product `lamp-voice.service` carries 143
`Environment=` lines, `lamp-body` 5, `lamp-eyes` 3, `lamp-update` 2.

## What this folder holds

| File | On the lamp | Why it is here |
|---|---|---|
| `lamp.env` | `/etc/lampOS/body/lamp.env` (2026-10-30 17:05 +07) | This lamp's poses, read at rest with torque off by `lamp-body-measure` on 2026-10-30; install never writes it. Two earlier versions sit beside it on the lamp (`lamp.env.before-4d`, `lamp.env.before-recalib-1793354573`) and are not copied. Its `--gaze` is the default, not measured, as its own comment says. |

Not copied, by design: `/etc/lampOS/setup/backend.json`,
`/etc/lampOS/voice/credential`, `provider.json` and `second.json` (0600,
the lamp's login and voice service), `/etc/lampOS/update/key.pub`,
`records.json`, `zone.env` and `setup/{serve,reports,reset-plan}.json`,
all of them written by install or setup.

## How it was read

One ssh session as `orangepi`, by key, `BatchMode=yes`, no password typed:

```sh
hostname; uname -r; uptime; date
systemctl list-units 'lamp*' 'hal*' --all --no-pager --plain
systemctl list-units 'os-server*' 'bootstrap*' 'autonomous*' --all
ls -la /opt/lampOS; readlink /opt/lampOS/*/current
journalctl -u lamp-update --since "8 hours ago" --no-pager | grep -iE 'install|build [0-9]|switch'
systemctl show lamp-voice lamp-body lamp-eyes lamp-update -p FragmentPath -p ExecStart
ls -la /etc/lampOS /etc/lampOS/*; cat /etc/lampOS/body/lamp.env /etc/lampOS/zone.env
df -h /; free -m; systemctl status lampos.target
```

Build 9528's commit comes from this repository's tag
`lampos-lamp-voice-9528` (`ba17e5438`), not from the lamp.

## What was not shown

- `sudo -n` needs a password for `orangepi` on this lamp, so nothing that
  needs root was read: `fuser` on the servo port and the camera,
  `/var/lib/lampos-update/state.json` (the updater's `installed.commit`),
  `/var/lib/lampos-setup/`, and `tools/switch-drift.sh` (it reads the
  voice's environment through `sudo -n`). The build number is from
  `lamp-update`'s journal instead.
- The lamp was not heard, seen or touched: no played line, no servo read,
  no picture. Nothing says here how this lamp behaves, only what it runs.
- `lamp-health`'s own line ("The lamp is well" or not) was not in the last
  40 lines of its journal at 18:20; the updater's "every part is well" at
  16:29 is the newest word on the parts' health.
- The lamp is heron's alone, so `lamp.lock` is not used on it; none exists.
