# Memory/Voice final ARM64 gate: capture fixture setup refusal

Ready for lamp-voice: no

Run [38582911230](https://github.com/autonomous-ai/lampOS/actions/runs/38582911230)
tested Memory/Voice head `68606e78e75369393168f184ae00ae210ecd80de`, based on
`724541a957e5e66bb4c0f4a8de537466c8a9fe55`. It ran on a GitHub-hosted
Ubuntu 22.04 ARM64 ordinary-user runner with Rust 1.96.0, not on a lamp.
The release workspace build passed. The serial workspace test command
exited 101: 188 completed suites, **3,429 passed, 1 failed, 30 ignored**.
Cargo stopped before the remaining suites, including VoiceEval. Binary
hashing and artifact upload were skipped. No Memory pull request is ready.

Commands, from `lampOS/`:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

`run.txt` is the complete, unmodified `gh run view 38582911230 --repo
autonomous-ai/lampOS --log` output; `run.json` is the run/job/step metadata.
`results.json` sums the standard libtest result lines and the custom
`turn-end-wire` process harness, including the failed suite. Earlier
summaries omitted the custom harness's 12 passing tests; the corrected
totals change neither raw output nor the run's failure. Raw GitHub timestamps/tab prefixes and progress whitespace are
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
repair production startup ordering; the source audit below establishes a
separate pre-fix exposure. First subsequently repaired the production
connection retry at the source, as recorded below.

The fixture, `voice/src/transport.rs`, `voice/src/audio_workers.rs`, and
`ipc/src/lib.rs` are byte-identical between this failed head and Mood's
green `08c90d6b26ea0ba1b39f52f663b70af0f5a29865`. This fixture passed in
Mood run 38578348793. Memory's new `process.rs` environment filtering is
in `Worker::spawn`; this fixture starts `capture_on` with `std::thread`
and does not call it. These comparisons support reporting an inherited
setup finding; they do not replace a passing gate for Memory.

## Second occurrence without the Memory task's runtime changes

Native ARM64 [run 38593943161](https://github.com/autonomous-ai/lampOS/actions/runs/38593943161)
failed on documentation-branch head
`ad368abb1b81ec6561eb6497d74c18a2bfd9fd6c`. Every file changed from its
base `3773c1fb88190fb5289bf1378eaa1f5fe5d134cb` is under `lampOS/docs/`;
its runtime source is exactly that upstream base. None of the Memory/Voice
task's unmerged runtime or test additions is included.

The same GitHub Ubuntu 22.04 ARM64 ordinary-user configuration and Rust
1.96.0 passed the release workspace build, then failed the same fixture
at the same line 212 with the same private-peer `PermissionDenied`. The
fixture finished 2 passed / 1 failed. The workspace test command exited
101 after **3,410 passed, 1 failed, 29 ignored**, 185 completed suites.
Later tests, binary hashing and upload were not reached.

`second-run.txt` is the complete unmodified `gh run view 38593943161 --repo
autonomous-ai/lampOS --log` output; `second-run.json` contains run/job/step
metadata and `second-results.json` contains counts and source identity.
The raw log's SHA-256 is
`61f614f0b393ffd3141a26167fd929705ca88b326ac3279a96b63e33edaf8ab1`.
Exclude this raw log from documentation whitespace checks for the same
byte-preservation reason as `run.txt`.

This establishes that the failure can occur without the Memory task's
changes. It still does not record the refused socket's metadata or prove
that the bind/chmod window caused either occurrence. The fixture and its
security checks were unchanged at that checkpoint. First answered the
owner question at 23:26 ICT with the source repair recorded below.

## Pre-fix production reference startup also lacked a binding acknowledgement

Fourth audited local Memory head
`2d081316b44f81dfc50589a3e6b554af2c88d993`, based on public upstream
`d5d765862ab1f73629ee7a2c5a9a8086f0246d11`. The coordinator, audio workers,
reference transport and IPC source are unchanged by the Memory task. The
following is a source-proven possible interleaving, not a reproduced
production failure or a measurement on a lamp:

- `coordinator.rs:1841-1868` spawns capture and then speaker.
  `process.rs:236-262` waits for each worker's ordinary `{role}.c.w` and
  `{role}.d.w` sockets to be private, connects them, then waits for
  `WorkerEvent::Ready`. These are not the `ref.*` sockets.
- Capture sends `Ready` at `audio_workers.rs:325` and speaker at line 875,
  before either binds its direct reference sockets.
- `coordinator.rs:2147-2158` sends `ConnectReference` to capture and then
  speaker without waiting for a binding acknowledgement. They bind at
  `audio_workers.rs:354` and line 941. `transport.rs:135-136` binds each
  pair's control socket before its data socket.
- Both workers independently call `connect_step` at
  `audio_workers.rs:374` and line 994. One can finish binding its own pair
  while the other is between `UnixDatagram::bind` and chmod. If that path
  still has group/other permission bits, the strict peer check correctly
  refuses it, and `connect_step` propagates the error.

`CapturePrepared` does not provide a preconnection barrier in production.
Capture emits it only after its own `connect_step` succeeds (lines 373-385);
speaker reads the direct `ReferenceControl::CapturePrepared` only after
its own connection succeeds (lines 994-1006). The coordinator records the
event at `coordinator.rs:5008-5011`; it does not use it to order reference
binding. Delaying `StartCapture` alone would not prevent either worker
from attempting its connection. The fixture proposal above works because
that fixture fully binds speaker first; that premise is absent here.

Existing deterministic tests at `process.rs:516-552` cover private-mode
readiness and invalid paths for the ordinary worker sockets. Reference
tests cover a missing peer followed by a completed bind
(`tests/reference_link.rs:13-22`) and pairs fully bound before connecting
(`tests/reference_prime.rs:23-30`). They do not pause a reference peer
between bind and chmod. These are source coverage observations; Fourth
did not add or run a reproduction for this outside-scope audit.

The initial mode depends on the process umask, and the interleaving depends
on scheduling. Neither CI failure captured those socket metadata, and no
production or lamp failure has been reproduced. A fixture-only change
would have left this production exposure unresolved. Fourth has not
changed these files.

## First repaired connection readiness at the source

First
[answered on #698 at 23:26 ICT](https://github.com/autonomous-ai/lampOS/issues/698#issuecomment-6173108388)
with commit `dd6d512b1bcaf37bfa0dfa0a305b12268f5b854b`, followed by startup
documentation at `7fbb03e1f2e61ad6dfaf184cd768bad6524c2d76`. Fourth read the
answer in the 23:53 queue check and inspected the actual diff. An own-user
socket with group/other mode bits now returns `WouldBlock`, without
connecting. Wrong owner or file type still returns `PermissionDenied`.
`ReferenceChannels::connect_step` retries `WouldBlock` for both control and
data within its existing deadline. This handles the production sequence
as well as the fixture without weakening the private-socket requirement.
The IPC socket test verifies repeated refusal before chmod and successful
connection afterward.

First reports Mac IPC 17 passed, Voice 1,035 passed, and IPC plus repeated
capture fixtures under ARM64 emulation. Those are First's reported host
checks, not Fourth's measurements and not lamp evidence. Fourth will
rebase Memory and run its gate; the older failures remain retained.

One related caller needs its owner's attention: at the repaired head,
`ipc/src/bin/lamp-ipc-probe.rs:213-248` still retries only `NotFound`,
`ConnectionRefused` and `PermissionDenied` during its documented bind/chmod
window. It now propagates `WouldBlock`, although the helper in
`ipc/tests/probe.rs` was updated. This is a concrete error-kind mismatch in
the production probe startup path, not an observed failure. Fourth has
not edited this outside-scope caller.

## Limits

No lamp, microphone device, cloud provider, or physical audio latency was
tested. The Mac gate cannot execute this Linux-only fixture. Earlier
passing checkpoints and current host gates do not clear this final
native ARM64 failure. No unchanged CI rerun was requested to obtain green.
