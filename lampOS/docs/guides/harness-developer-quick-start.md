# Program your lamp from your computer: developer quick start

**Draft for #5413. Not live until the SDK local leader has checked every line marked TO CHECK, and until the pieces named under "What works today" are on stable (remove the "Notes for reviewers" section at the end before publishing).**

You can read and drive your own lamp from a program on your computer: light it, make it speak, move it, hear its own phrases, see that someone arrived. This page gets you from nothing to a program that runs, in about ten minutes. For the full tour, see [develop.md](../develop.md).

## What you get

- **Pairing a computer with your lamp.** The owner pairs a computer once, from the Autonomous app (*Add a developer computer*), choosing what it may do. The computer then connects to the lamp on your home network. The owner can revoke it at any time. The person always wins: a hand on the lamp's head, "stop", the privacy switch, or a conversation takes control back from your program ([develop.md](../develop.md), "What takes control back").
- **The SDK.** Python (`lampos`) and JavaScript (`lampos`), the same calls in both: events (`person.arrived`, `phrase.heard`, `touch`...), state (`get`), and actions (`light`, `say`, `gesture`, `express`, `sound`...). Each needs a grant the owner gave. See [the SDK map](../../sdk/README.md) for every call and its status.
- **A fake lamp** on your own computer, so you can try everything with no lamp.
- **Also there:** `lamp-mcp`, to let an agent drive the lamp ([sdk/mcp](../../sdk/mcp/README.md)), and skills you push to the lamp ([develop.md](../develop.md), part 5).

What your program never gets: camera frames or audio unless the owner grants `camera` or `microphone` and the lamp isn't private, face prints, names, or the words around a phrase it listens for.

**Harness and the SDK are two different doors.** The SDK talks to the lamp from your computer. Harness is a separate program, by the Harness team, that carries the lamp's spoken tasks to the AI helpers on your computer ([the getting-started guide](harness-getting-started.md)). You can use either or both.

## What works today

| Piece | State on lamp-voice |
|---|---|
| The SDKs and the fake lamp | Merged. Use them now |
| An example against the fake lamp | Runs (output below) |
| The lamp's side, `lamp-control` | Merged, but **not a product part yet**: it runs only when `LAMP_CONTROL=on`, and no product unit passes it. PR #4526 makes it one. **TO CHECK (SDK local leader)** |
| The app's *Add a developer computer* screen | The lamp's side is merged (`control.code`, `control.clients`, `control.revoke`). **TO CHECK (mobile, SDK local leader):** that the screen is in the app build customers get |

So until #4526 is on stable, a real lamp pairs only when its keeper turns lamp-control on. If that is you, ask your lamp's keeper, or use the fake lamp below.

## 1. Install the SDK (2 minutes)

You need Python 3.10 or later. The SDK is not on PyPI yet, so install from a checkout of this repository. On Debian or Ubuntu run `sudo apt install python3-venv` first.

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python
```

**TO CHECK (SDK local leader):** `sdk/README.md` says `pip install lampos` and `npm install lampos`, but the Python and JavaScript READMEs say neither is published yet. This page follows the READMEs.

## 2. Start a fake lamp (1 minute)

The fake lamp is a lamp on your computer with every grant, already paired with you. Give the trial its own `LAMPOS_HOME` so it never touches a real pairing.

```sh
# terminal 1
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
LAMPOS_HOME=/tmp/lampos-try python first_light.py
```

Terminal 2 prints:

```text
connected to lamp-fake with ['body', 'light', 'speak', 'look', 'hear', 'sound', 'skills', 'air', 'device', 'camera', 'microphone']
lamp state: {'lamp': {'talking': False, 'paused': False, 'private': False, 'asleep': False}}
```

Terminal 1 shows what the fake lamp was asked:

```text
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

1. In the Autonomous app, open your lamp and choose *Add a developer computer*. Tick what the computer may do. The app shows an 8-digit code, the lamp's pin (64 hex characters) and a 5-minute countdown. The lamp also says the first 8 characters of the pin aloud: they must match what the app shows.
2. On your computer, with no `LAMPOS_HOME` set:

