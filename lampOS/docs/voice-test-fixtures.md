# Voice fixtures on a loaded development host

Task [#739](https://github.com/autonomous-ai/lampOS/issues/739) concerns host
tests, not a change to the lamp's timing policy. `scenario_clock` and the
fixture helpers are compiled only under `cfg(test)`. There is no runtime
flag, environment setting or wider production deadline. Ordinary builds
continue to use the existing monotonic clock.

The diagnostic DSP-reset sequence fixture uses the recorder's existing
`MAX_FINISH_WAIT` (1 s) for its real file syncs. It checks queued submissions,
an acknowledged `Complete` for sequence 2, and acknowledged `Invalid` results
for sequences 1 and 3, with matching summaries, markers and frame/end counts.
A timeout can no longer satisfy either negative case. Production's 500 ms
default and 1 s maximum are unchanged. The original 500 ms timeout remains a
failed observation; this fixture budget neither proves its cause nor guarantees
completion under arbitrary OS or filesystem delays.

## Kept privacy lease resynchronization

The kept-value resync fixture deliberately receives no datagrams. Its local
declaration uses a one-second publication period so the stock 20 ms
datagram-silence notice cannot end the reader during its two bounded 500 ms
reads. Kept-file renewals do not reset that silence timer.

Only this fixture's publication period changes. Its envelope lifespan and
typed capture lease remain 50 ms, its zero-datagram and sequence-gap checks
stay intact, and resync still ends at the first datagram or the one-second
boundary. The separate post-datagram negative uses the stock declaration;
the Bus period/late and typed privacy freshness controls remain unchanged.
This isolates the resync assertion from publisher liveness, not from real
lease expiry: a host stall beyond 50 ms can still exhaust its existing ten
attempts and fails the test. No product declaration or timing policy changes.

## Histories on a freshly booted host

The #1569 Voice fixtures that place events before the present choose a
600-second epoch with `Scope::enter_at`. Their timestamp producers and age
readers share that scope, preserving the original 400-second room history,
follow-up windows and every assertion. `Scope::enter` continues to choose
its epoch from host uptime; unscoped tests continue to read the host clock.

The hand-control scope starts after real child readiness, before the first
queued fault and the camera observer's timestamp, and lasts through rig
teardown. Multiple turn-taking harnesses on one thread share the same
scope lifetime. The explicitly real-time hand-latency fixture stays on the
host clock. Only the paused spoken-to follow-up fixture selects this epoch;
its existing logical-time pump is unchanged.

The stale-audio fixture uses a separate `cfg(test)` clock stored in its
`Live` instance: 600 seconds plus real elapsed time. Both its sender and
actual worker use that clock. The production three-second age bound and
revocation lock remain unchanged, and unconfigured instances use the same
raw clock as before. A control that admits four-second-old audio still
fails the original audio-count assertion.

The [verification record](https://github.com/autonomous-ai/lampOS/tree/lamp-tests/lampOS/docs/verification/voice-boot-clock-20261023)
keeps original failures, exact case lists and Linux x86/Mac clock controls.
These fixture results do not establish lamp latency or qualify the separate
Policy service clock, privacy-lease history or ignored host-latency probe.

The separate same-voice loading-child fixture is outside that 86-case clock
inventory. It observes a complete PID and distinct acknowledgements from a
child held on a private FIFO before and after the three original line checks;
the child never sends Ready. Startup, probes and cleanup use five-second real
host watchdogs, independent of paused Tokio time. Cleanup closes the real
`Prints` request sender before releasing the hold, checks the child's release
acknowledgement and observes supervisor reaping without consuming its status
or sending a PID-based signal. A failed startup or cleanup fails the fixture;
an unproved cleanup retains its directory for diagnosis rather than removing
a script that may still be starting. Production readiness and response
deadlines remain unchanged.

## Stepped scenarios

The six endpoint cases with an in-process classifier explicitly select
synchronous peers:

- `endpoint_session::complete_verdict_ends_early_but_resumed_speech_keeps_the_turn`
- `endpoint_session::verdicts_after_the_result_timeout_fall_back_on_every_turn`
- `endpoint_session::rapid_resumes_never_end_the_turn_early`
- `endpoint_session::a_complete_verdict_ends_at_one_second_and_its_answer_plays`
- `endpoint_session::a_worker_failure_falls_back_through_recovery_and_then_resumes`
- `endpoint_session::duplex_complete_verdict_ends_at_one_second_and_its_answer_plays`

The pilot fixture makes its mode explicit at every scenario. Its 32
single-cycle scenarios use stepped peers, including the original ring and
SIGTERM regressions and the deliberate privacy-expiry regression. Rapid
mute/reopen retains real-time peers because it exercises actual service
privacy waits across controller lifetimes. The stalled metadata writer
remains a separate real thread while its synthetic peers use stepped time.

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

Real `ProcessClassifier` workers retain real-time peers. Their finite ordering
fixture explicitly selects the existing per-instance test privacy allowance
of 100,000 us and a 12-second real conversation watchdog. This covers missing
model, hung worker, loading worker and duplex missing-model scenarios, plus
the existing ignored real-model provenance case. The six in-process stepped
scenarios and all three endpoint mute/cleanup scenarios retain the default
50,000 us gate. Every ordinary build retains that default too.

The allowance is capped at the existing 100,000 us IPC ceiling; it changes
neither envelope/source timestamps nor input admission. Worker startup,
result, hang, recovery and teardown deadlines remain real and unchanged.
The watchdog starts immediately before conversation, excluding startup and
cleanup. A host-clock guard rejects an active logical scope before creating
the real endpoint child. There is no new Ready wait: missing and still-loading
workers are intentional states in these tests. This is bounded scheduling
tolerance, not a qualification of arbitrary load or the lamp's 50 ms margin.

The stepped endpoint entry accepts an in-process classifier double explicitly,
including its scripted failure/recovery timer. Original functional assertions remain.
Pilot speaker-stall and revocation assertions now measure policy time;
the separately printed conversation/cleanup elapsed duration remains host
time. This does not qualify every other timed test as immune to scheduling.

The endpoint fixture's synthetic capture stream has one immutable source
origin and a sample block every 10 ms. Both peer modes retain each block's
scheduled acquisition timestamp when delivering a backlog. A delayed thread
therefore cannot manufacture a gap in these continuous synthetic samples.
Envelope delivery, the real classifier and unscoped policy clocks stay real;
old source audio is never refreshed to the delivery time.

Two controlled-clock regressions send through actual Unix channels: a 60 ms
delivery pause preserves the original contiguous sample stream, and fresh
envelopes carrying source audio aged 98/100/102 ms retain the unchanged
admission result (accept/reject/reject). Failed scenarios print bounded
after-send source-gap, delivery-gap and source-age diagnostics. These are
producer observations, not coordinator consumption ages or lamp latency.
The real-worker fixture also retains bounded privacy producer diagnostics;
they cannot distinguish coordinator consumption age from host scheduling.

The pilot fixture uses the same continuous source model in both modes,
with its origin set when the capture peer receives `StartCapture`. Each
bounded peer step sends at most 64 due blocks, retaining their original
10 ms acquisition schedule. Three deterministic checks cover a 60 ms
delivery pause, input admission at 99,999 us versus refusal at 100,000 us,
and rejection of an actual dropped block or source gap. Rapid-reopen
failures print each unsuccessful cycle's diagnostics before the service
assertion, including the largest capture send-attempt gap and oldest source
age at an attempt. These include failed send attempts; they do not measure
coordinator consumption. Its real privacy peer and 50 ms bound remain
unchanged.

## Explicit transport success observations

Test-only `Channel::install_probe()` attaches a bounded observer before any
send or receive attempt, including an attempt that fails or finds no packet.
Ordinary channel, worker-channel and reference-channel constructors remain
unobserved. No probe API or state exists in non-test builds.

A probe records only an actual successful kernel send or a fully decoded and
validated accepted receive, after the original sequence/receipt commit. It adds
no I/O, receive, clock sample, pump, advance or await. Original receive samples,
boot/order/age validation, startup grace, stall handling and gap acceptance stay
unchanged. Failed sends and refused/empty receives do not increment observations.
Probe faults do not change the transport operation's actual success or error.

Snapshots retain an allocation identity for that exact channel, including after
the channel and probe drop. Each direction records its configured boot, accepted
count, actual last sequence, consecutive prefix from sequence 1, and first gap.
An accepted gap stops that prefix permanently; a highest sequence or count alone
cannot prove complete consumption. State has fixed size and stores no event log.

Publication and snapshot reads are nonblocking. A snapshot makes one bounded
consistency attempt; `Busy` supplies no receipt and never implies completion.
Counter/publication overflow, invalid state and duplicate/late installation
latch failure without further successful evidence. Snapshots are historical
transport observations: they prove neither worker handling, Actor/service
progress, downstream completion nor child exit. A retained snapshot does not
validate the probe's later health. A later controller must check the live probe,
retain causal targets and use its own real watchdog; this preparation adds no
controller and migrates no scenario.

## Real-provider ordering scenarios

The EarlyEnd module runs the actual provider worker, Gemini supervisor and
a scripted WebSocket service on a real Tokio runtime. Its ten coordinator
scenarios run eleven `Setup` conversations (the option comparison runs
twice). Every conversation explicitly selects a per-instance privacy
freshness bound of 100,000 us, matching the existing worker IPC ceiling,
and a 12-second real conversation watchdog. This is bounded host scheduling
tolerance, not an arbitrary-load guarantee. The test-only constructor
accepts positive bounds at most 100,000 us; it and the override field do not
exist in normal builds. Every ordinary gate remains at 50,000 us, including
unrelated tests.

Capture/privacy/speaker producers start after the actual provider Ready
barrier. Slow provider setup cannot fill their queues with old samples.
The synthetic capture source retains its immutable 10 ms acquisition
schedule even when delivery is delayed. Original source timestamps, packet
order, 100 ms IPC/capture bounds, 60 ms stable reopening and the shared
300 ms shutdown remain intact. The 12-second watchdog begins immediately
before conversation; it excludes the existing three-second provider
readiness wait and subsequent cleanup. The fixture's sleeping child
processes still require forced termination, as before; their teardown is
not claimed as cooperative shutdown evidence.

The provider and scripted service share one current-thread Tokio runtime.
The fixture obtains the service's abort handle within the existing Ready
budget, then requests its cancellation before coordinator shutdown sends
provider Stop. Capture, privacy and provider peers retain their normal
cleanup order and budget. Both Tokio tasks are joined; unexpected task
panics propagate, and only cancellation requested by the fixture is accepted.
This prevents a closing provider connection from racing further scripted
answer sends, without swallowing a background failure.

The separate Tokio name-note test exercises the Gemini supervisor directly;
it does not construct `Setup`, capture peers or a privacy gate. It retains
its original continued/abandoned request flow. Both fixture entrypoints
reject an active scenario clock before starting child or provider resources,
so provider, envelope and coordinator clocks remain real and coherent.

Six privacy regressions cover exact observation, state and continuity
boundaries; per-instance/default isolation; immediate mute and 60 ms
reopening; invalid configuration; and future, stale and reordered
observations. Two EarlyEnd fixture regressions prove rejection of a scoped
clock and continuous source timestamps after delayed delivery, including
actual input admission at 99,999 us and refusal at exactly 100,000 us.
Three further checks cover requested cancellation, unexpected cancellation
and propagation of a background panic.
Every original scenario assertion remains, including the lone-name case's
one request with audio blocks 76 through 290, one early end, no abandoned
request and 48 speaker chunks. These tests do not measure the lamp's 50 ms
privacy response.

## Holding-answer reminder ordering

The provider-worker scenario
`a_reminders_holding_words_grant_nothing_and_the_next_heard_request_still_qualifies`
runs on a paused Tokio runtime and the existing `logical_clock()` fixture
(#1569, row 77). It takes the shared worker-state lock before starting the
scenario watchdog, so waiting for another test does not consume that budget.
The rig's heartbeat, owner and worker advance together in 2 ms ticks. A host
pause between the reminder's last heartbeat and `complete_turn` therefore
cannot expire the fixture's unchanged 100 ms input leases.

The scenario still completes the reminder through the real turn controller
before admitting the next heard request. It checks that the reminder grants
no holding-answer observation, executive call or camera permission, and that
the next heard request produces exactly one holding-answer observation with
its own request and session. Host watchdogs and channel operations remain
real. This is test-ordering evidence, not a measurement of lamp latency or
privacy response. The [verification record](verification/holding-answer-clock-20261021/README.md)
keeps the host-pause controls, reminder-origin mutation and local gate results.

## Actual processes and cleanup

Child startup, exit observation and the shared shutdown watchdog keep real
host deadlines. Scoped protocol deadlines, including camera teardown, use
logical time. Stepped pilot children are
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
The shared real deadline still terminates a child that has not exited; these
stepped cases do not measure the camera's separate teardown deadline.

Clock tests cover host pauses, independent scopes, unscoped threads,
reentrancy, panic cleanup, callback-error recovery, watchdog expiry before and
after a peer step, frozen shutdown time and unchanged channel expiry.
A stepped coordinator regression advances exactly 50 ms after an accepted
privacy lease, then queues a fresh mute on the real channel. The existing
pre-drain freshness check must fail without a restartable physical-mute
token. Consumption also includes the ordinary 2 ms step and the lease's prior
age; this proves expiry and ordering, not exact consumption age. Actual
cleanup and reaping are still asserted.

The stalled-metadata scenario submits an event through the normal logging
path and waits for the actual writer's entered receipt before starting
children or scenario time. The writer stays blocked on its own real channel
through audio delivery, actual SIGTERM and coordinator cleanup. Separate
host instants measure signal send through verified child exit/reap, retaining
the original limit of less than one second. `finish_logging` still waits on
the real 500 ms bound; its test allows 250 ms of host scheduling margin and
requires the unacknowledged report. Logical time cannot satisfy either
measurement. Normal release is checked, and a later real writer-finish
receipt confirms it drained after release without changing the earlier
unacknowledged report.

A test-only release guard also unblocks that writer during assertion unwind.
A deterministic regression unwinds an entered real stalled writer with a
typed panic, preserves that panic and requires its eventual finish receipt.
It does not suppress writer failure or turn the stalled report into success.

The cue peer-loss fixture establishes actual socket closure after dropping
its receiver: a copied receiver can outlive that local handle. Fresh
nonblocking, connect-only probes must return `ConnectionRefused` within a
five-second host setup watchdog. Each live probe is dropped before a 1 ms
poll; any other error fails immediately. The probes send no cues. Once
closure is observed, the original single `CueSink::scan` must still latch the
send failure. The stale-cue checks, trace, timestamps and all original
assertions remain unchanged. This setup observation does not retry a product
send or change its deadline, and does not identify the cause of a particular
failed run.

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

## Head qualification on one clock

The head-qualification fixtures (`motor/tests/native_head.rs`) run the real
Supervisor, Driver, qualification and worker against an inert child process.
On the host clock they failed under load: on 2026-10-15, on an Apple M-series
Mac at load average 14 to 37, 6 of 60 runs of these 11 tests. A pause of either
process for longer than one of production's own bounds expired a segment
(`SegmentExpired`), a 100 ms fixture lease or a 100 ms packet age. Widening
the fixture's inputs to the largest values production accepts only halved that
(10 of 200), since pauses over 600 ms were seen.

So parent and child share one stepped clock: in ten of the eleven tests, including the two that start the motor after the head (`fixture("startup")` before, now `stepped_fixture("startup")`). The eleventh, `native_head_binding_changes_only_explicit_camera_boot`, checks a config binding and starts no child. Since then the module runs on pull requests again; it is no longer in the realtime group (`realtime-tests.md`). `stepped_fixture` writes
`scenario-now` in the session directory before the child starts, and enters a
`scenario_clock` scope whose every step writes the parent's instant there and
waits until the child has written the same instant to `scenario-done`. The
child sees the file, reads its time from it (`FixtureClock`, for its motor
core and worker), and in place of `worker::run`'s sleep waits for the next
step after each poll. A finished child writes `u64::MAX`, and the parent's
step then waits until that child has exited, so the child's host exit time
reaches no teardown deadline. A child that has exited ends the parent's wait.
Time therefore moves only in 2 ms steps that both processes have finished, and
no host pause reaches a deadline.

A child held at its `go` gate (`TestLink::gate`) still finishes each step:
it writes the instant to `scenario-done`, waits for the next one, and only then
looks for `go` again, so it never works at an instant it has already finished.
The privacy-revoke fixture runs this way (#949), with two causal regressions:
a 30 ms host pause before its release, and a child that takes 300 ms to exit
after finishing. On the host clock the first failed 10 of 10 runs with
`motor inhibit acknowledgement timed out`. Without the exit wait the second
failed 10 of 10 with an unconfirmed privacy release. #906's three gated tests
(`late_started_callback_…`, `same_owner_input_end_…` and `pending_inspection_is_revoked_…`) run the same way, each
with a 30 ms-pause twin; on the host clock each failed with that pause.

The parent's Supervisor measures its deadlines on the same clock:
`Supervisor<C: Clock = SystemClock>` in `crates/body/src/runtime/supervisor.rs`,
with `spawn_with_clock`, and the qualification's polls read `supervisor.now()`.
Production builds name `SystemClock` everywhere, through `Supervisor::spawn`,
and the voice's motor adapter and head driver read `lamp_ipc::monotonic_us`;
only `cfg(test)` builds read the scenario clock, which is the host clock in a
test that enters no scope. Every production constant, deadline and order is
unchanged, and the fixtures again use their original 30 ms segment, 100 ms
lease and 400 ms step bound. The 15 s preflight wait stays: it is the fixture's
own first exec of a fresh script, before any Supervisor deadline, and it ran
past the 3 s worker startup bound once under load. Real watchdogs (30 s each
way, the scope's own) end a stalled scenario as a failure.

When a fixture child ends early it logs every Rejected, Inhibited, Stopped or
Fault event to its `child-output`, and the parent's failure message includes
that file's first 2 KiB.

## Qualification and limits

Run from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --locked --offline -p lamp-voice -- --test-threads=1
cargo test --locked --offline -p lamp-voice-eval -- --test-threads=1
cargo test --locked --offline -p lamp-memory -- --test-threads=1
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

## Endpoint fixture cleanup after an expected failure

Task [#860](https://github.com/autonomous-ai/lampOS/issues/860) follows the two
endpoint-session `LEAK` warnings in full ARM64 run
[38737583302](https://github.com/autonomous-ai/lampOS/actions/runs/38737583302).
Both tests passed their assertions while a fixture could still hold inherited
stderr. That run remains the original evidence; a passing assertion does not
establish clean child teardown.

The unconfirmed-exit scenario retains its 1,500 ms worker grace and the
coordinator's 500 ms teardown wait. It first checks the original `Pending`,
unreaped record, non-restartable error and refusal to reopen service. Only
after those assertions does a fixture guard wait for the existing supervisor
to confirm actual reaping, with a separate five-second host cleanup bound.
This wait does not alter the recorded runtime result or grant the runtime a
longer teardown deadline.

The still-loading worker uses `exec sleep 6`, so the supervisor owns the
sleeper itself instead of a shell with a sleep descendant. It still sends no
Ready during the same four-second session, and the original 1,200 ms silence
fallback assertion remains. It uses the same final cleanup guard; stderr
remains inherited and production process supervision is unchanged.

Before handoff the guard owns the endpoint. Its destructor closes any
endpoint still in that slot before waiting. Once the endpoint moves into a
helper, that helper drops the classifier on return or unwind before the
caller's guard waits. A cleanup timeout or unreaped result fails a normal
test. During an existing panic it prints the cleanup failure and preserves
the original panic rather than aborting with a second one.

One real-child regression covers both a panic before handoff and a panic
inside the receiving helper. Each child must report Ready while its monitor
is still pending; after the typed panic is caught, a zero-duration monitor
wait must report Reaped, with one spawned and one killed worker. The zero
wait cannot finish cleanup for a guard that returned too early. This is
fixture lifetime evidence, not a new product shutdown guarantee.

The source change alone does not qualify the fix. Required evidence remains
the original assertion and identity comparison, this panic regression and
the two original cases without nextest `LEAK`, the crate/dependent gates and
a green exact-head native ARM64 run. No lamp or production-worker behavior
is measured by these fixtures.

## Transport exit deadline fixture

The original Eval transport-expiry audit keeps its live `/bin/sleep 30` child,
the full five-second deadline, and every original certification assertion. Its
explicit unit-test clock advances only at the original 20 ms wait points.
Clock reads do not advance time. Each loop still asks the actual child whether
it exited before checking the deadline; expiry still calls actual kill and
wait, and returns no successful transport status.

The fixture verifies 250 waits, the complete logical interval and an actually
reaped SIGKILL status. A separate immutable five-second host watchdog is
checked before each logical advance. Process startup and actual kill/wait
retain their original blocking calls; this is not an end-to-end host bound.
An unwind guard kills and reaps the child if a fixture check panics.
Boundary cases cover live expiry, zero budget, actual successful and
failed exits, pure clock reads and watchdog cleanup. A stepped deadline does
not certify a synthetic exit or establish real-device shutdown latency.

The clock installation is compiled only for unit tests. Ordinary backend
construction, all integration tests and the runtime continue to use the same
system monotonic samples, 20 ms sleeps, exit checks and cleanup order.

## Delayed final-dispatch line fixture

The original final-drain session-end case explicitly selects a test-local
driver for its real shell and stdout pipe. The first `cat` must have returned
and its complete line must be queued by the actual backend reader before
logical time advances. At the original 300 ms delay, a private FIFO releases
the unchanged later bytes. The second `cat` completion marker and the reader's
second complete-frame count are both required before advancing farther.

The driver never consumes a backend line. The original 800 ms dispatch target
and final drain still discover the terminal line, withhold playback and retain
the RunEnd for the runner. Clock reads are pure. Ordinary helper calls keep
their host sleeps; this first migration does not control the closed-transport
case, whose real reader/channel closure needs separate evidence.

Control writes are nonblocking. One immutable ten-second host watchdog bounds
the driver's progress checks; it does not interrupt blocking filesystem or OS
cleanup calls. Dropping the driver attempts an abort token and closes its FIFO.
The backend retains ownership of stdin close, actual exit and its existing
two-second kill/wait fallback, including abandonment before the child opens the
FIFO. Neither a completion marker nor an empty queue certifies child reaping.

## Reminder round trip on real Stores

Task [#935](https://github.com/autonomous-ai/lampOS/issues/935). One consumer
tick runs under one 250 ms socket budget (`REQUEST_BUDGET`), and the budget
also spans the ledger's durable writes between exchanges: the Get, the Ack
plan committed to the ledger, and the Ack that Schedule commits before it
replies. Both Stores sync in full (`PRAGMA fullfsync=ON`), which on a Mac
flushes the drive. Measured on an idle Intel Mac, the Ack tick spends about
160 ms of its 250 ms: 77 ms to commit the plan, 80 ms for Schedule's commit
and reply. On a busy disk the plan alone took 318 to 340 ms, so the Ack was
never sent (`pending` stayed true), or Schedule committed it after the
reply's deadline had passed (one Ack request, no `AckConfirmed`). Production
recovers on the next tick with the same durable Ack, so the cause is the
tests' assumption that one tick on the host's disk fits the budget, not the
consumer.

The two tests that tick on real Stores now run in a scenario scope:
`reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`
and the four tests in `coordinator::tests::reminder_roundtrip`. In a scope
no logical time passes inside a tick, so `request_deadline()` gives the
socket exchanges the scope's 24 s host watchdog in place of the budget, and
the consumer, the parent's reminder lease (750 ms) and the round-trip fixture
read `scenario_clock::monotonic_us`. Every original assertion stays,
including exactly one Ack request. Outside a scope nothing changes: the
budget is still `Instant::now() + REQUEST_BUDGET`, and the consumer tests
that hold it (a slow Get withdraws the offer, a delayed prepare cannot
publish an expired offer) stay on the host clock.

## Speaking speed through a small reply buffer

`provider_worker::tests::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`
(and the ignored sixty-second and three-minute answers, through the same
`speaking_speed`) failed in the reviewer's #985 gate with a ReadTimeout and
then a ConnectionReset at the rig's heartbeat. The test plays the
coordinator, the provider worker and the scripted service as tasks on one
thread. Between two coordinator ticks the worker and the service decode and
encode the largest messages, which in a debug build held a tick back by
60–99 ms on an idle Intel Mac and by 101–144 ms in the runs that failed.
Past 100 ms the coordinator's capture and admission leases lapsed, the
controller gave up the turn, and the worker retired the answer it was still
receiving: what arrived after that put the session behind a barrier, which
the script's 3 s network stall ended with BarrierTimeout, or later
ReadTimeout. The worker then stopped and the next heartbeat met a reset.
On a lamp the coordinator is a process of its own.

`speaking_speed` now enters a scenario scope, and each coordinator tick of
`play_at_speaking_speed` is one 2 ms step. The provider worker reads
`scenario_clock::monotonic_us` under `cfg(test)`, so its 250 ms heartbeat
bound and the authority it installs are on the same steps as the
coordinator's 100 ms leases. Under 24 CPU-bound processes on the same Mac,
the test passed 0 of 10 runs before (all BarrierTimeout) and 10 of 10 after.

Since 2026-10-16 (#823) playback, the four-second hold and the service's
pacing are on Tokio's clock (the modeled speaker and both elapsed times
read `tokio::time::Instant`), and the twenty-second answer runs on a paused
runtime (`#[tokio::test(start_paused = true)]`). Each coordinator tick is one
scenario step and then a 2 ms Tokio sleep, so the two clocks move together
and nothing waits on the host. Every assertion and bound is unchanged. On
the Intel Mac the test took 24.04 to 24.10 s before and 2.8 to 3.8 s after
(5.6 to 7.1 s with six copies at once), and its `MEASURED` line is the same
on every run: hold 4,000 ms, playback finished at 24,004 ms, completion at
20,004 ms, starved 0 ms, written in 4,200 ms, 500 packets, peak 5.00 s.
Before, those were 4,000 to 4,001, 24,024 to 24,028, 20,003 to 20,007, 0 to
1, and 4,326 to 4,350 ms. The ignored sixty-second and three-minute answers
keep an ordinary runtime, so they still play in real time.


## Keep-alive HTTP fixture framing

The two chat keep-alive mocks answer complete header-only GET requests, not
individual TCP reads. Like the TTS mocks, they accumulate through CRLF-CRLF
and keep any following bytes for the next request, with a 4,096-byte header
cap. Idle durations, the 50 ms overlap delay, pool limits and all original
connection-count assertions are unchanged. Production client code is unchanged.

A split-header control waits until the server has processed an incomplete
header, sends the final separator, then half-closes the request stream. It
reads responses through EOF and requires exactly one response. This detects
premature per-read replies without a short absence window or an assumption
that two writes are two reads. The retained three-versus-two full-run outcome
is not assigned a historical cause by this fixture correction.

## Mock Eyes accepted connections

`remember_speaker::tests::own_name_exchange` polls a nonblocking Unix
listener so its peer can observe completion without waiting for another
connection. Each accepted stream explicitly enters blocking mode before
the existing two-second socket read timeout and `read_line`.

On macOS the accepted stream can inherit the listener's nonblocking mode;
an empty read then returns `WouldBlock` instead of waiting under that
timeout. Clearing the accepted stream's flag leaves listener polling,
request/reply assertions, timeout values and cleanup unchanged. This is a
test-fixture correction, not a runtime or historical failure-cause claim.

## Memory foreground admission without a stopwatch

The two Days writer admission fixtures retain their real client and Unix socket
peer. The idle fixture holds the append acknowledgement until `finished_exchange`
returns, then checks the original append contents, source ID, submission flag,
conversation prefix and idle lower bound. The full-queue fixture observes the
first held append, admits exactly the configured queue capacity, refuses the next
exchange and counts exactly one loss before releasing the acknowledgement. The
peer no longer releases itself on a timer.

Each scenario runs as exactly one selected test in a child process, with private
socket/log paths. The parent requires native success and the exact one-test pass,
so selecting no tests cannot qualify it. A fifteen-second diagnostic watchdog
kills and reaps a stuck scenario, including its threads; it does not measure
product latency or promise an operating-system teardown deadline. Child stdout
and stderr have separate files, so diagnostics cannot split a native result line.
Only owned fixture files are removed after child cleanup. Failed scenarios keep
their stdout/stderr files for diagnosis, including watchdog failures.

The original 25 ms and 100 ms admission upper bounds are replaced by these
return-before-release observations. The 25 ms idle lower bound, the Group B
request-budget checks and all production timeouts, queue bounds and code remain
unchanged. A production writer timeout is still a test failure if it loses the
held exchange; it is never counted as a successful acknowledgement or release.

## Routine policy time and event transport

Three Skills Routine fixtures step their Manager's clock to check delayed
wall-clock samples and a future sample: `a_delayed_wall_sample_cannot_renew_or_admit_after_its_window_ended`,
`a_delayed_sample_inside_the_window_keeps_its_original_deadline`, and
`a_future_wall_sample_cannot_admit_work_or_extend_the_cached_window`.
Their `clock_policy` fixture keeps only the request publisher's transport
header alive for a bounded five host seconds. The request declaration stays
an Event with a 500 ms lifespan; production publishers and all other fixtures
keep that lifespan. The synthetic clock, payload deadlines, cancellation
assertions and 50 ms receive bound stay unchanged.

A paired real-bus control uses fresh fixtures and the same 700 ms host pause.
It peeks the actual queued header and payload without consuming the message,
then the original subscriber records a stock event expiring and the bounded
fixture delivering its exact request. Both publications must be accepted,
with no dropped sends; Manager time and deadlines must not advance. Header-age
and elapsed-time checks reject observations outside the controlled window,
and all diagnostics wait until both observations are captured. Missing-request
diagnostics identify the None/Rest/Wake iteration, expected operation,
publisher/expiry counters and prior request before cancellation.

This isolates policy assertions from unrelated host scheduling during a
real-wall transport lease. It does not increase any product deadline,
qualify arbitrary host stalls, prove device latency, or retrospectively
identify the cause of older failures that retained no expiry evidence.

## Joining the initial Actions mock reply

The Skills repeat-call fixture for a tool containing only made steps keeps
its two original calls, refusal assertions and three-second light duration.
Its Actions mock starts before the child, as originally, and handles one
initial `light_colour` request in a scoped thread. After both call assertions, the fixture joins that worker and checks
the exact white-light request and complete ACK before dropping the child.
This observes the initial exchange, not completion of the three-second run;
no synchronization was added before or between the calls.

The worker uses an absolute 20-second accept watchdog and a separate
absolute two-second request/ACK budget, with an 8,192-byte request cap.
Cancellation is checked in 20 ms I/O slices. Ordinary I/O, parse and deadline
errors fail the test; only explicit teardown returns cancellation. The scope
joins during unwind while the child still exists. Controls close the peer
after a witnessed complete request before releasing the ACK, require a
typed failure witness emitted by the actual write, and unwind
both before connect and while a partial request's peer stays open. They
require a joined error or explicit cancellation, rather than a detached
panic or an EOF standing in for cancellation. Host overruns fail those
controls; no product timeout or readiness policy changes.

## Timed light calls beside disk flushes

`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
keeps its 25 ms median for a light call (#1806). On macOS it failed in full
runs of Voice's `it` binary and passed alone (#3315): its calls took about
19, 38, 57 ... 164 ms. That is a whole number of `F_FULLFSYNC`s, which
`File::sync_all` and `sync_data` are on macOS, about 19 ms each on the office
Mac. The bus writes and renames each topic's `latest` file. Under `/tmp` on
the Mac's disk, those writes wait for a flush running beside them. Paired with
each module of the binary in turn, only `diagnostics` (the recorder syncs its
files) and `news_told` (the tasks part syncs its store) slowed it. Sixteen
busy processes did not. An outside process doing nothing but `F_FULLFSYNC`
slowed it the same way, and a plain `fsync` (0.1 ms) did not. A lamp's bus is
on tmpfs (`/run/lampos-bus`), and Linux has no `F_FULLFSYNC`.

So those tests take turns (`tests/it/disk_flush.rs`). `diagnostics`'
`Private` and `news_told`'s `Place` hold a shared `disk_flush::flushing()`
for as long as they exist, so they still run beside each other and every
other test. A thread takes the shared lock once however many it holds, so a
test with two folders never waits behind itself. The light-call test holds
`disk_flush::timing()` alone while it measures. The assertion, the 2 ms ring
pass and the calls are unchanged. Under `cargo nextest` each test is its own
process, and the lock orders nothing. A flush from outside the binary, such
as another build on the same Mac, can still slow the timed calls.
