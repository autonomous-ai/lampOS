# lampos: program a lampOS lamp from your computer

The Python client for the lamp's v1 control protocol
([docs/control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md),
messages in
[control.schema.json](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.schema.json)).
It runs on your computer, never on a lamp.

**New here?** [docs/develop.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/develop.md) is the 10-minute quick-start: the fake lamp, a first program, pairing, lamp-mcp and skills.

**On a customer lamp today (stable 10824):** lamp-control is on every lamp,
and answers once the owner turns on developer access in the app. Five gaps,
and what to do ([more](../README.md#on-a-customer-lamp-today)):
- **Skill push** is not yet on customer lamps: every skill op is refused, "skill push is not on this lamp". Try it on `lamp fake`, or run the behaviour as a program on your computer. `lamp skill check` works anywhere.
- **Joints and play:** `joints` takes `yaw` and `pitch` only; `roll`, `shoulder` and `elbow` are refused, and so is `play`. Use `look`, `gesture` or `move`.
- **Limp and pose:** the body's limp and hold-a-pose can't be reached from a computer.
- **No mDNS:** `lamp find` finds nothing. Pair with `--host lamp-xxxx.local` (the lamp's host name) or its IP address; the address is kept for later connects.
- **Event times are UTC:** `at` ends in `Z`. Convert to local time yourself:
  `datetime.fromisoformat(event["at"].replace("Z", "+00:00")).astimezone()`. Key a "today" by your own
  clock (`datetime.date.today()`), not by `at`'s date: UTC's day turns over at 08:00 in Shenzhen.

Also: a motion asked while the last one sent through lamp-control plays is refused `busy` with `retry_ms` (wait, ask again), and a `look` on a resting lamp may end `cut_short` with no hand on it.

## Changes

- **0.2.0:** `await lamp.get(...)` returns the states asked, by name
  (`{"presence": {...}, "light": {...}}`), as lamp-control's reply carries them
  under `state`; 0.1.0 returned the whole reply. A `light` or `mood` the lamp
  hasn't heard yet is `None`, on the fake lamp too.

## Install

It isn't on PyPI yet. From a checkout of this repository's `lamp-voice`
branch (the default branch, `main`, has no SDK):

```sh
git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git && cd lampOS
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python             # or -e, to work on it; '.[find]' to find lamps by mDNS
```

Without a clone: `pip install "git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/python"`.
`pip install lampos` from PyPI fails: install from the checkout or that line.
`[find]` finds no lamp yet: none announces itself on stable 10824.

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
example runs this way: `tour.py` (every kind of call, one step at a time; with `lamp fake --everything` nothing is refused), `greet.py`, `follow.py`, `follow_hand.py` (with `lamp fake --hands`),
`heat_watch.py` (a pulsing orange light once when a joint runs hot: `body.state`'s `temp_c`), `notes.py` (a run counter kept in a note on the lamp), `phrase.py`, `pomodoro.py`, `ci_light.py`, `ask_the_person.py`, `camera.py` (needs a pairing with the `camera` grant, which the app does not offer yet: only a pairing code made on the lamp itself carries it, so on a lamp paired through the app the call is refused `no_grant`; saves `snapshot.jpg` and a stream's `last-frame.jpg`), `curious.py` (a move of your own from poses), `touch.py` (a tap lights the ring, a pet is answered with a feeling; a tap reaches a program from the next build), and `push_skill.py ../examples/hello-skill` (a pairing with `skills` and `speak`).

It answers as a lamp does today: `gesture` and `move` with `{"to": "body"}`,
`express` as the feelings skill's move and light, `say` and `sound`
(`hum`, the lamp's own short hum) with `{"to": "voice"}` and their `done`,
`start_turn` with `{"to": "voice"}` and its `done`, `end_turn` and
`set_listening` as lamp-control does (`FakeLamp.hand_on_head()` lifts a
mute), `look` at `ahead`, `hand` or a yaw and pitch, and `joints` naming
only `yaw` and `pitch`, as a customer lamp (stable 10824) takes them; and
`play`, and `joints` naming `roll`, `shoulder` or `elbow`, refused
(`refused`, "not routed yet: waits for …"), in lamp-control's own words.
The fake takes what a customer lamp refuses: a skill push, and a motion asked while another plays
(stable 10824 answers `busy`). `--everything` answers every op instead.

## Pair with a real lamp

```sh
lamp pair 12345678 --pin 3f9a… --host lamp-xxxx.local   # the code and the lamp's pin (64 hex), both shown in the app
```

No lamp announces itself by mDNS yet (stable 10824), so give `--host`: the
lamp's host name, `lamp-xxxx.local`, or its IP address. Without it, `lamp
pair` finds no lamp. The address is kept with the pairing, so
`Lamp.connect()` needs nothing more; if it changes, pass `host=`.

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
  `lamp.off(name, handler)`. A `phrase.heard` handler given `phrases=[...]`
  hears only those; one given none hears every phrase the program listens for:

  ```python
  await lamp.on("phrase.heard", start, phrases=["start studying"])  # start() hears only this
  await lamp.on("phrase.heard", stop, phrases=["all done here"])    # stop() hears only this
  await lamp.on("phrase.heard", log)                                # log() hears both
  ```

  Each needs its grant:

  | event | fields | grant |
  |---|---|---|
  | `person.arrived`, `person.left` | `count`, `near` (0–1, share of the picture's height) | `look` |
  | `person.stood_up` | `side` | `look` |
  | `person.facing` | `facing` | `look` |
  | `hand` | `seen`; when seen, `x`, `y` (the palm's centre, 0–1 from the left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. By edge, at most 4 a second; `seen: false` once, 1 s after the last hand | `look` |
  | `phrase.heard` | `phrase`: one you registered (`on(..., phrases=[...])`), never what else was said | `hear` |
  | `name.heard` | `score`: how sure the listener was, 0–1 to a hundredth (real calls 0.90 to 1.00; a sound like the name has reached 0.94–0.98, so a program acting on its own may want 0.99) | `hear` |
  | `sound.heard` | `class`, `score` | `sound` |
  | `touch` | `kind` (`tap`, `pet`, `hold`), `where` | none |
  | `button` | `press` (`press`, `long_press`) | none |
  | `time` | your schedule | none |
  | `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep`, `lease`, and `quiet_hours` (planned: not produced yet; on a lamp that sets quiet hours, off by default, the door refuses `say` and `sound` in them, or while the lamp's local time isn't known, with the error `quiet_hours`) | none |
  | `volume.changed` | `level` (0–100), `muted`: the speaker's volume changed, whoever changed it (the person by voice or in the app, or a computer) | `speak` |
  | `body.state` | `joints` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`: each `deg`, `counts`, `goal_deg`, `load`, `moving`, `temp_c`) and `lamp` (`light`, `talking`, `private`), at most 25 a second (12.5 today) | `body` |

- **State:** `await lamp.get("presence", "lamp")` returns the states asked,
  by those names, as lamp-control answers them: `{"presence": {...},
  "lamp": {...}}`. Behaviour change: it used to return the whole reply.
  `light` is the person's own (`on`, `colour`, `level`) with `shows`, what the
  ring draws now; the fake's `shows` is the newest light asked while it lasts.
- **Actions:** `express`, `gesture`, `look`, `move`, `play`, `joints`,
  `light`, `say`, `sound`, `stop`, and the turn and mute calls
  `start_turn`, `end_turn`, `set_listening`. Each returns when the lamp has taken it;
  `wait=True` on a gesture, move or play returns its `done` when it has
  ended: `finished`, or `dropped` / `cut_short` with the body's `why` (the
  person using the lamp, replaced by a newer motion). A lamp whose hello
  `limits` don't say `motion_done` (lamp-control from before #4209) says
  nothing back for `gesture` and `move`, so there their wait returns at the
  answer, `{"to": "body"}`.
- **A new motion cuts the one playing short:** a gesture, move or feeling's
  move asked while another plays (whoever asked it) replaces it, eased out
  from where the body is under its own caps. While the person is using the
  lamp (a conversation, a hand on it), the new one is dropped by the body
  ("the person is using the lamp"); the door has already answered
  `{"to":"body"}`. **Stable 10824 and older** refuse it instead: `busy`,
  "busy: a motion plays for N ms more", with `retry_ms`. Wait `retry_ms`
  and ask again.
- **Closing gives your light back at once.** When a computer's last
  connection closes, the lamp gives back the light it asked for and stops
  each `say` of yours, waiting or playing, as `stop` does: its `done` comes
  `dropped`, "dropped: stopped by its asker". A script that lights the ring and exits shows it for a
  moment only: stay connected while it should show (`examples/ci_light.py`).
- **Leases:** `async with lamp.lease("body"):` holds the body for looks,
  moves and joints, renewed for you, until the block ends or the lamp takes
  it back (a hand on its head, a conversation): `lease.state`,
  `lease.lost_why`. When the connection closes the lamp ends the lease and
  says nothing: `lease.state` is `lost`, `lost_why` "the connection closed",
  at once; take a new lease on the new connection.
- **Following a hand:** `async with lamp.follow_hand() as following:` has
  the body follow the hand it sees, under a body lease of its own, waiting
  while the hand is gone, until the block ends, `lamp.stop()`, another
  `lamp.look(...)`, or the lamp takes the body back. lamp-control answers it
  `{"to": "body", "follow": "hand"}` with no `done`, so
  `await following.wait()` is told here: `how` and `why` (`stopped`,
  `stop`, `another look`, or the lease's why). Needs `body` and `look`;
  each start is a look. See
  `examples/follow_hand.py`.
  A lamp that takes the look without answering `follow: "hand"` (a
  lamp-control from before #4183) is refused, "this lamp doesn't follow a
  hand yet", and the lease let go.
- **Refusals** are typed: `lampos.errors.BY_CODE["rate"]` (with
  `retry_ms`), `["no_grant"]`, `["private"]`, ...
- **Synthetic hands:** `lampos.testing.hand_track(path, seconds=, hz=,
  seed=)` makes a track in `humans/hands/tracked`'s shape (`x`, `y`,
  `size`, `open`, `confidence`, `t` in µs; `wave`, `circle`, `across`,
  `still`), `hand_events(track)` the `hand` events lamp-control's watch.rs
  tells for it (by change, at most four a second, rounded to 2 places,
  `seen: false` 1 s after the last), and `FakeLamp.emit_hand_track(track,
  realtime=)` says them. `lamp fake --hands` waves a hand for 3 s after
  each arrival and answers a look at it.
- **In your tests:** `await fake.connect()` gives a `Lamp` already paired with
  the fake (its own temporary keyring, closed and removed with the fake), so a
  test of your program needs no code, pin or `LAMPOS_HOME`:

  ```python
  async with FakeLamp() as fake:
      lamp = await fake.connect()
      await lamp.say("hello")
      assert fake.asked("say") == [{"text": "hello"}]
  ```

  The fake pairs with `body`, `light` and `speak` only, as a real pairing the
  owner chose might; an app that listens (`hear`) or reads the air needs more:
  `FakeLamp(grants="all")` is every grant the protocol names, or name your own.

  `lampos.testing.FakeLamp(grants=...)` is the same fake
  lamp: `emit(event, **fields)`, `refuse_next(op, code, why)`,
  `done(re)`, `lease_event(lease, state, why)`, and `requests` /
  `received` for what it took. Its `light` keeps lamp-control's pattern levels
  (a `breathe` at level 1 is refused) and answers `{"to": "ring"}`.
  `pulses_dropped` lists a second pulse within a minute, which the ring shows
  steady, without its pulse (and says nothing). A `microphone_stream` on it
  sends a quiet 440 Hz tone, one block every 20 ms (`tone_block`);
  `lose_audio(n)` skips `n` blocks, said in the next one's `lost`, and
  `await set_private()` or `await set_in_view(2)` ends each stream with
  `microphone.ended`.
  A `move` it takes is one lamp-control takes: steps of 40 to 1,000 ms, 1.5 to
  10 s in all, from home and back, each joint within the body's reach and speed
  (`lampos.moves.move_refusal`, its words).

**When the lamp restarts** (every update), the connection closes and
`wait_closed()` returns. A long-running program runs under
`Lamp.keep_connected(setup)` instead: it connects, runs `await setup(lamp)`
(subscribe, take leases), waits for the connection to end, then connects
again and runs `setup` again, pausing 1, 2, 5, then 10 s while the lamp
can't be reached. It stops at once on what connecting again can't mend (no
pairing, another lamp at the address, a revoked computer, a refused
subscription such as `no_grant`) and on anything `setup` raises; cancel it
to stop.

```python
async def setup(lamp):
    await lamp.on("person.arrived", greet)

await Lamp.keep_connected(setup)
```

`FakeLamp.restart()` closes every connection as a lamp's restart does, to
test it.

**Dependencies** (`websockets`, `cryptography`; `zeroconf` to find lamps)
have lower bounds only, as a library's should: the application that uses
it pins them.

Tests: `PYTHONPATH=src python3 -m pytest tests` (needs `pytest`; with
`jsonschema`, the fake lamp checks every request against the schema).

**Seeing every frame:** with `LAMPOS_LOG=debug`, every frame to and from the
lamp is printed to stderr, one line each (`lampos -> {...}` sent, `lampos <-
{...}` received). It's off by default. Nothing that would pair or say hello as
this computer, or give away the lamp's pin, is printed: `proof`, `sig`, `key`,
`nonce`, `pin` and `fp` show as `<redacted>`, and so does a pair request's
`code`. A said `text` shows only as its
length and a tag keyed for this run (an HMAC: the same text tags the same within
a run, and nobody can test a guess against it), and a pushed skill part's
`data` only as its length. The fields the protocol has are printed as they
are: a registered phrase, events' fields and a refusal's `why`. A field the SDK
doesn't know, such as one from a newer lamp, is printed as `<not shown>`.

## Check a skill folder

A skill is a folder the lamp runs without code: its rules in `skill.toml`
([the skill format, version 1](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/skill-format.md)). Check one
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

## Push a skill to your own lamp

**Not yet on customer lamps** (stable 10824): there every skill op is
refused, "skill push is not on this lamp". Try it on `lamp fake`.

With the `skills` grant, which the owner ticks at pairing, a computer can put
a skill of its own on its own lamp, with no Store
([control.md](../../docs/control.md), "Skills pushed from a computer"):

```sh
# from lampOS/sdk/python
lamp skill push ../examples/hello-skill   # checks it, packs it, sends it in parts
lamp skill list
lamp skill remove hello-skill
```

`../examples/hello-skill` is a small skill to try it with: one short hello,
asking only `speak`.

What a push needs:
- **The grants:** `skills`, and every grant the skill's `[permissions]` asks
  (`hello-skill` asks `speak`).
- **The size:** a pushed skill is 1 MiB at most, packed, sent in parts of
  36 KiB. A skill with a recording or other large files is too big; so is a
  source template whose files must be rendered first.
- **The lamp:** lamp-control started with `--manage-socket`, its link to the
  root helper that installs skills ([control.md](../../docs/control.md),
  piece 7). Without it the lamp answers `refused`, "skill push is not on this
  lamp". The product unit on stable 10824 doesn't pass it; the next build's
  does.
- **jsonschema** for `push`'s first check of `skill.toml`'s shape, with
  `pip install './lampOS/sdk/python[check]'`. Without it the push goes on, says the shape
  wasn't checked here, and leaves it to the lamp.

`push` first runs `lamp skill check`, and refuses on this computer what the
lamp would refuse:
- a check error;
- a grant this pairing doesn't hold;
- a `core` or add-on skill;
- `hear` or `sound` topics, which wait for format 2's phrase and class lists;
- a raw topic, or a key outside the protocol's grants.

The lamp checks it all again. A skill that sees the picture is held off until
the owner says yes in the app, and the answer says so. In code:
`await lamp.push_skill(folder)`, `await lamp.skills()`,
`await lamp.remove_skill(name)`. A pushed skill keeps running after this
computer goes; revoking the computer, or taking back `skills`, turns its
skills off.

## Everything you can read

What the lamp tells a paired computer, read from lamp-control's `watch.rs`,
[control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md) and the schema. Every event comes **by
edge**: when something changes, never once per sensor reading. Each event
carries `at`, a UTC time ending `Z`, besides the fields below. Subscribe with
`on(event, …)`. An event your pairing lacks the grant for is refused at
subscribe.

### Events

| event | fields | example | fires when | grant |
|---|---|---|---|---|
| `person.arrived` | `count`: faces in view now. `near`: the nearest face's height, as a share of the picture (0–1, 2 places) | `{"ev":"person.arrived","count":1,"near":0.31}` | a face is seen while the lamp had nobody: at its start, or after `person.left` (nobody for 5 s). A face gone for less than that fires nothing | `look` |
| `person.left` | `count`: 0 | `{"ev":"person.left","count":0}` | nobody has been in view for 5 s, after someone was | `look` |
| `person.facing` | `facing`: true or false | `{"ev":"person.facing","facing":true}` | someone turns to the lamp, or the last one turns away | `look` |
| `person.stood_up` | `side`: `left`, `right` or `ahead`, when the room's watcher says | `{"ev":"person.stood_up","side":"left"}` | someone in view stands up | `look` |
| `phrase.heard` | `phrase`: one of the phrases you registered | `{"ev":"phrase.heard","phrase":"good night lamp"}` | a finished line the lamp heard holds your phrase as whole words, ignoring case and punctuation. Only the phrase leaves the lamp | `hear` |
| `name.heard` | `score` | `{"ev":"name.heard","score":0.97}` | someone says the lamp's name, or a sound like it | `hear` |
| `sound.heard` | `class`: `music`, `laughter`, `door` or `footsteps`. `score`: 0.5–1, 2 places | `{"ev":"sound.heard","class":"door","score":0.72}` | a class's score rises past 0.5 (again only after it falls under). Never speech | `sound` |
| `touch` | `kind`: `tap` (two or more taps are one tap) or `pet`. The schema also has `hold` and `where`, not sent yet | `{"ev":"touch","kind":"pet"}` | a tap or a pet on the lamp | none |
| `button` | `press`: `long_press`. The schema also has `press`, not sent yet | `{"ev":"button","press":"long_press"}` | the button is held | none |
| `lamp.changed` | `lamp_state`: `talking`, `paused`, `private`, `asleep` (each true or false). `quiet_hours` is planned, not sent yet | `{"ev":"lamp.changed","lamp_state":{"talking":true,"paused":false,"private":false,"asleep":false}}` | any of the four changes | none |
| `volume.changed` | `level`: 0–100 of what the lamp allows. `muted`: true at 0 | `{"ev":"volume.changed","level":30,"muted":false}` | the volume changes, whoever changed it: the person by voice or in the app, or a computer | `speak` |
| `time` | your schedule | | accepted in a subscription, not produced yet | none |
| `hand` | `seen`. When seen: `x`, `y` (the palm's centre, 0–1 from the picture's left and top), `size` (the palm's width, 0–1 of the picture's), `open`, `confidence`. All to 2 places, never the tracker's clock | `{"ev":"hand","seen":true,"x":0.42,"y":0.55,"size":0.12,"open":true,"confidence":0.91}` and `{"ev":"hand","seen":false}` | a hand is first seen, moves or grows by 0.05, opens or closes, at most 4 a second (the newest kept). `seen: false` once, 1 s after the last. Under 0.5 confidence is no hand | `look` |
| `body.state` | `joints`: each of `pitch`, `roll`, `yaw`, `shoulder`, `elbow` the body read, with `deg` (degrees from its home, as `get('body')` counts them, to a tenth; `null` until the body has said its limits), `counts` (0–4095), `goal_deg`, `load` (0–1), `moving` (5°/s or more) and `temp_c` (from the body's once-a-second report). `lamp`: `light`, `talking`, `private` | `{"ev":"body.state","joints":{"yaw":{"deg":-25.8,"counts":1722,"goal_deg":-22.9,"load":0.1,"moving":true,"temp_c":41}},"lamp":{"light":{"on":true,"colour":"warm","level":3},"talking":false,"private":false}}` | each `body/joint_states` the body sends, at most 25 a second: 12.5 today | `body` |
| `air.changed` | the air sensor's reading: `pm1_0`, `pm2_5`, `pm4_0`, `pm10` (µg/m³), `co2_ppm`, `humidity` (%), `temperature` (°C, at the sensor inside the base, so warmer than the room), each to a tenth (carbon dioxide whole) and left out while the sensor doesn't know it; `feel`: `fresh`, `fine`, `stuffy`, `very_stuffy` or `unknown`, from the carbon dioxide | `{"ev":"air.changed","pm2_5":7.4,"co2_ppm":912,"humidity":44.2,"temperature":28.3,"feel":"fine"}` | the first reading, then a value moves by its step (particles 5, carbon dioxide 50 ppm, humidity 3, temperature 0.5 °C), comes or goes, or `feel` changes; at most once every 5 s, the newest kept | `air` |

And two that come by themselves, with no subscription:

| event | fields | example | fires when |
|---|---|---|---|
| `lease` | `lease`, `state` (`held`, `suspended`, `lost`), `why` (`conversation`, `hand`, `stop_heard`, `private`, `paused`, `asleep`, `expired`, `taken_by_owner_app`, `revoked`, `released`, `slumped`) | `{"ev":"lease","lease":"l_3","state":"suspended","why":"conversation"}` | one of your leases changes: talking suspends it and resting holds it again; a hand on the body, a stop, a pause, privacy or sleep loses it. Hear a lease end with `lamp.on('lease', handler)` and `state` `lost` (and its `why`); there is no event named `lease.ended`, and `lease` comes without subscribing |
| `done` | `re` (your request's id), `how` (`finished`, `cut_short`, `dropped`), `why` | `{"ev":"done","re":14,"how":"finished"}` | an action that ends later has ended. Today that's `say`, `sound`, `start_turn`, a `look`, and a mute `set_listening(False)` the lamp lifted (`cut_short`, "a hand on the head") |

### States, with `get`

| state | fields | example | grant |
|---|---|---|---|
| `presence` | `count`, `near` (0–1), `facing`. Counts and how near, never who | `{"count":1,"near":0.31,"facing":true}` | `look` |
| `light` | the person's own light: `on`, `colour`, `level`. Also `shows`: what the ring draws now, `{on, colour, level}`. `null` until the lamp has said either | `{"on":true,"colour":"warm","level":3,"shows":{"on":true,"colour":"green","level":2}}` | none |
| `mood` | `energy`, `warmth`, `awake`, `asleep`, the lamp's inner state. `null` until known | `{"energy":0.6,"warmth":0.7,"awake":true,"asleep":false}` | none |
| `air` | the newest reading, as `air.changed` gives it, and `known: true`; `{"known": false}` with no sensor, its part not running, or no reading in 15 s | `{"pm2_5":7.4,"co2_ppm":912,"feel":"fine","known":true}` | `air` |
| `device` | the lamp's own computer: `cpu_load` (the last minute's, 1 is every processor busy), `memory_used` and `disk_free` (0–1), `uptime_s`, `temperature_c` (by sensor, °C), and `known: true`; `{"known": false}` with no reading in 15 s | `{"cpu_load":0.42,"memory_used":0.55,"disk_free":0.71,"uptime_s":3600,"temperature_c":{"cpu_thermal":61.5},"known":true}` | `device` |
| `lamp` | `talking`, `paused`, `private`, `asleep` | `{"talking":false,"paused":false,"private":false,"asleep":false}` | none |
| `volume` | `level` (0–100 of what the lamp allows), `muted`. `null` until lamp-actions has said it | `{"level":50,"muted":false}` | `speak` |
| `body` | `known`; `joints` (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`), each with `counts`, `deg` from its home, `min_deg`, `max_deg`, `min_counts`, `max_counts`, `home_counts`, `folded_counts`, `stand_counts`; `stands`, `speed_deg_s`, `acceleration_deg_s2`, `load_limit`, `heat_stop_c`, `counts_per_turn` | `{"known":true,"joints":{"yaw":{"counts":2015.0,"deg":0.0,"min_deg":-3.5,"max_deg":3.5,...},...},"stands":true,"speed_deg_s":4.4,...}` | body |

### The lamp's model

`lampos.model()` is the lamp's kinematic model, the package's copy of
[lamp.model.json](../../docs/lamp.model.json), written from the rigged CAD
(`robots/lamp/hardware/cad/glb/lamp.glb`) by the JS SDK's `tools/lamp-model.js`.
It needs no lamp. It gives:

- `chain`: the joints from the base out, under the `joints` op's names.
- Each joint's `bone` in lamp.glb, `parent`, `origin_m`, `rest_quat`, `axis`,
  `reach_deg` around the lamp's home, and its servo's number and bus name.
- `measured`: what is still the CAD's. The origins are, and each joint's
  `zero_deg` and `sign`, the joint's `deg` in `body.state` (from its home) at the CAD's rest
  pose, are too, until measured on a lamp. Shoulder and elbow have one measure so far: sign -1
  and lamp-75c9's stand angle as zero (from its numbers and the render; a photo of lamp-75c9 standing at rest agrees with the zero, the sign is still from the render);
  a lamp's own stand angle differs by a few degrees.

`lampos.hardware()` is the lamp's parts as data ([lamp.hardware.json](../../docs/lamp.hardware.json), [hardware.md](../../docs/hardware.md)): the board, servos, camera, microphones, speaker, light, touch, buttons, air sensor and power, each with where its facts come from.

With `body.state` it draws the lamp as it stands: turn each bone of lamp.glb
about its `axis` by `sign * (deg - zero_deg)` after its `rest_quat`.

### Never readable

Through this door, by any grant: transcripts or the words around a phrase,
face prints, and anyone's name. The camera's pictures come only with the
`camera` grant, on the lamp's own network and never while it is private
(`examples/camera.py`); the room's sound comes only
with the `microphone` grant, on the same terms (`microphone_stream`). With `look` alone, a hand's or a face's place in the picture is a
share of it, never the picture.

## Reference

Every public call and `lamp` command, with what the lamp takes. The ranges are
what lamp-control and the ring refuse past, read from
[control.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/control.md) and their code. The SDK checks names
against the schema before sending, and the lamp checks the rest. A test keeps
every public method named here.

**Today a lamp (stable 10824) refuses** `look` at `face` (the next build turns once toward the face in view) or `name` (the lamp has no sound direction) ("not in
this release"), `joints` but for `yaw` and `pitch`, and `play`, with
`refused`, "not routed yet: waits for …"; every skill op with `refused`,
"skill push is not on this lamp"; a gesture, move or feeling's move asked
while another plays with `busy` and `retry_ms`; and `say` and `sound` in its quiet
hours when it sets them (off by default; a lamp sets its own window, said in
`lamp.limits["quiet_hours"]`), or while its time isn't known, with `quiet_hours`.
`lampos.testing.LAMP_CONTROL_REFUSES` is what `lamp fake` refuses, as a
stable lamp does: `play` (`FakeLamp(refuses=LAMP_CONTROL_REFUSES)`). Every
fake refuses `joints` naming `roll`, `shoulder` or `elbow` with
`lampos.testing.JOINTS_WAIT`, lamp-control's words.

### Connecting

| call | parameters | returns |
|---|---|---|
| `await Lamp.pair(code, host, *, pin, port=7731, name=None, keyring=None)` | `code`: the owner's 8-digit pairing code. `host`: the lamp's address. `pin`: the certificate's SHA-256 the app shows, 64 hex. `port`: 7731. `name`: this computer's name in the app, 1–64 characters, default the host name. `keyring`: where the key and pairings are kept, default `Keyring()` (`LAMPOS_HOME`, else `~/.config/lampos`) | a `Pairing` (`lamp`, `client`, `grants`, …) |
| `await Lamp.keep_connected(setup, lamp=None, *, host=None, keyring=None, pauses=(1, 2, 5, 10))` | `setup(lamp)`: called (and awaited) on each new connection. `pauses`: seconds before connecting again, longer while the lamp can't be reached | never returns; raises `NotPaired`, `WrongLamp`, a hello's or subscription's refusal, or what `setup` raised. "When the lamp restarts" |
| `await Lamp.connect(lamp=None, *, host=None, keyring=None)` | `lamp`: a paired lamp's id, or the only one paired. `host`: an address to use instead of the kept one | a `Lamp`, after hello. Also `async with await Lamp.connect() as lamp:` |
| `lamp.info`, `lamp.grants`, `lamp.limits` | | the lamp's id and name, this computer's grants, the lamp's limits: each rate as `{most, within_ms}` windows (`joints`, `motion`, `light`, `say`, `express`, …), `hold_max_ms` (a light's longest `for_ms`, 30000), `lease_max_ms` (the longest one lease take or renewal asks for, 600000), `say_max_bytes` (the longest say's text in UTF-8 bytes, 900) and `quiet_hours` (`{from, to}`, or `off`, the default). There is no ceiling on renewing a lease, and a say is counted in bytes (older lamps also sent `take_max_ms` and `say_max_chars`). By default the rates are off: `rates: "off"`, and only the `refused` and `phrases` windows (a lamp run with `--rates default` gives every class). The fake lamp sends the same as a lamp with its rates off |
| `await lamp.close()` | | lets go of every lease, then closes |
| `await lamp.wait_closed()` | | returns when the connection ends, and raises a subscription's refusal nobody awaited |
| `await find(seconds=3.0)` (`lampos.discovery`) | `seconds`: how long to listen for mDNS | the lamps on this network (`id`, `name`, `host`, `port`, `fp`). Needs `pip install './lampOS/sdk/python[find]'`. Finds none today: no lamp announces itself (stable 10824) |

### Events and state

| call | parameters | grant |
|---|---|---|
| `lamp.on(event, handler, *, phrases=None)` | `event`: one of the schema's events (below), or `lease` or `done`. `handler(event)`: a function or coroutine; one that raises is logged with its traceback on the `lampos` logger (stderr by default; a program that configures `logging` can route or silence it) and the connection goes on. `phrases`: with `phrase.heard` only, up to 16 for the computer, each of 64 characters at most and 8 letters at least. Changing the phrases is 6 a minute | the event's. Returns a `Subscription`: `await` it to get the lamp's refusal |
| `lamp.off(event, handler)` | as `on` | unsubscribes once the event's last handler is gone |
| `await lamp.subscribe(events, phrases=None)`, `await lamp.unsubscribe(events)` | a list of events | each event's |
| `await lamp.get(*states)` | `presence` (needs `look`), `light`, `mood`, `lamp`, `air` (needs `air`), `device` (needs `device`), `body` (needs `body`) | the states asked, by those names |
| `await lamp.air()` | | the `air` grant: the air sensor's newest reading (`pm1_0`, `pm2_5`, `pm4_0`, `pm10`, `co2_ppm`, `humidity`, `temperature`, `feel`) and `known`; only `{"known": False}` with no reading. Changes come as `air.changed` |
| `await lamp.device()` | | the `device` grant: the lamp's own computer (`cpu_load`, `memory_used`, `disk_free`, `uptime_s`, `temperature_c`) and `known` |
| `await lamp.body_info()` | none; what the body may do and where it is: each joint (`yaw`, `pitch`, `roll`, `shoulder`, `elbow`) with `deg` from its home, `min_deg`, `max_deg`, its counts and poses; the speed, load and heat limits | `body` | no | `known: false` until the body has said its limits |

Every event and state, with its fields, an example and when it fires, is
"Everything you can read" above.

### Leases

| call | parameters | |
|---|---|---|
| `lamp.lease(*parts, for_ms=30_000, why=None)` | `parts`: `body` (grant `body`), `light` (`light`), `voice` (`speak`). `for_ms`: 1–600,000; renewed in time, a lease has no ceiling. `why`: up to 200 characters, shown to the owner | a `Lease`. `async with lamp.lease("body") as lease:` takes it, renews it at half its time, and lets go at the end |
| `await lease.take()`, `await lease.release()` | | `lease.id`, `lease.state` (`held`, `suspended`, `lost`, `released`), `lease.held`, `lease.lost_why` |

### Actions

Each returns the lamp's answer when the action is taken. `wait=True` returns
its `done` instead (`gesture` and `move` too, where hello's `limits` say
`motion_done`; on an older lamp they return the answer).

| call | parameters | grant | lease |
|---|---|---|---|
| `await lamp.express(feeling, strength=None, *, wait=False)` | `feeling`: `glad`, `lively` or `low`. `strength`: 0–1, default 0.5. `wait`: returns its move's `done` where the lamp says `motion_done` (the answer at once when there is no move). Under 0.5 a smaller move; under 0.0625 no move, only the light | `body` and `light` | no |
| `await lamp.gesture(name, *, wait=False)` | `name`: `nod`, `headshake`, `greeting`, `laugh`, `shy`, `stretching`, `acknowledge`, `curious`, `happy`, `caring`, `surprised`, `confused`, `sleepy`, `excited`, `dance_chill`, `dance_groove`, `dance_hype`, `dance_waltz`, `dance_jazz`, the clips offered to programs; the lamp takes `idle` and `scanning` too, and any other lowercase name the body answers (rule 8: keep `scanning` for when a person asks) | `body` | no |
| `await lamp.look(at=None, *, yaw=None, pitch=None, wait=False)` | `at`: `face` (needs `look` too), `hand` (needs `look` too: the body follows the hand in view until another look, a `stop` or the lease ends; answered `{"to":"body","follow":"hand"}`, no `done`; needs the body's lease socket on the lamp), `name` or `ahead`. Or `yaw` and `pitch` in degrees, right and up positive, any finite number: the body clamps them to its reach. On stable 10824, `face` and `name` are refused, "not in this release" (`name` stays so: the lamp has no sound direction; `face` on the next build is one turn toward the face in view, its `done` `finished` with where the head got to, or `dropped`, "no face in view"), and a look on a resting lamp may end `cut_short`, "not there in time: something holds the head", with no hand on it: treat one close to the target as arrived, or ask again. With `wait=True`, the body's `done` instead of the door's answer: `finished` with `yaw`, `pitch` and `clamped`, or `dropped` with why (`the body is not held` when a conversation took the body first). A wait on `look("hand")` is a `bad_request`: a hand-follow has no `done`. | `body` | `body` |
| `lamp.follow_hand(*, for_ms=10_000, why=None)` | `async with lamp.follow_hand() as following:`, or `await lamp.follow_hand().start()` then `await following.stop()`: a `look` at `hand` under a body lease of its own (`for_ms`, renewed), until the block ends, `stop()`, another look, or the lamp takes the body back. `await following.wait()` says how: `finished`/`stopped`, or `cut_short` with the why. `lamp.stop()` lets go of its lease too. After another look the lease stays held (that look runs under it), and the lamp's own life stands back until `stop()` or the block ends; a new `follow_hand()` lets go of it first. A lamp that takes the look without following (before #4183) is `refused`, and the lease let go. Where the hand is comes as `hand` events | `body`, `look` | its own |
| `await lamp.move(frames, step_ms, *, wait=False)` | `frames`: 2 frames at least (at most what 10 s allows: 251 at 40 ms) of 5 joints (pitch, roll, yaw, shoulder, elbow) in counts from home, the first and last all 0. `step_ms`: 40–1,000, and 1.5 to 10 s in all. Each joint within the body's reach and speed, else `bad_request` in the body's words | `body` | `body` |
| `await lamp.move_poses(poses, *, hold_ms=400, wait=False)` | A `move` through `poses`, each a dict of the joints it turns in degrees from the standing pose (`pitch` and `shoulder`/`elbow` up, `yaw` right, `roll`; one left out stays put), eased there and back at 80% of the body's speeds, each pose held `hold_ms`; at least 1.5 s, at most 10 s. `LampError` (`bad_request`) before sending for an unknown joint, one farther than a move takes it (pitch and roll 17.6°, yaw 13.2°, shoulder and elbow 30.8°) or poses longer than 10 s. `lampos.poses_to_frames(poses, step_ms=50, hold_ms=400)` gives the frames alone | `body` | `body` |
| `await lamp.play(name, *, wait=False)` | `name`: a move of your installed package. Refused on every lamp today, "not routed yet: waits for client packages" | `body` | `body` |
| `await lamp.joints(lease, within_ms, *, wait=False, **targets)` | `lease`: your held `body` lease. `within_ms`: 0 or more (the body's caps pace each step). `targets`: `pitch`, `roll`, `yaw`, `shoulder`, `elbow` in degrees, −180 to 180, at least one. The head's `yaw` and `pitch` go as a look; `roll`, `shoulder` and `elbow` as the body's pose, taken only while the lamp stands (else `done` `dropped`, with why). Naming both: one `done`, `finished` only when both are. On stable 10824 only `yaw` and `pitch` go; `roll`, `shoulder` and `elbow` are refused, "not routed yet: …". With the body silent for 15 s, name both yaw and pitch. With `wait=True`, the body's `done`, as `look`'s. | `body` | `body` (named) |
| `await lamp.light(colour, level, for_ms, pattern=None)` | `colour`: `yellow`, `red`, `green`, `blue`, `cyan`, `purple`, `orange`, `pink`, `white`, `warm`. `level`: 1–5. `for_ms`: 1–30,000. `pattern`: none (steady), `breathe` (level 2 or up), `candle` (3 or up) or `pulse` (2 or up). Under its level a pattern is `refused`. The ring plays one `pulse` a minute for a computer: a second within the minute shows its colour and level steady, without its pulse. A light never blinks. To hold one longer than 30 s use `lamp.hold_light` (next row); with a loop of your own, send it again before `for_ms` runs out (about every 25 s, `examples/pomodoro.py`) and wake that loop at once on a stop or a skip, or the program answers late; `stop` ends it | `light` | no |
| `await lamp.hold_light(colour, level, pattern=None, on_error=None)` | The same light, kept on past one ask's 30 s: asked now and again at half the lamp's hold limit until the handle's `stop()`. A refused renewal ends it and goes to `on_error` (else a warning) | `light` | no |
| `await lamp.ring(pixels, *, for_ms, brightness=None, fade_ms=None)` | `pixels`: one colour per pixel, from pixel 0 around the ring, each `"#rrggbb"` or `(r, g, b)` 0–255, exactly `lamp.ring_pixels` of them (hello's `limits.ring.pixels`, 32 on today's ring). `brightness`: 0–100, 100 by default, under the ring's own ceiling (lower at night). `for_ms`: up to 30,000. `fade_ms`: 1,000–60,000 | `light` | no | as `light` |
| `await lamp.light_rgb(rgb, *, for_ms, brightness=None, fade_ms=None)` | one colour of your own for the whole ring, `"#rrggbb"` or `(r, g, b)` | `light` | no | as `light` |
| `await lamp.light_off(*, for_ms=30_000)` | put the light out for `for_ms`: a ring of black pixels (there is no level 0 for `light`), so the lamp's own light is back when it lapses or at `stop()`; to keep it out longer (a bedtime wants minutes) ask again before it lapses, about every 25 s as the `light` row says (`examples/pomodoro` does the same for a colour), and wake that loop at once on a stop or a wake-up word, or the lamp answers late. A turn's face, the privacy and mute marks, the camera mark and the person's own light stay on top. A lamp with no pixel ring: `bad_request` | `light` | no | as `ring` |
| `lamp.ring_pixels` | how many pixels the ring has, from hello; `None` from a lamp before pixel lights | | | |
| `await lamp.say(text, *, rephrase=False, wait=False)` | `wait`: returns its `done` once said (`finished`), or `dropped`/`cut_short` with why. `text`: words, at most 900 UTF-8 bytes (about 900 Latin letters, 300 in a three-byte script), the voice's own cap, checked here before sending as the lamp checks it, never stricter; spoken **exactly as written** through the lamp's speech service (no model, no tools) once the person isn't talking. `rephrase=True`: the lamp's model says it in its own words instead, as before. Since #4958 `say` is exact by default; before it, every say was the model's version, which `rephrase=True` keeps | `speak` | no | 6 a minute, 60 an hour with the rates on; refused in quiet hours where the lamp sets them |
| `await lamp.camera_snapshot()` | none; returns the camera's newest picture, JPEG bytes about 320 across. Refused while the lamp is private, or with the camera closed. On the fake, `fake.picture = jpeg_bytes` sets the picture it carries; the default is the smallest whole JPEG, which no image library opens. | `camera` | no |
| `await lamp.camera_stream(on=True, fps=None, size=None)` | `fps`: 1 to 30 a second at most (2); `size`: `640x480` (the default) or `1280x720`. Where the eyes serve every frame (the answer's `fast: true`; not on customer lamps, stable 10824), each `camera.frame` comes as the camera made it, with the JPEG as `data` (bytes); otherwise the eyes' newest frame, about 1 to 2 a second, with `jpeg` in base64. `fps_now` is the rate lately; each new frame once, until `camera_stream(False)`, `stop` or a `camera.ended` event. Ends with `camera.ended` when the lamp turns private, or after 10 s with the camera closed | `camera` | no |
| `await lamp.microphone_stream(on=True)` | none but `on`. The room's sound as `microphone.audio` events, one each 20 ms block as the lamp's ears hear it: mono, 16 kHz, signed 16-bit little-endian samples as `data` (bytes, 640 a block), with `sequence`, `lost` (blocks lost since the last one sent: on the way, or because this computer read too slowly), `sample_rate`, `channels`, `format` (`s16le`) and `bytes`. The answer says the same shape and `source` (`mic1`). On again replaces it; until `microphone_stream(False)`, `stop` or a `microphone.ended` event: at once when the lamp turns private, at once when more than one person is in view ("someone else is here": the lamp can't tell its owner from anyone else), within 2 s when the lamp's eyes stop saying who is in view ("the room isn't seen"), within 1 s when the ring doesn't say it drew its mark ("the ring is not showing the microphone's mark"), or within 2 s when no sound comes (a lamp that doesn't share its microphone, `LAMP_BUS_AUDIO` off). A re-pairing without `microphone` ends it. Refused while private, with more than one person in view or the room unseen, from beyond the lamp's own network, where the ring can't show its mark, and unless the owner switched it on (`LAMP_CONTROL_MICROPHONE=on`, off by default) | `microphone` | no |
| `await lamp.sound(name)` | `name`: `hum`, the lamp's own short hum, or one of your clips (`upload_sound`), a clip plays once the person isn't talking, as an exact `say` does; the hum is rarer: its `done` says `dropped` within 15 s of the lamp's last turn ("the lamp spoke a moment ago"), or with another sound in the last 30 s | `speak` | no |
| `await lamp.upload_sound(name, wav)` | `name`: a lowercase letter, then lowercase letters, digits, `_` or `-`, up to 64, never `hum`. `wav`: bytes or a path, a WAV of 16-bit mono PCM at 24 kHz, up to 30 s, no sample past −12 dBFS and no 10 ms past −30 dBFS RMS (the lamp's speaker cap: a steady tone peaks near −27 dBFS at most, so a clip at −14 dBFS is refused). Sent in 36 KiB parts and checked whole on the lamp, over a clip of the same name. Returns `{name, bytes, ms}`. Each computer keeps up to 64 clips and 20 MB (`limits["sounds"]`); revoking it deletes them | `speak` | no |
| `await lamp.sounds()` | your clips on the lamp, each `{name, bytes, ms}` | `speak` | no |
| `await lamp.remove_sound(name)` | deletes one of your clips | `speak` | no |
| `lamp.notes`, `await lamp.notes.set(key, value)` | `lamp.notes` is the namespace of the four calls below, each also `await lamp.notes_set(key, value)`, `await lamp.notes_get(key)`, `await lamp.notes_delete(key)`, `await lamp.notes_list()`. `set` keeps `value` (any JSON, at most 16384 bytes serialized) under `key` (1 to 64 of letters, digits and `. _ : -`), over one of the same key: `{key, bytes}`. A computer keeps 256 keys and 256 KiB of keys and values at most (`lamp.limits["notes"]`); one past either is refused (`refused`, "full"). Your notes outlive your program and the lamp's restart, are only yours, and go when the owner lets you go | `notes` | no |
| `await lamp.notes.get(key, default=None)` | the value under `key`, or `default` when none is kept (a stored `None` comes back as `None`). | `notes` | no |
| `await lamp.notes.delete(key)` | drops `key`; whether it was kept. | `notes` | no |
| `await lamp.notes.list(prefix="")` | your keys that start with `prefix`, sorted: `keys` (each `{key, bytes}`), `count`, `total`, `bytes`, `keys_max`, `bytes_max`. | `notes` | no |
| `await lamp.volume(*, to=None, step=None, mute=None)` | exactly one of `to`: 0–100 of what the lamp allows (0 is muted), `step`: `"up"` or `"down"` (the voice's own step, within the lamp's floor and ceiling), or `mute`: `True` or `False` (an unmute brings it back to where it was). Returns `{"level": ..., "muted": ...}`, the level it left. The person's own volume, the same the voice and the app set | `speak` | no |
| `await lamp.stop()` | | none | no |
| `await lamp.start_turn(prompt=None, *, wait=False)` | `prompt`: words said first, as `say`'s `text`. Opens a conversation turn as a tap on the head does: the person's next line is answered as spoken to the lamp. Answered `{"to": "voice"}`; its `done` is `finished` once the answer has sounded, else `dropped` with the voice's why (`dropped: nobody spoke`). Refused in quiet hours where the lamp sets them, as `say` | `speak` and `hear` | no |
| `await lamp.end_turn()` | Ends the turn this computer started, never another's: `{"turn": "ended", "started": <the turn.start's id>}`, and the start's `done` follows. With none, `busy` ("not this computer's turn") | `speak` | no |
| `await lamp.set_listening(on)` | `False` holds this computer's mute: `{"mute": "held"}` (again: the same, nothing new). `True` lifts it: `{"mute": "lifted"}`, or `{"mute": "nothing_held"}`. The person wins: a hand on the head lifts it, said as a `done` `cut_short`, "a hand on the head", naming the `listen.set` that held it; and it ends when this computer's last connection closes or it is revoked. A `stop` doesn't lift it. Nothing the lamp hears reaches a computer either way | `hear` | no |
| `await lamp.request(op, *, on_id=None, **fields)` | any op of the schema, checked against it first. `on_id(id)`: told the request's id before it is sent, to match a `done` that may come before the reply is read | the op's | |

**A look's or a `joints`' `done` on a resting lamp:** a resting head or base
can stop a little short of where it was sent, against its own friction or
weight. Stopped within 24 counts (about 2 degrees), the `done` is `finished`
with `yaw` and `pitch` where the joints stopped, and `clamped: True` when that
is more than half a degree from what was asked; stopped farther short, it is
`cut_short` after 5 s, saying how many counts short. Read the angles from the
`done`, not from what you asked.

A lamp has no rate limits, except two guards: refused actions, 20 a second (past it the connection closes), and changes of the phrases a connection listens for, 6 a minute. Hello's `limits` says `"rates": "off"` with those two as `refused` and `phrases`. `say`, `sound` and `turn.start` are still refused in quiet hours where this lamp sets them (off by default).

**Refusals** raise `lampos.errors.BY_CODE[code]`, a `LampError` with `code`,
`why` (the lamp's own words), for `rate` `retry_ms`, and `what_to_do`, the
line below for its code (`lampos.errors.WHAT_TO_DO`):

| code | what to do |
|---|---|
| `unpaired` | Pair again with a new code from the owner's app (`lamp pair CODE --pin PIN`): this computer isn't paired, was revoked, or its key no longer matches. |
| `no_hello` | Say hello first: `Lamp.connect()` does; with `request()` alone, send `hello` before anything else. |
| `no_grant` | Ask the owner to tick this grant for this computer in the app, then connect again: grants are read at hello. |
| `no_lease` | Hold a `body` lease first (Python `lamp.lease("body")`, JS `lamp.withLease(['body'], ...)`); `joints` names it. |
| `lease_lost` | The lamp took the body back; `why` says why (a conversation, a hand, private, a stop). Let it be, and take a new lease later. |
| `busy` | Someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first. |
| `refused` | The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again. |
| `quiet_hours` | It's the lamp's quiet hours (`limits.quiet_hours`, off unless this lamp sets them): `say`, `sound` and `turn.start` wait until they end. A light is still fine. |
| `private` | The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false. |
| `paused` | The person paused the lamp. Wait for `lamp.changed` with `paused` false. |
| `asleep` | The lamp is asleep. Wait for `lamp.changed` with `asleep` false. |
| `rate` | Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded. |
| `bad_request` | The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json). |
| `off` | Developer access is off on this lamp: the owner turns it on in the app. |

On stable 10824 and older, `busy` with `retry_ms` also means a motion still
plays (the last gesture, move or feeling's move lamp-control sent, from any computer; the lamp's own motions don't count): wait `retry_ms`
and send it again. Without `retry_ms`, it is a held part, as above.

### Skills

| call | parameters | grant |
|---|---|---|
| `await lamp.push_skill(folder)` | not yet on customer lamps (stable 10824: refused). A skill folder (`SKILL.md`, `skill.toml` with a `version`): checked as `lamp skill check` does, packed and sent in parts | `skills` |
| `await lamp.skills()` | | `skills` |
| `await lamp.remove_skill(name)` | one of this computer's own pushed skills | `skills` |

### The `lamp` command

| command | options |
|---|---|
| `lamp find` | none: lamps on this network by mDNS (needs `[find]`). Finds none today: no lamp announces itself (stable 10824) |
| `lamp pair CODE --pin PIN [--host HOST] [--port 7731] [--name NAME]` | `CODE`: the 8-digit code. `--pin`: the 64-hex pin the app shows (required). `--host`: the address, `lamp-xxxx.local` or an IP; give it, as no lamp announces itself by mDNS yet (stable 10824). `--port`: 7731. `--name`: this computer's name in the app |
| `lamp hello [LAMP_ID]` | a paired lamp's id, or the only one: connects and shows its grants |
| `lamp fake [--every 10] [--cycles N] [--everything] [--hands]` | `--every`: seconds between arrivals (10). `--cycles`: stop after N (default: until Ctrl-C). `--everything`: answer every op, without refusing what lamp-control doesn't route yet. `--hands`: a hand waves for 3 s after each arrival, and looks are answered, as on a lamp with the body's lease socket |
| `lamp skill push FOLDER [LAMP_ID]` | a skill folder: checked first, then put on the paired lamp (`skills` grant). Push, list and remove are not yet on customer lamps (stable 10824) |
| `lamp skill list [LAMP_ID]` | this computer's own pushed skills |
| `lamp skill remove NAME [LAMP_ID]` | takes one of them off the lamp |
| `lamp skill check FOLDER...` | each a skill folder (`SKILL.md` and `skill.toml`), checked against the skill format |

`LAMPOS_HOME` sets where the key and pairings are kept, for every command.
`LAMPOS_LOG=debug` prints every frame to and from the lamp on stderr, secrets
redacted ("Seeing every frame" above).
