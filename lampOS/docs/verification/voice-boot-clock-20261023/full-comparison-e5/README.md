# e5 baseline and current boot-clock full Voice comparison

**Overall qualification: pending; Ready: NO.** This receipt covers only the
completed candidate Voice step and its fixed [e5 baseline](../baseline-e5/README.md).
The reader and remaining top-level gates were still active when packaging
began and are outside this receipt. No overall gate completion is claimed.
The baseline package remains frozen at manifest SHA256
`7810f3343053d2b7997ba96e7a15d1b0f12087cc8c3b3c4457eb3198e72c0e08`.

| Source | Full package passed / failed / ignored | Library passed / failed / ignored | Command exit |
| --- | --- | --- | ---: |
| e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc | 3,860 / 10 / 15 | 3,580 / 9 / 15 | 101 |
| 0f2e6439e018680af247080983c9ec0c258c3229 | 3,869 / 1 / 15 | 3,589 / 0 / 15 | 101 |

Both observations completed all **6 targets / 3,885 case identities**, with the
same complete target, case and 15-name ignored census. All **86 selected cases
passed in each**. There are **no added failed cases** in the candidate observation.
Nine baseline library failures pass after; the same light-call integration
case remains failed. Both package commands remain failed and unwaived.

The command was identical in both arms:

```text
cargo test --offline --locked -p lamp-voice --no-fail-fast --message-format=json-render-diagnostics
```

This was one predeclared observation per source, not a retry-until-green.
Candidate wall time was **361.240300 s**, including compilation; its library
reported **236.67 s** and integration **10.10 s**. Baseline command wall time
was 618.682987 s (library 288.13 s, integration 10.36 s). These are not comparable
latency measurements: compilation/cache and host scheduling are not isolated.
Compiler stderr's `1m 46s` is a rounded compiler message, not a separately timed
compile run. No per-case elapsed times were emitted.

## Each baseline failure

[Exact failure comparison](baseline-failure-comparison.json) retains every
baseline diagnostic and both actual rows. Complete baseline streams remain in
[baseline-e5](../baseline-e5/README.md); complete candidate human stdout and
stderr are retained here.

| Case | Baseline | Candidate |
| --- | --- | --- |
| coordinator::motor::tests::privacy_revoke_survives_a_host_pause_before_the_gated_worker_is_released | FAILED | ok |
| coordinator::motor::tests::unresolved_cleanup_removes_the_service_mute_restart_token | FAILED | ok |
| coordinator::motor::tests::privacy_does_not_hide_actual_discard_or_close_failure | FAILED | ok |
| coordinator::motor::tests::readback_and_original_placement_are_required_and_bad_offers_do_not_enqueue | FAILED | ok |
| coordinator::motor::tests::same_owner_input_end_cancels_queued_gesture_before_inert_bus_write | FAILED | ok |
| coordinator::motor::tests::same_owner_input_end_survives_a_host_pause_before_the_gated_worker_is_released | FAILED | ok |
| coordinator::motor::tests::stale_snapshot_does_not_revoke_newer_idle_authority | FAILED | ok |
| coordinator::motor::tests::native_head::native_head_replayed_receipt_and_changed_camera_grant_are_terminal | FAILED | ok |
| privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second | FAILED | ok |
| light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them | FAILED | FAILED |

The remaining light-call failure is still the original `<25 ms` upper-middle
assertion at `crates/voice/tests/it/light_call.rs:90:5`. The sorted array's element
10 is **38.435873 ms before** and **49.055730 ms after**. Both full arrays and the
complete candidate failure are retained in the baseline comparison and
[candidate-failure.json](candidate-failure.json). This is not a physical-device
latency measurement or an exemption from the assertion.

## Earlier five candidate-only library failures

The [earlier f477/da595 receipt](../mac-full-comparison/README.md) remains valid
and unwaived. [Historical comparison](historical-five-comparison.json) preserves
all five original diagnostics, their exact source locations, complete failure
excerpts and actual outcomes in all four observations:

