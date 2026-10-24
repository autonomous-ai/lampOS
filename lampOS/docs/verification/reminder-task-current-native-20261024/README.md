# Reminder and Task state: current native qualification

This receipt qualifies [PR #3312](https://github.com/autonomous-ai/lampOS/pull/3312) head `4ba9d959ff8e3da495e699dc9edf6cdbf6e6d528` on base `9ed44f88c573837c50451c94f4d88f010e8a6721`. All three checkout logs identify merge `ee65e8fcd7c61b6d2298d27fbf0771fca67696e4`; its API parents are exactly that base and head. No later revision is qualified here.

| Run | Result |
|---|---|
| [Dedicated released-binary cases 40057006999](https://github.com/autonomous-ai/lampOS/actions/runs/40057006999) | Schedule 4 passed in 11.87 seconds, Tasks 2 in 4.23 seconds, Voice 6 in 14.72 seconds; zero failed, ignored or filtered |
| [Ordinary ARM64 40057006981](https://github.com/autonomous-ai/lampOS/actions/runs/40057006981) | 11,164 passed, 0 failed, 83 skipped; 201.221 seconds |
| [Strict x86 Clippy 40057007027](https://github.com/autonomous-ai/lampOS/actions/runs/40057007027) | Passed across every workspace crate and target; no Rust warning/error diagnostic |

The ordinary total consists of **11,163 PASS rows and one LEAK-marked pass**, `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. The leak is included in 11,164, not an extra clean pass. Four doctests, the separate 12-case process harness, the relay check and Python publisher/artifact/real-CLI suites of 35/12/7 passed. Captured nested output after the nextest summary is not counted again. Skipped names were not emitted. Publication was skipped; no release was made by these PR runs.

The dedicated tests ran on Linux aarch64 with Rust 1.96.0. For each of `schedule`, `tasks` and `voice`, the command was:

```text
cargo test --locked --release -p lamp-PART --test release_pair -- --ignored --nocapture --test-threads=1
```

They execute the pinned published ARM64 releases 6408 (`ee53765c8d014eb00e41c1a5a7054d2546c18bdb`) and 6498 (`846b3d203fc00ec3886d71f7673d8cc29cc4a634`). Exact executable hashes and sizes are recorded in `ci.json`, captured before/after metadata and the case transcripts. Schedule's two payloads are byte-identical; this is not a Schedule implementation-delta qualification.

The 12 top-level tests contain:

- Schedule: eight direction/boot/receipt scenarios, 32 successful waited process exits and 32 stopped SQLite snapshots checked immutable/read-only. Store identity, pending and snoozed entries, completed-item absence, receipt replay and opposite writes persist. Stage 2 deliberately starts without wall-clock trust; same-boot monotonic timing remains usable, while a changed synthetic boot waits for trust. No physical reboot occurs.
- Tasks: six schema-1/2/3 direction cases. Each explicitly records SIGKILL and reaping of Tasks and its blocked owned backend; all 18 JSON snapshot hashes were checked. Accepted launch/observation data persists, claimed speech becomes uncertain, attempted work becomes uncertain, queued/prepared work is invalidated and opposite writes reopen. Target/name companions are checked by the fixture. Completion remains `unverified`; Harness and Voice are synthetic. The 12 explicit cut reaps do not independently enumerate every transient subprocess.
- Voice: six unfinished/completed/ack-intent direction cases, 18 matching PID/stage/release SIGKILL/reap records, and 18 verified ledger-file hashes. Each cut has its expected persisted prefix, commits exactly one acknowledgement and leaves an empty ledger after opposite and return reopening. The consumer is the released executable; Schedule API and completion events are authored. No physical speech/audio is tested.

All **6,977 captured source files** match the authored Git tree before and after execution. `git ls-tree -rz HEAD:lampOS` enumerated every path; one `git cat-file --batch` read all 5,866 distinct blobs (184,542,457 bytes). Comparing every SHA-256 gave zero missing, extra or mismatched files. The authored and tested root tree is `d2c1b1ac6633cde9ddc5b505e188935df43320eb`. `ci.json` binds the captured source-manifest digest and independently generated authored-map digest `bffd496ed41d2335059ee28068f703cc56e5c1fc733db1e3902e6a6a31f9043c`.

The dedicated evidence ZIP, artifact `12545139282`, matched GitHub's size and SHA-256: **704,862 bytes**, `c215d652bf0891bb9a51c2dba2f28ab91953b18907d448771937f7e9f0e44468`. Its 472 regular archive members were extracted with path/type checks. Safe regular UTF-8 case files, including state JSON, requests and manifests, are copied byte-for-byte under `pair/`. The three dedicated `.log` files become `.txt` without changing bytes. `evidence-map.json` binds every copy and `excluded-case-files.json` identifies each omitted binary database/storage file. No executable, archive, Python helper or identity-bearing API object is included.

The dedicated build recovered from 58 transient Cargo download warnings; those and all certificate/Node warnings are retained in `ci.json`. Ordinary ARM64 also reported existing Cargo example filename collisions. Its build packaged 47 binaries plus two selected examples, but only artifact metadata was checked here, not that executable payload.

No actual provider, updater installation, device reboot, physical power-loss durability or physical output acceptance is claimed. This audit checked the retained release pins and full evidence payload; it did not independently fetch the release executables again. The [earlier failed attempts](https://github.com/autonomous-ai/lampOS/tree/993fc3530/lampOS/docs/verification/reminder-task-release-pair-failed-20261024) and [successful prior-head proof](../published-state-pairs-passed-20261024/README.md) remain separate evidence.
