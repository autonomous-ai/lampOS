# Control: programming the lamp from your own code

**New here?** [develop.md](develop.md) is the quick-start: pair, a first program in Python or JavaScript, lamp-mcp, and a skill pushed and shared, in 10 minutes.

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
- **piece 4**: the routing (`parts.rs`). `gesture` and `move` go to the body's mood socket, `light` is a `light/request` on the bus, `say` goes to the voice's say socket with its outcome as a `done` event, `sound` is the voice's built-in short hum through the same socket and gates (`hum` is the only name in this release), and `express` is the `feelings` skill's own move and light (`feelings.rs`). Each is at `skill` priority as `lamp-control`, held to the part's own checks. `look` at `hand` has the body follow the hand in view, said with the body's lease (below); other looks, `joints` (the body's lease socket) and `play` are refused with what they wait for. A computer's last connection closing, or a revoke, stops what it asked for, and actions a part refuses are bounded at 20 a second.
- **piece 5**: the bus as events, lease signals and state (`watch.rs`, with `--bus-root`). Events go out by edge, never per message: someone arrived or left, turned to or from the lamp, or stood up; the lamp's name heard; a sound class past its bar (never `speech`); a touch or the button; the lamp talking, paused, private or asleep. Each goes only to a connection subscribed to it that holds its grant. Words heard are matched on the lamp against each connection's own phrases, and only the phrase leaves it. Talking suspends every lease and resting holds them again; a hand on the body, a stop, a pause, privacy or sleep loses them. `get` answers presence (counts, never who), the light, the inner state and the lamp's own.
- **piece 6**: the body's lease (`body_lease.rs`): with `--lease-socket`, a computer's `body` lease holds lamp-body's own, so the lamp's own life stands back, and the body's end of it is the computer's (`slumped` added to the whys).
- **piece 7**: skills pushed from a computer (`push.rs`, with `--manage-socket`): the `skills` grant, `skill.push` in parts, `skill.list` and `skill.remove`, each asked of root's manage helper off the door's lock, and a revoke turning the computer's pushed skills off. The helper's side is lamp-skills' (Eighth's piece B); until it is in, the helper refuses what it doesn't know.

**No unit carries it yet**, so nothing listens on a lamp; discovery is the next piece, off unless `LAMP_CONTROL=on`, and the owner turns developer
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
| `look` | presence events and state: arrivals, leavings, facing, standing up, a hand in view. Positions, never who |
| `hear` | `phrase.heard` for the client's own phrases (never the words around them), `name.heard` |
| `sound` | `sound.heard`, by class |
| `skills` | `skill.push`, `skill.list`, `skill.remove`: its own skills put on the lamp, which run on their own after it goes, never beyond its other grants ("Skills pushed from a computer") |

**Events**, pushed to a client that subscribed:

