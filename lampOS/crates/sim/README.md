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

`LAMP_SIM_SERVO_RANGES` sets the angle limits the simulated servos store (what `lamp-body` reads with `LAMP_BODY_OWN_RANGES` or `LAMP_BODY_APP_REACH`): unset or `full`, 0..4095 on all five, as before (the body then keeps its own caps); `75c9`, lamp-75c9's read on 10 Nov (yaw 820-3268, roll 897-3153, pitch 1457-3029; shoulder and elbow stay 0..4095, their reach comes from the arm's window, and no other number is invented); or a list such as `yaw=820-3268,pitch=1457-3029` (joints `yaw`, `shoulder`, `elbow`, `roll`, `pitch`). A bad value stops lamp-sim at start.

Camera and microphone are off unless asked for. `LAMP_SIM_CAMERA` and
`LAMP_SIM_MIC` name a device other than the default (`/dev/video0` or camera
`0`; `default` PulseAudio source or `:0` on macOS). `LAMP_SIM_CAMERA=lavfi:SOURCE` gives a picture ffmpeg makes itself (a test pattern, e.g. `lavfi:testsrc2=size=320x180:rate=15`) in place of the camera: a machine with no webcam, or a test that must see the same picture every time (a lavfi source can read local files, so `LAMP_SIM_CAMERA` is the developer's own setting, never taken from a request). A paired program sees the
pictures and sound only as on a real lamp: never while the lamp is private,
never with more than one person in view, only while the ring shows the mark.

`tools/lamp-sim-check.sh BIN_DIR` starts it, pairs the Python SDK's CLI and the
JS SDK, runs the JS SDK's tour unchanged and a Python round trip, ends the lamp
and checks that nothing is left running: the SDKs against the virtual lamp with
no special mode. `tools/lamp-sim-conformance.sh BIN_DIR` does the same with
every SDK example (`sdk/js/conformance/examples.js --sim`, paired with every
grant): each runs as a program of its own and the lamp must show what it is for.
Both run in CI on every pull request that can change what a program sees
(`.github/workflows/lampos-sim.yml`, Linux, release binaries, no camera or
microphone).

## What is real and what stands in

| | |
|---|---|
| Real | `lamp-control` (pairing, grants, leases, rates, refusals, events), `lamp-body` (limits, caps, speeds, gestures, the body lease, `body/joint_states`), the bus |
| `servo` | five simulated STS3215 servos on a pty (`servo.rs`): the packets, registers, a goal followed at the servo's speed; found folded at the body counts' home: lamp-fc4b's measured home, or lamp-75c9's with `LAMP_SIM_ARM=75c9` |
| Ring (`ring.rs`) | light claims by priority, `light/state` and `light/drawn`, the camera and microphone marks drawn and said (`light/mark_drawn`) as the real ring does; the 32 pixels are written to `DIR/ring.json` (whole files, about 25 times a second) for a viewer. A light asked with a pattern moves as lamp-voice's ring draws it (`breathe`, `candle`, `pulse`: shapes, periods and bounds from `ring_face.rs` `moving`; the sim plays every pulse, not one a minute) and `ring.json` then also carries `"pattern": "<name>"` and the pixels as drawn at that moment; the pattern ends with its light. The real ring draws a pattern only while its face rests, and after a pulse it shows its slow breath, where the sim shows a steady light and draws the pattern whatever the lamp is doing. A light with none has no `pattern` key and steady pixels, as before. No quiet hours or current cap |
| Skills manage socket (`manage.rs`) | `DIR/manage.sock`, as systemd's `lamp-skills-manage.socket`: one REAL `lamp-skills asked` per connection (on the lamp folder's own `skills`, `skills-state`, `skills-dev`, `state/push`), so `skill.push`, `skill.list` and `skill.remove` are checked by the lamp's own code. As on no lamp: it runs as the developer, any peer may ask (`LAMP_SKILLS_ANY_ASKER=on`), the Store is off, and a pushed skill is not run by lamp-sim. On only when `lamp-skills` is beside `lamp-sim` |
| Voice (`voice.rs`) | the say socket: `begun`, the host's `say`/`espeak-ng`/`espeak`/`spd-say` (speech-dispatcher, the usual one on a Linux desktop) if there is one (the text on its stdin; a cut-off `spd-say` is also told `--cancel`, as speech-dispatcher goes on speaking a queued line; known limit: that cancel is global, so a stop also cuts any other speech-dispatcher speech of this user, e.g. a screen reader), else a silent wait as long as the line, then `spoken`; a computer's sound clip is played by the host's `paplay`/`aplay`/`afplay`/`ffplay` (an absolute path only; an engine that exits with an error is passed over for the next, and none working is the silent wait); one line at a time (a second `say` waits), and a `stop` cuts the speech off and drops the waiting ones. A computer's own sound for the speaker (`speaker.stream`, off unless the owner switches lamp-control's `--speaker`/`LAMP_CONTROL_SPEAKER` on, and the pairing has the `speaker` grant): lamp-control asks on this socket, the stand-in answers `{"event":"stream","state":"open"}`, reads the blocks lamp-control publishes on `speaker/audio` (24 kHz mono S16LE) while the connection is open, plays them on the host (`paplay`, `aplay` or `ffplay`, the first that starts; `LAMP_SIM_SPEAKER=silent` plays nothing) and says `stats` each second with the `blocks` and `ms` heard; a player that does not keep up is not waited for (blocks its queue has no room for are not given to it, counted and said when the stream ends), and it is ended 2 s after the stream does; the door closing its end ends it, and lamp-sim says on stderr how many blocks it heard. It has no talking state, so the stream is never ended by the lamp speaking or a hand on the head. A computer's word-for-word `say` is also told on `voice/line` as it begins (`by` `computer`, `source` `exact`, `app` its id), as the real voice does, so `lamp.said` reaches a computer holding `speech` (the default pairing gives it, as it gives every grant); the lamp's own replies and cached phrases are not told, as the sim has no model. The volume socket (lamp-actions' four app calls: `volume_level`, `volume_to`, `volume_step` of 20 with a floor of 10, `volume_muted` that remembers the level it muted from) keeps the level and does not apply it, and says `speaker/volume` once at the start and after each change, as lamp-actions does, so `get volume`, the `volume` op and `volume.changed` work through lamp-control; it starts at 50 |
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

