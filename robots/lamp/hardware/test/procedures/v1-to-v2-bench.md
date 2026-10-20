# A V1 bench lamp to lampOS V2, then the factory steps

How a hardware builder puts lampOS V2 on a bench lamp that still runs V1
(`hal`, `os-server`) and takes it through `deploy/factory/steps.sh`, so the
routine run (#1849, `cases/test-cases-v0.toml`) can start. Shown on
lamp-dbda on 19 Oct 2026 by `hw-member1` with its person at the lamp;
[lampOS/deploy/lamp-dbda/README.md](../../../../lampOS/deploy/lamp-dbda/README.md)
is the result. It follows lampOS `docs/install.md` and `docs/updates.md`
("by hand"), with what they do not yet say.

**For a bench lamp only.** It leaves a **test** update key on the lamp.
A lamp that ships gets the owner's key and an official channel build.

No password, key or auth key goes in this repository or an issue. The
board's `sudo` password is in the team's password manager.

## 0. Before

- The lamp on a network the builder's machine reaches; its person beside it
  and saying yes before every `ssh` (the repository's CLAUDE.md).
- The builder's SSH key on the lamp. Once, with the board's password, the
  person runs `ssh-copy-id orangepi@<lamp>`.
- Optional, to watch the arm: a camera on the lamp. An agent inside tmux
  cannot reach the Mac's camera (macOS gives it to the app that started
  tmux); a local page in Chrome that posts a frame every 2 s to
  `127.0.0.1` works. Pictures stay on that machine and are deleted after.

## 1. Write down and back up what it runs

```sh
B=~/hw-bench-backup-$(date +%Y%m%d); mkdir -p $B
systemctl list-units --all 'lamp*' 'hal*' 'os-server*' 'led*' > $B/units-before.txt
for u in hal os-server led-boot led-shutdown; do cp "$(systemctl show -p FragmentPath --value $u)" $B/; done
sudo fuser -v /dev/ttyACM0 /dev/video0 > $B/devices-before.txt 2>&1
sudo cp -a /root/config $B/root-config; sudo chown -R orangepi $B; chmod -R go-rwx $B   # holds the voice key
```

Check `/dev/net/tun`: sun60iw2 boards have none, so Tailscale, if wanted,
runs with `FLAGS="--tun=userspace-networking"` in `/etc/default/tailscaled`.

## 2. A channel on the builder's machine

From a lampOS checkout at the commit of a green `lampOS on ARM64` run of
`lamp-voice` (`B` its run id, `N` its run number, `SHA` its commit):

```sh
gh run download B -R autonomous-ai/lampOS -n lampos-arm64-SHA -D ~/lampos-channel
cargo build --release -p lamp-update            # in lampOS/
```

The **test key is made by the person**, not the agent (an agent's
permission system refuses to make a trust root, rightly):

```sh
(umask 077; openssl genpkey -algorithm ed25519 -outform DER | base64 > ~/keys/lampos-update-test.key)
```

Then the agent:

```sh
U=target/release/lamp-update
$U public-key --key-file ~/keys/lampos-update-test.key > ~/keys/lampos-update-test.pub
$U manifest --binaries ~/lampos-channel --deploy deploy --channel lamp-voice \
  --sequence N --commit SHA --files-url file:///var/tmp/lampos-channel/
$U sign --key-file ~/keys/lampos-update-test.key \
  --manifest ~/lampos-channel/manifest.json --out ~/lampos-channel/lampos-manifest.json
rsync -a ~/lampos-channel/ orangepi@<lamp>:/var/tmp/lampos-channel/
scp ~/keys/lampos-update-test.pub orangepi@<lamp>:/var/tmp/key.pub
```

## 3. lamp-update and the key on the lamp (the person)

Placing a trust root is the person's step too. In their own terminal on the
lamp (`H` is the SHA-256 of `lamp-update`):

```sh
sudo install -D -m 0755 /var/tmp/lampos-channel/lamp-update /opt/lampOS/update/$H/lamp-update
sudo mkdir -p /etc/lampOS/update
sudo install -m 0644 /var/tmp/key.pub /etc/lampOS/update/key.pub
rm /var/tmp/key.pub; sudo sync
```

Long lines pasted into an agent's prompt with `!` get broken; type them in
a terminal.

## 4. Install

```sh
sudo /opt/lampOS/update/$H/lamp-update check --channel-url file:///var/tmp/lampos-channel/   # "up to date: build N"
sudo /opt/lampOS/update/$H/lamp-update install --channel-url file:///var/tmp/lampos-channel/
```

It writes the parts, units and settings, enables them, starts nothing, and
copies V1's voice key. On lamp-dbda: "93 changes; nothing started".

## 5. The models

While release `models-2026-10-16` is a draft, `install` says each model
"could not be had". On the builder's machine:

```sh
gh release download models-2026-10-16 -R autonomous-ai/lampOS -D ~/models -p '*.onnx' -p '*.tgz'
```

Check each against `deploy/product/models.json` (SHA-256 and size; the ONNX
Runtime library is the member `onnxruntime-linux-aarch64-1.30.0/lib/libonnxruntime.so.1.30.0`
of its archive), copy them over, and `sudo install -m 0644` each at its
`path`. Check the SHA-256 again on the lamp.

## 6. Stop V1

```sh
sudo systemctl disable --now hal os-server led-boot
sudo systemctl stop led-shutdown       # its unit is in /etc/systemd/system, so it cannot be masked
```

Wi-Fi stays: `wpa_supplicant` runs on its own. The voice refuses to start
while `led-shutdown` is active ("explicit exclusive cutover required").

## 7. The factory steps

`install` places each part's main binary only, so the body's tools come
from the channel folder:

```sh
C=/var/tmp/lampos-channel
sudo MEASURE_BIN=$C/lamp-body-measure MOOD_BIN=$C/lamp-body-mood \
  bash steps.sh --unit-id <lamp> --to 3            # dry: reads, writes nothing
```

Written (`--write`), steps 1, 2 and 4 ask the person at a console and are
refused without one. An agent can relay the person's yes: it runs
`ssh -tt` reading from a FIFO, and writes `y` into the FIFO only after the
person has said ready:

```sh
mkfifo ctl; (sleep 3600 > ctl &)
ssh -tt orangepi@<lamp> "sudo MEASURE_BIN=... MOOD_BIN=... bash /var/tmp/steps.sh --unit-id <lamp> --write --to 3" < ctl > console.log &
echo y > ctl        # only after the person says ready
```

- **Step 1** stands the arm at 30% and 100% (about 34 s each): a hand by
  the arm. lamp-dbda: stand at shoulder 2635, elbow 2602.
- **Step 2**, 30 s: hands off 1 s, then pad 1 twice, pad 2 twice. Anything
  but two lines each touched twice writes nothing (lamp-dbda: 37, 96, 97).
- **Step 3** keeps a key already there.

## 8. Start V2

A lamp set up under V1 has no setup marker, and `lampos.target` waits for it:

```sh
sudo mkdir -p /var/lib/lampos-setup && sudo touch /var/lib/lampos-setup/done && sudo sync
sudo systemctl start lampos.target
sudo lamp-health show            # /opt/lampOS/health/current/lamp-health
```

Known on build 3309, each mended on the lamp with a drop-in (the product's
way for one lamp, `deploy/product/README.md`) until the software side fixes
it (#1722):

| Seen | Drop-in |
|---|---|
| body: `--gaze-socket and --gaze go together, and --gaze-reach needs them` (no `--gaze` before it is measured) | `lamp-body.service.d/no-gaze-yet.conf`: `ExecStart=` then the unit's line without `--gaze-socket` and `--gaze-reach` |
| eyes: `camera did not open: ... stage: Identity` | `lamp-eyes.service.d/camera-port.conf`: the unit's line with `--usb 01da:5875@<this unit's port>`; the port is `basename` of the `/sys/bus/usb/devices/*` whose `idVendor` is `01da` |

## 9. Step 4, the factory self-test

```sh
... bash /var/tmp/steps.sh --unit-id <lamp> --write --from 4 --to 4
```

For every check to be judged, not `not_checked`:

- **speaker:** the lamp has spoken for 4 s at a stretch since the voice
  started. In a busy room it may decline to answer (lamp-dbda, #1722);
  speak to it facing it, in a quiet moment.
- **head_move:** the lamp is standing.
- **touch_by_hand:** within 20 s of the prompt, touch both head pads and
  press the button (the base strip is off on V2). Tell the person before
  sending `y`: an agent's message can reach them after the window closes.

The report is `/var/lib/lampOS/selftest/<started>.json`; it goes to the fleet
record, never this repository.

## 10. Back to V1

```sh
sudo systemctl disable --now lampos.target lampos-setup-done.path
sudo systemctl enable --now hal os-server led-boot
```
