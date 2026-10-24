# Published Actions cap-state readers

This is the narrow Actions output-cap slice of #2809, based on
`9ed44f88c573837c50451c94f4d88f010e8a6721`. It adds two explicit native
release-pair cases without changing production behavior.

**The original native pair passed.** Head `65e7b57fa` executed **2/0/0 ignored**
in official ARM64 run **40060504367**. Ordinary ARM64 **40060504303** passed
**11,163 PASS + 1 LEAK**, zero failures, 73 skipped. The native receipts remain
bound to that head. This continuation has now rebased onto `d7b85b1dd`;
fresh current-base gates are required before integration. The original
Mac host gate below ran with Rust 1.96.0. Ordinary ignored results do not
establish native execution.

| Host command | Result |
|---|---|
| `cargo +1.96.0 fmt --all --check` | Passed; 5.64 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-actions --all-targets -- -D warnings` | Passed; 5.98 s |
| `cargo +1.96.0 test --locked --offline -p lamp-actions` | 120 passed / 0 failed / 2 ignored / 0 filtered; 15.09 s |

The passing count is 65 + 3 + 29 + 23. The two new actual-release cases were
ignored. Full logs and exit codes are retained locally for publication;
the fixture and dependency files were unchanged after these gates.

| Case | Actual released processes |
|---|---|
| `release_6498_keeps_6408_caps_and_6408_reads_its_changes` | 6408 → 6498 → 6408 |
| `release_6408_keeps_6498_caps_and_6498_reads_its_changes` | 6498 → 6408 → 6498 |

Both published binaries are **identical**, with 4,517,592 bytes and SHA-256
`b5359557611b4fa3aa09709b659c7243f77c9ae945c326937f55adbc928903d6`.
Their source references are `ee53765c8d014eb00e41c1a5a7054d2546c18bdb`
(6408) and `846b3d203fc00ec3886d71f7673d8cc29cc4a634` (6498). The Actions
and underlying Schedule-store implementation files are identical at these
release sources and the integration base. A passing run would qualify these
exact published artifacts, not distinct versions of that implementation.

The [driver](../../../crates/actions/tests/release_pair/fixture.rs) directly
executes the pinned private copies. The initial release commits active and
cancelled state, the opposite release reads it and writes new state, and the
returning original reads those writes. The driver never authors the positive
store or uses today's Controller as either released reader. Every replacement
follows an observed real SIGKILL and reap. This is a cut after acknowledged
commit, not inside a transaction or fsync.

Exact RPC snapshots, private bus publications and generic read-only SQLite
inspection jointly check revision, active caps, minima and cancellation
tombstones. Cancelled retries remain cancelled; same-token changes, a
replacement with an equal issue time, wrong-token removal and an extra RPC
field leave the persisted JSON unchanged. Fresh writes after refusals prevent an unresponsive process
from passing. A strictly later same-source/new-token put replaces a tombstone
exactly once while preserving an unrelated active cap. The final revision is
10 with four cancelled source records and no active entries.

Stored compatibility equality compares exact singleton JSON BLOB bytes,
revision and original file identity/mode. Complete database copies and hashes
are retained separately. Copies use generic SQLite read-only inspection,
`quick_check`, exact singleton selection and DELETE-journal verification;
no current domain deserializer substitutes for the executable reader.

Each test has a 90-second budget, ten-second process/wait steps, two-second
socket I/O and five-second failure cleanup. Fresh issue times come from the
real clock immediately before RPC; all authored expiries are one hour later.
Crossing any expiry fails the test. Host clock synchronization, unknown clock,
expiry transitions and clock corrections are not qualified.

All children use `env_clear`, private mode-0700 directories and separate
runtime/bus namespaces. The only switch supplied is `LAMP_OUTPUT_CAPS=on`.
No mixer, provider, lamp service or actuator owner participates. Volume caps
must report unconfirmed hardware and withdraw software readiness while a
volume cap is required. Ring bus publication remains unconfirmed output.
Neither the test bounds nor publication latency claim physical lamp latency.

The dedicated workflow explicitly selects both ignored cases and retains
raw logs, JSON/SQLite snapshots, artifact identity, source hashes and cleanup
receipts. The [fixture instructions](../../../crates/actions/tests/release_pair/README.md)
give exact environment names, command and sequence. Native execution and the
ordinary ARM64/strict-Clippy gates remain separate requirements.

Based on released source, stored additive fields are tolerated but unknown
plain `status` values are refused; private tagged requests and their nested
arguments remain strict. Only the extra-field RPC refusal is executed here.
No newer-schema/unknown-enum synthetic writer, Store trust, release-signature
acceptance, updater installation, cold boot, flash durability or physical
output is claimed. No binary artifact is committed.

RPC replies now use one absolute deadline bounded by the original step/case
and two-second I/O budgets. Each read uses the remaining time; partial bytes
cannot renew it. Ordinary controls cover a complete response and a dribbling
peer. Dedicated `--ignored` execution still requires two actual artifact
cases, with the two ordinary controls filtered out. Earlier fast native
passes did not exercise this timeout boundary.
