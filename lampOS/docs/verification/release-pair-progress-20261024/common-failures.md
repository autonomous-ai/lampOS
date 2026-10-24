# Frozen Common full Voice failure audit — 2026-10-24

This is a read-only log/source audit, not a rerun or a passing gate. Candidate tree `7b5df82fd031432c9ee0cb32754fd5b9f622f351` is based on `1d915d44c97fcdd980a8a5d9c9e4fecc9570d746`; comparison main is pinned `4a84fa5ab6a9133aeec555ec4bc0636dd953f660`. No source/index/ref/mtime/target changes, process kills, Cargo commands or external posts were made.

No concrete Common implementation or merge defect is established by these logs. All nine source files containing the 15 Mac failures, the Linux `capture_hold_worker.rs` fixture, and the inspected directly relevant production/helper files are byte-identical to both comparison commits. That establishes source ownership, **not the cause or harmlessness of the failures**. No unexpected private grant or disclosure is shown; several privacy, cancellation and response assertions were never reached because their setup or earlier boundary failed.

## Terminal totals

| Platform / target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Mac library | 3,782 | 15 | 15 |
| Mac main + integration + two examples | 281 | 0 | 0 |
| **Mac parent total** | **4,063** | **15** | **15** |
| Linux library | 3,827 | 0 | 17 |
| Linux main | 1 | 0 | 0 |
| Linux integration | 293 | 8 | 1 |
| Linux two examples | 10 | 0 | 0 |
| **Linux parent total** | **4,131** | **8** | **18** |

Both commands exited 101. Doctests selected zero tests. The isolated ReportsStop child has a separate 0/1 result, already represented by its failing Mac parent; it is **not a sixteenth parent failure**. The no-fail-fast parent commands reached all listed targets. Assertions after a test panic are not counted as executed.

## Common named controls, separately recounted

`common-controls-current-source.json` rederives each exact name and line from the frozen tree, hashes its current source, and records the actual outer-library result rows. It does not reuse an earlier CI status.

- All **63 core Common controls passed on both platforms**, including the real Days event-lock retry, returned-event provenance, original response ceiling, Checking/returned identity, carried-current proof and short-line cases.
- Of 17 separately named carry/integration controls, **15 passed on Mac and 16 on Linux**. The Update scratch census is not selected by a full Voice command. `shared_session_rotation_keeps_granted_descriptors_and_private_health` is Linux-only and passed there; its absence on Mac is not a pass.
- Thus the 80-name inventory contains **78 observed Mac passes and 79 observed Linux passes**, with the above selection distinctions. This does not override either failed full Voice gate.
- Broader Mac prefixes (`memory_scope`, private-context, lookaway, pending-context, kept-name privacy) separately contain 94 passes; these are overlapping inventories and must not be added to the 80-name totals.

## Mac failures and actual boundaries

Paths below are relative to `lampOS/crates/voice/src/`. Each failing fixture file is unchanged in candidate/base/current-main. Full exact failure blocks are retained in `failure-blocks.log`.

| Exact test (common prefix abbreviated only in this table) | Failure boundary / observed evidence | Narrow next diagnosis, not executed |
|---|---|---|
| `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture` | `coordinator/tests/second_route.rs:384`, `Run::assert_clean`: scenario watchdog; child exit observation timeout and forced 300 ms worker/ring cleanup; no verified ring black-write receipt. | Capture each fixture child's exit observation and wait-clock interval before the clean assertion. |
| `coordinator::tests::second_route::a_push_once_the_looks_picture_went_cuts_its_answer` | Same helper and cleanup boundary; cancellation events are in the trace, but subsequent case assertions did not run. | Same owned-child exit/receipt diagnosis; do not count trace events as the later assertions passing. |
| `coordinator::tests::second_route::the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken` | Same helper: child exit observation timed out repeatedly; scenario watchdog; nod/sway assertions not reached. | Capture fixture lifecycle before diagnosing head-motion behavior. |
| `coordinator::tests::service_mute::repeated_verified_mute_cycles_keep_one_logger_and_never_open_audio_while_waiting` | `coordinator/tests/service_mute.rs:387`: `finish_logging().unwrap().acknowledged` was false. Earlier cycle/one-logger assertions passed. | Retain the full finish Report and eventual writer completion. `service_trace.rs:397–402` maps any receive-timeout/disconnection error to absent acknowledgement; the log does not distinguish them. |
| `hand_controls::tests::with_the_wait_a_head_touch_is_read_as_it_arrives` | `hand_controls/tests.rs:1529`: no touch by 2 s; health received=2, rejected=1, discontinuities=2, pump_running=false. | Capture rejected reason and direct pump-thread result. The running flag alone does not prove this helper thread exited. |
| `hand_controls::tests::with_the_wait_an_idle_pump_wakes_five_times_a_second_and_stops_at_once` | `hand_controls/tests.rs:1375`: 76 passes versus maximum 10. `idle_pump` samples around a nominal 1 s sleep without recording elapsed time. | Record actual measurement interval and wake reasons; oversleep versus excessive wakes is unresolved. |
| `privacy_bus::tests::after_a_datagram_a_gap_ends_the_lease_reader_inside_its_first_second` | `privacy_bus.rs:1425`: ten attempts reported “invalid privacy message discarded”. | Capture the validation reason and actual lease age before asserting a scheduling explanation. |
| `privacy_bus::tests::selected_endpoints_bind_and_read_a_real_bus` | `privacy_bus.rs:1205`: five attempts exceeded the lease freshness interval. | Record send/read/validation monotonic times and actual errors. |
| `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second` | `privacy_bus.rs:1313`: ten attempts reported “invalid privacy message discarded”. | Same validation/age diagnosis; the fixture's broad “stalled” text classification is not causal evidence. |
| `privacy_bus::tests::the_lease_watch_answers_at_once_through_refused_input_and_lets_a_lease_expire` | `privacy_bus.rs:1577`: five initial fresh-lease acquisitions failed. | Inspect initial acquisition before claiming later refused-input/expiry checks ran. |
| `provider_worker::tests::catalog::no_renewal_while_a_turn_is_open` | `provider_worker/tests/catalog.rs:362`: “renewed with a turn open”; log also records held-up snapshots. | Capture the actual `Moment` fields and turn/visual ownership at renewal (`provider_worker.rs:2577–2599`). The test label is not evidence that authority was still open at that point. |
| `provider_worker::tests::review_2026_10_14::an_accepted_decline_allows_reports_withdrawal_without_reviving_speech` | Parent `review_2026_10_14.rs:1001` reports child failure. Child fails `response_id` at759 because the next message lacks `toolResponse.functionResponses`; owner answered ReportsStop in293ms, response was not sent. | Preserve actual received message and supervisor/worker terminal result. “connection has gone” at `provider_worker.rs:4170–4195` is a generic failed-response path, not proof of a specific socket cause. |
| `provider_worker::tests::spoken_to::the_same_turn_with_the_lamps_name_changes_the_lamp` | `provider_worker.rs:9978`: heartbeat publish unwrap received ConnectionReset (OS54), after earlier actions succeeded. | Capture the worker's prior terminal reason before attributing the reset. Later action assertions were not reached. |
| `provider_worker::tests::stop_and_heartbeat_are_serviced_while_a_reconnect_hangs` | Same heartbeat helper/reset while pumping the fixture; final Stop/exit-time checks were not reached. | Capture worker/supervisor terminal result and lease timing before the failed publish. |
| `task_news::tests::a_skill_made_by_text_is_told_as_kept_by_the_texts_reader` | `task_news.rs:3088`: no first untold item by 5 s. Missing installed news.words fallback is the only emitted diagnostic. | Capture first text-read/RPC/keeper/filter outcomes. The reader polls every10s (`task_news.rs:58,605`) after its first read; the 5s test window alone does not identify which stage failed. |

