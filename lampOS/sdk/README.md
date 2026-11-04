# lampOS SDKs: what a developer can do with the lamp

The target is a lamp a developer controls 100%: everything the lamp can do or
sense, a program on a computer can do or read through lamp-control and the
SDKs. Only physical safety, the privacy grants and the person-wins rule stay.

This page maps each capability to its lamp-control op or event, its call in
each SDK and in lamp-mcp, its status and its owner. Use it to find what is
missing and who is building it.

- Python: [`python/`](python/README.md), `pip install lampos`
- JavaScript: [`js/`](js/README.md), `npm install lampos`
- MCP server for agents: [`mcp/`](mcp/README.md), `lamp-mcp`
- Agent skill: [`agent-skill/`](agent-skill/README.md)
- The protocol: [`docs/control.md`](../docs/control.md) and
  [`docs/control.schema.json`](../docs/control.schema.json)

## Status

- **done**: on lamp-voice, in lamp-control, both SDKs and lamp-mcp.
- **SDK missing**: lamp-control has it, a client doesn't yet.
- **PR #n**: open pull request; the call named is what it adds.
- **missing**: nothing in lamp-control yet.

**Validated** gives the lamp and the date a row's call was confirmed on a
real lamp, watched with the laptop's camera, microphone and speaker (for
example `75c9 2 Nov`), or `not yet`. A row is 100% only when it is
done and validated.

## Coverage map

As of lamp-voice `a22552f167` (4 Nov). `lamp` below is a connected client:
Python names first, JavaScript where it differs.

### Connection and state

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Pair a computer, connect | `pair`, `hello` | `pair`, `connect` | (paired at start) | done | Nine | not yet |
| Who is there (never who) | `get presence` | `get("presence")` | `lamp_state`, `lamp://state/presence` | done | Nine | not yet |
| The light now | `get light` | `get("light")` | `lamp_state` | done | Nine | not yet |
| The lamp's mood | `get mood` | `get("mood")` | `lamp_state` | done | Nine | not yet |
| Talking, paused, private, asleep | `get lamp`, `lamp.changed` | `get("lamp")`, `on("lamp.changed")` | `lamp_state`, `lamp_events` | done (quiet hours not produced yet) | Nine | not yet |
| A stored grant a build doesn't know: kept, not granted | lamp-control's start (#4936) | — | — | done | harness-reviewer | 75c9, 2 Nov, harness-reviewer's run (root steps by the local leader): an unknown stored grant kept and written back, only `light` granted |
| The door's limits | `hello` `limits` | `lamp.limits` | — | done; policy limits lifted in PR #4579 | Nine | not yet |
| lamp-control on every lamp | product unit | — | — | PR #4526 | Nine | not yet |

### Body

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Hold the body | `lease.take/renew/release` | `lease("body")` / `withLease` | `lamp://lease`, held per call | done | Nine | not yet |
| Built-in gestures | `gesture` | `gesture` | `lamp_gesture` | done | Nine | not yet |
| A feeling in body and light | `express` | `express` | `lamp_express` | done (3 feelings) | Nine | not yet |
| Look at face, hand, name, angle | `look` | `look` | `lamp_look` | done | Nine | not yet |
| Follow a hand | `look at hand` | `follow_hand` / `followHand` | `lamp_follow_hand`, `lamp_follow_stop` | done; product lamp needs PR #4568 (Ten) | Nine | not yet |
| Each joint (pitch, roll, yaw, shoulder, elbow) | `joints` | `joints` | `lamp_joints` | done | Ten | not yet |
| A move of your own | `move` | `move` | `lamp_move` | done | Nine | not yet |
| A recorded move | `play` | `play` | `lamp_play` | done | Nine | not yet |
| A new motion interrupts the one playing | `busy` today | — | — | PR #4618 | Nine | not yet |
| Each joint's degrees, bounds and caps | `get body` | `get("body")` | `lamp_state` | PR #4580 | Ten | not yet |
| Where each joint is, live; model and twin | `body.state` | model, JS twin | — | PR #4564 | Eighth | not yet |
| Record a move from poses | — | — | — | missing | Ten | not yet |

### Light

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Colour, level, pattern, time | `light` | `light` | `lamp_light` | done: 10 colours, levels 1–5, breathe/candle/pulse, ≤ 30 s | Eighth | not yet |
| Any colour (RGB), each pixel, any brightness | `light` with `rgb` or `pixels`, `brightness`, `fade_ms` | `light_rgb(rgb, …)`, `ring(pixels, for_ms=…, brightness=…)` / `lightRgb(rgb, …)`, `ring(pixels, { forMs, brightness, fadeMs })` | `lamp_ring` | done | Eighth | not yet |
| Every effect and pattern the ring draws (`ring.play`, `ring.stream`) | — | — | — | missing | Eighth | not yet |

### Voice and sound

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Speak a line exactly as sent, or in the lamp's own words | `say`, `rephrase` | `say(text, rephrase=…)` / `say(text, { rephrase })` | `lamp_say` | done (≤ 300 chars until PR #4579) | Eighth | 75c9, 2 Nov, by the local leader: exact, journal and done (finished in 6.1 s; 4 Nov, 5.1 s). Not confirmed by ear |
| The lamp's hum, or a computer's own clips | `sound`, `sound.upload`, `sound.list`, `sound.remove` | `sound`, `upload_sound`, `sounds`, `remove_sound` / `uploadSound`, `removeSound` | `lamp_sound`, `lamp_sounds` | done | Eighth | 75c9, 4 Nov, build 10114, by the local leader: a clip uploaded, listed, played and removed |
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
| The microphone's audio, local only | `microphone` grant only, not routed yet | — | — | missing | harness-reviewer | not yet |

### Eyes and camera

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Someone arrives, leaves, stands, faces it | `person.*` | `on("person.arrived")`… | `lamp_events` | done | Nine | not yet |
| A hand in view | `hand` | `on("hand")` | `lamp_events` | done | Nine | not yet |
| A camera frame, a frame stream, local only | `camera.snapshot`, `camera.stream` | `camera_snapshot`, `camera_stream` / `cameraSnapshot`, `cameraStream` | `lamp_camera_snapshot` | done | Ten | 75c9, 3 Nov 17:46, by the local leader: snapshot 0.14 s; stream at 640x480 and 1280x720, about 15 fps; 4 Nov 10:22, the fast path at 1280x720, 62 frames in 6 s. The ring's mark not seen by eye |

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
| Put a skill on the lamp, list, remove | `skill.push`, `skill.list`, `skill.remove` | `push_skill`, `skills`, `remove_skill` / `pushSkill`, `skills`, `removeSkill`; `lamp skill push/list/remove` | `lamp_skill_push`, `lamp_skills`, `lamp_skill_remove` | done | blue | 75c9, 4 Nov, build 10114, by the local leader: check, push, list, remove. Needs lamp-control started with `--manage-socket` |
| Check a skill folder before pushing | — | `lamp skill check` | — | done | blue | not yet |

## Never readable

The privacy grants stay: who a person is, the words said around the
lamp's own phrases, and the camera and microphone beyond the local network.
The person wins: anything the person does with the lamp comes before a
program's lease.
