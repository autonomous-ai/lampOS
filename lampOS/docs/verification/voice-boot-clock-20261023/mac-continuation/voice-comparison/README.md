# Frozen Mac Voice comparison — completed observations

This receipt covers one full-package observation per source: baseline `f477d6c49732dbd5cba84cbe18ea568692c056e6` and candidate `da595851f239cd6456cc1fb4565d3fc272675702`. Both ran `cargo test -p lamp-voice --offline --no-fail-fast` with default libtest parallelism, jobs 4, dev/test debug information disabled, incremental compilation disabled, and the same target directory. Compilation was separately recorded before each execution. This receipt does not qualify the later current-base carry.

The immutable archive contains the tracked `lampOS` tree only; its exact archive command is in results.json. It omits repository-root `robots/lamp`, which later Update reader tests require. Those later failed reader invocations and this verifier omission will be retained in the final receipt. This export contains only the completed Voice comparison and makes no repository-wide qualification claim.

Ready: **NO**. The five candidate-only library failures remain unclassified. Passing focused controls does not waive them or establish inherited behavior. No full run was repeated to obtain green.

| Target | Baseline passed / failed / ignored | Candidate passed / failed / ignored |
| --- | --- | --- |
| Library | 3548 / 0 / 14 | 3543 / 5 / 14 |
| Main | 1 / 0 / 0 | 1 / 0 / 0 |
| Integration | 269 / 1 / 0 | 269 / 1 / 0 |
| Example render_line_second | 4 / 0 / 0 | 4 / 0 / 0 |
| Example stop_words_stt_replay | 6 / 0 / 0 | 6 / 0 / 0 |
| Doctests | 0 / 0 / 0 | 0 / 0 / 0 |

Both commands exited 101 without timeout or a leftover process group: baseline 260.902505 s, candidate 379.012020 s. All 86 affected case names occur exactly once and pass in both full runs. All five compiled target inventories are identical (3843 identities, including 3562 library cases); [inventories.json](inventories.json) retains those names. [artifacts.json](artifacts.json) binds each binary hash and size to its recorded archived-source aggregate and selected source hashes. [environment.json](environment.json) records the bounded Cargo environment. The captured toolchain and executable-format streams under raw/ identify macOS x86_64; physical hardware and translation status were not established. These results make no device or ARM64 claim.

The five candidate-only library failures are:

- `coordinator::tests::reminders::on_unmute_the_owed_reminder_is_offered_to_the_first_turn`: `roundtrip_tests.rs:371`, fresh durable offer after its due notice aged while the ledger was written.
- `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture`: `second_route.rs:384`, scenario host watchdog and verified cleanup failure for child 88029.
- `coordinator::tests::second_route::a_push_once_the_looks_picture_went_cuts_its_answer`: the same assertion and cleanup diagnostic for child 89478.
- `hand_controls::tests::with_the_wait_a_head_touch_is_read_as_it_arrives`: `hand_controls/tests.rs:1529`, touch never arrived; health reports `pump_running: false`.
- `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`: `privacy_bus.rs:1313`, privacy publisher unavailable in all ten existing fixture attempts.

`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them` fails its original latency assertion in both integration runs. Complete diagnostics and both duration lists are preserved in [results.json](results.json) and the raw streams. The cleanup wording `(5 occurrences)` is retained verbatim and is not interpreted as five independent retries.

At 2026-10-23T02:37:15.809569Z, one read-only `ps` observation found both child PIDs absent (exit 1, empty stdout/stderr). [child-absence.json](child-absence.json) records the exact command and time. Absence then does not establish exit times, exit statuses, or why the earlier cleanup observation timed out. No signal was sent.

The files in [raw/](raw/) preserve complete stdout/stderr payloads, including failures, with explicit byte lengths, offsets, SHA256s, and closing markers. The payload offsets in results.json reconstruct the original streams byte-for-byte. The separate reader/build command was still running when this completed comparison was exported. Its result and the 175 clock-control invocations belong to the final continuation receipt, not this partial export.

No scripts, binaries, Cargo machine-output streams, or personal account metadata are included. Original machine-output compilation evidence and frozen execution records remain at the external source paths recorded in results.json; those paths are not portable downloads. This receipt provides the binary/source bindings and full test output, but does not reproduce the compiler-artifact streams.
