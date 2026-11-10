# Control: programming the lamp from your own code

**New here?** [develop.md](develop.md) is the quick-start: pair, a first program in Python or JavaScript, lamp-mcp, and a skill pushed and shared, in 10 minutes (skill push is not yet on customer lamps: "Skills pushed from a computer").

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
- **piece 4**: the routing (`parts.rs`). `gesture` and `move` go to the body's mood socket, `light` is a `light/request` on the bus, `say` goes to the voice's say socket with its outcome as a `done` event (a computer's `stop` cancels its own say there, waiting or playing), `sound` is the voice's built-in short hum through the same socket and gates, or a clip of the computer's own (`sounds.rs`, "Sounds" below), played as an exact `say` is, and `express` is the `feelings` skill's own move and light (`feelings.rs`). Each is at `skill` priority as `lamp-control`, held to the part's own checks. `look` at `ahead` or `{yaw, pitch}` goes on the body's lease socket under the lease the door holds (`body_lease.rs`), its end a `done` with the angles reached; `look` at `hand` has the body follow the hand in view, said with the body's lease (below). `look` at `face` or `name` is refused as out of this release ("not in this release: look at a face or a name"); `joints` and `play` are refused with what they wait for. A computer's last connection closing, or a revoke, stops what it asked for, and actions a part refuses are bounded at 20 a second.
- **piece 5**: the bus as events, lease signals and state (`watch.rs`, with `--bus-root`). Events go out by edge, never per message: someone arrived or left, turned to or from the lamp, or stood up; the lamp's name heard; a sound class past its bar (never `speech`); a touch or the button; the lamp talking, paused, private or asleep. Each goes only to a connection subscribed to it that holds its grant. Words heard are matched on the lamp against each connection's own phrases, and only the phrase leaves it. Talking suspends every lease and resting holds them again; a hand on the body, a stop, a pause, privacy or sleep loses them. `get` answers presence (counts, never who), the light, the inner state and the lamp's own.
- **piece 6**: the body's lease (`body_lease.rs`): with `--lease-socket`, a computer's `body` lease holds lamp-body's own, so the lamp's own life stands back, and the body's end of it is the computer's (`slumped` added to the whys).
- **piece 7**: skills pushed from a computer (`push.rs`, with `--manage-socket`): the `skills` grant, `skill.push` in parts, `skill.list` and `skill.remove`, each asked of root's manage helper off the door's lock, and a revoke turning the computer's pushed skills off. The helper's side is lamp-skills' (Eighth's piece B); until it is in, the helper refuses what it doesn't know.

