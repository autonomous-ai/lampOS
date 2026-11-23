# Private socket startup

A full offline voice test run exposed a startup race before any microphone or
cloud work: `Worker::spawn` failed with `peer must be a private socket owned by
this user`. The parent waited only for both worker pathnames to exist.
`Endpoint::bind` creates a Unix socket and then sets its mode to 0600, so the
path can exist before that final permission change. Connecting in that interval
correctly fails the endpoint's privacy check.

The parent now waits for both paths to be actual sockets owned by its effective
user, with no group/other access. A missing path or a socket still becoming
private is not ready. A regular file, symlink or different owner remains an
error. A permanently unprivate socket times out; it is never accepted.

The original three-second startup deadline, five-millisecond poll interval,
child-exit checks and supervisor heartbeat callback remain unchanged. This
wait happens before listening readiness. Final `Endpoint::connect` validation
and every runtime transport freshness check still run normally. There is no
permission relaxation, process-wide umask change, or reconnect of a partially
connected channel pair.

Deterministic tests exercise missing paths, both non-private sockets, only one
private socket, the fully ready pair, files and symlinks. Existing real child
handshake and fixture-provider tests cover the completed startup path. The
combined source's commands and receipts are recorded in
[provider flow control](provider-flow-control.md); host results are not a
Linux/ARM64 or physical-device qualification.

The same interval could still be hit by a connect that did not go through that
wait: one capture hold worker test failed in a native ARM64 CI run on
2026-10-14 with this refusal (the refused path's mode was not recorded, so this
is the likely cause, not a reproduced one). `Endpoint::connect` now answers `WouldBlock` ("peer is not private
yet") for a socket owned by this user that still has group or other access, and
the callers that retry on `NotFound` retry on it too. A socket of another user,
a file or a symlink is still `PermissionDenied`, and a socket that stays open is
never connected: the caller's deadline ends the wait. After the change, on the
Mac `lamp-ipc` passed 17/0 and `lamp-voice` 1035/0 (one test at a time); under
aarch64 emulation `lamp-ipc` passed and `capture_hold_worker` passed 3/3 in each
of five runs. `crates/ipc/tests/sockets.rs` checks both answers.


## A worker that cannot be spawned

Every worker is spawned from the voice's own binary, `current_exe()`. A spawn
that fails names its program: `spawn <program>: <error>`, with the error's
kind kept (#7402). A bare `No such file or directory (os error 2)` had not said
which program was missing. A failure to read `current_exe()` itself is said as
`spawn lamp-voice's own binary: <error>`. Only the words change: the start
fails as before.

A binary removed while it runs (a build folder cleaned away under the
service) reads from `/proc/self/exe` as `<path> (deleted)`, the same rule as
lamp-update's `own.rs`. `serve` reads it once before its first worker and,
when it reads so, ends with `lamp-voice: its own binary <path> was removed
while it ran: it cannot start its workers; start lamp-voice again from an
installed build`. It exits 1, as any failed start does, so systemd, the
restart guard and the start limit see what they saw before. There is no
retry and no fallback path.

## Optional ring startup

An unavailable ring no longer turns a healthy voice startup into a failed run.
The ring uses the same three-second socket/Ready deadline and five-/two-millisecond
poll cadence as before. During that wait, required provider/audio workers still
receive supervision; a mandatory callback error, including termination, retains
its original error kind and remains terminal. Required supervision also runs
before the optional attempt and after a fast bind/exec failure. Privacy activation
still follows startup; no input permission or ready cue is synthesized for a
missing ring.

A failed ring attempt retains its unready child in `OptionalRing`, inhibits it
before one Stop attempt, and uses the existing 300 ms cooperative grace and
nonblocking kill/status path. Cleanup continues while healthy voice runs. There
is at most one ring attempt per session, with no reconnect or replacement.
Only a completed handshake creates an available ring/choreographer. Late Ready,
Fault or presentation messages from a failed startup remain historical evidence.

Each optional attempt owns a fresh mode-0700 `ring/` namespace within the private
session directory. After confirmed child exit, cleanup removes only expected,
owned peer socket inodes. A changed inode, symlink, file or replaced directory is
a cleanup failure and is left intact; cleanup never deletes recursively or fixes
permissions. Parent endpoints remain owned until Worker drop. Same-UID processes
are outside the endpoint authentication boundary, as before. Native process and
filesystem syscalls have no hard userspace execution deadline.

Ring startup observations have their own 16-event allowance, separate from the
required workers' startup capacity. Overflow is counted as
`ring_startup_observations_dropped`; it cannot terminate healthy voice. Required
startup faults and their capacity checks are unchanged. The count and retained
ring observations transfer to the normal trace at activation.

Failed stop delivery, forced termination, abnormal exit, missing shutdown black
receipt, or unverified socket cleanup remain final cleanup failures. A child that
was never created cannot report a black write; its unavailability event records
`process_started=false` and does not claim a visible state. A failed running
writer cannot prove a dark physical ring, and uncertain cleanup still prevents
service-level automatic reopen after physical mute.

Host regressions exercise actual child spawn and private channels with benign
sleep children/fake ring peers: missing executable, failed and successful Ready
handshakes, both startup timeouts, mandatory faults during the wait, retained
ownership, late Ready, forced reap, unsafe path preservation, metadata capacity,
and continuing cancellation/privacy authority. These establish software ordering
and resource ownership, not optical darkness, loaded-device timing, or physical
voice qualification.

The overflow count includes lifecycle receipts as well as worker observations.
For example, after 19 queued observations, the 16-slot buffer discards three;
a later degradation notice makes four, and an observed owned-child exit makes
five. A grace-expiry kill request is also a separate lifecycle receipt. These
are truthful losses of optional evidence, not consumption of required startup
capacity. Tests force a pre-spawn namespace failure for the no-child case and
separately exercise an owned exited child with empty, nearly full and overflowing
buffers. Neither a discarded exit receipt nor a successful child exit proves a
black physical ring, and repeated cleanup ticks cannot recount the same exit.


## Integration verification, 2026-10-11

The exact composed source passed host formatting, strict live-package Clippy and
58 focused tests. Root's source-only Linux/ARM64 gate passed formatting, strict
live all-target Clippy, 113 selected test executions (18 optional-ring, 18 process,
22 camera unit and 55 integration tests), and the release build. These are
emulated software checks, not a native ring or optical qualification.

The initial ARM run exposed a test-only assumption that a missing executable
never yields an owned child. The fixture now separates guaranteed pre-spawn
failure from actual exited-child cleanup; the original failed assertion is
retained. Production behavior was not changed to suppress a valid exit receipt.

Frozen source manifest:
`5dec54ea3a142bba37fef085161df5e04f3f3e1c1bb9dbcf590a369e9bc27d0b`.
Extracted AArch64 runtime (8,405,512 bytes):
`1f4c117a817243cba8ceb147bfa93fe341ca5a6c8d537808823f83ee5696697f`.
Local gate receipts: `/private/tmp/lamp-ring-budget-v2-ep7a3ssb/arm/evidence/`.
All compiled changed files match the integrated bytes; only subsequent product
and evidence documentation differs. This binary has not yet run on a Lamp.
