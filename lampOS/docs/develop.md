# Program your lamp in 10 minutes

A developer's quick-start, for decision 1, developers first. It covers:
- turning the lamp's door on, and pairing your computer;
- the Python and JavaScript SDKs: hello, events, say, light, and a gesture under a body lease;
- `lamp-mcp`, for an agent;
- writing a skill the lamp runs on its own, then checking, pushing, signing and sharing it.

The protocol itself is [control.md](control.md). The skill format is
[skill-format.md](skill-format.md).

**What runs today, and what is pending.** Every command below is in merged
code, or marked **pending** with the PR or branch it waits on. Until those
land, the pending steps run against the SDKs' fake lamp only, never a real
lamp.

| step | state |
|---|---|
| The fake lamp, pairing, the SDKs, `lamp-mcp`, `lamp skill check` | merged |
| `express`: a feeling as the feelings skill's own move and light | merged: #4020 |
| `look` at `ahead` or an angle, under a body lease | pending: `ten/look` |
| lamp-control as a product part, off by default | pending: #3876 |
| The owner's "Add a developer computer" screen in the app | pending: a later piece of #3504 |
| `lamp skill check` reading format 2 as the lamp does | merged: #3979 |
| `skill.push` on the lamp: the door and the `skills` grant | merged: #3910 |
| Loading pushed skills, and checking signed ones | not in this release: #3879 and Eighth's piece B (`eighth/skills-dev-p3`) are parked |
| `lamp skill keygen`, `pack --sign`, `push`, `install`, `list`, `remove` | pending: blue's `blue/sdk-skill-sign`, not yet a PR |
| Packing and checking an author's signature on the lamp (`lamp-skills pack --author`) | merged: #3931, #3938 |

## 1. Install the SDK, and try it with no lamp (2 minutes)

From a checkout of this repository. On Debian or Ubuntu, run
`sudo apt install python3-venv` first: without it, Python 3.12's `venv`
fails.

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install './lampOS/sdk/python[check]'      # [check] for `lamp skill check`
```

`lamp fake` is a lamp on your own computer. It has every grant, pairs with
this computer at once, and every few seconds says that a person arrived,
the phrase "good night lamp", and that the person left. It prints every
request it gets. Give the trial its own `LAMPOS_HOME`, apart from your
real pairings:

```sh
# terminal 1
LAMPOS_HOME=/tmp/lampos-try lamp fake
```

It answers as a lamp does today. `gesture`, `move` and `express` are taken
(`{"to": "body"}`, and `express` says its move and that it showed the
light: `{"to": "body", "move": "glad-lift", "light": true}`). `play`, `sound`, `look` and `joints` are refused, in
lamp-control's own words ("not routed yet: waits for …").
`--everything` answers every op instead.

## 2. Your first program (3 minutes)

Save this as `first.py`. When someone arrives, it lights the lamp warm and
says hello. When it hears "good night lamp", it nods, holding the body
while it does:

```python
"""The quick-start's first program: say hello, light up and nod when a person arrives."""

import asyncio

from lampos import Lamp
from lampos.errors import LampError


async def main():
    async with await Lamp.connect() as lamp:
        print("connected to", lamp.info.get("id"), "with", lamp.grants)

        async def arrived(event):
            try:
                await lamp.light("warm", 3, 10_000, pattern="breathe")
                await lamp.say("Hello there.")
            except LampError as refused:  # private, paused, asleep, quiet_hours, rate...
                print("not now:", refused)

        async def heard(event):
            print("heard:", event["phrase"])
            # Hold the body while it nods, so the lamp's own idle life stands back.
            async with lamp.lease("body", why="quick-start") as lease:
                await lamp.gesture("nod")
                print("lease:", lease.state)  # held; "suspended" or "lost" if someone takes it back

        await lamp.on("person.arrived", arrived)
        await lamp.on("phrase.heard", heard, phrases=["good night lamp"])
        await lamp.wait_closed()


