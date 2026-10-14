# Motor full-suite ordering audit — read-only, 2026-10-15 local

The retained failures are consistent with startup or parent-service delay,
but the failing runs do not record enough state to choose between them.
Full-suite ordering can change process state, and one earlier test helper has
a concrete detached-listener lifetime problem. It is not a demonstrated cause:
the uninstrumented clean-7f motor-only run also failed without that prefix.
No motor repair, deadline change, gate pass, or scope expansion follows from
this audit.

No Cargo command, test binary, diagnostic child, or inspected application
process was launched. No live process inspection was performed. Reads used
the retained logs/metadata and repository source; this file is the only new
audit artifact. Main worktree source and all original diagnostic reports were
left unchanged.

## Sources and identities

The working HEAD read during this audit was
`bc3fccb546a22baaef180fcd57b3291a44e0e4c1`. A read-only diff from the failed
gate's base, `dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`, was empty for the
four motor files below. Their current SHA-256 values are:

| File under `lampOS/` | SHA-256 |
|---|---|
| `crates/voice/src/coordinator/motor/tests.rs` | `62d1f0b5746ecca1167bc02ce0377cf24d2a94ff3e1e73b92aa972d2ab67af33` |
| `crates/voice/src/coordinator/motor/tests/native_head.rs` | `a6870c6a1c2f063f5b83ec4e17bf3ed3f7e4d1c6c0bff3ee1942d1108ada38cc` |
| `crates/voice/src/coordinator/motor.rs` | `29b82801801be234ae865f7593c957a79537acc3469805401b759c21cf6c748e` |
| `crates/body/src/runtime/supervisor.rs` | `103009ea8a3baf449a760cf7f1dc43d0aa0b47d999b585c4084c65ff63ff2a3f` |

The retained dcc Task 31 binary was not run. Its existing
`/private/tmp/lamp-third-motor-original-binaries/task-dcc.json` records
42,478,616 bytes and SHA-256
`1e2ce0c369f34205f435a67c3751d48e4e1bc8ccfce7ea2d99681c60efd24393`,
captured after the failed full gate, not at execution time.

Principal logs and metadata:

- `/private/tmp/lamp-third-schedule-voice-dcc-gate-tests.txt` and
  `/private/tmp/lamp-third-schedule-voice-dcc-gate-results.json`.
- `/private/tmp/lamp-third-schedule-voice-gate-first-tests.txt` and
  `/private/tmp/lamp-third-schedule-voice-final-gate-tests.txt`.
- `/private/tmp/lamp-third-schedule-7f-motor-first.txt` and its `.json`.
- `/private/tmp/lamp-third-schedule-7f-clean-motor-first.txt` and its `.json`.
- `/private/tmp/lamp-third-schedule-motor-diagnostic-analysis.json` and
  `/private/tmp/lm709-e-y5uq6td4/module-ddbthlnu/results.json`.

All quoted run measurements are from the retained host records, not the
lamp: Intel Core i7-10700K at 3.80 GHz, 16 logical CPUs, 64 GiB RAM,
macOS 26.6.2 build 25G83 / Darwin 25.6.0 x86_64, Rust 1.96.0
(`ac68faa20`) and Cargo 1.96. The host record is
`/private/tmp/lamp-third-schedule-voice-machine.json`.

## What the retained order and backtraces establish

The dcc gate used `cargo test --offline --locked -p lamp-actions -p
lamp-schedule -p lamp-voice -p lamp-voice-eval -- --test-threads=1` with
`RUST_BACKTRACE=1`. The Voice library harness ran 929 tests: 918 passed,
3 failed, 8 ignored, in 491.25 seconds. Aggregate completed-harness counts
were 1,111 passed / 3 failed / 8 ignored. The later workspace build was not
reached.

Its motor tests were the contiguous 131st through 158th Voice test results;
the preceding 130 results passed. These are distinct processes from the
earlier Actions/Schedule test harnesses: those harnesses cannot directly
mutate the Voice process's environment, current directory, or Rust statics.
Their host load, filesystem activity, and any leaked external child would
still be possible indirect influences; those were not measured here.

