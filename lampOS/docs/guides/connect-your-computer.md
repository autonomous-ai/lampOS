# Connect your computer to your lamp

For a developer whose lamp's owner has turned on developer access in the
app. It takes about five minutes and ends with your program talking to the
lamp. It is short on purpose: the whole protocol is
[control.md](../control.md), every SDK call is in
[sdk/GUIDE.md](../../sdk/GUIDE.md), and the ten-minute quick-start (the fake
lamp, a first program, lamp-mcp, skills) is [develop.md](../develop.md).

You need:

- a lamp, and its owner with the app for steps 1 and 2 (the owner decides
  what your computer may do, and can take it back at any time);
- a computer on the same network as the lamp, with Python 3.10 or newer.

## 1. The owner turns developer access on

In the app, the owner opens Lamp's settings, then the row **Developer
computers**, and turns on the switch **Developer access**. The lamp starts
its door within moments. Until then nothing listens on port 7731, so a
computer that tries gets a refused connection (see "If it fails").

## 2. Get a code from the app

On that page the owner taps **Add a developer computer** and ticks what it
may do (the grants). The app then shows, under **Pairing code**:

- **the code**: 8 digits, with a countdown **Expires in m:ss** (5 minutes);
  a new code voids the old one;
- the command to copy, `lamp pair` with the code: `lamp pair 12345678`.

Copy it as it is. The computer finds the lamp on its network by itself: no
pin is needed, and an address only when several lamps answer (step 4).

When the code runs out the page says "The code expired. Make a new one to pair a
computer." with a **New code** button.

## 3. Install the SDK

The SDKs are not on PyPI or npm yet: `pip install lampos` finds nothing.
Install from this repository's `lamp-voice` branch (the default branch,
`main`, has none). Without a clone:

```sh
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install "git+https://github.com/autonomous-ai/lampOS@lamp-voice#subdirectory=lampOS/sdk/python"
```

or from a checkout:

```sh
git clone --branch lamp-voice https://github.com/autonomous-ai/lampOS.git && cd lampOS
python3 -m venv ~/lampos-venv && . ~/lampos-venv/bin/activate
pip install ./lampOS/sdk/python
```

On Debian or Ubuntu, run `sudo apt install python3-venv` first. The
JavaScript SDK is in [sdk/js](../../sdk/js/README.md).

Check it installed: `lamp --help` lists `pair`, `find`, `fake`, `hello` and
`skill` (a newer build adds more).

## 4. Pair

Within the code's 5 minutes, run the command the app shows:

```sh
lamp pair CODE
```

`CODE` is the 8 digits. `lamp pair` looks for the lamp by mDNS on this
computer's network and keeps only lamps that have developer access on. With
one, it pairs. With several, it names them and stops:
`lamp: 2 lamps answered: lamp-one (lamp-one.local), lamp-two (lamp-two.local); say which with --host`.
Then add `--host` with the lamp's name or IP address:

```sh
lamp pair CODE --host lamp-xxxx.local
```

The pairing is kept in `~/.config/lampos/`, with the lamp's name, so later
connections need no code.

The computer trusts the certificate the lamp shows at pairing, as ssh does a
new host, and keeps its pin. Every later connection checks the lamp shows the
same certificate, after the TLS and WebSocket handshakes and before any
protocol frame. If you have the lamp's pin (a test lamp's keeper gets it from
`lamp-control code`), `--pin PIN` checks the lamp before the pairing too.

## 5. Say hello

```sh
lamp hello
```

connects to the paired lamp and shows what it may do. Against the SDK's fake
lamp (`lamp fake`, no real lamp needed) it prints one line in this shape;
yours names your lamp and lists the grants the owner ticked:

```
lamp-fake: grants body, light, speak, look, hear, sound, skills, air, device, camera, microphone, speaker, notes, speech
```

A lamp on a build that says its own build adds `build <number>` after the colon
(the same text the app shows): quote it in a bug report. The fake lamp has none.

Then a first program (it needs the `speak` grant):

```python
import asyncio
from lampos import Lamp

async def main():
    async with await Lamp.connect() as lamp:
        said = asyncio.Event()
        await lamp.on("done", lambda event: said.set())  # the say's end
        await lamp.say("Hello from my computer.")
        await asyncio.wait_for(said.wait(), 30)  # closing earlier stops it

asyncio.run(main())
```

