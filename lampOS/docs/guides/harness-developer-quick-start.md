# Program your lamp from your computer: developer quick start

You can read and drive your own lamp from a program on your computer: light it, make it speak, move it, hear its own phrases, see that someone arrived. This page gets you from nothing to a program that runs, in about ten minutes. For the full tour, see [develop.md](../develop.md).

## What you get

- **Pairing a computer with your lamp.** The owner pairs a computer once with a code, choosing what it may do. The Autonomous app's *Add a developer computer* screen is meant to make that code, but it is not out yet (see "What works today"); until it ships, the lamp's keeper makes it. The computer then connects to the lamp on your home network. The owner can revoke it at any time. The person always wins: a hand on the lamp's head, "stop", the privacy switch, or a conversation takes control back from your program ([develop.md](../develop.md), "What takes control back").
- **The SDK.** Python (`lampos`) and JavaScript (`lampos`), the same calls in both: events (`person.arrived`, `phrase.heard`, `touch`...), state (`get`), and actions (`light`, `say`, `gesture`, `express`, `sound`...). Each needs a grant the owner gave. See [the SDK map](../../sdk/README.md) for every call and its status.
- **A fake lamp** on your own computer, so you can try everything with no lamp.
- **Also there:** `lamp-mcp`, to let an agent drive the lamp ([sdk/mcp](../../sdk/mcp/README.md)), and skills you push to the lamp ([develop.md](../develop.md), part 5).

What your program never gets: camera frames or audio unless the owner grants `camera` or `microphone` and the lamp isn't private, face prints, names, or the words around a phrase it listens for.

**Harness and the SDK are two different doors.** The SDK talks to the lamp from your computer. Harness is a separate program, by the Harness team, that carries the lamp's spoken tasks to the AI helpers on your computer ([the getting-started guide](harness-getting-started.md)). You can use either or both.

## What works today

| Piece | State today |
|---|---|
| The SDKs and the fake lamp | Merged. Use them now |
| An example against the fake lamp | Runs (output below) |
| The lamp's side, `lamp-control` | Merged, but **not a product part yet**: it runs only when the lamp's keeper turns it on (`LAMP_CONTROL=on`). A pull request that makes it a product part is open |
| The app's *Add a developer computer* screen | **Not out yet.** The lamp's side is merged, so the app can ask the lamp for a code once the screen ships |

So today a real lamp pairs only when its keeper turns lamp-control on and makes the pairing code at the lamp. Ask your lamp's keeper, or use the fake lamp below.

## 1. Install the SDK (2 minutes)

You need Python 3.10 or later. The SDK is not on PyPI yet, so install from a checkout of this repository. On Debian or Ubuntu run `sudo apt install python3-venv` first.

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python
```

## 2. Start a fake lamp (1 minute)

The fake lamp is a lamp on your computer with every grant, already paired with you. Give the trial its own `LAMPOS_HOME` so it never touches a real pairing.

Open two terminals. In **each** one, activate the environment from step 1 first (`. ~/lampos-venv/bin/activate`), or the `lamp` and `python` commands won't find the SDK.

```sh
# terminal 1
. ~/lampos-venv/bin/activate
LAMPOS_HOME=/tmp/lampos-try lamp fake
```

## 3. Run one example (2 minutes)

Save [examples/first_light.py](examples/first_light.py) as `first_light.py`:

```python
"""First program: read what the lamp is doing, light it warm, say hello, nod."""

import asyncio

from lampos import Lamp
from lampos.errors import LampError

async def main():
    async with await Lamp.connect() as lamp:
        print("connected to", lamp.info.get("id"), "with", lamp.grants)
        print("lamp state:", await lamp.get("lamp"))
        try:
            await lamp.light("warm", 3, 5_000, pattern="breathe")
            await lamp.say("Hello from my computer.")
            await lamp.gesture("nod")
        except LampError as refused:  # private, paused, asleep, busy, quiet_hours...
            print("not now:", refused)
        await asyncio.sleep(1)  # stay connected a moment: closing hands the light back

asyncio.run(main())
```

```sh
# terminal 2
. ~/lampos-venv/bin/activate
LAMPOS_HOME=/tmp/lampos-try python first_light.py
```

Terminal 2 prints:

```text
connected to lamp-fake with ['body', 'light', 'speak', 'look', 'hear', 'sound', 'skills', 'air', 'device', 'camera', 'microphone']
lamp state: {'lamp': {'talking': False, 'paused': False, 'private': False, 'asleep': False}}
```

Terminal 1 shows what the fake lamp was asked. Its own "said person.arrived" line (it announces a visitor every few seconds) is part of the fake and appears too:

```text
said person.arrived
  asked {"op": "get", "state": ["lamp"]}
  asked {"op": "light", "colour": "warm", "level": 3, "pattern": "breathe", "for_ms": 5000}
  asked {"op": "say", "text": "Hello from my computer."}
  asked {"op": "gesture", "name": "nod"}
```

Press Ctrl-C in terminal 1 when you're done.

Why it is written this way:

- `Lamp.connect()` signs in with this computer's key, and `lamp.grants` says what the owner allowed.
- A refusal is a typed error (`private`, `paused`, `asleep`, `busy`, `quiet_hours`, `no_grant`). Catch it: your program should expect the lamp to say "not now".
- Your light lasts only while you stay connected, so the program waits a moment before it ends.
- A motion asked while another still plays is refused `busy` with a `retry_ms`: wait, then ask again. Don't loop motions; a lamp that twitches every few seconds reads as a machine.

## 4. Point it at your real lamp

1. **Get a pairing code.** The owner's code comes with the lamp's pin (64 hex characters) and lasts 5 minutes. For this example tick three grants: `light` (the light), `speak` (the voice) and `body` (the nod).
   - **When the app screen ships:** in the Autonomous app, open your lamp and choose *Add a developer computer*. The lamp also says the first 8 characters of the pin aloud, and they must match what the app shows.
   - **Today:** on a lamp with lamp-control turned on, the lamp's keeper runs `lamp-control code --grants body,light,speak` as root. It prints the code and the pin.
2. **On your computer**, with the environment active and no `LAMPOS_HOME` set:

```sh
lamp pair 12345678 --pin <the 64 characters of the pin>
lamp hello                 # connects and shows the grants
python first_light.py      # now it runs against your lamp
```

The SDK checks the lamp's certificate against the pin before it sends anything, so the code never reaches another machine. Pairing is kept in `~/.config/lampos/`. The owner can revoke a computer at any time, which closes its connections at once.

## Where next

- [develop.md](../develop.md): events, state, leases, `lamp-mcp`, skills.
- [The SDK map](../../sdk/README.md): every capability, its call in each SDK, its status, whether it was validated on a lamp.
- [control.md](../control.md): the protocol.
- JavaScript: the same calls in camelCase, `npm i /path/to/lampOS/sdk/js`, and `lampos fake` ([sdk/js](../../sdk/js/README.md)).

