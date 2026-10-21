# Historical initial implementation

This is the initial evidence for `52b2f5f3dc21dabda8f043650295dd8984ccb657`.
The separate query reader, its four tests, six cut fixtures and process
dev-dependency were removed after First's review. Their results below
remain historical; [README.md](README.md) describes the revised source.
The initial [native result](https://github.com/autonomous-ai/lampOS/blob/2d9b18d7f/lampOS/docs/verification/update-self-recovery-native-20261022/README.md)
qualifies that initial head only.

---

# Independent recovery before updater handover (#2700)

Second, 2026-10-22, Mac fixtures; no device run.
Mac gate base: `66518bb4c803a8a3439cc3ff2d0743bf460ba92e` on `lamp-voice`.
The single commit was then rebased cleanly onto
`849c0829fd29837bc28fd8b44341e361550e7918`: range-diff `=`, all six Rust
source hashes and the crate manifest unchanged, and post-rebase formatting
passed ([rebase.json](rebase.json)). The upstream changes include other
crates, so the PR's native merge run qualifies that integration separately;
the local test counts below are not represented as a second test run.

## Behavior

An actual self-update candidate now requires the installed, regular executable
`/usr/local/lib/lampos/lamp-update-back`, a loaded recovery service in an
accepted non-runtime unit-file state, and a loaded, persistently enabled,
active timer targeting that service. An inactive oneshot service is normal.
The query changes no systemd state. It checks a five-second deadline, limits
output to 64 KiB, and allows up to 250 ms to reap the direct child on failure.

A refusal happens before candidate download, pending state or link replacement.
Ordinary updates retain their completed parts outcome; the updater-first
`min_updater` path retains its original refusal without changing parts. A later
round may retry the same candidate. Off, unchanged and previously refused
candidates skip the check.

This is a snapshot of selected properties, not a check of the service
fragment's storage location, arbitrary unit overrides, the executable's
architecture or an administrator's later changes. It neither installs recovery
nor enables or starts its timer. The existing self-update opt-in is unchanged.

## Coverage

- Four integration tests cover absent, non-executable, directory and symlink
  fallback files; inactive or failed recovery queries; a successful later retry;
  the updater-first refusal; and unchanged/refused candidates with and without
  a fallback. The existing off-switch test also checks that no query occurs.
- Parser and real local child-process tests cover the ready timer/inactive
  service, rejected timer/service states, missing/duplicate/malformed records,
  failed queries, oversized/non-UTF-8 output, timeout cleanup and spawn failure.
- Six filesystem fixtures cover a synced candidate before pending state, saved
  pending state before a failed link move, a moved link before confirmation,
  cleared confirmation, rollback interrupted after link restoration, and
  refusal saved before pending cleanup. The last boundary is explicitly
  reconstructed with the real durable record/link helpers; the others use
  existing callbacks or failed filesystem replacements in the actual path.
  They do not simulate physical power loss, flash behavior or a device reboot.

## Validation

All three final stages exited 0 on the base above, with all source hashes
unchanged. [gate.json](gate.json) preserves every exact result line and command.

| Mac stage | Result | Elapsed, including Cargo |
|---|---|---|
| `cargo +1.96.0 fmt --all --check` | exit 0 | 4.719 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-update --all-targets -- -D warnings` | exit 0 | 16.727 s |
| `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-update -- --test-threads=1` | 296 passed, 0 failed, 2 ignored, 0 filtered out | 233.934 s |

The two existing ignores are `fetch::tests::a_release_file_from_github` and
`records::tests::write_records_fixtures` (manual fixture generation). The final
suite includes all 19 new tests, existing successful and failed self-proof
coverage, and the existing updater-first cases.

Before that gate, two compiled mutation controls proved the new assertions:

- [guard-control.json](guard-control.json): remove only the handover readiness
  call, run `cargo +1.96.0 test --locked --offline -p lamp-update --test round
  self_recovery:: -- --test-threads=1`. Exit 101; **1 passed, 3 failed, 0 ignored,
  99 filtered out**. Missing fallback, unavailable recovery and updater-first
  refusal each failed; the unchanged/refused-candidate test still passed.
- [cleanup-control.json](cleanup-control.json): remove only production query
  failure cleanup, run the exact timeout unit test. Exit 101; **0 passed,
  1 failed, 0 ignored, 111 filtered out**. The independent `waitpid` observation
  found `Ok(None)` (child still running); the fixture killed and reaped its
  own child before failing. Its one-second startup window fails explicitly if
  the child never records its PID. The fixture's cleanup is bounded by two
  seconds; the production cleanup remains 250 ms.

Each temporary mutation was restored byte-for-byte before the next command.
[initial-control.json](initial-control.json) preserves the earlier guard control
before service-state and timeout-witness review fixes. It has the same
1/3/0/99 result and is not the final-source control. These expected failures
are kept separately from the passing gate.

The gate used Rust 1.96.0, two build jobs, no incremental compilation, debug
symbols disabled for dev/test profiles, and a separate shared target cache.
A private orchestration script ran commands sequentially, saved their complete
logs, copied every `test result:` line into the receipts, summed their counts,
and verified the six changed Rust source files and crate manifest were unchanged across each
run. Only receipts and log hashes are committed; local machine paths are not.

`rg` across `crates/` found the changed readiness names and the updater's
`Systemd` implementations only in `lamp-update`; Keeper's same-named trait is
separate. No full local workspace gate, local ARM64 build or device action was
run. Linux ARM64 qualification belongs to the PR workflow and is recorded
separately after it finishes.
