# lampOS SDKs: what a developer can do with the lamp

The target is a lamp a developer controls 100%: everything the lamp can do or
sense, a program on a computer can do or read through lamp-control and the
SDKs. Only physical safety, the privacy grants and the person-wins rule stay.

This page maps each capability to its lamp-control op or event, its call in
each SDK and in lamp-mcp, its status and its owner. Use it to find what is
missing and who is building it.

- Python: [`python/`](python/README.md), `pip install ./lampOS/sdk/python`, or without a clone:
  `pip install "git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/python"`
  (installed in a clean virtual environment on 10 Nov 2026; `lamp --help` runs)
- JavaScript: [`js/`](js/README.md), `npm i --install-links /path/to/lampOS/lampOS/sdk/js`
- MCP server for agents: [`mcp/`](mcp/README.md), `lamp-mcp`
  (`pip install ./lampOS/sdk/python ./lampOS/sdk/mcp`)

None of them is on PyPI or npm yet: `pip install lampos` and `npm install
lampos` find nothing. Install from a checkout of this repository's
`lamp-voice` branch, where the SDKs live (the default branch, `main`, has
none): `git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git`.
- Agent skill: [`agent-skill/`](agent-skill/README.md)
- The protocol: [`docs/control.md`](../docs/control.md) and
  [`docs/control.schema.json`](../docs/control.schema.json)

## Try it without a lamp: the virtual desktop lamp

A lamp that runs on your computer, drawn in 3D, that the SDKs and lamp-mcp drive like a real one: the
real `lamp-control` (pairing, grants, leases, rates, refusals, events) and the real `lamp-body` (its
limits, speeds, gestures and lease) run as they are, on a simulated body. Only the hardware is
stood in for: the servos, the light ring, the speaker, and the camera, microphone and the room's
people, air and hands, by your computer's camera and microphone or by hand. Linux for now.

```sh
cd lampOS
cargo build -p lamp-sim -p lamp-body -p lamp-control
cd sdk/js && npm ci
node examples/twin/virtual-lamp.js --lamp-sim ../../target/debug/lamp-sim
```

It prints the twin's URL and opens it, and pairs this computer in a keyring of its own: run your
programs with `LAMPOS_HOME=~/.lampsim/virtual/sdk` (or the `--dir` you gave) and they drive the lamp on
the screen, with no special mode: the same Python and JavaScript SDK calls and lamp-mcp tools, the same
refusals and limits.

What is not the real thing: it moves no real servo and draws no real ring, a `sound` clip is played by the host's
`paplay`, `aplay`, `afplay` or `ffplay` where there is one (else a short silence),
there are no face, hand or sound-class models (the room's people, hands, words and air are made with
`lamp-sim people|hear|hand|air`), and the body's numbers are one lamp's. A lamp check on a real
lamp stays a lamp check. To see where it differs from the SDK's own fake or from a real lamp, run
`node conformance/conformance.js --compare` in `sdk/js`; the same steps run against any of them.

## On a customer lamp today

Customer lamps run the stable build, **11450** (lamp-voice `135bc417d4`,
train 778, promoted 10 Nov, 19:4x). This list was brought to 11450 from the
git history of that commit (what is in it and what is not), and was last
checked on a lamp row by row on **10824**, with skill push on 11091 and the
tap on 11372. This page and the SDK pages describe lamp-voice, which
is newer; where stable does less, it is said here. Check again after the next
stable: this list names the build so you can tell when it is old.

**What works on a customer lamp:** pairing through the app, `hello`, every
state (`get`) and event, leases, `gesture`, `move`, `express`, `look` at
`ahead`, an angle or a `hand`, `joints` for all five joints (`roll`, `shoulder`
and `elbow` while the lamp stands), a head tap and pet as `touch` events,
`body.state`, `light` (named
colours, RGB, each pixel), `say`, `sound` (the hum and your own clips),
`volume`, `stop`, `turn.start`/`turn.end`, `listen.set`,
`camera.snapshot` and `camera.stream`. lamp-control is on every lamp, and
nothing listens until the owner turns developer access on in the app. Rates
and quiet hours are off.

**The gaps, and what to do instead:**