asyncio.run(main())
```

```sh
# terminal 2
LAMPOS_HOME=/tmp/lampos-try python first.py
```

Terminal 1 shows each request: `subscribe` (twice), `light`, `say`,
`lease.take`, `gesture` and `lease.release`. Terminal 2 shows
`connected to lamp-fake with ['body', 'light', 'speak', 'look', 'hear', 'sound', 'skills']`,
then `heard: good night lamp` and `lease: held`.

- **`hello`** is `Lamp.connect()`. It signs the lamp's nonce with this
  computer's key, and `lamp.grants` says what the owner granted.
- **Events** are `await lamp.on(name, handler)`. Each needs its grant:
  `person.*` needs `look`, and `phrase.heard` needs `hear` and the phrases
  you listen for, 8 letters at least. You only ever get the phrase you
  registered, never what else was said. The full table is in the SDK's
  README.
- **Actions** return once the lamp has taken them. A refusal is a typed
  error, such as `rate` (with `retry_ms`), `no_grant`, `private` or
  `quiet_hours`. `say` and `sound` are refused in the lamp's quiet hours,
  22:00 to 07:00 its local time unless the lamp sets its own; hello's
  `limits.quiet_hours` says this lamp's window, or `"off"`.
- **A body lease** (`lamp.lease("body")`) is needed for `look`, `move`,
  `play` and `joints`. For a gesture it keeps the lamp's own idle life
  back. It is renewed for you every 15 s, lasts at most 10 minutes a take,
  and is let go when the block ends.

### What takes control back

The person always wins (control.md, "Who wins"). Your lease follows the
lamp's lease events (`lease.state`, `lease.lost_why`):

| what happens at the lamp | your lease |
|---|---|
| someone talks to the lamp | `suspended` (`conversation`), then `held` again when the talk ends |
| a hand on its head, or someone says "stop" | `lost` (`hand`, `stop_heard`) |
| the privacy switch, a pause, or sleep | `lost` (`private`, `paused`, `asleep`); new takes are refused |
| the owner takes it in the app, or revokes this computer | `lost` (`taken_by_owner_app`, `revoked`) |
| the body gives way under load | `lost` (`slumped`) |

An act on a lost lease is refused with how it ended. `await lamp.stop()`
ends this computer's own actions at once, and needs no grant.

**When the lamp restarts** (every update), the connection closes and
`wait_closed()` returns. Connect again in a loop, with a few seconds'
pause.

### The same in JavaScript

Node 20.11 or later. In your project, install the SDK as a copy, not a
link, so Node finds its `ws`:

```sh
npm i --install-links /path/to/lampOS/sdk/js
```

```js
// first.mjs
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
console.log('connected to', lamp.info.id, 'with', lamp.grants);

await lamp.on('person.arrived', async () => {
  try {
    await lamp.light('warm', 3, 10_000, 'breathe');
    await lamp.say('Hello there.');
  } catch (refused) { // private, paused, asleep, quiet_hours, rate...
    console.log('not now:', refused.message);
  }
});

await lamp.on('phrase.heard', async (event) => {
  console.log('heard:', event.phrase);
  // Hold the body while it nods, so the lamp's own idle life stands back.
  await lamp.withLease(['body'], async (lease) => {
    await lamp.gesture('nod');
    console.log('lease:', lease.state); // held; "suspended" or "lost" if someone takes it back
  }, { why: 'quick-start' });
}, { phrases: ['good night lamp'] });

await lamp.closed();
```

```sh
LAMPOS_HOME=/tmp/lampos-try npx --no-install lampos fake    # terminal 1
LAMPOS_HOME=/tmp/lampos-try node first.mjs                  # terminal 2
```

The names are the Python SDK's, in camelCase, with promises
([sdk/js/README.md](../sdk/js/README.md)). The pairing folder is shared, so
a computer paired by one SDK is paired for both. Run the CLI as
`npx --no-install lampos`, never `npx lamp`, which is an unrelated npm
package.

## 3. Your lamp (2 minutes)

**Turn the door on.** `lamp-control` serves the protocol on
`wss://<lamp>.local:7731/v1`, only with `LAMP_CONTROL=on`
([control.md](control.md), "Running the door").
- **Pending (#3876):** it becomes a product part on every lamp, installed
  and off, with `LAMP_CONTROL=off`.
- **Later:** the owner turns developer access on in the app.
- **Until both:** a test lamp needs a /run drop-in setting
  `LAMP_CONTROL=on`, by whoever looks after that lamp.

**Pair once.** In the app, the owner chooses *Add a developer computer*
and ticks the grants. The app shows an 8-digit code, the lamp's pin (64
hex) and a 5-minute expiry.
- **Pending:** that app screen is a later piece.
- **Until then:** on a test lamp, its keeper runs this as root:
  `lamp-control code --grants body,light,speak,look`. It prints the code
  and the pin.

```sh
lamp pair 12345678 --pin 3f9a…     # the code and the pin
lamp hello                         # connects and shows the grants
```

The pin is checked before anything is sent, so the code's proof never
reaches another host. Then run `first.py` with no `LAMPOS_HOME`, against
the real lamp. The lamp gives a small nod when a computer pairs, and the
owner can revoke it at any time, which closes its connections at once.

## 4. An agent drives the lamp: lamp-mcp (1 minute)

`lamp-mcp` serves one paired lamp as MCP tools, on your computer, through
the SDK's pairing ([sdk/mcp/README.md](../sdk/mcp/README.md)):

```sh
pip install ./lampOS/sdk/python ./lampOS/sdk/mcp
claude mcp add lamp -- lamp-mcp        # or any MCP client; --lamp ID picks one of several
```

