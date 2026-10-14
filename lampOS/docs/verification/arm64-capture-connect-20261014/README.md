# Memory/Voice final ARM64 gate: capture fixture setup refusal

Ready for lamp-voice: no

Run [38582911230](https://github.com/autonomous-ai/lampOS/actions/runs/38582911230)
tested Memory/Voice head `68606e78e75369393168f184ae00ae210ecd80de`, based on
`724541a957e5e66bb4c0f4a8de537466c8a9fe55`. It ran on a GitHub-hosted
Ubuntu 22.04 ARM64 ordinary-user runner with Rust 1.96.0, not on a lamp.
The release workspace build passed. The serial workspace test command
exited 101: 187 completed suites, **3,417 passed, 1 failed, 30 ignored**.
Cargo stopped before the remaining suites, including VoiceEval. Binary
hashing and artifact upload were skipped. No Memory pull request is ready.

Commands, from `lampOS/`:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

`run.txt` is the complete, unmodified `gh run view 38582911230 --repo
autonomous-ai/lampOS --log` output; `run.json` is the run/job/step metadata.
`results.json` sums the final Cargo result lines, including the failed
suite. Raw GitHub timestamps/tab prefixes and progress whitespace are
preserved; exclude `run.txt` from documentation whitespace checks.
Its SHA-256 is
`cd4faa5f9446631d17a932f24423f3124b5b713059208d5138e1fe5549887261`.

## Found, not mine

The Linux-only `crates/voice/tests/capture_hold_worker.rs` target finished
2 passed / 1 failed. Test
`another_microphone_is_handed_on_at_once_and_its_early_sound_is_not_accounted_for`
panicked in `Rig::start`, line 212, at
`rig.speaker.connect_step().unwrap()`, before its audio assertions:

```text
PermissionDenied: peer must be a private socket owned by this user
```

Source inspection at the tested head establishes a possible setup race:

- `ipc/src/lib.rs:129-136` publishes a socket with `UnixDatagram::bind`
  before setting mode 0600. The path can be visible in between.
- `Endpoint::connect` rejects a non-socket, wrong owner, or group/other
  permission bits. `ReferenceChannels::connect_step` retries only
  `NotFound`; the fixture unwraps a `PermissionDenied` immediately.
- The fixture binds its speaker sockets completely before starting the
  capture thread, but starts its reciprocal connection before the capture
  thread has necessarily finished binding its sockets.
- `ipc/src/bin/lamp-ipc-probe.rs:213-248` already documents this same
  bind/chmod window and handles it in its bounded private startup loop.

The CI run did not record the refused path's metadata. The bind/chmod
window is source-proven; its occurrence in this run is an inference, not
a reproduced diagnosis. Wrong file type/owner also uses the same error.

A narrow fixture repair for its owner to assess is to keep servicing
authority/control, but call the speaker's `connect_step` only after
`rig.prepared` becomes true. `WorkerEvent::CapturePrepared` is sent over
the independently established parent control channel after both capture
reference sockets have been bound and capture has connected to the
already-bound speaker. It does not require the speaker's reciprocal
connection, so this ordering does not introduce a handshake cycle.
`WorkerEvent::Ready` is too early. Keep the existing deadlines, private
socket checks, error handling, and audio assertions intact. Do not broadly
ignore `PermissionDenied` or loosen socket permissions.

This is outside Fourth's three authorized Memory/Voice files
(`process.rs`, `remember.rs`, `thread.rs`); Fourth has not edited it.
The owner should run the complete capture fixture on Linux and the full
native ARM64 gate after a repair. This proposed fixture ordering does not
establish that every production reference-channel caller is race-free;
that wider startup audit belongs to the owner.

The fixture, `voice/src/transport.rs`, `voice/src/audio_workers.rs`, and
`ipc/src/lib.rs` are byte-identical between this failed head and Mood's
green `08c90d6b26ea0ba1b39f52f663b70af0f5a29865`. This fixture passed in
Mood run 38578348793. Memory's new `process.rs` environment filtering is
in `Worker::spawn`; this fixture starts `capture_on` with `std::thread`
and does not call it. These comparisons support reporting an inherited
setup finding; they do not replace a passing gate for Memory.

## Limits

No lamp, microphone device, cloud provider, or physical audio latency was
tested. The Mac gate cannot execute this Linux-only fixture. Earlier
passing checkpoints and current host gates do not clear this final
native ARM64 failure. No unchanged CI rerun was requested to obtain green.
