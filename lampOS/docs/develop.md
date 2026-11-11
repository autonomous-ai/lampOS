# Program your lamp in 10 minutes

A developer's quick-start, for decision 1, developers first. It covers:
- turning the lamp's door on, and pairing your computer;
- the Python and JavaScript SDKs: hello, events, say, light, and a gesture under a body lease;
- `lamp-mcp`, for an agent;
- writing a skill the lamp runs on its own, then checking, pushing, signing and sharing it.

The protocol itself is [control.md](control.md). The skill format is
[skill-format.md](skill-format.md).

**What runs today.** Customer lamps run the stable build, **11450**
(lamp-voice `135bc417d4`, promoted 10 Nov). The table below was brought to 11450
from the git history of that commit (what is in it and what is not); a row was
last tried on a lamp on 10824, skill push on 11091 and a head tap on 11372.
Everything below works there, except what this table marks. The full list, with what to do instead, is in
[sdk/README.md](../sdk/README.md#on-a-customer-lamp-today).

| step | on a customer lamp (stable 11450) |
|---|---|
| The fake lamp, the SDKs, `lamp-mcp`, `lamp skill check` | on your computer; the SDKs aren't on PyPI or npm yet, so install from a checkout |
| lamp-control on the lamp | on every lamp, off until the owner turns developer access on in the app |
| Pairing through the app | works; pass the lamp's address with `--host` (no lamp announces itself by mDNS yet) |
| Events, states, `light`, `say`, `sound`, `volume`, `express`, `gesture`, `move` | works. A motion asked while another plays replaces it (10824 and older refused it `busy` with `retry_ms`). A head tap or pet is a `touch` event. `wait=True` on `gesture`, `move` and `express` returns when the lamp takes the motion; the next build ends it at the body's `done` |
| `look` at `ahead`, an angle or a `hand`, under a body lease | works; on a resting lamp a look ends `finished` or `clamped` with where the head stopped (on 10824 it could end `cut_short`) |
| `look` at `face` | next build (one turn toward the face in view) |
| `look` at `name` | not in this release (the lamp has no sound direction) |
| `joints` | all five joints: `roll`, `shoulder` and `elbow` as the body's pose, only while the lamp stands (11048 and older: `yaw` and `pitch` only) |
| `play` | refused: "not routed yet: waits for client packages" |
| `camera.snapshot`, `camera.stream` | works, local network only (not the fast frame path) |
| `skill.push`, `lamp skill push/list/remove` | works from 11091 on (the product unit passes `--manage-socket`), onto the developer's own lamp, with a yes at the lamp; refused on 10824 and older: "skill push is not on this lamp". No route to the Store from the SDK yet |
| Signed skills (`keygen`, `pack --sign`, `install`) | not in this release |

Event times (`at`) are UTC, `…Z`, on every build.

## 1. Install the SDK, and try it with no lamp (2 minutes)

From a checkout of this repository's `lamp-voice` branch, where the SDKs
live (`git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git && cd lampOS`;
the default branch, `main`, has none). On Debian or Ubuntu, run
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
light: `{"to": "body", "move": "glad-lift", "light": true}`), and `say`
and `sound` (`hum`, the lamp's own short hum) with `{"to": "voice"}` and
their `done`. `look` at `ahead`, `hand` or an angle is taken, and `joints`
naming only `yaw` and `pitch`, as stable 10824 took them.
`play`, and `joints` naming `roll`, `shoulder` or `elbow`, are refused in
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
        print("connected to", lamp.info.get("id"), "with", lamp.grants, flush=True)

        async def arrived(event):
            try:
                await lamp.light("warm", 3, 10_000, pattern="breathe")
                await lamp.say("Hello there.")
            except LampError as refused:  # private, paused, asleep, quiet_hours, rate...
                print("not now:", refused, flush=True)

        async def heard(event):
            print("heard:", event["phrase"], flush=True)
            # Hold the body while it nods, so the lamp's own idle life stands back.
            async with lamp.lease("body", why="quick-start") as lease:
                await lamp.gesture("nod")
                print("lease:", lease.state, flush=True)  # held; "suspended" or "lost" if someone takes it back

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
(`flush=True` shows each line at once when the output is piped or logged: Python holds it back otherwise.)

- **`hello`** is `Lamp.connect()`. It signs the lamp's nonce with this
  computer's key, and `lamp.grants` says what the owner granted.
- **Events** are `await lamp.on(name, handler)`. Each needs its grant:
  `person.*` needs `look`, and `phrase.heard` needs `hear` and the phrases
  you listen for, 8 letters at least. You only ever get the phrase you
  registered, never what else was said. The full table is in the SDK's
  README.
- **Actions** return once the lamp has taken them. A refusal is a typed
  error, such as `rate` (with `retry_ms`), `no_grant`, `busy` or
  `quiet_hours`. Quiet hours are off by default; a lamp that sets them
  (`--quiet-hours`) refuses `say` and `sound` inside them, and hello's
  `limits.quiet_hours` says this lamp's window, or `"off"`. Privacy, a
  pause or sleep don't refuse a request: they lose your leases (with that
  `why`) and drop a `say` (its `done` says `dropped`). A `say` is at
  most 900 UTF-8 bytes, the voice's own cap, a gesture may be any of the body's clips, and a
  `look` any angle: the body clamps it to its reach.