| Case | Old f477 | Old da595 | e5 baseline | Current candidate |
| --- | --- | --- | --- | --- |
| coordinator::tests::reminders::on_unmute_the_owed_reminder_is_offered_to_the_first_turn | ok | FAILED | ok | ok |
| coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture | ok | FAILED | ok | ok |
| coordinator::tests::second_route::a_push_once_the_looks_picture_went_cuts_its_answer | ok | FAILED | ok | ok |
| hand_controls::tests::with_the_wait_a_head_touch_is_read_as_it_arrives | ok | FAILED | ok | ok |
| privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second | ok | FAILED | FAILED | ok |

The four entire files containing those five failure sites are byte-identical
across all four commits; their hashes and Git blob IDs are recorded. This is
not a claim that dependencies, clock helpers, the full repository or host
conditions are identical. The old pair had 3,843 case identities, versus 3,885
here. A later pass does not establish an original cause or resolve the historical
hold. The repeated privacy failure on e5 confirms only that its exact boundary
can also occur on the later base without this fixture patch. It does not justify
classifying all five historical failures as inherited. Raw `5 occurrences`
cleanup wording is preserved without interpreting it as five independent waits.

## Proof and scope

[Pair summary](pair-summary.json), [targets](targets.json),
[86 selected comparisons](affected-86-comparison.json) and
[census bindings](census.json) distinguish execution from listing. Every case
and ignored row was independently matched; the three libtest ` - should panic`
annotations map only to unique compiled names, without dropping any row.
The current compiled/listed inventories are byte-identical to the baseline
inventories and therefore reference those immutable payloads instead of copying
them again. This is identity of inventory text, **not binary identity**.

[Artifacts](artifacts.json) preserves the captured source/binary hashes and
profile metadata for all 13 executable artifacts; five are listed test targets.
[Source proof](source-and-scope.json) independently reconstructs all 9,992 tracked
paths at the committed candidate and matches the full source aggregate embedded
in the artifact bindings. The exact eight changed paths and before/after hashes
are included. No live working-tree file or build-target artifact was read or
modified during packaging.

Only the completed Voice folder was read from the active gate run. Its command,
profile and Mach-O x86_64 identities are captured here. Full-run inherited
environment, rustc identity, source-freshness and final restoration results were
not reread from mutable top-level files and remain outside this step receipt.
Physical hardware/translation and matched host load are not established.

The passing library step also emitted `/bin/sh: .../lamp-same-voice-loading-85526-37679030/lamp-same-voice: No such file or directory`.
That exact line remains in full stderr. This receipt does not attribute it,
call the log warning-free, or invent an additional failed test.

## Raw evidence and publication dependencies

[Commands](commands.json) binds all 16 completed-step command records and their
32 stdout/stderr streams. The complete non-Cargo stdout and entire stderr are
lossless. Only complete Cargo machine-record lines before the first test suite
are omitted; explicit original offsets/lengths/hashes and the complete original
stream hash/path remain. No test or failure text was omitted. The compiler
stderr and normalized artifact/compilation proof are preserved.

[Raw payloads](raw-payloads.json) contains exact byte offsets, lengths and hashes.
Entries beginning `../baseline-e5/` depend on the frozen sibling baseline package;
all referenced payload bytes were compared directly. Historical failure excerpts
are retained locally with offsets into the verified old stdout payload.
[Completed step seal](completed-step-seal.json) pins all files in the finished
Voice folder. Large duplicate metadata and Cargo machine output remain external
with explicit path/size/hash in [external-files.json](external-files.json).
No Python scripts, binaries, Cargo machine streams or person attribution are
included. No Cargo command, source edit, target change or publication was done
for this audit.

[validation.json](validation.json) records byte/hash/census/link checks, and
[SHA256SUMS](SHA256SUMS) covers this package. The intended publication directory
is `full-comparison-e5/`, sibling to `baseline-e5/` and `mac-full-comparison/`.
Publication and final gate disposition remain with the parent.
