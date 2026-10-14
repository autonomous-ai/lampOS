# Voice fixtures on a loaded development host

Task [#739](https://github.com/autonomous-ai/lampOS/issues/739) concerns host
tests, not a change to the lamp's timing policy. `scenario_clock` and the
fixture helpers are compiled only under `cfg(test)`. There is no runtime
flag, environment setting or wider production deadline. Ordinary builds
continue to use the existing monotonic clock.

## Stepped scenarios

These existing cases explicitly select synchronous peers:

- `endpoint_session::complete_verdict_ends_early_but_resumed_speech_keeps_the_turn`
- `pilot_faults::ring_fault_during_an_answer_keeps_the_conversation_and_cleans_up`
- `pilot_faults::sigterm_while_the_failure_notice_plays_stops_cleanly`

The production coordinator, Unix channels, packet identities/sequences,
privacy gate, input admission and speaker `BoundaryGuard` remain in use.
Before each coordinator iteration, every emulated peer completes one
bounded step; scenario time then advances by 2 ms. A clock read never
advances time or runs a peer. Capture retains its 10 ms frame schedule.
The pilot privacy peer's requested 5 ms cadence becomes 6 ms on this
2 ms grid; permitted freshness remains strictly below 50 ms. The endpoint
peer's 10 ms privacy schedule is unchanged.

The scope belongs to one thread and cannot be sent or shared. Concurrent
scopes are independent; newly spawned threads retain the host clock.
Nested scopes and recursive pumps are rejected. A callback panic releases
its state and restores the host clock; an old guard cannot clear a later
scope. Session lifetime, peer schedules, envelope timestamps and age checks
share one clock. The 50 ms privacy, 60 ms reopening and 100 ms channel
boundaries remain unchanged.

All other endpoint/pilot scenarios retain real-time peers, including real
`ProcessClassifier` workers, mute/reopen cycles and the stalled trace writer.
The endpoint scenario entry accepts an in-process classifier double explicitly.
Original functional assertions remain in the three named cases. This does
not qualify every other timed test as immune to host scheduling.

## Actual processes and cleanup

Startup and shutdown keep real host deadlines. Stepped pilot children are
actual `cat` processes: a bounded echo proves that each has run and read its
input before scenario time starts. After closing input, bounded
`waitid(WNOWAIT)` observes successful exit of that exact PID. Nonzero exit,
signal death, timeout and an already reaped child are errors. The production
supervisor still owns the actual reap and exit-status checks.

Shutdown pumps receive the coordinator's existing shared 300 ms real
deadline (750 ms for diagnostic cleanup). They neither grant a fresh budget
per child nor advance scenario time or bypass the final receipt drain.
Fault injection cannot advance the shutdown clock. SIGTERM still signals
the actual test process. Real wall-clock watchdogs remain; logical scenario
durations and revocation intervals are not host or lamp latency measurements.

Clock tests cover host pauses, independent scopes, unscoped threads,
reentrancy, panic cleanup, frozen shutdown time and unchanged channel expiry.
A stepped coordinator regression advances exactly 50 ms after an accepted
privacy lease, then queues a fresh mute on the real channel. The existing
pre-drain freshness check must fail without a restartable physical-mute
token. Actual cleanup and reaping are still asserted.

## Motor setup

The motor fixture executes and reaps an inert preflight arm of its exact
frozen wrapper before starting the real supervisor protocol. It requires
the one-byte marker and successful exit; missing/wrong marker, nonzero exit
or timeout fail and reap the owned child. Preflight does not start a worker,
bind its sockets, open a device or establish motor readiness. The frozen
executable's identity and hash remain checked.

This separates a measured fresh-script execution cost on Fourth's Intel
Mac from Ready/authority tests. It does not explain every historical timeout
or qualify cold production startup. Original two-second fixture predicates
and production startup, progress, inhibit, permit and teardown bounds remain.
Failure output includes child phase, connection/Ready/reap state and the
observed parent poll gap.

The crash-preserves-voice fixture creates its active reply after config
freezing and unrelated child setup. While servicing the optional motor it
refreshes that same owner's existing 500 ms capture/admission lease, as live
capture would. It neither creates a replacement owner nor enlarges the
lease. Original audio-dispatch and cleanup assertions remain.

## Qualification and limits

Run from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --locked --offline -p lamp-voice -- --test-threads=1
cargo test --locked --offline -p lamp-voice-eval -- --test-threads=1
cargo build --workspace --locked --offline
```

The issue also requires ten full serial `lamp-voice` runs on the Mac beside
a Podman ARM64 build, and a green exact-head native ARM64 workspace workflow.
Source/build identity, raw results, load boundaries and failures belong in
the task's verification folder and Fourth's log. An isolated passing case
is not a passing full gate.

No lamp is accessed. A privacy peer's send-side gap is not the coordinator's
consumption age. The real 50 ms bound may still be tight on a loaded, hot
lamp; host fixtures cannot settle that question. First must measure physical
observation through coordinator consumption on the lamp with its actual
services and temperature, without widening the bound on synthetic evidence.