| # | What | On stable 11450 | Do instead |
|---|---|---|---|
| 1 | Skill push (`lamp skill push/list/remove`, `push_skill`, `lamp_skill_push`) | **Works on a stock lamp from 11091**, onto the developer's own lamp: the product unit passes `--manage-socket` (shown on a stock 11091 lamp, 9 Nov; the tag `lampos-lamp-voice-11308`'s `deploy/product/lamp-control.service` passes it too (line 31)). Only 11048 and older refuse it, "skill push is not on this lamp". This is the developer's own lamp: there is no route to the Store from the SDK yet (`lamp skill submit` waits for the Store's side) | Push, list and remove on the lamp (`lamp skill push NAME`; the owner says yes at the lamp). On 11048 and older, use the fake lamp (`lamp fake`). `lamp skill check` works anywhere, offline |
| 2 | `play`; a look at a face or a name | `play` is refused, "not routed yet: waits for client packages" (from the next build: a pushed skill's recorded move). `look` at `face` or `name` is refused, "not in this release". `joints` for `roll`, `shoulder` and `elbow` works in 11450 (the body's pose, only while the lamp stands; 11048 and older refuse them, "not routed yet") | Send a recorded move as `move` (your own frames of all 5 joints); look at `ahead` or an angle; or a built-in `gesture` |
| 3 | Letting the base go limp, a holder's pose | Only in lamp-body's own lease socket; lamp-control sends neither, so no SDK call reaches them | None from a computer yet |
| 3a | A motion's end (`wait=True`) | `gesture`, `move` and `express` answer `{"to":"body"}` and no `done` follows: a wait returns when the lamp takes the motion. The next build ends it at the body's `done` (hello's `limits.motion_done`, #5835) | Treat the answer as "taken"; watch `body.state` for where the joints are |
| 4 | Finding the lamp by mDNS | Nothing on the lamp announces `_lampos._tcp`, so `lamp find` finds nothing, and `lamp pair` without `--host` finds nothing | Pass the lamp's address: `--host lamp-xxxx.local` (the lamp's own name) or its IP address. After pairing the address is kept |
| 5 | Event times | `at` is UTC, written `…Z` (`2026-11-09T06:30:01Z`), on every build, not the lamp's local time with an offset | Turn it into local time on your side |

**On older stables** (10824 and before): a motion asked while another plays was
refused `busy`, "busy: a motion plays for N ms more", with `retry_ms`: wait
`retry_ms` and ask again. In 11450 a new `gesture`, `move` or `express`
replaces the one playing instead (the body may still drop yours for the
person); and a `look` on a resting lamp, which on 10579 and 10824 could end
`cut_short`, "not there in time: something holds the head", now ends
`finished` or `clamped` with where the head stopped.

Also not on customer lamps: the camera's fast frame path and the microphone's
audio are merged but off by default (`LAMP_EYES_FAST_STREAM`, `--microphone`),
so `camera.stream` sends the lamp's working frames. `look` at `face` or
`name` is "not in this release" on every build.

## Pair a computer with a customer lamp

You need the lamp's owner, with the app, for steps 1 and 2. The app's words
may differ slightly from the ones here; what it sends to the lamp is in
[docs/control.md](../docs/control.md), "Running the door" and "Pairing,
once per computer".

1. **Turn developer access on.** In the app, the owner turns on the lamp's
   developer setting. The lamp starts lamp-control within moments; until
   then nothing listens on port 7731. Turning it off again closes every
   computer's connection.
2. **Get a code.** The owner chooses to add a developer computer and ticks
   what it may do (the grants: `body`, `light`, `speak`, `look`, `hear`,
   `sound`, `skills`, `air`, `device`: not `camera`, `microphone` or `notes`, which the app does not offer; see the GUIDE's "Granting camera, microphone and notes today"). The lamp says the first 8 characters of its pin
   aloud, and the app shows the same 8 for the owner to confirm they match:
   that is how you know the app is talking to the lamp in front of you. Only
   after that does the app show the 8-digit code and the lamp's full pin (64
   hex characters). If the lamp couldn't say its pin, the app cancels the
   code; ask for a new one. The code lasts 5 minutes, and a new one voids the
   old one.
3. **Install the SDK** on your computer, from a checkout of this repository:

   ```sh
   git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git && cd lampOS
   python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
   pip install ./lampOS/sdk/python
   ```

4. **Pair**, on the same network as the lamp, with its address (gap 4):

   ```sh
   lamp pair 12345678 --pin 3f9a… --host lamp-xxxx.local   # or --host 192.168.1.23
   lamp hello                                               # connects, shows the grants
   ```

   The SDK checks the lamp's certificate against the pin after the TLS and
   WebSocket handshakes and before any protocol frame or pairing proof, so
   the code's proof never goes to another host. The lamp gives a small
   nod. The pairing is kept in `~/.config/lampos/`; later connections need no
   code.
