# Boot-clock Mac verification — failed and incomplete gates retained

Ready: **NO**. This receipt is bound only to baseline `f477d6c49732dbd5cba84cbe18ea568692c056e6` and candidate `da595851f239cd6456cc1fb4565d3fc272675702`. It does not qualify the separately carried current-base branch. There were no automatic retries or source fixes during verification.

The native macOS x86_64 toolchain/binary evidence identifies the tested ISA; physical hardware and translation status were not established. No device or ARM64 execution is claimed here. [The full Voice comparison](voice-comparison/README.md) retains its environment, binary identities, all shared inventory names, and complete stdout/stderr.

## Clock controls

All 175 planned exact invocations are accounted for once. The identical scratch IPC shims changed `monotonic_ns`, keeping nanoseconds and microseconds coherent while leaving real `std::Instant` watchdogs intact. The modulo arm uses literal modulo 240 seconds and records that no process crossed a modulo wrap. The near-boot arm starts at 500 ms once per process and advances with real elapsed time. The patches are under [patches/](patches/).

| Observation | Passed | Named libtest failures | Abort |
| --- | ---: | ---: | ---: |
| Baseline modulo held-lines | 0 | 1 expected overflow | 0 |
| Candidate modulo held-lines | 1 | 0 | 0 |
| Baseline near500ms, 86 cases | 34 | 51 | 1 |
| Candidate near500ms, same 86 | 86 | 0 | 0 |
| Candidate accepts stale 4 s audio mutant | 0 | 1 expected original assertion | 0 |

The first run stopped at baseline index 13 after subtraction overflow at `coordinator/tests/hand_controls.rs:2358`, followed by TLS-destruction AccessError and fatal abort (exit -4). Its original failure and marker remain unchanged. A separately approved continuation retained that outcome as `aborted_after_overflow`, never a pass or ordinary libtest failure, then executed only previously unrun indices. The abort exception was restricted to the declared baseline fixture locations and exact overflow/TLS signature. Candidate controls required success.

The stale-audio mutant admits the existing 4 s block by changing only the scratch age guard; it fails the unchanged `service.audio() == 320` assertion with actual 640. [controls/observations.json](controls/observations.json) preserves every exact command, duration, exit, binary hash, classification and raw-stream reference. [raw/clock-controls.txt](raw/clock-controls.txt) retains all 350 stdout/stderr payloads, including the original abort.

## Full Voice observations

One unmodified full-package run per source used default libtest parallelism. Baseline: library **3548 passed / 0 failed / 14 ignored**, integration **269 / 1**. Candidate: library **3543 / 5 / 14**, integration **269 / 1**. Main and both test examples passed in both; doctests had zero cases. All 86 affected cases pass exactly once in each full run. Both full inventories are identical: 3843 identities, including 3562 library cases.

The five candidate-only library failures remain **unclassified**:

- `coordinator::tests::reminders::on_unmute_the_owed_reminder_is_offered_to_the_first_turn`
- `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture`
- `coordinator::tests::second_route::a_push_once_the_looks_picture_went_cuts_its_answer`
- `hand_controls::tests::with_the_wait_a_head_touch_is_read_as_it_arrives`
- `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`

`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them` fails its original latency assertion in both integration runs. Passing focused controls does not waive these failures, identify their cause, or establish inherited behavior. Full failure text and timings are in [voice-comparison/results.json](voice-comparison/results.json).

The two search failures name children 88029 and 89478. One read-only process observation later found both PIDs absent; that does not establish exit time/status or explain the cleanup timeout. [The child-absence record](voice-comparison/child-absence.json) preserves the exact observation. The raw `(5 occurrences)` wording is not treated as a retry count.

## Reader snapshot defect and authorized abort

The frozen verifier archived **only `lampOS`**. Update's image tests resolve repository-root `robots/lamp`, which was absent although those files are tracked in the frozen base. This is a verifier fixture-scope omission. The six completed Update image/publish/journal/ramlog failures are failed invocations on that incomplete tree, not evidence of a product regression or inherited flake.

The parent authorized ending this invalid reader invocation. After verifying the runner PID/start/command and the Cargo leader PID **34013**, parent **30102**, start identity, process group and descendant membership, the controller sent SIGTERM only to group 34013 at **03:00:35.835320Z**. The reader command exited **-15** after **1857.912342 s**, without timeout or a leftover process group. The frozen runner rejected that status, restored exact candidate/IPC bytes in `finally`, and wrote its terminal manifest. No fixture was added in-place and no reader run was repeated.

[reader-abort/partial-target-outcomes.json](reader-abort/partial-target-outcomes.json) binds all completed targets to compiled inventories. It records **29 completed targets: 1210 passed / 6 failed / 11 ignored**. The interrupted Update `round` target has **75 completed pass rows**, no final target summary, and a partial `no_second_round_begins_while_lamp_updates_own_switch_waits` row. Its remaining cases, five unlaunched executable targets, and all doctests are explicitly unrun. These partial counts do not constitute a passing reader gate. One normal libtest ` - should panic` display annotation is recorded explicitly when matching the inventory; no test identity was dropped.

[reader-abort/interruption.json](reader-abort/interruption.json) records ownership checks and the authorized signal. [raw/reader-partial.txt](raw/reader-partial.txt) retains both complete partial streams. Correct reader qualification requires the complete repository fixture tree in a separate run; it was not performed here.

## Completed gates and restoration

Formatting, strict all-target Voice Clippy, and strict all-target Clippy for Skills, Voice Eval, Memory, Update and Cloud passed. The original planned workspace build was unrun because the reader invocation was aborted. A separately authorized **single** `cargo build --workspace --offline` passed in **60.949639 s** on the restored snapshot, with no timeout or leftover process group. Original-range `git diff --check` also passed. The build does not repair or waive the incomplete reader gate.

[gates.json](gates.json) and [raw/final-gates.txt](raw/final-gates.txt) retain exact commands and output. The authored worktree remained clean at the frozen candidate. All 6523 archived lampOS files (203361607 bytes) retain aggregate SHA256 `47551982f2a0f34789d2c0284e2df5c1bf9223221a28cb2bbbe0f210254e0388`; original IPC bytes are restored. [source-integrity.json](source-integrity.json) records selected hashes, all 106 unchanged prior receipt bindings, and the 525-entry terminal manifest check. MAIN and local Cargo CPU were released after the build and source checks.

## Evidence format and limits

Raw wrappers preserve original payload bytes, including unterminated lines. [raw-payloads.json](raw-payloads.json) records byte offsets, lengths and SHA256s; offsets and lengths define the payload boundary when a closing marker follows an unterminated line. One interleaved baseline failure stream contains original trailing whitespace; that record explicitly uses JSON-string encoding and `stored_payload_bytes`. Decode it before hashing its original UTF-8 payload. Every payload was recovered and rehashed after packaging. The full Voice subfolder has its own payload manifest.

[artifacts.json](artifacts.json), [reader inventories](reader-abort/inventories.json), and [compile-provenance.json](compile-provenance.json) retain actual binary/source bindings, command status and compile-output hashes. Human compiler stderr is retained. Cargo JSON stdout, binaries, source snapshots and verifier Python are deliberately not copied here; their exact external paths/hashes are recorded, with a nonportable availability limit. Source/fixture and architecture limits above remain material. No new test execution is authorized by this receipt.