| dcc Voice test ordinal | Failure | Backtrace locates the timed wait |
|---:|---|---|
| 132 | `crash_and_stalled_control_never_allow_replacement_or_false_cleanup` | `tests.rs:645`: reaping wait inside the `crash`, then `hang` loop. The raw log does not identify which mode failed. |
| 138 | `late_started_callback_cannot_restore_an_offer_after_owner_replacement` | `tests.rs:865`: its first `is_ready` wait, immediately after `fixture("gate_segment")`, before the inspection, gesture, owner replacement, or late callback. |
| 144 | `native_head_initial_privacy_acquisition_is_not_a_revoke_or_grant` | `native_head.rs:491`: first `is_ready` wait immediately after the explicit start, before the subsequent refresh/admission/frame/ticket checks. |

The corresponding dcc raw result lines are 424, 430, and 436; the retained
backtrace caller locations are at raw lines 1236, 1257, and 1278,
respectively, in `/private/tmp/lamp-third-schedule-voice-dcc-gate-tests.txt`.

All three report the original `tests.rs:441` assertion:
`owned child timed out: None`. This is not evidence that the advertised
late-callback or privacy semantics failed. It is also not sufficient evidence
that the child never entered its executable or that no IPC occurred.

The first new reminder-coordinator test appears at raw-log line 674; the
provider reminder tests at 937; reminder consumer/transport tests at 1043.
They run after the motor group. The earlier new
`admission::outage_drain_tests::prospective_speech_blocks_a_reminder_before_it_owns_a_stream`
test is local state-machine work (`admission.rs:737`), with no child, listener,
environment change, or scheduling change. The reminder child scheduling code
is Linux-only, and this gate ran on the x86_64 Mac. There is no evidence of a
reminder worker leaking into motor startup from an earlier test.

There is no stable failing motor ordinal across retained runs. The earlier
3773 full gate failed `frozen_config_and_executable...`; the d5 full gate
failed crash/stall and failed-reap; the dcc gate failed crash/stall and two
later startup waits. In isolation, uninstrumented Task 31 on 7f failed
crash/stall (27/1), and clean 7f failed crash/stall, explicit-current-segment,
and failed-reap (25/3). The clean isolated failure is direct evidence that
the full-suite prefix and Task 31 implementation are not necessary for this
class of failure to occur. It does not prove Task 31 has no effect on timing.

The instrumented clean-7f binary later passed three separate exact tests and
one 28-test motor module. Instrumentation, executable identity, timing, and
host conditions differ from the original failing runs. No causal or
qualification claim is made from those passes.

## Shared state and cleanup: concrete source findings

### Environment, directory, and clocks

Searches across local Rust sources found no explicit parent `set_var`,
`remove_var`, `set_current_dir`, `chdir`, `setenv`, or `unsetenv` mutation.
No pre-motor test was found changing a process resource limit or priority.
This is a source search, not an observation of the historical environment or
all behavior in external dependencies.

The fixture writes a private wrapper script with a fixed `/bin/sh` shebang;
it exports the five `LAMP_LIVE_MOTOR_TEST_*` values only in the child and
executes the absolute `current_exe()` path with exactly the `inert_child`
test selected (`tests.rs:400`). The motor supervisor supplies absolute
configuration and private-directory arguments. It does not clear inherited
environment or change cwd (`supervisor.rs:199` onward). Thus parent mutation
from a previous test is not found, but unrecorded launch-time loader/test
environment is not ruled out. The retained gate metadata records selected
environment entries, not a sanitized complete environment snapshot.

Each motor fixture uses a random-boot private `/tmp/lampOS-<boot>` directory
(`process.rs:53`), fresh controller/worker identities, and private socket
paths below that directory. The task helper's `/tmp/agent-tasks-<pid>` socket
paths are different. There is no observed shared motor endpoint or reused
temporary directory that would connect a motor fixture to the earlier helper.

Motor timestamps use `lamp_ipc::monotonic_us()` / the host monotonic clock
(`ipc/src/lib.rs:41`, `body/src/runtime.rs:16`), not a test-mutated global
clock. The helper uses a new `Instant` deadline per wait. Suspend behavior
and host scheduling were not captured in the failed runs.

