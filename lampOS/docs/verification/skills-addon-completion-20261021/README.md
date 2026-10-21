# Skills add-on process completion verification, 2026-10-21

For the later quiet-say rename correction and fresh gates, see the
[rebase verification](../skills-addon-rebase-20261022/README.md). The measurements
below remain historical on their stated base.

## Scope

This test-only change replaces settling sleeps in the two add-on process
tests in `crates/skills/tests/it/addon.rs`. The before and after runs use
the same base, `f349d63c7935dd16decc0badd4dd4e18c10094eb`. The existing
test clock helper in `tests/it/skills.rs` is made visible to its sibling
test module. Product code, configuration and proposal limits are unchanged.
Baseline test-source hashes are in
[baseline-host-timings.json](baseline-host-timings.json); the candidate's
four verified source hashes are in [source-sha256.json](source-sha256.json).

With `LAMP_ADDONS=on`, each of three resting-state publications waits for
`quiet-note`'s `SkillStage::Done` before the next publication. Every event
read during those waits is kept for the original assertions. This ensures
that all three triggers are processed separately, including proposal-limit
refusals; Latest-state publications cannot coalesce during a busy run.
Each wait has a 10-second deadline and reports child exit or captured
stderr when it fails.

Because `Done` has no request ID, correlation relies on this fixture
having one skill and publishing serially.

With the switch off, the fixture observes the loaded announcement and
then requests a zero-duration clock skip. The skip handler runs after
startup subscription setup, so its acknowledgement establishes that the
subscriber-absence check is made after setup was attempted. It does not
guarantee that an OS subscription succeeded. The enabled case waits for
the actual subscriber and fails its bounded wait if no subscriber appears.
Subsequent zero skips
let the loop consume pending input without advancing time. They are not
used as run-completion acknowledgements: the enabled test uses `Done`
and does not pass a clock-control flag.

Both tests retain the final 500 ms proposal and 100 ms event absence
drains, all proposal/event assertions, and the real 10-second proposal
limit. The changed waits observe completion or startup; the absence
observation intervals remain intact.

## Measurement

Host: macOS 26.6.2, x86_64; `rustc 1.96.0 (ac68faa20 2026-05-25)`.
Each measurement invokes one exact test in the compiled libtest binary.
Wall time includes process launch, execution, cleanup and output capture;
compilation is excluded. Each pair ran sequentially three times before
and after. The target is **under one second for each warm test invocation**.
These are fixture-duration measurements, not device latency measurements.

Commands run from `lampOS/`:

```sh
export PATH=/Users/autonomous/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target
export CARGO_BUILD_JOBS=4
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo test -p lamp-skills --test it --no-run --message-format=json
/private/tmp/tests-codex-1569-target/debug/deps/it-73966c5ffb3365b1 addon::with_the_switch_the_program_writes_one_proposal_stamped_with_the_add_on --exact --nocapture
/private/tmp/tests-codex-1569-target/debug/deps/it-73966c5ffb3365b1 addon::without_the_switch_no_add_on_is_loaded_and_nothing_is_written --exact --nocapture
```

Seconds below are in run order. The first enabled invocation is retained
in each series; it was slower than the following two.

| Test | Before: 3 runs | After: 3 runs | Median before → after |
|---|---|---|---|
| Enabled: one stamped proposal, later proposals limited | 3.0274 / 1.6089 / 1.6143 | 2.0284 / 0.6933 / 0.6918 | 1.6143 → 0.6933 |
| Disabled: no add-on loaded, no proposals/events | 1.9027 / 1.9135 / 1.9067 | 0.6475 / 0.6463 / 0.6467 | 1.9067 → 0.6467 |

The warm after runs meet the target. The first enabled after invocation
still takes 2.0284 s, so these results do not show a sub-second first use.
Both tests passed all three before and after runs. Full precision and
exact commands are retained in
[baseline-host-timings.json](baseline-host-timings.json) and
[after-host-timings.json](after-host-timings.json);
[timings.txt](timings.txt) keeps every measured run's output.

## Controls

All three isolated mutants compiled successfully and then failed their
selected test with exit 101 at the intended assertion:

| Mutation | Observed failure | Test-process wall seconds |
|---|---|---:|
| [Reset proposal limits before every admission](mutant-rate-limit-bypassed.patch) | Enabled test received 3 proposals where it expected 1 | 1.4521 |
| [Ignore the off filter and stall 500 ms after the loaded announcement, before subscriptions](mutant-off-filter-ignored-with-slow-startup.patch) | Disabled test: `an add-on was loaded without the switch` | 1.9239 |
| [Suppress the add-on Done event](mutant-done-suppressed.patch) | `timed out waiting for quiet-note completion after publication 1` at the 10-second wait bound | 10.9329 |

Each mutant is compiled separately and uses the same exact named test.
The original main source was restored byte for byte in `finally` before
the next mutant and the final gates. All four recorded source hashes
still matched after the final gates. [controls.txt](controls.txt) contains
the exact commands and full assertion output. To replay, apply one patch
from the repository root with `git apply`, compile and run its selected
test using the commands above, then reverse it with `git apply -R` before
the next patch. The measuring Python script remains outside `lampOS/`.

## Gates

The restored candidate passed these commands sequentially, with exit 0:

| Command | Result | Wall seconds |
|---|---|---:|
| `cargo fmt --all --check` | Passed | 12.9022 |
| `cargo clippy -q -p lamp-skills --all-targets --message-format=short -- -Dwarnings` | Passed | 41.1458 |
| `cargo test -q -p lamp-skills` | 129 library + 19 binary + 238 integration tests passed; 0 failed or ignored | 20.1232 |
| `cargo build -q --workspace` | Passed | 59.1646 |
| `git diff --check` | Passed | 0.0732 |

Full output is in [gates.txt](gates.txt). Only documentation and evidence
were finalized after these gates; the verified source hashes remain unchanged.

## Evidence files

| File | Contents |
|---|---|
| [environment.json](environment.json) | Exact base, host, Rust version, build environment and measurement boundary |
| [baseline-host-timings.json](baseline-host-timings.json), [after-host-timings.json](after-host-timings.json) | All six before and six after measurements, exact commands and exits |
| [timings.txt](timings.txt) | Raw measured-test output, grouped under original filenames/labels |
| [controls.txt](controls.txt), [gates.txt](gates.txt) | Exact commands, exit codes and full stdout/stderr for the controls and final gates |
| [compilation.txt](compilation.txt) | Test-executable and build-finished JSON records plus full compiler stderr; unrelated dependency-artifact metadata is omitted |
| [verification-results.json](verification-results.json) | All after/compile/control/gate command records, exits and wall times |
| [source-sha256.json](source-sha256.json) | The two test files and two product source files, checked unchanged after mutation restoration |

Original stdout/stderr filenames in the JSON map to the section labels
in the combined text files. Compilation stdout sections retain only the
two records described above. The three linked `.patch` files are the
exact Rust mutation records relative to the repository root.

## Earlier ARM64 context

Successful native ARM64 [run #5191](https://github.com/autonomous-ai/lampOS/actions/runs/39633785316),
head `0af798d348ba06f7163f7aac90b79a381263888c`, measured the enabled test
at 1.568 s and the disabled test at 1.822 s. This is before-only queue
context on a different machine and source head, separate from the
same-base Mac comparison above. It does not measure this change.
The two extracted durations are in
[arm64-before-5191.json](arm64-before-5191.json).

No native ARM64 run of the changed candidate or device execution is
included here. New CI on the published PR head is still required.
