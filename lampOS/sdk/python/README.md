# lampos: program a lampOS lamp from your computer

The Python client for the lamp's v1 control protocol
([docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md),
messages in
[control.schema.json](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.schema.json)).
It runs on your computer, never on a lamp.

**New here?** [docs/develop.md](../../docs/develop.md) is the 10-minute quick-start: the fake lamp, a first program, pairing, lamp-mcp and skills.

**Not usable on a lamp yet:** the lamp's side, `lamp-control`, is being built
(#3504). Until then, try it with the fake lamp below.

## Install

It isn't on PyPI yet. From a checkout of this repository:

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python             # or -e, to work on it; '.[find]' to find lamps by mDNS
```

## Try it with the fake lamp (no lamp needed)

`lamp fake` starts a fake lamp on this computer with every grant, pairs this
computer with it at once, and every 10 seconds says a person arrived, the
phrase "good night lamp", and the person left, printing what it is asked.
Keep the trial apart from real pairings with its own `LAMPOS_HOME`:

```sh
# terminal 1
LAMPOS_HOME=/tmp/lampos-try lamp fake
# terminal 2 (from lampOS/sdk/python)
LAMPOS_HOME=/tmp/lampos-try python examples/greet.py
```

Terminal 1 then shows `asked {"op": "gesture", "name": "greeting"}`. Every
example runs this way: `greet.py`, `follow.py`, `phrase.py`, `pomodoro.py`,
`ci_light.py`.

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
`express` as the feelings skill's move and light,
and `play`, `sound`, `look` and `joints` refused (`refused`,
"not routed yet: waits for …"), in lamp-control's own words, so a program
built against it meets the same refusals on a lamp. `--everything` answers
every op instead.

## Pair with a real lamp

```sh
lamp pair 12345678 --pin 3f9a…              # the code and the lamp's pin (64 hex), both shown in the app
```

The lamp's certificate is checked against the pin before any frame is sent
(only the TLS handshake and the WebSocket's upgrade request, which carry no
secret, go first), so the code's proof never reaches another host, and the pin is inside the
proof too. There is no pairing without it. The pairing is kept in
`~/.config/lampos/` (or `$LAMPOS_HOME`, which is made private too, so point it at a folder of its own): the folder is yours alone (0700),
the key and `lamps.json` 0600, and the pin is checked again on every
connection.

## Program it

```python
import asyncio
from lampos import Lamp

async def main():
    async with await Lamp.connect() as lamp:
        await lamp.on("person.arrived", lambda event: lamp.gesture("greeting"))
        await lamp.wait_closed()

asyncio.run(main())
```

- **Events:** `await lamp.on(name, handler)` (a handler may be a
  coroutine) subscribes, and raises the lamp's refusal, `NoGrantError`
  without the event's grant: `person.*` needs `look`, `phrase.heard` needs
  `hear`. Not awaited, the refusal comes out of `wait_closed()` instead.
  `on()` returns an awaitable `Subscription`, not a future: to wait on
  several, `await asyncio.gather(lamp.on(...), lamp.on(...))`.
  `lamp.off(name, handler)`. Each needs its grant:

  | event | fields | grant |
  |---|---|---|
  | `person.arrived`, `person.left` | `count`, `near` (0–1, share of the picture's height) | `look` |
  | `person.stood_up` | `side` | `look` |
  | `person.facing` | `facing` | `look` |
  | `hand` | `seen`; when seen, `x`, `y` (the palm's centre, 0–1 from the left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. By edge, at most 4 a second; `seen: false` once, 1 s after the last hand | `look` |
  | `phrase.heard` | `phrase`: one you registered (`on(..., phrases=[...])`), never what else was said | `hear` |
  | `name.heard` | | `hear` |
  | `sound.heard` | `class`, `score` | `sound` |
  | `touch` | `kind` (`tap`, `pet`, `hold`), `where` | none |
  | `button` | `press` (`press`, `long_press`) | none |
  | `time` | your schedule | none |
  | `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `lease`, and `quiet_hours` (planned: not produced yet; the door already refuses `say` and `sound` in quiet hours, or while the lamp's local time isn't known, with the error `quiet_hours`) | none |

- **Actions:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`. Each returns when the lamp has taken it;
  `wait=True` on a gesture, move or play returns when it has finished, or,
  where the lamp hands it on and says no more (today `gesture` and `move`,
  answered `{"to": "body"}`), at that answer.
- **Leases:** `async with lamp.lease("body"):` holds the body for looks,
  moves and joints, renewed for you, until the block ends or the lamp takes
  it back (a hand on its head, a conversation): `lease.state`,
  `lease.lost_why`.
- **Refusals** are typed: `lampos.errors.BY_CODE["rate"]` (with
  `retry_ms`), `["no_grant"]`, `["private"]`, ...
- **In your tests:** `lampos.testing.FakeLamp(grants=...)` is the same fake
  lamp: `emit(event, **fields)`, `refuse_next(op, code, why)`,
  `done(re)`, `lease_event(lease, state, why)`, and `requests` /
  `received` for what it took.

**When the lamp restarts** (every update), the connection closes and
`wait_closed()` returns: a long-running program connects again, for example
in a loop with a few seconds' pause.

**Dependencies** (`websockets`, `cryptography`; `zeroconf` to find lamps)
have lower bounds only, as a library's should: the application that uses
it pins them.

Tests: `PYTHONPATH=src python3 -m pytest tests` (needs `pytest`; with
`jsonschema`, the fake lamp checks every request against the schema).

## Check a skill folder

A skill is a folder the lamp runs without code: its rules in `skill.toml`
([the skill format, version 1](../../docs/skill-format.md)). Check one
before it goes to a lamp or the Store:

```sh
pip install './lampOS/sdk/python[check]'   # the format's schema check needs jsonschema
lamp skill check path/to/my-skill
```

It checks SKILL.md's front matter as the lamp reads it (`---` exactly on
the first line, `name:` and `description:` at a line's start, a value taken
as written, quotes and all), with the folder's own `name`; `skill.toml`'s
shape against the format's schema (a misspelt key is an error, as on the
lamp); that the folder holds no script (by suffix, executable bit or `#!`)
and no link where the lamp reads (it never follows one); its moves, held to
lamp-body's own bounds (name, `step_ms` 40 to 1000, 1.5 to 10 s, from rest
to rest, each joint's reach and speed); and its sounds (16-bit mono 24 kHz,
3 s at most). A file it can't read is that folder's error, and the next
folder is still checked. It warns
on `[permissions] look = true` without `camera = true`, which format 1
still reads as the full picture: ask `camera` for the picture; in format 1,
presence is the presence topics (`humans/faces/tracked`, `room/stood_up`). A
skill that says `format = 2` reads `look` as presence alone and `camera` as
the only picture, so a `look = true` step there needs `camera = true`; a
format other than 1 or 2 is refused. The rules between keys, such as permissions
covering every read, are the lamp's own load check: a folder that passes
here can still be refused there, with its reason.
