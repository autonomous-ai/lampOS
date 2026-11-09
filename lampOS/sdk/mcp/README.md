# lamp-mcp

One paired lampOS lamp as MCP tools, for an agent on your own computer: Claude
Code, Claude Desktop, or any MCP client. It runs on your computer, never on the
lamp, and reaches the lamp only through the [`lampos`](https://github.com/autonomous-ai/lampOS/tree/lamp-voice/lampOS/sdk/python) SDK and the
pairing `lamp pair` made. The protocol is [docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md).

```sh
pip install ./sdk/python ./sdk/mcp   # from lampOS/ in a checkout of the lamp-voice branch: neither is on PyPI
lamp pair 12345678 --pin 3f9a… --host lamp-xxxx.local  # once, with the code and the lamp's pin the app shows
claude mcp add lamp -- lamp-mcp
```

**On a customer lamp today (stable 10824):** lamp-control is on every lamp,
and answers once the owner turns on developer access in the app. Five gaps,
and what to do ([more](../README.md#on-a-customer-lamp-today-stable-10824)):
- **Skill push** is not yet on customer lamps: `lamp_skill_push`, `lamp_skills` and `lamp_skill_remove` (offered with the `skills` grant) are refused, "skill push is not on this lamp". Try them against `lamp fake`.
- **Joints and play:** `lamp_joints` takes `yaw` and `pitch` only; `roll`, `shoulder` and `elbow` are refused, and so is `lamp_play`. Use `lamp_look`, `lamp_gesture` or `lamp_move`.
- **Limp and pose:** the body's limp and hold-a-pose can't be reached from a computer.
- **No mDNS:** `lamp pair` without `--host` finds nothing. Give `--host lamp-xxxx.local` (the lamp's host name) or its IP address; the pairing keeps it.
- **Event times are UTC:** an event's `at` ends in `Z`. Convert to local time yourself.

Also: a gesture, move or feeling asked while another motion plays is refused `busy` (`try again in N ms`), and `lamp_look` on a resting lamp may end `cut_short` with no hand on it.

`--lamp ID` picks a lamp when this computer is paired with more than one, and
`--client NAME` names this computer in the lamp's leases and journal. `--verbose` prints every frame to and from the lamp on stderr, with the SDK's
`LAMPOS_LOG=debug` and its redaction: proofs, signatures, keys, nonces and pins
never, and a said text only by its length and a tag keyed for the run. stdout stays the MCP stream.
Phrases and events' fields are logged as they are, and your MCP client may keep a server's stderr in its own logs
(Claude Code does), so with `--verbose` the phrases the lamp matched can outlive the session.

**When the lamp restarts** (every update), lamp-mcp connects again by itself
and makes its event subscriptions again, pausing 1, 2, 5, then 10 s while the
lamp can't be reached. A tool called while the lamp is away answers `closed`;
the next call after it is back works. Leases end with the restart; the next
body tool takes a new one. It stops trying, and says why once on stderr, when
connecting again can't mend it: the computer was removed in the app (`unpaired`),
or another lamp answers at the address. Then pair again and restart lamp-mcp.

## Tools

Each is one op, offered only when the lamp granted what it needs. Every
parameter, as `src/lamp_mcp/server.py` declares it:

| tool | parameter | type | allowed | default | grant | lease |
|---|---|---|---|---|---|---|
| `lamp_state` | `which` | list of strings | some of `presence`, `light`, `mood`, `lamp`, `air`, `device`, `body`; a state the grants don't allow is refused | every state the grants allow | none (`presence`: look, `air`: air, `device`: device) | no |
| `lamp_wait_for` | `events` | list of strings, required | some of `person.arrived`, `person.left`, `person.stood_up`, `person.facing`, `phrase.heard`, `name.heard`, `sound.heard`, `hand`, `touch`, `button`, `time`, `lamp.changed`, `air.changed`; one the grants don't allow is refused before anything is sent | — | the event's: `person.*` and `hand` look, `phrase.heard` and `name.heard` hear, `sound.heard` sound, `air.changed` air | no |
| `lamp_wait_for` | `events` | list of strings, required | some of `person.arrived`, `person.left`, `person.stood_up`, `person.facing`, `phrase.heard`, `name.heard`, `sound.heard`, `hand`, `touch`, `button`, `time`, `lamp.changed`; one the grants don't allow is refused before anything is sent | — | the event's: `person.*` and `hand` look, `phrase.heard` and `name.heard` hear, `sound.heard` sound | no |
| | `phrases` | list of strings | the phrases to listen for; required with `phrase.heard` | none | | |
| | `timeout_s` | number | 0 to 120; more waits 120 | 60 | | |
| `lamp_express` | `feeling` | string, required | a feeling of the `feelings` skill: `glad`, `lively` or `low` (the lamp refuses another) | — | body, light | no |
| | `strength` | number | 0 to 1: from 0.5 up the whole move, a smaller step of it below | 0.5 | | |
| `lamp_gesture` | `name` | string, required | `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz` (the schema's offered gestures; the lamp itself also takes `idle` and `scanning` from a program, but an agent is offered the clips a person expects, rule 8) | — | body | no |
| `lamp_look` | `at` | string | `face`, `name` or `ahead` (stable 10824 refuses `face` and `name`, "not in this release"; a look there may end `cut_short`, "not there in time: something holds the head", with no hand on it: close to the target is arrived) | none: then `yaw` and `pitch` | body (and look for `face`) | yes, 10 s |
| | `yaw` | number | degrees, right positive, within the head's reach | 0 | | |
| | `pitch` | number | degrees, up positive, within the head's reach | 0 | | |
| `lamp_follow_hand` | `seconds` | number | 0 to 60; more follows 60. `look` at `hand` under a body lease of its own, with `hand` events heard meanwhile. It says how it ended (the hand gone, `seen: false`, after it was seen; time up; the lamp taking its body back; or `lamp_follow_stop` or `lamp_stop`), how many `hand` events told where it was, and where it last was (shares of the picture, never a picture). One at a time: a second while one runs is `busy`. A lamp that takes the look without saying it follows (before #4183) is `refused`, and its lease let go | 10 | body, look | yes, its own, listed on `lamp://lease` while it follows |
| `lamp_follow_stop` | none | | ends a running `lamp_follow_hand`, or one still starting as soon as it has started; "not following" when none runs | | body, look (listed with `lamp_follow_hand`) | lets go of its lease |
| `lamp_move` | `step_ms` | integer, required | 20 to 1000 | — | body | yes, 10 s |
| | `frames` | list of lists of integers, required | each 5 joint offsets in counts (pitch, roll, yaw, shoulder, elbow), the first and last all 0; the body checks it | — | | |
| `lamp_move_poses` | `poses` | list of objects, required | each the joints it turns in degrees from the standing pose: `pitch` (up positive), `roll`, `yaw` (right positive), `shoulder`, `elbow` (up positive), at most pitch and roll 17.6°, yaw 13.2°, shoulder and elbow 30.8°; eased there and back at 80% of the body's speeds, 1.5 to 10 s in all; one it can't make is `bad_request` naming the pose and joint, before anything is sent | — | body | yes, 10 s |
| | `hold_ms` | integer | 0 to 10000: how long each pose is held | 400 | | |
| `lamp_play` | `name` | string, required | a recorded move's name: lowercase letters, digits, `-` and `_`, 40 at most; a lamp that doesn't route `play` yet refuses it, as every lamp does today (stable 10824) | — | body | yes, 10 s |
| `lamp_joints` | `within_ms` | integer | 0 or more: how soon the joints get there (the body's caps pace each step). On stable 10824 only `yaw` and `pitch` go; `roll`, `shoulder` and `elbow` are refused, and with the body silent for 15 s both yaw and pitch must be named | 1000 | body | yes, 10 s, named in the request |
| | `pitch` | number | degrees, −180 to 180; at least one of the five | none | | |
| | `roll` | number | degrees, −180 to 180 | none | | |
| | `yaw` | number | degrees, −180 to 180 | none | | |
| | `shoulder` | number | degrees, −180 to 180 | none | | |
| | `elbow` | number | degrees, −180 to 180 | none | | |
| `lamp_light` | `colour` | string, required | `yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm` | — | light | no |
| | `level` | integer | 1 to 5 | 3 | | |
| | `for_ms` | integer | milliseconds; more than 30000 lights 30000 | 10000 | | |
| | `pattern` | string | `breathe`, `candle` or `pulse` | none: steady | | |
| `lamp_ring` | `pixels` | list of strings | one `#rrggbb` per pixel, from pixel 0 around the ring, the lamp's count of them (`ring_pixels` in the answer). Exactly one of `pixels` or `rgb` | none | light | no |
| | `rgb` | string | one `#rrggbb` for the whole ring | none | | |
| | `brightness` | integer | 0 to 100 of the given colours, under the ring's own ceiling | 100 | | |
| | `for_ms` | integer | milliseconds; more than 30000 lights 30000 | 10000 | | |
| `lamp_say` | `text` | string, required | words, at most 900 UTF-8 bytes, the voice's own cap (the lamp refuses more, so lamp-mcp does too), said exactly as written once the person isn't talking | — | speak | no |
| | `rephrase` | boolean | `true`: the lamp says it in its own words instead | `false` | | |
| | `wait` | boolean | false: returns once the lamp has taken the line ("taken"); true: once it was said ("said") or not (`dropped` with the voice's why), 120 s at most | false | | |
| `lamp_sound` | `name` | string, required | `hum`, the lamp's own short hum, or one of this computer's clips (`lamp_upload_sound`), played once the person isn't talking; refused in quiet hours where this lamp has them set, and by a lamp that doesn't route `sound` yet | — | speak | no |
| `lamp_upload_sound` | `name` | string, required | the clip's name: a lowercase letter, then lowercase letters, digits, `_` or `-`, 64 at most, never `hum`; replaces a clip of the same name | — | speak | no |
| | `path` | string, required | a WAV file on this computer: 16-bit mono PCM at 24 kHz, up to 30 s, no sample past −12 dBFS and no 10 ms past −30 dBFS RMS. Each computer keeps up to 64 clips and 20 MB on the lamp; revoking it deletes them | — | | |
| `lamp_sounds` | none | | | | speak | no |
| `lamp_remove_sound` | `name` | string, required | one of this computer's clips | — | speak | no |
| `lamp_volume` | `to` | integer | 0 to 100 of what the lamp allows; 0 is muted. Exactly one of `to`, `step` or `mute`. The person's own volume, the same they set by voice or in the app; it returns the level it left, `{level, muted}` | none | speak | no |
| | `step` | string | `up` or `down`: the lamp's own step, within its floor and ceiling | none | | |
| | `mute` | boolean | `true` mutes; `false` brings it back to where it was | none | | |
| `lamp_start_turn` | `prompt` | string | words the lamp says first, at most 900 UTF-8 bytes, as `lamp_say`. Opens a conversation turn as a tap on the head does: the person's next line is answered as spoken to the lamp. What the person says never comes back, only how the turn ended (`turn finished`, or `turn dropped: …` with the voice's why); refused in quiet hours where this lamp has them set, as `lamp_say` | none | speak, hear | no |
| | `wait` | boolean | return when the turn ends, 120 s at most | false | | |
| `lamp_end_turn` | none | | ends the turn this computer started, never another's; with none, `busy: not this computer's turn` | | speak | no |
| `lamp_set_listening` | `on` | boolean, required | `false` mutes the lamp's listening for this computer until `true`, a hand on its head, or lamp-mcp going (its connection closes); `lamp_stop` doesn't lift it. The person always wins | — | hear | no |
| `lamp_events` | `since` | integer | a `seq` from an earlier answer; 0 for every event kept (the last 200) | 0 | the event's (`person.*` and `hand` look, `name.heard` hear, `sound.heard` sound; `touch`, `button`, `lamp.changed` none) | no |
| | `wait_s` | number | 0 to 120: with nothing after `since`, how long to wait for the next | 0 | | |
| `lamp_stop` | none | | | | none | lets go of every lease this server holds |

**An argument a tool doesn't take is refused**, never read past. Each
tool's schema says `"additionalProperties": false`, and a call such as
`lamp_light(colour="warm", for_s=5)` comes back as the tool's error:
`bad_request: lamp_light takes no for_s; it takes colour, for_ms, level, pattern`.
Before, it lit the default 10 s. A call missing an argument the tool
needs is refused the same way, `bad_request: lamp_light needs colour`, never
in the validator's words. `serverInfo.version` is lamp-mcp's own.

`lamp_joints` aims named joints once; it is not a 20 Hz stream. `play` needs a
skill package; `sound` plays `hum` or a clip the computer uploaded. With the
`camera` grant, `lamp_camera_snapshot` returns the newest picture (on stable
10824 from the eyes' sight file; the fast frame path is not on customer
lamps). With `skills`, `lamp_skill_push`, `lamp_skills` and
`lamp_skill_remove` are offered; customer lamps refuse them (see above).

**Leases** are taken for you: a body tool (`lamp_look`, `lamp_move`,
`lamp_play`, `lamp_joints`) takes a 10 s body lease, or renews the one this
server still holds, so tools called one after another aren't refused `busy`.
It is let go at the action's `done`, or left to lapse 10 s after the last body
tool if `done` doesn't come: while an agent keeps calling body tools, the
lamp's own life (its rest glance, its feelings' small moves) stands back, and
it comes back about 10 s after the agent goes idle. A lease another computer holds, or one the lamp suspends because
someone is talking to it, comes back as the tool's error, never retried.

**Refusals** come back as the tool's error, with the lamp's code and words:
`busy`, `refused: centre of gravity 27 mm > 22 mm`, `quiet_hours`,
`rate: … (try again in 900 ms)`. On stable 10824 and older, a gesture, move
or feeling asked while another motion plays is `busy: busy: a motion plays
for N ms more (try again in N ms)`: wait that long and call it again. A `busy`
without a time is a part someone else holds. The server keeps to the lamp's `limits` itself,
so an agent in a loop is slowed before the lamp refuses it.

`lamp_state`, and the `lamp://state/<name>` resources, return the states by name (`{"presence": {...}}`), as lamp-control's `get` carries them under `state`. Before 0.2.0 they returned that whole `{"state": {...}}`. A `light` or `mood` the lamp hasn't heard yet is `null`.

## Resources

All JSON, read when asked; none takes a parameter.

| resource | what it is | grant |
|---|---|---|
| `lamp://state/presence` | who is there: a count and how near, never who | look (listed only with it) |
| `lamp://state/light` | its light | none |
| `lamp://state/mood` | its mood | none |
| `lamp://state/lamp` | whether it is talking, paused, private or asleep | none |
| `lamp://state/volume` | its speaker's volume: `level` 0 to 100 and `muted` | speak (listed only with it) |
| `lamp://state/body` | what its body may do, joint by joint (degrees from home, bounds, poses, speed, load and heat limits), and where each joint is | body (listed only with it) |
| `lamp://lease` | the leases this server holds now | none |
| `lamp://events/recent` | the last 50 events `lamp_wait_for` heard, newest last; never a transcript: `phrase.heard` says only which registered phrase it was | none |
| `lamp://events` | the events stream: every event the grants let this computer hear but `phrase.heard`, the last 200, newest last, each with its `seq`, and `latest`. Subscribable: a subscribed client gets `notifications/resources/updated` for each new event, and for `lamp://state/presence` or `lamp://state/lamp` when one changes it | none (each event needs its own grant) |

## Tests

Against the SDK's `FakeLamp`, with no lamp:

```sh
pip install -e ./sdk/python -e './sdk/mcp[test]'
pytest sdk/mcp/tests
```