- **The rates are off by default** (the person's decision, 30 Oct 02:1x): hello's `limits`
  says `"rates": "off"`, and only the refused actions' guard and the phrase
  changes (6 a minute) hold. `lamp-control --rates default` (or
  `LAMP_CONTROL_RATES=default`) turns the rates below back on. Either way
  the body bounds every move, whatever the rate.
- **A rate is a ceiling, not a target, and so is no rate.** With the rates
  on, a computer may ask for a motion each second and an `express` 20 times
  a minute, but a lamp switching
  feelings every 3 s reads as twitchy, a machine, not a living thing (rule
  8). Ask for a feeling when something happened, and let the move finish:
  a feeling's move lasts 2.5 to 7 s, and a new motion asked while one
  plays cuts it short (smoothly, from where the body is), unless the
  person is using the lamp.
- **A body lease** (`lamp.lease("body")`) is needed for `look`, `move`,
  `play` and `joints`. For a gesture it keeps the lamp's own idle life
  back. It is renewed for you every 15 s, held as long as it is renewed
  (one take or renewal asks for up to 10 minutes), and let go when the
  block ends.

**Reading state:** `await lamp.get("presence", "lamp")` gives the states asked,
by name: `{"presence": {"count": 1, "near": 0.31, "facing": true}, "lamp": {...}}`.
That's the lamp's reply's `state` (since SDK 0.2.0). A `light` or `mood` the
lamp hasn't heard yet is `None`.

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

### Everything you can read

Events come by edge, when something changes, never once per reading. The SDK
READMEs' "Everything you can read" gives each one's fields and an example.

- **`look` grant:**
  - `person.arrived`: a face while the lamp had nobody (at start, or after `person.left`), with `count` and `near`.
  - `person.left`: nobody for 5 s.
  - `person.facing`: someone turns to the lamp, or away.
  - `person.stood_up`, with `side`.
  - `hand`: a hand seen, moved, opened, closed or gone, at most 4 a second.
- **`hear` grant:** `phrase.heard` (only your registered phrase, never the words around it) and `name.heard`.
- **`sound` grant:** `sound.heard`: `music`, `laughter`, `door` or `footsteps` past 0.5, never speech.
- **No grant:**
  - `touch` (`tap`, `pet`) and `button` (`long_press`).
  - `lamp.changed` (`talking`, `paused`, `private`, `asleep`).
  - `time` (accepted, not produced yet).
- **Without subscribing:** `lease` (your lease held, suspended or lost, and why) and `done` (an action that ends later, today `say`).
- **`get`:**
  - `presence` (`count`, `near`, `facing`; needs `look`).
  - `light` (the person's own, and `shows`, what the ring draws now).
  - `mood` (`energy`, `warmth`, `awake`, `asleep`).
  - `lamp`.
  - `air` is planned.
- **Never:** transcripts, the words around a phrase, face prints or names. The camera's pictures only with the `camera` grant (`sdk/python/examples/camera.py`); the `microphone` grant is not routed yet, so no audio.

### The same in JavaScript

Node 20.11 or later. In your project, install the SDK as a copy, not a
link, so Node finds its `ws`:

```sh
npm i --install-links /path/to/lampOS/lampOS/sdk/js
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
`wss://<lamp>.local:7731/v1` ([control.md](control.md), "Running the
door"). It is on every lamp, and nothing listens until the owner turns
developer access on in the app, which starts it within moments. Turning it
off closes every computer's connection.

**Pair once.** In the app, the owner adds a developer computer and ticks
the grants. The lamp says the first 8 characters of its pin aloud, and the
app shows the same 8 for the owner to confirm they match; only then does it
show the 8-digit code, the lamp's full pin (64 hex) and a 5-minute expiry. If
the lamp couldn't say its pin, the app cancels the code.
No lamp announces itself by mDNS yet, so give `lamp pair` the lamp's
address, its name (`lamp-xxxx.local`) or its IP address. A step-by-step
walkthrough is in [sdk/README.md](../sdk/README.md#pair-a-computer-with-a-customer-lamp).

On a test lamp without the app, its keeper can make a code as root:
`lamp-control code --grants body,light,speak,look`. It prints the code and
the pin.

```sh
lamp pair 12345678 --pin 3f9a… --host lamp-xxxx.local   # the code, the pin, the lamp's address
lamp hello                         # connects and shows the grants
```

The pin is checked after the TLS and WebSocket handshakes and before any
protocol frame, so the code's proof never
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

The tools send every op of the protocol: `lamp_state`, `lamp_wait_for`,
`lamp_gesture`, `lamp_light`, `lamp_say`, `lamp_sound`, `lamp_look`,
`lamp_move`, `lamp_play`, `lamp_joints`, `lamp_express`, `lamp_follow_hand`,
`lamp_follow_stop`, `lamp_start_turn`, `lamp_end_turn`, `lamp_set_listening`
and `lamp_stop` (the README's table has each one's
parameters). Each is offered only with the grant it needs. A body tool takes
a short body lease of its own, or renews the one it holds, and keeps to what
hello's `limits` says. A refusal comes back as the tool's error,
in the lamp's words. `lamp_look` works on customer lamps (the product
lamp-body opens its lease socket, stable 10824 and later) and on `lamp fake`.

### Your coding agent knows the SDK: the agent skill

`sdk/agent-skill/lampos-sdk` is a skill for coding agents (Claude Code,
Codex, Cursor and others that read `SKILL.md`): when you ask your agent to
program the lamp, it reads the mental model (grants, leases, who wins,
limits, quiet hours), the calls side by side in Python, JavaScript and MCP,
ready patterns, the gotchas, and how to test with the fake lamp. A test in
the SDK keeps it naming every call, tool, event and error code.

```sh
# Claude Code, as a plugin (also adds lamp-mcp's tools when lamp-mcp is installed)
claude plugin marketplace add ./lampOS/sdk/agent-skill
claude plugin install lampos-sdk@lampos
# or as a plain skill, for you or for one project
cp -r lampOS/sdk/agent-skill/lampos-sdk ~/.claude/skills/        # or .claude/skills/ in a repo
```

For another agent, point it at `lampos-sdk/SKILL.md`, or copy the folder
where it keeps skills.

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

**Push works on the developer's own lamp from stable 11091 on** (the product
lamp-control is started with `--manage-socket`); on 10824 and older every
push, list and remove is refused, "skill push is not on this lamp". There is
no route to the Store from the SDK yet (`lamp skill submit` waits for it). Signing waits on blue's
`blue/sdk-skill-sign` in the SDK. Push, list and remove run against the fake
lamp (`lamp fake`); the signing steps only against that branch's. On a real lamp, run the same behaviour as a program on your
computer while it is connected (step 2).

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
