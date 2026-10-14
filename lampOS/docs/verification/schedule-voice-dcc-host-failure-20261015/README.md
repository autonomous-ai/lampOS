# Task 31 host failure checkpoint for the Third log

**Not ready.** The completed dcc Task 31 gate failed: 1,111 passed, 3 failed,
8 ignored in 16 completed harnesses. Test command exit 101 after
658.218369088 s; Voice lib reported 918 passed, 3 failed and 8 ignored in
491.25 s. Formatting and strict all-target Clippy passed. The later workspace
build stage was not reached. No dcc source-only baseline or implementation
comparison has run. This folder records failures and diagnostic limits; it is
not a ready-for-merge or lamp qualification report.

All execution evidence here came from the Third development Mac:
Intel Core i7-10700K 3.80 GHz, 16 logical CPUs, 64 GiB RAM, macOS 26.6.2 /
Darwin 25.6.0, x86_64, Rust/Cargo 1.96.0. [machine.json](machine.json) retains
the original machine record. No lamp, live provider, microphone, speaker or
physical motor result is claimed. Build/test elapsed times are not UX latency.

## Failed dcc gate

The exact base is `dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`.
[gate/source.json](gate/source.json) contains all 675 frozen Rust/manifest
hashes for this attempt. [gate/results.json](gate/results.json) records exact
argv, elapsed time, exit status, source checks and every ignored-test reason.
All three stages reported source unchanged during execution.

Commands ran sequentially from `/Users/d/code/autonomous-os/lampOS`, with
`RUST_BACKTRACE=1`:

```sh
cargo fmt --all --check
cargo clippy --offline --locked -p lamp-actions -p lamp-schedule -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings
cargo test --offline --locked -p lamp-actions -p lamp-schedule -p lamp-voice -p lamp-voice-eval -- --test-threads=1
```

The format stage took 3.833277004 s and Clippy 34.057569004 s. Their
[format output](gate/format.txt), [Clippy output](gate/clippy.txt) and
[complete failed test output](gate/tests.txt) are unedited. An empty format
file alone is not the pass evidence; the recorded exit code is.

All failures were under `coordinator::motor::tests`, at the shared
`motor/tests.rs:441` assertion, `owned child timed out: None`:

| Failed case | Backtrace wait call |
| --- | --- |
| `crash_and_stalled_control_never_allow_replacement_or_false_cleanup` | `motor/tests.rs:645` |
| `late_started_callback_cannot_restore_an_offer_after_owner_replacement` | `motor/tests.rs:865` |
| `native_head::native_head_initial_privacy_acquisition_is_not_a_revoke_or_grant` | `motor/tests/native_head.rs:491` |

The preceding incoming-provider contract review is retained exactly in
[merge-review-dcc.md](merge-review-dcc.md), with [rebase-dcc.json](rebase-dcc.json).
The rebase record preserves the original stash-apply exit 1; the frozen map
identifies the integrated source actually tested. Separately, later source
review found a real Task 31 pending-camera/SecondLook priority defect. Its
correction changes source identity and requires fresh qualification. No
pre-fix failing transcript for that source-review finding is fabricated here.
These failed gate hashes must not be relabeled as the corrected source.

## Earlier clean-7f failures and separate diagnostics

The earlier clean integration source was
`7fbb03e1f2e61ad6dfaf184cd768bad6524c2d76`, without Task 31 changes.
[Its original metadata](clean7f-original-failures/clean-7f.json) and
[raw output](clean7f-original-failures/clean-7f.txt) retain 25 passed / 3 failed /
0 ignored (41.22 s tests; 80.425533549 s command). The command was:

```sh
cargo test --offline --locked -p lamp-voice --lib coordinator::motor::tests:: -- --test-threads=1 --nocapture
```

It ran in `/private/tmp/lamp-third-schedule-7fbb03e1f-baseline-kg0lzf0c/lampOS`
with `RUST_BACKTRACE=1` and the shared target directory recorded in the JSON.
The original Task 31 7f run is also retained as
[metadata](clean7f-original-failures/task31-7f.json) and
[output](clean7f-original-failures/task31-7f.txt): 27 passed / 1 failed /
0 ignored. [Binary references](clean7f-original-failures/binary-references.json)
explicitly limit those original hashes to captures after the failed runs;
they are not invented execution-time identities.

Later diagnostic instrumentation in a separate clean 7f source copy passed
three exact cases and one 28-test module invocation. No earlier timeout was
reproduced; its cause remains unproven. No production code, deadline or
assertion changed. These passes do not erase any failed gate or qualify dcc.

- [Diagnostic report](clean7f-diagnostics/report.md) and
  [compact command/result/source summary](clean7f-diagnostics/run-summary.json).
- [Exact-case results](clean7f-diagnostics/exact-cases/results.json) and
  [module results](clean7f-diagnostics/motor-module/results.json), with the
  three individual raw test outputs and module output in the same folders.
- [Clean source map](clean7f-diagnostics/source/source-clean.json),
  [instrumented source map](clean7f-diagnostics/source/source-instrumented.json),
  and [test-only instrumentation diff](clean7f-diagnostics/source/instrumentation.diff).
  Only `crates/voice/src/coordinator/motor/tests.rs` changed among 1,436 files.
- [Derived phase analysis](clean7f-diagnostics/analysis.json), whose raw sample
  dependencies are deliberately omitted from this compact publication copy.

The compact summary records the exact compiler command, binary hash, per-case
argv, working directories, explicit environment and unchanged-source checks.
Passing phase observations suggest a substantial interval before inert-child
entry but do not establish an OS/loader cause for the earlier failures.
Instrumentation changes execution timing; no failing instrumented trace was
obtained. Do not call these failures a known flake or relax a deadline based
on this evidence.

## Compact retention and pending full publication

All copied source records, logs, reports and diffs are exact original bytes.
[files.json](files.json) records their original paths, retained paths, lengths
and SHA-256. [omitted-full-diagnostic-files.json](omitted-full-diagnostic-files.json)
identifies omitted full-bundle payloads by path, length and hash. This avoids
copying the 88 raw wait samples and repeated diagnostic source snapshots into
the Third log branch a second time. Absolute scratch paths inside original
reports remain provenance, not requirements for reading the copied outputs.

Full diagnostic raw evidence already exists locally in the main task's
verification folder. Its publication is **pending**: [future full diagnostic
bundle](https://github.com/autonomous-ai/lampOS/blob/third/schedule-voice/lampOS/docs/verification/schedule-voice-host-20261014/motor-diagnostics-clean7f/README.md). This link is a future location, not a claim that the
branch or folder has been published. The full local bundle has 127 text files
and 8,991,986 bytes; its earlier README and manifests retain their exact scope.

This compact folder was assembled using filesystem reads and copies only.
No Cargo, test, benchmark, provider, source-copy script, runtime edit, commit,
push or device action ran during assembly. No executable, archive, Python
source, bytecode or credentials are included.
