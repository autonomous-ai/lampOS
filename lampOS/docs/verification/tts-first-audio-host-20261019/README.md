# Configured voice's first audio: host verification, 19 October 2026

Validation base: `d9e90ad58` (PR #1712's head, with #1896's chat warming)
plus this branch's local patch. Part of #1712 (the first builder at 19:57).

**Host only.** Run on an Apple Silicon Mac. No lamp was accessed, no key was
used and no speech was asked of ElevenLabs. The lamp-4ace timing (about
690 ms from the first piece to its first sound with `eleven_v4_turbo`) is the
first builder's to take, with the journal lines below.

## What changed

- Each piece asked of the configured voice is journaled at its first audio
  byte: `request R ElevenLabs piece K: C chars, sent S ms after the words were
  heard on a reused|new connection; its first audio byte A ms after it went`.
  At the turn's end: `request R ElevenLabs: B of N pieces began, U on a reused
  connection, C chars; the first piece's first audio byte A ms after it went,
  the slowest M ms`. No words.
- "Reused" is observed: the response's local socket (hyper-util's `HttpInfo`,
  which reqwest puts in the response's extensions) is compared with the last
  8 seen. Ceiling: a new connection given a recent local port again would read
  as reused.
- `LAMP_SECOND_SPEECH_WARM=on` (off by default, as `LAMP_SECOND_CHAT_WARM`):
  as a line begins, after 60 s with no piece, a `GET <base>/models` with the
  key opens the connection; no text, no characters billed.

## Local server (the tests)

`cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 --nocapture second_provider::net::tts`:
a keep-alive HTTP/1.1 server on 127.0.0.1 counting its connections.

| Case | Connections | First piece | First audio byte |
|------|-------------|-------------|------------------|
| warmed, then one piece | 1 (GET then POST on it) | reused | 0 ms |
| no warming, two pieces | 1 | new, then reused | 2 ms, 1 ms |
| warming hangs, one piece | 2 | new, not held | 0 ms |

Plain HTTP on loopback has no handshake to save, so these show reuse and
that a warming never holds a piece, not the time it saves.

## What the warming saves, from the Mac

`curl` asking the same URL twice in one run (the second on the kept
connection), no key, five runs each; time to the first byte of the answer:

| URL | New connection (median, range) | Reused (median, range) | TLS done at (new, median) |
|-----|------|------|------|
| `https://api.elevenlabs.io/v1/models` (404 without a key) | 359 ms (335–583) | 247 ms (237–322) | 110 ms |
| `https://campaign-api.autonomous.ai/api/v1/ai/v1/elevenlabs/models` (404, `Cannot GET …`) | 336 ms (321–804) | 222 ms (201–233) | 120 ms |

About 110 ms a line from the Mac, up to 0.57 s on a slow first connection.
The proxy has no route for `models` but keeps the connection after its 404,
so the warming still opens the one the first piece takes. On the lamp the
journal's `piece 1: … on a reused connection` and its A, with the switch on
and off after quiet minutes, are the measure.

## Gates

From `lampOS/`, `RUSTC_WRAPPER=` `CARGO_PROFILE_DEV_DEBUG=0`
`CARGO_PROFILE_TEST_DEBUG=0` `CARGO_INCREMENTAL=0`, own `target/`:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings
cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 second_provider::net second_provider::services
cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

All exit 0: focused 95 passed; `lamp-voice` 2,444 passed, 0 failed, 12
ignored across 32 result summaries.
