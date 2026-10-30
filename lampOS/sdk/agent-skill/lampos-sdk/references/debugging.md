# Debugging

## On your computer

- **Every frame:** `LAMPOS_LOG=debug python my_program.py` (or
  `LAMPOS_LOG=debug node my_program.js`) prints each frame to and from the
  lamp on stderr: `lampos -> {...}` sent, `lampos <- {...}` received.
  Secrets (`proof`, `sig`, `key`, `nonce`, `pin`, `fp`) show as
  `<redacted>`, said text as its length and a tag, and a field the SDK
  doesn't know as `<not shown>`. `lamp-mcp --verbose` does the same while
  stdout stays the MCP stream.
- **The pairing and grants:** `lamp hello`, or this skill's
  `scripts/check_pairing.py`, which also prints `limits` and what each
  refusal means.
- **Read the refusal:** `error.code` says what kind, `error.why` the lamp's
  own words (often the exact field or rule). See [errors.md](errors.md).
- **What the fake was asked:** in a test, `fake.asked(op)`; by hand, run
  your program with `LAMPOS_LOG=debug`. `lamp fake` prints `asked` lines
  only between its own events, so it can miss some.
- **Try against the fake first:** `lamp fake --everything` answers every op;
  if it works there and not on the lamp, it's the lamp's state (grants,
  private, quiet hours, a part that doesn't route the op yet).

## On the lamp (the journal recipe)

With ssh to the lamp (the owner's, `orangepi@lamp-xxxx.local`), its journal
says what the door and the parts did with each request:

```sh
# the door: every request, refusal and lease, with your client id (c_…)
journalctl -u lamp-control --since "-10 min" --no-pager
# follow it while your program runs
journalctl -u lamp-control -f
# the body: a move dropped or bounded, and why
journalctl -u lamp-body --since "-10 min" --no-pager | grep -i -E "drop|refus|bound"
# the voice: a say refused or cut short
journalctl -u lamp-voice --since "-10 min" --no-pager | grep -i -E "say|mute"
```

Match by time: each event's `at` is UTC, and `journalctl --utc` prints the
journal in UTC too. Your client id is in `lamp hello`'s answer and in the
door's lines.

## When nothing happens

1. Did the call answer OK? An OK `{"to": "body"}` means handed to the body,
   which may still drop it (out of reach); the body's journal says.
2. Is the lamp private, paused or asleep? `get("lamp")`.
3. Is it quiet hours (`say`, `sound`)? `lamp.limits.get("quiet_hours")`.
4. Did your program exit right after? Closing gives the light back at once.
5. Did you lose the lease? Listen for `lease` events.
6. Is the light yours? `get("light")["light"]["shows"]` is what the ring draws.