**The product unit `deploy/product/lamp-control.service` carries it**, as `lampos`, installed and enabled on every lamp
and off unless `LAMP_CONTROL=on`, so nothing listens until the owner turns developer access on in the app (`control.set`,
which writes the unit's drop-in and restarts it; off removes it and stops it); discovery is the next piece. This
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
| `speak` | `say`, `sound`, `turn.end`, `volume`; with `hear`, `turn.start`; the `volume` state and `volume.changed` |
| `look` | presence events and state: arrivals, leavings, facing, standing up, a hand in view. Positions, never who |
| `hear` | `phrase.heard` for the client's own phrases (never the words around them), `name.heard`, `listen.set` (its own mute); with `speak`, `turn.start` |
| `sound` | `sound.heard`, by class |
| `skills` | `skill.push`, `skill.list`, `skill.remove`: its own skills put on the lamp, which run on their own after it goes, never beyond its other grants ("Skills pushed from a computer") |
| `air` | `air.changed` and `get` `air`: the air sensor's readings (particles, carbon dioxide, humidity, temperature at the sensor). Carbon dioxide and humidity rise when someone comes in and stays, so they can show when someone is in the room, even to a computer without `look`: the pairing screen says so |
| `device` | `get` `device`: the lamp's own computer (processor load, memory, disk, its temperatures) |
| `camera` | `camera.snapshot`, `camera.stream`: the camera's pictures, the eyes' own newest frame, never while the lamp is private ("The camera") |
| `microphone` | `microphone.stream`: the room's sound as the ears hear it, 20 ms blocks, never while the lamp is private ("The microphone") |

**Events**, pushed to a client that subscribed:

| event | fields | from | grant |
|---|---|---|---|
| `person.arrived`, `person.left` | `count`, `near` (share of the picture's height) | `humans/faces/tracked`, as skills' `appears` and `absent_for` | `look` |
| `person.stood_up` | `side` | `room/stood_up` | `look` |
| `person.facing` | `facing` | `humans/faces/tracked` | `look` |
| `hand` | `seen`; when seen, `x`, `y` (the palm's centre, a share of the picture from its left and top), `size` (the palm's width, a share of the picture's), `open`, `confidence` | `humans/hands/tracked`: one message per look that sees a hand, none without. Told when first seen, moved or grown by 0.05, opened or closed, at most every 250 ms with the newest held back until then; `seen: false` once, 1 s after the last message. A message under 0.5 confidence, or a share outside 0 to 1, is no hand. Never the picture, keypoints or the tracker's clock | `look` |
| `phrase.heard` | `phrase`: one the client registered | `voice/heard`, matched on the lamp: the client is told which phrase, **never what was said** | `hear` |
| `name.heard` | `score` (the listener's, 0–1 to a hundredth; left out when the bus gives none). A sound like the name can pass the lamp's own bar (0.9): lamp-75c9 woke at 0.941 on no words on 9 Nov, while real calls scored 0.90 to 1.00, so a program acting on its own sets its own bar | `name/heard` | `hear` |
| `sound.heard` | `class` (`knock`, `music`, …), `score` | `ears/classes` | `sound` |
| `touch` | `kind` (`tap`, `pet`, `hold`), `where` | `controls/touch` (a head contact let go quickly is a `tap`), `controls/pet` (one `pet` for a contact), `controls/gesture` (a button's or the base's tap), `body/hand`. A head `hold` is not produced yet | none |
| `button` | `press` (`press`, `long_press`) | `controls/gesture` | none |
| `time` | the client's own schedule | the lamp's clock and zone | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `quiet_hours` (planned, not produced yet), `lease` | `voice/state`, `lamp/pause`, privacy, `voice/mute`, the leases | none |
| `body.state` | `joints`: `pitch`, `roll`, `yaw`, `shoulder`, `elbow`, each `deg`, `counts`, `goal_deg`, `load`, `moving`, `temp_c`; `lamp`: `light`, `talking`, `private` ("Where the body is") | `body/joint_states`, the body's `diagnostics`, the light and the voice | `body` (proposed: a read-only `body_state`, Queue #3997) |
| `air.changed` | `pm1_0`, `pm2_5`, `pm4_0`, `pm10` (µg/m³), `co2_ppm`, `humidity` (%), `temperature` (°C at the sensor, inside the base), each to a tenth (carbon dioxide whole) and left out while the sensor doesn't know it; `feel` (`fresh`, `fine`, `stuffy`, `very_stuffy`, `unknown`) | `air/state`, every 5 s. Told on the first reading, then when a value moves by its step (particles 5, carbon dioxide 50 ppm, humidity 3, temperature 0.5 °C, `watch::AIR_STEPS`), comes or goes, or `feel` changes; at most every 5 s, the newest held back until then | `air` |
| `volume.changed` | `level` (0–100 of what the lamp allows), `muted` | `speaker/volume`, which lamp-actions says after each change: the person's by voice or in the app, or a computer's | `speak` |

lamp-control reads each topic on its own thread. A state topic's message that says nothing new for these, the same privacy, pause, sleep, talking, light or mood as the last one, is passed over on that thread and never handed to the watch that turns the bus into events; a repeat still goes on every 250 ms. `voice/privacy` alone comes a hundred times a second (its lease, renewed every 10 ms), and handing each one on was half of lamp-control's idle CPU on lamp-75c9 (2.1% of a core before, 1.1% after; the rest is that thread reading them). A privacy lease not handed on for 1 s counts as private: every lease is lost with `private` and `lamp.changed` says so, until the lease is heard again. That is later than the voice's own microphone gate, which closes once a lease is 50 ms old; lamp-control says it 0.75 to 1.25 s after the last lease heard (1 s after the last one handed on, which is up to 250 ms older, and then the next 250 ms check), so a running voice, renewing every 10 ms, never reads as private. Each message is timed when its thread receives it, and every message already come is taken before a lapse is judged, so a lamp-control held up by a slow save doesn't read a renewed lease as lapsed. Before the first lease nothing lapses, so a lamp whose voice publishes none is never private for that.

**Actions:**

| op | args | carried out by | grant | lease |
|---|---|---|---|---|
| `express` | `feeling`, `strength` 0–1 (0.5) | the `feelings` skill's (#3691) move on `mood.sock` `{move}` and its light as a `light/request`, so one set of feelings (below). One action: its move replaces the motion playing, as `gesture` and `move` do (below); below strength 0.0625 there's no move, and the light goes alone | `body`, `light` | no |
| `gesture` | `name`, a body clip (below; any lowercase name, the body answers one it doesn't have) | `mood.sock` `{gesture}` | `body` | no |
| `look` | `at`: `face`, `hand`, `name`, `ahead`, or `{yaw, pitch}` in degrees (yaw right and pitch up positive; any finite number, the body clamps it to its reach). `hand` follows it until `stop`, another look or the lease ends | `ahead` and `{yaw, pitch}`: the body's lease socket `{look}`, eased to and held while the lease is, inside the gaze's reach and the pose's bounds (below); `hand` with the lease socket's hold; `face` and `name` refused, "not in this release: look at a face or a name" | `body` (and `look` for `face` and `hand`) | body |
| `move` | `step_ms` 40–1,000, `frames` (2 at least; at most the body's 10 s allows, 251 at a 40 ms step) of 5 joints in counts, first and last all 0, lasting 1.5–10 s, each joint within the body's reach and speed | `mood.sock` `{move}`; the door holds it to `moves.rs`'s check first, so a move the body would drop is refused with its words | `body` | body |
| `play` | `name` of a move in the client's installed package | **today:** refused, "not routed yet: waits for client packages". Later: the same, from its `moves/NAME.json` | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`, degrees from home; one at least), `within_ms` (any, 0 or more) | **today:** `yaw` and `pitch` go as the holder's look on the body's lease socket, said `"from":"home"`: the body takes its pose's offset off, so on a standing lamp too they're the degrees `get` `body` says, cut by the body from its pose as every look is, with the look's `done` in the same frame (a joint not named stays where `get` `body` says it is; before the body has said its limits, name both). `roll`, `shoulder` and `elbow` go as the holder's pose on the same socket (`{"pose":{…}}`, lamp-body's `Posing`, docs/body.md), in degrees as `get` `body` counts them (the roll from its found home, the arm from the folded arm): the body takes a pose only while it stands still at its full height, inside the arm's window and the anti-tip rule, and its `done` is `finished` with `roll`, `shoulder`, `elbow` as reached and `clamped`, or `dropped` with why (`refused` by the body: not standing, say), or `cut_short`; an arm joint not named stays where the pose has it. A `joints` naming the head and the arm sends both and has one `done`, `finished` only when both are. Each line carries the door's tag (`"tag":N`) for a body that says it says tags back (`"tags":true` on its lease answer), and each answer is matched by it; to an older body, one pose at a time: a second while one is owed is `dropped`, "another pose is still owed, and this body can't tell its answers apart; ask again once it is answered". **Not a goal stream yet:** `within_ms` isn't used (the body eases at its own pace), and each `joints` replaces the look and the pose before it, whose `done` comes `cut_short`, so send one per move, not 40 a second | `body` | body, named |
| `light` | `colour` (one of 10), `level` 1–5, `pattern`, `for_ms` ≤ 30 s | `light/request` at the priority `lamp-control` stamps | `light` | no |
| `light` with `rgb` or `pixels` | `rgb` (`#rrggbb`), or `pixels` (one `#rrggbb` per pixel, hello's `limits.ring.pixels` of them), `brightness` 0–100, `fade_ms`, `for_ms` ≤ 30 s | `light/request` with `pixels`, at the same priority and claim as a named light, and the nearest named colour and level beside them for a ring that draws none ("The ring, pixel by pixel") | `light` | no |
| `say` | `text`: words, at most 900 UTF-8 bytes, the voice's own cap (a longer one is refused at the door, not sent and dropped); optional `rephrase` | `say.sock` with `exact: true`: the words as sent, rendered straight through the voice's speech service (no model, no tools) and played as the lamp's own line once nobody is talking with the lamp (`say::may_play_exact`: it waits while a conversation with the lamp is going on, its speaker is busy, or an answer or reminder is owed, never for speech in the room that nobody addresses to the lamp; the keeper's pause and the privacy switch refuse it; no quiet gap or rarity). With `rephrase: true`, a written turn for the voice's model, through its quiet after a turn, privacy and busy gates, as before | `speak` | no |
| `sound` | `name`: `hum`, the voice's built-in short hum (no file, no model), or one of the computer's own clips by name ("Sounds" below); the recorded notices are never a computer's, and no path is ever asked | `hum`: `say.sock` `{cue: "hum"}`, with say's quiet, rarity, privacy and busy gates. A clip: `say.sock` `{clip: PATH}`, its path in the clips folder, read by the voice with a skill sound's format and loudness checks and up to 30 s (`say::read_clip`) and played as an exact `say` is (`say::may_play_exact`). Its `done` follows | `speak` | no |
| `sound.upload`, `sound.list`, `sound.remove` | a clip of the computer's own, kept, listed or deleted ("Sounds" below) | the state folder's `sounds/` | `speak` | no |
| `volume` | one of `to` (0–100), `step` (`up`, `down`) or `mute` (`true`, `false`) | lamp-actions' `call.sock` (`--actions-socket`): `volume_to`, `volume_step` or `volume_muted`, the speaker's only writer, so the person's own volume within its ceiling; the reply is the level it left, `{level, muted}` | `speak` | no |
| `turn.start` | `prompt` (optional) | `say.sock` `{turn: "start"}`, with its prompt as `text` ("Turns and listening") | `speak`, `hear` | no |
| `turn.end` | | `say.sock` `{turn: "end", id}`, the id of this computer's own turn | `speak` | no |
| `listen.set` | `on` | `say.sock` `{listen}` on one connection the door holds for the computer | `hear` | no |
| `stop` | | ends the client's own actions now (not its mute): gives its light back, ends its hand follow, and cancels each `say` of its own still waiting or playing (`{"cancel":true}` on the say socket connection that asked it; the voice lets a waiting one go and cuts a playing one, that turn alone), its `done` then `dropped`, "dropped: stopped by its asker". Never another computer's say, the person's turn or the voice's own words | none | no |

**Gestures** offered to programs: `nod`, `headshake`, `greeting`, `laugh`,
`shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`,
`surprised`, `confused`, `sleepy`, `excited`, and the five dances
`dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`.
These are the body's own clips (`crates/body/src/clips/data.rs`) but two,
`idle`, the lamp's own life, and `scanning`, a room sweep. The door takes
those two as well (the owner's "no limits for developers", 30 Oct): it
checks only a name's shape, and the body answers a name it doesn't have.
A program that sweeps a room unasked reads as a machine (CLAUDE.md's rule
8), so keep `scanning` for when a person asks. A test in `lamp-body` keeps
this list (the schema's gesture `examples`) in step with the clips.

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
  wants it back. At level 1 `glad` and `lively` warm a white light alone,
  steady at 1, as the skill's `warm_at_dimmest` does (#3691). With the light
  off or not known, or at level 1 already warm (and always for `low`), the
  move goes alone.
- **Not carried:** the skill's triggers, waits, caps and quiet hours, which are
  the lamp's unprompted life. Nor whether the owner turned the skill off: the
  owner's say here is the `body` and `light` grants. A computer asking is held to `express`'s rate,
  20 a minute.
- **The answer** is `{"to":"body","move":"glad-lift","light":true}`, with
  `move` the one sent (`glad-lift-50` at 0.25, null under 0.0625) and `light`
  the reason when none was shown. No `done` follows, as with `gesture`.

Tests keep the compiled moves equal to the skill's files, the table equal to
its `[[do]]`, and the light equal to lamp-skills' for every light a person may
have on.

**Looking** (`look`, `crates/body/src/lease.rs` `Looking`, the door's
`body_lease.rs`): with `--lease-socket` on both lamp-body and lamp-control,
a look goes to the body as `{"look":{"yaw":Y,"pitch":P}}` or
`{"look":"ahead"}` on the lease connection the door holds for the computer,
so it never outlives that lease, and only the holder's connection is read.
- **Where:** degrees become counts (4,096 a turn). The base turns right with
  more counts, as every measured `--gaze` yaw has it, and the head looks up
  with fewer. They are cut to the base's
  reach (`--reach`'s yaw, or the gaze's `--gaze-reach` if larger) and the head's
  (`--reach` up and down), and every pose's own bounds still hold.
- **How:** eased to as the gaze's own look is, in its place while held, and
  held there while the lease is. When the lease ends or is let go, the head
  goes back to the pose's own direction. A conversation cuts a look short at
  once (`cut_short`, why `conversation`) but keeps its aim: when the talk ends
  and the lease is held again, the head goes back to it, with no new `done`.
- **Its end:** the body answers each look once: `there` when the head and
  base are within 6 counts of where the look sent them and have stopped
  moving for 10 steps (0.2 s); `there` too when they stopped within 24 counts
  (`REST_SLACK`, a rest's own slack) and stayed still for 50 steps (1 s), with
  the angles where they stopped and `clamped` when that is more than half a
  degree from the look (a resting head stops short against its own friction
  or weight: lamp-75c9 on 7 Nov, 17 counts); or `cut_short`, after 5 s with
  how many counts short. The door makes that the computer's `done`.
- **On a product lamp** lamp-control's product unit passes `--lease-socket`
  (`/run/lampos-body/lease.sock`, #4526) and the product lamp-body unit opens
  it (#4568). Both are on stable 10824. A lamp-control without the socket
  retries it, and `look` is refused as the body not reachable.

**Colours and patterns:** the ten of `lamp_msgs::Light::COLOURS` (`yellow`,
`red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`),
and the three of `PatternKind`: `breathe`, `candle`, `pulse`. A light never
blinks or flashes. A test in `lamp-msgs` keeps the schema's lists in step.
- **Levels:** each pattern has a least level, checked before the ring
  (`LightPattern::check`, `parts.rs`). A `breathe` needs level 2 or up, since its
  low is level 1. A `candle` needs 3 or up, and a `pulse` 2 or up. Under that the
  light is `refused` with the ring's own words.
- **Pulses:** the ring plays at most one `pulse` a minute for one computer
  (`PULSE_GAP_US`, counted from when a pulse was drawn). A second within the
  minute is still shown, its colour and level steady, but without its pulse
  (`ring_light.rs`'s `start_moving` starts it "as already played"). The door has
  answered `{"to":"ring"}` by then, so nothing says so. A program that renews a
  pulsing light keeps its colour lit, but it pulses once a minute at most
  (lamp-a0ae, 29 Oct).

**State** (`get`, or the events above): presence (`count`, `near`,
`facing`, never who), the light (the person's own, `on`, `colour` and
`level` from `light/state`, and `shows`, what the ring draws now from
`light/drawn`: the person's light, or one a computer or skill asked for
over it; each left out until the lamp has said it; a ring that stops
leaves `shows` at its last draw, though `light/drawn` itself lapses in 3 s),
mood
(`mood/state`'s energy, warmth, awake, asleep), and `lamp_state`.

**Never through this door:** transcripts, face prints or names. The
camera's pictures go only with the `camera` grant ("The camera"), and the
microphone's sound only with the `microphone` grant ("The microphone"); each
has its own line on the consent screen.

## The doors, and when

| door | for | phase |
|---|---|---|
| Live local API: WebSocket JSON over TLS, paired keys (mDNS planned, not announced yet) | a developer's own code | 1 |
| MCP: the same API as MCP tools, served on the developer's computer | Claude, ChatGPT and other agents | 1 |
| SDKs: thin Python and JavaScript clients | developers | 1 |
| Rules skills: today's `skill.toml`, published as a versioned format | anyone, no code | 2 |
| Store: signed packages, permissions shown first | everyone | 2 |
| Script skills: code on the lamp, sandboxed, the same protocol on a local socket | developers who want it to run without their computer | 3 |
| No-code editing by voice and the app | everyone | 4 |

## Running the door (piece 3)

```text
lamp-control serve [--state-dir DIR] [--listen ADDR] [--admin-socket PATH] [--name NAME]
                   [--mood-socket PATH] [--say-socket PATH] [--actions-socket PATH] [--bus-root DIR] [--boot-id ID]
                   [--lease-socket PATH] [--zone ZONE] [--manage-socket PATH]
lamp-control code --grants body,light,speak,look  # the owner's side: a pairing code and the pin; look for presence events
lamp-control revoke c_…
lamp-control clients
```

- **Off by default:** `serve` runs only with `LAMP_CONTROL=on`; otherwise it says so and exits 0. The product unit (`deploy/product/lamp-control.service`, as `lampos` in `lampos-parts.slice`) doesn't set it.
- **The owner's developer setting:** the owner turns developer access on or off from the app, with no ssh: `control.set {"on": true|false}` (cloud.md, with `LAMP_SETTINGS_CHANGE=on`) goes to root's change helper (setup.md, "Changed after setup"), which acts on lamp-update's lock (never during an update's round) but never waits for the lamp to be quiet, since lamp-control's restart makes no sound. **On** writes `/etc/systemd/system/lamp-control.service.d/developer.conf` (`[Service]` / `Environment=LAMP_CONTROL=on`, 0644) and answers `success`, then, once no update is at work, runs `systemctl daemon-reload` and `systemctl restart lamp-control.service`: the door opens within moments after `success`, or once an update in progress finishes (at most 30 minutes, else at lamp-control's next start). **Off** closes the door before the app is told `success`: it takes lamp-update's lock (at most 8 s, under lamp-cloud's 10 s wait), removes the drop-in, runs `systemctl daemon-reload` and `systemctl stop lamp-control.service`, and only then answers `{"on": false, "closed": true}`; while an update installs it is refused, "an update is installing: try again in a few minutes", with nothing changed; a stop that fails is answered `{"on": false, "closed": false, "note": …}`, saying the door may stay open until the lamp next restarts. Off with no drop-in is a plain `success`. Off by default: no drop-in until the owner turns it on, and a reset wipes it.
- **The parts it reaches:** `--mood-socket` (the body's gestures and moves), `--say-socket` (the voice), `--actions-socket` (lamp-actions' `call.sock`, for `volume`), `--bus-root` (light requests out; events and state in) and `--boot-id` (the clock a light request names, `/proc/sys/kernel/random/boot_id` by default). A part not given is refused with "not reachable from lamp-control here".
- **The body's lease:** with `--lease-socket` (lamp-body's, docs/body.md "Held by lamp-control"), while a computer holds `body` the door holds the body too, so the lamp's own life (the rest glance, Expression's offsets, a stand timing out) stands back. It holds for 30 s, renewed every 10 s, and lets go when no computer holds `body`. The body's own end of it (`lost` for a hand, privacy, a pause, a sleep, a slump or a lapse) ends the computer's `body` lease with that why, a lapse as `expired`; a why the door doesn't know, or a hold the body refuses, ends it as `released`, so a computer never thinks the lamp's own life stands back when it doesn't. A body that restarts is held again at the next 250 ms check, and a line from a connection the door has dropped is ignored. A body that can't be reached is journaled once, and the lamp's own life goes on.
- **Following a hand:** a `look` at `hand` under the `body` lease has each hold say `"follow":"hand"` (`{"hold":{"for_ms":30000,"follow":"hand"}}`), said at once when it starts or stops, not at the next renewal; a hold without it follows nothing. lamp-body does the fast loop on the lamp from `humans/hands/tracked`, so the picture and the loop never leave it. It ends with the holder's `stop` (never rated), another look once it is taken (rated as motion, one a second; a refused look, such as one at `face`, leaves it following), or the lease ending, and a new holder never starts out following. A body that doesn't know `follow` refuses the hold, which ends the lease as `released`, as any refused hold does.
- **Rates off, the default** (the person's decision, 30 Oct 02:1x: "remove the rate"): unless `--rates default` (or `LAMP_CONTROL_RATES=default`; the flag wins; `off` is the default) turns them back on, lamp-control lifts every rate in the table below: `express`, motion, `joints`, `light`, `say`/`sound`, `skill.push`, `skill.list`/`skill.remove`. Two stay: the refused actions' guard (20 a second, then the connection closes), and the phrase changes (6 a minute), which bound `hear` for privacy, not pace: lifted, a program could cycle a word list to learn what people near the lamp say. Hello's `limits` then says `"rates": "off"`, with only the `refused` and `phrases` windows, so a client doesn't pace itself to rates the door doesn't hold. The body still bounds every move itself (below). It's read once at start, journaled ("rates off" or "rates on"), and a bad value stops lamp-control. The product unit sets neither, so it runs with the rates off.
- **The quiet hours, off by default** (the owner's "no limits for developers", 30 Oct): `--quiet-hours HH:MM-HH:MM` (or `LAMP_CONTROL_QUIET`; the flag wins) sets this lamp's window and turns them on; unset, or `off`, they are off. It is read once at the start, and a bad value (a window whose ends are the same minute too) stops lamp-control there. The window, or `off`, is journaled at start and said in hello's `limits`. `--zone` (an IANA name, or `system`, the default) is the lamp's local time for the quiet hours below. With a window set, a zone that can't be read is journaled, and every computer's `say` and `sound` is refused as quiet until it can, never a silent UTC; with them off, the time is never asked and nothing is refused.
- **Skill push:** with `--manage-socket` (root's manage helper, `/run/lampos-skills-manage/manage.sock`), the skill ops are served and a pushed package is kept in `--state-dir`'s `push/` for the helper ("Skills pushed from a computer" below). Without it, each is refused with "skill push is not on this lamp". The product unit doesn't pass it, so skill push is not yet on customer lamps.
- **The certificate:** made on first start in `--state-dir` (`/var/lib/lampos-control`): P-256, valid to 2100, `cert.der` and `key.der`, the key 0600. Its pin, the certificate's SHA-256 in hex, is journaled at start and returned with every pairing code.
- **Listening:** `--listen`, `0.0.0.0:7731` by default. TLS 1.3 only, and only `/v1`: any other path is 404 before the WebSocket opens.
  - Frames are capped at 64 KiB, text only.
  - The TLS and WebSocket handshakes must finish within 10 s of the TCP connection, or it is closed.
  - At most 32 connections at once, and 4 from one address. Past either, a new connection is closed at once, holding nothing. An accept that fails waits 100 ms before the next.
  - A connection that hasn't said `hello` within 30 s of its welcome is closed, a `pair` first included, and other frames before it buy no more time. One quiet for 2 minutes after `hello` is closed too. A WebSocket ping or pong counts, so a client that only listens stays open on its keepalive.
  - Lease changes (a lapse, a conversation, a hand) go to every connection of the computer that held the lease.
- **The owner's side:** `--admin-socket`, `/run/lampos-control/admin.sock`, 0600 in a 0700 folder, one JSON object a line:
  - `{"code": {"grants": [...]}}` → `{"code", "pin", "shown", "until_unix_ms"}`, `shown` the pairing screen's words for each grant, in the grants' order, from `Grant::shown()` (the `skills` one: "can put skills on the lamp that run on their own"). `until_unix_ms` is when the code runs out by the wall clock: the code lapses 5 min from now by the lamp's monotonic clock, so this is the wall clock now plus 5 min. With `"say_pin": true` (lamp-cloud's `control.code` asks it), the lamp also says the first 8 characters of its pin aloud, as the lamp's own exact `say` ("My pairing check is 3 F 9 A, 0 B 2 C."), and the answer adds `"pin_said"`: whether the voice took it (the spoken pin check, #5299). The pin is in no journal line;
  - `{"take_back": true}` (and `"client": "c_…"` for one computer) → `{"taken": N}`: every lease of that computer, or of every computer, is lost as `taken_by_owner_app` and its holder told; the computers stay paired. The owner's way back from the app, since a lease has no renewal ceiling;
  - `{"cancel_code": true}` → `{"dropped": true|false}`: the code out is dropped, so a pairing with it is refused ("no pairing code is out"), and whether there was one;
  - `{"revoke": "c_…"}` → `{"revoked": true, "saved": true}`. The revoke takes effect at once either way. If the file can't take it yet, the answer is `"saved": false` with an `error`, because a restart before a save succeeds brings the key back; the next save that succeeds writes it out;
  - `{"clients": true}` → `{"clients": [{"id", "name", "grants", "paired_at", "paired_at_unix_ms"}]}`. `paired_at` is Unix seconds, as `clients.json` keeps it, left for older readers; `paired_at_unix_ms` is the same in milliseconds.

  **The app's path:** the app reaches it through lamp-cloud, given the socket with `--control-admin-socket` (docs/cloud.md, "The developer door"): `control.code`, `control.clients`, `control.revoke`, `control.code_cancel` and `control.take_back`. `control.code`'s code and pin go back to the app sealed to a one-time key the app sends (HPKE), never in the clear on the company link; what that does and does not stop is in docs/cloud.md, "The developer door".

## The wire

**Where:** `wss://<lamp>.local:7731/v1`, served by `lamp-control`. Nothing
listens until the owner turns developer access on in the app. Script skills
(phase 3) use the same messages on `/run/lampos-control/control.sock`.

**Finding the lamp:** planned: mDNS `_lampos._tcp`, with TXT `id`, `name`,
`v=1` and `fp`: the first 16 hex characters of the SHA-256 of the lamp's
certificate. The SDKs look for it, but nothing on the lamp announces it yet,
on any build: `lamp find` finds nothing, and pairing needs the lamp's address,
`--host <lamp>.local` (the lamp's own host name, such as `lamp-75c9.local`)
or its IP address. The SDKs keep the address with the pairing, so later
connects need nothing. The certificate is self-signed, made per lamp on its
first start, and the client pins it at pairing.

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
- An event: `{"ev": "...", ...}`. The events about the room carry `at`, in
  UTC as RFC 3339 with `Z` (`"2026-10-27T08:40:01Z"`, `server::rfc3339_utc`);
  a client converts it to local time itself. `welcome`, `lease` and `done`
  may not carry it.
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
2. **`lamp pair`** (in the SDKs) connects to the lamp at `--host` (mDNS
   would find it without, but no lamp announces it yet: "Finding the lamp"), and
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
   and the same key pairing again replaces itself; a grant the file names
   that this build doesn't know, a newer lamp-control's kept before an
   update went back, is never granted and is written back as it was, so an
   older lamp-control still starts and going forward again keeps it), answers
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
connection closes. `limits` says the rates the door holds, so an
SDK can keep under them. With the rates off, the default, it says `"rates": "off"` and only the `refused` and `phrases` windows. With `--rates default`, it gives for each class (`joints`, `motion`, `light`, `say`,
`express`, `refused`, `phrases`) a list of `{"most": N, "within_ms": M}`. Either way it says
`hold_max_ms`, a light's longest `for_ms` (30000, as before, so a client that sizes a light or a renewal from it still works), `lease_max_ms`, the longest one lease take or renewal asks for (600000), `say_max_bytes`, the longest say's text in
UTF-8 bytes (900, the voice's own cap, lamp-voice's `MOST_WORDS_BYTES`), and `quiet_hours`: this lamp's window,
`{"from": "22:00", "to": "07:00"}`, or `"off"`, the default. There is no ceiling on renewing, and a say is counted
in bytes, not characters, so `take_max_ms` and `say_max_chars`, which older lamps sent, are gone.

### Subscribe and state

```json
{"id":2,"op":"subscribe","events":["person.arrived","touch","phrase.heard"],"phrases":["lights please","good night lamp"]}
{"id":3,"op":"get","state":["presence","light","lamp"]}
{"id":4,"op":"unsubscribe","events":["touch"]}
```

`phrase.heard` needs `phrases`, the ones to listen for, and `phrases` go
only with it. A `get` reply carries each state asked for: `presence`,
`light`, `mood`, `lamp_state` for `lamp`, and `volume` (`{level, muted}`, from
`speaker/volume`; `null` until lamp-actions has said it, and only with the
`speak` grant). `air` (the `air` grant) is
the newest `air/state` reading, as `air.changed` tells it, with `known:
true`. `device` (the `device` grant) is `board/state`'s `cpu_load`,
`memory_used`, `disk_free`, `uptime_s` and `temperature_c`, with `known:
true`. Either is only `{"known": false}` while there is no reading: no
sensor, its part not running, or none in the last 15 s. Both go on while
the lamp is private, `air.changed` included, as every event does: privacy
closes the camera and the microphone, not the readings of the room's air or
the lamp's own computer. An owner who wants none of what the air shows
doesn't give `air`.

#### The body's limits: `get` `body`

`body` says what the lamp's body may do, joint by joint, and where each joint
is now (`crates/control/src/body_info.rs`). It comes from the body's own
`body/limits`, said once it has found where it is and every 10 s, and its
`body/joint_states`. It needs the `body` grant. Until the body has said its limits, or once it has said none in the last 15 s (lamp-body stopped or restarting), it is `{"known": false, "why": ...}`; a joint with no position in the last 15 s has `counts` and `deg` null. The head's and base's `min_deg` and `max_deg` are a folded lamp's, around the found home: a standing lamp's head can sit outside them, so a client clamping a target to them may cut a reach the body would take. The body holds every look to its own bounds either way.

- `joints`: `yaw`, `pitch`, `roll`, `shoulder` and `elbow`, the names
  `joints` takes. Each has `counts` (where it is, of 4,096 a turn), `deg`
  (degrees from its home), `min_deg` and `max_deg`, and `min_counts`,
  `max_counts`, `home_counts`, `folded_counts` and `stand_counts`.
- **Home** is where the head and base were found when the body started, and
  the arm's folded pose. The base turns right and the arm rises with more
  counts, so `yaw`, `shoulder` and `elbow` are positive that way. The head
  looks up with fewer counts, and `pitch` is positive up. `roll` is positive
  with more counts.
- **The bounds** are the body's: the head's reach from home (`--reach`, and
  the gaze's yaw if wider), inside the pitch window and never under its floor,
  and the arm's window from folded to stood up. On a lamp that may not stand
  (`stands: false`), the arm has no poses and its whole turn.
  - The head's and base's `min_deg` and `max_deg` are those of a **folded**
    lamp, around the found home. Standing, the reach is measured from the
    stand pose instead (its pitch about 20 degrees up on a lamp-75c9), so
    what a look reaches moves with it.
  - The body may cut further than these say: the pitch window, the servos'
    own stored ranges (`LAMP_BODY_OWN_RANGES`), V1's anti-tip rule and the
    stand pose's own bounds. A `joints` answer says what was reached and
    `clamped` when anything cut it.
- `speed_deg_s` and `acceleration_deg_s2` are the head's and base's caps.
  `load_limit` is the force cap as a share, `null` where each servo keeps its
  own. `heat_stop_c` is where a joint stops.
- **Nothing here can be raised from outside the body.**

**One convention for every joint's degrees.** `get` `body` and `joints`'
targets count degrees the same way (and so will `body.state`, a planned
event; nothing sends it yet), from one function
(`body_info::from_home`): from home (the found home for the head and base,
the folded pose for the arm), to a tenth of a degree. `yaw`, `shoulder`,
`elbow` and `roll` are positive with more counts, and `pitch` with fewer, so
up is positive. A `joints` target of `{"yaw": 10}` is 10 degrees right of
where the base was found, and `{"pitch": 5}` is 5 degrees up from where the
head was found. Raw counts stay counts.

### The ring, pixel by pixel

`light` also takes a computer's own colours, drawn by the ring as given:

- **`rgb`:** one `#rrggbb` for the whole ring.
- **`pixels`:** one `#rrggbb` per pixel, from pixel 0 around the ring. There are exactly `hello`'s `limits.ring.pixels` of them, 32 on today's ring, so a computer never assumes the count.
- **`brightness`:** 0–100, scaling the given colours (100 by default). The ring then scales every channel to its own ceiling, which is lower at night, so no computer can go past the eye-safety cap.
- **`fade_ms`:** how slowly the ring comes into the light.

lamp-control sends them as a `light/request` with `pixels` (`lamp_msgs::LightRequest`), under the same claim and priority as a named light. Beside them goes the nearest named colour and level, which a ring that draws no pixels shows instead and `light/drawn` says.

**What stays on top:**

- **The conversation:** a turn's face draws over the pixels while the lamp listens or talks, and they come back at rest.
- **The privacy and mute marks,** which the ring draws on their own path.
- **The camera mark** (#4521, `ring_mark`): drawn over a computer's pixels and frames like over every frame, and kept apart from them (`ring_mark::marked_apart`). While the ring draws a computer's own colours and the mark is on, the mark keeps its own brightness, the two pixels beside it are dark, and every other pixel is at a third of what the computer drew, so the mark is the brightest red on the ring, alone at the top: no computer can hide it among red dots of its own. The mark's pixel, pixel 0, never shows a computer's red (its red is held to its green or blue), so no computer can draw a mark while the camera is off.
- **The person's own light:** a change to it or a touch ends the computer's light, as for any `light`.
- **The ring's channel ceiling.**

A computer's pixels can blink or cycle colours. That is the developer's explicit ask, CLAUDE.md rule 8's exception (the local leader, 2 Nov); the lamp's own unprompted patterns keep `lamp_msgs::PatternKind`'s rules.

One physical bound stays, enforced by the ring itself (`ring_face::Face`): **at most 3 flashes a second**, the photosensitive-epilepsy limit.
- **A flash** is two swings of the ring's light at rest. A swing is its mean brightness turning back by a tenth of the ring's ceiling or more from its last peak or trough, however small each step, or going into or out of a saturated red over a quarter of the ring, however dim.
- **Every resting frame counts,** whatever drew it: a computer's pixels alternating with a named light, from one computer or two, is held like a blink of pixels alone.
- **A faster blink** keeps the frame already shown until the window allows the next one, so it comes out at the limit, never faster. The lamp's own light never swings that often, so only a computer's blink is held.
- **A small change,** one pixel's blink, or a fade that only goes one way, is drawn at once.

**For animations,** each `light` replaces the last under the same claim, so a computer can send a frame at a time. `ring.play` (frames played by the ring's own worker) and `ring.stream` (binary frames under a `light` lease, up to the ring's resting refresh of 40 a second) come next.

### Where the body is

`body.state` says where each joint is, from the body's own reads, so that
a computer can draw the lamp as it stands (a digital twin). It goes out with
each `body/joint_states` message, at most 25 a second (`body_state::EVERY_US`).
The body says its joints every fourth step of its 20 ms loop, so that is 12.5
a second today. The voice's `body_motion` and the eyes' `full_look` judge a
moving joint by that 80 ms, so it stays as it is, and a page drawing the lamp
eases between two messages.

`load` and `moving` also show a hand on the lamp: a computer with the `body`
grant, and no lease, sees when someone pushes or turns the arm, a few times
a second. The pairing screen says the grant moves the body, not that it
sees the lamp handled; a read-only `body_state` grant is proposed on Queue
#3997 and would settle it.

```json
{"ev":"body.state","at":"2026-10-30T08:40:00Z","joints":{"yaw":{"deg":-25.8,"counts":1722,"goal_deg":-22.9,"load":0.1,"moving":true,"temp_c":41},"pitch":{"deg":17.2,"counts":2244,"goal_deg":17.2,"load":0.06,"moving":false,"temp_c":44}},"lamp":{"light":{"on":true,"colour":"warm","level":3},"talking":false,"private":false}}
```

- **Each joint**, under the `joints` op's names, holds these fields:
  - `deg`: degrees from its home, to a tenth, as `get` `body` counts them (`body_info::from_home`, "The body's limits"): from the found home for the head and base and the folded pose for the arm, `pitch` positive looking up. `null` until the body has said its limits (`body/limits`).
  - `counts`: the servo's encoder counts, 4,096 a turn, 2,048 its middle.
  - `goal_deg`: where it was last asked to be, in the same degrees.
  - `load`: the share of its servo's strength, 0 to 1, without a direction.
  - `moving`: turning at 5° a second or more (`MOVING_DEG_S`), since a held servo's reading wanders a count or two.
  - `temp_c`: from the body's once-a-second report, where each joint's `temperature_<joint>` follows its other facts. It is absent until the first report, and again once no report has said it for 5 s (`body_state::TEMPERATURE_KEPT_US`), so an old one never passes for now.
- **A slow computer:** each connection holds only its newest `body.state` not yet sent, apart from its other events; a newer one takes its place, so a computer that reads slowly gets fewer, never a growing queue, and may get one after an event that came later.
- **The lamp beside them:** the light it shows now (`light/drawn`, or `light/state` before that), whether it talks, and whether it is private.
- **The model:** [lamp.model.json](lamp.model.json) is the lamp's kinematic model, written from the rigged CAD (`robots/lamp/hardware/cad/glb/lamp.glb`) by the JS SDK's `tools/lamp-model.js` and shipped in both SDKs as `model`. It holds the chain from the base out, each joint's bone, origin, axis and reach. Each joint's `zero_deg` and `sign` (its `deg` here, from its home, at the CAD's rest pose) are the CAD's until measured on a lamp. The JS SDK's `examples/twin/` draws the lamp with it, live.
- **The cost:** each `body/joint_states` message, and every part's once-a-second `diagnostics`, now reaches lamp-control's watch. Twelve and a half messages a second is an eighth of the `voice/privacy` hundred that once cost 1% of a core on lamp-75c9; the cost on a lamp is to be measured.

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

A take or renewal asks for up to 10 minutes (`for_ms` 1–600,000), and a
lease renewed before it lapses is held as long as its holder keeps renewing:
there is no ceiling (the owner's "no limits for developers", 30 Oct). One
not renewed in time is lost as `expired`, and every loss below still ends
it. The lamp says what happens to it:
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
{"id":18,"op":"turn.start","prompt":"Shall we go over today's list?"}
{"id":19,"op":"turn.end"}
{"id":20,"op":"listen.set","on":false}
{"id":15,"op":"stop"}
```

A reply comes when the action is taken. A long one also ends with
`{"ev":"done","re":11,"how":"finished","why":"…"}`, `how` one of
`finished`, `cut_short`, `dropped`. Today `say`, `sound`, `turn.start` and
`look` end with `done`, and a mute `listen.set` held ends with one when the
lamp lifts it ("Turns and listening").
`say`'s is `finished` once it's spoken, or `dropped` with the voice's own word
and why (`expired`, `refused: …`, `dropped: …`, or the voice not reached).
`look`'s is `finished` once the head is there, with `yaw` and `pitch` as the
body applied them, after its reach and its joints' own bounds (the pitch
window), and `clamped` when any of those cut the look; on a resting lamp whose
head or base stopped short inside a rest's slack (24 counts), `finished` with
the angles where they stopped, `clamped` when more than half a degree off; or
`cut_short` (a conversation, the lease ending, another look, a hand or
anything else holding the head for 5 s, then with how many counts short,
`reconnected` or `released`), or
`dropped` (the body not held, or its lease socket not reached). A look asked
right after `lease.take`, before the door has taken the body's hold (its
check runs every 250 ms), waits for that hold rather than being dropped; if
`body` is let go first, it is `dropped` ("the body lease ended"), and it is
only ever sent under the lease it was asked under, never another taken in its
place. `gesture`
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
| `busy` | another client holds the part, or this one by another lease; a `turn.end` with no turn of this computer's, "not this computer's turn" |
| `refused` | the body's own check, with its why: "centre of gravity 27 mm > 22 mm", "pitch past its window"; the voice's, for `turn.end` and `listen.set`, and "the voice here doesn't take turns from a computer" or "…doesn't take a computer's mute" from a voice older than them |
| `quiet_hours` | `say`, `sound` and `turn.start` in this lamp's quiet hours, when it has them set (off by default; see "Limits, rates and quiet hours") |
| `private`, `paused`, `asleep` | reserved: no request is refused with them today. These states reach a client another way: its leases are lost with that `why` (a `lease` event), `lamp.changed`'s `lamp_state` says them, and a `say` the voice's privacy gate doesn't speak ends with a `done` of `how: "dropped"` and the voice's why. A gesture or move the body drops while private or asleep is not told |
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
| `lease.take` | `parts` (`body`, `light`, `voice`), `for_ms` 1–600,000, `why` ≤ 200 chars | `body`, `light` or `speak` for each part | — |
| `lease.renew` | `lease`, `for_ms` 1–600,000; renewed in time, no ceiling | — | — |
| `lease.release` | `lease` | — | — |
| `express` | `feeling` (a lowercase name, the `feelings` skill's), `strength` 0–1 (0.5) | `body` + `light` | — |
| `gesture` | `name` (a lowercase name of a body clip, `idle` and `scanning` too; ≤ 64 chars) | `body` | — |
| `look` | `at`: `"face"` (+ `look`), `"hand"` (+ `look`, followed), `"name"`, `"ahead"`, or `{yaw, pitch}` in degrees (any finite number; the body clamps it to its reach); `"face"` and `"name"` are refused in this release, "not in this release: look at a face or a name" | `body` | body |
| `move` | `step_ms` 40–1,000, `frames` 2 at least (at most what 10 s allows: 251 at 40 ms) of five joints (body counts from home), first and last all 0, lasting 1.5–10 s ((frames − 1) × `step_ms`), within the body's reach and speed (`lamp_body::moves::check`) | `body` | body |
| `play` | `name` (a recorded move of the client's package); refused today, "not routed yet: waits for client packages" | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`; degrees from −180 to 180, at least one), `within_ms` (0 or more; the body's caps pace each step) | `body` | body |
| `light` | `colour` (one of the ten), `level` 1–5, optional `pattern` (`breathe`, `candle`, `pulse`), `for_ms` 1–30,000 | `light` | — |
| `light` (own colours) | exactly one of `colour`, `rgb` or `pixels`; with `rgb` or `pixels`: optional `brightness` 0–100 (no `level` or `pattern`); optional `fade_ms` 1,000–60,000 | `light` | — |
| `say` | `text`: words, no control characters (a newline or tab included, as the voice refuses them), at most 900 UTF-8 bytes (`text: at most 900 bytes as the voice takes it`): the voice's say socket takes no more (`MOST_WORDS_BYTES`), so the door refuses it rather than send a say the voice drops. About 900 Latin letters, 300 in a three-byte script. A longer text is several says; optional `rephrase` (boolean) | `speak` | — |
| `sound` | `name`: `hum`, or a clip's name (a lowercase letter, then lowercase letters, digits, `_` or `-`, ≤ 64) | `speak` | — |
| `sound.upload` | `name` (a clip's, never `hum`), `sha256` (64 hex), `size` 1–1,444,140, `part` from 0, `parts` 1–64, `data` (base64, ≤ 49,152 chars) | `speak` | — |
| `sound.list` | — | `speak` | — |
| `sound.remove` | `name` | `speak` | — |
| `volume` | exactly one of `to` 0–100, `step` `up`/`down`, `mute` `true`/`false` | `speak` | — |
| `stop` | — | — | — |
| `turn.start` | optional `prompt`: words said first, as `say`'s `text` (no control characters, at most 900 UTF-8 bytes) | `speak` + `hear` | — |
| `turn.end` | —; only a turn this computer started | `speak` | — |
| `listen.set` | `on`: `true` or `false` | `hear` | — |
| `skill.push` | `name` (a skill's, `a-z0-9-`, ≤ 64), `sha256` (64 hex), `size` 1–1,048,576, `part` from 0, `parts` 1–32, `data` (base64, ≤ 49,152 chars) | `skills` | — |
| `skill.list` | — | `skills` | — |
| `camera.snapshot` | — | `camera` | — |
| `camera.stream` | `on` (true or false), `fps` 1–30 (2), `size` `640x480` or `1280x720` for the fast stream | `camera` | — |
| `microphone.stream` | `on` (true or false) | `microphone` | — |
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

## Turns and listening

The door serves these three ops (`crates/control/src/parts.rs`), and the
voice takes them on its say socket ([speaking-first.md](speaking-first.md),
"A turn for lamp-control" and "A computer's mute for lamp-control").
`control.schema.json` names them, with their replies' `turn`, `started` and
`mute`; the SDKs call them as `start_turn`, `end_turn` and `set_listening`
(Python) and `startTurn`, `endTurn` and `setListening` (JavaScript), and
lamp-mcp's tools are `lamp_start_turn`, `lamp_end_turn` and
`lamp_set_listening`.

A computer can start a conversation turn, end one, and set the lamp's
listening. The person wins over each, as "Who wins" says.

| op | what the lamp does |
|---|---|
| `turn.start` | Opens a turn as a tap on the head does: the next line the person starts within 8 s is answered as spoken to the lamp. With `prompt`, the lamp says it first, and the 8 s start when it ends. The lamp shows its normal listening cue, and the journal names the computer. `done`: `finished` when the person spoke and the lamp answered; `cut_short`, "the person is talking", when the lamp's answer to a line of the person's own begins to sound while the window is open; or `dropped` with why (`nobody spoke`, `muted`, `busy`, `refused: …`, and for a turn not yet answered when the same computer opened a newer one, `dropped: a newer turn of its computer's`). The same computer may open a new turn while its own earlier turn's conversation goes on: the new turn continues it, with a new id, in place of the earlier one. |
| `turn.end` | Ends a turn this computer started with `turn.start`, and the conversation that grew from it, as the button's stop does: the answer playing, what waits to be said, and the open window. It closes the conversation too, as the person's own stop does: the next room line needs the lamp's name, a turn or a tap. Never the person's own conversation, another computer's turn or the lamp's own words: the conversation outranks a computer ("Who wins"), so any other is refused `busy`, "not this computer's turn". The person's own follow-ups within the turn's conversation count as the turn's own, so `turn.end` can end them and a later `after` start continues over them, until the conversation goes quiet. |
| `listen.set` | `false`: this computer's mute, `muted_by: control`, shown on the ring the way the person's mute is shown, so nobody thinks the lamp listens when it doesn't, and journaled with the computer. `true`: lifts only this computer's own mute, never another computer's, nor the person's switch, button or sleep mute. While two computers hold one, the lamp listens again only when both are lifted. A computer's mute ends when its connection does, or when it is revoked, so a computer gone can't leave the lamp deaf; the person's hand on the head lifts it too, as it lifts the restart guard's. |

- **The person wins.** The privacy switch, the button, sleep and "stop" are
  checked before any of these. The person's mute ends the whole
  conversation, a computer's turn with it. A person already talking with
  the lamp wins too: when the lamp's answer to their line begins to sound
  while a computer's window is open, the computer's turn ends at once
  (`cut_short`, "the person is talking") and their conversation goes on.
- **When `turn.start` is refused:** while the lamp is muted, by anyone;
  while the person's own conversation goes on, or another computer's turn;
  and while the lamp speaks its own words. It is never queued behind them.
  The same computer's own turn going on, and the conversation it grew,
  refuse nothing: its new turn continues that conversation (lamp-75c9,
  3 Nov 19:27: a computer's next turn was refused by its own turn's
  conversation). A person's answer in hand still refuses it. With or without a
  `prompt`, it meets the same quiet-hours and night gates as a `say`.
- **A computer gone before its turn opens gets no cue.** When its last
  connection closes or it is revoked while a `turn.start` still waits (for
  listening to be ready, the model service, the speaker), the door shuts
  that start's say-socket connection and the voice drops it
  (`dropped`, "its asker went away"): no listening cue and no 8 s window.
  A turn already open is left to its `turn.end` and the person.
- **No microphone the person closed is opened.** `listen.set true` can't
  open the switch, the button's mute or sleep, as the app's `mic.set`
  can't.
- **Nothing heard crosses the door:** no audio and no transcript. A
  computer learns what it learns today: its own phrases (`phrase.heard`),
  the name (`name.heard`) and the `done`. The 16-phrase and
  6-changes-a-minute caps stay, as `hear`'s privacy bound.
- **No touch is faked.** The ops go to the voice over its say socket, as
  new request kinds beside `say`, under the same gates. A voice that
  doesn't know them refuses them, and the door answers `refused`, "the
  voice here doesn't take turns from a computer" (for `listen.set`, "the
  voice here doesn't take a computer's mute"). A `turn.start` has been
  answered by then, so its `done` says it: `dropped`, "refused: the voice
  here doesn't take turns from a computer".

**On the wire, as built:**

```json
{"id":18,"op":"turn.start","prompt":"Shall we go over today's list?"}
{"id":19,"op":"turn.end"}
{"id":20,"op":"listen.set","on":false}
{"id":21,"op":"listen.set","on":true}
```

- **`turn.start`** is held to `say`'s quiet hours, with or without a
  `prompt`, before the rate, and counts in `say`'s rate class when the rates
  are on. It needs no lease, and another computer's `voice` lease doesn't
  stop it, as it doesn't stop a `say`. It goes to the voice as
  `{"skill":"control","turn":"start","expires_in_ms":30000}`, the prompt as
  `text`. The reply is `{"to":"voice"}` once it is handed on; then `done`:
  `finished` on the voice's `spoken`, else `dropped` with the voice's word
  and why (`dropped: nobody spoke`, `refused: the lamp is muted`,
  `expired`, or the voice not reached). The turn's id from the voice's
  `begun` is kept for that computer, with the request that started it,
  past its outcome, since the voice holds the conversation a turn grew
  until it is quiet and may still end it by that id. It is replaced only
  when a newer turn's `begun` comes, and forgotten once a `turn.end` used
  it (ended, or the voice's "not this computer's turn"). While one is kept,
  the next start names it as `"after":N`
  (`{"skill":"control","turn":"start","expires_in_ms":30000,"after":7}`),
  so the voice knows the turn going on is this computer's own; a start
  refused or dropped before its `begun` leaves the earlier id, so a
  `turn.end` still ends that conversation.
- **`turn.end`** sends `{"skill":"control","turn":"end","id":N}` with the id
  of this computer's latest begun turn. With no turn of its own it is
  refused `busy`, "not this computer's turn", and the voice isn't asked;
  with no id kept and a start asked that the voice hasn't begun yet,
  `busy`, "this computer's turn has not begun yet". The
  reply waits for the voice: `{"turn":"ended","started":18}` (the
  `turn.start` it ended) on `ended`; the voice's "not this computer's turn"
  is the same `busy`; anything else is `refused` with the voice's why. The
  start's `done` follows: `dropped: ended by the computer` if nothing of
  its answer had sounded, or `cut_short`, "ended by this computer", if the
  end cut an answer that had begun, so a developer can tell it from one
  heard whole. The end closes the conversation: what was said in the turn
  no longer counts as a conversation going on, so the next room line needs
  the lamp's name, a new turn or a tap to be answered.
- **`listen.set false`** opens one say socket connection for the computer
  and sends `{"skill":"control","listen":false}` on it; the reply waits for
  the voice's `muted`: `{"mute":"held"}`. The door keeps that connection,
  and the voice holds the mute while it is open. Asked again while held:
  `{"mute":"held"}`, nothing new; asked while the voice is still taking the
  first, `busy`. **`listen.set true`** sends `{"skill":"control",
  "listen":true}` on that connection and closes it: `{"mute":"lifted"}`; with
  nothing held, `{"mute":"nothing_held"}` and the voice isn't asked.
- **A computer gone lifts it.** The door closes the held connection when the
  computer's last connection to the door closes and when it is revoked,
  and the voice lifts the mute as its connection closes. A `stop` doesn't
  lift it.
- **A hand on the head lifts it** in the voice, which says `lifted`, "a hand
  on the head", and closes the connection. The door forgets it and tells the
  computer with a `done` naming the `listen.set` that held it:
  `{"ev":"done","re":20,"how":"cut_short","why":"a hand on the head"}` (or
  "the voice let it go", if the voice closed it without a word).
- **Replies that wait on the voice** (`turn.end`, `listen.set false`) are
  waited for off the door's lock and sent on the asking connection only,
  and the journal gives each a second line, "answered ok" or the refusal's
  code. A command's own line says "handed to the voice, by say.sock".

## Limits, rates and quiet hours

Every motion goes through `lamp-body`'s limits, which no sender can raise:
its speed and acceleration caps and pitch floor (`body.rs`), each joint's
farthest and fastest in a move (`moves.rs`: farthest 200, 200, 150, 350, 350
counts and fastest 900, 1,300, 1,000, 800, 800 counts a second for head
pitch, head roll, base yaw, shoulder and elbow; 1.5 to 10 s, from the pose
and back), the 22 mm centre-of-gravity rule (`cog.rs`), its force limit,
strain rests and heat stop. `joints` goals get the same caps at each step.
There is no raw servo or register access.

Rates a client is held to with `--rates default`. By default the rates are off, and only the last three rows hold: actions a part refused, the phrase cap and changing the phrases (see "Rates off, the default" above):

| what | at most |
|---|---|
| `joints` | 40 a second |
| `gesture`, `move`, `play`, `look` | 1 a second |
| `light` | 5 a second |
| `say`, `sound`, `turn.start` | 6 a minute and 60 an hour |
| `express` | 20 a minute |
| `skill.push` | 1 every 10 s and 20 an hour, counted once at part 0 |
| `skill.list`, `skill.remove` | 6 a minute; one at a time for a computer, and 4 with the helper at once on the lamp (`busy` past either) |
| actions a part refused | 20 a second, then the connection closes |
| registered phrases | 16 a computer, across all its connections; each of 64 characters at most and 8 letters at least, so no short word or pair ("is a"; "ok lamp" too, at 6 letters: "okay lamp" passes). The floor is friction, not a guard: what keeps `hear` narrow is the owner's grant, this cap, the rate on changing them, and that only the registered phrase leaves the lamp |
| changing the phrases | 6 a minute; the same phrases again cost nothing |

The rates bound how often a computer asks, never what a move may do. The
body bounds every move itself, whatever the rate:
- **Its own check:** reach, speed, length and starting at home (`lamp_body::moves::check`), run again by the body on every sent move (`moves::Sent::keep`).
- **Its own pose rule:** may a move play now, from this pose (the anti-tip rule, `cog.rs`).
- **Priority and caller caps:** with `--arbitration` and `--caller-caps`, it holds a computer to `skill` priority.

A computer's new `gesture`, `move` or `express` move interrupts the motion playing: the door sends each to the body with `"replace": true` (`{"gesture":"nod","replace":true,...}`, `{"move":{...},"replace":true,...}`), and holds no slot and refuses none as busy. The body ends what plays, if it is at `skill` priority or lower (another computer's motion, a skill's, the lamp's own idle life), easing it out at once from where it is under its own caps (the gesture step, the arm's step, the anti-tip rule), and plays the new one after; its journal says it once, "gesture stretching ended for nod from control:<client>", and `body/gesture` says the ended one `dropped`, "ended: replaced by a newer motion". The new one is checked from the pose it begins in, as any gesture is, so one that would tip there is still dropped. The person always wins: while someone is talking with the lamp (listening, thinking or speaking), over a gesture paused for that talk, over the conversation's motion (the lamp's glance for its caller, the voice's gestures), a hand on the lamp or the person's own interaction, the body refuses it with why "dropped: the person is using the lamp", with or without `--arbitration`, whatever is playing. A voice gesture still waiting for the lamp to stand is kept by its priority only under `--arbitration`; without it, the talk it belongs to is what refuses the replace. The body honours `replace` only from lamp-control, an asker named `control:<client>` (or `lamp-control/control:<client>` under `--caller-caps`), with or without `--arbitration`; the same mark from a skill or any other asker is taken as absent, and its gesture asked over one playing is dropped by the body as before ("dropped: another gesture plays", or "dropped: busy, skill" with `--arbitration`). `look` and `joints` are not motions here.

**On stable 10824 and older** there is no replace: a `gesture`, `move` or `express` move asked while the door's last motion still plays is refused `busy`, "busy: a motion plays for N ms more", with `retry_ms` (#4209). Wait `retry_ms` and ask again. The replace above is from the build after 10824.

`express` is counted only in its own class: not in motion, though it moves the body, and not in `light`, though it lights the ring. The ring's own arbitration and the light rate bound its light.

`express` sends the same built-in move each time. The body plays the kept one again (`Sent::keep`) and never counts it against the 64 changed moves a run (`MOST_REPLACED`).

The ring's one pulse a minute per asker (`lamp_msgs::arbitration` `PULSE_GAP_US`) is separate and stays.

`gesture`, `move`, `play` and `look` share one class, `motion`. A refusal
says `retry_ms`, the time until the oldest counted action leaves its window.
A refused action isn't counted.

At night the ring's night caps still clamp every light, and nothing moves
while the lamp sleeps. **Quiet hours:** off by default (the owner's "no
limits for developers", 30 Oct). When this lamp sets a window
(`--quiet-hours`, one definition in `lamp_local_time::QuietWindow`;
lamp-skills keeps its own), a computer's `say`, `sound` and `turn.start`
are refused inside it with `quiet_hours`, the window in its why ("quiet hours: 22:00
to 07:00 local, this lamp's setting"), and so are they while the lamp's
local time isn't known (its clock not yet set, or its zone unreadable:
"quiet hours: the lamp's local time isn't known"). A refusal reaches no
part and isn't counted against the rate, and it's journaled once a
computer a night. With no window, or `off`, nothing is refused, even while
its time isn't known. Besides that setting,
the owner's per-client allowance, with the app's setting, will be the only
thing that lifts it (Fourteen, Eighth and Nine on #3504). Every command and its answer is
journaled as one line (client id and name, op, result: "The journal"
below); the app can show the last 100 and revoke a client, which closes its
connections at once.

## The journal

`lamp-control` writes one journal line for every command and its answer,
every lease the lamp takes back, suspends or holds again, and every event it
sends but `body.state`, which at 12.5 a second would spend a computer's lines
a second on itself (`crates/control/src/journal.rs`).

**Command lines.** Each says:
- the computer: its id and, when plain (letters, digits, `_` and `-`, 32
  at most), the name it paired with. Any other name is written "a name the
  door doesn't write", and the id alone names the computer. A plain name can
  still name a person (a first name and `-mac`), so the journal holds what
  the developer chose to call their computer;
- the op, and what it asked in its harmless fields, so two requests that
  went to the same place can be told apart:
  - `light`: colour, level, pattern (or steady) and `for_ms`;
  - `gesture`, `play`, `sound` and `sound.remove`: the name;
  - `sound.upload`: the name, and which part of how many;
  - `move`: how many frames, and `step_ms`;
  - `express`: the feeling and strength;
  - `look`: the target, or its angles;
  - `get`: the states;
  - `subscribe` and `unsubscribe`: the events;
  - `lease.take`: the parts and `for_ms`, and `lease.renew` its `for_ms`;
  - `turn.start`: its prompt as a `say`'s words are written (how many
    characters and a tag), or "no prompt";
  - `listen.set`: `on true` or `on false`;
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
- `say`, `turn.start`, `turn.end` and `listen.set` go to the voice, by
  `say.sock`. A `turn.end` or `listen.set` whose reply waits on the voice is
  "handed to the voice, by say.sock", and a second line, "answered ok" or
  "answered refused: busy", says how it ended.
- `look` goes to the body, by `lease.sock`, once it is routed; until then
  it's refused, and a refusal names no place.
- `skill.push`, `skill.list` and `skill.remove` go to the skill helper.
- `microphone.stream` goes to the ears, by `mic1/audio`, and its line says
  `(on)` or `(off)`.
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

**The same command again is counted, not written.** Within a computer's
second, a command with the same fields and answer as the one just before it
adds to a count. One line says it when the run ends, at another line, the
second's end or the door's tick: `…: get (presence): ok, to the door, again
19 times`. So a dashboard polling `get` 20 times a second writes about two
lines a second, and every command is still told.

**At most 10 lines a second for each computer.** Past that, its next second
begins with one line, `c_3f9a: N more not logged`: at the computer's next
line, or at the door's next 250 ms tick once the second is over, whichever
comes first. So a burst followed by silence still says it.

## The camera

With the `camera` grant, ticked by the owner at pairing like any other, a computer sees what the lamp's camera sees (`crates/control/src/camera.rs`).

- **The eyes own the camera, and the door never opens it.** With `--sight-file`, the eyes keep their newest frame in one file while the camera is open: a JPEG 320 across, about six a second, removed when the camera closes (eyes.md). lamp-control reads that file (`--sight-file`, `/run/lampos-eyes/sight.jpg` unless given, `off` for none). The eyes and the voice go on as before.
- **`camera.snapshot`** answers with that frame, as `jpeg` (base64), `bytes` and `age_ms`. A frame older than 2 s is the camera closed (`refused: the camera is closed`). A frame caught mid-write is refused with `ask again`.
- **`camera.stream`** with `on: true` sends each new frame once as a `camera.frame` event (`jpeg`, `bytes`, `age_ms`), `fps` a second at most (1 to 30, 2 if not given; each new frame as it comes, never one twice: the eyes give about 1 to 2 new frames a second today, so the rate asked is a most, not a promise). The rate is a schedule, a frame each 1/`fps` of a second, not a wait of 1/`fps` after the last frame sent: a frame up to half that early still goes, so a camera already at the rate asked loses none of its frames (the wait kept 2 in 3: 16 a second of a 25 fps camera at `fps: 25`, lamp-75c9 9 Nov; with the camera at 15 a second at night, `fps: 15` gave 10.4 a second and now 13.2, `fps: 10` gave 6.7 and now 9.7, lamp-75c9 10 Nov), and in any one second a stream sends `fps` frames and one more at most. Its answer carries `fps_now`, the rate new frames came at over the last 2 s (`null` before two were seen), and so does each `camera.frame`. It runs until `on: false`, `stop`, the connection closes, or a re-pairing drops the computer's `camera` grant. It also ends by itself with a `camera.ended` event and its `why`: when the lamp turns private, after 10 s with the camera closed, or when the ring doesn't show its mark (below). Another `camera.stream` with `on: true` takes the new rate. Each `camera.ended` is a journal line with its why, and the frames still queued for the connection never go. A stream's `camera.ended` goes only while it is the computer's latest stream: an older stream's end that comes after a newer `on` never ends the newer one, nor drops its frames. Each `on` is a stream of its own: a frame goes, fast or as an event, only while the very stream that made it runs, so a frame queued before an `off`, or before another `on`, never goes through the stream that follows, before that one's mark is drawn.
- **The fast stream.** Where the eyes serve their frames (`--frames-socket`, eyes.md), a stream gets every frame the camera makes, as the camera made it: binary WebSocket frames (a 4-byte header length, a JSON header with `bytes`, `width`, `height`, `fps_now`, then the JPEG), at `size` `640x480` (the default) or `1280x720`, up to 25 frames a second; its answer says `"fast": true` and the size. That is far more than the 320-across sight frame. Each frame is checked against privacy right before it goes, as every `camera.frame` is. No product unit passes `--frames-socket`; the product's lamp-eyes serves the frames with `LAMP_EYES_FAST_STREAM=on`, off (switches.md). On lamp-75c9 (9 Nov, 11048's own parts) the eyes' CPU and look time didn't change with a 640x480 or 1280x720 reader, and 13 to 16 frames a second of the camera's 25 reached the SDK; lamp-control's CPU went from about 2.5% of one core to about 3.4% while it streamed (10 Nov, 11091's own lamp-control, a 15 fps camera at night).
- **Never while the lamp is private.** Both are refused `private` while the privacy switch is on, or while the voice's privacy lease has stopped coming. A stream ends at once when it turns private. The voice also closes the eyes' camera then, but the door doesn't wait on that. Without `--bus-root` the door can't know, so it offers no camera at all.
- **Local only, and journaled.** The door takes connections only from the lamp's own network, judged by the interface each connection arrives on, never by the address alone (`server::is_local_peer`): loopback (the lamp itself, or an ssh tunnel the owner gave); a link-local peer (fe80::/10) whose scope is a physical interface; or a peer on-link on the same physical interface the connection came in on (the lamp's routes with no gateway, read at each connection: lamp-75c9's office LAN is 172.168.20.0/23 on wlan0). A physical interface is Ethernet or Wi-Fi with a device behind it (`server::is_physical`): never a VPN's (`wg*`, `tun*`, a tailnet, ZeroTier's `zt*`), a bridge, docker's or a veth. A private or link-local address by itself is not local, since a VPN hands out 10/8, fc00::/7 and its own fe80:: from anywhere (Thirteen's P2s on #4521). So "local" means one hop on the lamp's own wire or air, with no router between: on some networks an on-link subnet is wide, and everyone on it counts as local. Any other peer is closed before TLS and said in the journal (`server::is_local`). With `--remote on` (or `LAMP_CONTROL_REMOTE=on`; off unless given, said in the journal at start) any address may connect, a VPN's 100.64/10 included, but a computer beyond the lamp's own network is never given the camera or the microphone, whatever it was granted: `refused`, "local network only…", and no `camera.*` event reaches that connection. Each snapshot and each stream's start and stop is a journal line ("The journal") naming the computer, never the picture.
- **The ring shows it, and says so.** While a stream runs, the ring shows one small steady red dot, bright on a lit ring and dim on a dark one, and a snapshot blinks it (`light/mark`, ring-choreography.md, "A computer's mark"). The mark is asked for before a picture leaves, and **a picture goes only while the ring says it drew it**: the ring tells `light/mark_drawn` (`lamp_msgs::RingMarkDrawn`) after each frame it shows with the mark, and only when the dot, as written under the ring's ceiling, is not black and the frame was painted. lamp-control hears that word only from the ring's own publisher name (`RingMarkDrawn::SOURCE`; a bus source is the publisher's word, so this keeps out a stray publisher, not one that lies), and sends a frame only within 1 s of it for its computer (`camera::MARK_SHOWN_WITHIN`). A binary frame goes only while its computer's stream still runs and it holds `camera`, so none queued before an `off`, a `stop`, a re-pairing or an end goes after it. A snapshot's wait holds the door for up to that second, so others' requests wait with it. A stream holds its frames back for its first second while the ring starts drawing, then ends, "the ring is not showing the camera's mark", if it never does. A snapshot waits up to 1 s for it, and is refused the same way. So a mark asked but not drawn (the ring's authority lapsed, its ceiling at 0, the shutdown's warning, its marks never started) stops the pictures: it fails closed (Thirteen's P2 on #4521). Where lamp-control can't ask for the mark, or hear the ring's word (no bus), there is no camera.
- **The microphone** has its own section ("The microphone").

## The microphone

With the `microphone` grant, ticked by the owner at pairing like any other, a computer hears what the lamp's room microphone hears (`crates/control/src/microphone.rs`).

- **The ears own the microphone, and the door never opens it.** With `LAMP_BUS_AUDIO=on`, the ears publish each block they hear on `mic1/audio` (`lamp_msgs::Audio`, `json+pcm`: mono, 16 kHz, S16LE, 320 samples every 20 ms, `sequence` counting blocks; ears.md, "Optional mic1 PCM export"). That switch is off unless set, and no ears unit sets it: it is the owner's privacy decision, not this door's. (The voice's unit sets the same name for its own export, `mic2/audio` after its echo canceller, which lamp-name reads; this door never reads it.) lamp-control subscribes to `mic1/audio` only while at least one stream is on, on one thread for every stream, and lets the subscription go when the last one ends, so an idle door reads nothing. Without `--bus-root` there is no microphone at all.
- **Off unless the owner switches it on.** lamp-control serves `microphone.stream` only with `--microphone on` (or `LAMP_CONTROL_MICROPHONE=on`; off unless given, said in the journal at start). Off, `on: true` is refused, "the microphone stream is off on this lamp (LAMP_CONTROL_MICROPHONE)". No unit or deploy file sets it, nor the ears' `LAMP_BUS_AUDIO`: both stay off until the owner decides.
- **Never while someone else may be in the room.** The door can't tell the owner from anyone else: faces on the bus carry no identity, and one person in view who isn't the owner can't be told apart today. So the gate is the presence it already watches (`Watch::in_view`, the newest `humans/faces/tracked` count): with more than one person in view `on: true` is refused, "someone else is here", and a running stream ends at once with `microphone.ended` `{"why":"someone else is here"}`. **A count is only as good as its last message:** the eyes say their faces several times a second, nobody there included, so with no `humans/faces/tracked` for 2 s (`watch::FACES_STALE_US`; the eyes stopped or restarted, the camera out) the count is unknown, never its last value, and the room is closed the same way: refused, or ended at once, "the room isn't seen" (the local leader's P2). The room is unseen until the first faces message, and without a watch at all.
- **`microphone.stream`** with `on: true` answers `{"stream":"on","sample_rate":16000,"channels":1,"format":"s16le","block_ms":20,"source":"mic1"}`, and from then on sends each block as one binary WebSocket message, as the camera's fast stream does: the header's length (4 bytes, big-endian), the header (JSON: `ev` `microphone.audio`, `sequence`, `lost`, `sample_rate`, `channels`, `format`, `bytes`), then the samples, 640 bytes. `lost` is the blocks lost since the last one this stream sent: a jump in the ears' `sequence`, and the blocks this computer's connection had no room for. `on: false` answers `{"stream":"off"}`. On again while one runs replaces it.
- **How it ends.** On `on: false`, `stop`, the connection closing, or a re-pairing that no longer grants `microphone` (ended at once, as the camera's is), with no event. By itself with a `microphone.ended` event and its `why`: "the lamp is private" at once when the lamp turns private, "someone else is here" at once when a second person comes into view, "the room isn't seen" within 2 s of the eyes going quiet, "the ring is not showing the microphone's mark" when the ring hasn't said it drew the mark (below), and "the lamp's microphone isn't shared here" when no block has come for 2 s. A lamp whose ears don't publish their sound (the ears' `LAMP_BUS_AUDIO` off, as every unit leaves it) takes the stream and then ends it that way within 2 s. A stream's `microphone.ended` goes only while it is the computer's newest stream: an older stream's end that comes after a newer `on` never ends the newer one, nor drops its queued blocks.
- **Never while the lamp is private.** Refused `private` while the privacy switch is on, or while the voice's privacy lease has stopped coming. The door checks this and the people in view (a fresh count) again before each block leaves, so a stream ends at once when either changes. The ears also stop publishing then, but the door doesn't wait on that.
- **Local only, and journaled.** As the camera: the door takes connections only from the lamp's own physical network ("The camera", `server::is_local_peer`), and a computer beyond it (`--remote on`) is refused, "local network only…", and no `microphone.*` event or block reaches that connection. Each stream's start and stop is a journal line (`microphone.stream (on)`, `microphone.stream (off)`), and so is its end by itself, with its why (`event microphone.ended (the lamp is private) sent`, as the camera's), naming the computer, never the sound.
- **The ring shows it, and says so.** The ring's mark (`light/mark`, kind `microphone`; the same dot as the camera's) is asked before the first block leaves, and renewed every second while the stream runs (`camera::MARK_EVERY`, 2 s ahead), so a person can always tell. **A block goes only while the ring says it drew the mark**, by the camera's rule (`camera::shown`): within 1 s of the ring's `light/mark_drawn` for this computer. A stream holds its blocks back for its first second while the ring starts drawing, counting them in the next block's `lost`, then ends, "the ring is not showing the microphone's mark", if it never does. So a mark asked but not drawn stops the sound: it fails closed. Where lamp-control can't ask for the mark, or hear the ring's word (no bus), there is no microphone: refused, "not shown on the ring".
- **A slow computer loses sound, never memory.** Blocks go through the same queue as the camera's binary frames, 4 at most for each connection (`server::FRAMES_QUEUED`, 80 ms of sound); past that a block is dropped, never waited for, and counted in the next one's `lost`. Each queued frame carries its kind (`parts::Media`): a block goes only to a connection whose computer holds `microphone`, and only while its microphone stream still runs, so none queued before an `off`, a `stop`, a re-pairing or an end goes after it. The thread that reads the bus never waits on a connection.
- **Latency:** one 20 ms block, plus the network. A block leaves as soon as the ears publish it; nothing is gathered on the lamp.

## The SDKs

Thin Python and JavaScript clients generated from
[control.schema.json](control.schema.json): finding the lamp, `lamp pair`,
hello, `on(event, handler)`, every action as a call, leases renewed for the
caller, and the error codes as typed errors. For example, greeting a person
who arrives:

```python
lamp.on("person.arrived", lambda e: lamp.gesture("greeting"))
```

Following a hand is `follow_hand()` (Python) or `followHand()` (JS): `look`
at `hand` under a body lease of its own, with `hand` events for where it is,
and a fake lamp that says synthetic hand tracks in `humans/hands/tracked`'s
shape (`lamp fake --hands`); lamp-mcp's `lamp_follow_hand` is the same for an
agent.

They run on the developer's computer, never on a lamp. Until `lamp-control`
is built they are tested against a fake server that speaks this schema.

## Sounds

A paired computer keeps clips of its own on its lamp and plays them by name
(the local leader's 3 Nov, for "an SDK that controls the lamp 100%"):

```json
{"id":30,"op":"sound.upload","name":"chime","sha256":"41a68bae2ca6137df21f7702589888700856c7da2e55804d76a7b72eec7246a4","size":96044,"part":0,"parts":3,"data":"UklGRiR3AQBXQVZF"}
{"re":30,"ok":true,"got":1}
…
{"re":32,"ok":true,"sound":{"name":"chime","bytes":96044,"ms":2000}}
{"id":33,"op":"sound","name":"chime"}
{"re":33,"ok":true,"to":"voice"}
{"ev":"done","re":33,"how":"finished"}
```

- **A clip** is a WAV of 16-bit mono PCM at 24 kHz, the voice's speaker
  path's, at most 30 s (1,444,140 bytes with its header). It goes up in
  parts of at most 49,152 base64 characters (36 KiB), in order from 0, at
  most 64 parts and 120 s from the first to the last; a broken upload is
  sent again from 0. The last part checks the size and the SHA-256 and then
  the clip itself, with the voice's own checks: the format and length, no
  sample past −12 dBFS and no 10 ms past −30 dBFS RMS. That loudness cap is
  the speaker's and the ear's bound, the same one a skill's sound has; it is
  the only bound on what a clip may hold. A clip of the same name is
  replaced.
- **Kept** in lamp-control's state folder, `sounds/<client>/<name>.wav`,
  0600 in a 0700 folder, written aside and renamed into place. Each computer
  keeps at most 64 clips and 20 MB in all, so no computer can fill the
  lamp's disk; past either, the upload is `refused` with why. Hello's
  `limits.sounds` says each bound.
- **Played** by `sound` with its name: lamp-control sends the voice its
  path (`say.sock` `{clip}`), which the voice reads only from under its
  clips folder (`LAMP_SAY_CLIPS`, `/var/lib/lampos-control/sounds` by
  default), every link followed, with the same checks again, on the asker's
  thread. It plays as an exact `say` does: it waits while someone is talking
  with the lamp, the speaker is busy, or an answer or reminder is owed;
  the keeper's pause and the privacy switch refuse it; there is no one in
  30 s rarity. Quiet hours refuse it at the door, as `say`. A name the
  computer hasn't kept is `refused`, "no clip named …".
- **Its own only:** `sound.list` answers the computer's clips (`sounds`,
  each name, bytes and ms), `sound.remove` deletes one (`removed`), and no
  computer can play, list or delete another's.
- **A revoke deletes them all**, with its upload in flight.
- **Not logged:** the journal says the name and the part, never the bytes.
- A lamp-control without `--say-socket` keeps none: each clip op is
  `refused`, "this lamp keeps no computer's clips".
- **lamp-control and lamp-voice go together.** The say socket refuses a
  field it doesn't know, so a lamp-control from this change with a
  lamp-voice from before it gets "not a say request" for every clip, as one
  from #4958 does for every `say` (its `exact`). A product round ships both;
  a `lamp-try` of lamp-control alone needs the lamp-voice of the same
  round.

## Skills pushed from a computer (piece 7)

Eighth's proposal on #3504, agreed by Nine and blue. A paired computer can
put a rules skill of its own on its own lamp, with no Store, the way a skill
the owner makes by voice is installed today. `lamp-control` serves the ops
below with `--manage-socket`, which the product unit passes; the manage
helper's side is lamp-skills' (piece B). The schema has the three request ops, and both SDKs push, list
and remove (`push_skill`/`pushSkill`, `lamp skill push|list|remove`).

**Not yet on customer lamps.** The product unit passes no `--manage-socket`,
so on a customer lamp (stable 10824 and lamp-voice alike) every skill op,
`skill.push`, `skill.list` and `skill.remove`, is refused `refused`, "skill
push is not on this lamp". Try a push on the fake lamp (`lamp fake`), or run
the same behaviour as a program on the computer while it is connected.
`lamp skill check` runs on the computer, offline, and works anywhere.

**What is different about it:** every other grant ends with the
connection or the lease. **A pushed skill keeps running after the computer
goes**, as a skill: under lamp-skills' own caps and at a skill's priority,
not held to this page's rates. The pairing screen says so: "can put skills
on the lamp that run on their own".

- **The grant, `skills`,** a seventh, ticked by the owner at pairing. A
  pushed skill may ask only for what the pairing grants, mapped onto the
  skill format's `[permissions]` ([skill-format.md](skill-format.md)):
  `body`, `light` and `speak`, and `look` as the presence topics alone
  (`humans/faces/tracked`, `room/stood_up`). The topics the protocol's
  events take with no grant are open to it too: `clock/local`,
  `voice/state`, `lamp/pause` and `controls/touch` (lamp-skills'
  `PUSHED_TOPICS`). So a computer can't push a skill that does more than it
  may do live.
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
  lamp-skills' own quiet hours (planned, 22:00 to 07:00 local), whatever the
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

- **The reset** (the 5 s pinhole, setup's product plan) wipes
  `/var/lib/lampOS/skills-dev`: every pushed skill and the owner's yes to
  any that sees the picture (`.yes.json`) go with the owner, and the next
  owner's lamp has neither (Reviewer-three's P2 on #4514). The pairings
  themselves are lamp-control's to wipe.
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
  isn't asked again. Asking again is harmless: each change sets that
  computer's state (off, or held to these grants), so one the helper
  served but didn't answer in time is served the same way twice. That is
  also why the journal's `gave up telling the helper … after 3 tries` line
  ends "it may still be served": an ask that timed out after it was
  written may have reached the helper.
- **What the owner does then:** nothing else asks that change again, so
  the journal line, with the computer's id and the change, is the owner's
  cue. Revoking the computer tells the helper `off_pushed_by` again (if
  the lost change was a revoke's, pair the computer again first), so its
  pushed skills are off whichever change was lost; paired again, it pushes
  them again under its grants then. A re-pair with the same grants tells
  the helper nothing, so it isn't a way to send a lost `grants_pushed_by`.
- **On in the product:** `deploy/product/lamp-control.service` passes
  `--manage-socket /run/lampos-skills-manage/manage.sock`, the socket
  `lamp-skills-manage.socket` listens on, and is ordered after it (test
  `the_door_passes_the_skills_manage_socket_it_listens_on` in
  `lamp-update`). It waited for lamp-skills' manage helper to keep its
  record of revokes and grant changes (Eighth's piece B, `.revoked.json`):
  an install that the door stops waiting for at 60 s may still finish in
  the helper, and only that record refuses it if the computer's pairing
  changed meanwhile. The door itself is still off until the owner's
  developer setting writes `LAMP_CONTROL=on`.
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

Landed, on stable 10824: the server (WSS, the per-lamp certificate, pairing
and `hello` with Ed25519, #3736); the actions routed to their parts; the
events from the bus; the app's developer setting (`control.set`) and pairing
codes (`control.code`); the SDKs (`sdk/python`, `sdk/js`) and `lamp-mcp`
(`sdk/mcp`), installed from a checkout, not published to PyPI or npm.

Still to come:
- the mDNS announcement (`_lampos._tcp`), so `lamp find` and `lamp pair`
  without `--host` find a lamp;
- skill push on customer lamps (`--manage-socket` in the product unit);
- `joints` for `roll`, `shoulder` and `elbow`, `play`, and the microphone's
  sound;
- the fast camera stream on customer lamps (`--frames-socket` in the
  product lamp-eyes unit).

## Open questions

Nine's, for the first builder (#3504, 12:24):

1. ~~Is one pairing code over the owner's account channel "approval, never
   control"? Or must pairing stay on the LAN?~~ The app asks the code over
   the account channel (`control.code`, sealed to the app), and pairing
   itself is on the LAN, checked against the certificate's pin.
2. ~~Port 7731 and the name `lamp-control`.~~ Both shipped.
3. ~~Should `express` wait for the `feelings` skill (#3691) to fix its
   names?~~ It carries that skill's three, `glad`, `lively` and `low`; the
   schema still takes any short lowercase name, so a lamp with more is no
   SDK change.
4. The script runtime of phase 3: WASM or QuickJS.