Install the two in one `pip` command: the SDK (`lampos`) isn't on PyPI, so
`sdk/mcp` installed alone looks for it there and fails.

The tools are `lamp_state`, `lamp_wait_for`, `lamp_gesture`, `lamp_light`,
`lamp_say`, `lamp_look`, `lamp_move`, `lamp_express` and `lamp_stop`. Each
is offered only with the grant it needs, takes its own short body lease,
and keeps to the lamp's rates. A refusal comes back as the tool's error,
in the lamp's words: until `look` lands (above), `lamp_look` is refused
"not routed yet: waits for the body's lease socket".

## 5. A skill the lamp runs on its own (2 minutes)

Code on your computer stops when your computer does. A **skill** is a
folder of rules that the lamp runs itself, with no code: `SKILL.md` and
`skill.toml`. Write it in **format 2**, where `look` means presence alone
(someone there, how near), never the camera's picture:

```
desk-hello/
  SKILL.md
  skill.toml
```

```markdown
---
name: desk-hello
description: A small nod and a warm light when someone sits down at the desk, at most once in ten minutes.
---

# Desk hello
```

```toml
format = 2
version = "0.1.0"

# Format 2: `look` is presence alone (someone there, how near), never the picture.
[permissions]
body = true
light = true
look = true

[trigger]
topic = "humans/faces/tracked"
when = "appears"
cooldown = "10m"

[ask]
policy = "nudge"

[[do]]
body = { gesture = "nod" }

[[do]]
light = { colour = "warm", level = 2, for = "30s" }
```

- **Permissions** are the skill's manifest. A skill that reads or does
  more than it declares isn't loaded. The owner sees them as plain words
  before it is installed.
- **The picture** is its own grant, `camera = true`, never given by
  default. A pushed skill that asks it lands off, until the owner says yes
  in the app.
- Triggers, conditions, steps and the light, gesture and sound values are
  in [skill-format.md](skill-format.md).

**Check it:**

```sh
lamp skill check desk-hello
```

It checks the front matter, the shape against the format's schema, the
moves and the sounds, and refuses any script. It says
`desk-hello: ok, format 2`, and reads `look = true` as the lamp does: in
format 2 that is presence (someone there, how near), never the picture.
`lamp-skills install` says the same: "It may use: light, body, look".

## 6. Push it, sign it, share it (not in this release)

Pushing a skill from a computer is parked for this release (#3879 and
Eighth's piece B on the lamp), and signing waits on blue's
`blue/sdk-skill-sign` in the SDK. Against that branch's fake lamp these
steps run as shown.

**Push** it to your own lamp. It needs the `skills` grant, and lands in
your computer's own folder there:

```sh
lamp skill push desk-hello         # desk-hello 0.1.0 pushed, on
lamp skill list                    # desk-hello  0.1.0  on
lamp skill remove desk-hello
```

A pushed skill keeps running after your computer goes. It is held to what
your pairing grants now, and turns off if the owner takes a grant back. It
is held to the live door's rates and to the quiet hours, whatever it
declares (control.md, "Skills pushed from a computer").

**Sign it to share it.** Make an author key once. It is apart from your
pairing key, and you keep it:

```sh
lamp skill keygen                           # author key dfd647200ea711de made: …/author.pem
lamp skill pack --sign desk-hello           # desk-hello-0.1.0.tar <sha256> signed
lamp skill check desk-hello-0.1.0.tar       # … signed by key dfd647200ea711de, not the Store
```

The package carries your public key inside it, and the signature covers
every file. Share the `.tar` as a file, or put it at an `https://` URL.

**Install a shared skill.** Anyone paired with their own lamp runs:

```sh
lamp skill install desk-hello-0.1.0.tar               # or an https:// URL, 1 MiB at most
# desk-hello 0.1.0
#   signed by key dfd647200ea711de, not the Store
#   asks to move the lamp (body)
#   asks to change the light (light)
#   asks to know when someone is at the lamp (look)
```

It asks before it installs (`--yes` skips the question). The lamp checks
the package again: the signature against the key it carries, then every
file against its manifest. It refuses a package that is unsigned, changed
after signing, names another key, or carries the Store's key. After that,
the skill's name stays with your key: an update must be signed by the same
key, or it is refused until the skill is removed.

**What a signature means:** the bytes are that key's, not that its owner is
anyone in particular. What the skill may do is still only what the owner
granted, and the picture still waits for the owner's yes.

## Where next

- [control.md](control.md): every op, its fields, the rates and errors.
- [sdk/python/README.md](../sdk/python/README.md), [sdk/js/README.md](../sdk/js/README.md):
  each SDK, its examples (`greet`, `follow`, `phrase`, `pomodoro`,
  `ci_light`) and its `FakeLamp` for your tests.
- [skill-format.md](skill-format.md) and [skills.md](skills.md): the skill
  format, and how the lamp loads, checks and runs skills.
