# Published Actions cap-state native evidence

This qualifies the prior #3334 head `65e7b57fa785acc37196d4b5a110f576d3741926`
on base `9ed44f88c573837c50451c94f4d88f010e8a6721`, before its planned
integration carry. It does **not** qualify a subsequent rebase. The tested
merge is `cf09de5d5be29f7cf3917249e55d3a04a394ec92`; GitHub's commit record
confirms those exact parents and the authored root tree.

The [dedicated native run](https://github.com/autonomous-ai/lampOS/actions/runs/40060504367)
executed **2 passed / 0 failed / 0 ignored / 0 filtered** in **0.58 seconds**
on Linux AArch64 with Rust 1.96.0. Both published Actions executables are
byte-identical: 4,517,592 bytes, SHA-256
`b5359557611b4fa3aa09709b659c7243f77c9ae945c326937f55adbc928903d6`.
The named shipped pair is qualified; no implementation difference is implied.

| Case | Released process sequence | Actual owned exits | Final state |
|---|---|---|---|
| [Forward transcript](pair/cp-gCUaeY/transcript.jsonl) | 6408 → 6498 → 6408 | 3 SIGKILL exits, all reaped | Revision 10, four cancelled sources |
| [Reverse transcript](pair/cp-D59VrG/transcript.jsonl) | 6498 → 6408 → 6498 | 3 SIGKILL exits, all reaped | Revision 10, four cancelled sources |

Each case has 33 actual RPCs: 10 mutations, four accepted exact no-ops,
six refusals and 13 exact `caps_get` snapshots. Across both cases, all six
children were matched to their spawns and reaped with signal 9 before their
replacement started. No failure-cleanup or unexpected-exit record exists.
The retained identities bind direct private executable paths, empty inherited
environments plus `LAMP_OUTPUT_CAPS=on`, and live `/proc` executable hashes.

The six refusals per case are one changed same-token request, two stale
requests, two wrong remove tokens and one extra-field request. The first
stale control uses the same issue timestamp with another token; the later
control retries the displaced older token. Refusals and accepted exact
retries preserve the exact singleton JSON BLOB, revision, mode, inode and
device. Cancelled tokens remain cancelled across both returns. A fresh
strictly later replacement and fresh sources are accepted while unrelated
caps remain intact; an unresponsive or empty fallback cannot pass.

All 58 captured SQLite copies and both final live database files passed
independent generic read-only SQL checks, including `quick_check`, DELETE
journal mode and the exact singleton BLOB. Every BLOB matches its retained
JSON and recorded hash. The final state is revision 10 with four tombstones.
The audit replays all recorded RPC outcomes against the snapshots without
using the current Actions Controller as a reader. SQLite container bytes are
hashed separately; continuity compares BLOB bytes and original file identity.

Every one of the **6,971 tracked lampOS source files** captured before and
after execution matches the exact authored Git blob, with zero missing,
extra or changed paths. One `git cat-file --batch` read hashed 5,860 distinct
blobs (184,483,821 bytes); the workflow's before/after maps are byte-identical.
The full-map digests and extraction method are in [ci.json](ci.json).

The [ordinary ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/40060504303)
on that same head and merge reports **11,164 passed / 0 failed / 73 skipped**
in **196.266 seconds**: **11,163 PASS rows plus one LEAK-marked pass**.
The leak is `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`;
captured nested test output is not counted again. Four doctests, 12 separate
process-harness cases, the relay harness and Python suites of 35, 12 and 7
tests passed. The build packaged 47 binaries and two selected examples;
its artifact payload was not downloaded for this audit. Publication was
skipped. Skipped test names were not emitted. X86 Clippy is not included in
this receipt and must be qualified separately.

Five dedicated and 14 ordinary workflow warning rows are retained in
`ci.json`: certificate/Node infrastructure warnings and ordinary example-name
collisions. No failed test result was discarded. The original case output is
retained byte-identically as [actions-tests.txt](pair/actions-tests.txt).

The dedicated artifact ZIP was independently downloaded and its API size
and digest checked: **613,476 bytes**, SHA-256
`352c612f6509536df78fdc10196282895533a8f002a8ba57d0cefa2853133fc5`.
Its tar payload matches the separate download: SHA-256
`691f89ef2efa4131e169294bfb3e798aabe4c7673afbd830ccf1e0f061b48244`.
All 208 archive members were safe regular files. This folder publishes 146
UTF-8 files byte-identically; one `.log` suffix becomes `.txt` without changing
bytes. [evidence-map.json](evidence-map.json) binds every copied file.
[excluded-evidence.json](excluded-evidence.json) lists all 60 binary SQLite
files and two complete source maps omitted from publication, with hashes.
Raw API objects, full workflow logs, archives and audit scripts stay private.

```sh
cargo test --locked --release -p lamp-actions --test release_pair -- --ignored --nocapture --test-threads=1
```

The fixture uses private runtime/bus/store paths and no mixer, output owner,
provider or device. Durable acceptance and bus publication explicitly remain
unconfirmed output; readiness is absent while a volume cap requires a mixer.
The cuts happen after acknowledged commits. No physical power cut, flash
durability, expiry, unknown clock, newer schema, unknown enum writer,
signature trust or updater installation is established. The observed RPC
timings are local process boundaries, not hardware latency. See the
[exact-head fixture scope](https://github.com/autonomous-ai/lampOS/blob/65e7b57fa785acc37196d4b5a110f576d3741926/lampOS/crates/actions/tests/release_pair/README.md).
