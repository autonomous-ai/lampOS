# Motor timeout diagnostics on the Third development host

All three exact diagnostic cases and one full 28-test motor module passed.
No earlier timeout was reproduced, and its cause remains unproven. These are
instrumented diagnostics on a clean integration source copy, not Task 31's
qualification gate or a lamp result. No production source, deadline, assertion,
main working-tree file, or original failure artifact was changed.

## Provenance and commands

- Host: Intel Core i7-10700K at 3.80 GHz, 16 logical CPUs, 64 GiB RAM;
  macOS 26.6.2 build 25G83 / Darwin 25.6.0, x86_64. Existing machine record:
  `/private/tmp/lamp-third-schedule-voice-machine.json`.
- Toolchain: Rust/Cargo 1.96.0. All commands used offline, locked dependencies;
  the Unix socket tests ran outside the filesystem sandbox with authorization.
  No lamp, provider, physical motor, microphone, or speaker was used.
- Base: `7fbb03e1f2e61ad6dfaf184cd768bad6524c2d76`. Source root:
  `/private/tmp/lm709-src-qhqu_pyi/lampOS`.
- Compared all 1,436 copied files: only
  `crates/voice/src/coordinator/motor/tests.rs` differs from clean 7f.
  Original SHA-256:
  `62d1f0b5746ecca1167bc02ce0377cf24d2a94ff3e1e73b92aa972d2ab67af33`;
  diagnostic SHA-256:
  `3e8146db4a627c2d6ef307191e70df7e0fd33143ac7526ae94d840fee2c557e3`.
- The 1,436-file source map remained identical before/after compile and all
  four test invocations. Main Task 31 source was never used or edited here.
- Compile started `2026-10-14T17:12:38Z`: exit 0, 43.334618403 s:

```text
cargo test --offline --locked -p lamp-voice --lib --no-run --message-format=json-render-diagnostics
```

The compiled binary is
`/private/tmp/lm709-src-qhqu_pyi/target/debug/deps/lamp_voice-a98444cc1c3f37e5`,
33,316,136 bytes, SHA-256
`e0a5caa128cd7912f507a7e716fc24bde94cb8148480a68a28482341b1a77b5a`.
Its hash was checked before and after every invocation. An identical copy is
retained at
`/private/tmp/lm709-e-y5uq6td4/module-ddbthlnu/instrumented-clean7f-test-binary`.
Do not commit that binary.

The three exact invocations used the binary above with
`--exact <name> --test-threads=1 --nocapture`, separately and serially:

| Exact case suffix under `coordinator::motor::tests::` | Test result | Command elapsed |
|---|---|---|
| `crash_and_stalled_control_never_allow_replacement_or_false_cleanup` | 1 passed, 0 failed, 0 ignored | 2.286751793 s |
| `failed_reap_after_stopped_is_latched_and_reported_once` | 1 passed, 0 failed, 0 ignored | 0.726539779 s |
| `explicit_current_segment_reaches_inert_counts_without_completing_voice` | 1 passed, 0 failed, 0 ignored | 0.785322131 s |

Result and raw compile/case logs:
`/private/tmp/lm709-e-y5uq6td4/run-pkppz5ov/results.json` and sibling
`compile.txt`, `case-1.txt`, `case-2.txt`, `case-3.txt`. Four fixture modes
produced ten saved wait records under
`/private/tmp/lm709-d-bxvzktai/run-pkppz5ov/`.

One further invocation, authorized after the exact-case report, used that same
binary without rebuilding:

```text
/private/tmp/lm709-src-qhqu_pyi/target/debug/deps/lamp_voice-a98444cc1c3f37e5 coordinator::motor::tests --test-threads=1 --nocapture
```

It started `2026-10-14T17:16:12Z`: exit 0, 17.261044278 s command elapsed;
libtest reports 28 passed, 0 failed, 0 ignored, 820 filtered out, 17.21 s.
Raw result/log:
`/private/tmp/lm709-e-y5uq6td4/module-ddbthlnu/results.json` and `tests.txt`.
There are 78 saved wait records from 29 fixture instances under
`/private/tmp/lm709-d-bxvzktai/mod-3uskl9aa/`. All saved waits say
`predicate_satisfied`. There was no retry and no further compile or test.

## Observed phases

For the exact cases, the following times are in milliseconds. “After start”
is the parent marker immediately after `start_before_capture` returns;
“entry” is the inert child's first marker inside its test function. That
interval includes shell/exec, loader/test-harness work and any scheduling
delay, which this instrumentation cannot distinguish.

