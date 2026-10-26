# Reader unchanged-base control

The unchanged `3d9722f38817a4dd1601fac051d36ac9f2292969` baseline reproduces the same three Voice-eval worker PID-existence failures as the Reader candidate. One serial `relay_hardening::` module observation completed **4 passed / 3 failed / 0 ignored**, with 231 filtered out and exit 101. The successful controller completion does not mean the module passed.

The three failures have identical assertion locations and messages in both runs:

- `a_closed_runner_transport_stops_the_runtime_and_its_workers`: line 228, “worker survived the stop”.
- `an_early_relay_error_after_the_runtime_exited_still_stops_its_workers`: line 326, “a worker survived the relay's early return”.
- `workers_left_after_a_normal_exit_are_stopped_and_reported`: line 248, “worker survived the relay”.

All are in `lampOS/crates/voice-eval/tests/it/relay_hardening.rs`. Each tests `process_gone(read_pid(&pid_file))`, a three-second bounded PID-existence check. The preceding relay/status assertions completed.

Both runs kept their source unchanged across execution. The clean baseline and candidate share the same commit and index; comparison of all 10,564 tracked entries finds exactly the candidate's two Reader Rust files and realtime-tests document different. The saved baseline and original candidate integration executables were independently rehashed and are identical: `4750b34999755b985db25b1dbcaf0200781b35a719141c967b22b4b3935a344b`; their recorded before/after hashes are stable. The artifact has debug information 0, default assertions/overflow checks enabled, and no features.

The control ran on the pinned Linux AMD64 image as UID/GID 1000:1000, without init or extra capabilities; PID 1 was `tail`. Its post-module snapshot shows one `sh` and two `sleep` zombies parented to PID 1. This snapshot was not linked to each asserted PID/starttime. It supports an unreaped-child hypothesis for this control only; historical original-run cause remains unproven.

The three failures therefore reproduce without the Reader patch in this lane. The original full run remains **FAILED: 7,454 passed / 3 failed / 53 ignored across 79 summaries**. This focused control is not a full-suite pass, a gate waiver, current-c5 qualification, ARM64 qualification or device acceptance. The companion JSON contains exact outcomes, source/artifact bindings and hashes of the retained raw receipts. No raw logs, private paths or person data are included here. This audit ran no tests/compiler and made no posts or source edits.
