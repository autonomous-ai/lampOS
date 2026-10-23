# Rebased PR #3190 CI evidence — 23 October 2026

Both required workflows and their actual jobs completed successfully on merge `8843f45cab0b21c3035d1707199a7dc46852249a`. Its parents are base `e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc` then own head `0f2e6439e018680af247080983c9ec0c258c3229`. The complete merge tree `ae902f07d55fec0816543294d866e6ab37043c74` equals the own head. The eight changed source identities, exact configuration snapshots and subtree binding are retained. This receipt qualifies that merge; the previous-head receipt remains separate.

[Clippy run 39895141878](https://github.com/autonomous-ai/lampOS/actions/runs/39895141878), actual job 120223719617: `cargo clippy --locked --workspace --all-targets -- -D warnings` succeeded with Rust 1.96.0 x86 Linux. The strict Clippy step took 90 s (Cargo 1m 27s); it executes no runtime tests.

[Linux ARM64 run 39895142741](https://github.com/autonomous-ai/lampOS/actions/runs/39895142741), actual job 120223721854: started 03:27:53Z, completed 03:40:00Z, total 12m 7s. Root observed terminal success at 03:42:01Z. It ran Rust 1.96.0 aarch64 Linux with nextest 0.9.148 on four cores. The effective `pr` default filter is `all()` and includes the realtime group despite stale workflow comments. The command intersects it with the 12-of-46-package PR selection and excludes the two own harnesses, which run separately.

Nextest ran **7,136 tests: 7,136 passed, including one leaky pass; 32 tests and 106 binaries skipped**. The test phase lasted 141.134 s after Cargo reported 4m 31s compilation; the combined step took 413 s. Startup reports 62 binaries; terminal rows identify 58. All ordinals 1–7,136 are present, with 34 identical final-output repetitions counted once. The console does not enumerate skipped names/reasons or the four starting binaries without terminal rows; those identities are not inferred. This is selected-package runtime coverage, not every workspace test.

All 86 authored clock-fix cases have PASS evidence: 85 literal-name matches plus one visible `coordinator::tests::hand_controls::a_hand_kept_on_the_head_si***` prefix (raw line 3924), uniquely matched to the expected held-head/sigh function in exact tested source. Its full name is absent from the log; the attribution method is explicit in `arm64/masked-name-matches.json`. The leaky pass is `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`, 0.227 s, ordinal 5017, raw line 5705. There are no failed terminal rows; the leak remains disclosed.

Separate commands passed 4 doctests across 46 doc targets and 12 process-harness cases. The relay harness printed `relay_signals: ok` without a numeric case count. These are not added to nextest totals. Release workspace build, example builds, classification, export and upload all succeeded; all step metadata and full console output are retained.

Compared with the separately retained prior-head CI census, this changed upstream tree has 14 additional selected Voice cases and no removed selected cases; the aggregate skipped count increased by one, whose identity is not printed. This is not a controlled causal or speedup comparison. The carried patch is range-diff-identical; seven of its eight touched files are byte-identical across heads, while worker.rs includes upstream memory journal/test-helper changes. All four CI configuration snapshots are equal. `carry-comparison.json` and `previous-ci-census-comparison.json` retain these limits.

The slowest binary sums count each unique case once and exclude compilation/shared startup. Parallel durations overlap and include per-case setup; these are not binary wall times or speedup measurements.

| Binary | Cases | Summed case seconds |
|---|---:|---:|
| `lamp-voice` | 3609 | 243.474 |
| `lamp-skills::it` | 304 | 97.539 |
| `lamp-voice::it` | 301 | 49.238 |
| `lamp-cloud` | 166 | 37.041 |
| `lamp-update::round` | 116 | 18.920 |

The slowest non-Voice binary was `lamp-skills::it`. Its five longest cases were:

| Case | Seconds |
|---|---:|
| `night_sleep::a_lamp_nobody_saw_all_day_keeps_its_head_down_into_the_next_night` | 5.208 |
| `addon::only_a_say_kept_by_the_policy_is_asked_of_the_voice_quoted` | 4.761 |
| `skills::asking_the_policy_on_the_hosts_clock_gives_up_after_its_wait` | 4.518 |
| `skills::guard_mode_from_the_app_is_the_owners_alone_and_the_switch_closed_ends_it` | 4.249 |
| `skills::a_reading_pose_is_held_on_the_mood_socket_for_its_for_and_then_let_go` | 3.617 |

The artifact upload reports 47 workspace binaries, 2 selected examples and 50 files, ZIP SHA-256 `503fdc60ed137679db328229ff7dcca27c66df2895f2564d2fc4a7be24d0040d` (114,924,997 bytes; seven-day retention). The artifact itself was not downloaded or independently hashed. Both jobs restored cached output/source times; exact checkout identity is not a cold-build claim.

Published API files are minimal provenance projections without person attribution. Each complete original response remains external, bound by byte count and SHA-256 in `metadata-projections.json`. Full log JSON strings are byte-identical to their retained raw streams. The human attribution tokens in captured metadata do not occur in the decoded logs or other published text; no log redaction was needed. Organization/service identities are not classified as people; the scan’s scope and limits are explicit. No scripts, binaries, local ARM builds, workflow reruns or additional test execution are part of this evidence collection.