## Your own voice keys (for a real voice later)

`lamp-sim voice-keys` says which of a developer's voice keys are set and from where, and never says a key. Nothing uses them yet: this is the reader a real voice will be fed from. The keys come from `~/.config/lampos/voice-keys.toml` (`$XDG_CONFIG_HOME` honoured; `LAMPOS_VOICE_KEYS_FILE` names another file), or from environment variables, which win over the file. An empty value is no key. A missing file is no keys, not an error.

```toml
[main]
gemini = "..."          # a Google (Gemini) API key: LAMPOS_VOICE_GEMINI_KEY
[own]                   # a person's own keys on the voice's second route
deepseek = "..."        # LAMPOS_VOICE_DEEPSEEK_KEY
deepseek_url = "https://..."   # optional, LAMPOS_VOICE_DEEPSEEK_URL
deepgram = "..."        # LAMPOS_VOICE_DEEPGRAM_KEY
```

The file must be a regular file you own, mode 0600 (or 0400), and not a symbolic link, as lamp-voice demands of its own credential files; otherwise `voice-keys` stops with the refusal and the `chmod 600` line that fixes it (exit 2). A name lamp-sim does not know is refused by its line only (a key pasted where a name belongs would otherwise be echoed), and a known key that is not quoted text by its name and line; a value is never shown. A key is never written, logged or put in a lamp's folder, and its type prints only its name, where it came from and its length.

## Known gaps

- No ears or eyes models run: hands, sound classes and phrases are made by hand (above); no name, touch or real face events.
- The body's home, folded-arm and stand counts are lamp-fc4b's unless `LAMP_SIM_ARM=75c9` (a bad value stops lamp-sim): 75c9's are shoulder 2076 / elbow 2045 folded and 2639 / 2570 at the stand (`get body` read on 10 Nov, as in `docs/lamp.model.json` "measured"), head pitch/roll/yaw home 1791 / 2057 / 2000 and at the stand 1780 / 2057 / 2026 (the real lamp-body's own journal reads of lamp-75c9 on 11 Nov: standing 17:35 [1780, 2057, 2026], folded 17:42 [1756-1760, 2057, 2000], folded 18:05 [1785, 2057, 1999]; the folded pitch is the 10 Nov `get body` 1791, 6 counts over the 18:05 read, and 17:42's pitch reads as mid-move). `LAMP_SIM_ARM=75c9` (the body's counts) and `LAMP_SIM_SERVO_RANGES=75c9` (the stored angle limits) are two settings that combine: both together are all of 75c9's known numbers. The camera's warp (`pose.rs`) still takes fc4b's home and stand as its zero: a stood 75c9's head is 11 counts under its home pitch (about 1 degree) and 26 over its home yaw (about 2.3 degrees).
- Windows and macOS: not built or tried (`lamp-body`'s runtime is Linux only).