```sh
lamp pair 12345678 --pin <the 64 characters the app shows>
lamp hello                 # connects and shows the grants
python first_light.py      # now it runs against your lamp
```

The SDK checks the lamp's certificate against the pin before it sends anything, so the code never reaches another machine. Pairing is kept in `~/.config/lampos/`. To remove a computer, revoke it in the app.

**TO CHECK (SDK local leader and mobile):** the app screen's exact name and steps are written from the lamp's side (docs/cloud.md, "The developer door") and docs/develop.md. The writer of this draft could not read the app's code. Until #4526 is on stable, a lamp needs `LAMP_CONTROL=on` first.

## Where next

- [develop.md](../develop.md): events, state, leases, `lamp-mcp`, skills.
- [The SDK map](../../sdk/README.md): every capability, its call in each SDK, its status, whether it was validated on a lamp.
- [control.md](../control.md): the protocol.
- JavaScript: the same calls in camelCase, `npm i /path/to/lampOS/sdk/js`, and `lampos fake` ([sdk/js](../../sdk/js/README.md)).

---

## Notes for reviewers (remove before publishing)

Sources, in `origin/lamp-voice` at 39fddfe02 (lampOS/ paths).

| Claim | Source |
|---|---|
| Python 3.10 or later; version 0.2.0 | sdk/python/pyproject.toml:6-10 |
| Not on PyPI or npm; install from checkout | sdk/python/README.md:21-28, sdk/js/README.md:22-28 |
| `lamp fake`, its `LAMPOS_HOME`, every grant | sdk/python/README.md:30-57 |
| Pair with `lamp pair CODE --pin PIN`; pin checked first; kept in `~/.config/lampos/` | sdk/python/README.md:59-72 |
| The app shows an 8-digit code, the pin, a 5-minute expiry | docs/control.md:297-300, docs/develop.md:259-261 |
| The lamp says the first 8 pin characters aloud | docs/cloud.md, "The spoken pin check" (`control.code`) |
| The owner can revoke at any time | docs/cloud.md `control.revoke`; docs/develop.md:276 |
| The person wins (hand, stop, privacy, conversation) | docs/develop.md, "What takes control back" |
| Never gets camera or audio unless granted and not private; never names or face prints | crates/control/src/protocol.rs:118-135 (`Grant::shown`); docs/develop.md, "Never" |
| `lamp-control` runs only with `LAMP_CONTROL=on`; no product unit passes it | docs/control.md, "Running the door"; `ls deploy/product` has no lamp-control unit; PR #4526 (open) |
| lamp-cloud answers `control.code`, `control.clients`, `control.revoke` | docs/cloud.md:157 and :2145 (merged as commit ae813eee0) |
| Busy refusal and `retry_ms` | sdk/python/README.md, "A motion asked while another still plays" |
| Closing the connection gives the light back | sdk/python/README.md, "Closing gives your light back at once" |
| Harness is separate and carries the lamp's tasks | docs/tasks.md:9-15 |

### The example's run

Run on 5 Nov 2026 from a checkout of `origin/lamp-voice` at 39fddfe02, macOS, Python 3.13.7, a fresh venv with `pip install ./sdk/python`, and `LAMPOS_HOME` set to a temporary folder: `lamp fake` in one terminal, `python first_light.py` in another. Exit code 0, output as pasted above. **It ran against the fake lamp, not a real lamp**, because no lamp-control product unit exists yet. Not run: the JavaScript SDK, `lamp pair` against a real lamp, the app screen.

### TO CHECK

- **SDK local leader:** the install commands (PyPI vs checkout); that the example's `light` arguments (`"warm", 3, 5_000`) and the `nod` gesture are valid on a real lamp's grants; the pairing steps on a real lamp with `LAMP_CONTROL=on`.
- **Mobile:** the *Add a developer computer* screen is in a build customers get, with these words (ecm-sds-mobile #88 was not readable by the writer of this draft).
- **Harness team:** the one paragraph about Harness ("two doors").
