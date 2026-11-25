# lamp-8ffc, heron's lamp, a product lamp on stable

lamp-8ffc has been heron's since 2026-11-25, in place of lamp-fc4b ([LAMP.md](LAMP.md),
[the role file](../../builders/heron.md)). It is a product lamp: its units are the
product's (`deploy/product/`), written by `lamp-update` from the stable channel, and it
updates itself. No builder has installed anything on it by hand.

Everything below was read over ssh on 2026-11-25 between 11:56 and 12:05 +07, with the
person's yes. Nothing was started, stopped, installed or moved.

## What it runs (2026-11-25, 11:56 +07)

- **Build:** stable **15986** (lamp-voice `7dbd1a1444`, the signed `lampos-manifest.json`
  of `lampos-stable`), channel `lamp-voice`. `lamp-update` wrote "build 15986 runs" at
  09:07:54 and "up to date: build 15986" at 09:09:00. The updater went from
  `42d72ccfadbf` to `9f48218399e9` in the same round.
- **The parts:** each `/opt/lampOS/<part>/current` names a SHA-256 directory; among
  them voice `b6b3afc7fe5f`, body `985c5af1a964`, eyes `93c9cb620191`, update
  `9f48218399e9`.
- **Update switches** (`/etc/lampOS/update.env`, dated 23 Nov): `LAMP_APP_ENGINES`,
  `LAMP_SKILL_TASKS`, `LAMP_SKILL_DEFAULT_AGENT`, `LAMP_UPDATE_NOW` and
  `LAMP_UPDATE_CDN_FEED` are `on`.
- **Set up:** `/etc/lampOS/setup/backend.json`, `voice/`, `person/` and `zone.env` are
  from 2026-11-20 12:58, so the lamp was set up with a phone that day.
- **The board:** kernel `5.15.147-sun60iw2`, 5,854 MB of memory (4,087 MB available), the
  disk 58 GB with 50 GB free (14 % used). It had been up 4 days 21 h.
- **No V1 on it:** `systemctl list-units 'lamp*' 'hal*' 'os-server*' 'bootstrap*'
  'autonomous*'` lists only lampOS units; there is no `hal`, `os-server` or
  `bootstrap`. Nothing is there to go back to.

## The body's counts

`/etc/lampOS/body/lamp.env` (20 Nov 12:30) was read at rest with torque off by
`lamp-body-measure` on 2026-11-19T11:45Z:

```text
BODY_POSES="--home 2006,2021,2018 --folded-arm 2042,2045 --stand 1772,2021,2018,2635,2568 --pitch-window 1740,2030 --torque on --gaze 755,555"
```

The servos' own limits are yaw 772-3261, shoulder and elbow 0-4095, roll 800-3278 and
pitch 1375-3054. Gaze is **not measured** for this lamp: `755,555` is lamp-fc4b's.
`lamp.env.before-4d` (19 Nov) is the same file without `--gaze`.

## Not well when read

- **`lamp-body.service` is `failed`.** It exited with status 1 at 2026-11-20
  15:35:35 +07 after 3 restarts, which is its start limit (3 in 600 s). `/dev/ttyACM0` was
  made again at 15:35 that day. The journal no longer holds that boot's lines, so the
  reason is not known. As a result `lamp-selftest` fails `head_roll` and `head_pitch`
  ("the newest body/joint_states is 409043.2 s old"). Nothing was restarted. Starting
  the body moves the lamp, so it waits for the person to be by the lamp.
- **`sudo -n` asks for a password** on this lamp, so `fuser` on `/dev/ttyACM0` and
  `/dev/video0`, and `/run/lampos-exits`, were not read.

## The commands (read-only)

```sh
date; hostname; uptime; uname -r; free -m; df -h /
systemctl list-units --plain 'lamp*' 'hal*' 'os-server*' 'bootstrap*' 'autonomous*'
for p in /opt/lampOS/*/current; do echo "$p -> $(readlink $p)"; done
journalctl -u lamp-update -g 'build|switched|up to date' -n 8
systemctl status lamp-body; systemctl show lamp-body -p NRestarts,ExecMainExitTimestamp
journalctl -u lamp-selftest -n 12
ls -la /etc/lampOS /etc/lampOS/setup /etc/lampOS/body; cat /etc/lampOS/body/lamp.env
```