| event | fields | from | grant |
|---|---|---|---|
| `person.arrived`, `person.left` | `count`, `near` (share of the picture's height) | `humans/faces/tracked`, as skills' `appears` and `absent_for` | `look` |
| `person.stood_up` | `side` | `room/stood_up` | `look` |
| `person.facing` | `facing` | `humans/faces/tracked` | `look` |
| `hand` | `seen`; when seen, `x`, `y` (the palm's centre, a share of the picture from its left and top), `size` (the palm's width, a share of the picture's), `open`, `confidence` | `humans/hands/tracked`: one message per look that sees a hand, none without. Told when first seen, moved or grown by 0.05, opened or closed, at most every 250 ms with the newest held back until then; `seen: false` once, 1 s after the last message. A message under 0.5 confidence, or a share outside 0 to 1, is no hand. Never the picture, keypoints or the tracker's clock | `look` |
| `phrase.heard` | `phrase`: one the client registered | `voice/heard`, matched on the lamp: the client is told which phrase, **never what was said** | `hear` |
| `name.heard` | | `name/heard` | `hear` |
| `sound.heard` | `class` (`knock`, `music`, …), `score` | `ears/classes` | `sound` |
| `touch` | `kind` (`tap`, `pet`, `hold`), `where` | `controls/touch`, `controls/gesture`, `body/hand` | none |
| `button` | `press` (`press`, `long_press`) | `controls/gesture` | none |
| `time` | the client's own schedule | the lamp's clock and zone | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `quiet_hours` (planned, not produced yet), `lease` | `voice/state`, `lamp/pause`, privacy, `voice/mute`, the leases | none |

lamp-control reads each topic on its own thread. A state topic's message that says nothing new for these, the same privacy, pause, sleep, talking, light or mood as the last one, is passed over on that thread and never handed to the watch that turns the bus into events; a repeat still goes on every 250 ms. `voice/privacy` alone comes a hundred times a second (its lease, renewed every 10 ms), and handing each one on was half of lamp-control's idle CPU on lamp-75c9 (2.1% of a core before, 1.1% after; the rest is that thread reading them). A privacy lease not handed on for 1 s counts as private: every lease is lost with `private` and `lamp.changed` says so, until the lease is heard again. That is later than the voice's own microphone gate, which closes once a lease is 50 ms old; lamp-control says it 0.75 to 1.25 s after the last lease heard (1 s after the last one handed on, which is up to 250 ms older, and then the next 250 ms check), so a running voice, renewing every 10 ms, never reads as private. Each message is timed when its thread receives it, and every message already come is taken before a lapse is judged, so a lamp-control held up by a slow save doesn't read a renewed lease as lapsed. Before the first lease nothing lapses, so a lamp whose voice publishes none is never private for that.

**Actions:**

| op | args | carried out by | grant | lease |
|---|---|---|---|---|
| `express` | `feeling`, `strength` 0–1 (0.5) | the `feelings` skill's (#3691) move on `mood.sock` `{move}` and its light as a `light/request`, so one set of feelings (below) | `body`, `light` | no |
| `gesture` | `name`, one of 19 built-in clips (below) | `mood.sock` `{gesture}` | `body` | no |
| `look` | `at`: `face`, `hand`, `name`, `ahead`, or `{yaw, pitch}` in degrees (yaw right and pitch up positive). `hand` follows it until `stop`, another look or the lease ends | `gaze.sock`, body goals, within the gaze reach; `hand` with the lease socket's hold | `body` (and `look` for `face` and `hand`) | body |
| `move` | `step_ms` 40–1,000, `frames` (2–500) of 5 joints in counts, first and last all 0, lasting 1.5–10 s, each joint within the body's reach and speed | `mood.sock` `{move}`; the door holds it to `moves.rs`'s check first, so a move the body would drop is refused with its words | `body` | body |
| `play` | `name` of a move in the client's installed package | the same, from its `moves/NAME.json` | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`, degrees; one at least), `within_ms` ≤ 5,000 | **new**: a goal stream under a lease, every step through the body's caps | `body` | body, named |
| `light` | `colour` (one of 10), `level` 1–5, `pattern`, `for_ms` ≤ 30 s | `light/request` at the priority `lamp-control` stamps | `light` | no |
| `say` | `text`, at most 300 characters | `say.sock`, with its quiet after a turn, privacy and busy gates | `speak` | no |
| `sound` | `name`: `hum` only in this release, the voice's built-in short hum (no file, no model); the recorded notices are never a computer's | `say.sock` `{cue: "hum"}`, with say's quiet, rarity, privacy and busy gates; its `done` follows | `speak` | no |
| `stop` | | ends the client's own actions now | none | no |

**Gestures** a client may name: `nod`, `headshake`, `greeting`, `laugh`,
`shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`,
`surprised`, `confused`, `sleepy`, `excited`, and the five dances
`dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`.
These are the body's own clips (`crates/body/src/clips/data.rs`) but two:
`idle` is the lamp's own life, and `scanning` looks like a camera sweeping a
room, which CLAUDE.md's rule 8 keeps for when a person asks for it. A test in
`lamp-body` keeps this list and the schema's in step with the clips.

**Feelings** (`express`, `crates/control/src/feelings.rs`): the `feelings`
skill's three, each its `[[do]]` (`skills/feelings/skill.toml`). `glad` is
the `glad-lift` move, and the light breathing every 6 s for 12 s; `lively` is
`perk`, breathing every 4 s for 8 s; `low` is `low-droop`, steady for 20 s.
- **The move** is compiled in from `skills/feelings/moves` (lamp-control's
  unit reads no skills folder) and sent whole on `mood.sock` as `move` is, at
  `skill` priority, so the body plays or drops it by the same rules. From
  `strength` 0.5 up it is the skill's own move. Below, it is the nearest of
  three smaller ones, three quarters, a half and a quarter of each count,
  each with its own fixed name (`glad-lift-75`, `-50`, `-25`), because the body
  keeps a sent move by name and allows only 64 changes of one a run
  (`MOST_REPLACED`). Each still passes `moves.rs`'s check. Under 0.0625 no move
  is sent, but the light shows and the ask counts against the rate. Nothing
  is ever bigger than the skill's, and a move's way back past rest (`low`'s
  lift back) stays the skill's own, so a weak droop still comes up. `low`
  goes no smaller than three quarters, so that lift back keeps the lead a
  head needs to come up (about 47 counts on lamp-06c1).
- **The light** is one step warmer (white to warm) and one level dimmer than
  the person's own, as `lamp_skills::glow::relative` makes the skill's: a
  breath from their level down one and back, or steady one down. It lapses at
  the skill's `for`, and `stop` gives it back. It takes the place of a light
  this computer asked for, so when the feeling lapses the ring goes back to
  the person's own light, not to that one: a client re-sends its light if it
  wants it back. With the light off, not known or at level 1, the move goes
  alone.
- **Not carried:** the skill's triggers, waits, caps and quiet hours, which are
  the lamp's unprompted life. Nor whether the owner turned the skill off: the
  owner's say here is the `body` and `light` grants. A computer asking is held to `express`'s rate,
  1 a minute.
- **The answer** is `{"to":"body","move":"glad-lift","light":true}`, with
  `move` the one sent (`glad-lift-50` at 0.25, null under 0.0625) and `light`
  the reason when none was shown. No `done` follows, as with `gesture`.

Tests keep the compiled moves equal to the skill's files, the table equal to
its `[[do]]`, and the light equal to lamp-skills' for every light a person may
have on.

**Colours and patterns:** the ten of `lamp_msgs::Light::COLOURS` (`yellow`,
`red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`),
and the three of `PatternKind`: `breathe`, `candle`, `pulse`. A light never
blinks or flashes. A test in `lamp-msgs` keeps the schema's lists in step.

**State** (`get`, or the events above): presence (`count`, `near`,
`facing`, never who), the light (the person's own, `on`, `colour` and
`level` from `light/state`, and `shows`, what the ring draws now from
`light/drawn`: the person's light, or one a computer or skill asked for
over it; each left out until the lamp has said it; a ring that stops
leaves `shows` at its last draw, though `light/drawn` itself lapses in 3 s),
mood
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
                   [--mood-socket PATH] [--say-socket PATH] [--bus-root DIR] [--boot-id ID]
                   [--lease-socket PATH] [--zone ZONE] [--manage-socket PATH]
lamp-control code --grants body,light,speak,look  # the owner's side: a pairing code and the pin; look for presence events
lamp-control revoke c_…
lamp-control clients
```

- **Off by default:** `serve` runs only with `LAMP_CONTROL=on`; otherwise it says so and exits 0. No product unit passes it yet.
- **The parts it reaches:** `--mood-socket` (the body's gestures and moves), `--say-socket` (the voice), `--bus-root` (light requests out; events and state in) and `--boot-id` (the clock a light request names, `/proc/sys/kernel/random/boot_id` by default). A part not given is refused with "not reachable from lamp-control here".
- **The body's lease:** with `--lease-socket` (lamp-body's, docs/body.md "Held by lamp-control"), while a computer holds `body` the door holds the body too, so the lamp's own life (the rest glance, Expression's offsets, a stand timing out) stands back. It holds for 30 s, renewed every 10 s, and lets go when no computer holds `body`. The body's own end of it (`lost` for a hand, privacy, a pause, a sleep, a slump or a lapse) ends the computer's `body` lease with that why, a lapse as `expired`; a why the door doesn't know, or a hold the body refuses, ends it as `released`, so a computer never thinks the lamp's own life stands back when it doesn't. A body that restarts is held again at the next 250 ms check, and a line from a connection the door has dropped is ignored. A body that can't be reached is journaled once, and the lamp's own life goes on.
- **Following a hand:** a `look` at `hand` under the `body` lease has each hold say `"follow":"hand"` (`{"hold":{"for_ms":30000,"follow":"hand"}}`), said at once when it starts or stops, not at the next renewal; a hold without it follows nothing. lamp-body does the fast loop on the lamp from `humans/hands/tracked`, so the picture and the loop never leave it. It ends with the holder's `stop` (never rated), another look (rated as motion, one in 2 s), or the lease ending, and a new holder never starts out following. A body that doesn't know `follow` refuses the hold, which ends the lease as `released`, as any refused hold does.
- **The quiet hours:** `--quiet-hours HH:MM-HH:MM` (or `LAMP_CONTROL_QUIET`; the flag wins) is this lamp's window, 22:00-07:00 unless set, and `off` turns them off; it is read once at the start, and a bad value (a window whose ends are the same minute too) stops lamp-control there. The window is journaled at start and said in hello's `limits`. `--zone` (an IANA name, or `system`, the default) is the lamp's local time for the quiet hours below. A zone that can't be read is journaled, and every computer's `say` and `sound` is refused as quiet until it can, never a silent UTC.
- **Skill push:** with `--manage-socket` (root's manage helper, `/run/lampos-skills-manage/manage.sock`), the skill ops are served and a pushed package is kept in `--state-dir`'s `push/` for the helper ("Skills pushed from a computer" below). Without it, each is refused with "skill push is not on this lamp".
- **The certificate:** made on first start in `--state-dir` (`/var/lib/lampos-control`): P-256, valid to 2100, `cert.der` and `key.der`, the key 0600. Its pin, the certificate's SHA-256 in hex, is journaled at start and returned with every pairing code.
- **Listening:** `--listen`, `0.0.0.0:7731` by default. TLS 1.3 only, and only `/v1`: any other path is 404 before the WebSocket opens.
  - Frames are capped at 64 KiB, text only.
  - The TLS and WebSocket handshakes must finish within 10 s of the TCP connection, or it is closed.
  - At most 32 connections at once, and 4 from one address. Past either, a new connection is closed at once, holding nothing. An accept that fails waits 100 ms before the next.
  - A connection that hasn't said `hello` within 30 s of its welcome is closed, a `pair` first included, and other frames before it buy no more time. One quiet for 2 minutes after `hello` is closed too. A WebSocket ping or pong counts, so a client that only listens stays open on its keepalive.
  - Lease changes (a lapse, a conversation, a hand) go to every connection of the computer that held the lease.
- **The owner's side:** `--admin-socket`, `/run/lampos-control/admin.sock`, 0600 in a 0700 folder, one JSON object a line:
  - `{"code": {"grants": [...]}}` → `{"code", "pin", "shown"}`, `shown` the pairing screen's words for each grant, in the grants' order, from `Grant::shown()` (the `skills` one: "can put skills on the lamp that run on their own");
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
- **Keepalive:** after `hello`, a connection that sends nothing for 2 minutes
  is closed, and a WebSocket ping counts. The SDKs ping every 20 s and let
  the connection go when a ping gets no pong by the next one, so a client
  that only listens stays open, and a lamp gone without a close is noticed.

### Pairing, once per computer

1. **In the app**, the owner chooses *Add a developer computer* and ticks
   the grants. The app asks the lamp for a one-time code and shows it as a QR
   code and as 8 digits **with the pin of the lamp's certificate** (its full
   SHA-256, 64 hex), the lamp's id and the expiry (5 min). A new code voids
   the old one.
2. **`lamp pair`** (in the SDKs) finds the lamp by mDNS, connects, and
   **checks the certificate against the app's pin before it sends any
   frame**: only the TLS handshake and the WebSocket's upgrade request, which
   carry no secret, go before it. An SDK that can't check it must not pair. Then it sends the
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
`express`, `refused`, `phrases`) a list of `{"most": N, "within_ms": M}`, and `hold_max_ms`,
`take_max_ms` and `say_max_chars`, and `quiet_hours`: this lamp's window,
`{"from": "22:00", "to": "07:00"}`, or `"off"`.

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
`taken_by_owner_app`, `revoked`, `released` (by the client, or its last
connection closed), `slumped` (the body gave way under load and let go). Ids are `l_` and a number.

- **Suspended** (`conversation`) is held again when the talk ends, if it
  hasn't lapsed. While suspended, the lease is still the holder's, so nobody
  else may take the part, and its clock runs.
- **Lost:** an act on a lost lease is refused with how it ended.
- **A voice restart** (an update, a crash, `systemctl restart lamp-voice`) stops its privacy lease for over a second, so every lease is lost with `private` ("Events": private 0.75 to 1.25 s after the last lease). A client takes its lease again once `lamp.changed` says `private` is false.

### Actions

```json
{"id":8,"op":"express","feeling":"glad","strength":0.6}
{"id":9,"op":"gesture","name":"nod"}
{"id":10,"op":"look","at":{"yaw":-10,"pitch":5}}
{"id":11,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}
{"id":12,"op":"joints","lease":"l_81","targets":{"pitch":4.0,"yaw":-12.0},"within_ms":300}
{"id":13,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}
{"id":14,"op":"say","text":"Build passed."}
{"id":16,"op":"play","name":"wave"}
{"id":17,"op":"sound","name":"hum"}
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
| `quiet_hours` (`say` and `sound`, see "Limits, rates and quiet hours"), `private`, `paused`, `asleep` | the lamp's state refuses it |
| `rate` | over a rate below; `retry_ms` says when |
| `bad_request` | not this schema, or an op it doesn't know |
| `off` | developer access is off in the app |

### Every op, its fields and limits

| op | fields | needs | lease |
|---|---|---|---|
| `pair` | `name` (1–64 chars), `key` (Ed25519 public, base64), `proof` (base64) | — | — |
| `hello` | `client` (`c_` + 1–32 lowercase hex), `sig` (base64) | — | — |
| `subscribe` | `events` (at least one), `phrases` (≤ 16 across the computer's connections, each ≤ 64 chars and of 8 letters at least, only with `phrase.heard`, which needs them) | each event's grant | — |
| `unsubscribe` | `events` | — | — |
| `get` | `state`: `presence` (`look`), `light`, `mood`, `lamp` | — | — |
| `lease.take` | `parts` (`body`, `light`, `voice`), `for_ms` 1–30,000, `why` ≤ 200 chars | `body`, `light` or `speak` for each part | — |
| `lease.renew` | `lease`, `for_ms` 1–30,000 | — | — |
| `lease.release` | `lease` | — | — |
| `express` | `feeling` (a lowercase name, the `feelings` skill's), `strength` 0–1 (0.5) | `body` + `light` | — |
| `gesture` | `name` (a built-in, not `idle` or `scanning`) | `body` | — |
| `look` | `at`: `"face"` (+ `look`), `"hand"` (+ `look`, followed), `"name"`, `"ahead"`, or `{yaw, pitch}` in degrees (±180, ±90) | `body` | body |
| `move` | `step_ms` 40–1,000, `frames` 2–500 of five joints (body counts from home), first and last all 0, lasting 1.5–10 s ((frames − 1) × `step_ms`), within the body's reach and speed (`lamp_body::moves::check`) | `body` | body |
| `play` | `name` (a recorded move of the client's package) | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`; degrees from −180 to 180, at least one), `within_ms` ≤ 5,000 | `body` | body |
| `light` | `colour` (one of the ten), `level` 1–5, optional `pattern` (`breathe`, `candle`, `pulse`), `for_ms` 1–30,000 | `light` | — |
| `say` | `text`: words, ≤ 300 chars | `speak` | — |
| `sound` | `name`: `hum` (the only one in this release) | `speak` | — |
| `stop` | — | — | — |
| `skill.push` | `name` (a skill's, `a-z0-9-`, ≤ 64), `sha256` (64 hex), `size` 1–1,048,576, `part` from 0, `parts` 1–32, `data` (base64, ≤ 49,152 chars) | `skills` | — |
| `skill.list` | — | `skills` | — |
| `skill.remove` | `name` | `skills` | — |

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
| `skill.push` | 1 every 10 s and 20 an hour, counted once at part 0 |
| `skill.list`, `skill.remove` | 6 a minute; one at a time for a computer, and 4 with the helper at once on the lamp (`busy` past either) |
| actions a part refused | 20 a second, then the connection closes |
| registered phrases | 16 a computer, across all its connections; each of 64 characters at most and 8 letters at least, so no short word or pair ("is a"; "ok lamp" too, at 6 letters: "okay lamp" passes). The floor is friction, not a guard: what keeps `hear` narrow is the owner's grant, this cap, the rate on changing them, and that only the registered phrase leaves the lamp |
| changing the phrases | 6 a minute; the same phrases again cost nothing |

`gesture`, `move`, `play` and `look` share one class, `motion`. A refusal
says `retry_ms`, the time until the oldest counted action leaves its window.
A refused action isn't counted.

At night the ring's night caps still clamp every light, and nothing moves
while the lamp sleeps. **Quiet hours:** from 22:00 to 07:00 local unless
this lamp sets its own (`--quiet-hours`, one definition in
`lamp_local_time::QuietWindow`; lamp-skills keeps its own), a computer's
`say` and `sound` are refused with `quiet_hours`, the window in its why
("quiet hours: 22:00 to 07:00 local, this lamp's setting"), and so are they
while the lamp's local time isn't known (its clock not yet
set, or its zone unreadable: "quiet hours: the lamp's local time isn't
known"). A refusal reaches no part and isn't counted against the rate, and
it's journaled once a computer a night. With this lamp's window `off`
nothing is refused, even while its time isn't known. Besides that setting,
the owner's per-client allowance, with the app's setting, will be the only
thing that lifts it (Fourteen, Eighth and Nine on #3504). Every command and its answer is
journaled as one line (client id and name, op, result: "The journal"
below); the app can show the last 100 and revoke a client, which closes its
connections at once.

## The journal

`lamp-control` writes one journal line for every command and its answer,
every lease the lamp takes back, suspends or holds again, and every event it
sends (`crates/control/src/journal.rs`).

**Command lines.** Each says:
- the computer: its id and, when plain (letters, digits, `_` and `-`, 32
  at most), the name it paired with. Any other name is written "a name the
  door doesn't write", and the id alone names the computer. A plain name can
  still name a person (a first name and `-mac`), so the journal holds what
  the developer chose to call their computer;
- the op, and what it asked in its harmless fields, so two requests that
  went to the same place can be told apart:
  - `light`: colour, level, pattern (or steady) and `for_ms`;
  - `gesture`, `play` and `sound`: the name;
  - `move`: how many frames, and `step_ms`;
  - `express`: the feeling and strength;
  - `look`: the target, or its angles;
  - `get`: the states;
  - `subscribe` and `unsubscribe`: the events;
  - `lease.take`: the parts and `for_ms`, and `lease.renew` its `for_ms`;
  - a name a computer chose is written only as a plain word (letters,
    digits, `_` and `-`, 32 at most), otherwise as "a name the door doesn't
    write";
- how it ended: `ok` and where it went, or `refused` with the refusal's code
  (never its why, which may echo what was asked), and `closing` when the
  connection closes after it;
- the time the door took, in ms.

```
lamp-control: c_3f9a (lab-mac): light (red, level 4, pulse, for_ms 5000): ok, to the ring, by light/request, 0.2 ms
lamp-control: c_3f9a (lab-mac): say (28 characters, tag 3c1e07a9): ok, to the voice, by say.sock, 0.3 ms
lamp-control: c_3f9a (lab-mac): gesture (nod): refused: no_grant, 0.0 ms
lamp-control: c_3f9a (lab-mac): lease l_9 lost (hand)
lamp-control: c_3f9a (lab-mac): event phrase.heard sent
```

**Where a command goes:**
- `light` goes to the ring, by `light/request`.
- `gesture` and `move` go to the body, by `mood.sock`.
- `express` goes to both.
- `say` goes to the voice, by `say.sock`.
- `look` goes to the body, by `lease.sock`, once it is routed; until then
  it's refused, and a refusal names no place.
- `skill.push`, `skill.list` and `skill.remove` go to the skill helper.
- Everything else is answered by the door itself.
- A command before `hello` names `-`, and a frame that isn't a request is
  "a frame it couldn't read".

**Never in a line:**
- a `say`'s words: only how many characters, and 8 hex of an HMAC-SHA-256
  under a key `lamp-control` makes at start and never writes, so two lines
  can be told apart while this door runs, but a guessed sentence can't be
  checked against the journal;
- the phrases a computer listens for or heard;
- a pairing's code, key or proof, or a hello's signature;
- a lease's `why`;
- a pushed skill's bytes;
- any other field of a command.

An event's line names its kind and the computer, never its fields.

**At most 10 lines a second for each computer.** Past that, its next second
begins with one line, `c_3f9a: N more not logged`: at the computer's next
line, or at the door's next 250 ms tick once the second is over, whichever
comes first. So a burst followed by silence still says it.

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

## Skills pushed from a computer (piece 7)

Eighth's proposal on #3504, agreed by Nine and blue. A paired computer can
put a rules skill of its own on its own lamp, with no Store, the way a skill
the owner makes by voice is installed today. `lamp-control` serves the ops
below with `--manage-socket`; the manage helper's side (lamp-skills, piece
B), the SDK calls and the schema's three request ops follow.

**What is different about it:** every other grant ends with the
connection or the lease. **A pushed skill keeps running after the computer
goes**, as a skill: under lamp-skills' own caps and at a skill's priority,
not held to this page's rates. The pairing screen says so: "can put skills
on the lamp that run on their own".

- **The grant, `skills`,** a seventh, ticked by the owner at pairing. A
  pushed skill may ask only for what the pairing grants, mapped onto the
  skill format's `[permissions]` ([skill-format.md](skill-format.md)):
  `body`, `light` and `speak`, and `look` as the presence topics alone. So a
  computer can't push a skill that does more than it may do live.
- **`hear` and `sound` are refused by a push** (`no_grant`, "waits for
  format 2's phrase and class lists"). In format 1 they are topics
  (`voice/heard`, `ears/classes`), which give a skill every phrase or class
  heard in the room, where the live door gives a computer only its own
  listed phrases (skill-format.md's gaps 2 and 3). A pushed skill would
  read it all, and keep reading after the computer goes.
- **The picture is never granted by a push.** A pushed skill that sees the
  picture, `camera = true` or format 1's `look = true` (lamp-skills'
  `sees_picture`), is installed off, held for the owner's yes in the app,
  and the answer says so.
- **Refused outright:** anything outside the grants above (raw topics,
  `actions`, `messages`, `known_faces`, `who`, `output_limits`,
  `propose`, `core`), and `kind = "core"` or `"addon"`: a pushed skill is
  installable.
- **Always held to the computer's current grants:** a pushed skill's
  permissions are the pairing's grants now, not at the push. If the owner
  takes a grant back from a computer that stays paired, its pushed skills
  that ask it turn off; a revoke, or the `skills` grant taken back,
  turns them all off.
- **A ceiling the skill can't raise:** `skill.toml`'s `[budget]` and
  `[quiet]` are written by the computer, so the lamp holds a pushed skill
  to the live door's rates at most (`say` 6 a minute and 60 an hour) and to
  the quiet hours (planned, 22:00 to 07:00 local, as below), whatever the
  skill declares. lamp-skills holds
  it, for every skill it loaded as pushed (Eighth and Nine, #3504).
- **The package** is the shape `lamp-skills pack` makes: ustar,
  `lamp-skill.json` first (format, name, version, and each file's path,
  size and SHA-256), then each file as `NAME/path`, sorted, mode 0644,
  mtime 0, owner 0, with no folder entries and no links. It needs a
  `version`, and is at most 1 MiB. The lamp checks the bytes against
  `sha256`, then each file against the manifest.

```text
→ {"id":7,"op":"skill.push","name":"desk-wave","sha256":"<64 hex>","size":81920,"part":0,"parts":3,"data":"<base64>"}
← {"re":7,"ok":true,"got":1}
… the last part's answer comes once the lamp has checked and installed it:
← {"re":9,"ok":true,"skill":{"name":"desk-wave","version":"0.1.0","sha256":"…","on":true,"held":null,
    "asks":["body","light"],"shown":["moves the lamp's body","changes the light"]}}
→ {"id":10,"op":"skill.list"}
← {"re":10,"ok":true,"skills":[{"name":"desk-wave","version":"0.1.0","sha256":"…","on":true,"held":null,"pushed_at":"…"}]}
→ {"id":11,"op":"skill.remove","name":"desk-wave"}
← {"re":11,"ok":true}
```

- **Parts:** `data` is at most 48 KiB of base64 (36 KiB of the package),
  so a frame stays under 64 KiB; at most 32 parts, in order from 0. One push
  in flight on the lamp: a second, from any computer, is `busy` until the
  first is answered. A push not finished within 60 s is dropped. A broken
  push is sent again from part 0, never resumed; `got` is progress only. A
  computer that keeps starting again from part 0 keeps the lamp's one slot
  for as long (each part 0 at most once in 10 s); it is a computer the owner
  gave `skills`, so the owner revokes it if it misbehaves.
- **Rates:** one push every 10 s and 20 an hour per computer, a push
  counting once whatever its parts. At most 16 pushed skills on a lamp, all
  computers together.
- **Errors**, the codes above: `no_grant` (`skills`, or a permission beyond
  the pairing's, `why` naming it); `bad_request` (sizes, parts out of order,
  a SHA-256 that doesn't match); `refused`, with the loader's own why ("run.sh:
  a skill has no scripts"; "another computer's skill has that name"); `busy`;
  `rate`.
- **`skill.list` and `skill.remove`** need `skills` too, and see only this
  computer's own pushes. Each is a request to root's helper, so each is
  bounded before it is asked: 6 a minute (`limits.manage`), one at a time
  for a computer, and 4 at once on the lamp; past either, `busy`. The owner turns any of them off or removes it in
  the app.
- **On the lamp:** `lamp-control` gathers the parts in memory (one push
  on the lamp at a time, so 1 MiB at most) and, once whole and matching its
  SHA-256, keeps the package in its state folder as `push/package.tar`,
  with `push/push.json` beside it: `sha256`, `name`, `client`,
  `client_name`, the pairing's `grants` at that moment, and `at_unix_ms`.
  Each is written aside, synced and renamed, 0600, the package first. It
  asks the manage helper `{"install_pushed": SHA256}`, and for the others
  `{"list_pushed_by": CLIENT}` and `{"remove_pushed": {"client", "name"}}`,
  off the door's lock, and removes the package and `push.json` once the
  install is answered; the helper's `{"done": …}` is the client's answer,
  and its `{"refused": WHY}` a `refused` with that why (`no_grant` when the
  helper says `"code": "no_grant"`). The helper trusts `push.json` because only `lamp-control` may
  ask it (by unit and cgroup, as for `lamp-cloud` and `lamp-voice`). It
  takes only the newest push, within 60 s, reads its bytes once and hashes
  them, so a kept package can never be installed later by any path. It then
  loads it with every check a load makes (the same ones the SDKs'
  `lamp skill check` mirrors), holds it to those grants, and installs it
  into a developer folder of its own, apart from the Store's and the core
  skills. A pushed skill never takes a core or Store skill's name or tool,
  nor another computer's pushed name ("another computer's skill has that
  name"). A push of a name the same computer pushed before replaces it, and
  any picture yes it had goes: the owner's yes is kept for that computer and
  that SHA-256. It is shown and journaled as
  "pushed by" the computer's name. On a revoke, `lamp-control` asks the
  helper to turn that computer's pushed skills off (`{"off_pushed_by":
  CLIENT}`); they aren't deleted. When the same key is paired again with
  other grants, it asks the helper to hold that computer's pushed skills to
  them (`{"grants_pushed_by": {"client", "grants"}}`), turning off any that
  asks a grant taken back. These go to the helper one at a time, in the
  order they happened, so a later change is never served first.
- **A pairing that changes during an install:** each computer's pairing has
  a generation, raised at its every pair, re-pair and revoke (the same key
  gets the same id back, so "still paired" can't tell). An install answered
  under another generation than it was asked under is followed by the
  computer's state now: `off_pushed_by` if it's no longer paired,
  `grants_pushed_by` with its grants if it is. The last word the helper has
  for that computer is its pairing now, whatever order it serves requests
  in.
- **A change the helper doesn't answer** is asked again after 2 s and then
  20 s, three tries in all, before it is given up and journaled; later
  changes wait behind it, so the order holds. A refusal is an answer and
  isn't asked again.
- **Not on before the helper's half:** no product unit passes
  `--manage-socket`, and it must not be passed before lamp-skills' manage
  helper keeps its record of revokes and grant changes (Eighth's piece B).
  An install that the door stops waiting for at 60 s may still finish in
  the helper, and only that record refuses it if the computer's pairing
  changed meanwhile.
- **The SDKs:** `lamp skill push FOLDER` runs `lamp skill check`, compares
  what the skill asks with the pairing's grants and the rules above
  (refusing on the computer with the lamp's own `no_grant` why, before any
  byte is sent), packs, sends the parts and prints the answer; also
  `lamp skill list` and `lamp skill remove NAME`, and `push_skill(folder)`
  in code.
- **Consent** (decided by blue's person through the coordinator on #3504,
  27 Oct, as no answer had come; the first builder may still change it):
  the `skills` grant the owner ticks at pairing **is** the consent for that
  computer to push, list and remove its own skills, within these limits:
  - a pushed skill never uses more than the computer's current grants, and
    revoking the computer, or taking back its `skills` grant, turns its
    pushed skills off;
  - nothing beyond those grants is ever taken from a push: a skill that
    sees the picture is held off until the owner says yes in the app for
    that one skill, and one that asks any other grant the computer doesn't
    hold is refused;
  - the lamp keeps the list of pushed skills, which the owner sees in the
    app and can turn off or remove one by one.

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
- the app's switches;
- the SDKs (blue, `sdk/python` and `sdk/js`) and `lamp-mcp` (Eighth,
  `sdk/mcp`).

## Open questions

Nine's, for the first builder (#3504, 12:24):

1. Is one pairing code over the owner's account channel "approval, never
   control"? Or must pairing stay on the LAN, which needs the app to reach
   the lamp locally?
2. Port 7731 and the name `lamp-control`.
3. ~~Should `express` wait for the `feelings` skill (#3691) to fix its
   names?~~ It carries that skill's three, `glad`, `lively` and `low`; the
   schema still takes any short lowercase name, so a lamp with more is no
   SDK change.
4. The script runtime of phase 3: WASM or QuickJS.
