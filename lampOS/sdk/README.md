# lampOS SDKs: what a developer can do with the lamp

The target is a lamp a developer controls 100%: everything the lamp can do or
sense, a program on a computer can do or read through lamp-control and the
SDKs. Only physical safety, the privacy grants and the person-wins rule stay.

This page maps each capability to its lamp-control op or event, its call in
each SDK and in lamp-mcp, its status and its owner. Use it to find what is
missing and who is building it.

- Python: [`python/`](python/README.md), `pip install ./lampOS/sdk/python`
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

## On a customer lamp today (stable 10824)

Customer lamps run the stable build, **10824** (lamp-voice `65991c7229`,
stable since 9 Nov). This page and the SDK pages describe lamp-voice, which
is newer; where stable does less, it is said here. Check again after the next
stable: this list names the build so you can tell when it is old.

**What works on a customer lamp:** pairing through the app, `hello`, every
state (`get`) and event, leases, `gesture`, `move`, `express`, `look` at
`ahead`, an angle or a `hand`, `joints` for `yaw` and `pitch`, `light` (named
colours, RGB, each pixel), `say`, `sound` (the hum and your own clips),
`volume`, `stop`, `turn.start`/`turn.end`, `listen.set`,
`camera.snapshot` and `camera.stream`. lamp-control is on every lamp, and
nothing listens until the owner turns developer access on in the app. Rates
and quiet hours are off.

**The gaps, and what to do instead:**

| # | What | On stable 10824 | Do instead |
|---|---|---|---|
| 1 | Skill push (`lamp skill push/list/remove`, `push_skill`, `lamp_skill_push`) | Refused `refused`, "skill push is not on this lamp": the product unit doesn't start lamp-control with `--manage-socket`. Not yet on customer lamps | Try it on the fake lamp (`lamp fake`). On a real lamp, run the same behaviour as a program on your computer while it is connected. `lamp skill check` works anywhere, offline |
| 2 | `joints` beyond yaw and pitch; `play` | `roll`, `shoulder` and `elbow` are refused, "not routed yet: waits for the body's goal stream for roll, shoulder and elbow; yaw and pitch go now". `play` is refused, "not routed yet: waits for client packages". `sound` is served | Name only `yaw` and `pitch` (both, if the body hasn't said where it is in the last 15 s). For a whole-body motion, use `move` (your own frames of all 5 joints) or a built-in `gesture` |
| 3 | Letting the base go limp, a holder's pose | Only in lamp-body's own lease socket; lamp-control sends neither, so no SDK call reaches them | None from a computer yet |
| 4 | Finding the lamp by mDNS | Nothing on the lamp announces `_lampos._tcp`, so `lamp find` finds nothing, and `lamp pair` without `--host` finds nothing | Pass the lamp's address: `--host lamp-xxxx.local` (the lamp's own name) or its IP address. After pairing the address is kept |
| 5 | Event times | `at` is UTC, written `…Z` (`2026-11-09T06:30:01Z`), on every build, not the lamp's local time with an offset | Turn it into local time on your side |

**Two more differences from lamp-voice:**

- **A motion while another plays is refused `busy`** on stable 10824, "busy:
  a motion plays for N ms more", with `retry_ms`, while the last motion
  lamp-control sent (from any computer) still plays: wait `retry_ms` and ask
  again. The lamp's own motions don't count; the body may still drop yours
  for one of those. On lamp-voice a new `gesture`, `move` or `express` replaces the one
  playing instead; that comes with the next stable.
- **A `look` on a resting lamp may end `cut_short`**, "not there in time:
  something holds the head", with no hand on it (lamp-75c9, stable 10579). The
  head has stopped just short of where it was asked. Treat a `cut_short` close
  to your target as there, or ask again. Fixed on lamp-voice (#5711): the
  next stable answers `there` with where the head stopped, `clamped`.

Also not on customer lamps: the camera's fast frame path (lamp-eyes doesn't
serve its frames socket there; `camera.stream` sends the lamp's working
frames instead), the microphone's audio (a grant only, not routed), and
`body.state` (on lamp-voice since #4564, with the next stable). `look` at `face` or `name` is "not in
this release" on every build.

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
   `sound`, `camera`, …). The lamp says the first 8 characters of its pin
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
  stable 10824 (customer lamps) does less, the row says so; see
  [On a customer lamp today](#on-a-customer-lamp-today-stable-10824).
- **SDK missing**: lamp-control has it, a client doesn't yet.
- **PR #n**: open pull request; the call named is what it adds.
- **missing**: nothing in lamp-control yet.

**Validated** gives the lamp and the date a row's call was confirmed on a
real lamp, watched with the laptop's camera, microphone and speaker (for
example `75c9 2 Nov`), or `not yet`. A row is 100% only when it is
done and validated.

## Coverage map

