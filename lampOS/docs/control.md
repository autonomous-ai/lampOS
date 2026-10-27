# Control: programming the lamp from your own code

The lamp's programming protocol, v1: what a developer's code, an agent or a
skill may ask of the lamp, and what the lamp tells it. Designed by Nine on
#3504 ([12:24](https://github.com/autonomous-ai/lampOS/issues/3504#issuecomment-6434496168),
after the control map at [11:37](https://github.com/autonomous-ai/lampOS/issues/3504#issuecomment-6433873729)),
written up here by blue as its contract. The messages are
[control.schema.json](control.schema.json), JSON Schema 2020-12, which the
SDKs generate from.

**Built so far** (Nine, `crates/control`):
- **piece 1** (#3715): the protocol's messages and checks (`protocol.rs`), the lease rules (`lease.rs`) and the rates (`rate.rs`), with their tests;
- **piece 2** (#3736): pairing and hello (`pairing.rs`), and each connection's session over grants, leases and rates (`session.rs`).
- **piece 3**: the door on the network (`server.rs`, `cert.rs`, the `lamp-control` binary): WebSocket over TLS 1.3 at `/v1`, the lamp's own certificate, and the owner's admin socket. Its actions were refused until piece 4.
- **piece 4**: the routing (`parts.rs`). `gesture` and `move` go to the body's mood socket, `light` is a `light/request` on the bus, and `say` goes to the voice's say socket with its outcome as a `done` event. Each is at `skill` priority as `lamp-control`, held to the part's own checks. `look` and `joints` (the body's lease socket), `express`, `play` and `sound` are refused with what they wait for. A computer's last connection closing, or a revoke, stops what it asked for, and actions a part refuses are bounded at 20 a second.

**No unit carries it yet**, so nothing listens on a lamp; discovery, and the bus's events and state, are the next pieces, off unless `LAMP_CONTROL=on`, and the owner turns developer
access on in the app. This
page is the contract it and the SDKs are built to. Its protocol types, lease
rules and rates are `crates/control` (Nine, #3715), and the schema follows
its types. A few of the crate's checks can't be written in JSON Schema, so a
request can pass the schema and still be refused with `bad_request` saying
which:
- a say's text and each phrase must hold a letter or a digit (a say may hold
  a newline, no other control character, C0 or C1);
- a move's last frame is all 0, like its first.

What is decided is marked as such; the open questions are at the end.

## What it is for

The owner's words (#3504): "a protocol that's convenient for everyone to
program with the lamp; everyone who buys the lamp can easily customise it
their way". Four decisions shape it:

1. **Developers first.** The live API, Python and JavaScript SDKs and an MCP
   server come first; no-code editing by voice or the app later.
2. **Local only.** Control works on the home network or on the lamp itself.
   Nothing of it crosses the cloud.
3. **Open, within the body's limits.** A paired client may move the body
   freely inside the limits `lamp-body` keeps for every sender. A client that
   isn't paired gets nothing.
4. **Sharing from day one.** Skills go to the Store signed, and installing
   one shows its permissions first.

## One vocabulary under every door

Every door (the live API, MCP, rules skills, script skills, Store packages)
speaks the same names. Each maps onto what a lamp part already publishes or
takes; the grant is what a client must hold.

**Who may call:** only a computer the owner paired, with the grants the
owner ticked. An unpaired caller gets `unpaired` and the connection closes.

| grant | allows |
|---|---|
| `body` | `gesture`, `move`, `play`, `look`, `joints`; with `light`, `express` |
| `light` | `light`; with `body`, `express` |
| `speak` | `say`, `sound` |
| `look` | presence events and state: arrivals, leavings, facing, standing up. Positions, never who |
| `hear` | `phrase.heard` for the client's own phrases (never the words around them), `name.heard` |
| `sound` | `sound.heard`, by class |

**Events**, pushed to a client that subscribed:

| event | fields | from | grant |
|---|---|---|---|
| `person.arrived`, `person.left` | `count`, `near` (share of the picture's height) | `humans/faces/tracked`, as skills' `appears` and `absent_for` | `look` |
| `person.stood_up` | `side` | `room/stood_up` | `look` |
| `person.facing` | `facing` | `humans/faces/tracked` | `look` |
| `phrase.heard` | `phrase`: one the client registered | `voice/heard`, matched on the lamp: the client is told which phrase, **never what was said** | `hear` |
| `name.heard` | | `name/heard` | `hear` |
| `sound.heard` | `class` (`knock`, `music`, …), `score` | `ears/classes` | `sound` |
| `touch` | `kind` (`tap`, `pet`, `hold`), `where` | `controls/touch`, `controls/gesture`, `body/hand` | none |
| `button` | `press` (`press`, `long_press`) | `controls/gesture` | none |
| `time` | the client's own schedule | the lamp's clock and zone | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `quiet_hours`, `lease` | `voice/state`, `lamp/pause`, privacy, `voice/mute`, the leases | none |

**Actions:**

| op | args | carried out by | grant | lease |
|---|---|---|---|---|
| `express` | `feeling`, `strength` 0–1 (0.5) | the `feelings` skill (#3691), so one set of feelings | `body`, `light` | no |
| `gesture` | `name`, one of 19 built-in clips (below) | `mood.sock` `{gesture}` | `body` | no |
| `look` | `at`: `face`, `name`, `ahead`, or `{yaw, pitch}` in degrees (yaw right and pitch up positive) | `gaze.sock`, body goals, within the gaze reach | `body` (and `look` for `face`) | body |
| `move` | `step_ms` 20–1,000, `frames` (2–500) of 5 joints in counts, first and last all 0 | `mood.sock` `{move}`, through `moves.rs`'s check | `body` | body |
| `play` | `name` of a move in the client's installed package | the same, from its `moves/NAME.json` | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`, degrees; one at least), `within_ms` ≤ 5,000 | **new**: a goal stream under a lease, every step through the body's caps | `body` | body, named |
| `light` | `colour` (one of 10), `level` 1–5, `pattern`, `for_ms` ≤ 30 s | `light/request` at the priority `lamp-control` stamps | `light` | no |
| `say` | `text`, at most 300 characters | `say.sock`, with its quiet, privacy and busy gates | `speak` | no |
| `sound` | `name` of a clip of 3 s or less in the client's package | `say.sock` `{sound}` | `speak` | no |
| `stop` | | ends the client's own actions now | none | no |

**Gestures** a client may name: `nod`, `headshake`, `greeting`, `laugh`,
`shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`,
`surprised`, `confused`, `sleepy`, `excited`, and the five dances
`dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`.
These are the body's own clips (`crates/body/src/clips/data.rs`) but two:
`idle` is the lamp's own life, and `scanning` looks like a camera sweeping a
room, which CLAUDE.md's rule 8 keeps for when a person asks for it. A test in
`lamp-body` keeps this list and the schema's in step with the clips.

**Colours and patterns:** the ten of `lamp_msgs::Light::COLOURS` (`yellow`,
`red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`),
and the three of `PatternKind`: `breathe`, `candle`, `pulse`. A light never
blinks or flashes. A test in `lamp-msgs` keeps the schema's lists in step.

**State** (`get`, or the events above): presence (`count`, `near`,
`facing`, never who), the light (the person's own and what shows), mood
(`mood/state`'s energy, warmth, awake, asleep), and `lamp_state`.

**Never through this door:** camera frames, audio, transcripts, face prints
or names. A later grant for any of them would be its own decision, with its
own consent screen.

## The doors, and when

| door | for | phase |
|---|---|---|
| Live local API: WebSocket JSON over TLS, mDNS, paired keys | a developer's own code | 1 |
| MCP: the same API as MCP tools, served on the developer's computer | Claude, ChatGPT and other agents | 1 |
| SDKs: thin Python and JavaScript clients | developers | 1 |
| Rules skills: today's `skill.toml`, published as a versioned format | anyone, no code | 2 |
| Store: signed packages, permissions shown first | everyone | 2 |
| Script skills: code on the lamp, sandboxed, the same protocol on a local socket | developers who want it to run without their computer | 3 |
| No-code editing by voice and the app | everyone | 4 |

## Running the door (piece 3)

```text
lamp-control serve [--state-dir DIR] [--listen ADDR] [--admin-socket PATH] [--name NAME]
lamp-control code --grants body,light,speak,look  # the owner's side: a pairing code and the pin; look for presence events
lamp-control revoke c_…
lamp-control clients
```

- **Off by default:** `serve` runs only with `LAMP_CONTROL=on`; otherwise it says so and exits 0. No product unit passes it yet.
- **The certificate:** made on first start in `--state-dir` (`/var/lib/lampos-control`): P-256, valid to 2100, `cert.der` and `key.der`, the key 0600. Its pin, the certificate's SHA-256 in hex, is journaled at start and returned with every pairing code.
- **Listening:** `--listen`, `0.0.0.0:7731` by default. TLS 1.3 only, and only `/v1`: any other path is 404 before the WebSocket opens.
  - Frames are capped at 64 KiB, text only.
  - The TLS and WebSocket handshakes must finish within 10 s of the TCP connection, or it is closed.
  - At most 32 connections at once, and 4 from one address. Past either, a new connection is closed at once, holding nothing. An accept that fails waits 100 ms before the next.
  - A connection that hasn't said `hello` within 30 s of its welcome is closed, a `pair` first included, and other frames before it buy no more time. One quiet for 2 minutes after `hello` is closed too. A WebSocket ping or pong counts, so a client that only listens stays open on its keepalive.
  - Lease changes (a lapse, a conversation, a hand) go to every connection of the computer that held the lease.
- **The owner's side:** `--admin-socket`, `/run/lampos-control/admin.sock`, 0600 in a 0700 folder, one JSON object a line:
  - `{"code": {"grants": [...]}}` → `{"code", "pin"}`;
  - `{"revoke": "c_…"}` → `{"revoked": true, "saved": true}`. The revoke takes effect at once either way. If the file can't take it yet, the answer is `"saved": false` with an `error`, because a restart before a save succeeds brings the key back; the next save that succeeds writes it out;
  - `{"clients": true}` → the list.

  The app reaches it through the lamp's cloud part in a later piece.

## The wire

**Where:** `wss://<lamp>.local:7731/v1`, served by `lamp-control`. Nothing
listens until the owner turns developer access on in the app. Script skills
(phase 3) use the same messages on `/run/lampos-control/control.sock`.

**Finding the lamp:** mDNS `_lampos._tcp`, with TXT `id`, `name`, `v=1` and
`fp`: the first 16 hex characters of the SHA-256 of the lamp's certificate.
The certificate is self-signed, made per lamp on its first start, and the
client pins it at pairing.

**Frames:** UTF-8 JSON, one message per text frame. The lamp's first frame
on every connection is a welcome, carrying a nonce of 32 random bytes made for
that connection, which pairing's proof and hello's signature cover:
`{"ev":"welcome","at":"…","nonce":"<base64>","lamp":{"id":"lamp-75c9","name":"…","v":1}}`.
- A request: `{"id": 7, "op": "...", ...}`. `id` is the client's, a whole
  number, echoed as `re`. Every op but `pair` and `hello` needs `hello`
  first on the connection.
- A reply: `{"re": 7, "ok": true, ...}`, or
  `{"re": 7, "ok": false, "error": {"code": "...", "why": "..."}}`. `re` is
  null when the request's id couldn't be read. A field the op doesn't know
  refuses it (`bad_request`), so a typo is never a step silently not taken.
- An event: `{"ev": "...", ...}`. The events about the room carry `at`, the
  lamp's local time with its offset (`"2026-10-27T12:30:01+07:00"`);
  `welcome`, `lease` and `done` may not.
- A client ignores a field it doesn't know, in a reply, an error or an
  event, so new fields can be added without a new version.

### Pairing, once per computer

1. **In the app**, the owner chooses *Add a developer computer* and ticks
   the grants. The app asks the lamp for a one-time code and shows it as a QR
   code and as 8 digits **with the pin of the lamp's certificate** (its full
   SHA-256, 64 hex), the lamp's id and the expiry (5 min). A new code voids
   the old one.
2. **`lamp pair`** (in the SDKs) finds the lamp by mDNS, connects, and
   **checks the certificate against the app's pin before it sends
   anything**. An SDK that can't check it must not pair. Then it sends the
   computer's new Ed25519 public key with a proof it holds the code:
   ```json
   {"id":1,"op":"pair","name":"Dev's laptop","key":"<32 bytes, base64>","proof":"<32 bytes, base64>"}
   ```
   The proof is HMAC-SHA256 keyed with the code's 8 ASCII digits, over
   `lampos/v1/pair` and a zero byte, the pin's 32 bytes, the 32 key bytes,
   then the welcome's 32 nonce bytes. A wrong proof closes its connection,
   and pairing tries are one a second across every connection (`rate`, with
   `retry_ms`); junk proofs never void the owner's code.
3. **The lamp** checks the proof, keeps `{key, name, grants, paired_at}` in
   `/var/lib/lampos-control/clients.json` (0600, written whole and renamed
   over, at most 16 computers; the code isn't spent when the lamp is full,
   and the same key pairing again replaces itself), answers
   `{"re":1,"ok":true,"client":"c_3f9a","grants":["body","light","speak"]}`,
   and gives a small nod with the ring's "someone new paired" cue.
4. **No pairing without the app's pin.** A code alone, said aloud or typed
   without its pin, can't pair (the spoken-code fallback is dropped, below).

**The attacker pairing is built against** (the Reviewer-lead's and
Fourteen's P2 on #3736): a host on the home network that answers the
computer's first connection as `<lamp>.local` before the real lamp does. An
8-digit code can be searched offline from one `pair` frame in seconds, so
the frame must never reach such a host. The pin from the app, checked before
`pair`, keeps it from being sent there; the pin inside the proof makes a
proof relayed through a host in the middle fail at the real lamp, even from
a client that forgot to check. On the lamp, 300 tries in a code's 5 minutes
against 10^8 codes leaves online guessing out of reach.

### Hello, every connection

```json
{"id":1,"op":"hello","client":"c_3f9a","sig":"<64 bytes, base64>"}
→ {"re":1,"ok":true,"lamp":{"id":"lamp-75c9","name":"…","v":1},"grants":["body","light","speak"],"limits":{…}}
```

`sig` is the client key's Ed25519 signature over `lampos/v1/hello` and a
zero byte, then the welcome's 32 nonce bytes. Each proof names what it is for,
so one made for pairing can never stand as a hello's, or the other way round
(Nine, #3504). An unpaired client, a revoked one or a bad signature gets
`unpaired` and the connection closes. One `hello` a connection: a second is
`bad_request`. A revoked computer's open connections close at once, a
listener's included, after its leases' `lost`, and its leases end; they also end when its last
connection closes. `limits` says the rates below, so an
SDK can keep under them: for each class (`joints`, `motion`, `light`, `say`,
`express`, `refused`) a list of `{"most": N, "within_ms": M}`, and `hold_max_ms`,
`take_max_ms` and `say_max_chars`.

### Subscribe and state

```json
{"id":2,"op":"subscribe","events":["person.arrived","touch","phrase.heard"],"phrases":["lights please","good night lamp"]}
{"id":3,"op":"get","state":["presence","light","lamp"]}
{"id":4,"op":"unsubscribe","events":["touch"]}
```

`phrase.heard` needs `phrases`, the ones to listen for, and `phrases` go
only with it. A `get` reply carries each state asked for: `presence`,
`light`, `mood`, and `lamp_state` for `lamp`.

### Leases

A body lease is needed for `look`, `move`, `play` and `joints`. The lease
belongs to the client that took it, on any of its connections, and ends
when the client's last connection closes; only `joints` names it. Parts are
`body`, `light` and `voice` (`say`, `sound`), each needing its grant, and one
client holds a part at a time: a second take is `busy`.

```json
{"id":5,"op":"lease.take","parts":["body","light"],"for_ms":30000,"why":"desk robot demo"}
→ {"re":5,"ok":true,"lease":"l_81","until":"…"}
{"id":6,"op":"lease.renew","lease":"l_81","for_ms":30000}
{"id":7,"op":"lease.release","lease":"l_81"}
```

A lease lasts at most 30 s before it must be renewed, and a take can be
renewed for at most 10 minutes in all; then the client takes it again. The
lamp says what happens to it:
`{"ev":"lease","lease":"l_81","state":"suspended","why":"conversation"}`,
later `held` again or `lost`, with `why` one of `conversation`, `hand`,
`stop_heard`, `private`, `paused`, `asleep`, `expired`,
`taken_by_owner_app`, `revoked`, `released` (by the client, or its
connection closed). Ids are `l_` and a number.

- **Suspended** (`conversation`) is held again when the talk ends, if it
  hasn't lapsed. While suspended, the lease is still the holder's, so nobody
  else may take the part, and its clock runs.
- **Lost:** an act on a lost lease is refused with how it ended.

### Actions

```json
{"id":8,"op":"express","feeling":"glad","strength":0.6}
{"id":9,"op":"gesture","name":"nod"}
{"id":10,"op":"look","at":{"yaw":-10,"pitch":5}}
{"id":11,"op":"move","step_ms":40,"frames":[[0,0,0,0,0],[20,0,30,0,0],[0,0,0,0,0]]}
{"id":12,"op":"joints","lease":"l_81","targets":{"pitch":4.0,"yaw":-12.0},"within_ms":300}
{"id":13,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}
{"id":14,"op":"say","text":"Build passed."}
{"id":15,"op":"stop"}
```

A reply comes when the action is taken. A long one also ends with
`{"ev":"done","re":11,"how":"finished","why":"…"}`, `how` one of
`finished`, `cut_short`, `dropped`. Today only `say` ends with `done`:
`finished` once it's spoken, or `dropped` with the voice's own word and why
(`expired`, `refused: …`, `dropped: …`, or the voice not reached). `gesture`
and `move` answer `{"to":"body"}` once the body has them, and no `done`
follows yet: the body plays or drops them by its own checks and doesn't say
which back. An SDK's wait for them returns at the answer.

### Errors

| code | when |
|---|---|
| `unpaired` | no pairing, a revoked one, or a bad signature |
| `no_hello` | an op before `hello` on this connection |
| `no_grant` | the client lacks the grant; `why` names it |
| `no_lease`, `lease_lost` | the action needs a lease the client doesn't hold |
| `busy` | another client holds the part, or this one by another lease |
| `refused` | the body's own check, with its why: "centre of gravity 27 mm > 22 mm", "pitch past its window" |
| `quiet_hours`, `private`, `paused`, `asleep` | the lamp's state refuses it |
| `rate` | over a rate below; `retry_ms` says when |
| `bad_request` | not this schema, or an op it doesn't know |
| `off` | developer access is off in the app |

### Every op, its fields and limits

| op | fields | needs | lease |
|---|---|---|---|
| `pair` | `name` (1–64 chars), `key` (Ed25519 public, base64), `proof` (base64) | — | — |
| `hello` | `client` (`c_` + 1–32 lowercase hex), `sig` (base64) | — | — |
| `subscribe` | `events` (at least one), `phrases` (≤ 16, ≤ 64 chars, only with `phrase.heard`, which needs them) | each event's grant | — |
| `unsubscribe` | `events` | — | — |
| `get` | `state`: `presence` (`look`), `light`, `mood`, `lamp` | — | — |
| `lease.take` | `parts` (`body`, `light`, `voice`), `for_ms` 1–30,000, `why` ≤ 200 chars | `body`, `light` or `speak` for each part | — |
| `lease.renew` | `lease`, `for_ms` 1–30,000 | — | — |
| `lease.release` | `lease` | — | — |
| `express` | `feeling` (a lowercase name, the `feelings` skill's), `strength` 0–1 (0.5) | `body` + `light` | — |
| `gesture` | `name` (a built-in, not `idle` or `scanning`) | `body` | — |
| `look` | `at`: `"face"` (+ `look`), `"name"`, `"ahead"`, or `{yaw, pitch}` in degrees (±180, ±90) | `body` | body |
| `move` | `step_ms` 20–1,000, `frames` 2–500 of five joints (body counts from home), first and last all 0 | `body` | body |
| `play` | `name` (a recorded move of the client's package) | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`; degrees from −180 to 180, at least one), `within_ms` ≤ 5,000 | `body` | body |
| `light` | `colour` (one of the ten), `level` 1–5, optional `pattern` (`breathe`, `candle`, `pulse`), `for_ms` 1–30,000 | `light` | — |
| `say` | `text`: words, ≤ 300 chars | `speak` | — |
| `sound` | `name` (a clip of the client's package, ≤ 3 s) | `speak` | — |
| `stop` | — | — | — |

`move`'s shape is checked here, and the body's move check
(`crates/body/src/moves.rs`) judges the rest.

## Who wins

1. The body's safety limits.
2. The person: a hand on the head, the privacy switch, "stop", pause.
3. The conversation.
4. The owner's app.
5. A client, or an installed third-party skill.
6. The lamp's own idle life.

This is the body's own order (`docs/body.md`, person > safety >
conversation > skill > idle) with the owner's app and clients placed in it.
A conversation suspends a client's body lease, easing out as a skill's
gesture does; a hand or "stop" ends it; pause, privacy and sleep refuse new
takes. A client's light goes as a `light/request` at the priority
`lamp-control` stamps, never one the client names, and the person's own
light comes back when it ends. While anyone holds a lease, the ring shows a
small steady "held" mark. A client's `say` is always in the lamp's own voice,
through its own gates.

## Limits, rates and quiet hours

Every motion goes through `lamp-body`'s limits, which no sender can raise:
its speed and acceleration caps and pitch floor (`body.rs`), each joint's
farthest and fastest in a move (`moves.rs`: farthest 200, 200, 150, 350, 350
counts and fastest 900, 1,300, 1,000, 800, 800 counts a second for head
pitch, head roll, base yaw, shoulder and elbow; 1.5 to 10 s, from the pose
and back), the 22 mm centre-of-gravity rule (`cog.rs`), its force limit,
strain rests and heat stop. `joints` goals get the same caps at each step.
There is no raw servo or register access.

Rates a client is held to:

| what | at most |
|---|---|
| `joints` | 20 a second |
| `gesture`, `move`, `play`, `look` | 1 every 2 s |
| `light` | 5 a second |
| `say`, `sound` | 6 a minute and 60 an hour |
| `express` | 1 a minute |
| actions a part refused | 20 a second, then the connection closes |
| registered phrases | 16, of 64 characters at most |

`gesture`, `move`, `play` and `look` share one class, `motion`. A refusal
says `retry_ms`, the time until the oldest counted action leaves its window.
A refused action isn't counted.

In the lamp's quiet hours the ring's night caps still clamp every light,
`say` and `sound` are refused unless the owner allowed that client at night,
and nothing moves while the lamp sleeps. Every command and its answer is
journaled as one line (client id and name, op, result); the app can show the
last 100 and revoke a client, which closes its connections at once.

## The SDKs

Thin Python and JavaScript clients generated from
[control.schema.json](control.schema.json): finding the lamp, `lamp pair`,
hello, `on(event, handler)`, every action as a call, leases renewed for the
caller, and the error codes as typed errors. For example, greeting a person
who arrives:

```python
lamp.on("person.arrived", lambda e: lamp.gesture("greeting"))
```

They run on the developer's computer, never on a lamp. Until `lamp-control`
is built they are tested against a fake server that speaks this schema.

## Versioning

The path carries the version, `/v1`. New fields are additive; a client
ignores fields it doesn't know. An op the lamp doesn't know is a
`bad_request`. A change that isn't additive is `/v2`.

## To come

These pieces follow one PR each, each off by default:
- the server: WSS, mDNS, the per-lamp certificate, pairing and `hello`
  with Ed25519 (Nine, #3736);
- each action routed to the part that does it, under the lamp's own user,
  with the priority stamped by `lamp-control`, never the caller's word;
- the events from the bus;
- the journal line per command;
- the app's switches;
- the SDKs (blue, `sdk/python` and `sdk/js`) and `lamp-mcp` (Eighth,
  `sdk/mcp`).

## Open questions

Nine's, for the first builder (#3504, 12:24):

1. Is one pairing code over the owner's account channel "approval, never
   control"? Or must pairing stay on the LAN, which needs the app to reach
   the lamp locally?
2. Port 7731 and the name `lamp-control`.
3. Should `express` wait for the `feelings` skill (#3691) to fix its names?
   Until then the schema takes any short lowercase name, and the lamp answers
   `bad_request`.
4. The script runtime of phase 3: WASM or QuickJS.
