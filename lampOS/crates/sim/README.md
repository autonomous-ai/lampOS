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
| `--controls` | starts the real `lamp-controls` beside the lamp (it must be in `--bin-dir` with the others: `cargo build --release -p lamp-controls`): it publishes `controls/state` on the lamp's bus, as a real voice's privacy route waits for, on this boot's clock. It runs as on a lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`, no pins file): it reads no pin, reports its input as missing and says `no_privacy_slide` on every state (about 50 a second); no button, touch or slide is simulated. Off unless given; it stops with the lamp, and the lamp ends if it ends |
| `--real-voice` | starts the real `lamp-voice` on its LIVE route with your own Gemini key (see "Your own voice keys"); `--real-voice=second` starts it on its SECOND route (the one a customer lamp runs) with your own DeepSeek, Deepgram and ElevenLabs-or-OpenAI keys; needs `--microphone` (the voice hears the host's microphone on the lamp's bus), `lamp-voice` in `--bin-dir` (`cargo build --release -p lamp-voice`), and a Linux with systemd (`/usr/bin/systemctl` must exist: lamp-voice's start asks it whether the legacy services are idle and refuses without it, so not a container or NixOS) and ALSA. lamp-voice starts its workers with an empty environment, so its speaker worker has no `XDG_RUNTIME_DIR`: on a desktop with PipeWire or PulseAudio the `default` ALSA device may not find the sound server and the voice is silent or its speaker faults. `LAMP_SIM_VOICE_SPEAKER=hw:CARD,DEV` (or `plughw:CARD,DEV`) names a device that needs no sound server; the default is unchanged. **`--real-voice` is the voice's Live route, not the route a customer lamp runs (every product lamp answers through its second route); `--real-voice=second` is that route.** It turns `--controls` on. With no key: one line (`no voice key: set main.gemini in ... (or LAMPOS_VOICE_GEMINI_KEY); using the stand-in voice`) and the stand-in as always. lamp-voice takes its credential only from a file, so lamp-sim keeps ONE private copy of the key for the run, in `$XDG_RUNTIME_DIR/lamp-sim-voice-<lamp>-<hash>/gemini.key` (memory-backed storage; folder 0700, file 0600) beside the `provider.json` that names it, and removes it when the voice or lamp-sim ends (a stale one from a killed run goes at the next start); without a private `XDG_RUNTIME_DIR` the copy is on disk in `DIR/voice/keep` until lamp-sim ends, and lamp-sim says so in one line. The key is never in an argument, the environment lamp-voice is given or any output; lamp-voice gets only a short list of lamp-sim's variables (the login's basics, the locale, the sound-session names, and the proxy and CA names `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` in both cases and `SSL_CERT_FILE`, `SSL_CERT_DIR`; a proxy URL may carry a password, so it goes to lamp-voice only and is never printed) and none of the others. A killed lamp-sim: its watcher removes the key's copy within about a second; the copy stays only if the watcher is killed too (then until the next start of the same lamp, or, in memory-backed storage, until you log out). The model defaults to the product's name (`gemini-3.8-live`), which may differ on Google's own API: `LAMPOS_VOICE_GEMINI_MODEL` names another, and the first real run settles it. With the real voice, a person's talk is answered by it, but the SDK's `lamp.say` and the volume ops still go to lamp-sim's stand-ins (lamp-control's `say` and `volume` ops need a socket, and the real voice's own say socket is `LAMP_SAY`, which a desktop leaves unset), so `lamp.say` is spoken by the host's speech program, not by the real voice. The session folder `DIR/voice/session-N` holds the voice's own events (transcripts of what was said) until the next `--real-voice` start removes it, or the lamp's folder does. If the voice ends, lamp-sim says so and the lamp stays up. Not yet run with a real key; wear headphones for a first run (the speakers feed the microphone) |

`LAMP_SIM_SERVO_RANGES` sets the angle limits the simulated servos store (what `lamp-body` reads with `LAMP_BODY_OWN_RANGES` or `LAMP_BODY_APP_REACH`): unset or `full`, 0..4095 on all five, as before (the body then keeps its own caps); `75c9`, lamp-75c9's read on 10 Nov (yaw 820-3268, roll 897-3153, pitch 1457-3029; shoulder and elbow stay 0..4095, their reach comes from the arm's window, and no other number is invented); or a list such as `yaw=820-3268,pitch=1457-3029` (joints `yaw`, `shoulder`, `elbow`, `roll`, `pitch`). A bad value stops lamp-sim at start.

