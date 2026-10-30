# The lamp's hardware, for a developer

[lamp.hardware.json](lamp.hardware.json) is the lamp's parts as data, shipped
in both SDKs as `hardware` (`lampos.hardware()`, `import { hardware } from
'lampos'`). [lamp.model.json](lamp.model.json) is its kinematic model, as
`model` ([control.md](control.md), "Where the body is"). Every part in the
JSON says where its facts come from: the hardware docs in
`robots/lamp/hardware/` (components.md, wiring.md, power.md) or lampOS's
code. No maker's figure is given that the repository doesn't hold, so a
servo's torque and speed aren't there yet. Where two docs differ, both are
given.

| Part | In short |
|---|---|
| Board | OrangePi 4 Pro (Allwinner A733), 4 or 6 GB |
| Joints | 5 × Feetech STS3215, 4,096 counts a turn, read and written every 20 ms: `yaw`, `shoulder`, `elbow`, `pitch`, `roll` |
| Camera | USB UVC, 78° field of view; MJPG 1080p30 (wiring.md), 1280 × 960 as lampOS uses it. Never readable through lamp-control |
| Microphones | a Jieli dual-mic ENC module (voice, 48 kHz mono) and a C-Media adapter (sensing); lampOS captures at 16 kHz. Never readable through lamp-control |
| Speaker | 2 × 3 W on a PAM8610 amplifier |
| Light | a WS2812B ring of 32, ten named colours, levels 1 to 5, brightness capped in software |
| Touch | an MPR121 strip in the base (12 electrodes) and TTP223 head pads (2 of 4 positions wired) |
| Buttons | user (pin 35), reset (pin 37, held 5 s is a factory reset), privacy slide (pin 11: microphones, camera and speaker off) |
| Air | Sensirion SEN63C, optional and off by default: particles, temperature, humidity, CO2 |
| Power | 12 V 5 A in (60 W, about 42 W sustained), a 5 V 3 A buck for the board, ring and fan |

To be measured on a lamp: each joint's zero and direction against the CAD
(the model's `measured`), and the servos' torque and speed from the maker's
sheet.
