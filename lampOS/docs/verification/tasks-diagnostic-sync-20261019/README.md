# Tasks diagnostic synchronization, 2026-10-19

For #1569. The setup-later fixture restores sign-in and corrupts the targets
file independently. A diagnostic can legitimately contain `targets_invalid`
while retaining the prior signed-out observation; the runtime gives sign-in
priority. Waiting for the file observation alone makes the exact diagnostic
assertion depend on timing.

The wait now requires both invalid targets and `signed_in=yes`. Its existing
level, exact message and retained-target assertions remain unchanged. There
is no production change, added sleep, ignore or increased timeout.

The first failure was found in the full Bedtime preparation on `08d9177cb4`,
whose entire Tasks tree matched base `5aff095719`. Three unchanged focused
retries passed 1/0/0 each (24 filtered), consistent with the asynchronous
ordering. Per the team request, this correction is a separate test-only PR.

On base `b919291458`, [the complete local gate](gates.json) passed:

| Command | Result | Elapsed |
|---|---|---|
| `cargo fmt --all --check` | Passed | 3.66 s |
| `cargo clippy -p lamp-tasks --all-targets -- -D warnings` | Passed | 13.33 s |
| `cargo test -p lamp-tasks -- --test-threads=1` | 179 passed, 0 failed, 0 ignored, none filtered | 39.94 s |

The identifier scan (`tasks_diagnostic_until` and the failing test name)
finds only `crates/tasks/tests/runtime.rs`; no API or reverse consumer changes.
All source hashes matched after the gate. Native ARM64 qualifies the published
PR separately. These are local fake-service checks, with no lamp or cloud use.
