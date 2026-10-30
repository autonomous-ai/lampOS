# lampos (JavaScript): program a lampOS lamp from your computer

The Node client for the lamp's v1 control protocol
([docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md),
messages in
[control.schema.json](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.schema.json)).
Same names as the Python SDK, in camelCase, with promises. It runs on your
computer, never on a lamp. Node 20.11 or later.

**New here?** [docs/develop.md](../../docs/develop.md) is the 10-minute quick-start: the fake lamp, a first program, pairing, lamp-mcp and skills.

**Not usable on a lamp yet:** the lamp's side, `lamp-control`, is being built
(#3504). Until then, try it with the fake lamp below.

## Changes

- **0.2.0:** `await lamp.get(...)` returns the states asked, by name
  (`{"presence": {...}, "light": {...}}`), as lamp-control's reply carries them
  under `state`; 0.1.0 returned the whole reply. A `light` or `mood` the lamp
  hasn't heard yet is `null`, on the fake lamp too.

## Install

It isn't on npm yet. From a checkout of this repository, in your project:

```sh
npm i /path/to/lampOS/sdk/js               # add bonjour-service to find lamps by mDNS
```

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
example runs this way: `greet.js`, `follow.js`, `follow-hand.js` (with `lamp fake --hands`), `phrase.js`, `pomodoro.js`,
`ci-light.js`.

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
`express` as the feelings skill's move and light,
`look` refused as "the body's lease socket is not on here" (no product
unit passes it yet), and `play`, `sound` and `joints` refused (`refused`,
"not routed yet: waits for …"), in lamp-control's own words, so a program
built against it meets the same refusals on a lamp. `--everything` answers
every op instead.

## Pair with a real lamp

Run the command as `npx --no-install lampos …` (or from `node_modules/.bin`),
in a project with `lampos` installed: npm has an unrelated package called
`lamp`, and a plain `npx lamp` outside such a project would offer to install
it.

```sh
npx --no-install lampos pair 12345678 --pin 3f9a…   # the code and the lamp's pin (64 hex), both shown in the app
```

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
  | `name.heard` | | `hear` |
  | `sound.heard` | `class`, `score` | `sound` |
  | `touch` | `kind` (`tap`, `pet`, `hold`), `where` | none |
  | `button` | `press` (`press`, `long_press`) | none |
  | `time` | your schedule | none |
  | `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `lease`, and `quiet_hours` (planned: not produced yet; the door already refuses `say` and `sound` in quiet hours, or while the lamp's local time isn't known, with the error `quiet_hours`) | none |
  | `volume.changed` | `level` (0–100), `muted`: the speaker's volume changed, whoever changed it (the person by voice or in the app, or a computer) | `speak` |

- **State:** `await lamp.get('presence', 'lamp')` resolves with the states
  asked, by those names, as lamp-control answers them: `{ presence: {...},
  lamp: {...} }`. Behaviour change: it used to resolve with the whole reply.
  `light` is the person's own (`on`, `colour`, `level`) with `shows`, what the
  ring draws now; the fake's `shows` is the newest light asked while it lasts.
- **Actions:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`. Each resolves when the lamp has taken it;
  `{ wait: true }` on a gesture, move or play resolves when it has finished,
  or, where the lamp hands it on and says no more (today `gesture` and
  `move`, answered `{"to": "body"}`), at that answer.
- **A motion asked while another still plays is refused `busy`**, with
  `retry_ms`, when it ends (#4209, lamp-control #4494): `byCode.busy`. The body plays
  one at a time (a gesture or move 2 to 10 s, a feeling's move 2.5 to 7 s),
  whoever asked. Wait `retry_ms` and ask again.
- **Closing gives your light back at once.** When a computer's last
  connection closes, the lamp gives back the light it asked for and stops a
  `say` still waiting. A script that lights the ring and exits shows it for a
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
  `how` and `why`. Needs `body` and `look`; each start is a look, held to
  the motion rate (one in 2 s). See `examples/follow-hand.js`.
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
`lamp.closed()` resolves: a long-running program connects again, for
example in a loop with a few seconds' pause.

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

## Everything you can read

What the lamp tells a paired computer, read from lamp-control's `watch.rs`,
[control.md](../../docs/control.md) and the schema. Every event comes **by
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
| `name.heard` | none | `{"ev":"name.heard"}` | someone says the lamp's name | `hear` |
| `sound.heard` | `class`: `music`, `laughter`, `door` or `footsteps`. `score`: 0.5–1, 2 places | `{"ev":"sound.heard","class":"door","score":0.72}` | a class's score rises past 0.5 (again only after it falls under). Never speech | `sound` |
| `touch` | `kind`: `tap` (two or more taps are one tap) or `pet`. The schema also has `hold` and `where`, not sent yet | `{"ev":"touch","kind":"pet"}` | a tap or a pet on the lamp | none |
| `button` | `press`: `long_press`. The schema also has `press`, not sent yet | `{"ev":"button","press":"long_press"}` | the button is held | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep` (each true or false). `quiet_hours` is planned, not sent yet | `{"ev":"lamp.changed","lamp_state":{"talking":true,"paused":false,"private":false,"asleep":false}}` | any of the four changes | none |
| `volume.changed` | `level`: 0–100 of what the lamp allows. `muted`: true at 0 | `{"ev":"volume.changed","level":30,"muted":false}` | the volume changes, whoever changed it: the person by voice or in the app, or a computer | `speak` |
| `time` | your schedule | | accepted in a subscription, not produced yet | none |
| `hand` | `seen`. When seen: `x`, `y` (the palm's centre, 0–1 from the picture's left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. All to 2 places, never the tracker's clock | `{"ev":"hand","seen":true,"x":0.42,"y":0.55,"size":0.12,"open":true,"confidence":0.91}` and `{"ev":"hand","seen":false}` | a hand is first seen, moves or grows by 0.05, opens or closes, at most 4 a second (the newest kept). `seen: false` once, 1 s after the last. Under 0.5 confidence is no hand | `look` |

And two that come by themselves, with no subscription:

| event | fields | example | fires when |
|---|---|---|---|
| `lease` | `lease`, `state` (`held`, `suspended`, `lost`), `why` (`conversation`, `hand`, `stop_heard`, `private`, `paused`, `asleep`, `expired`, `taken_by_owner_app`, `revoked`, `released`, `slumped`) | `{"ev":"lease","lease":"l_3","state":"suspended","why":"conversation"}` | one of your leases changes: talking suspends it and resting holds it again; a hand on the body, a stop, a pause, privacy or sleep loses it |
| `done` | `re` (your request's id), `how` (`finished`, `cut_short`, `dropped`), `why` | `{"ev":"done","re":14,"how":"finished"}` | an action that ends later has ended. Today that's `say` |

### States, with `get`

| state | fields | example | grant |
|---|---|---|---|
| `presence` | `count`, `near` (0–1), `facing`. Counts and how near, never who | `{"count":1,"near":0.31,"facing":true}` | `look` |
| `light` | the person's own light: `on`, `colour`, `level`. Also `shows`: what the ring draws now, `{on, colour, level}`. `null` until the lamp has said either | `{"on":true,"colour":"warm","level":3,"shows":{"on":true,"colour":"green","level":2}}` | none |
| `mood` | `energy`, `warmth`, `awake`, `asleep`, the lamp's inner state. `null` until known | `{"energy":0.6,"warmth":0.7,"awake":true,"asleep":false}` | none |
| `lamp` | `talking`, `paused`, `private`, `asleep` | `{"talking":false,"paused":false,"private":false,"asleep":false}` | none |
| `volume` | `level` (0–100 of what the lamp allows), `muted`. `null` until lamp-actions has said it | `{"level":50,"muted":false}` | `speak` |
| `air` | the room's air (particles, humidity, temperature, CO2), from the lamp's air part | planned: not a state `get` answers yet | |

### Never readable

Through this door, by any grant: camera frames or pictures, audio, transcripts
or the words around a phrase, face prints, and anyone's name. A hand's or a
face's place in the picture is a share of it, never the picture.

## Reference

Every public call and `lamp` command, with what the lamp takes. The ranges are
what lamp-control and the ring refuse past, read from
[control.md](../../docs/control.md) and their code. The SDK checks names
against the schema before sending, and the lamp checks the rest. A test keeps
every public method named here.

**Today a lamp refuses** `look` (but at `'hand'`, on a lamp with the body's lease
socket), `joints`, `play` and `sound` with `refused`, "not routed yet: waits for
…" (`LAMP_CONTROL_REFUSES` in `lampos/testing`), and `say` and `sound` in its quiet
hours (by default 22:00 to 07:00 local; a lamp can set its own or `off`, said in
`lamp.limits.quiet_hours`), or while its time isn't known, with `quiet_hours`.

### Connecting

| call | parameters | resolves with |
|---|---|---|
| `Lamp.pair(code, host, { pin, port = 7731, name, keyring })` | `code`: the owner's 8-digit pairing code. `host`: the lamp's address. `pin`: the certificate's SHA-256 the app shows, 64 hex. `port`: 7731. `name`: this computer's name in the app, 1–64 characters, default the host name. `keyring`: default `new Keyring()` (`LAMPOS_HOME`, else `~/.config/lampos`) | the pairing (`lamp`, `client`, `grants`, …) |
| `Lamp.connect(lamp, { host, keyring })` | `lamp`: a paired lamp's id, or `undefined` for the only one paired. `host`: an address to use instead of the kept one | a `Lamp`, after hello |
| `lamp.info`, `lamp.grants`, `lamp.limits` | | the lamp's id and name, this computer's grants, the lamp's limits: each rate as `{most, within_ms}` windows (`joints`, `motion`, `light`, `say`, `express`, …), `hold_max_ms`, `take_max_ms`, `say_max_chars` and `quiet_hours` (`{from, to}` or `off`). By default the rates are off: `rates: "off"`, and only the `refused` and `phrases` windows (a lamp run with `--rates default` gives every class). The fake lamp sends only `joints_hz` |
| `lamp.close()` | | lets go of every lease, then closes |
| `lamp.closed()` | | resolves when the connection ends |
| `find(ms = 3000)` | `ms`: how long to listen for mDNS | the lamps on this network (`id`, `name`, `host`, `port`, `fp`) |

### Events and state

| call | parameters | grant |
|---|---|---|
| `lamp.on(event, handler, { phrases })` | `event`: one of the schema's events (`person.arrived`, `person.left`, `person.stood_up`, `person.facing`, `hand`, `phrase.heard`, `name.heard`, `sound.heard`, `touch`, `button`, `time`, `lamp.changed`, `volume.changed`; their fields and grants are [control.md](../../docs/control.md)'s Events), or `lease` or `done`. `handler(event)`: a function, or async. `phrases`: with `phrase.heard` only, up to 16 for the computer, each of 64 characters at most and 8 letters at least. Changing the phrases is 6 a minute | the event's. Rejects with the lamp's refusal |
| `lamp.off(event, handler)` | as `on` | unsubscribes once the event's last handler is gone |
| `lamp.subscribe(events, phrases)`, `lamp.unsubscribe(events)` | an array of events | each event's |
| `lamp.get(...states)` | `presence` (needs `look`), `light`, `mood`, `lamp` | the states asked, by those names |

### Leases

| call | parameters | |
|---|---|---|
| `lamp.withLease(parts, body, { forMs = 30_000, why })` | `parts`: `body` (grant `body`), `light` (`light`), `voice` (`speak`). `forMs`: 1–30,000. `why`: up to 200 characters, shown to the owner. `body(lease)`: async | takes the lease, renews it at half its time, and lets go when `body` ends |
| `lamp.lease(...parts)`, then `await lease.take()` and `await lease.release()` | as above, for 30 s | `lease.id`, `lease.state` (`held`, `suspended`, `lost`, `released`), `lease.held`, `lease.lostWhy` |

### Actions

Each resolves with the lamp's answer when the action is taken. `{ wait: true }`
resolves with its `done` instead, or with the answer where no `done` follows
(`gesture`, `move`).

| call | parameters | grant | lease | rate |
|---|---|---|---|---|
| `lamp.express(feeling, strength)` | `feeling`: `glad`, `lively` or `low`. `strength`: 0–1, default 0.5 | `body` and `light` | no | 1 a minute |
| `lamp.gesture(name, { wait })` | `name`: `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz` (never `idle` or `scanning`) | `body` | no | motion: 1 every 2 s |
| `lamp.look(at)` | `at`: `'face'` (needs `look` too), `'hand'` (needs `look` too: the body follows the hand in view until another look, a `stop` or the lease ends; answered `{"to":"body","follow":"hand"}`, no `done`; needs the body's lease socket on the lamp), `'name'`, `'ahead'`, or `{ yaw, pitch }` in degrees, −180 to 180 and −90 to 90, right and up positive | `body` | `body` | motion |
| `lamp.followHand({ forMs, why })`, `lamp.withHand(body, options)` | `await lamp.followHand().start()` then `await following.stop()`, or `lamp.withHand(async (following) => ...)`: a `look` at `'hand'` under a body lease of its own (`forMs`, default 10,000, renewed), until `stop()`, another look, or the lamp takes the body back. `following.ended` says how: `finished`/`stopped`, or `cut_short` with the why. `lamp.stop()` lets go of its lease too. After another look the lease stays held (that look runs under it), and the lamp's own life stands back until `stop()`; a new `followHand()` lets go of it first. A lamp that takes the look without following (before #4183) is `refused`, and the lease let go. Where the hand is comes as `hand` events | `body`, `look` | its own | motion |
| `lamp.move(frames, stepMs, { wait })` | `frames`: 2–500 frames of 5 joints in counts from home, the first and last all 0. `stepMs`: 40–1,000, and 1.5 to 10 s in all. Each joint within the body's reach and speed, else `bad_request` in the body's words | `body` | `body` | motion |
| `lamp.play(name, { wait })` | `name`: a move of your installed package | `body` | `body` | motion |
| `lamp.joints(lease, withinMs, targets)` | `lease`: your held `body` lease. `withinMs`: up to 5,000. `targets`: `{ pitch, roll, yaw, shoulder, elbow }` in degrees, −180 to 180, at least one | `body` | `body` (named) | 20 a second |
| `lamp.light(colour, level, forMs, pattern)` | `colour`: `yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`. `level`: 1–5. `forMs`: 1–30,000. `pattern`: none (steady), `'breathe'` (level 2 or up), `'candle'` (3 or up) or `'pulse'` (2 or up). Under its level a pattern is `refused`. The ring plays one pulse a minute for a computer: a second within the minute shows its colour and level steady, without its pulse. A light never blinks | `light` | no | 5 a second |
| `lamp.say(text)` | `text`: words, up to 300 characters | `speak` | no | 6 a minute, 60 an hour; refused in quiet hours |
| `lamp.volume({ to, step, mute })` | exactly one of `to`: 0–100 of what the lamp allows (0 is muted), `step`: `'up'` or `'down'` (the voice's own step, within the lamp's floor and ceiling), or `mute`: `true` or `false` (an unmute brings it back to where it was). Resolves to `{ level, muted }`, the level it left. The person's own volume, the same the voice and the app set | `speak` | no | none |
| `lamp.sound(name)` | `name`: a clip of 3 s or less in your package | `speak` | no | as `say` |
| `lamp.stop()` | | none | no | |
| `lamp.request(op, fields)` | any op of the schema, checked against it first | the op's | | |

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
| `busy` | A motion still plays (a gesture, move or feeling's move, whoever asked): wait `retry_ms` and send it again. Without `retry_ms`, someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first. |
| `refused` | The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again. |
| `quiet_hours` | It's the lamp's quiet hours (`limits.quiet_hours`, by default 22:00 to 07:00): `say` and `sound` wait until they end. A light is still fine. |
| `private` | The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false. |
| `paused` | The person paused the lamp. Wait for `lamp.changed` with `paused` false. |
| `asleep` | The lamp is asleep. Wait for `lamp.changed` with `asleep` false. |
| `rate` | Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded. |
| `bad_request` | The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json). |
| `off` | Developer access is off on this lamp: the owner turns it on in the app. |

### The `lamp` command

| command | options |
|---|---|
| `lamp find` | none: lamps on this network by mDNS |
| `lamp pair CODE --pin PIN [--host HOST] [--port 7731] [--name NAME]` | `CODE`: the 8-digit code. `--pin`: the 64-hex pin the app shows (required). `--host`: the address, found by mDNS without it, by the pin's first 16 hex. `--port`: 7731. `--name`: this computer's name in the app |
| `lamp hello [LAMP_ID]` | a paired lamp's id, or the only one: connects and shows its grants |
| `lamp fake [--every 10] [--cycles N] [--everything] [--hands]` | `--every`: seconds between arrivals (10). `--cycles`: stop after N (default: until Ctrl-C). `--everything`: answer every op, without refusing what lamp-control doesn't route yet. `--hands`: a hand waves for 3 s after each arrival, and looks are answered, as on a lamp with the body's lease socket |

`lamp skill check` is the Python SDK's. `LAMPOS_HOME` sets where the key and
pairings are kept, for every command.
`LAMPOS_LOG=debug` prints every frame to and from the lamp on stderr, secrets
redacted ("Seeing every frame" above).
