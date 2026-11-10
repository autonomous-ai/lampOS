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
| keep running across the lamp's restarts (every update) | `await Lamp.keep_connected(setup)`: connects, runs `setup(lamp)`, connects again after each close; raises on what can't mend (`NotPaired`, `no_grant`, ...) | `await Lamp.keepConnected(setup, { signal })` | — (an agent's tools have no setup to keep: lamp-mcp's README says how it meets a restart) |
| find lamps on this network | `await find()` (`lampos.discovery`, the `[find]` extra) | `await find()` | — |
| any op of the schema | `await lamp.request(op, **fields)` (`on_id=` is told its id before it is sent) | `lamp.request(op, fields)` | — |

No lamp announces itself by mDNS yet (stable 10824): `find()` and `lamp find`
return none, so give `host` (`lamp-xxxx.local` or the IP address). The
pairing keeps it for later connects.

## Events and state

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hear an event | `lamp.on(event, handler, phrases=None)` | `lamp.on(event, handler, { phrases })` | `lamp_wait_for(events, phrases, timeout_s)` |
| stop hearing | `lamp.off(event, handler)` | `lamp.off(event, handler)` | — |
| subscribe by hand | `await lamp.subscribe(events, phrases)`, `await lamp.unsubscribe(events)` | `lamp.subscribe(events, phrases)`, `lamp.unsubscribe(events)` | — |
| read state | `await lamp.get("presence", "light", "mood", "lamp")` | `await lamp.get('presence', 'light')` | `lamp_state(which)`, `lamp://state/<name>` |
| the room's air, the lamp's own computer | `await lamp.air()`, `await lamp.device()` | `await lamp.air()`, `await lamp.device()` | `lamp_state(["air", "device"])`, `lamp://state/air` |
| the body: each joint's place and bounds, the caps | `await lamp.body_info()` | `await lamp.bodyInfo()` | `lamp_state(["body"])` |
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
(`level`, `muted`; needs speak), `body` (needs body: `yaw`, `pitch`, `roll`,
`shoulder`, `elbow`, each with `deg` from its home, `min_deg` and `max_deg`;
the speed, load and heat caps). `light`, `mood` and `volume` are `null` until
the lamp has heard them; `air` and `device` are `{"known": false}` with no
reading in 15 s, and `body` until the body has found home.

## Leases

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hold the body | `async with lamp.lease("body", why=...) as lease:` | `await lamp.withLease(['body'], async (lease) => …, { why })` | each body tool takes its own, or renews the one it holds |
| by hand | `lease = lamp.lease("body")`, `await lease.take()`, `await lease.release()` | `lamp.lease(['body'])`, `lease.take()`, `lease.release()` | — |
| still mine? | `lease.held`, `lease.state`, `lease.lost_why` | `lease.held`, `lease.state` | `lamp://lease` |

Parts: `body` (grant body), `light` (light), `voice` (speak). `for_ms` up to
600,000 a take, renewed for you while held.

## Actions

| what | Python | JavaScript | MCP | grant, lease |
|---|---|---|---|---|
| a feeling (move and light) | `await lamp.express("glad", 0.5)` (`glad`, `lively`, `low`) | `lamp.express('glad', 0.5)` | `lamp_express(feeling, strength)` | body and light |
| a gesture | `await lamp.gesture("nod", wait=False)` | `lamp.gesture('nod', { wait })` | `lamp_gesture(name)` | body |
| look (`face`, `name` refused on stable 10824) | `await lamp.look("ahead")`, `look(yaw=20, pitch=5)`, `look("hand")` | `lamp.look('ahead')`, `lamp.look({ yaw, pitch })` | `lamp_look(at, yaw, pitch)` | body, a body lease |
| follow a hand | `async with lamp.follow_hand() as following:` | `lamp.withHand(async (following) => …)`, `lamp.followHand().start()` | `lamp_follow_hand(seconds)`, `lamp_follow_stop()` | body and look |
| a move of your own | `await lamp.move(frames, step_ms)` | `lamp.move(frames, stepMs)` | `lamp_move(step_ms, frames)` | body, a body lease |
| a move through poses in degrees, eased | `await lamp.move_poses([{"pitch": 10}, {"yaw": -8}])` | `lamp.movePoses([{ pitch: 10 }, { yaw: -8 }])` | `lamp_move_poses(poses)` | body, a body lease |
| a recorded move (refused on every lamp today) | `await lamp.play(name)` | `lamp.play(name)` | `lamp_play(name)` | body, a body lease |
| aim the joints (stable 10824: `yaw`, `pitch` only) | `await lamp.joints(lease, 1000, pitch=5)` | `lamp.joints(lease, 1000, { pitch: 5 })` | `lamp_joints(within_ms, pitch, roll, yaw, shoulder, elbow)` | body, the lease named |
| light | `await lamp.light("green", 3, 10_000, pattern="breathe")` (no `pattern`: steady) | `lamp.light('green', 3, 10000, 'breathe')` (`undefined`: steady) | `lamp_light(colour, level, for_ms, pattern)` | light |
| ring, pixel by pixel | `await lamp.ring(pixels, for_ms=3_000, brightness=50)`: `lamp.ring_pixels` colours (`"#rrggbb"` or `(r, g, b)`), from pixel 0 around the ring | `lamp.ring(pixels, { forMs: 3000, brightness: 50 })` (`lamp.ringPixels` of them) | `lamp_ring(pixels, rgb, brightness, for_ms)` | light |
| one colour of your own | `await lamp.light_rgb("#ff8000", for_ms=10_000, brightness=40)` | `lamp.lightRgb('#ff8000', { forMs: 10000, brightness: 40 })` | | light |
| keep a light on past 30 s | `holding = await lamp.hold_light("warm", 3)`; `holding.stop()` | `const holding = await lamp.holdLight('warm', 3)`; `holding.stop()` | asked again at half the hold limit; a refusal goes to `on_error` / `onError` | light |
| say | `await lamp.say("Build passed.")` | `lamp.say('Build passed.')` | `lamp_say(text)` | speak |
| a sound (`hum`, or a clip of yours) | `await lamp.sound(name)` | `lamp.sound(name)` | `lamp_sound(name)` | speak |
| keep a clip on the lamp | `await lamp.upload_sound(name, "chime.wav")` | `lamp.uploadSound(name, bytes)` | `lamp_upload_sound(name, path)` | speak |
| your clips | `await lamp.sounds()` | `lamp.sounds()` | `lamp_sounds()` | speak |
| delete a clip | `await lamp.remove_sound(name)` | `lamp.removeSound(name)` | `lamp_remove_sound(name)` | speak |
| keep a note of your own (any JSON, small) | `await lamp.notes.set("mood", {"calm": True})` or `notes_set(key, value)` | `lamp.notes.set('mood', { calm: true })` or `notesSet(key, value)` | `lamp_notes_set(key, value)` | notes |
| read a note | `await lamp.notes.get("mood", default=None)` or `notes_get(key)` | `lamp.notes.get('mood', fallback)` or `notesGet(key)` | `lamp_notes_get(key)` | notes |
| drop a note | `await lamp.notes.delete("mood")` or `notes_delete(key)` | `lamp.notes.delete('mood')` or `notesDelete(key)` | `lamp_notes_delete(key)` | notes |
| your note keys | `await lamp.notes.list("mo")` or `notes_list(prefix)` | `lamp.notes.list('mo')` or `notesList(prefix)` | `lamp_notes_list(prefix)` | notes |
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
- **notes:** a key of 1 to 64 bytes of letters, digits and `. _ : -`; a value
  of any JSON, at most 16384 bytes serialized; up to 256 keys and 256 KiB of
  keys and values a computer (`limits.notes`), one past either refused
  whole. Only yours (the lamp knows you by your pairing), kept across your
  program's and the lamp's restarts, deleted by a revoke. 30 writes and 120
  reads a minute.
