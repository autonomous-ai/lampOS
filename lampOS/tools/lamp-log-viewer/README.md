# Lamp log viewer

A **developer tool**, not a product feature: a page on the developer's computer that shows one dev lamp's journal live, and what the lamp heard and said, with the time each step took. Nothing of it runs on a lamp, and nothing ships in a lamp image.

## What it is, and is not

- **Read only.** It runs `journalctl` and one read-only sqlite query on the lamp over ssh, and writes nothing there.
- **127.0.0.1 only.** The server binds the loopback address and answers only requests addressed to `127.0.0.1:<port>` or `localhost:<port>` (a page on another site cannot read it through your browser). It has no login: anyone who can use this computer's loopback can read the page.
- **Needs the lamp's ssh key** in your agent or `~/.ssh` (`ssh -o BatchMode=yes`, so it never asks for a password) and, for the Conversation tab, **`sudo -n` on the lamp** (the memory store is readable only by its own user).
- **The Conversation tab shows every person the lamp talked to.** It reads lamp-memory's store (`/var/lib/lampos-memory/memory-days.sqlite3`, opened `mode=ro`, active exchanges only, so a forgotten exchange never shows) and prints what each person said and what the lamp answered, for the room and for every known person. Use it on **dev lamps only**, never on a lamp in someone's home, and do not paste or post what it shows: what a person said stays off GitHub.

## Run

```
python3 -I lamp_log_viewer.py --lamp orangepi@lamp-XXXX.local
```

then open http://127.0.0.1:8790/. The lamp is always an argument; none is built in.

| Option | Meaning |
|---|---|
| `--lamp user@host` | The lamp to read (required). |
| `--port N` | The local port (default 8790). |
| `--cache-dir DIR` | Where the timings cache is kept (default `~/.cache/lamp-log-viewer/`). |

## Log tab

Type in the box to filter (`request`, `refused`, `heard`), click a service chip to hide it, "Hide body steps" drops lamp-body's position lines, "lamp-* only" off shows every system line. Warnings are amber, errors red. The page keeps the last 5,000 lines.

## Conversation tab and timings

Under each turn, its timings from lamp-voice's journal lines, matched to the turn on the request's "first piece to the voice" time (within 300 ms):

| chip | from the line |
|---|---|
| total heard→sound | `first answer audio N ms after the words were heard` |
| in: last word | `heard K words, the last N ms after the line ended` (speech-to-text) |
| LLM TTFT / LLM end | `chat answer: its first words or call N ms and its end M ms after it was asked` |
| gen tok/s | completion tokens ÷ (end − first): the streaming tail, rough for short answers |
| tokens | the same line's prompt / cached / completion tokens |
| to TTS | `first piece to the voice N ms after the words were heard` |
| TTS first byte | `ElevenLabs piece 1 … its first audio byte N ms after it went` |

A chat answer line is paired to its request by the gap: the answer line is written at the answer's end and the request's round line at its first event, so `answer_at − round_at` equals `end − first` (within 150 ms) for the request's own answer. A side call (a summary, a title) has another gap and is paired with no request.

The lamp's journal keeps under an hour (#7254), so the server copies the timings (numbers only, no words) to `<cache-dir>/<lamp>.metrics.json` every minute while it runs. Turns from before it ran show "no timings".

## Tests

```
python3 -m unittest test_lamp_log_viewer
```

from this folder: fixture journal lines with made-up timings and no words, no lamp needed. They hold the parser, the pairing of each chat answer to its request (a side call must not pair), the turn-to-request match, the lamp argument's shape, the cache location and the host check.
