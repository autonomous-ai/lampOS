# Rested state: corrected native qualification

This receipt covers [PR #3327](https://github.com/autonomous-ai/lampOS/pull/3327) head `8bc821736be09d62bd27f635f054350fca85a1d0` on base `9ed44f88c573837c50451c94f4d88f010e8a6721`. All three checkout logs identify merge `9ca22aff7e09b3ca4937e9a23a5b9dceea1aabb6`. This record does not qualify a later rebase.

| Run | Result |
|---|---|
| [Dedicated published readers 40057211166](https://github.com/autonomous-ai/lampOS/actions/runs/40057211166) | 6 passed, 0 failed, 0 ignored, 0 filtered; 8.93 seconds |
| [Ordinary ARM64 40057211171](https://github.com/autonomous-ai/lampOS/actions/runs/40057211171) | 11,164 passed, 0 failed, 77 skipped; 199.891 seconds |
| [Strict Linux Clippy 40057211165](https://github.com/autonomous-ai/lampOS/actions/runs/40057211165) | Workspace and every target passed with warnings as errors |

The ordinary total includes **11,163 PASS rows plus one LEAK-marked pass**, `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. It is not an additional clean pass. Four doctests, the separate 12-case process harness, the relay command and publisher/artifact/CLI suites of 35/12/7 also passed. Skipped names were not emitted. Nested captured output is not counted again. The ordinary run packaged 47 binaries and two examples; its artifact payload was not downloaded. Certificate setup, Node deprecation and Cargo example filename-collision notices remain distinct from the clean Rust Clippy diagnostics.

The native fixture ran `cargo test --locked --release -p lamp-skills --test release_pair -- --ignored --nocapture --test-threads=1` on Linux aarch64. The four existing daily/on/off cases and both rested directions executed. Actual published Skills 6408 and 6498 processes perform all live writes and reads; the harness supplies only synthetic definitions and peers. The fixed executable pins are retained in each case's `case.json` and checked before execution and again afterward.

## Rested continuity and counterexamples

Both 6408 → 6498 → 6408 and 6498 → 6408 → 6498 completed. Each A process refused a stale receipt and a fresh wrong-episode receipt, emitted the matching Cancels, then accepted the exact fresh receipt and wrote `rested-check 20749`. Same-episode renewals followed after 1,001,652 and 1,002,781 microseconds. Every request identity and sequence was checked by the source-bound fixture.

B consumed the unchanged night record without restoring active ownership; its no-owner Cancel probe succeeded without a rest request. The morning produced exactly one private `{"gesture":"stretching"}` datagram in each direction. A synthetic Body Over was delivered after that datagram and before the actual terminal Done. The returning A refused the spent morning, retained identical bytes, and passed stale/fresh readiness probes without emitting a rest request. All other private output traps stayed empty under the fixture assertions.

The manifests remain byte-identical across all replacements. Raw daily schema-2 bitmaps contain exactly the three named night claims on day 20749, then the morning claim on 20750. `rested` retains SHA-256 `ca9a8a44196f47452fad3060f0aaa14c2528d0c7c0262bc963e2c80b36b26aef`; B's morning daily file and returning A's unchanged file retain `3f2bae542fe279a87e61e9b9dd238fd8f2f1f2e26d669257954cf486447159c0`. Both rested cases contain exact owned SIGKILL/reap receipts for A, B and returning A: six reaps, not a global process inventory. Detailed checks are in [native-audit.json](native-audit.json).

## Source binding and retained evidence

All **6,974 authored lampOS file paths and SHA-256s** were independently read through `git cat-file --batch` (5,863 distinct blobs). Every path and digest matches both native source manifests, with zero missing, extra or changed files. Both captured manifests and the independently generated authored manifest have SHA-256 `9470dbd00b9d627ac2b24222b865dbafe4a5725f3a19ed2eb9f01160998f14fc`. [source-binding.json](source-binding.json) records the method and result.

Regular UTF-8 case proof and selected provenance are under `pair/`. [evidence-map.json](evidence-map.json) binds each byte-identical copy to its source, size and SHA-256. The complete `skills-tests.log` is renamed `skills-tests.txt` without changing bytes. No log line was redacted. [excluded-case-files.json](excluded-case-files.json) lists omitted bus transport storage and non-text payloads. No executable, archive, Python helper, credential or identity-bearing API object is included. The downloaded retained tar's digest is recorded in [ci.json](ci.json); its artifact API ZIP metadata was not separately requested. Local scoped gate evidence and exact-head offline Linux formatting/owned-container cleanup are retained separately under `local/`.

The [earlier collision failure](../rested-state-failed-20261024/README.md) remains separate. Its A-only reaps and absent B startup are not relabelled as this successful continuation.

This qualifies only the exact published pair's synthetic daily/on/off/rested persistence. Fixed offsets map consecutive local days; they do not simulate an elapsed physical night, real clock synchronization or timezone migration. Synthetic Expression receipts and Body Over prove protocol sequencing, not real owner behavior, motion or physical completion. No speech/provider, signed Store trust, updater installation, mid-write cut or flash/power-loss durability is claimed. Published pins provide artifact consistency; this audit did not re-download release executables or verify them against an independently trusted signing key.
