# lamp-dbda

The hardware lead's bench lamp, given to `hw-member1` on 19 Oct 2026 for the
routine run (#1849). Moved from V1 to lampOS V2 that day by
[the bench procedure](../../../robots/lamp/hardware/test/procedures/v1-to-v2-bench.md).
Reached on the office network by key (`orangepi@<its address>`); ask its
person before every `ssh`.

## What it runs, and what is this lamp's own

| What | Where | Why |
|---|---|---|
| lampOS build 3309 (`4a39f0412`) | `/opt/lampOS/*/current`, by `lamp-update install` from a local channel | the routine run needs V2 |
| A **test** update key | `/etc/lampOS/update/key.pub` | the channel release `lampos-lamp-voice` does not exist and the owner's key is not on the bench; the lamp refuses official builds until the owner's key replaces it. Not for shipping |
| The seven models | `/opt/lampOS/{voice/current,eyes/models,name/models}` | from release `models-2026-10-16` (a draft, so `install` could not fetch them), checked against `deploy/product/models.json` |
| `lamp.env` | `/etc/lampOS/body/lamp.env` | factory step 1: home 1769,2059,2038; folded arm 2041,2047; stand 1772,2059,2038,2635,2602; no `--gaze` yet |
| Pins | `/etc/lampOS/controls/controls-pins.json`, the product default (37, 96, active-high) | factory step 2 found three lines (37, 96, 97) and wrote nothing |
| `lamp-body.service.d/no-gaze-yet.conf` | drop-in | the product unit passes `--gaze-socket` and `--gaze-reach`, which the body refuses without `--gaze` |
| `lamp-eyes.service.d/camera-port.conf` | drop-in | this unit's camera is on USB port `3-1.3.4`, the product unit says `3-1.1.4` |
| `/var/lib/lampos-setup/done` | written by hand | the lamp was set up under V1 (its Wi-Fi works); `lampos.target` waits for the marker |
| Passwordless `sudo` for `orangepi` | `visudo`, by its person | bench convenience; not for shipping |
| Tailscale (userspace) | `tailscaled` | in the auth key's tailnet, not the bench Mac's |

V1 is disabled, not removed: `hal`, `os-server` and `led-boot` disabled,
`led-shutdown` stopped. Its unit files and `/root/config` are backed up in
`~orangepi/hw-bench-backup-20261019/` (mode 0700: the config holds the
voice key).

## Back to V1

```sh
sudo systemctl disable --now lampos.target lampos-setup-done.path
sudo systemctl enable --now hal os-server led-boot
```
