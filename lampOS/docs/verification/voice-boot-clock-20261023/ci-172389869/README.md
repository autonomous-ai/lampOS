# PR #3190 CI evidence — 23 October 2026

Both required workflows and their actual jobs completed successfully on merge `286466bc4bb6922a1ec2f50282f35118b194f327`. Its parents are base `70cbefceee62c0bc4cad4ec0b7054335dd8f0a5d` and PR head `172389869129a7466623cc9b45d1c536f42f7429`, in that order. The merge’s complete Git tree `75c7bd43a9d78a9f16dc1b63987c7b21b009d876` equals the PR head; `source-binding.json` records the lampOS/CI subtrees, eight changed file identities and exact configuration snapshots. This qualifies that merge, not a later parent.

[Clippy run 39891902443](https://github.com/autonomous-ai/lampOS/actions/runs/39891902443), actual job 120213379860: `cargo clippy --locked --workspace --all-targets -- -D warnings` succeeded with Rust 1.96.0 x86 Linux. The Clippy step took 68s (Cargo 1m06s). It executes no runtime tests.

[Linux ARM64 run 39891902416](https://github.com/autonomous-ai/lampOS/actions/runs/39891902416), actual job 120213379835: completed 03:12:33Z after 20m24s; root observed terminal success at 03:22:48Z. Rust 1.96.0 aarch64 Linux, nextest 0.9.148, four cores. The PR package filter selected 12 of 46 crates. The effective `pr` profile includes the realtime group despite stale workflow comments.

Nextest ran 7,122 tests: 7,122 passed, including one leaky pass; 31 tests and 106 binaries were skipped. Its test phase lasted 141.230s after 11m45s compilation (combined step 847s). Startup reports 62 binaries; individual terminal rows identify 58. All ordinals 1–7,122 are present, with 34 identical final-output repetitions counted once. The console does not enumerate skipped names/reasons or the four starting binaries without terminal rows; those identities are not invented. This is selected-package coverage, not every workspace test.

The leaky pass is `lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`, 0.224s, ordinal 5004 (raw line 5710). There are no FAIL/TIMEOUT/ABORT terminal rows. All 86 authored clock-fix cases have PASS evidence: 85 literal-name matches plus one visible prefix `coordinator::tests::hand_controls::a_hand_kept_on_the_head_si***` (line 3942, 0.034s) uniquely matches the expected sigh/held-head function in exact-head source. The full name is absent from the log; that source-based attribution is explicit in `arm64/masked-name-matches.json`.

The separate step passed 4 doctests across 46 doc targets, printed `relay_signals: ok` (no numeric case count), and passed 12 process-harness cases. These are separate from nextest counts. Publication helper regressions reported 35, 12 and 7 passed cases. Release workspace build, example builds, classification, export and upload succeeded; all step durations are retained.

The slowest binaries below use sums of individual nextest case durations, excluding compilation/shared startup and duplicate output. Parallel case times overlap, so these are not binary wall times or before/after speedup measurements.

| Binary | Cases | Summed case seconds |
|---|---:|---:|
| `lamp-voice` | 3595 | 240.712 |
| `lamp-skills::it` | 304 | 97.883 |
| `lamp-voice::it` | 301 | 49.313 |
| `lamp-cloud` | 166 | 37.048 |
| `lamp-voice-eval::it` | 238 | 19.562 |

The slowest non-Voice binary was `lamp-skills::it`. Its five longest cases were:

| Case | Seconds |
|---|---:|
| `night_sleep::a_lamp_nobody_saw_all_day_keeps_its_head_down_into_the_next_night` | 5.216 |
| `addon::only_a_say_kept_by_the_policy_is_asked_of_the_voice_quoted` | 4.763 |
| `skills::asking_the_policy_on_the_hosts_clock_gives_up_after_its_wait` | 4.518 |
| `skills::guard_mode_from_the_app_is_the_owners_alone_and_the_switch_closed_ends_it` | 4.293 |
| `skills::a_reading_pose_is_held_on_the_mood_socket_for_its_for_and_then_let_go` | 3.622 |

The artifact upload reports 47 workspace binaries, 2 selected examples and 50 files, with ZIP SHA-256 `01e9c35583bc579bb5a9a57c1ef5dfb674ae02896c0a42ad7d1c45cc36fcc88f` (114,871,456 bytes; seven-day retention). The artifact was not downloaded or independently rehashed. Both workflows restored cached output/source times; checkout identity is not a cold-build claim.

Each `job.log.json` is a lossless UTF-8 wrapper around the complete downloaded console stream, including its original byte count and SHA-256; the raw logs remain externally alongside this publication bundle. No bytes are silently normalized. Metadata, all selected terminal rows, 86-case census, separate harness census, provenance and duration tables are retained. `manifest.json` hashes the publication files except itself. External parser/finalizer utilities and binary artifacts are not publication inputs. No local ARM build, workflow rerun or new test execution was performed.

Published API JSON files are explicit minimal provenance projections: commit/tree/parents/times, workflow and job identities/status/steps, and PR head/base/title. Person attribution is omitted. `metadata-projections.json` retains each complete external response’s original byte length and SHA-256. The decoded full logs were scanned against the human names, handles and emails in those responses; no matching person attribution was found, so both published log strings remain byte-identical to their original streams with no redactions. The repository organization and GitHub service account are not classified as people. `attribution-scan.json` records the method and limits.
