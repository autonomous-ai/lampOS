# lamp-sim: the virtual desktop lamp

A lamp that runs on a desktop. The real `lamp-control` (the protocol door) and
the real `lamp-body` (the body's limits, gestures and lease) run as they are;
`lamp-sim` stands in for what they touch: the servos, the ring, the voice, the
speaker's volume, the camera, the microphone and the eyes' count of who is in
view. A developer pairs it and drives it with the same SDKs and lamp-mcp as a
real lamp, with no special mode. Design: [simulator-lamp.md](../../docs/simulator-lamp.md).

Linux first (the real `lamp-body` runs only on Linux, and the bus is Unix
sockets). Not a replacement for a lamp check: it moves no real servo and
draws no real ring.

## Quick start

```sh
BIN=target/debug            # or a folder with lamp-sim, lamp-body and lamp-control
$BIN/lamp-sim run ~/.lampsim/b --bin-dir $BIN &       # the lamp, on 127.0.0.1:7731 (--listen to change)
$BIN/lamp-sim code ~/.lampsim/b                       # a code and a pin (every grant; --grants a,b for fewer)
python -m lampos.cli pair CODE --pin PIN --host 127.0.0.1     # the JS SDK and lamp-mcp pair the same way
# then drive it as any lamp: lamp.gesture("nod"), lamp.light(...), lamp.say(...), look/joints under lamp.lease("body")
$BIN/lamp-sim people 1 ~/.lampsim/b                   # someone in view; `hear`, `hand`, `touch`, `air` below
```

`lamp-sim --help` lists every command.

## Run it

```sh
cargo build -p lamp-sim -p lamp-body -p lamp-control -p lamp-skills
target/debug/lamp-sim run ~/.lampsim/b            # the whole lamp, on 127.0.0.1:7731
target/debug/lamp-sim code ~/.lampsim/b           # a pairing code and pin, then pair as for any lamp
```

`DIR` must be private and outside any folder that group or others can write to
(`lamp-body` refuses a lock folder under `/tmp`, or under a home made with umask
0002), and short: Unix socket paths hold 103 bytes. Make the parent yourself with
`mkdir -m 700 ~/.lampsim`; `lamp-sim run` says which folder is the trouble before it
starts, and, if the body still ends at start, prints its reason (the last line of
`DIR/body.log`), not `null`.

| Flag | What |
|---|---|
| `--bin-dir BIN` | where `lamp-body` and `lamp-control` are (default: beside `lamp-sim`) |
| `--listen ADDR` | the door's address (default `127.0.0.1:7731`) |
| `lamp-sim code DIR [--grants a,b]` | a pairing code; the grants default to every one (body, light, speak, look, hear, sound, skills, air, device, camera, microphone, notes), so say `--grants` for fewer |
| `--camera=always` | the host camera (ffmpeg) is open for the lamp's whole life, as the eyes' is; the host camera light stays on. `--camera` opens it only while a computer's camera mark is on the ring (a mark seen in the last 1.5 s counts) and keeps it open 10 s after that, so it closes about 10 to 11.5 s after the mark is let go (lamp-sim passes lamp-control's `--camera-opens-on-demand`, docs/control.md "The camera"): the host camera light means someone is looking; snapshots and streams wait up to 3 s for the first frame |
| `--microphone` | the host microphone (ffmpeg) feeds `mic1/audio`; it stays open for the lamp's life, so a macOS microphone indicator stays on; a computer still hears it only through `microphone.stream` and the ring's mark |

Camera and microphone are off unless asked for. `LAMP_SIM_CAMERA` and
`LAMP_SIM_MIC` name a device other than the default (`/dev/video0` or camera
`0`; `default` PulseAudio source or `:0` on macOS). A paired program sees the
pictures and sound only as on a real lamp: never while the lamp is private,
never with more than one person in view, only while the ring shows the mark.

`tools/lamp-sim-check.sh BIN_DIR` starts it, pairs the Python SDK's CLI and the
JS SDK, runs the JS SDK's tour unchanged and a Python round trip, ends the lamp
and checks that nothing is left running: the SDKs against the virtual lamp with
no special mode.

