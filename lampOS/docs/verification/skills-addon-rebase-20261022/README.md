# Skills add-on rebase correction, 2026-10-22

The plain rebase of #2693 onto `74b90c317` kept the completion match for
`quiet-note`, although upstream had renamed the loaded fixture to
`quiet-say`. [ARM64 #5410](https://github.com/autonomous-ai/lampOS/actions/runs/39696094934)
failed at that wait after 10.059 s: 4,874 passed, one failed, 63 skipped;
fail-fast left 5,031 of 9,906 selected tests unrun. This was a real
integration error in this PR, not an inherited failure.

The corrected fixture uses its local add-on name both when creating the
folder and when matching `SkillStage::Done`. Its original proposal-name,
version, act, note, validity and rate-limit assertions are unchanged. The
500 ms proposal and 100 ms event absence drains, 10 s proposal limit,
and disabled-case startup acknowledgement are unchanged. Product source
is unchanged. The current documentation names `quiet-say`; the
[earlier evidence](../skills-addon-completion-20261021/README.md) remains
historical on its stated base.

## Reproduction and controls

On this same base, the exact stale-name fixture compiled and failed once
at `quiet-note completion after publication 1` (11.5854 s process wall).
The corrected enabled and disabled cases each passed three precompiled
invocations. Enabled: 2.3785 / 0.6957 / 0.6896 s. Disabled:
0.6458 / 0.6464 / 0.6398 s. These include process startup, execution and
cleanup, excluding compilation. The first enabled invocation remains over
one second; no new baseline speedup is claimed for this correctness fix.

All three refreshed production controls compiled and failed the intended
original assertion: bypassing proposal limits produced 3 instead of 1;
ignoring the off filter with a 500 ms slow startup loaded the forbidden
add-on; suppressing Done hit the `quiet-say` completion watchdog. Exact
commands and all four failed outputs are in [failures.txt](failures.txt).
The three `mutant-*.patch` files apply independently to this candidate;
restore each before the next. `fixture-correction.patch` records the fix
relative to the rebased failing fixture. All five final source hashes
match the frozen pre-control values in [results.json](results.json).

## Local checks and incomplete broad attempt

Native macOS x86_64, Rust 1.96.0; target directory
`/private/tmp/tests-codex-1569-target`, four build jobs, debug information
and incremental compilation disabled, offline. No cross-target or test
thread override was set. All tracked crate/Cargo/toolchain mtimes were
refreshed before Cargo, with unchanged content hashes.

Passed sequentially after restoring all controls:

- `cargo fmt --all --check` (10.4157 s).
- Strict all-target Clippy for the twelve-package dependency closure
  (80.3547 s; the exact command is in `results.json`).
- `cargo test -q -p lamp-skills`: 137 library + 19 binary + 261 integration
  tests, **417 passed, zero failed or ignored** (71.1157 s).
- `cargo build -q --workspace` (80.2456 s) and `git diff --check`.

Between Clippy and the final Skills suite, an optional twelve-package host
suite hit the external runner's 600 s wall limit. It is **not a passed
check**. That runner did not retain its buffered partial output, so no
counts or test-failure cause are claimed. Afterwards, process inspection
found no remaining executable under the owned Cargo target. The required
local checks above cover the changed test module and whole-workspace
build; full workspace tests still require native Linux ARM64 CI on the
published head. This folder contains no new ARM64 result.

The initial metadata-only preflight also stopped because the runner's
no-reverse-dependency assumption was false; it was corrected before any
test ran. This is recorded separately from test outcomes. Full passing
output and runners remain external with hashes, not durable repo links.
Only evidence documentation was added after the final source checks.
