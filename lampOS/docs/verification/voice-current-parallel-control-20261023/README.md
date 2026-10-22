# Current-head Voice parallel control, 23 October 2026

Ready for lamp-voice: **no, [#3023](https://github.com/autonomous-ai/lampOS/pull/3023)**.
One unchanged-current-head library run failed **3,387 passed / 7 failed / 14
ignored**, Cargo exit **101**. Two failures also occurred in the candidate's
earlier 18-failure run. They occur here without any of its eight fixture
changes. Five additional reminder failures are separate unmodified-current-head
observations. **All 18 candidate failures and the full Update consumer failure
remain unwaived.** No run was retried.

## Exact execution

Pinned source: `bad1144960da9dcdbbcc29072e86a767bcbd0437`, clean owned branch
`tests/1569-voice-current-parallel-control-20261023`. The
[Queue claim](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6346630405)
preceded execution. The shared target was reserved for this one command:

```sh
cargo test -p lamp-voice --lib --offline
```

The command ran from the worktree's `lampOS` directory with Rust 1.96.0,
four Cargo compiler jobs, dev/test debug 0, incremental 0 and offline mode.
`RUST_TEST_THREADS` and `CARGO_BUILD_TARGET` were unset: default libtest
parallelism, with no stress workload, mutation or retry. The exact paths,
environment and 1,800-second owned-process timeout are in [plan.json](plan.json).
It started at **2026-10-22 17:17:53 UTC** (23 October 00:17:53 +0700).
Libtest took **342.98 s**; whole-command time **473.237265 s** includes compilation.
The timeout did not fire. Its process session was empty at cleanup, with no
cleanup signal or remaining owned child. Host load averages are context in
[commands.json](commands.json), not causal evidence or controlled variables.

The original command's freshly compiled artifact was subsequently invoked
once with `--list --format terse` for metadata only (exit 0, **0.213458 s**).
It lists **3,408** unique identities. All 3,408 terminal result rows were checked
against that inventory, including the three `- should panic` rows; the counts
are 3,387 `ok`, 7 `FAILED`, 14 `ignored`. Compared with both earlier EC and 735
inventories of 3,398, there are **11 additions and 1 removal**. The complete
names and both exact deltas are in [inventory-comparison.json](inventory-comparison.json).
This metadata invocation ran no test bodies.

## Exact seven failures

The two overlaps with the candidate's 18 are:

| Full test identity | Original failure |
|---|---|
| `coordinator::motor::tests::privacy_does_not_hide_actual_discard_or_close_failure` | `motor/tests.rs:883:9`: owned child timed out; worker ended independently, connected true, ready false, reaped true, supervisor `ControlLost`; largest poll gap 3,118 us. |
| `coordinator::optional_ring_tests::a_faulted_ring_starts_again_off_the_loop_and_a_stop_after_it_is_clean` | `optional_ring_tests.rs:881:5`: restart tick took **81.160744 ms**. |

The five additional failures on unchanged current source are:

| Full test identity | Original failure |
|---|---|
| `coordinator::tests::reminders::a_pulse_held_back_keeps_the_time_it_was_owed_from` | `reminders/consumer/roundtrip_tests.rs:371:13`: `fresh durable offer`. |
| `coordinator::tests::reminders::a_reminder_cancelled_while_muted_is_never_said` | Same original `fresh durable offer` assertion and location. |
| `coordinator::tests::reminders::a_reminder_held_for_its_skill_keeps_other_unasked_speech_waiting_while_it_is_offered` | Same original `fresh durable offer` assertion and location. |
| `coordinator::tests::reminders::a_reminder_pending_at_a_start_is_owed_to_no_mute` | Same original `fresh durable offer` assertion and location. |
| `coordinator::tests::reminders::reminder_waits_for_named_request_second_look_before_its_window_can_expire` | `coordinator/tests/reminders.rs:287:56`: `Result::unwrap()` on `Err(StaleOwner)`. |

[Complete failure blocks](failure-diagnostics.json) preserve byte offsets,
lengths and hashes of every block, including all captured motor child output.
The embedded child excerpt ends mid-event at `motion`; no separate complete
child-output file was retained. The original helper reads at most 2,048 bytes
of that file at the assertion. The [full stdout](raw/full-library-stdout.json) and
[full stderr](raw/full-library-stderr.json) retain every original byte.

## Assigned-case rows

The following exact result rows are present once each in the stored stdout;
their passing status is read directly, not inferred from the aggregate:

```text
test provider_worker::tests::requested_audio_crosses_the_live_worker_only_after_its_tool_reply ... ok
test provider_worker::tests::policy_holds::a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet ... ok
test provider_worker::tests::reminders::a_reminder_rung_into_a_busy_room_is_no_answer_to_end_it_by ... ok
test remember::tests::the_oldest_of_a_long_kept_list_is_found_and_the_rooms_are_marked ... ok
test second_provider::worker::tests::stop_during_music_rpc_revokes_scope_and_finishes_without_waiting_for_the_reply ... ok
test second_provider::worker::tests::the_lamps_own_answers_to_talk_keep_no_conversation_going ... ok
test coordinator::tests::requested_audio::cancelled_gapped_or_expired_music_never_acknowledges_its_confirmation_news ... ok
test provider_worker::tests::kept_start::with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end ... ok
```

All 18 candidate-failure identities are also present in the current inventory;
two are `FAILED` here and sixteen are `ok`. Their actual rows are in
[results.json](results.json), alongside the eight rows above and the parser
expression. **Light-call is integration coverage and was outside this `--lib`
command.** Passing rows do not waive any prior failed full-suite gate.

## Source and architecture evidence

[Candidate-delta absence](candidate-delta-absence.json) records all eight
changed fixture paths: every current Git blob equals the EC baseline blob
and differs from the 735 candidate blob. The current control contains none
of those fixture deltas. [Failing-source identity](failing-source-identity.json)
also covers every current failure's test-definition and panic-site file:
`coordinator/motor/tests.rs`, `coordinator/optional_ring_tests.rs`,
`coordinator/tests/reminders.rs` and `reminders/consumer/roundtrip_tests.rs`.
Each is byte-identical across EC, candidate 735 and this current head.
That file identity does not establish equal whole-program state or scheduling.

All **6,426 Git-tracked lampOS files**, including docs, Cargo/toolchain/config
and fixture inputs, were verified against their committed bytes. Their mtimes
were refreshed before compiling with identical contents. Every byte remained
unchanged after the command and inventory; the control worktree stayed clean.
The complete map is [source-manifest.json](source-manifest.json); its aggregate
SHA256 is `76acb1087287e1f515baf6297bf3e1cf7f6b0f243a88c4c195679cb7f5af2e0e`.
The aggregation uses Python's default JSON encoding of the sorted
path-to-SHA256 map, as specified in the plan. [Freshness](source-freshness.json)
and [restoration](restoration.json) retain the before/after checks. This scope
excludes ignored/untracked files, paths outside lampOS and compiler/registry
contents; it is not a whole-filesystem reproducibility claim.

`rustc -vV` reports **1.96.0, host x86_64-apple-darwin**. `file` and `lipo`
report the actual compiled artifact as **Mach-O x86_64**, 107,034,176 bytes,
SHA256 `893b4b82d791966c858fc411834c4c1804ae64b62d1e8378d4f928a41f0e4156`.
Its fresh compile line names the control worktree and its timestamp follows
command start. [Artifact metadata](binary-architecture.json) records both
architecture commands and unchanged artifact bytes during inspection.

Hardware metadata is separate: the OS reports Darwin 25.6.0, `iMac20,1`,
`x86_64` and an Intel Core i7-10700K brand string. The translation-status and
ARM-option sysctl keys were unavailable. [Escalated platform reads](platform-toolchain-escalated.json)
used the same execution context as the test; the earlier
[sandbox-limited metadata reads](platform-toolchain.json) remain recorded.
Those denied metadata reads were not extra test attempts. No physical-hardware
or native-ISA conclusion is inferred from uname or brand strings.
**No ARM64 build, Linux qualification or device execution occurred here.**

## Limits and retained gates

The [earlier fixed EC/735 pair](../voice-ec-parallel-20261022/) was EC
`ec5428a34c0f66688e54200473cf26e6c3dd2b88` at **3,384 / 0 / 14**, then 735
`735b2cb5db21176a0ad80fcb42c48e739f8362fe` at **3,366 / 18 / 14**.
This current upstream has changed and its inventory differs. It is **not a
matched causal pair** with either earlier arm. It proves only that the two
overlapping named failures also occur without our patch. It establishes
neither a common cause for those failures, inheritance of the remaining
sixteen, host-load causation nor patch causation. No speedup is claimed.

The earlier full Update consumer gate remains failed at
`self_cuts::interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart`
and its original two-second updater-exit assertion. Its
[full failed receipt](../voice-latest-refresh-20261022/) remains authoritative;
the separate focused EC pass does not clear it. #3023 remains not ready.

## Receipt recovery

[Raw stream index](raw/index.json) contains all four command streams, including
the empty inventory stderr. Join each wrapper's `text_lines` without
separators, encode UTF-8, then check its original byte count and SHA256.
The wrapper retains blank lines and trailing whitespace exactly while keeping
the JSON files whitespace-clean. The compiled binary and external runner code
are not published; their hashes do not guarantee ongoing local availability.
[Provenance](provenance.json) preserves the external records and runner digests.
The shared target was released after artifact inspection. Publication reran
no tests and changed no source. [Validation](validation.json) records receipt
checks, and [manifest](manifest.json) hashes every receipt file except itself.
