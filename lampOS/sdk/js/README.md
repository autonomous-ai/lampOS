# lampos (JavaScript): program a lampOS lamp from your computer

The Node client for the lamp's v1 control protocol
([docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md),
messages in
[control.schema.json](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.schema.json)).
Same names as the Python SDK, in camelCase, with promises. It runs on your
computer, never on a lamp. Node 20.11 or later.

**New here?** [docs/develop.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/develop.md) is the 10-minute quick-start: the fake lamp, a first program, pairing, lamp-mcp and skills.

**On a customer lamp today (stable 10824):** lamp-control is on every lamp,
and answers once the owner turns on developer access in the app. Five gaps,
and what to do ([more](../README.md#on-a-customer-lamp-today-stable-10824)):
- **Skill push** is not yet on customer lamps: every skill op is refused, "skill push is not on this lamp". Try it on `lamp fake`, or run the behaviour as a program on your computer. The Python SDK's `lamp skill check` works anywhere.
- **Joints and play:** `joints` takes `yaw` and `pitch` only; `roll`, `shoulder` and `elbow` are refused, and so is `play`. Use `look`, `gesture` or `move`.
- **Limp and pose:** the body's limp and hold-a-pose can't be reached from a computer.
- **No mDNS:** `lamp find` finds nothing. Pair with `--host lamp-xxxx.local` (the lamp's host name) or its IP address; the address is kept for later connects.
- **Event times are UTC:** `at` ends in `Z`. Convert to local time yourself.

Also: a motion asked while the last one sent through lamp-control plays is refused `busy` with `retryMs` (wait, ask again), and a `look` on a resting lamp may end `cut_short` with no hand on it.

## Changes

- **0.2.0:** `await lamp.get(...)` returns the states asked, by name
  (`{"presence": {...}, "light": {...}}`), as lamp-control's reply carries them
  under `state`; 0.1.0 returned the whole reply. A `light` or `mood` the lamp
  hasn't heard yet is `null`, on the fake lamp too.

## Install

It isn't on npm yet. Clone this repository's `lamp-voice` branch (the
default branch, `main`, has no SDK), then install it in your project:

```sh
git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git /path/to/lampOS
npm i --install-links /path/to/lampOS/lampOS/sdk/js   # a copy, not a link, so Node finds its ws; the clone, then its lampOS/ folder
```

`npm install lampos` from the registry fails: install from the checkout, as
above. Finding by mDNS finds no lamp yet: none announces itself on stable 10824.

## Try it with the fake lamp (no lamp needed)

`lampos fake` (also `lamp fake`) starts a fake lamp on this computer with every grant, pairs this
computer with it at once, and every 10 seconds says a person arrived, the
phrase "good night lamp", and the person left, printing what it is asked.
Keep the trial apart from real pairings with its own `LAMPOS_HOME`
(the fake makes its certificate with `openssl`):

```sh
# terminal 1 (from lampOS/sdk/js, after npm install)
LAMPOS_HOME=/tmp/lampos-try npx --no-install lampos fake
# terminal 2 (the same folder)
LAMPOS_HOME=/tmp/lampos-try node examples/greet.js
```

Terminal 1 then shows `asked {"op":"gesture","name":"greeting"}`. Every
example runs this way: `tour.js` (every kind of call, one step at a time; with `lamp fake --everything` nothing is refused), `greet.js`, `follow.js`, `follow-hand.js` (with `lamp fake --hands`), `phrase.js`, `pomodoro.js`,
`ci-light.js`, `ask-the-person.js`, `camera.js` (a pairing with `camera`: saves `snapshot.jpg` and a stream's `last-frame.jpg`), `curious.js` (a move of your own from poses), and `push-skill.js ../examples/hello-skill` (a pairing with `skills` and `speak`).

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
`express` as the feelings skill's move and light, `say` and `sound`
(`hum`, the lamp's own short hum) with `{"to": "voice"}` and their `done`,
`startTurn` with `{"to": "voice"}` and its `done`, `endTurn` and
`setListening` as lamp-control does (`fake.handOnHead()` lifts a mute),
`look` at `'ahead'`, `'hand'` or a yaw and pitch, and `joints` naming only
`yaw` and `pitch`, as a customer lamp (stable 10824) takes them; and `play`,
and `joints` naming `roll`, `shoulder` or `elbow`, refused (`refused`, "not
routed yet: waits for …"), in lamp-control's own words. The fake takes what a customer lamp
refuses: a skill push, and a motion asked while another plays (stable 10824
answers `busy`). `--everything` answers every op instead.

## See the lamp as it stands: the twin

`examples/twin/` draws the lamp in 3D from its rigged CAD and `model`, moving
with `body.state`, the ring in the light's colour. Dragging a joint's slider
sends `joints` under a body lease, and the lamp follows.

```sh
node examples/twin/server.js --fake   # no lamp: a simulated body
node examples/twin/server.js          # the paired lamp (needs the body grant)
# then open the URL it prints: http://localhost:8737/?t=TOKEN
```

The page and its live link answer only with that token, made fresh at each
start, and the live link only from the page's own origin. Any website open in
the browser can reach localhost, so without them one could drive the lamp.

It needs the CAD from the repository (`git lfs pull`, or `--glb PATH`). Two
limits for now: the lamp sends `body.state` 12.5 times a second and the page
eases between frames, and of `joints` the lamp takes `yaw` and `pitch` only,
refusing `roll`, `shoulder` and `elbow` until the body's own goal stream. A
slider is a joint's degrees from its home, the same `body.state`,
`get('body')` and `joints` use. Its "Calibrate"
panel sets each joint's zero and direction against the real lamp, for
lamp.model.json.

## Pair with a real lamp

Run the command as `npx --no-install lampos …` (or from `node_modules/.bin`),
in a project with `lampos` installed: npm has an unrelated package called
`lamp`, and a plain `npx lamp` outside such a project would offer to install
it.

```sh
npx --no-install lampos pair 12345678 --pin 3f9a… --host lamp-xxxx.local   # the code and the lamp's pin (64 hex), both shown in the app
```

No lamp announces itself by mDNS yet (stable 10824), so give `--host`: the
lamp's host name, `lamp-xxxx.local`, or its IP address. Without it, `lamp
pair` finds no lamp. The address is kept with the pairing, so
`Lamp.connect()` needs nothing more; if it changes, pass `{ host }`.

The lamp's certificate is checked against the pin before any frame is sent
(only the TLS handshake and the WebSocket's upgrade request, which carry no
secret, go first), so the code's proof never reaches another host, and the pin is inside the
proof too. There is no pairing without it. The pairing is kept in
`~/.config/lampos/` (or `$LAMPOS_HOME`, which is made private too, so point it at a folder of its own), the same folder as the Python
SDK's, so a computer paired by one is paired for both: the folder is yours
alone (0700), the key and `lamps.json` 0600, and the pin is checked again on
every connection.

## Program it

```js
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
await lamp.on('person.arrived', () => lamp.gesture('greeting'));
await lamp.closed();
```

- **Events:** `await lamp.on(name, handler)` resolves once subscribed, and
  rejects with the lamp's refusal, `NoGrantError` without the event's grant:
  `person.*` needs `look`, `phrase.heard` needs `hear`. `lamp.off(name,
  handler)`. Each needs its grant:

  | event | fields | grant |
  |---|---|---|
  | `person.arrived`, `person.left` | `count`, `near` (0–1, share of the picture's height) | `look` |
  | `person.stood_up` | `side` | `look` |
  | `person.facing` | `facing` | `look` |
  | `hand` | `seen`; when seen, `x`, `y` (the palm's centre, 0–1 from the left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. By edge, at most 4 a second; `seen: false` once, 1 s after the last hand | `look` |
  | `phrase.heard` | `phrase`: one you registered (`on(..., { phrases })`), never what else was said | `hear` |
  | `name.heard` | `score`: how sure the listener was, 0–1 to a hundredth (real calls 0.90 to 1.00; a sound like the name has reached 0.94–0.98, so a program acting on its own may want 0.99) | `hear` |
  | `sound.heard` | `class`, `score` | `sound` |
  | `touch` | `kind` (`tap`, `pet`, `hold`), `where` | none |
  | `button` | `press` (`press`, `long_press`) | none |
  | `time` | your schedule | none |
  | `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `lease`, and `quiet_hours` (planned: not produced yet; on a lamp that sets quiet hours, off by default, the door refuses `say` and `sound` in them, or while the lamp's local time isn't known, with the error `quiet_hours`) | none |
  | `volume.changed` | `level` (0–100), `muted`: the speaker's volume changed, whoever changed it (the person by voice or in the app, or a computer) | `speak` |
  | `body.state` | `joints` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`: each `deg`, `counts`, `goal_deg`, `load`, `moving`, `temp_c`) and `lamp` (`light`, `talking`, `private`), at most 25 a second (12.5 today) | `body` |

- **State:** `await lamp.get('presence', 'lamp')` resolves with the states
  asked, by those names, as lamp-control answers them: `{ presence: {...},
  lamp: {...} }`. Behaviour change: it used to resolve with the whole reply.
  `light` is the person's own (`on`, `colour`, `level`) with `shows`, what the
  ring draws now; the fake's `shows` is the newest light asked while it lasts.
- **Actions:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`, and the turn and mute calls `startTurn`,
  `endTurn`, `setListening`. Each resolves when the lamp has taken it;
  `{ wait: true }` on a gesture, move or play resolves when it has finished,
  or, where the lamp hands it on and says no more (today `gesture` and
  `move`, answered `{"to": "body"}`), at that answer.
- **A new motion cuts the one playing short:** a gesture, move or feeling's
  move asked while another plays (whoever asked it) replaces it, eased out
  from where the body is under its own caps. While the person is using the
  lamp (a conversation, a hand on it), the new one is dropped by the body
  ("the person is using the lamp"); the door has already answered
  `{"to":"body"}`. **Stable 10824 and older** refuse it instead: `busy`,
  "busy: a motion plays for N ms more", with `retryMs`. Wait `retryMs`
  and ask again.
- **Closing gives your light back at once.** When a computer's last
  connection closes, the lamp gives back the light it asked for and stops
  each `say` of yours, waiting or playing, as `stop` does: its `done` comes
  `dropped`, "dropped: stopped by its asker". A script that lights the ring and exits shows it for a
  moment only: stay connected while it should show (`examples/ci-light.js`).
- **Leases:** `await lamp.withLease(['body'], async (lease) => ...)` holds
  the body for looks, moves and joints, renewed for you, until the callback
  ends or the lamp takes it back: `lease.state`, `lease.lostWhy`.
- **Following a hand:** `const following = await lamp.followHand().start()`
  (or `lamp.withHand(async (following) => ...)`) has the body follow the
  hand it sees, under a body lease of its own, waiting while the hand is
  gone, until `following.stop()`, `lamp.stop()`, another `lamp.look(...)`,
  or the lamp takes the body back. lamp-control answers it `{"to":"body",
  "follow":"hand"}` with no `done`, so `following.wait()` is told here:
  `how` and `why`. Needs `body` and `look`; each start is a look. See
  `examples/follow-hand.js`.
  A lamp that takes the look without answering `follow: "hand"` (a
  lamp-control from before #4183) is refused, "this lamp doesn't follow a
  hand yet", and the lease let go.
- **Refusals** reject with typed errors: `byCode.rate` (with `retryMs`),
  `byCode.no_grant`, `byCode.private`, ...
- **Synthetic hands:** `handTrack(path, { seconds, hz, seed })` from
  `lampos/testing` makes a track in `humans/hands/tracked`'s shape (`t` in
  µs), `handEvents(track)` the `hand` events lamp-control's watch.rs tells
  for it (by change, at most four a second, rounded to 2 places,
  `seen: false` 1 s after the last), and `fake.emitHandTrack(track, {
  realtime })` says them. `lamp fake --hands` waves a hand for 3 s after
  each arrival and answers a look at it.
- **In your tests:** `FakeLamp` from `lampos/testing` is the same fake lamp:
  `emit`, `refuseNext`, `done`, `leaseEvent`, `requests`, `received`. Its
  `light` keeps lamp-control's pattern levels (a `breathe` at level 1 is refused)
  and answers `{"to":"ring"}`. `pulsesDropped` lists a second pulse within a
  minute, which the ring shows steady, without its pulse (and says nothing).
  A `move` it takes is one lamp-control takes: steps of 40 to 1,000 ms, 1.5 to
  10 s in all, from home and back, each joint within the body's reach and speed
  (`moveRefusal`, its words).

**When the lamp restarts** (every update), the connection closes and
`lamp.closed()` resolves. A long-running program runs under
`Lamp.keepConnected(setup)` instead: it connects, runs `await setup(lamp)`
(subscribe, take leases), waits for the connection to end, then connects
again and runs `setup` again, pausing 1, 2, 5, then 10 s while the lamp
can't be reached. It rejects at once on what connecting again can't mend (no
pairing, another lamp at the address, a revoked computer, a refused
subscription such as `no_grant`) and on anything `setup` throws; pass an
`AbortSignal` as `signal` to stop it.

```js
await Lamp.keepConnected(async (lamp) => {
  await lamp.on('person.arrived', greet);
});
```

`fake.restart()` closes every connection as a lamp's restart does, to test
it.

**Seeing every frame:** with `LAMPOS_LOG=debug`, every frame to and from the
lamp is printed to stderr, one line each (`lampos -> {...}` sent, `lampos <-
{...}` received). It's off by default. Nothing that would pair or say hello as
this computer, or give away the lamp's pin, is printed: `proof`, `sig`, `key`,
`nonce`, `pin` and `fp` show as `<redacted>`, and so does a pair request's
`code`. A said `text` shows only as its
length and a tag keyed for this run (an HMAC: the same text tags the same within
a run, and nobody can test a guess against it), and a pushed skill part's
`data` only as its length. The fields the protocol has are printed as they
are: a registered phrase, events' fields and a refusal's `why`. A field the SDK
doesn't know, such as one from a newer lamp, is printed as `<not shown>`.

**Dependencies:** `ws` and the optional `bonjour-service` are caret ranges
with no lock file, as a library's are: the application that uses it locks
them.

Tests: `npm test`.

## Push a skill to your own lamp

**Not yet on customer lamps** (stable 10824): there every skill op is
refused, "skill push is not on this lamp". Try it on `lamp fake`.

With the `skills` grant, which the owner ticks at pairing, a computer can put
a skill of its own on its own lamp, with no Store
([control.md](../../docs/control.md), "Skills pushed from a computer"):
`await lamp.pushSkill(folder)` packs the folder and sends it in parts, and
resolves with the lamp's answer (on, or held for the owner's yes);
`await lamp.skills()` and `await lamp.removeSkill(name)`; on the command line,
`lamp skill push FOLDER`, `lamp skill list` and `lamp skill remove NAME`. It refuses a
folder without a `version`, or a `core` or add-on skill. The fuller check is
the Python SDK's `lamp skill check`, and the lamp checks everything again.

`../examples/hello-skill` is a small skill to try it with: one short hello,
asking only `speak`. What a push needs:
- **The grants:** `skills`, and every grant the skill's `[permissions]` asks
  (`hello-skill` asks `speak`).
- **The size:** a pushed skill is 1 MiB at most, packed, sent in parts of
  36 KiB. A skill with a recording or other large files is too big; so is a
  source template whose files must be rendered first.
- **The lamp:** lamp-control started with `--manage-socket`, its link to the
  root helper that installs skills ([control.md](../../docs/control.md),
  piece 7). Without it the lamp answers `refused`, "skill push is not on this
  lamp". The product unit on stable 10824 doesn't pass it; the next build's
  does.

## Everything you can read

What the lamp tells a paired computer, read from lamp-control's `watch.rs`,
[control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md) and the schema. Every event comes **by
edge**: when something changes, never once per sensor reading. Each event
carries `at`, a UTC time ending `Z`, besides the fields below. Subscribe with
`on(event, …)`. An event your pairing lacks the grant for is refused at
subscribe.

### Events

| event | fields | example | fires when | grant |
|---|---|---|---|---|
| `person.arrived` | `count`: faces in view now. `near`: the nearest face's height, as a share of the picture (0–1, 2 places) | `{"ev":"person.arrived","count":1,"near":0.31}` | a face is seen while the lamp had nobody: at its start, or after `person.left` (nobody for 5 s). A face gone for less than that fires nothing | `look` |
| `person.left` | `count`: 0 | `{"ev":"person.left","count":0}` | nobody has been in view for 5 s, after someone was | `look` |
| `person.facing` | `facing`: true or false | `{"ev":"person.facing","facing":true}` | someone turns to the lamp, or the last one turns away | `look` |
| `person.stood_up` | `side`: `left`, `right` or `ahead`, when the room's watcher says | `{"ev":"person.stood_up","side":"left"}` | someone in view stands up | `look` |
| `phrase.heard` | `phrase`: one of the phrases you registered | `{"ev":"phrase.heard","phrase":"good night lamp"}` | a finished line the lamp heard holds your phrase as whole words, ignoring case and punctuation. Only the phrase leaves the lamp | `hear` |
| `name.heard` | `score` | `{"ev":"name.heard","score":0.97}` | someone says the lamp's name, or a sound like it | `hear` |
| `sound.heard` | `class`: `music`, `laughter`, `door` or `footsteps`. `score`: 0.5–1, 2 places | `{"ev":"sound.heard","class":"door","score":0.72}` | a class's score rises past 0.5 (again only after it falls under). Never speech | `sound` |
| `touch` | `kind`: `tap` (two or more taps are one tap) or `pet`. The schema also has `hold` and `where`, not sent yet | `{"ev":"touch","kind":"pet"}` | a tap or a pet on the lamp | none |
| `button` | `press`: `long_press`. The schema also has `press`, not sent yet | `{"ev":"button","press":"long_press"}` | the button is held | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep` (each true or false). `quiet_hours` is planned, not sent yet | `{"ev":"lamp.changed","lamp_state":{"talking":true,"paused":false,"private":false,"asleep":false}}` | any of the four changes | none |
| `volume.changed` | `level`: 0–100 of what the lamp allows. `muted`: true at 0 | `{"ev":"volume.changed","level":30,"muted":false}` | the volume changes, whoever changed it: the person by voice or in the app, or a computer | `speak` |
| `time` | your schedule | | accepted in a subscription, not produced yet | none |
| `hand` | `seen`. When seen: `x`, `y` (the palm's centre, 0–1 from the picture's left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. All to 2 places, never the tracker's clock | `{"ev":"hand","seen":true,"x":0.42,"y":0.55,"size":0.12,"open":true,"confidence":0.91}` and `{"ev":"hand","seen":false}` | a hand is first seen, moves or grows by 0.05, opens or closes, at most 4 a second (the newest kept). `seen: false` once, 1 s after the last. Under 0.5 confidence is no hand | `look` |
| `body.state` | `joints`: each of `pitch`, `roll`, `yaw`, `shoulder`, `elbow` the body read, with `deg` (degrees from its home, as `get('body')` counts them, to a tenth; `null` until the body has said its limits), `counts` (0–4095), `goal_deg`, `load` (0–1), `moving` (5°/s or more) and `temp_c` (from the body's once-a-second report). `lamp`: `light`, `talking`, `private` | `{"ev":"body.state","joints":{"yaw":{"deg":-25.8,"counts":1722,"goal_deg":-22.9,"load":0.1,"moving":true,"temp_c":41}},"lamp":{"light":{"on":true,"colour":"warm","level":3},"talking":false,"private":false}}` | each `body/joint_states` the body sends, at most 25 a second: 12.5 today | `body` |
| `air.changed` | the air sensor's reading: `pm1_0`, `pm2_5`, `pm4_0`, `pm10` (µg/m³), `co2_ppm`, `humidity` (%), `temperature` (°C, at the sensor inside the base, so warmer than the room), each to a tenth (carbon dioxide whole) and left out while the sensor doesn't know it; `feel`: `fresh`, `fine`, `stuffy`, `very_stuffy` or `unknown`, from the carbon dioxide | `{"ev":"air.changed","pm2_5":7.4,"co2_ppm":912,"humidity":44.2,"temperature":28.3,"feel":"fine"}` | the first reading, then a value moves by its step (particles 5, carbon dioxide 50 ppm, humidity 3, temperature 0.5 °C), comes or goes, or `feel` changes; at most once every 5 s, the newest kept | `air` |

And two that come by themselves, with no subscription:

| event | fields | example | fires when |
|---|---|---|---|
| `lease` | `lease`, `state` (`held`, `suspended`, `lost`), `why` (`conversation`, `hand`, `stop_heard`, `private`, `paused`, `asleep`, `expired`, `taken_by_owner_app`, `revoked`, `released`, `slumped`) | `{"ev":"lease","lease":"l_3","state":"suspended","why":"conversation"}` | one of your leases changes: talking suspends it and resting holds it again; a hand on the body, a stop, a pause, privacy or sleep loses it |
| `done` | `re` (your request's id), `how` (`finished`, `cut_short`, `dropped`), `why` | `{"ev":"done","re":14,"how":"finished"}` | an action that ends later has ended. Today that's `say`, `sound`, `startTurn`, a `look`, and a mute `setListening(false)` the lamp lifted (`cut_short`, "a hand on the head") |

### States, with `get`

| state | fields | example | grant |
|---|---|---|---|
| `presence` | `count`, `near` (0–1), `facing`. Counts and how near, never who | `{"count":1,"near":0.31,"facing":true}` | `look` |
| `light` | the person's own light: `on`, `colour`, `level`. Also `shows`: what the ring draws now, `{on, colour, level}`. `null` until the lamp has said either | `{"on":true,"colour":"warm","level":3,"shows":{"on":true,"colour":"green","level":2}}` | none |
| `mood` | `energy`, `warmth`, `awake`, `asleep`, the lamp's inner state. `null` until known | `{"energy":0.6,"warmth":0.7,"awake":true,"asleep":false}` | none |
| `air` | the newest reading, as `air.changed` gives it, and `known: true`; `{"known": false}` with no sensor, its part not running, or no reading in 15 s | `{"pm2_5":7.4,"co2_ppm":912,"feel":"fine","known":true}` | `air` |
| `device` | the lamp's own computer: `cpu_load` (the last minute's, 1 is every processor busy), `memory_used` and `disk_free` (0–1), `uptime_s`, `temperature_c` (by sensor, °C), and `known: true`; `{"known": false}` with no reading in 15 s | `{"cpu_load":0.42,"memory_used":0.55,"disk_free":0.71,"uptime_s":3600,"temperature_c":{"cpu_thermal":61.5},"known":true}` | `device` |
| `lamp` | `talking`, `paused`, `private`, `asleep` | `{"talking":false,"paused":false,"private":false,"asleep":false}` | none |
| `volume` | `level` (0–100 of what the lamp allows), `muted`. `null` until lamp-actions has said it | `{"level":50,"muted":false}` | `speak` |
| `body` | `known`; `joints` (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`), each with `counts`, `deg` from its home, `min_deg`, `max_deg`, `min_counts`, `max_counts`, `home_counts`, `folded_counts`, `stand_counts`; `stands`, `speed_deg_s`, `acceleration_deg_s2`, `load_limit`, `heat_stop_c`, `counts_per_turn` | `{"known":true,"joints":{"yaw":{"counts":2015.0,"deg":0.0,"min_deg":-3.5,"max_deg":3.5,...},...},"stands":true,"speed_deg_s":4.4,...}` | body |

### The lamp's model

`model` (`import { model } from 'lampos'`) is the lamp's kinematic model, the package's copy of
[lamp.model.json](../../docs/lamp.model.json), written from the rigged CAD
(`robots/lamp/hardware/cad/glb/lamp.glb`) by `tools/lamp-model.js`.
It needs no lamp. It gives:

- `chain`: the joints from the base out, under the `joints` op's names.
- Each joint's `bone` in lamp.glb, `parent`, `origin_m`, `rest_quat`, `axis`,
  `reach_deg` around the lamp's home, and its servo's number and bus name.
- `measured`: what is still the CAD's. The origins are, and each joint's
  `zero_deg` and `sign`, the joint's `deg` in `body.state` (from its home) at the CAD's rest
  pose, are too, until measured on a lamp.

`hardware` (`import { hardware } from 'lampos'`) is the lamp's parts as data ([lamp.hardware.json](../../docs/lamp.hardware.json), [hardware.md](../../docs/hardware.md)): the board, servos, camera, microphones, speaker, light, touch, buttons, air sensor and power, each with where its facts come from.

With `body.state` it draws the lamp as it stands: turn each bone of lamp.glb
about its `axis` by `sign * (deg - zero_deg)` after its `rest_quat`.

### Never readable

Through this door, by any grant: transcripts or the words around a phrase,
face prints, and anyone's name. The camera's pictures come only with the
`camera` grant, on the lamp's own network and never while it is private
(`examples/camera.js`); the `microphone` grant is not routed yet, so no audio
leaves. With `look` alone, a hand's or a face's place in the picture is a
share of it, never the picture.

## Reference

Every public call and `lamp` command, with what the lamp takes. The ranges are
what lamp-control and the ring refuse past, read from
[control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md) and their code. The SDK checks names
against the schema before sending, and the lamp checks the rest. A test keeps
every public method named here.

**Today a lamp (stable 10824) refuses** `look` at `'face'` or `'name'` ("not
in this release"), `joints` but for `yaw` and `pitch`, and `play`, with
`refused`, "not routed yet: waits for …"; every skill op with `refused`,
"skill push is not on this lamp"; a gesture, move or feeling's move asked
while another plays with `busy` and `retryMs`; and `say` and `sound` in its quiet
hours when it sets them (off by default; a lamp sets its own window, said in
`lamp.limits.quiet_hours`), or while its time isn't known, with `quiet_hours`.
`LAMP_CONTROL_REFUSES` in `lampos/testing` is what `lamp fake` refuses, as a
stable lamp does: `play` (`FakeLamp.start({ refuses: LAMP_CONTROL_REFUSES })`).
Every fake refuses `joints` naming `roll`, `shoulder` or `elbow` with
`JOINTS_WAIT`, lamp-control's words.

### Connecting

| call | parameters | resolves with |
|---|---|---|
| `Lamp.pair(code, host, { pin, port = 7731, name, keyring })` | `code`: the owner's 8-digit pairing code. `host`: the lamp's address. `pin`: the certificate's SHA-256 the app shows, 64 hex. `port`: 7731. `name`: this computer's name in the app, 1–64 characters, default the host name. `keyring`: default `new Keyring()` (`LAMPOS_HOME`, else `~/.config/lampos`) | the pairing (`lamp`, `client`, `grants`, …) |
| `Lamp.keepConnected(setup, { lamp, host, keyring, pauses, signal })` | `setup(lamp)`: called (and awaited) on each new connection. `pauses`: seconds before connecting again (`[1, 2, 5, 10]`), longer while the lamp can't be reached. `signal`: an `AbortSignal` that stops it | resolves only once `signal` aborts; rejects with `NotPaired`, `WrongLamp`, a hello's or subscription's refusal, or what `setup` threw. "When the lamp restarts" |
| `Lamp.connect(lamp, { host, keyring })` | `lamp`: a paired lamp's id, or `undefined` for the only one paired. `host`: an address to use instead of the kept one | a `Lamp`, after hello |
| `lamp.info`, `lamp.grants`, `lamp.limits` | | the lamp's id and name, this computer's grants, the lamp's limits: each rate as `{most, within_ms}` windows (`joints`, `motion`, `light`, `say`, `express`, …), `hold_max_ms` (a light's longest `for_ms`, 30000), `lease_max_ms` (the longest one lease take or renewal asks for, 600000), `say_max_bytes` (the longest say's text in UTF-8 bytes, 900) and `quiet_hours` (`{from, to}`, or `off`, the default). There is no ceiling on renewing a lease, and a say is counted in bytes (older lamps also sent `take_max_ms` and `say_max_chars`). By default the rates are off: `rates: "off"`, and only the `refused` and `phrases` windows (a lamp run with `--rates default` gives every class). The fake lamp sends the same as a lamp with its rates off |
| `lamp.close()` | | lets go of every lease, then closes |
| `lamp.closed()` | | resolves when the connection ends |
| `find(ms = 3000)` | `ms`: how long to listen for mDNS | the lamps on this network (`id`, `name`, `host`, `port`, `fp`). Finds none today: no lamp announces itself (stable 10824) |

### Events and state

| call | parameters | grant |
|---|---|---|
| `lamp.on(event, handler, { phrases })` | `event`: one of the schema's events (`person.arrived`, `person.left`, `person.stood_up`, `person.facing`, `hand`, `phrase.heard`, `name.heard`, `sound.heard`, `touch`, `button`, `time`, `lamp.changed`, `volume.changed`, `body.state`; their fields and grants are [control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md)'s Events), or `lease` or `done`. `handler(event)`: a function, or async. `phrases`: with `phrase.heard` only, up to 16 for the computer, each of 64 characters at most and 8 letters at least. Changing the phrases is 6 a minute | the event's. Rejects with the lamp's refusal |
| `lamp.off(event, handler)` | as `on` | unsubscribes once the event's last handler is gone |
| `lamp.subscribe(events, phrases)`, `lamp.unsubscribe(events)` | an array of events | each event's |
| `lamp.get(...states)` | `presence` (needs `look`), `light`, `mood`, `lamp`, `air` (needs `air`), `device` (needs `device`), `body` (needs `body`) | the states asked, by those names |
| `lamp.air()` | | the `air` grant: the air sensor's newest reading (`pm1_0`, `pm2_5`, `pm4_0`, `pm10`, `co2_ppm`, `humidity`, `temperature`, `feel`) and `known`; only `{ known: false }` with no reading. Changes come as `air.changed` |
| `lamp.device()` | | the `device` grant: the lamp's own computer (`cpu_load`, `memory_used`, `disk_free`, `uptime_s`, `temperature_c`) and `known` |
| `lamp.bodyInfo()` | none; what the body may do and where it is: each joint (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`) with `deg` from its home, `min_deg`, `max_deg`, its counts and poses; the speed, load and heat limits | `body` | no | `known: false` until the body has said its limits |

### Leases

| call | parameters | |
|---|---|---|
| `lamp.withLease(parts, body, { forMs = 30_000, why })` | `parts`: `body` (grant `body`), `light` (`light`), `voice` (`speak`). `forMs`: 1–600,000; renewed in time, a lease has no ceiling. `why`: up to 200 characters, shown to the owner. `body(lease)`: async | takes the lease, renews it at half its time, and lets go when `body` ends |
| `lamp.lease(...parts)`, then `await lease.take()` and `await lease.release()` | as above, for 30 s | `lease.id`, `lease.state` (`held`, `suspended`, `lost`, `released`), `lease.held`, `lease.lostWhy` |

### Actions

Each resolves with the lamp's answer when the action is taken. `{ wait: true }`
resolves with its `done` instead, or with the answer where no `done` follows
(`gesture`, `move`).

| call | parameters | grant | lease |
|---|---|---|---|
| `lamp.express(feeling, strength)` | `feeling`: `glad`, `lively` or `low`. `strength`: 0–1, default 0.5 | `body` and `light` | no |
| `lamp.gesture(name, { wait })` | `name`: `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`, the clips offered to programs; the lamp takes `idle` and `scanning` too, and any other lowercase name the body answers (rule 8: keep `scanning` for when a person asks) | `body` | no |
| `lamp.look(at, { wait })` | `at`: `'face'` (needs `look` too), `'hand'` (needs `look` too: the body follows the hand in view until another look, a `stop` or the lease ends; answered `{"to":"body","follow":"hand"}`, no `done`; needs the body's lease socket on the lamp), `'name'`, `'ahead'`, or `{ yaw, pitch }` in degrees, right and up positive, any finite number: the body clamps them to its reach. On stable 10824, `'face'` and `'name'` are refused, "not in this release", and a look on a resting lamp may end `cut_short`, "not there in time: something holds the head", with no hand on it: treat one close to the target as arrived, or ask again. With `{ wait: true }`, the body's `done` instead of the door's answer: `finished` with `yaw`, `pitch` and `clamped`, or `dropped` with why (`the body is not held` when a conversation took the body first). A wait on `look('hand')` is a `bad_request`: a hand-follow has no `done`. | `body` | `body` |
| `lamp.followHand({ forMs, why })`, `lamp.withHand(body, options)` | `await lamp.followHand().start()` then `await following.stop()`, or `lamp.withHand(async (following) => ...)`: a `look` at `'hand'` under a body lease of its own (`forMs`, default 10,000, renewed), until `stop()`, another look, or the lamp takes the body back. `following.ended` says how: `finished`/`stopped`, or `cut_short` with the why. `lamp.stop()` lets go of its lease too. After another look the lease stays held (that look runs under it), and the lamp's own life stands back until `stop()`; a new `followHand()` lets go of it first. A lamp that takes the look without following (before #4183) is `refused`, and the lease let go. Where the hand is comes as `hand` events | `body`, `look` | its own |
| `lamp.move(frames, stepMs, { wait })` | `frames`: 2 frames at least (at most what 10 s allows: 251 at 40 ms) of 5 joints in counts from home, the first and last all 0. `stepMs`: 40–1,000, and 1.5 to 10 s in all. Each joint within the body's reach and speed, else `bad_request` in the body's words | `body` | `body` |
| `lamp.movePoses(poses, { holdMs, wait })` | A `move` through `poses`, each an object of the joints it turns in degrees from the standing pose (`pitch` and `shoulder`/`elbow` up, `yaw` right, `roll`; one left out stays put), eased there and back at 80% of the body's speeds, each pose held `holdMs` (400); at least 1.5 s, at most 10 s. A `LampError` (`bad_request`) before sending for an unknown joint, one farther than a move takes it (pitch and roll 17.6°, yaw 13.2°, shoulder and elbow 30.8°) or poses longer than 10 s. `posesToFrames(poses, { stepMs, holdMs })` gives the frames alone | `body` | `body` |
| `lamp.play(name, { wait })` | `name`: a move of your installed package. Refused on every lamp today, "not routed yet: waits for client packages" | `body` | `body` |
| `lamp.joints(lease, withinMs, targets, { wait })` | `lease`: your held `body` lease. `withinMs`: 0 or more (the body's caps pace each step). `targets`: `{ pitch, roll, yaw, shoulder, elbow }` in degrees, −180 to 180, at least one. The head's `yaw` and `pitch` go as a look; `roll`, `shoulder` and `elbow` as the body's pose, taken only while the lamp stands (else `done` `dropped`, with why). Naming both: one `done`, `finished` only when both are. On stable 10824 only `yaw` and `pitch` go; `roll`, `shoulder` and `elbow` are refused, "not routed yet: …". With the body silent for 15 s, name both yaw and pitch. With `{ wait: true }`, the body's `done`, as `look`'s. | `body` | `body` (named) |
| `lamp.light(colour, level, forMs, pattern)` | `colour`: `yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`. `level`: 1–5. `forMs`: 1–30,000. `pattern`: none (steady), `'breathe'` (level 2 or up), `'candle'` (3 or up) or `'pulse'` (2 or up). Under its level a pattern is `refused`. The ring plays one pulse a minute for a computer: a second within the minute shows its colour and level steady, without its pulse. A light never blinks | `light` | no |
| `lamp.ring(pixels, { forMs, brightness, fadeMs })` | `pixels`: one colour per pixel, from pixel 0 around the ring, each `'#rrggbb'` or `[r, g, b]` 0–255, exactly `lamp.ringPixels` of them (hello's `limits.ring.pixels`, 32 on today's ring). `brightness`: 0–100, 100 by default, under the ring's own ceiling (lower at night). `forMs`: up to 30,000. `fadeMs`: 1,000–60,000 | `light` | no | as `light` |
| `lamp.lightRgb(rgb, { forMs, brightness, fadeMs })` | one colour of your own for the whole ring, `'#rrggbb'` or `[r, g, b]` | `light` | no | as `light` |
| `lamp.ringPixels` | how many pixels the ring has, from hello; `undefined` from a lamp before pixel lights | | | |
| `lamp.say(text, { rephrase })` | `text`: words, at most 900 UTF-8 bytes (about 900 Latin letters, 300 in a three-byte script), the voice's own cap, checked here before sending as the lamp checks it, never stricter; spoken **exactly as written** through the lamp's speech service (no model, no tools) once the person isn't talking. `{ rephrase: true }`: the lamp's model says it in its own words instead, as before. Since #4958 `say` is exact by default; before it, every say was the model's version, which `{ rephrase: true }` keeps | `speak` | no | 6 a minute, 60 an hour with the rates on; refused in quiet hours where the lamp sets them |
| `lamp.cameraSnapshot()` | none; resolves to the camera's newest picture, a JPEG `Buffer` about 320 across. Refused while the lamp is private, or with the camera closed | `camera` | no |
| `lamp.cameraStream(on = true, fps, size)` | `fps`: 1 to 30 a second at most (2); `size`: `640x480` (the default) or `1280x720`. Where the eyes serve every frame (the answer's `fast: true`; not on customer lamps, stable 10824), each `camera.frame` comes as the camera made it, with the JPEG as `data` (a Buffer); otherwise the eyes' newest frame, about 1 to 2 a second, with `jpeg` in base64. `fps_now` is the rate lately; each new frame once, until `cameraStream(false)`, `stop` or a `camera.ended` event. Ends with `camera.ended` when the lamp turns private, or after 10 s with the camera closed | `camera` | no |
| `lamp.sound(name)` | `name`: `hum`, the lamp's own short hum, or one of your clips (`uploadSound`), a clip plays once the person isn't talking, as an exact `say` does; the hum is rarer: its `done` says `dropped` within 15 s of the lamp's last turn ("the lamp spoke a moment ago"), or with another sound in the last 30 s | `speak` | no |
| `lamp.uploadSound(name, wav)` | `name`: a lowercase letter, then lowercase letters, digits, `_` or `-`, up to 64, never `hum`. `wav`: a Buffer, a Uint8Array or a path, a WAV of 16-bit mono PCM at 24 kHz, up to 30 s, no sample past −12 dBFS and no 10 ms past −30 dBFS RMS (the lamp's speaker cap). Sent in 36 KiB parts and checked whole on the lamp, over a clip of the same name. Resolves to `{name, bytes, ms}`. Each computer keeps up to 64 clips and 20 MB (`limits.sounds`); revoking it deletes them | `speak` | no |
| `lamp.sounds()` | your clips on the lamp, each `{name, bytes, ms}` | `speak` | no |
| `lamp.removeSound(name)` | deletes one of your clips | `speak` | no |
| `lamp.volume({ to, step, mute })` | exactly one of `to`: 0–100 of what the lamp allows (0 is muted), `step`: `'up'` or `'down'` (the voice's own step, within the lamp's floor and ceiling), or `mute`: `true` or `false` (an unmute brings it back to where it was). Resolves to `{ level, muted }`, the level it left. The person's own volume, the same the voice and the app set | `speak` | no |
| `lamp.stop()` | | none | no |
| `lamp.startTurn(prompt, { wait })` | `prompt`: words said first, as `say`'s `text` (`undefined`: none). Opens a conversation turn as a tap on the head does: the person's next line is answered as spoken to the lamp. Resolves `{"to": "voice"}`; with `wait`, its `done`: `finished` once the answer has sounded, else `dropped` with the voice's why (`dropped: nobody spoke`). Refused in quiet hours where the lamp sets them, as `say` | `speak` and `hear` | no |
| `lamp.endTurn()` | Ends the turn this computer started, never another's: `{"turn": "ended", "started": <the turn.start's id>}`, and the start's `done` follows. With none, `busy` ("not this computer's turn") | `speak` | no |
| `lamp.setListening(on)` | `false` holds this computer's mute: `{"mute": "held"}` (again: the same, nothing new). `true` lifts it: `{"mute": "lifted"}`, or `{"mute": "nothing_held"}`. The person wins: a hand on the head lifts it, said as a `done` `cut_short`, "a hand on the head", naming the `listen.set` that held it; and it ends when this computer's last connection closes or it is revoked. A `stop` doesn't lift it. Nothing the lamp hears reaches a computer either way | `hear` | no |
| `lamp.request(op, fields)` | any op of the schema, checked against it first | the op's | |

**A look's or a `joints`' `done` on a resting lamp:** a resting head or base
can stop a little short of where it was sent, against its own friction or
weight. Stopped within 24 counts (about 2 degrees), the `done` is `finished`
with `yaw` and `pitch` where the joints stopped, and `clamped: true` when that
is more than half a degree from what was asked; stopped farther short, it is
`cut_short` after 5 s, saying how many counts short. Read the angles from the
`done`, not from what you asked.

A lamp has no rate limits, except two guards: refused actions, 20 a second (past it the connection closes), and changes of the phrases a connection listens for, 6 a minute. Hello's `limits` says `"rates": "off"` with those two as `refused` and `phrases`. `say`, `sound` and `turn.start` are still refused in quiet hours where this lamp sets them (off by default).

**Refusals** reject with `byCode[code]`, a `LampError` with `code`, `why`
(the lamp's own words), for `rate` `retryMs`, and `whatToDo`, the line below
for its code (`WHAT_TO_DO`):

| code | what to do |
|---|---|
| `unpaired` | Pair again with a new code from the owner's app (`lamp pair CODE --pin PIN`): this computer isn't paired, was revoked, or its key no longer matches. |
| `no_hello` | Say hello first: `Lamp.connect()` does; with `request()` alone, send `hello` before anything else. |
| `no_grant` | Ask the owner to tick this grant for this computer in the app, then connect again: grants are read at hello. |
| `no_lease` | Hold a `body` lease first (Python `lamp.lease("body")`, JS `lamp.withLease(['body'], ...)`); `joints` names it. |
| `lease_lost` | The lamp took the body back; `why` says why (a conversation, a hand, private, a stop). Let it be, and take a new lease later. |
| `busy` | Someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first. |
| `refused` | The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again. |
| `quiet_hours` | It's the lamp's quiet hours (`limits.quiet_hours`, off unless this lamp sets them): `say`, `sound` and `turn.start` wait until they end. A light is still fine. |
| `private` | The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false. |
| `paused` | The person paused the lamp. Wait for `lamp.changed` with `paused` false. |
| `asleep` | The lamp is asleep. Wait for `lamp.changed` with `asleep` false. |
| `rate` | Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded. |
| `bad_request` | The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json). |
| `off` | Developer access is off on this lamp: the owner turns it on in the app. |

On stable 10824 and older, `busy` with `retryMs` also means a motion still
plays (the last gesture, move or feeling's move lamp-control sent, from any computer; the lamp's own motions don't count): wait `retryMs`
and send it again. Without `retryMs`, it is a held part, as above.

### Skills

| call | parameters | grant |
|---|---|---|
| `lamp.pushSkill(folder)` | not yet on customer lamps (stable 10824: refused). A skill folder (`SKILL.md`, `skill.toml` with a `version`): packed and sent in parts. The fuller check is the Python SDK's `lamp skill check` | `skills` |
| `lamp.skills()` | | `skills` |
| `lamp.removeSkill(name)` | one of this computer's own pushed skills | `skills` |

### The `lamp` command

| command | options |
|---|---|
| `lamp find` | none: lamps on this network by mDNS. Finds none today: no lamp announces itself (stable 10824) |
| `lamp pair CODE --pin PIN [--host HOST] [--port 7731] [--name NAME]` | `CODE`: the 8-digit code. `--pin`: the 64-hex pin the app shows (required). `--host`: the address, `lamp-xxxx.local` or an IP; give it, as no lamp announces itself by mDNS yet (stable 10824). Without it, found by mDNS by the pin's first 16 hex. `--port`: 7731. `--name`: this computer's name in the app |
| `lamp hello [LAMP_ID]` | a paired lamp's id, or the only one: connects and shows its grants |
| `lamp skill push FOLDER [LAMP_ID]` | a skill folder, put on the paired lamp (`skills` grant). Push, list and remove are not yet on customer lamps (stable 10824) |
| `lamp skill list [LAMP_ID]` | this computer's own pushed skills |
| `lamp skill remove NAME [LAMP_ID]` | takes one of them off the lamp |
| `lamp fake [--every 10] [--cycles N] [--everything] [--hands]` | `--every`: seconds between arrivals (10). `--cycles`: stop after N (default: until Ctrl-C). `--everything`: answer every op, without refusing what lamp-control doesn't route yet. `--hands`: a hand waves for 3 s after each arrival, and looks are answered, as on a lamp with the body's lease socket |

`lamp skill check` is the Python SDK's. `LAMPOS_HOME` sets where the key and
pairings are kept, for every command.
`LAMPOS_LOG=debug` prints every frame to and from the lamp on stderr, secrets
redacted ("Seeing every frame" above).