- **say:** at most 900 UTF-8 bytes (the voice's own cap), no control characters, said exactly as written since #4958;
  `rephrase=True` (`{ rephrase: true }`, `lamp_say(text, rephrase)`) for the
  model's own words, every say's behaviour before it. **look:** `yaw` and `pitch` in degrees,
  right and up positive, any finite number: the body clamps them to its reach.
  Gestures `idle` and `scanning` are taken too (keep `scanning` for when a
  person asks).

## The microphone

| what | Python | JavaScript | MCP | grant |
|---|---|---|---|---|
| stream the room's sound | `await lamp.microphone_stream(True)` | `lamp.microphoneStream(true)` | — (for a program, not an agent's turn) | microphone |
| stop it | `await lamp.microphone_stream(False)`, or `stop()` | `lamp.microphoneStream(false)`, or `stop()` | — | none |

The answer is `{"stream": "on", "sample_rate": 16000, "channels": 1,
"format": "s16le", "block_ms": 20, "source": "mic1"}`. Then, unasked:

| event | fields | fires when |
|---|---|---|
| `microphone.audio` (microphone) | `sequence`, `lost`, `sample_rate`, `channels`, `format`, `bytes`, and the samples as `data` (bytes in Python, a Buffer in JS; 640 a block) | each 20 ms block the lamp's ears hear. `lost` counts blocks lost since the last one sent: on the way, or because this program read too slowly (the lamp never queues more than 4) |
| `microphone.ended` (microphone) | `why` | the lamp turned private, or more than one person came into view ("someone else is here"), both at once; the lamp's eyes stopped saying who is in view for 2 s ("the room isn't seen"); the ring didn't say it drew its mark within 1 s ("the ring is not showing the microphone's mark"); or no sound came for 2 s: "the lamp's microphone isn't shared here", which is what a lamp says whose ears don't publish their sound (`LAMP_BUS_AUDIO` off) |

- **Never while private,** and refused `private` then. **Never with more
  than one person in view:** the lamp can't tell its owner from anyone else,
  so it is refused, "someone else is here", and with no fresh word from its
  eyes on who is in view it is refused too, "the room isn't seen". **Off
  unless the owner switched
  it on** (`LAMP_CONTROL_MICROPHONE=on`): refused, "the microphone stream is
  off on this lamp". **Only on the lamp's own network,** as the camera. The ring shows its mark while it streams,
  and sound goes only once the ring says it drew it; where it can't, there
  is no microphone.
- Listen with `lamp.on("microphone.audio", handler)`: no subscription, it
  comes with the stream you asked for. On again replaces the stream.
- The fake lamp streams a quiet 440 Hz tone; `lose_audio(n)` / `loseAudio(n)`
  makes a gap; `set_private()` / `setPrivate()` and `set_in_view(2)` /
  `setInView(2)` end it.

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
1 MiB at most packed, and needs lamp-control started with `--manage-socket`.
**Not on stable 10824's customer lamps:** their product unit doesn't pass it,
so every skill op is refused, "skill push is not on this lamp". The next
build's product unit passes it. Try it on `lamp fake` meanwhile. To try it:
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