Camera and microphone are off unless asked for. `LAMP_SIM_CAMERA` and
`LAMP_SIM_MIC` name a device other than the default (`/dev/video0` or camera
`0`; `default` PulseAudio source or `:0` on macOS). `LAMP_SIM_MIC=lavfi:SOURCE` gives a sound ffmpeg makes itself (e.g. `lavfi:sine=frequency=440:sample_rate=16000`), read at its own pace (`-re`), in place of the microphone; measured on macOS 26 at 50.6 blocks a second, none missed (`lamp-bus hz mic1/audio`). `LAMP_SIM_CAMERA=lavfi:SOURCE` gives a picture ffmpeg makes itself (a test pattern, e.g. `lavfi:testsrc2=size=320x180:rate=15`) in place of the camera: a machine with no webcam, or a test that must see the same picture every time (a lavfi source can read local files, so `LAMP_SIM_CAMERA` is the developer's own setting, never taken from a request). A paired program sees the
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

## Your own voice keys

`lamp-sim voice-keys` says which of a developer's voice keys are set and from where, and never says a key. `lamp-sim run DIR --real-voice` reads the Gemini key from the same place, and `--real-voice=second` reads `own.deepseek` (and `own.deepseek_url`), `own.deepgram` and `own.elevenlabs` or `own.openai` (the one that speaks; ElevenLabs when both are set). The second route talks to DeepSeek, Deepgram and ElevenLabs/OpenAI at their own addresses, never the company's relay, and ALL THREE are billed to you (every spoken turn: text, listening and speech); when both speech keys are set, ElevenLabs speaks; `own.deepseek_url` must be https://. Wear headphones for a first run (the speakers feed the microphone); lamp-sim never names the product's `second.json`. The keys come from `~/.config/lampos/voice-keys.toml` (`$XDG_CONFIG_HOME` honoured; `LAMPOS_VOICE_KEYS_FILE` names another file), or from environment variables, which win over the file. An empty value is no key: an empty environment variable is as if it were not set, so the file's value is used; an empty value in the file is no key either. A missing file is no keys, not an error. A key given by a variable stays in lamp-sim's own environment and reaches no process lamp-sim starts (every one is made with `keys::command`, which drops every `LAMPOS_VOICE_*` name, and a test refuses a bare `Command::new`); the voice is given only a short list of names (`PATH`, `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_*`, `TZ`, `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`, `PULSE_*`, `PIPEWIRE_*`, `ALSA_*`) and what lamp-sim sets for it, none of your other variables; if a real lamp-voice needs another name, the first real run will say so.

```toml
[main]
gemini = "..."          # a Google (Gemini) API key: LAMPOS_VOICE_GEMINI_KEY
[own]                   # a person's own keys on the voice's second route
deepseek = "..."        # LAMPOS_VOICE_DEEPSEEK_KEY
deepseek_url = "https://..."   # optional, LAMPOS_VOICE_DEEPSEEK_URL
deepgram = "..."        # LAMPOS_VOICE_DEEPGRAM_KEY
```

The file must be a regular file you own, mode 0600 (or 0400), and not a symbolic link, as lamp-voice demands of its own credential files; otherwise `voice-keys` stops with the refusal and the `chmod 600` line that fixes it (exit 2). A name lamp-sim does not know is refused by its line only (a key pasted where a name belongs would otherwise be echoed), and a known key that is not quoted text by its name and line; a value is never shown. A key is never written, logged or put in a lamp's folder, and its type prints only its name, where it came from and its length.

## A local speech service (`lamp-sim speech`)

```sh
lamp-sim speech ~/.lampsim/b [--listen 127.0.0.1:7732] [--engine resident|say] [--language en-US]   # the token is written to ~/.lampsim/b/speech.token (0600)
```

A text-to-speech service in the host's own voice for the real voice's SECOND
route (sim-3-P4 step 1, `speech.rs`): it answers the OpenAI-compatible
`POST /audio/speech` (or `/v1/audio/speech`) the route already calls, with
`response_format` `pcm` only (24 kHz mono S16LE, chunked); `model` and `voice`
are read past. It listens on loopback only (another address is refused at
start) and speaks only for a request whose `Authorization: Bearer` is the run's
token, at most 4 lines at once (more are answered 503), a line at most 4,096
characters. Each line says on stderr its characters, when its first bytes went
and how much sound it made.

On macOS the engine is the **resident synthesizer** (`apple_speech.rs`, step
1b): Apple's `AVSpeechSynthesizer`, kept for the service's life and holding
the main thread (its callbacks come only with the main run loop running; the
HTTP side runs on another thread), writing each line to buffers made into
24 kHz S16LE as they come (from its 22,050 Hz, linear). Measured on macOS 26
(arm64) through this service, 5 requests each a second apart, medians, first
bytes after 19 ms for "Hi there.", 13 ms for 8 words and 18 ms for 20 (the
whole line after 29, 32 and 61 ms); the first line after start about 290 ms;
whisper.cpp (`base.en`) wrote back a 15-word line's output word for word. A
line ends at the synthesizer's empty end buffer, or 300 ms after its last
buffer if that never comes (it did not on macOS 14, Heron's read on #6961);
at rest the main thread waits on the channel and runs no run loop. It is
the only `unsafe` in lamp-sim: one module, `#[allow(unsafe_code)]` on it alone,
a SAFETY note on each of its 11 blocks (the first builder's conditions, #6811 21:28); its
`objc2` packages are macOS-only dependencies (MIT, or Zlib/Apache/MIT).

`--engine say` (and any system other than macOS) uses macOS's `say` instead,
one process a line, rendered whole before the first byte; each `say` is started before its line comes (it waits on its
stdin), so a line pays only its rendering, not the ~0.4 s start. Measured on
macOS 26 (arm64) through this service, 5 requests each a second apart,
medians, first bytes after 371 ms for "Hi there.", 517 ms for 8 words and
828 ms for 20 (715, 887 and 1,298 ms with a `say` started with the line); a
second line at the same moment starts its own and takes about 0.9 s. A killed
run's leftover files are cleared at the next start. Off macOS there is no engine yet: a request is answered 503 with the reason (`say` is not found).

The real lamp-voice cannot use it yet: its service URLs must be https and it
trusts only its built-in roots. A loopback switch for it is ten's step 3, and
`lamp-sim run --real-voice=local` (step 4) will start this service with it.

## A local speech-to-text socket (`lamp-sim listen`)

```sh
whisper-server -m ggml-base.en.bin --host 127.0.0.1 --port 7734    # whisper.cpp, your own install and model
lamp-sim listen ~/.lampsim/b [--listen 127.0.0.1:7733] [--whisper http://127.0.0.1:7734]   # token in ~/.lampsim/b/listen.token (0600)
```

The listening half of the real voice's SECOND route on a desktop (sim-3-P4
step 2, `listen.rs`): a WebSocket in the part of Deepgram's streaming protocol
the route uses. It serves `/ws/audio/transcriptions` (the route's path when
`stt_base_url` is set) and `/v1/listen`, takes only linear16, 16 kHz, mono
(a socket asking for another is refused 400), and opens only for this run's
token as `Authorization: Token` (or `Bearer`). In: binary audio, `Finalize`,
`CloseStream`, `KeepAlive`. Out: `SpeechStarted` when a 20 ms frame rises over
the speech level after 0.3 s under it, and at each `Finalize` (and at
`CloseStream`, for what is held) one final `Results` (`is_final`,
`speech_final` and `from_finalize` all true, with its `start` and `duration`)
holding the words of the line. The route ends the line itself and waits 1 s
for that answer. No interim results. A line holds at most 60 s of audio (older
is let go); at most 4 sockets at once.

At `Finalize` the line goes, as a 16 kHz WAV, to the developer's own
whisper.cpp `whisper-server` (`/inference`), at a loopback address only; a
failed ask answers the final with no words and says why on stderr. Measured on
macOS 26 (arm64), whisper.cpp 1.9.2 with `ggml-base.en` kept warm, a client
speaking the route's protocol, 6 runs each: `Finalize` to the final `Results`
45 ms for a 1.2 s line and 72 ms for a 5 s one, both words exact (the clips
were made with `say`, not a person through a microphone).

As with `lamp-sim speech`, the real lamp-voice reaches it only with ten's
loopback switch (step 3), and `lamp-sim run --real-voice=local` (step 4)
starts both.

## Known gaps

- No ears or eyes models run: hands, sound classes and phrases are made by hand (above); no name, touch or real face events.
- The body's home, folded-arm and stand counts are lamp-fc4b's unless `LAMP_SIM_ARM=75c9` (a bad value stops lamp-sim): 75c9's are shoulder 2076 / elbow 2045 folded and 2639 / 2570 at the stand (`get body` read on 10 Nov, as in `docs/lamp.model.json` "measured"), head pitch/roll/yaw home 1791 / 2057 / 2000 and at the stand 1780 / 2057 / 2026 (the real lamp-body's own journal reads of lamp-75c9 on 11 Nov: standing 17:35 [1780, 2057, 2026], folded 17:42 [1756-1760, 2057, 2000], folded 18:05 [1785, 2057, 1999]; the folded pitch is the 10 Nov `get body` 1791, 6 counts over the 18:05 read, and 17:42's pitch reads as mid-move). `LAMP_SIM_ARM=75c9` (the body's counts) and `LAMP_SIM_SERVO_RANGES=75c9` (the stored angle limits) are two settings that combine: both together are all of 75c9's known numbers. The camera's warp (`pose.rs`) still takes fc4b's home and stand as its zero: a stood 75c9's head is 11 counts under its home pitch (about 1 degree) and 26 over its home yaw (about 2.3 degrees).
- macOS (arm64): lamp-sim, lamp-body, lamp-control and lamp-skills build and their tests pass, and the whole lamp runs: the real lamp-body on lamp-sim's pty (ten's portable body), CI's `sim-macos` starting it and seeing it stand. Run here on 13 Nov: `gesture("nod", wait=True)` finished after 9.7 s (the arm stood up first). The pty's modes are not set on its master on macOS (the system refuses it); the body's own open makes the port raw. A pty slave takes `TIOCEXCL` but does not enforce it there, so the body's lock file is the only guard against a second writer. The camera's and the microphone's `lavfi:` test sources work there (three camera snapshots through the Python SDK in 84 to 281 ms, 13 Nov; 50.6 sound blocks a second, none missed); not tried there: the AVFoundation camera and microphone themselves (they would take in the room), the SDK tour.
- Windows: not built or tried (the bus is Unix sockets).
