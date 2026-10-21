# Wi-Fi helper completion, 22 October 2026

Part of #1806. Kind: test harness only. Base:
`b87b3e91ad04b5811fe441efa118773d33416630` on `lamp-voice`.

The three `online::tests` reconnect scenarios joined a setup-helper thread
that held its socket open for three seconds after receiving `online`.
That hold kept the connection alive but was not an observation asserted by
any scenario. `JoiningHelper::join` now sends finish after the scenario
stops and awaits the cloud task, then joins the helper and returns the same
timestamps and online line. The helper reads that line and records its
timestamp before it can receive finish. A helper panic still reaches the
test through its join result. Dropping the sender releases the final wait
if the scenario unwinds.

The new finish wait has a fifteen-second failure bound, longer than
`wifi_against`'s ten-second deadline. This bounds only the final hold;
earlier blocking socket accept/read operations are inherited unchanged.
No product source or scenario body changed, including all assertions.
The real 300/1,600 ms helper delays, one-second broker ACK delay and
2,400 ms gate opening remain. The production prefix and scenario suffix
are also checked byte for byte and hashed in [source-sha256.json](source-sha256.json).

## Comparable host measurements

The planned targets were under two seconds each for the stale-link and
will cases, and under 3.5 seconds for backoff. Each measurement launches
one exact test in an already compiled libtest executable and includes
launch, execution, cleanup and output capture, excluding compilation.
All three cases passed three times before and three times after, serially,
at the same base and with the same environment.

| Case | Before, run order (s) | After, run order (s) |
|---|---|---|
| Stale link, confirmation starts at ACK | 5.2423 / 4.3879 / 4.3859 | 2.3298 / 1.3853 / 1.3818 |
| Replacement connection retains its will | 4.3875 / 4.3873 / 4.3854 | 1.3897 / 1.3897 / 1.3864 |
| Join interrupts backoff and resets it | 6.2008 / 6.1999 / 6.1977 | 3.2485 / 3.2535 / 3.2562 |
| Sum of the three separate processes | 15.8306 / 14.9751 / 14.9690 | 6.9680 / 6.0285 / 6.0244 |

The first stale-link invocation still misses the two-second target; its
result is retained. Subsequent invocations meet all planned targets.
The backoff test remains over three seconds because its original timing
scenario is retained. This batch does not complete #1806, and no timing
here measures interaction latency on a lamp.

[Before timings](before-host-timings.json), [after timings](after-host-timings.json),
[before transcript](before.txt), [after transcript](after.txt).

The successful full-main [native ARM64 run #5220](https://github.com/autonomous-ai/lampOS/actions/runs/39639887119)
at this base recorded 4.362 / 4.368 / 6.168 s for the three cases,
14.898 s combined. [Extracted rows](arm64-before-5220.json) come from the
first nextest result section, before its summary and success-output
recaps. They are before-only native context, not a Mac/native comparison.
The PR records its own exact-head native result before it is marked ready.

## Mutation controls

Each temporary product mutation compiled, then its exact test exited 101
at the original assertion. All source bytes were restored after each
control and checked again after the final gate.

- [Window before ACK](mutant-window-before-ack.patch): calling
  `changes.acked` when taking the outcome, before publication, failed the
  original `window >= 1 second` assertion at 35.758988 ms.
- [Reconnect without a will](mutant-reconnect-without-will.patch): only
  the replacement client used plain options, failing `a will on each
  connection` for that connection.
- [Backoff not reset](mutant-backoff-not-reset.patch): resetting to seven
  times the initial wait failed the original `next < 600 ms` assertion,
  with its next attempt about 701 ms later.

[Raw control output](mutation-controls.txt). Each control ran once and
failed its intended check; none of these product mutations is shipped.

## Local gate and replay

The final gate, sequentially from `lampOS/`, passed:

```text
cargo fmt --all --check
cargo clippy -q -p lamp-cloud --all-targets --message-format=short -- -Dwarnings
cargo test -q -p lamp-cloud
cargo build -q --workspace
git diff --check
```

Tests: 142 library and 23 integration passed, **165 passed, zero failed**.
The one existing ignored test is `reports::tests::write_a_reports_fixture`,
an opt-in fixture writer requiring `LAMP_CLOUD_REPORTS_FIXTURE`; no ignore
was added. The initial formatting check found two signature wraps, which
were corrected before the final gate. [Gate transcript](local-gate.txt).

[Environment](environment.json) records the host, Rust 1.96.0 and build
settings. [Commands](commands.json) records each exact argv, exit code and
wall duration. Its stdout/stderr names identify sections in the grouped
transcripts: all test and gate output and all compiler stderr are retained.
Compiler stdout keeps the tested executable's `compiler-artifact` and
`build-finished` records; unrelated dependency metadata is omitted.

The measuring Python scripts stay outside the repository. To reproduce,
compile the unmodified base with `cargo test -p lamp-cloud --lib --no-run
--message-format=json`, find the `lamp_cloud` test executable, and run each
full test name from the timing JSON with `--exact --nocapture`, three times
serially. Repeat with this fixture patch. For each control, apply its Rust
patch separately over the fixture change, compile and run the corresponding
exact test, verify its failure, then restore the product bytes before the
next control and final gate. Measure each subprocess with a monotonic host
clock. No ARM64 build ran on the Mac; no device was contacted.