## What is real and what stands in

| | |
|---|---|
| Real | `lamp-control` (pairing, grants, leases, rates, refusals, events), `lamp-body` (limits, caps, speeds, gestures, the body lease, `body/joint_states`), the bus |
| `servo` | five simulated STS3215 servos on a pty (`servo.rs`): the packets, registers, a goal followed at the servo's speed; found folded at lamp-fc4b's measured home (a file for another lamp is to come) |
| Ring (`ring.rs`) | light claims by priority, `light/state` and `light/drawn`, the camera and microphone marks drawn and said (`light/mark_drawn`) as the real ring does; the 32 pixels are written to `DIR/ring.json` (whole files) for a viewer. No quiet hours or current cap |
| Skills manage socket (`manage.rs`) | `DIR/manage.sock`, as systemd's `lamp-skills-manage.socket`: one REAL `lamp-skills asked` per connection (on the lamp folder's own `skills`, `skills-state`, `skills-dev`, `state/push`), so `skill.push`, `skill.list` and `skill.remove` are checked by the lamp's own code. As on no lamp: it runs as the developer, any peer may ask (`LAMP_SKILLS_ANY_ASKER=on`), the Store is off, and a pushed skill is not run by lamp-sim. On only when `lamp-skills` is beside `lamp-sim` |
| Voice (`voice.rs`) | the say socket: `begun`, the host's `say`/`espeak-ng`/`espeak` if there is one (the text on its stdin), else a silent wait as long as the line, then `spoken`; a computer's sound clip is played by the host's `paplay`/`aplay`/`afplay`/`ffplay`; one line at a time (a second `say` waits), and a `stop` cuts the speech off and drops the waiting ones. The volume socket stores the level and does not apply it |
| People (`people.rs`) | `humans/faces/tracked` from the number in `DIR/people` (one if absent), no face model |

## Keeping it up

`lamp-body` is the real one and stops, as it should, when the servo bus misses
a reply. The simulated servo answers the moment the body writes (it waits on
the port, not a timer), and if the body ends all the same, `lamp-sim` says why
on its stderr (the exit report's `stop`, also in `DIR/body.log`) and starts it
again, on a fresh port under the same link (a body's exclusive open stays on a
pty). A sixth end within ten minutes, or `lamp-control` ending, end the lamp. A body restart is not invisible: a computer's body lease is lost (`lease_lost`) and a gesture in flight is dropped, so a program that sees that on a busy laptop knows why. Every exit report is kept in `DIR/body.log`, one after another.

## A person in front of the lamp, by hand

The examples that react (`air`, `phrase`, `follow-hand`, a person arriving) need
someone to do something. No model runs, so each is a command that writes a file
in the lamp's folder, which the running lamp takes within 200 ms:

```sh
lamp-sim people 0 DIR          # nobody in view; then `people 1` for person.arrived (`people 1` gives person.arrived only after person.left, which needs 5 s of nobody; arrival wants 3 s since the last face, never less: watch.rs `ABSENT_FOR_US`, `LEFT_AFTER_US`)
lamp-sim hear "good night lamp" DIR   # voice/heard, finished: a registered phrase fires
lamp-sim sound door DIR        # a door is heard for 1.5 s (sound.heard; music, laughter, door or footsteps)
lamp-sim touch tap DIR         # a tap on the head (touch kind tap); `touch pet` is a pet
lamp-sim hand DIR              # a hand waves across the picture for 1.5 s (humans/hands/tracked)
lamp-sim air pm2_5=41 co2_ppm=1350 DIR   # an air reading, said every 5 s; feel follows the carbon dioxide
```

Each lamp folder has its own lamp name, `virtual-` and six hex digits of the folder's path, so two virtual lamps are two lamps to a computer's key store. A second `lamp-sim run` on the same address says so and names `--listen`.

## Known gaps

- No ears or eyes models run: hands, sound classes and phrases are made by hand (above); no name, touch or real face events.
- The body's home counts are lamp-fc4b's, fixed in `main.rs`.
- Windows and macOS: not built or tried (`lamp-body`'s runtime is Linux only).
