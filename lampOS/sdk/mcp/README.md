# lamp-mcp

One paired lampOS lamp as MCP tools, for an agent on your own computer: Claude
Code, Claude Desktop, or any MCP client. It runs on your computer, never on the
lamp, and reaches the lamp only through the [`lampos`](../python) SDK and the
pairing `lamp pair` made. The protocol is [docs/control.md](../../docs/control.md).

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

Each is one op, offered only when the lamp granted what it needs.

| tool | op | grant |
|---|---|---|
| `lamp_state(which)` | `get`: presence (a count and how near, never who), light, mood, and talking/paused/private/asleep; by default every state the grants allow | none (presence: look) |
| `lamp_wait_for(events, phrases, timeout_s)` | `subscribe`, the first matching event or `timed_out` (120 s at most), then `unsubscribe`; an event the grants don't allow is refused before anything is sent | the event's: `person.*` look, `phrase.heard` and `name.heard` hear, `sound.heard` sound |
| `lamp_express(feeling, strength)` | `express` | body, light |
| `lamp_gesture(name)` | `gesture`: the lamp's own gestures, the schema's list less `idle` and `scanning` | body |
| `lamp_look(at \| yaw, pitch)` | `look`, under a body lease of its own | body (and look for `face`) |
| `lamp_move(step_ms, frames)` | `move`, under a body lease of its own | body |
| `lamp_light(colour, level, for_ms, pattern)` | `light`, 30 s at most | light |
| `lamp_say(text)` | `say`, 300 characters at most | speak |
| `lamp_stop()` | `stop`, and every lease let go | none |

`joints` is left out: a 20 Hz stream isn't something an agent drives turn by
turn. `play` and `sound` need a skill package.

**Leases** are taken for you: `lamp_look` and `lamp_move` take a 10 s body lease,
never renewed. It is let go at the action's `done`, or left to lapse if `done`
doesn't come. A lease another computer holds, or one the lamp suspends because
someone is talking to it, comes back as the tool's error, never retried.

**Refusals** come back as the tool's error, with the lamp's code and words:
`busy`, `refused: centre of gravity 27 mm > 22 mm`, `quiet_hours`,
`rate: … (try again in 900 ms)`. The server keeps to the lamp's `limits` itself,
so an agent in a loop is slowed before the lamp refuses it.

## Resources

`lamp://state/presence`, `lamp://state/light`, `lamp://state/mood`,
`lamp://state/lamp`, `lamp://lease` (the leases this server holds), and
`lamp://events/recent` (the last 50 events `lamp_wait_for` heard; never a
transcript: `phrase.heard` says only which registered phrase it was). `lamp://state/presence`
is listed only when the lamp granted `look`.

## Tests

Against the SDK's `FakeLamp`, with no lamp:

```sh
pip install -e ./sdk/python -e './sdk/mcp[test]'
pytest sdk/mcp/tests
```
