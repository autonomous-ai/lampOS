# The calls, side by side

The same protocol in three forms. Python calls are coroutines (`await`);
JavaScript calls return promises; lamp-mcp tools are what an agent calls.
"—" means that form doesn't offer it; use `request` (Python and JS) for any
op of the schema. The SDK READMEs (`lampOS/sdk/python/README.md`,
`lampOS/sdk/js/README.md`) are the full reference; a test keeps this page
naming every public call, tool, event and error code they have.

## Connecting

| what | Python | JavaScript | MCP |
|---|---|---|---|
| pair once (owner's code and pin) | `await Lamp.pair(code, host, pin=pin, name=name)` | `await Lamp.pair(code, host, { pin, name })` | run `lamp pair` first |
| connect, say hello | `await Lamp.connect()` (or `Lamp.connect(lamp_id, host=..., keyring=...)`) | `await Lamp.connect()` (or `Lamp.connect(lampId, { host, keyring })`) | `lamp-mcp` connects at start (`--lamp ID`) |
| the lamp, grants, limits | `lamp.info`, `lamp.grants`, `lamp.limits` | `lamp.info`, `lamp.grants`, `lamp.limits` | in `lamp_state("lamp")` and the tool list |
| close | `await lamp.close()`, or `async with` | `await lamp.close()` | — |
| wait until it ends | `await lamp.wait_closed()` | `await lamp.closed()` | — |
| find lamps on this network | `await find()` (`lampos.discovery`, `lampos[find]`) | `await find()` | — |
| any op of the schema | `await lamp.request(op, **fields)` (`on_id=` is told its id before it is sent) | `lamp.request(op, fields)` | — |

## Events and state

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hear an event | `lamp.on(event, handler, phrases=None)` | `lamp.on(event, handler, { phrases })` | `lamp_wait_for(events, phrases, timeout_s)` |
| stop hearing | `lamp.off(event, handler)` | `lamp.off(event, handler)` | — |
| subscribe by hand | `await lamp.subscribe(events, phrases)`, `await lamp.unsubscribe(events)` | `lamp.subscribe(events, phrases)`, `lamp.unsubscribe(events)` | — |
| read state | `await lamp.get("presence", "light", "mood", "lamp")` | `await lamp.get('presence', 'light')` | `lamp_state(which)`, `lamp://state/<name>` |
| the room's air, the lamp's own computer | `await lamp.air()`, `await lamp.device()` | `await lamp.air()`, `await lamp.device()` | `lamp_state(["air", "device"])`, `lamp://state/air` |
| recent events | — | — | `lamp://events/recent` |
| every event, as it comes | `lamp.on(...)` for each | `lamp.on(...)` for each | `lamp_events(since, wait_s)`, or subscribe to `lamp://events` (each new event sends `notifications/resources/updated`) |

**Events** (each with `at`, a UTC time ending `Z`; grant in brackets):

| event | fields | fires when |
|---|---|---|
| `person.arrived` (look) | `count`, `near` (0–1) | a face is seen while the lamp had nobody |
| `person.left` (look) | `count`: 0 | nobody in view for 5 s, after someone was |
| `person.facing` (look) | `facing` | someone turns to the lamp, or the last one turns away |
| `person.stood_up` (look) | `side` | someone in view stands up |
| `hand` (look) | `seen`; when seen `x`, `y`, `size`, `open`, `confidence` | a hand seen, moved, opened, closed or gone, at most 4 a second |
| `phrase.heard` (hear) | `phrase`: one of yours | a line the lamp heard holds your phrase; register up to 16 with `phrases` |
| `name.heard` (hear) | none | someone says the lamp's name |
| `sound.heard` (sound) | `class` (`music`, `laughter`, `door`, `footsteps`), `score` | a class's score rises past 0.5 |
| `touch` | `kind`: `tap` or `pet` | a tap or a pet |
| `button` | `press`: `long_press` | the button is held |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep` | one of them changes |
| `volume.changed` (speak) | `level` (0–100 of what the lamp allows), `muted` | the speaker's volume changes, whoever changed it: the person by voice or in the app, or a computer |
| `air.changed` (air) | `pm1_0`, `pm2_5`, `pm4_0`, `pm10`, `co2_ppm`, `humidity`, `temperature`, `feel` (`fresh`, `fine`, `stuffy`, `very_stuffy`, `unknown`); a value the sensor doesn't know is left out | the first reading, then a value moves by its step (particles 5, CO2 50 ppm, humidity 3, temperature 0.5 °C) or `feel` changes; at most every 5 s |
| `time` | your schedule | accepted, not produced yet |
| `lease` (unasked) | `lease`, `state` (`held`, `suspended`, `lost`), `why` | the lamp takes your lease back, or gives it again |
| `done` (unasked) | `re`, `how` (`finished`, `cut_short`, `dropped`), `why` | an action that ends later has ended (`say`, a turn, a `look` at an angle; a mute a hand lifted, `cut_short`) |

**State** with `get`: `presence` (`count`, `near`, `facing`; needs look),
`light` (the person's own `on`, `colour`, `level`, and `shows`: what the ring
draws now), `mood` (`energy`, `warmth`, `awake`, `asleep`), `lamp`
(`talking`, `paused`, `private`, `asleep`), `air` (the newest reading as
`air.changed` gives it; needs air), `device` (`cpu_load`, `memory_used`,
`disk_free`, `uptime_s`, `temperature_c`; needs device), `volume`
(`level`, `muted`; needs speak). `light`, `mood` and `volume` are `null`
until the lamp has heard them; `air` and `device` are `{"known": false}`
with no reading in 15 s.

## Leases

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hold the body | `async with lamp.lease("body", why=...) as lease:` | `await lamp.withLease(['body'], async (lease) => …, { why })` | each body tool takes its own, or renews the one it holds |
| by hand | `lease = lamp.lease("body")`, `await lease.take()`, `await lease.release()` | `lamp.lease(['body'])`, `lease.take()`, `lease.release()` | — |
| still mine? | `lease.held`, `lease.state`, `lease.lost_why` | `lease.held`, `lease.state` | `lamp://lease` |

Parts: `body` (grant body), `light` (light), `voice` (speak). `for_ms` up to
30,000, renewed for you while held.

## Actions

| what | Python | JavaScript | MCP | grant, lease |
|---|---|---|---|---|
| a feeling (move and light) | `await lamp.express("glad", 0.5)` (`glad`, `lively`, `low`) | `lamp.express('glad', 0.5)` | `lamp_express(feeling, strength)` | body and light |
| a gesture | `await lamp.gesture("nod", wait=False)` | `lamp.gesture('nod', { wait })` | `lamp_gesture(name)` | body |
| look | `await lamp.look("ahead")`, `look(yaw=20, pitch=5)`, `look("hand")` | `lamp.look('ahead')`, `lamp.look({ yaw, pitch })` | `lamp_look(at, yaw, pitch)` | body, a body lease |
| follow a hand | `async with lamp.follow_hand() as following:` | `lamp.withHand(async (following) => …)`, `lamp.followHand().start()` | `lamp_follow_hand(seconds)`, `lamp_follow_stop()` | body and look |
| a move of your own | `await lamp.move(frames, step_ms)` | `lamp.move(frames, stepMs)` | `lamp_move(step_ms, frames)` | body, a body lease |
| a recorded move | `await lamp.play(name)` | `lamp.play(name)` | `lamp_play(name)` | body, a body lease |
| aim the joints | `await lamp.joints(lease, 1000, pitch=5)` | `lamp.joints(lease, 1000, { pitch: 5 })` | `lamp_joints(within_ms, pitch, roll, yaw, shoulder, elbow)` | body, the lease named |
| light | `await lamp.light("green", 3, 10_000, pattern="breathe")` (no `pattern`: steady) | `lamp.light('green', 3, 10000, 'breathe')` (`undefined`: steady) | `lamp_light(colour, level, for_ms, pattern)` | light |
| ring, pixel by pixel | `await lamp.ring(pixels, for_ms=3_000, brightness=50)`: `lamp.ring_pixels` colours (`"#rrggbb"` or `(r, g, b)`), from pixel 0 around the ring | `lamp.ring(pixels, { forMs: 3000, brightness: 50 })` (`lamp.ringPixels` of them) | `lamp_ring(pixels, rgb, brightness, for_ms)` | light |
| one colour of your own | `await lamp.light_rgb("#ff8000", for_ms=10_000, brightness=40)` | `lamp.lightRgb('#ff8000', { forMs: 10000, brightness: 40 })` | | light |
| say | `await lamp.say("Build passed.")` | `lamp.say('Build passed.')` | `lamp_say(text)` | speak |
| a sound (`hum`, or a clip of yours) | `await lamp.sound(name)` | `lamp.sound(name)` | `lamp_sound(name)` | speak |
| keep a clip on the lamp | `await lamp.upload_sound(name, "chime.wav")` | `lamp.uploadSound(name, bytes)` | `lamp_upload_sound(name, path)` | speak |
| your clips | `await lamp.sounds()` | `lamp.sounds()` | `lamp_sounds()` | speak |
| delete a clip | `await lamp.remove_sound(name)` | `lamp.removeSound(name)` | `lamp_remove_sound(name)` | speak |
| the speaker's volume, the person's own (one of `to` 0–100, `step` `up`/`down`, `mute`) | `await lamp.volume(to=30)`, `volume(step="down")`, `volume(mute=True)` | `lamp.volume({ to: 30 })` | `lamp_volume(to, step, mute)`, `lamp://state/volume` | speak |
| start a turn (the lamp asks, the person answers) | `await lamp.start_turn("Shall we start?", wait=True)` | `lamp.startTurn('Shall we start?', { wait: true })` | `lamp_start_turn(prompt, wait)` | speak and hear |
| end the turn you started | `await lamp.end_turn()` | `lamp.endTurn()` | `lamp_end_turn()` | speak |
| your mute on, off | `await lamp.set_listening(False)`, `set_listening(True)` | `lamp.setListening(false)` | `lamp_set_listening(on)` | hear |
| stop everything of mine | `await lamp.stop()` | `lamp.stop()` | `lamp_stop()` | none |

Ranges, from lamp-control and the body:
- **light:** colours `yellow`, `red`, `green`, `blue`, `cyan`, `purple`,
  `orange`, `pink`, `white`, `warm`; level 1–5; `for_ms` 1–30,000; pattern
  none (steady), `breathe` (level 2 up), `candle` (3 up) or `pulse` (2 up).
- **gestures:** `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`,
  `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`,
  `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`,
  `dance_waltz`, `dance_jazz`.
- **move:** 2 frames at least (at most what 10 s allows: 251 at 40 ms) of 5 joints (pitch, roll, yaw, shoulder, elbow) in
  counts from home, first and last all 0; `step_ms` 40–1,000; 1.5 to 10 s
  in all; each joint within reach (200, 200, 150, 350, 350 counts) and speed.
- **clips:** a WAV of 16-bit mono PCM at 24 kHz, up to 30 s, no sample past
  −12 dBFS and no 10 ms past −30 dBFS RMS; a name of a lowercase letter,
  then lowercase letters, digits, `_` or `-`, never `hum`. Up to 64 clips and
  20 MB a computer (`limits.sounds`); a revoke deletes them. A clip plays
  as an exact `say` does: once the person isn't talking.
- **say:** at most 900 UTF-8 bytes (the voice's own cap), no control characters, said exactly as written since #4958;
  `rephrase=True` (`{ rephrase: true }`, `lamp_say(text, rephrase)`) for the
  model's own words, every say's behaviour before it. **look:** `yaw` and `pitch` in degrees,
  right and up positive, any finite number: the body clamps them to its reach.
  Gestures `idle` and `scanning` are taken too (keep `scanning` for when a
  person asks).

## Skills of your own

| what | Python | JavaScript | command line |
|---|---|---|---|
| check a skill folder | `lamp skill check FOLDER` | — | `lamp skill check FOLDER` |
| put a skill folder on the lamp | `await lamp.push_skill(folder)` | `await lamp.pushSkill(folder)` | `lamp skill push FOLDER` |
| this computer's pushed skills | `await lamp.skills()` | `await lamp.skills()` | `lamp skill list` |
| take one off | `await lamp.remove_skill(name)` | `await lamp.removeSkill(name)` | `lamp skill remove NAME` |

These need the `skills` grant. A pushed skill runs on the lamp after the
computer goes, never beyond the computer's other grants, and a revoke turns it
off. `lamp skill push` checks the folder first, as `lamp skill check` does.
A push needs `skills` plus every grant the skill's `[permissions]` asks, is
1 MiB at most packed, and needs lamp-control started with `--manage-socket`
(a product lamp has no lamp-control unit until #4526). To try it:
`lampOS/sdk/examples/hello-skill`, which asks only `speak`.
From lamp-mcp, with the same grant: `lamp_skill_push(folder)`, `lamp_skills()`
and `lamp_skill_remove(name)`.

## Errors

Every refusal is a `LampError` (Python `lampos.errors.BY_CODE[code]`, JS
`byCode[code]`) with `code`, `why` and, for `rate`, `retry_ms`. The codes:
`unpaired`, `no_hello`, `no_grant`, `no_lease`, `lease_lost`, `busy`,
`refused`, `quiet_hours`, `private`, `paused`, `asleep`, `rate`,
`bad_request`, `off`. What to do about each: [errors.md](errors.md).

## The command line

`lamp find`, `lamp pair CODE --pin PIN --host HOST [--port 7731] [--name NAME]`,
`lamp hello [LAMP_ID]`, `lamp fake [--every 10] [--cycles N] [--everything] [--hands]`,
`lamp skill check FOLDER...`, `lamp skill push FOLDER`, `lamp skill list`,
`lamp skill remove NAME`. `LAMPOS_HOME` sets where the key and pairings
are kept; `LAMPOS_LOG=debug` prints every frame.
