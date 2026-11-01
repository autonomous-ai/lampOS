# lamp-mcp

One paired lampOS lamp as MCP tools, for an agent on your own computer: Claude
Code, Claude Desktop, or any MCP client. It runs on your computer, never on the
lamp, and reaches the lamp only through the [`lampos`](https://github.com/autonomous-ai/lampOS/tree/lamp-voice/lampOS/sdk/python) SDK and the
pairing `lamp pair` made. The protocol is [docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md).

```sh
pip install ./sdk/python ./sdk/mcp
lamp pair 12345678 --pin 3f9a…  # once, with the code and the lamp's pin the app shows
claude mcp add lamp -- lamp-mcp
```

`--lamp ID` picks a lamp when this computer is paired with more than one, and
`--client NAME` names this computer in the lamp's leases and journal. `--verbose` prints every frame to and from the lamp on stderr, with the SDK's
`LAMPOS_LOG=debug` and its redaction: proofs, signatures, keys, nonces and pins
never, and a said text only by its length and a tag keyed for the run. stdout stays the MCP stream.
Phrases and events' fields are logged as they are, and your MCP client may keep a server's stderr in its own logs
(Claude Code does), so with `--verbose` the phrases the lamp matched can outlive the session.

## Tools

Each is one op, offered only when the lamp granted what it needs. Every
parameter, as `src/lamp_mcp/server.py` declares it:

| tool | parameter | type | allowed | default | grant | lease |
|---|---|---|---|---|---|---|
| `lamp_state` | `which` | list of strings | some of `presence`, `light`, `mood`, `lamp`; a state the grants don't allow is refused | every state the grants allow | none (`presence`: look) | no |
| `lamp_wait_for` | `events` | list of strings, required | some of `person.arrived`, `person.left`, `person.stood_up`, `person.facing`, `phrase.heard`, `name.heard`, `sound.heard`, `hand`, `touch`, `button`, `time`, `lamp.changed`; one the grants don't allow is refused before anything is sent | — | the event's: `person.*` and `hand` look, `phrase.heard` and `name.heard` hear, `sound.heard` sound | no |
| | `phrases` | list of strings | the phrases to listen for; required with `phrase.heard` | none | | |
| | `timeout_s` | number | 0 to 120; more waits 120 | 60 | | |
| `lamp_express` | `feeling` | string, required | a feeling of the `feelings` skill: `glad`, `lively` or `low` (the lamp refuses another) | — | body, light | no |
| | `strength` | number | 0 to 1: from 0.5 up the whole move, a smaller step of it below | 0.5 | | |
| `lamp_gesture` | `name` | string, required | `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz` (the schema's gestures less `idle` and `scanning`) | — | body | no |
| `lamp_look` | `at` | string | `face`, `name` or `ahead` | none: then `yaw` and `pitch` | body (and look for `face`) | yes, 10 s |
| | `yaw` | number | degrees, right positive, within the head's reach | 0 | | |
| | `pitch` | number | degrees, up positive, within the head's reach | 0 | | |
| `lamp_follow_hand` | `seconds` | number | 0 to 60; more follows 60. `look` at `hand` under a body lease of its own, with `hand` events heard meanwhile. It says how it ended (the hand gone, `seen: false`, after it was seen; time up; the lamp taking its body back; or `lamp_follow_stop` or `lamp_stop`), how many `hand` events told where it was, and where it last was (shares of the picture, never a picture). One at a time: a second while one runs is `busy`. A lamp that takes the look without saying it follows (before #4183) is `refused`, and its lease let go | 10 | body, look | yes, its own, listed on `lamp://lease` while it follows |
| `lamp_follow_stop` | none | | ends a running `lamp_follow_hand`, or one still starting as soon as it has started; "not following" when none runs | | body, look (listed with `lamp_follow_hand`) | lets go of its lease |
| `lamp_move` | `step_ms` | integer, required | 20 to 1000 | — | body | yes, 10 s |
| | `frames` | list of lists of integers, required | each 5 joint offsets in counts (pitch, roll, yaw, shoulder, elbow), the first and last all 0; the body checks it | — | | |
| `lamp_play` | `name` | string, required | a recorded move's name: lowercase letters, digits, `-` and `_`, 40 at most; a lamp that doesn't route `play` yet refuses it | — | body | yes, 10 s |
| `lamp_joints` | `within_ms` | integer | up to 5000: how soon the joints get there | 1000 | body | yes, 10 s, named in the request |
| | `pitch` | number | degrees, −180 to 180; at least one of the five | none | | |
| | `roll` | number | degrees, −180 to 180 | none | | |
| | `yaw` | number | degrees, −180 to 180 | none | | |
| | `shoulder` | number | degrees, −180 to 180 | none | | |
| | `elbow` | number | degrees, −180 to 180 | none | | |
| `lamp_light` | `colour` | string, required | `yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm` | — | light | no |
| | `level` | integer | 1 to 5 | 3 | | |
| | `for_ms` | integer | milliseconds; more than 30000 lights 30000 | 10000 | | |
| | `pattern` | string | `breathe`, `candle` or `pulse` | none: steady | | |
| `lamp_say` | `text` | string, required | 300 characters at most | — | speak | no |
| `lamp_sound` | `name` | string, required | a sound's name: lowercase letters, digits, `-` and `_`, 40 at most; `hum`, the lamp's own short hum, is the only one in this release; refused in quiet hours, and by a lamp that doesn't route `sound` yet | — | speak | no |
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

`joints` is left out: a 20 Hz stream isn't something an agent drives turn by
turn. `play` needs a skill package; `sound` takes only `hum` in this release.

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
`rate: … (try again in 900 ms)`. The server keeps to the lamp's `limits` itself,
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
| `lamp://lease` | the leases this server holds now | none |
| `lamp://events/recent` | the last 50 events `lamp_wait_for` heard, newest last; never a transcript: `phrase.heard` says only which registered phrase it was | none |
| `lamp://events` | the events stream: every event the grants let this computer hear but `phrase.heard`, the last 200, newest last, each with its `seq`, and `latest`. Subscribable: a subscribed client gets `notifications/resources/updated` for each new event, and for `lamp://state/presence` or `lamp://state/lamp` when one changes it | none (each event needs its own grant) |

## Tests

Against the SDK's `FakeLamp`, with no lamp:

```sh
pip install -e ./sdk/python -e './sdk/mcp[test]'
pytest sdk/mcp/tests
```
