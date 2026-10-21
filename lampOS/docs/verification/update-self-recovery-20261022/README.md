# Independent recovery before updater handover (#2700)

Second, 2026-10-22. Mac fixtures; no device run.
This revision follows [First's review](https://github.com/autonomous-ai/lampOS/pull/2709#issuecomment-6323539077)
and is based on `lamp-voice` `d904b01238a9e4a06d8804bb9df6caca7a128bce`.

Before downloading or switching an actual updater candidate, require the
fixed regular executable fallback, its loaded service in an accepted
non-runtime unit-file state, and its loaded, persistently enabled, active
timer targeting that service. The ordinary completed parts result remains
when self-update waits. The updater-first `min_updater` path leaves parts
unchanged and keeps its original refusal. Off, unchanged and refused
candidates do not query recovery; a later ready round can retry.

The query now calls the existing `world::output_within` with the five-second
`JOURNAL_WITHIN` deadline. A failed or timed-out command returns a fixed
refusal. The parser and its five tests are unchanged. The separate query
implementation, four query tests, six cut fixtures and process dev-dependency
are removed. The existing command-helper test still covers a timed-out
child, output larger than a pipe, and a failed command. This revision adds
no separate output cap or reap deadline.

The check is a snapshot of selected properties. It does not install or
start recovery, certify arbitrary overrides or the fallback architecture,
or protect against later administrator changes. The self-update opt-in is
unchanged.

## Validation

The revised Mac gate passed on the base above, with every checked source
hash unchanged across the controls and gate. [revision-gate.json](revision-gate.json)
keeps every exact result line and command. Its `base` field names the
pre-amend checkout of the task commit; the integration base is `d904b0123`
as given above.

| Mac stage | Result | Elapsed, including Cargo |
|---|---|---|
| `cargo +1.96.0 fmt --all --check` | exit 0 | 4.617 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-update --all-targets -- -D warnings` | exit 0 | 16.097 s |
| `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-update -- --test-threads=1` | 286 passed, 0 failed, 2 ignored, 0 filtered out | 214.858 s |

The two existing ignores are the manual/network tests
`fetch::tests::a_release_file_from_github` and
`records::tests::write_records_fixtures`. Tests include the nine retained
regressions and the existing shared-command helper coverage. Rust 1.96.0,
two build jobs, no incremental compilation and dev/test debug symbols off
were used, with the same shared target cache as the controls.

Only `lamp-update` has changed runtime/tests. A changed-name grep across
`crates/` finds the readiness calls, parser and shared query helper only
in that crate. No full local workspace or local ARM64 gate.

Two compiled controls were restored byte-for-byte before the final gate:

- [Timer-state control](timer-state-control.json): remove only the timer's
  persistent-enablement check in `self_recovery_show`:
  **4 passed / 1 failed / 0 ignored / 103 filtered out**, exit 101. The
  parser's disabled/runtime-enabled timer regression catches it.
- [Handover control](revision-guard-control.json): remove only the handover
  readiness call: **1 passed / 3 failed / 0 ignored /
  93 filtered out**, exit 101. Missing fallback, unavailable recovery and
  updater-first round fixtures catch it; unchanged/refused candidates pass.

The parser control runs its unit tests because the round fixtures stub the
`Systemd` result. The handover control separately proves that the real round
consults recovery. Nine new tests remain: five parser and four round tests.
An independent source read found no P1/P2 in the simplification.

[Initial evidence](initial.md), `gate.json`, `rebase.json`,
`guard-control.json`, `cleanup-control.json` and `initial-control.json` are
retained unchanged as historical receipts for the earlier implementation.
Its native green result is not evidence for this revised head. The PR's
new native run will qualify the revised head and merge separately.