5. **Say hello from your program** (`speak` grant):

   ```python
   import asyncio
   from lampos import Lamp

   async def main():
       async with await Lamp.connect() as lamp:
           said = asyncio.Event()
           await lamp.on("done", lambda event: said.set())  # the say's end
           await lamp.say("Hello from my computer.")
           await asyncio.wait_for(said.wait(), 30)  # closing earlier stops it

   asyncio.run(main())
   ```

   The lamp says the line in its own voice. Then try the examples in
   [`python/examples/`](python/examples/) and
   [`js/examples/`](js/examples/), or `lamp-mcp` for an agent.

The owner can take the body back or revoke a computer from the app at any
time; a revoke closes its connections at once.

## Status

- **done**: on lamp-voice, in lamp-control, both SDKs and lamp-mcp. Where
  stable 11450 (customer lamps) does less, the row says so; see
  [On a customer lamp today](#on-a-customer-lamp-today).
- **SDK missing**: lamp-control has it, a client doesn't yet.
- **PR #n**: open pull request; the call named is what it adds.
- **missing**: nothing in lamp-control yet.

**Validated** gives the lamp and the date a row's call was confirmed on a
real lamp, watched with the laptop's camera, microphone and speaker (for
example `75c9 2 Nov`), or `not yet`. A row is 100% only when it is
done and validated.

## Coverage map

As of lamp-voice `0df9b3b749` (9 Nov), with the stable column brought to 11450 (`135bc417d4`) from git on 10 Nov; the evidence column still names the build each run was made on. `lamp` below is a connected client:
Python names first, JavaScript where it differs.