The three second-route cases use `Worker::cooperative_ready_fixture` (`second_route.rs:1480–1490`) and production coordinator with fake peers; they do not instantiate Common's actual Second Engine worker. The shared-clock fixture still has real child-exit observation (`scenario_clock.rs:259–292`) and 300 ms shutdown budgets. Privacy freshness is 50,000 µs (`privacy.rs:5`); the four failures above use denied/muted leases and provide no evidence of an unintended grant.

## Linux failures and actual boundaries

All failures share `capture_hold_worker::` under `lampOS/crates/voice/tests/it/capture_hold_worker.rs`, unchanged across the three source identities. The fixture uses actual `capture_on::<ScriptedCapture>` (`:314–332`); inspected `audio_workers.rs` is also byte-identical. Full blocks are in `linux-failure-blocks.log`.

| Exact suffix | Observed failure | What remains unknown |
|---|---|---|
| `another_microphone_is_handed_on_at_once_and_its_early_sound_is_not_accounted_for` | `:472` ReferenceBacklog after218 frames; last accepted render10,901µs before the fault, zero microphone unread frames. | Which backlog condition fired, actual render lag and queue history. |
| `mic1_frames_wait_four_blocks_with_their_own_times_and_a_stop_drops_what_is_held` | `:472` ReferenceBacklog after104 frames;108 processed, last render17,731µs earlier, zero unread. | Same; last-render age alone is not the complete backlog predicate. |
| `mic1_muted_while_frames_are_held_hands_none_of_them_on` | `:575` reference-send unwrap: ConnectionRefused (OS111). | Capture-thread result/queued fault before the helper's bare send failure. |
| `mic1_with_a_recording_keeps_the_held_steps_before_the_first_block` | Same `:575` ConnectionRefused. | Same; do not infer ReferenceBacklog from another test. |
| `stalls_in_a_row_each_reopen_the_microphone_while_the_reference_goes_on` | `:472` RecoveryExhausted after100 frames; last render883µs earlier. | Recovery history and actual capture-thread result. |
| `stop_words_blocks_carry_what_was_asked_only_when_asked` | `:575` ConnectionRefused. | Prior worker failure/reason not captured by this unwrap. |
| `the_loop_waits_on_the_microphone_only_with_the_switch` | `:472` ReferenceBacklog after13 frames; last render19,543µs earlier, zero unread. | Full reference-window/backlog counters and input schedule. |
| `with_the_render_5_ms_after_each_block_all_but_a_few_blocks_are_read_within_a_ms` | `:575` ConnectionRefused, capture-wait enabled. | Prior worker failure; the later latency comparison is not established. |

These fixtures serialize one another with a local mutex; other binary/library load and actual clock scheduling were not measured here. Historical fixture comments describing starvation are not evidence that this run has the same cause.

## Nextest and remaining limits

`.config/nextest.toml:33` places every `capture_hold_worker::` test in the realtime group (max threads1 at28; group/priority assignment38). The Mac idle-pump case is also in that group. The PR profile at67–68 has `default-filter = 'all()'`: **these cases are not excluded**. The failed host commands use Cargo and therefore do not use nextest group scheduling. Neither fact is a waiver or a causal diagnosis.

`receipt.json` preserves source object IDs/SHA256s, actual frozen bytes/modes/mtime comparisons, terminal counts and shared-dependency comparisons. `focused-commands-NOT-RUN.txt` names exact per-case commands for a later authorized diagnostic; none was run here. Full Update was still owned by the parent gates during this audit, so its scratch-census result is not inferred from Voice output. A passing current-head native gate has not been claimed.