As of lamp-voice `0df9b3b749` (9 Nov); stable is 10824 (`65991c7229`). `lamp` below is a connected client:
Python names first, JavaScript where it differs.

### Connection and state

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Pair a computer, connect | `pair`, `hello` | `pair`, `connect` | (paired at start) | done; no mDNS announcement on any build, so pair with `--host` | Nine | 75c9, 9 Nov, stable 10824's binaries, by blue: paired through a code with `--host lamp-75c9.local`, `hello` |
| Who is there (never who) | `get presence` | `get("presence")` | `lamp_state`, `lamp://state/presence` | done | Nine | not yet |
| The light now | `get light` | `get("light")` | `lamp_state` | done | Nine | not yet |
| The lamp's mood | `get mood` | `get("mood")` | `lamp_state` | done | Nine | not yet |
| Talking, paused, private, asleep | `get lamp`, `lamp.changed` | `get("lamp")`, `on("lamp.changed")` | `lamp_state`, `lamp_events` | done (quiet hours not produced yet) | Nine | not yet |
| A stored grant a build doesn't know: kept, not granted | lamp-control's start (#4936) | — | — | done | harness-reviewer | 75c9, 2 Nov, harness-reviewer's run (root steps by the local leader): an unknown stored grant kept and written back, only `light` granted |
| The door's limits | `hello` `limits` | `lamp.limits` | — | done; policy-only limits lifted, rates and quiet hours off | Nine | not yet |
| lamp-control on every lamp | product unit | — | — | done: on stable, off until the owner turns developer access on in the app | Nine | not yet |

### Body

| Capability | lamp-control | Python / JS | lamp-mcp | Status | Owner | Validated |
|---|---|---|---|---|---|---|
| Hold the body | `lease.take/renew/release` | `lease("body")` / `withLease` | `lamp://lease`, held per call | done | Nine | not yet |
| Built-in gestures | `gesture` | `gesture` | `lamp_gesture` | done | Nine | not yet |
| A feeling in body and light | `express` | `express` | `lamp_express` | done (3 feelings) | Nine | not yet |
| Look at face, hand, name, angle | `look` | `look` | `lamp_look` | done for `ahead`, an angle and `hand`; `face` and `name` "not in this release". On a resting lamp a look may end `cut_short` on stable 10824 (fixed on lamp-voice, #5711) | Nine | not yet |
| Follow a hand | `look at hand` | `follow_hand` / `followHand` | `lamp_follow_hand`, `lamp_follow_stop` | done, on stable too | Nine | not yet |
| Each joint (pitch, roll, yaw, shoulder, elbow) | `joints` | `joints` | `lamp_joints` | `yaw` and `pitch` only; `roll`, `shoulder`, `elbow` refused "not routed yet" | Ten | 75c9, 7 Nov, stable 10579: pitch alone went; roll refused (again 9 Nov, 10824) |
| A move of your own | `move` | `move` | `lamp_move` | done | Nine | not yet |
| A recorded move | `play` | `play` | `lamp_play` | in the clients; lamp-control refuses it, "not routed yet: waits for client packages" | Nine | refused as written: 75c9, 9 Nov, 10824 |
| A new motion interrupts the one playing | `gesture`, `move`, `express` replace | (no new call) | — | on lamp-voice (`6167fe37a1`), not on stable 10824, which refuses `busy` with `retry_ms` | Nine | not yet |
| Each joint's degrees, bounds and caps | `get body` | `get("body")` | `lamp_state` | done | Ten | 75c9, 7 Nov, stable 10579: known, each joint's range |
| Where each joint is, live; model and twin | `body.state` | model, JS twin | — | done on lamp-voice (#4564); not on stable 10824 | Eighth | 75c9, 9 Nov, #4564's lamp-control: 12.5 events a second |
| Record a move from poses | — | — | — | missing | Ten | not yet |
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
| Put a skill on the lamp, list, remove | `skill.push`, `skill.list`, `skill.remove` | `push_skill`, `skills`, `remove_skill` / `pushSkill`, `skills`, `removeSkill`; `lamp skill push/list/remove` | `lamp_skill_push`, `lamp_skills`, `lamp_skill_remove` | done; on customer lamps from the next build (the product unit passes `--manage-socket`, #5819); stable 10824's unit doesn't, so it refuses "skill push is not on this lamp" (75c9, 9 Nov) | blue | 75c9, 4 Nov, build 10114, by the local leader: check, push, list, remove; 75c9, 9 Nov, 10824's lamp-control under the next build's unit: push, list, remove, push, revoke turned it off |
| Check a skill folder before pushing | — | `lamp skill check` | — | done | blue | not yet |

## Never readable

The privacy grants stay: who a person is, the words said around the
lamp's own phrases, and the camera and microphone beyond the local network.
The person wins: anything the person does with the lamp comes before a
program's lease.
