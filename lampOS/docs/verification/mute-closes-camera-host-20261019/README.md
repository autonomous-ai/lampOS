# A mute tells the eyes no at once (#1802)

Host run on the Mac (Darwin 25.6, Rust 1.96.0), 2026-10-19, based on
`lamp-voice` at `3c45161f9`. No lamp, camera or device was touched.

## What changed

Before, the voice left its conversation at a mute (the switch's privacy
event, the privacy gate, a software mute or a head put to sleep) without
reaching its per-loop `eyes.permit(..)`, and dropped the `EyesLink` silently:
the eyes closed the camera only on their own 1.5 s without a yes, 1.0 to
1.5 s after the mute. Now each of the three mute exits in
`crates/voice/src/coordinator.rs` calls `tell_eyes_no`, which sends
`{"camera":false}` at once (one non-blocking datagram, never waited on,
sent once), and dropping an `EyesLink` that last said yes says no too, so
any other end of the conversation (a fault, a stop) closes the camera
without the 1.5 s. The eyes are unchanged.

## Journal lines to time on a lamp

- Voice, at the mute: `lamp-voice: the eyes are told no at once: software mute`
  (also `head sleep`, `physical mute (privacy event)`,
  `physical mute (privacy gate)`); `crates/voice/src/coordinator.rs`,
  `tell_eyes_no`. Said only when the no was sent now.
- Eyes, the camera shut: `lamp-eyes: camera closed: not permitted`
  (`crates/eyes/src/main.rs`), or
  `lamp-eyes: camera closed: not permitted after opening in N ms` for a no
  that came while the camera was opening.

The time between the two, five times before this change and five after,
is the lamp measurement; it is not taken here.

## Commands (from `lampOS/`, sequential)

`PATH=$HOME/.cargo/bin:$PATH RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`, `cargo +1.96.0`, `-j2`,
`--test-threads=1`.

| Gate | Result |
|------|--------|
| `fmt --all --check` | clean |
| `clippy --workspace --all-targets -- -D warnings` | clean |
| `test -p lamp-voice --lib body_link` | 13 passed |
| `test -p lamp-voice --lib privacy` | 71 passed |
| `test -p lamp-voice` | 2365 passed, 0 failed, 12 ignored (2125 in the lib) |
| `build --workspace` | ok |

The base did not compile without #1919's two one-line fixes
(`UNITS: [&str; 24]` in `crates/cloud/src/reports.rs`, `breath: None` in a
`speak_up.rs` test). They were applied only for this run and are not part of
this change.

New tests in `crates/voice/src/body_link.rs`:
`a_closing_cycle_tells_the_eyes_no_at_once_and_only_once` (no within 50 ms
of the close, just after a yes; a second close and a drop add nothing; a
drop while permitted says no) and
`a_close_with_no_eyes_listening_neither_waits_nor_fails`. The three call
sites themselves are not driven by a coordinator rig test: the link comes
from `LAMP_EYES_PERMIT_SOCKET` in a continuous service only.