The lamp says the line in its own voice. More programs are in
[sdk/python/examples](../../sdk/python/examples/); `lamp-mcp` serves the lamp
to an agent ([develop.md](../develop.md), section 4).

## If it fails

What each message below means. They are the SDK's own words, most copied from
runs on 17 Nov 2026 (`lampos` 0.2.0), the ones about finding the lamp and a
changed key from the SDK that pairs without a pin; the address shows here as
`HOST`.

| You see | Why | What to do |
|---|---|---|
| `lamp: bad_request: the code is the 8 digits the app shows` | The code is not 8 digits. | Copy the 8 digits from the app. |
| `lamp: no lamp with developer access on answered on this network: …` | No lamp on this network answered with port 7731 open: developer access is off, the computer is on another network, or the network drops mDNS. | Ask the owner to turn developer access on; join the lamp's network; or add `--host lamp-xxxx.local` (or its IP address). |
| `lamp: 2 lamps answered: …; say which with --host` | More than one lamp with developer access on is on this network. | Add `--host` with one of the names it lists. |
| `lamp: wrong_lamp: the lamp at HOST shows another key than the one this computer paired with (…): …` | The lamp's certificate is not the one kept at pairing: the lamp was reset, or something else answers at its address. | If the lamp was reset, pair again (`lamp pair CODE` with a new code from the app). If not, don't connect: find out what answers there. |
| `lamp: wrong_lamp: no pin to check the lamp by: …` | `--pin` was given but is not the 64 hex characters. | Copy the whole pin, or leave `--pin` out. |
| `lamp: wrong_lamp: the certificate at HOST is not the lamp's: …` (the two fingerprints follow) | With `--pin`: something answered at that address, but not the lamp with that pin. | Check the address; check the pin is this lamp's. |
| `lamp: not_paired: no pairing here for that lamp: run `lamp pair` first` | `lamp hello` (or a program) ran before a pairing was kept on this computer. | Pair first. |
| A Python traceback ending in `ConnectionRefusedError: [Errno 111] Connect call failed ('HOST', PORT)` | Nothing listens at that address and port: the owner has not turned developer access on, or the address is wrong. | Ask the owner to turn developer access on; check the address. |
| A traceback ending in `OSError: [Errno 65] No route to host`, on a Mac | macOS's Local Network privacy setting can stop Python from reaching the lamp: `ping` and `nc -vz HOST 7731` from Terminal work, yet Python never appears in the setting, so you are never asked (seen with python.org Python 3.12). | Open System Settings, Privacy & Security, **Local Network**, and turn on Python if it is listed. If it is not, relay through `nc`, which Terminal may use: see "On a Mac when Python is not listed" below. |
| A traceback ending in `socket.gaierror: [Errno -2] Name or service not known` | The computer cannot resolve the lamp's name. | Use the lamp's IP address, and check the computer is on the lamp's network. |

### On a Mac when Python is not listed

Allowing your terminal does not carry over to Python started from it. Relay
through `nc`, which Terminal may use. Run this in Terminal and keep it open;
it takes one connection at a time (`HOST` is the lamp's name or IP address):

```sh
LAMP=HOST
F=/tmp/lamp-relay.fifo; rm -f "$F"; mkfifo "$F"
while true; do nc -l 127.0.0.1 17731 < "$F" | nc "$LAMP" 7731 > "$F"; done
```

Then pair and connect through it, from another Terminal window:

```sh
lamp pair CODE --host 127.0.0.1 --port 17731
lamp hello
```

The relay passes TLS through untouched, so the certificate check still holds end
to end. Pairing, `lamp hello`, `say` and full sessions worked through it on the
SDK builder's Mac. A program that reconnects right after closing can be
refused for about half a second while the loop restarts: retry once.

The lamp's own refusals come back in the same shape. From
[sdk/GUIDE.md](../../sdk/GUIDE.md), "Connect, pair, limits":

- `unpaired` "no pairing code is out: ask for one in the app" (the code ran
  out, was voided by a newer one, or the owner cancelled it): ask for a new
  code;
- `unpaired` "the proof does not hold for the code out" (a wrong code, or a
  second try on the same connection): ask for a new code and pair once;
- `rate` "one pairing try a second": wait a second;
- `unpaired` "this lamp keeps as many computers as it can": the owner removes
  one in the app (a lamp keeps at most 16);
- `unpaired` "this computer's pairing was revoked": the owner removed this
  computer; pair again with a new code.

The owner can take the body back or revoke your computer from the app at any
time; a revoke closes its connections at once.
