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
| any op of the schema | `await lamp.request(op, **fields)` | `lamp.request(op, fields)` | — |

## Events and state

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hear an event | `lamp.on(event, handler, phrases=None)` | `lamp.on(event, handler, { phrases })` | `lamp_wait_for(events, phrases, timeout_s)` |
| stop hearing | `lamp.off(event, handler)` | `lamp.off(event, handler)` | — |
| subscribe by hand | `await lamp.subscribe(events, phrases)`, `await lamp.unsubscribe(events)` | `lamp.subscribe(events, phrases)`, `lamp.unsubscribe(events)` | — |
| read state | `await lamp.get("presence", "light", "mood", "lamp")` | `await lamp.get('presence', 'light')` | `lamp_state(which)`, `lamp://state/<name>` |
| recent events | — | — | `lamp://events/recent` |

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
| `time` | your schedule | accepted, not produced yet |
| `lease` (unasked) | `lease`, `state` (`held`, `suspended`, `lost`), `why` | the lamp takes your lease back, or gives it again |
| `done` (unasked) | `re`, `how` (`finished`, `cut_short`, `dropped`), `why` | an action that ends later has ended (`say`, a `look` at an angle) |

**State** with `get`: `presence` (`count`, `near`, `facing`; needs look),
`light` (the person's own `on`, `colour`, `level`, and `shows`: what the ring
draws now), `mood` (`energy`, `warmth`, `awake`, `asleep`), `lamp`
(`talking`, `paused`, `private`, `asleep`). `light` and `mood` are `null`
until the lamp has heard them.

## Leases

| what | Python | JavaScript | MCP |
|---|---|---|---|
| hold the body | `async with lamp.lease("body", why=...) as lease:` | `await lamp.withLease(['body'], async (lease) => …, { why })` | each body tool takes its own |
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
| a recorded move | `await lamp.play(name)` | `lamp.play(name)` | — | body, a body lease |
| aim the joints | `await lamp.joints(lease, 1000, pitch=5)` | `lamp.joints(lease, 1000, { pitch: 5 })` | — | body, the lease named |
| light | `await lamp.light("green", 3, 10_000, pattern="breathe")` (no `pattern`: steady) | `lamp.light('green', 3, 10000, 'breathe')` (`undefined`: steady) | `lamp_light(colour, level, for_ms, pattern)` | light |
| say | `await lamp.say("Build passed.")` | `lamp.say('Build passed.')` | `lamp_say(text)` | speak |
| a sound | `await lamp.sound(name)` | `lamp.sound(name)` | — | speak |
| stop everything of mine | `await lamp.stop()` | `lamp.stop()` | `lamp_stop()` | none |

Ranges, from lamp-control and the body:
- **light:** colours `yellow`, `red`, `green`, `blue`, `cyan`, `purple`,
  `orange`, `pink`, `white`, `warm`; level 1–5; `for_ms` 1–30,000; pattern
  none (steady), `breathe` (level 2 up), `candle` (3 up) or `pulse` (2 up).
- **gestures:** `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`,
  `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`,
  `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`,
  `dance_waltz`, `dance_jazz`.
- **move:** 2–500 frames of 5 joints (pitch, roll, yaw, shoulder, elbow) in
  counts from home, first and last all 0; `step_ms` 40–1,000; 1.5 to 10 s
  in all; each joint within reach (200, 200, 150, 350, 350 counts) and speed.
- **say:** up to 300 characters. **look:** `yaw` −180 to 180, `pitch` −90 to
  90, right and up positive.

## Skills of your own

| what | Python | JavaScript | command line |
|---|---|---|---|
| check a skill folder | `lamp skill check FOLDER` | — | `lamp skill check FOLDER` |

Pushing a skill to your own lamp (`skills` grant) is coming with `skill.push`.

## Errors

Every refusal is a `LampError` (Python `lampos.errors.BY_CODE[code]`, JS
`byCode[code]`) with `code`, `why` and, for `rate`, `retry_ms`. The codes:
`unpaired`, `no_hello`, `no_grant`, `no_lease`, `lease_lost`, `busy`,
`refused`, `quiet_hours`, `private`, `paused`, `asleep`, `rate`,
`bad_request`, `off`. What to do about each: [errors.md](errors.md).

## The command line

`lamp find`, `lamp pair CODE --pin PIN --host HOST [--port 7731] [--name NAME]`,
`lamp hello [LAMP_ID]`, `lamp fake [--every 10] [--cycles N] [--everything] [--hands]`,
`lamp skill check FOLDER...`. `LAMPOS_HOME` sets where the key and pairings
are kept; `LAMPOS_LOG=debug` prints every frame.
