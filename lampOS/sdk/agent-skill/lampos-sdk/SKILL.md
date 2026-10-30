---
name: lampos-sdk
description: Program a lampOS lamp from a developer's own computer with the lampos SDK (Python or JavaScript) or the lamp-mcp server. Use when someone asks to program the lamp, control a lampOS lamp, make the lamp react to something (presence, a build, a timer, an agent's progress, a message), write a lamp script or skill, pair a computer with a lamp, or test lamp code against the fake lamp.
---

# Programming a lampOS lamp

A lampOS lamp is a desk lamp with a body (head and arm), a light ring, a
voice, eyes and touch. A computer the owner paired with it can ask it to
move, light, speak and look, and hear what it notices, over one protocol
(`lamp-control`, WebSocket over TLS on port 7731). Three ways in, the same
protocol underneath:

| you write | use | install |
|---|---|---|
| Python | `lampos` | `pip install "git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/python"` |
| JavaScript (Node 20.11+) | `lampos` | clone the repo, then `npm install ./lampOS/sdk/js` |
| an AI agent's tools | `lamp-mcp` | both in one command: `pip install "lampos @ git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/python" "lamp-mcp @ git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/mcp"`, then `claude mcp add lamp -- lamp-mcp` (or any MCP client) |

The packages aren't on PyPI or npm yet; install from the repository as above.
`lamp-mcp` needs `lampos` in the same `pip` command, or pip looks for it on
PyPI and fails.

## Start without a lamp

Always build and test against the **fake lamp** first: it speaks the real
protocol and answers as lamp-control does.

```sh
lamp fake --everything     # a fake lamp, paired with this computer; leave it running
python my_program.py       # in another terminal, same LAMPOS_HOME
```

`--everything` answers every op; without it the fake refuses what real lamps
don't route yet, as they do. In tests, use `lampos.testing.FakeLamp` (JS:
`FakeLamp` from `lampos/testing`): see [references/testing.md](references/testing.md).

## Pair with a real lamp

The owner makes a code in the lamp's app: an 8-digit code and the
certificate's pin (64 hex). Then, on the computer:

```sh
lamp pair CODE --pin PIN --host lamp-xxxx.local --name my-laptop
lamp hello                 # connects, says this computer's grants
python scripts/check_pairing.py   # this skill's check: grants, limits, what each refusal means
```

The pairing (a key and the lamp's pin) is kept in `~/.config/lampos`, or
`LAMPOS_HOME`.

## Reaching the lamp from off its network

By default the lamp answers only computers on its own network. Two ways from
elsewhere:

1. **The lamp's remote opt-in**, on a lamp whose lamp-control has it (`--remote`, or
   `LAMP_CONTROL_REMOTE=on`): off by default, and **the owner must switch it
   on** on the lamp. Then any address that reaches port 7731 is admitted
   (still only paired computers). Camera and microphone never leave the
   lamp's own network, whatever this says.
2. **An ssh tunnel to the lamp**, which needs the lamp's ssh: the owner's to
   give, and nothing else to switch on. `ssh -N -L 7731:127.0.0.1:7731
   orangepi@lamp-xxxx.local`, then `--host 127.0.0.1` for `lamp pair` and
   `Lamp.connect(host="127.0.0.1")`. Through a tunnel the lamp sees
   `127.0.0.1`, which is its own network (lamp-control counts loopback as
   local), so **a tunnel carries every grant, the camera and microphone
   included.** Never use or suggest a tunnel to get the camera or microphone
   off the lamp's network: that is the owner's decision when they give ssh,
   not a way around item 1.

## The mental model (read before writing code)

- **Grants:** the owner ticks what this computer may do: `body` (move),
  `light`, `speak`, `look` (presence, hands, face/hand targets), `hear`
  (phrases, the name), `sound` (sound classes), `skills` (push skills).
  A call without its grant is refused `no_grant`. Grants are read at hello.
- **Leases:** moving the body (`look`, `move`, `play`, `joints`) needs a
  `body` lease. Hold one with `async with lamp.lease("body"):` (JS
  `lamp.withLease(['body'], async (lease) => …)`). The lamp can take it back
  (a conversation, a hand on the head, private): you get a `lease` event
  with `state: lost` and the why. `gesture` and `express` need no lease.
- **Who wins:** the person always. A conversation, a touch, the app, private
  or paused mode come first, and the lamp tells you (`lease`, `lamp.changed`).
  Behind the person, one computer at a time holds the body.
- **Limits:** by default the rates are off: hello's `limits` says
  `"rates": "off"`, and only refused actions (20 a second, then the
  connection closes) and phrase changes (6 a minute) are bounded. A lamp run
  with `--rates default` sends a window for each kind. The body bounds every
  move whatever the rate. Read `lamp.limits` with `.get()`: an older lamp or
  fake leaves keys out.
- **Closing ends what you asked for:** when your program's last connection
  closes, the lamp gives back its light and stops its waiting lines at once.
  A script that lights the ring and exits shows it for a moment only: stay
  connected for as long as it should show.
- **One motion at a time:** a gesture, move or feeling's move asked while
  another still plays is refused `busy` with `retry_ms`. Wait and ask again.
- **Quiet hours:** a lamp setting (`limits.quiet_hours`, by default 22:00 to
  07:00, or `off`). `say` and `sound` are refused `quiet_hours` then; the
  light still works.
- **Be a living thing, not a machine:** react to something that happened, let
  a move finish, don't loop gestures. A lamp switching feelings every 3 s
  reads as twitchy.

## The calls

Every call, side by side in Python, JavaScript and MCP:
[references/api.md](references/api.md). The shape, in Python:

```python
import asyncio
from lampos import Lamp
from lampos.errors import LampError

async def main():
    async with await Lamp.connect() as lamp:          # the paired lamp
        await lamp.light("green", 3, 10_000, pattern="breathe")
        try:
            await lamp.say("The build passed.")
        except LampError as refused:                  # quiet_hours, private, busy...
            print(refused.code, refused.why)
        await lamp.on("person.arrived", lambda event: print("someone came", event))
        await lamp.wait_closed()

asyncio.run(main())
```

Every refusal is a `LampError` with `code` and `why`, the lamp's own words;
[references/errors.md](references/errors.md) has what to do about each code.

## Reading the lamp

- **Events** come by edge, when something changes: `person.arrived`,
  `person.left`, `person.facing`, `person.stood_up`, `hand`, `phrase.heard`,
  `name.heard`, `sound.heard`, `touch`, `button`, `lamp.changed`, `time`;
  `lease` and `done` come unasked. Subscribe with `lamp.on(event, handler)`.
- **State** with `lamp.get("presence", "light", "mood", "lamp")`.
- What each carries, and its grant: [references/api.md](references/api.md#events-and-state).
- Never readable, by any grant: pictures, audio, transcripts, face prints,
  names. A hand's or face's place is a share of the picture.
- **Coming:** camera, microphone, `body.state` and routed `joints` are being
  built; this skill gains them as they land.

## Patterns

Ready to adapt, each in Python and JS: react to presence, a CI light, a
pomodoro, an agent's progress shown on the lamp, a chat alert:
[references/patterns.md](references/patterns.md).

## Gotchas we hit

Read [references/gotchas.md](references/gotchas.md) before your first real
run: breathe and pulse levels, one pulse a minute, `busy` while a motion
plays, the light's `shows`, the restart mute.

## Debugging

`LAMPOS_LOG=debug` prints every frame to and from the lamp on stderr, secrets
redacted. The lamp's side and the journal recipe:
[references/debugging.md](references/debugging.md).