### A preceding task helper can retain two blocked listeners

The dcc source of `crates/voice/src/agent_tasks.rs` was read directly from
Git as well as the current version. `tasks_part` creates one listener and
detached thread for each of `request.sock` and `voice.sock`; each thread uses
`for (stream, answer) in listener.incoming().zip(answers)`. The JoinHandles
are discarded, and the caller removes the directory after its expected
requests without joining or otherwise stopping those threads. Current source
locations are `agent_tasks.rs:488–508` and `:514–580`; the same helper is
present in dcc, before the later status-query additions in current HEAD.

After the final supplied answer, the next loop advancement can enter the
left-hand incoming iterator before discovering that the right-hand answer
iterator is exhausted. That is consistent with the documented ordering of
[`Iterator::zip`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.zip)
and the listener-backed
[`Incoming` iterator](https://doc.rust-lang.org/std/os/unix/net/struct.Incoming.html).
The source therefore has no finite thread completion proof and can leave
the two listener threads, listener descriptors, and sender clones alive
while later tests run. This is a source-level resource-lifetime finding;
thread/FD counts were not measured in the failed process. The official API
documentation consulted is the current documentation; fetching the exact
1.96 source through the browsing tool was unsuccessful.

The dcc log runs the socket-using task test at line 309, well before motor.
Its final confirmation also clears the `PENDING` mutex; motor code has no
dependency on that mutex. The potential retained listeners are blocked,
not a demonstrated CPU loop, and two descriptors/threads by themselves do
not establish resource exhaustion or a two-second spawn delay. Clean-7f's
motor-only failures occur without this helper. Do not repair it as part of
this scope or call it the motor timeout cause.

### Other worker lifetime checks and failure aftermath

The pre-motor camera tests use the ordinary Voice `Worker` path. Its Drop
implementation checks exit, kills if needed, then waits and cleans owned
paths (`process.rs:555–564`). Bus publishers/subscribers do not spawn relay
threads in their implementation; their socket ownership is scoped. These
reads did not expose a comparable unjoined pre-motor worker path. This is
not an exhaustive proof that every pre-motor test releases every resource.

Motor cleanup differs on panic. Successful helper cleanup explicitly asks
for shutdown and polls until reaped. On unexpected unwinding,
`OptionalMotor::drop` retains launch files if reap is unproved
(`motor.rs:1023`); `Supervisor::drop` issues a kill and performs only one
`try_wait` (`supervisor.rs:587–598`). If that immediate poll has not observed
exit, no later reap is guaranteed by this owner. A failed fixture can
therefore leave unresolved child/temporary-path cleanup during the remainder
of the test process. No actual zombie or leaked-child count was measured.
This could influence cases following the first failure, but cannot explain
the first motor failure in a fresh invocation.

## Timeout accounting matters independently of ordering

`wait()` starts its two-second budget after fixture setup/spawn returns
(`tests.rs:437`), publishes authority about every 10 ms, polls, and parks
for 1 ms. Its assertion runs before the next publish/poll and prints only
the adapter's cached `failure()` field. A delayed test thread can therefore
reach the assertion before polling again to discover a child event or fault.

The supervisor records spawn time and allows three seconds before its
startup failure (`STARTUP_US`, `supervisor.rs:18, 424`). It enforces that
deadline during `poll`, and the adapter adopts stored failures after
polling (`motor.rs:702`). A two-second helper timeout with `None` is thus
consistent with startup still pending inside the three-second allowance,
or with a delay since the last serviced poll. It is not a live child-state
measurement. The existing budgets must not be widened on this inference.

Socket operations are nonblocking, but a supervisor poll also calls
`directory.observe_peers()`: synchronous directory and endpoint metadata
reads (`supervisor.rs:94–123, 544`). The failure logs cannot rule out
filesystem or scheduler delays on the parent. The fixture's config/script
copies and `sync_all` calls happen before `wait()` starts; those setup
costs are not themselves charged to its two seconds. Qualification has a
fresh ten-second expiry per fixture, not a suite-wide deadline.

The passing diagnostics measured roughly 0.515–0.839 seconds after the
parent start returned and before inert-child entry. Entry to Ready-sent was
1.120–3.918 ms, with Ready observed 0.073–1.313 ms later. Those records
localize the dominant delay only in the passing executions. They contain
neither a failing child phase nor a failing parent-gap trace. Fresh-process
loader work, general scheduling/I/O delay, resource residue, and an IPC
problem in an unrecorded failure remain hypotheses. A simple “only the first
load is slow” explanation is insufficient by itself because dcc later fails
after several successful child startups.

## One bounded next diagnostic proposal — not executed or authorized here

If one further diagnostic is approved, use the already-built instrumented
clean-7f binary at its original path,
`/private/tmp/lm709-src-qhqu_pyi/target/debug/deps/lamp_voice-a98444cc1c3f37e5`,
after checking its recorded
`e0a5caa128cd7912f507a7e716fc24bde94cb8148480a68a28482341b1a77b5a`
hash. Run exactly one serial invocation containing the pre-motor test
modules plus the motor module, with a fresh short private diagnostic root;
exclude the large post-motor tail. Select the available tests from these
observed predecessor modules: admission, agent_tasks, air_told, attention,
body_link, bus_out, called, camera_inspect, camera_worker, choreography,
config, coordinator::camera, coordinator::cloud, coordinator::early_end,
coordinator::endpoint, and coordinator::motor. Validate the selected list
before executing it; clean 7f lacks later Task 31/dcc additions.

This compares against the recorded motor-only pass using the same
instrumented source, binary, and original executable path, while adding
the relevant in-process prefix. Bound the one invocation to 180 seconds,
preserve the unchanged assertions/deadlines, record exact selection,
source/binary hashes and all bounded diagnostic JSON, and do not retry.
No rebuild or change to the retained original Task 31 binary is needed.

The useful result would be a failing parent gap/child phase under the
prefix, or evidence that this bounded prefix also completes. A pass would
still not qualify Task 31. A failure only with the prefix would support
investigating a state/load interaction, not prove that the two task-helper
threads caused it. This proposal is preferable to another unexplained
whole-gate repetition because it preserves phase evidence and removes the
long unrelated tail. It remains an optional scope decision for Main/First.

## Correction to the earlier retained diagnostic report

Preserve the original report bytes. These three retained copies contain the
same incorrect sentence at lines 99–100:

> Four recorded instances intentionally spawned no child.

- `/private/tmp/lamp-third-schedule-motor-diagnostic-report.md`
- `/private/tmp/lamp-third-motor-diagnostics-text-20261015-5_scopox/report.md`
- `/private/tmp/lamp-third-schedule-voice-dcc-failure-log-20261015-wh_z_tg2/clean7f-diagnostics/report.md`

Correct explanation: three markerless fixture records belong to
`configured_camera_or_original_permissions_are_required_before_spawn`.
For each of its three cases, the test first calls `fixture("normal")`,
which does spawn the original motor child; it immediately requests shutdown
and waits for reap, then creates a separate denied candidate whose startup
is refused (`tests.rs:517–542`). No child-entry marker was captured for
those first children, so their executed phase is unknown. The fourth
markerless record belongs to
`native_head_startup_announcement_has_no_input_or_motion_authority`; that
test uses `fixture("startup")`, never starts the motor, and explicitly
asserts `!m.started()` (`native_head.rs:511–546`).

The source-level no-start assertion applies only to that fourth test and to
the three separate denied candidates; absence of markers alone does not
establish absence of a child. Counts, hashes, raw observations, and the
previously reported measured intervals are unchanged. Main should add this
correction beside the preserved report rather than rewriting historical raw
records.

## What remains unshown

No historical live environment, thread count, file-descriptor count, child
phase, scheduler stall, filesystem stall, or resource-limit exhaustion was
measured for the failed full suite. No exact-1.96 iterator implementation
source was obtained during this audit. No new test, gate, lamp behavior,
motor safety, or reminder qualification was demonstrated. Task 31 remains
unqualified while its required gate fails; nothing here authorizes changes
outside issue #709.