### Connection and state

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Pair a computer, connect | `pair`, `hello` | `pair`, `connect` | (paired at start) | done; no mDNS announcement on any build, so pair with `--host` | Nine | 75c9, 9 Nov, stable 10824's binaries, by blue: paired through a code with `--host lamp-75c9.local`, `hello` |
| Who is there (never who) | `get presence` | `get("presence")` | `lamp_state`, `lamp://state/presence` | done | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `{count: 0, near: 0.0, facing: false}` with nobody in view |
| The light now | `get light` | `get("light")` | `lamp_state` | done | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): the person's light read (`blue`, level 1, off) with `shows` |
| The lamp's mood | `get mood` | `get("mood")` | `lamp_state` | done | Nine | not yet: on 75c9 (10824) `mood` answered `null`, no mood state on the bus then |
| Talking, paused, private, asleep | `get lamp`, `lamp.changed` | `get("lamp")`, `on("lamp.changed")` | `lamp_state`, `lamp_events` | done (quiet hours not produced yet) | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `get lamp` answered; `lamp.changed` not seen (nothing changed in the run) |
| A stored grant a build doesn't know: kept, not granted | lamp-control's start (#4936) | — | — | done | harness-reviewer | 75c9, 2 Nov, harness-reviewer's run (root steps by the local leader): an unknown stored grant kept and written back, only `light` granted |
| The door's limits | `hello` `limits` | `lamp.limits` | — | done; policy-only limits lifted, rates and quiet hours off | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `rates: off`, `lease_max_ms` 600000, `say_max_bytes` 900, `ring.pixels` 32 |
| lamp-control on every lamp | product unit | — | — | done: on stable, off until the owner turns developer access on in the app | Nine | not yet |

### Body

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Hold the body | `lease.take/renew/release` | `lease("body")` / `withLease` | `lamp://lease`, held per call | done | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `lease.take` held, `release`d |
| Built-in gestures | `gesture` | `gesture` | `lamp_gesture` | done | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `nod` played (the body's journal: `gesture nod`, `gesture over` 3 s later) |
| A feeling in body and light | `express` | `express` | `lamp_express` | done (3 feelings) | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `glad` 0.6: `glad-lift` played; light "the light is off" (the person's light was off) |
| Look at face, hand, name, angle | `look` | `look` | `lamp_look` | done for `ahead` and an angle; `hand` is in development (the lamp doesn't track hands yet, see Follow a hand); `name` "not in this release" (no sound direction); `face`: one turn toward the face in view, next build (11450 and older: "not in this release"). On a resting lamp a look could end `cut_short` on 10824; from 11048 it ends `finished` or `clamped` (#5711) | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): `look` yaw 15 taken; `look` at `face` refused "not in this release: look at a face or a name" |
| Follow a hand | `look at hand` | `follow_hand` / `followHand` | `lamp_follow_hand`, `lamp_follow_stop` | in development: the door is in; nothing publishes `humans/hands/tracked` and lamp-body refuses `follow` | Nine | not yet |
| Each joint (pitch, roll, yaw, shoulder, elbow) | `joints` | `joints` | `lamp_joints` | all five on stable 11450 (`roll`, `shoulder` and `elbow` as the body's pose, only while the lamp stands, d2da8d8914); 11048 and older: `yaw` and `pitch` only, the others refused "not routed yet" | Ten | 75c9, 7 Nov, stable 10579: pitch alone went; roll refused (again 9 Nov, 10824) |
| A move of your own | `move` | `move` | `lamp_move` | done | Nine | 75c9, 9 Nov, stock 10824, nine's SDK run (Python): a 2 s yaw move played (`gesture control`, `over` 2 s later) |
| A recorded move | `play` | `play` | `lamp_play` | in the clients and the fakes; lamp-control sends a pushed skill's `moves/NAME.json` as a `move` (in review); the customer build still refuses it, "not routed yet: waits for client packages" | Nine, Blue | refused as written: 75c9, 9 Nov, 10824 |
| A new motion interrupts the one playing | `gesture`, `move`, `express` replace | (no new call) | — | on stable 11450 (`6167fe37a1`); 10824 and older refuse `busy` with `retry_ms` | Nine | not yet |
| A motion's end told: `wait` ends when it has | `done` on gesture, move, express; hello `limits.motion_done` | `wait=True` / `{ wait: true }` | `lamp_gesture`, `lamp_move` | next build (#5835); stable 10824 returns at the answer | Nine | 75c9, 9 Nov, lamp-built lamp-control and lamp-body: each wait matched the body's own start and end (a 2 s move 2075 ms; a replaced move `cut_short`) |
| Each joint's degrees, bounds and caps | `get body` | `get("body")` | `lamp_state` | done | Ten | 75c9, 7 Nov, stable 10579: known, each joint's range |
| Where each joint is, live; model and twin | `body.state` | model, JS twin | — | done, on stable 11450 (#4564); not on 10824 | Eighth | 75c9, 9 Nov, #4564's lamp-control: 12.5 events a second |
| Record a move from poses (degrees, eased) | `move`, made by the client | `move_poses`, `poses_to_frames` / `movePoses`, `posesToFrames` | `lamp_move_poses` | done in the clients (it sends `move`, which stable 10824 takes) | Ten | 75c9, 9 Nov, stock 10824: played 2.8 s as computed; a 40° pose refused before sending |
| Let the base go limp; a holder's pose | — (lamp-body's lease socket only) | — | — | missing in lamp-control | — | not yet |

### Light

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Colour, level, pattern, time | `light` | `light` | `lamp_light` | done: 10 colours, levels 1–5, breathe/candle/pulse, ≤ 30 s | Eighth | not yet |
| Any colour (RGB), each pixel, any brightness | `light` with `rgb` or `pixels`, `brightness`, `fade_ms` | `light_rgb(rgb, …)`, `ring(pixels, for_ms=…, brightness=…)` / `lightRgb(rgb, …)`, `ring(pixels, { forMs, brightness, fadeMs })` | `lamp_ring` | done | Eighth | not yet |
| Every effect and pattern the ring draws (`ring.play`, `ring.stream`) | — | — | — | missing | Eighth | not yet |

### Voice and sound

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Speak a line exactly as sent, or in the lamp's own words | `say`, `rephrase` | `say(text, rephrase=…)` / `say(text, { rephrase })` | `lamp_say` | done (≤ 900 UTF-8 bytes) | Eighth | 75c9, 2 Nov, by the local leader: exact, journal and done (finished in 6.1 s; 4 Nov, 5.1 s). Not confirmed by ear |
| The lamp's hum, or a computer's own clips | `sound`, `sound.upload`, `sound.list`, `sound.remove` | `sound`, `upload_sound`, `sounds`, `remove_sound` / `uploadSound`, `removeSound` | `lamp_sound`, `lamp_sounds` | done | Eighth | 75c9, 4 Nov, build 10114, by the local leader: a clip uploaded, listed, played and removed |
| A computer's own notes on the lamp (small key/value memory, kept across restarts) | `notes.set`, `notes.get`, `notes.delete`, `notes.list` | `notes.set`, `notes.get`, `notes.delete`, `notes.list` / `notesSet`, `notesGet`, `notesDelete`, `notesList` | `lamp_notes_set`, `lamp_notes_get`, `lamp_notes_delete`, `lamp_notes_list` | fake lamp, tests | Banana | not yet (lamp run pending) |
| Speaker volume: a level, a step, mute | `volume`, `get volume`, `volume.changed` | `volume(to=…, step=…, mute=…)`, `get("volume")`, `on("volume.changed")` / `volume({ to, step, mute })` | `lamp_volume`, `lamp_state` | done | Eighth | 75c9, 4 Nov, build 10139, by the local leader |
| Stop what it does | `stop` | `stop` | `lamp_stop` | done | Nine | not yet |

### Ears and microphone

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Its own phrases heard | `phrase.heard` | `on("phrase.heard", phrases=…)` | `lamp_events`, `lamp_wait_for` | done | Nine | not yet |
| Its name called | `name.heard` | `on("name.heard")` | `lamp_events` | done | Nine | not yet |
| Kinds of sound | `sound.heard` | `on("sound.heard")` | `lamp_events` | done | Nine | not yet |
| Open a conversation turn, or end its own | `turn.start`, `turn.end` | `start_turn`, `end_turn` / `startTurn`, `endTurn` | `lamp_start_turn`, `lamp_end_turn` | done | Nine | 75c9 3–4 Nov, combined builds: turn answered, same computer continuing, own end cutting a story (by ear) |
| A computer's own mute | `listen.set` | `set_listening` / `setListening` | `lamp_set_listening` | done | Nine | not yet |
| The microphone's audio, local only | `microphone.stream`, `microphone.audio` (binary 20 ms blocks), `microphone.ended`; off unless `--microphone on` | `microphone_stream` / `microphoneStream`, `on("microphone.audio")` | — (a stream is for a program, as the camera's) | missing (local, waits for the owner's privacy yes) | banana | not yet |

### Eyes and camera

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Someone arrives, leaves, stands, faces it | `person.*` | `on("person.arrived")`… | `lamp_events` | done | Nine | not yet |
| A hand in view | `hand` | `on("hand")` | `lamp_events` | done | Nine | not yet |
| A camera frame, a frame stream, local only | `camera.snapshot`, `camera.stream` | `camera_snapshot`, `camera_stream` / `cameraSnapshot`, `cameraStream` | `lamp_camera_snapshot` | done; the fast frame path isn't on customer lamps | Ten | 75c9, 3 Nov 17:46, by the local leader: snapshot 0.14 s; stream at 640x480 and 1280x720, about 15 fps; 4 Nov 10:22, the fast path at 1280x720, 62 frames in 6 s. The ring's mark not seen by eye |

### Touch and controls

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Tap, double tap, long press, pet | `touch` | `on("touch")` | `lamp_events` | done | Nine | not yet |
| The button | `button` | `on("button")` | `lamp_events` | done | Nine | not yet |
| The time of day | `time` | `on("time")` | `lamp_events` | done | Nine | not yet |

### Air and device

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Particles, humidity, temperature, CO2 | `get air`, `air.changed` | `air()`, `on("air.changed")` | `lamp_state`, `lamp_wait_for` | done | harness-reviewer | 75c9, 2 Nov, by the local leader: answers known:false, no sensor (values not validated) |
| The lamp's processor, memory, disk, temperatures | `get device` | `device()` | `lamp_state` | done | harness-reviewer | 75c9, 2 Nov, by the local leader: against the lamp's /proc, df and thermal zones |

### Skills

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Put a skill on the lamp, list, remove | `skill.push`, `skill.list`, `skill.remove` | `push_skill`, `skills`, `remove_skill` / `pushSkill`, `skills`, `removeSkill`; `lamp skill push/list/remove` | `lamp_skill_push`, `lamp_skills`, `lamp_skill_remove` | done; on customer lamps from 11091 on (the product unit passes `--manage-socket`, #5819); stable 10824's unit didn't, so it refused "skill push is not on this lamp" (75c9, 9 Nov) | blue | 75c9, 4 Nov, build 10114, by the local leader: check, push, list, remove; 75c9, 9 Nov, 10824's lamp-control under the next build's unit: push, list, remove, push, revoke turned it off |
| Check a skill folder before pushing | — | `lamp skill check` | — | done | blue | not yet |

## Never readable

The privacy grants stay: who a person is, the words said around the
lamp's own phrases, and the camera and microphone beyond the local network.
The person wins: anything the person does with the lamp comes before a
program's lease.