| Mode | After start → entry | Entry → Ready sent | Ready sent → parent Ready observation | Core new | Core qualify |
|---|---:|---:|---:|---:|---:|
| crash | 534.674 | 3.623 | 0.463 | 0.018 | 0.018 |
| hang | 532.960 | 1.045 | 1.214 | 0.018 | 0.018 |
| gated_exit_failure | 619.946 | 3.580 | 0.473 | 0.018 | 0.018 |
| normal | 540.675 | 3.240 | 1.166 | 0.019 | 0.019 |

Crash reached the explicit `crash_exit_7` marker. Its last saved state was
reaped and not ready; the adapter stored `Connection reset by peer (os error
54)` and the supervisor stored `motor child failed or was killed`. Hang
reached `hang_enter`, then both adapter and supervisor stored `motor worker
progress timed out`; it was reaped and not ready. Normal completed its
segment, then emitted Stopped and `worker_run_ok`, with no stored failure.
The gated-exit case's saved trace ends at readiness: its custom subsequent
shutdown/reap loop is outside the sampled `wait()` helper. The exact test's
assertions passed, but no detailed shutdown timing is claimed for that case.

The full module recorded normal (16), crash (1), hang (2), gated_exit_failure
(1), gate_segment (3), startup (2), profile (1), gate (1), discard (1), and
bad_cleanup (1) fixture instances. Four recorded instances intentionally
spawned no child. Other custom loops may leave no saved record; 29 is the
number of fixture instances with saved records, not a count of all spawned
processes. All captured child marker sequences were contiguous.

For module fixtures with captured child startup, after-start → entry ranged
from 515.163 to 838.707 ms. The largest was the crash fixture in the second
test; it still passed. Entry → Ready sent ranged from 1.120 to 3.918 ms;
Ready sent → parent observation from 0.073 to 1.313 ms. Core construction
took 18–22 µs and qualification 16–32 µs. In startup mode the fixture's
`after_start` marker precedes the caller's actual start, so that mode's
interval is not a pure spawn-to-entry measurement.

Parent gaps need care. For every fixture's first saved wait, the recorded
global maximum was the initial fixture setup interval ending at
`before_start`, not a measured poll stall: 39.976–49.123 ms for the exact
cases, and 38.635–58.053 ms in the module. Only the first 16 and latest 256
samples are retained; the global maximum covers every sampled gap. The
largest retained first-wait sample gap after excluding `before_start` and
`after_start` was 1.325 ms in the exact cases and 1.423 ms in the module.
Those retained-sample maxima must not be presented as complete-loop maxima.
Later fixture-wide maxima include deliberate work between waits; the largest
was 354.189 ms in a profile fixture, not a demonstrated blocked poll.

The full derived per-fixture records, source paths, modes, phases, parent
states, coverage limits and retained-record list are in
`/private/tmp/lamp-third-schedule-motor-diagnostic-analysis.json`.

## Interpretation and limits

The dominant delay in these passing traces was before inert-child entry.
Configuration, inert motor construction/qualification, and Ready transport
were brief once entry occurred. No semantic motor failure was observed in
these diagnostics. A shell/loader/test-harness startup explanation for the
earlier timeouts remains a hypothesis: no failing instrumented trace or
OS-level loader measurement establishes it.

The earlier unchanged Task 31 motor-module run remains 27 passed / 1 failed,
and clean 7f remains 25 passed / 3 failed, all failures at the original
two-second `wait()` assertion with `owned child timed out: None`. Their raw
logs are `/private/tmp/lamp-third-schedule-7f-motor-first.txt` and
`/private/tmp/lamp-third-schedule-7f-clean-motor-first.txt`. The original
supervisor startup allowance is three seconds, so an adapter with no stored
failure at the helper's two-second boundary is consistent with pending
startup; it does not prove startup was the failing phase. Original binaries
remain under `/private/tmp/lamp-third-motor-original-binaries/`; their
manifest explicitly limits identity to hashes captured after those runs.

The extra nonblocking marker channel and parent samples perturb execution.
They cannot turn either earlier red run green or qualify Task 31. Do not
increase a deadline, weaken an assertion, or call the earlier failures a
known flake on this evidence. A complete unchanged-source Task 31 gate is
still required. If a timeout recurs, retain it and narrow diagnostics around
the failing phase rather than repeating whole gates until one passes.

No real-device timing, acoustic behavior, motor safety, fresh Linux process
startup, or Schedule reminder behavior was demonstrated. The execution slot
was returned to Main after the one module invocation.
