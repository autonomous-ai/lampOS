# lamp-06c1, set up the way a home is

lamp-06c1, shared by el and jarvis ([LAMP.md](LAMP.md)), brought
from V1 to lampOS by el on 2026-10-16 with #811's one install, then setup
with a phone (#806), as the first builder asked on #705 at 11:52. Its units
are the product's (`deploy/product/`), written by `lamp-update install`; this
folder holds only what install did not write, each file as it is on the lamp.

| File | On the lamp | Why it is here |
|---|---|---|
| `lamp.env` | `/etc/lampOS/body/lamp.env` | This lamp's poses; install never writes it |
| `serve.json` | `/etc/lampOS/setup/serve.json` | Setup's own page (`crates/setup/fixtures/serve-own-page.json`) |
| `reset-plan.json` | `/etc/lampOS/setup/reset-plan.json` | A copy of `crates/setup/fixtures/reset-plan-product.json`, kept equal by hand: copy it again whenever the product plan gains a path (by 17 Oct it was nine behind, #1401's backend link and the Harness sign-in among them). A product install writes the product plan itself. |
| `provider.json` | `/etc/lampOS/voice/provider.json` (0600) | The voice's service; no key in it |

## How it was done

1. **V1 off, kept:** `hal`, `os-server`, `hermes-gateway`, `bootstrap` and
   `led-boot` disabled and stopped; their units, `/etc/systemd/system`, udev
   rules, `/root/config`, the Wi-Fi file and nginx's site copied to
   `~/el-backup-20261016/` on the lamp. Back to V1:
   `sudo systemctl enable --now hal os-server hermes-gateway bootstrap led-boot`
   (and remove `/var/lib/lampos-setup/done` so lampOS stays down).
2. **Install:** a channel made on the Mac from ARM64 run 38851087915
   (`lamp-voice` `6d2bea2e1`, build 1009) and signed with a throwaway test key
   (updates.md, "On lamp-4ace", steps 3 and 4), then
   `lamp-update install --channel-url file:///var/tmp/lampos-channel/`:
   64 changes, nothing started.
3. **Models**, from their published copies, each equal in size and SHA-256 to
   the pinned one: the turn-end model and ONNX Runtime 1.30.0 beside the
   voice; YuNet and SFace in `/opt/lampOS/eyes/models/`; openWakeWord's
   `melspectrogram.onnx` and `embedding_model.onnx` in
   `/opt/lampOS/name/models/`. `lamp-turn-end --check-model --fast`: warm
   median 126 ms on cores 6 and 7, exit 0.
4. **Body poses,** read with `lamp-body-inspect` (read only, torque off on all
   five): at rest yaw 2034, shoulder 2050, elbow 2059, roll 2057, pitch 1772.
   `--gaze` and `--pitch-window` are lamp-52e6's, not measured here.
5. **The voice** goes the way V1's does: the campaign-api proxy's Gemini
   Live path with the lamp's device key in `x-goog-api-key`, resumption off
   (V1's `hal/config.py`: the proxy does not forward it). A websocket
   handshake from the lamp: 101 with the key, 401 without.
6. **Setup** with the phone, the page opened with `?llm_api_key=` from V1's
   config.

## What needed an engineer (shipping gaps)

- No build carries a model; `name.onnx` (the name's own) is published nowhere,
  so `lamp-name` cannot start.
- The body's poses by hand, and `--torque on` for servos found at torque 0,
  which the product unit lacks.
- Setup's two files and `provider.json` by hand (#1016, #1018 under way).
- Where a home lamp's voice key comes from: here, V1's device key in V1's
  config, put into the setup link by hand.
- V1's `bootstrap` updater, beside the units install.md names, to disable.
- The image has no `gpio` group (the chips are open to all here), no Harness,
  and nginx enabled where install.md expects it disabled.
- Found when the lamp first started, all fixed by hand and in nine's #1018 since:
  `/run/lampos-locks` and its `motor` and `camera` folders made 0775, which the body
  and the eyes refuse (the eyes did not open the camera until 17:12); no
  `/var/lib/lampOS/voice/sessions`, so the voice would not start; with `LAMP_SAY=on`
  the say socket root's alone (`LAMP_SAY_GROUP=lampos` for `lamp-skills`).

## Setup with the phone, 2026-10-16

From `lamp-setup`'s journal: the hotspot up 26 s after the reboot; the page's
first scans failed ("Could not look for networks") until "Look again" (#1045
fixes it); Setup to `Connected` and the marker in 14 s (12:40:12 to 12:40:26);
back on the office network at 12:40:48. The voice used V1's proxy route with
the lamp's device key, given to the page in the link.

## Not shown

A power cut; the gaze and pitch window measured on this lamp (lamp-52e6's are
used); setup on a second phone or a wrong password; a reset by the button.
