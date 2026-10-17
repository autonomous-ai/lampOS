# Realtime tests

**Status: temporary, to be reviewed.** Since 2026-10-15 the tests that wait
on the real clock form the nextest test group `realtime`
([`.config/nextest.toml`](../.config/nextest.toml)). Until 2026-10-17 pull
requests skipped it. Since then every run runs it, one test at a time and
started first, beside the rest (below). In the ARM64 run
(`.github/workflows/lampos-arm64.yml`):

| Run | Runners | nextest profile | Tests at once | Realtime group |
|---|---|---|---|---|
| Pull request into `lamp-voice`, manual run | 1 | `pr` | one per core | run, one at a time, started first |
| Push to `lamp-voice` (every merge; its binaries are what [updates](updates.md) downloads) | 1 | `pr` | one per core | run, one at a time, started first |
| Full run: nightly, or manual with `full` | 4 | `default` | one, as before | run |

The nightly full run starts at 19:00 UTC (02:00 in Vietnam) from
`.github/workflows/lampos-nightly.yml` on `main`: GitHub starts a scheduled
workflow only from the default branch. It starts this workflow on
`lamp-voice` as a manual run with `full`, so the job is defined in one place.
By hand: Actions, "lampOS on ARM64", "Run workflow", branch `lamp-voice`,
tick `full`; or `gh workflow run lampos-arm64.yml --ref lamp-voice -f full=true`.

When a full run fails, a job opens an issue titled "lampOS: the full ARM64
run failed", or comments on the open one. Nothing the tests assert was
changed. The runners are unchanged (`ubuntu-22.04-arm`): larger runners cost
too much.

Each runner compiles every test binary again (about 5 of its 10 minutes), so
pull requests and merges use one runner, where the realtime group was
skipped until 2026-10-17; those tests took
about a minute in parallel (run 38721013037: 4,399 tests, 55 s, 4 cores).
A merge costs one runner for about 11 minutes; until 2026-10-15 it cost four
for about 15.

## Why

The ARM64 run tests one test at a time, as on a lamp's few cores. On the
2026-10-15 runs 38715042828 and 38715014248, all 4,507 tests took 1,191 s,
and the 123 tests in this group took 977 s of it (82%). Each of the four
shards spent 4 to 6 minutes on tests, mostly waiting on sleeps, deadlines
and fakes that play a conversation at real speed. For example,
`serve_session::normal_eight_turn_conversation_in_one_serve_session` takes
65 s because `fixtures/voice-eval-serve-v1.json` has 26.5 s of `speech_ms`,
2 s `startup_ms`, 1.5 s `first_audio_ms` per turn and 6.6 s of trigger
`delay_ms`. Without the group, the rest take about 215 s one at a time.

The waiting is real time by design in most of these suites (see each file's
header). They are slow, not flaky; they did not fail in these runs (the
timed tests added later, under "What is in the group", are the exception). Skipping
them everywhere was rejected: they catch timing faults in whole
conversations, so the nightly full run still runs them.

## What is in the group

The group is a nextest filterset, written twice in `.config/nextest.toml`
(the group override and the `realtime` selection), which must stay
identical. Until 2026-10-17 it was written a third time, as the `pr`
profile's exclusion:

- the whole test binary `lamp-voice::capture_hold_worker` (the evaluator's
  four, `serve_session`, `physical_loopback`, `serve_relay` and
  `final_dispatch`, and six of `lamp-memory::health`'s seven tests left on
  2026-10-17, below);
- one test of `lamp-voice`'s `coordinator::tests::endpoint_session`, named
  exactly, `endpoint_fixture_reaps_on_panic_before_and_after_handoff` (the
  module's other 28 left on 2026-10-17, `hung_worker…` later that day, and
  `coordinator::tests::early_end` and `coordinator::tests::pilot_faults` on
  2026-10-16, below);
- one single test that took 4 s or more outside those, named exactly:
  `lamp-health::program`'s
  `between_two_healths_five_seconds_apart_the_program_is_not_taken_for_gone`
  (`lamp-actions::server`'s light test and `lamp-memory::health`'s
  `the_environment_alone_does_not_enable_reporting_or_create_bus_data` left
  on 2026-10-17, below);
- tests with a real-clock bound that a loaded runner misses: none now. A
  test goes here only until it gets its own time; the bound the lamp runs
  with is never raised for it. `lamp-voice`'s `coordinator::motor::tests::native_head`
  was here (a motor packet refused as older than `MAX_AGE_US`, runs
  38797329569 and 38776871505). It left once its tests ran parent and child
  on one stepped clock and a packet's age was read after it arrives (#900,
  #988, #1004), so pull requests run it again.

Because whole binaries and modules are matched, a new test added to one of
them joins the group: it runs one at a time, on pull requests too. Until
2026-10-17 it was skipped on pull requests and merges and first ran on the
night after it was merged.

## Running it

From `lampOS/`, the group alone, one test at a time as on the runner:

```
cargo nextest run --release --workspace --profile realtime -j 1
```

What a pull request runs: `--profile pr`, every test, the group one at a
time. Everything one test at a time, as the full run:
`cargo nextest run --release --workspace -j 1`. `cargo test` is unaffected
and runs everything.

## Every run runs the group (#823)

Once most of the group had moved onto test clocks, 9 tests were left, and 8
once memory's env-alone test leaves too (#1283). They wait on the real clock
by design: `capture_hold_worker` plays the production capture loop in real
10 ms ticks, `lamp-health::program`'s test runs the real program's 5 s
period against the bus's real 3 s lease, and `endpoint_fixture_reaps` needs
the real 1.5 s grace before a kill. A test clock in the capture loop was
rejected: it is the lamp's most sensitive path, for about 18 s a night (the
first builder on #823, 12:21). But while the group was skipped, a pull
request that changed the capture loop never ran these tests.

So the experiment #1292 (not merged) let pull requests run the group too, one
at a time beside the rest, to see whether the real-time bounds hold beside a
full core load. Five ARM64 runs, all green, every group test passing each
time, with the same times within 30 ms:

| Run | Group started | Tests | Test phase |
|---|---|---|---|
| 39005954204 | where nextest's order puts it | 6,379 | 128.7 s |
| 39006507964 (by hand) | where nextest's order puts it | 6,379 | 129.1 s |
| 39005954204, re-run | where nextest's order puts it | 6,379 | 130.2 s |
| 39008360397 | first (`priority = 100`) | 6,407 | 133.7 s |
| 39008376363 (by hand) | first (`priority = 100`) | 6,379 | 125.8 s |

The group's 9 tests took 39.9 s one after another (the 8 that stay, 35.4 s).
The other runs that day took 116 to 118 s for about 6,370 tests without the
group. In nextest's own order `capture_hold_worker` came last, from test
5,745 of 6,379 on, and ended the run. With `priority = 100` (nextest 0.9.91
on; the runner had 0.9.148) the group starts first and was done by about test
2,770. The rest of what it costs is the core it holds while it waits, about
10 s of a 10 to 16 minute job.

Since 2026-10-17 the `pr` profile's filter is `all()`, and the group's
override has `priority = 100`. The config requires nextest 0.9.91 or later
(`nextest-version`), so an older install stops and says so.

Seven green runs show the bounds held, not by how much. So the group's
override also has `success-output = "final"`: every run ends with the
group's own output, passing or not. `capture_hold_worker`'s two hold tests
print the medians they bound ("read … us after the block was there, handed
on … us after the read"): under 10 ms, and 35 to 60 ms with a hold of four
blocks or under 10 ms without. A drift toward a bound then shows in the log
before it fails a run (the reviewer's P3 on #1299). The workflow's table (DevOps' file) still says
pull requests skip the group until DevOps changes it.

## Slow-transport wait entry point (#823)

`fake_lamp::slow_transport_with_wait` accepts a fallible `FnOnce(Duration)`
after the original argument parsing. It passes the complete millisecond delay
once, then uses the original real `exec`, arguments, and inherited streams.
A wait error returns unchanged before exec. The default `slow_transport`
still performs its one host sleep; neither entrypoint adds a clock sample.

This is preparation only. It adds no CLI or global switch and migrates no
test: coordinating a delayed relay still needs an explicit bounded parent
wait and proof that the real exec reached the relay before runtime stepping.

## Relay clock preparation (#823)

`lamp_voice_eval::physical::session::run_with_clock` accepts an explicit
`Fn() -> lamp_turn_owner::MonoTime`. It reads that source for relay protocol
timestamps, the session deadline and the SIGTERM grace, including a fresh
sample after a successful signal. A body `Clock` can be passed through
`|| clock.now()`. Clock reads must not advance time, and the relay and runtime
must use the same monotonic domain.

`session::run` remains the system-clock entrypoint used by the CLI. The 2 ms
relay poll, 10 s stop grace, process-group cleanup and late-stdout drain keep
their existing bounds and behavior; cleanup and the drain stay on host time.
No current CLI option selects a test clock.

[Boundary regressions and local gate](verification/relay-clock-20261015/README.md):
287 evaluator tests passed, including the unchanged realtime cases.

This is preparation only: it does not shorten the existing realtime tests.
Stepping a whole evaluator attempt still needs coordinated runner, relay,
child and player wakeups and acknowledgements, plus recorder/transport
participants where present. Their original logical durations, test identities
and assertion bounds must remain unchanged.

## Bringing a test back

A test leaves the group when it no longer waits on the real clock: an
injectable clock, a paused tokio clock, a time scale on a fake, or a shorter
fixture, without weakening what it asserts. Remove it from both filtersets
in the same pull request, and update this list. When the group is empty,
delete it, the `realtime` profile, the `full` input and the nightly
workflow, and this page.

Review points:

- How often does the nightly full run fail on a realtime test, and how long
  until it is fixed? A failure names a day's merges, not one pull request.
- Who decides that a red full run holds the next merge? The workflow only
  opens the issue; it does not block merging.
- Does a pull request's run list the expected number of tests? Since
  2026-10-17 the `pr` profile skips nothing: nextest's "skipped" line no
  longer says "via profile.pr.default-filter". Before, it said "including 2
  tests and 1 binary": the named single test, the named `endpoint_session`
  test, and `capture_hold_worker` as one binary.

## The measured list

Seconds per test on the ARM64 runner, 2026-10-15 (the longer of the two
runs' times when both ran it). Binary, then test. The rows stay as measured
then. A test that has since left the group, such as the 19 of
`coordinator::tests::early_end` on 2026-10-16, is named in its own section
below, with the day it left.

| s | Binary | Test |
|---|---|---|
| 65.0 | `lamp-voice-eval::serve_session` | `normal_eight_turn_conversation_in_one_serve_session` |
| 63.6 | `lamp-voice-eval::serve_session` | `stale_duplicate_reordered_foreign_and_lost_cues_end_the_conversation` |
| 48.3 | `lamp-voice-eval::serve_session` | `a_runtime_stop_mute_or_ignored_sigterm_is_retained_and_reaped` |
| 34.7 | `lamp-voice-eval::serve_session` | `a_provider_outage_is_waited_out_by_event_and_a_terminal_link_ends_it` |
| 34.1 | `lamp-voice-eval::physical_loopback` | `directed_missing_or_stale_retirement_never_falls_back_to_a_timer_or_final_trace` |
| 34.1 | `lamp-voice-eval::physical_loopback` | `missing_or_stale_whole_completion_withholds_followup_after_valid_retirement` |
| 30.3 | `lamp-voice-eval::serve_session` | `delayed_readiness_is_awaited_by_event_not_by_completion` |
| 26.7 | `lamp-voice-eval::serve_session` | `a_late_first_copy_or_failed_player_withholds_every_later_turn` |
| 26.1 | `lamp-voice-eval::physical_loopback` | `directed_interruption_during_second_occurrence_preserves_owner_and_yields` |
| 26.1 | `lamp-voice-eval::physical_loopback` | `directed_natural_interruption_runs_during_reply_with_original_cancel_reason` |
| 25.1 | `lamp-voice-eval::physical_loopback` | `directed_two_occurrences_wait_for_true_completion_before_followup` |
| 24.1 | `lamp-voice-eval::physical_loopback` | `directed_followup_waits_for_whole_reply_completion_then_admits_a_new_turn` |
| 24.1 | `lamp-voice-eval::physical_loopback` | `physical_next_turn_ready_follows_lamp_voice_readiness_cues` |
| 24.0 | `lamp-voice` | `provider_worker::tests::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity` |
| 23.1 | `lamp-voice-eval::serve_session` | `a_declared_capture_end_before_the_next_prompt_and_its_window_withholds_it` |
| 21.7 | `lamp-voice-eval::serve_session` | `startup_cues_stay_fresh_through_a_slow_transport_and_a_slow_first_preparer` |
| 14.1 | `lamp-voice-eval::physical_loopback` | `cue_triggered_acknowledgment_is_injected_during_playback_and_scored` |
| 14.0 | `lamp-voice-eval::physical_loopback` | `echo_only_window_passes_through_the_relay_without_extra_admissions` |
| 14.0 | `lamp-voice-eval::physical_loopback` | `undelivered_stimuli_are_withheld_not_scored_as_silence` |
| 13.0 | `lamp-voice-eval::serve_relay` | `a_runtime_that_ignores_sigterm_is_force_killed_and_never_clean` |
| 12.7 | `lamp-voice-eval::serve_session` | `an_extra_candidate_during_the_wait_withholds_the_rest` |
| 10.8 | `lamp-voice` | `coordinator::tests::early_end::an_answer_that_arrives_after_the_lamp_is_sure_begins_400_ms_sooner_than_without` |
| 10.5 | `lamp-voice-eval::serve_session` | `the_runner_forwards_pilot_runtime_settings_through_the_relay` |
| 10.5 | `lamp-voice-eval::serve_session` | `a_nonzero_runtime_exit_never_completes_a_serve_attempt` |
| 10.5 | `lamp-voice-eval::serve_session` | `serve_readiness_comes_only_from_the_fresh_cue_never_from_stdout` |
| 9.4 | `lamp-voice` | `coordinator::tests::endpoint_session::a_worker_failure_falls_back_through_recovery_and_then_resumes` |
| 8.3 | `lamp-voice` | `coordinator::tests::pilot_faults::rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across` |
| 7.0 | `lamp-memory::health` | `legacy_reports_retained_counts_and_stays_well_after_caller_refusals` |
| 7.0 | `lamp-memory::health` | `a_failed_legacy_write_stays_warn_through_reads_until_a_write_succeeds` |
| 7.0 | `lamp-memory::health` | `sqlite_reports_retained_counts_and_stays_well_after_caller_refusals` |
| 7.0 | `lamp-memory::health` | `a_working_bus_that_fails_stays_failed_until_publication_really_recovers` |
| 6.4 | `lamp-voice` | `coordinator::tests::early_end::the_link_dropping_in_between_fails_the_request_unheard_and_the_next_is_answered` |
| 6.4 | `lamp-voice` | `coordinator::tests::early_end::speech_resumed_at_three_tenths_nothing_of_the_first_answer_is_played_and_one_turn_is_heard` |
| 6.4 | `lamp-voice` | `coordinator::tests::early_end::speech_resumed_at_three_tenths_under_the_lamps_duplex_policy` |
| 6.4 | `lamp-voice` | `coordinator::tests::early_end::without_the_option_every_message_and_record_is_what_it_was` |
| 6.4 | `lamp-voice` | `coordinator::tests::endpoint_session::hung_worker_never_delays_capture_endpoint_or_session_end` |
| 6.4 | `lamp-voice` | `coordinator::tests::endpoint_session::verdicts_after_the_result_timeout_fall_back_on_every_turn` |
| 6.1 | `lamp-voice-eval::serve_session` | `a_recorder_that_exits_zero_mid_conversation_withholds_the_rest_and_certifies_nothing` |
| 6.1 | `lamp-voice-eval::serve_session` | `a_room_recording_that_ends_early_withholds_the_rest` |
| 6.0 | `lamp-voice-eval::serve_relay` | `the_hard_deadline_stops_the_runtime_and_is_an_error` |
| 5.4 | `lamp-voice` | `coordinator::tests::early_end::a_lone_lamp_and_a_pause_and_the_question_go_as_one_request` |
| 5.4 | `lamp-voice` | `coordinator::tests::early_end::lamp_stop_over_an_answer_stops_it_and_only_the_answer_to_the_whole_is_played` |
| 5.4 | `lamp-voice` | `coordinator::tests::endpoint_session::duplex_complete_verdict_ends_at_one_second_and_its_answer_plays` |
| 5.4 | `lamp-voice` | `coordinator::tests::endpoint_session::a_complete_verdict_ends_at_one_second_and_its_answer_plays` |
| 5.4 | `lamp-voice` | `coordinator::tests::endpoint_session::rapid_resumes_never_end_the_turn_early` |
| 5.2 | `lamp-voice` | `provider_worker::tests::review_2026_10_14::review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer` |
| 5.1 | `lamp-voice::capture_hold_worker` | `another_microphone_is_handed_on_at_once_and_its_early_sound_is_not_accounted_for` |
| 5.1 | `lamp-voice::capture_hold_worker` | `mic1_frames_wait_four_blocks_with_their_own_times_and_a_stop_drops_what_is_held` |
| 5.0 | `lamp-voice-eval` | `physical::mac::audit_grant_tests::audit_transport_that_outlives_its_bound_is_killed_and_never_certified` |
| 5.0 | `lamp-health::program` | `between_two_healths_five_seconds_apart_the_program_is_not_taken_for_gone` |
| 5.0 | `lamp-records::records` | `real_reporter_reloads_switch_and_policy_and_requires_a_matching_receipt` |
| 5.0 | `lamp-voice` | `coordinator::tests::pilot_faults::a_turn_under_way_when_paused_is_finished_and_the_next_is_not_taken` |
| 4.9 | `lamp-voice` | `coordinator::tests::endpoint_session::a_worker_still_loading_leaves_the_baseline_window` |
| 4.5 | `lamp-voice` | `provider_worker::tests::kept_start::with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end` |
| 4.5 | `lamp-memory::health` | `the_environment_alone_does_not_enable_reporting_or_create_bus_data` |
| 4.4 | `lamp-voice` | `provider_worker::tests::review_2026_10_14::review_sound_a_name_heard_as_a_request_began_lets_out_the_answer_the_rule_would_hold` |
| 4.4 | `lamp-voice` | `coordinator::tests::early_end::an_answer_that_arrives_before_the_lamp_is_sure_begins_at_that_moment` |
| 4.4 | `lamp-voice` | `coordinator::tests::early_end::an_answer_with_nothing_said_before_the_lamp_is_sure_is_never_played` |
| 4.4 | `lamp-voice` | `coordinator::tests::endpoint_session::duplex_missing_model_session_ends_on_the_unchanged_silence_fallback` |
| 4.4 | `lamp-voice` | `coordinator::tests::endpoint_session::missing_model_session_ends_on_the_unchanged_silence_fallback` |
| 4.4 | `lamp-voice` | `coordinator::tests::early_end::quiet_to_the_end_the_answer_starts_when_the_lamp_is_sure_and_not_before` |
| 4.4 | `lamp-voice` | `coordinator::tests::endpoint_session::complete_verdict_ends_early_but_resumed_speech_keeps_the_turn` |
| 4.2 | `lamp-voice` | `coordinator::tests::pilot_faults::failure_notice_plays_once_keeps_the_failure_and_yields_to_the_next_owner` |
| 4.1 | `lamp-actions::server` | `the_light_is_offered_when_its_owner_appears_and_withdrawn_when_it_goes` |
| 4.1 | `lamp-voice` | `provider_worker::tests::resumed_question_survives_a_silent_service_and_the_assumption_is_reported` |
| 3.6 | `lamp-voice` | `coordinator::tests::pilot_faults::resumed_the_lamp_answers_again` |
| 3.0 | `lamp-voice` | `coordinator::tests::pilot_faults::failure_notice_yields_mid_play_to_a_new_owner_under_vad_only` |
| 3.0 | `lamp-voice-eval::serve_relay` | `a_nonzero_runtime_exit_after_clean_metadata_is_a_relay_error` |
| 3.0 | `lamp-voice-eval::serve_relay` | `a_runner_stop_is_one_sigterm_and_a_clean_reaped_end` |
| 3.0 | `lamp-voice-eval::serve_relay` | `the_pilot_runtime_settings_reach_the_serve_child_exactly` |
| 3.0 | `lamp-memory::health` | `an_unusable_bus_does_not_prevent_serving_and_reports_recover_when_it_returns` |
| 3.0 | `lamp-voice-eval::serve_relay` | `a_closed_runner_transport_stops_the_runtime_through_the_same_path` |
| 2.9 | `lamp-voice-eval::final_dispatch` | `a_relay_framing_error_first_seen_in_the_final_drain_withholds` |
| 2.8 | `lamp-voice-eval::final_dispatch` | `an_invalid_cue_first_seen_in_the_final_drain_withholds` |
| 2.8 | `lamp-voice-eval::final_dispatch` | `an_unobserved_event_in_the_final_drain_requires_revalidation` |
| 2.8 | `lamp-voice-eval::final_dispatch` | `a_session_end_first_seen_in_the_final_drain_withholds_and_is_kept` |
| 2.6 | `lamp-voice` | `coordinator::tests::pilot_faults::sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::camera_loss_during_an_answer_revokes_only_sight` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::control_one_question_is_answered_once_then_mute_ends_the_run` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::readiness_publishes_the_link_state_it_was_validated_against` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::ring_fault_during_an_answer_keeps_the_conversation_and_cleans_up` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::duplex_control_answers_once` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::ring_lost_without_blanking_keeps_the_answer_but_cleanup_fails_truthfully` |
| 2.5 | `lamp-voice` | `coordinator::tests::pilot_faults::provider_failure_during_output_never_becomes_a_completed_answer` |
| 2.4 | `lamp-voice-eval::final_dispatch` | `a_final_drain_that_exhausts_its_budget_requires_revalidation` |
| 2.3 | `lamp-voice` | `coordinator::tests::pilot_faults::terminal_provider_link_during_output_settles_failed_and_ends_service` |
| 2.2 | `lamp-voice-eval::final_dispatch` | `a_healthy_session_still_dispatches_once` |
| 2.2 | `lamp-voice` | `coordinator::tests::pilot_faults::partial_write_then_stall_is_bounded_under_v1_baseline` |
| 2.2 | `lamp-voice` | `coordinator::tests::pilot_faults::control_progress_without_settlement_is_bounded_under_v1_baseline` |
| 2.2 | `lamp-voice` | `coordinator::tests::pilot_faults::duplex_stalled_speaker_is_bounded` |
| 2.2 | `lamp-voice` | `coordinator::tests::pilot_faults::stalled_speaker_during_output_is_bounded_and_never_completes_the_answer` |
| 2.1 | `lamp-voice::capture_hold_worker` | `mic1_muted_while_frames_are_held_hands_none_of_them_on` |
| 2.1 | `lamp-voice-eval::final_dispatch` | `audit_physical_boundary_does_not_dispatch_during_post_exit_trace_relay` |
| 2.1 | `lamp-voice-eval::final_dispatch` | `audit_physical_boundary_rejects_a_target_already_over_100ms_late` |
| 2.1 | `lamp-voice-eval::final_dispatch` | `a_known_invalid_cue_withholds_dispatch_as_a_protocol_failure` |
| 2.0 | `lamp-voice-eval::final_dispatch` | `a_known_session_end_withholds_dispatch` |
| 2.0 | `lamp-voice-eval::final_dispatch` | `the_first_terminal_cause_wins_and_a_provider_failure_stays_provider_unavailable` |
| 2.0 | `lamp-voice-eval::final_dispatch` | `a_known_run_end_cue_withholds_dispatch` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::provider_process_lost_before_output_ends_the_run_without_speech` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::provider_process_lost_during_output_stops_speech_and_fails_the_turn` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::mute_while_answer_speech_is_queued_cuts_it_and_never_resumes` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::mute_while_the_failure_notice_plays_cuts_it_and_keeps_the_failure` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::capture_fault_during_output_cancels_the_answer_and_ends_the_run` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::sigterm_while_answer_speech_is_pending_stops_cleanly` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::sigterm_while_the_failure_notice_plays_stops_cleanly` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::provider_failure_before_output_settles_failed_without_speech` |
| 2.0 | `lamp-voice` | `coordinator::tests::pilot_faults::provider_worker_fault_during_output_fails_the_turn_and_ends_the_run` |
| 1.6 | `lamp-voice::capture_hold_worker` | `a_microphone_busy_for_a_moment_at_start_is_opened_once_it_is_free` |
| 1.6 | `lamp-voice` | `coordinator::tests::pilot_faults::a_paused_lamp_takes_no_question_and_says_why` |
| 1.6 | `lamp-voice` | `coordinator::tests::pilot_faults::stalled_speaker_under_vad_only_is_bounded_too` |
| 1.6 | `lamp-voice` | `coordinator::tests::pilot_faults::control_progress_without_settlement_is_bounded_under_vad_only` |
| 1.6 | `lamp-voice` | `coordinator::tests::pilot_faults::partial_write_then_stall_is_bounded_under_vad_only` |
| 1.1 | `lamp-voice` | `coordinator::tests::early_end::a_name_note_told_with_a_request_ended_early_does_not_survive_it` |
| 0.8 | `lamp-voice-eval::final_dispatch` | `a_transport_closed_during_the_final_drain_withholds` |
| 0.7 | `lamp-voice` | `coordinator::tests::endpoint_session::unconfirmed_endpoint_exit_fails_mute_cleanup_and_never_reopens_service` |
| 0.7 | `lamp-voice` | `coordinator::tests::pilot_faults::capture_stall_during_a_question_ends_the_run_without_an_answer` |
| 0.6 | `lamp-voice` | `coordinator::tests::endpoint_session::mute_then_reopen_never_overlaps_endpoint_workers` |
| 0.5 | `lamp-memory::health` | `a_store_that_cannot_open_never_claims_readiness_and_any_delivered_report_is_warn` |
| 0.3 | `lamp-voice` | `coordinator::tests::endpoint_session::confirmed_endpoint_exit_keeps_the_restartable_mute` |
| 0.0 | `lamp-voice-eval::physical_loopback` | `a_transport_that_closes_mid_trace_invalidates_the_attempt` |
| 0.0 | `lamp-voice-eval::physical_loopback` | `directed_mode_keeps_provider_fault_and_asset_restrictions` |
| 0.0 | `lamp-voice` | `coordinator::tests::pilot_faults::distinct_rejects_an_item_that_returns_later` |
| 0.0 | `lamp-voice` | `coordinator::tests::pilot_faults::speaker_progress_is_due_strictly_after_lease_plus_report_age` |

## Prepared fake-player clock source

`FakePreparedRoomPlayer { room, faults, prepared }.with_clock(...)` accepts a
pure, cloneable, thread-safe `Fn() -> MonoTime`. Each worker owns a clone; all
clones must read the same clock domain without advancing it. Attempt identity,
readiness, grant receipt and first copy retain four distinct clock samples.
The first-copy sample still refuses a grant at equality with its expiry.

Both paths share the original preparation implementation. Plain struct
construction keeps the host clock's full nanosecond precision. The explicit
clock uses checked microsecond-to-nanosecond conversion: overflow before
thread creation returns an error; later overflow returns an invalid failed
report identifying the sample stage, without sending the room datagram.
Host resource polling, fault delays, grant waits, validation, cancellation,
datagram sends and joins retain their existing behavior.

Focused tests synchronize through the existing ready and grant channels and
join the worker before checking delivery. Scripted read-order probes cover
fresh first-copy expiry and overflow; these probes are instrumentation, not
scheduler clocks. Existing tests are unchanged. This #823 seam does not
migrate a serve session or establish a speedup.

## Dispatch fixture shutdown

The 13 final-dispatch cases keep their assertions and timed evidence. The
stay-open shell transport now consumes control lines silently and exits on
stdin EOF from the existing backend Drop, avoiding its two-second kill
fallback on twelve cases. This replaces the helper's independent 30-second
lifetime; it does not change a production deadline or accelerate the remaining
timed stimuli. The selected group is unchanged.

[Local serial comparison](verification/dispatch-transport-20261015/README.md):
13 passed before and after, 29.938 s to 6.487 s on x86_64 macOS. This is one
fixture cleanup step in #823; the whole-group two-minute goal remains open.

## Recorder clock and wait preparation (#823)

`physical::fake_lamp::room_recorder_with_clock_and_wait` accepts a pure
nanosecond clock and one wait adapter for the recording-failure fixture. The
optional declared capture end samples that clock before the actual
`ready.json` write. After the write, the adapter receives the full original
wait duration and the same clock, so it can take a separate fresh sample for
its wait start. The declared capture end does not establish when the recorder
exits. Reading the clock must not advance time.

The ordinary `room_recorder` entrypoint still uses `lamp_ipc::monotonic_ns`
and one host sleep for `min(fail_after_ms, seconds * 1000)` milliseconds. Its
arguments, optional sample count, arithmetic, private directory and ready-file
contents, and configured return code are unchanged. This failure fixture
still writes no audio or final `metadata.json`, including when its return
code is zero. A function return does not replace observing the owned child
process's actual exit and reap.

This seam does not select a new CLI path, add a recorder participant, alter
backend readiness or cleanup bounds, migrate the three recorder integration
cases, or establish a speedup. Those cases retain their original bodies and
assertions. Coordinated recorder progress still needs a separate ready-write
acknowledgement, its full wait, and an actual-exit barrier in the shared clock
domain before it can join a controlled serve attempt.

## Evaluator backend clock entry point

`PhysicalBackend::new_with_clock` accepts a pure `Fn() -> MonoTime` for
runner timestamps, clock-map samples, cue freshness, grants, dispatch and
pre-end logical deadlines. `PhysicalBackend::new` keeps the existing system
clock. `FakeRoomPlayer { room }.with_clock(...)` uses the same source for its
immediate delivery report; the original struct construction still uses the
system clock and both paths share the datagram/report implementation. The
microsecond-to-nanosecond report conversion rejects overflow before delivery.

This prepares #823's coordinated evaluator fixture. It does not advance time,
replace the existing channel waits or sleeps, or accelerate any existing test.
Every participating relay, runtime and player must share a clock before a
scenario can use a controlled time source. Prepared players and recorders have
not been migrated. The 10 s runtime self-end wait and the overall pre-end bound
use the runner clock; the 300 ms drain after `session_end`, actual child exit,
25 s player join, 5 s unused-player join, recorder readiness and all cleanup
bounds remain on host time, with their existing limits.

Focused regressions cover pure reads, separate ping/receipt/cue-decision
samples, a completed session drained and reaped with a frozen clock, identical
fake-room datagrams and report shape, timestamp overflow refusal, and the
unchanged system defaults. Existing test assertions, identities, profiles and
workflows remain unchanged. This entry point alone establishes no speedup.

## Stepped finite loopback fixtures

The finite `physical_loopback` scenarios now select an explicit
`physical::fixture_driver::ClockedFixture`. The runner, immediate fake player,
real relay process and finite fake runtime share a pure `MonoTime` source.
Only the parent scheduler advances it. The two quick transport/restriction
checks keep their original construction. All 12 identities, all 100 assertion
macros, the original scenario durations, fault iterations, cue freshness and
grant bounds remain unchanged.

The driver recognizes only the evaluator's direct local fake-runtime command.
It selects `fake-clock-session` and `fake-clock-lamp-voice` with a private
control socket. Regular commands and constructors keep their system clocks,
2 ms host polling and existing reader bounds. Serve, prepared players,
recorders, arbitrary transports and reuse for another attempt are rejected by
this finite driver; they need their own coordinated participants.

Each control round names its time and the complete input frames it must
consume. The runtime polls first, then the relay consumes the acknowledged
runtime stdout and runner controls. Only successful room sends count as
expected runtime input. The relay acknowledges complete output after flushing;
the backend reader must actually enqueue that output before the parent moves
on. `next_event` drains queued frames before advancing. A scheduled injection
preserves its existing wait and final-drain ordering. Reads never move time,
and the original 2 ms runtime/relay and 10 ms model tick intervals remain.

The private socket uses bounded newline-delimited frames, role/round/counter
checks and absolute host watchdogs. Nonblocking reads/writes use bounded
readiness polling, so queued final frames remain readable after peer closure.
A failed participant closes control peers before
the backend's existing process cleanup. Terminal runtime progress freezes
logical time while the relay observes actual child exit and streams the final
trace. Only the relay's final flushed `Trace`/`SessionEnd` output completes its
acknowledgement; logical completion never substitutes for OS reaping.

Final trace output can exceed the normal 4,096-line reader queue while the
parent waits for that acknowledgement. The explicit fixture reader therefore
accepts at most 50,404 transcript records plus one reserved error slot, using
nonblocking enqueue. That bound covers a maximum 25,000-line trace, up to
25,000 cues, 400 pings and four control/fault records. The existing per-line and 96 MiB
byte caps still apply. Overflow publishes a terminal error and closes the
reader instead of blocking the child before its acknowledgement. The default
reader retains its original queue and blocking behavior.

[Fresh corrected-channel release evidence](verification/finite-loopback-clock-20261015/README.md)
passes the same 12 compiled tests before and after: **239.720 s to 11.439 s**,
serial nextest on x86_64 macOS, with no retries, ignored or skipped cases.
The evaluator helper hash is identical; only the authorized test constructors
change. Nextest execution excludes build/list time. This is an observed pair
on a shared host with uncontrolled independent load, not a quiet benchmark.

The corrected full local gate passed 352 evaluator tests before rebase. All
57 evaluator Rust files remain byte-identical across rebase; new qualification
passed 150 release library tests, evaluator all-target clippy, formatting,
workspace build and the release pair. The full 352-test run was not repeated
after rebase. The earlier 248.575 s to 28.471 s debug pair remains historical
pre-#879 evidence. Native ARM64 speed and the complete 123-test under-two-minute
target are not established. Profiles and workflow selection are unchanged.

## Finite fake-runtime clock and poll boundary

`physical::fake_lamp::run_with_clock_and_poll` is an explicit library seam for
coordinated finite fixtures. The clock callback returns `MonoTime` without
advancing it. A separate poll hook runs after the original tick, event/stdout
drain, production `CueSink` scan, fault relay and room-input consumption. The
snapshot counts complete flushed stdout lines, successfully decoded and
injected room datagrams, and trace records. Its poll number advances even when
time is frozen, so a sender's successful datagram write cannot masquerade as
runtime consumption.

The wrapper keeps the original base sample and fresh clock reads for the tick,
CueSink and fault relay. Runtime ticks remain 10 ms; the stale-cue fault still
holds the original bytes for 150 ms with their original 100 ms expiry. The
existing `run` and CLI keep system time, the two-millisecond sleep between
continuing polls, and their existing serve path. The controlled entrypoint
rejects serve before producing output because that path still needs
coordinated clock advancement and acknowledgements of completed polls. Its
clock source alone does not provide that control. No CLI flag or environment
variable enables this entrypoint.

A hook may acknowledge completed work and park until the coordinator's next
poll, with a separate bounded host watchdog supplied by that coordinator. The
terminal poll reports that the simulated runtime ended; the final trace is
written and synced only after the hook returns. That snapshot does not prove
file durability or subprocess exit. The outer fixture must still wait for the
actual child and relay completion. Focused tests cover frozen-time polling,
the exact tick boundary, consumed room input before its next tick, production
cue delivery and the unchanged stale-completion delay, serve refusal, and hook
failure propagation. This preparatory seam does not migrate the loopback
identities or establish a speedup.

## Serve fixture clock source

The offline `physical::fake_serve` runtime exposes
`run_with_clock(FakeServe, Fn() -> MonoTime)`. Its existing `run` entry point
still supplies the host monotonic clock. The caller owns time advancement;
reading the supplied clock does not advance it. The nine existing logical
clock reads use this same source, including separate fresh samples for cycle
metadata and the runtime's origin. Retained startup readiness keeps its
150 ms age and saturates at zero, as before.

The 2 ms host sleep, SIGTERM/SIGINT handling, room and cue sockets, fault
behavior, metadata flushes and final stdout receipt retain their existing
behavior. The focused clock tests call cycle helpers directly, without
registering process-wide signal handlers. They check repeated reads at the
same instant, distinct startup samples, restart identity and retained-history
timestamps. The 14 existing `serve_session` cases and their assertions remain
unchanged.

This is a clock-source seam for #823, not a serve-session speed measurement.
The CLI still uses the system clock. A later controlled serve scheduler must
coordinate prepared-player readiness and grants, recorder and delayed
transport progress, signal observation, final metadata output and actual
child exit before those cases can use logical time. A logical end marker is
not evidence that the process has exited.

## Body's live-hand age bound (#902)

`bus_out::tests::a_hand_that_waited_in_the_queue_is_not_said_as_fresh`
(`crates/body/src/bus_out.rs`) failed once on a loaded ARM64 runner (run
38787471620): its live case asserted `expires_ms >= 990`, and the step took
more than 10 ms before the hand was said. The live case now measures its own
interval, from the observation's start to `step` returning, and asserts
`expires_ms + took_ms + 1 >= 1000`: the observation's age can be no more than
that whole interval, and 1 ms covers truncation to whole milliseconds. Nothing
is relaxed for stale hands: the 1.2 s queued sample is still dropped, the
0.6 s one still expires within its residual 400 ms, and a slow preceding
joint-state write still ages the hand
(`a_hand_after_a_slow_joints_write_is_aged_by_it`).

Author's evidence (fifth, a Mac, not a reviewer run): 200 runs of this
test under parallel build load (load average about 25), no failure.

## Expression's runtime fixtures and the wall clock (#912)

The reviewer's full serial gate on #844 failed six Expression tests on
stale inputs (joints 1,263 ms and 2,301 ms old at the step):
`real_service_subscribes_to_exactly_one_complete_voice_protocol`
(`tests/rest_replay.rs`), and in `tests/runtime.rs`
`enabled_output_uses_a_new_session_and_expires_after_input_loss`,
`paused_by_the_keeper_nothing_is_asked_of_the_body_or_the_light_until_resumed`,
`without_a_pause_said_expression_asks_exactly_as_before` and the two
`sleep_name` cases. The service judges each input's age by the wall clock it
is given against the bus's own publication stamp, and four of these tests
read that clock only after making the service, with the inputs already
published.

What changed, tests only:
- `populate` returns the wall clock read just after its last publication,
  and the tests step with it (and `+ 2_000` where they step 2 s later). The
  sleeping-name fixture and the real-service case read it the same way. A
  stall after that read, making the service or anything else, no longer ages
  the inputs.
- Each fixture publishes from the loosest limit to the strictest, so the
  inputs with short limits go back to back just before the clock read:
  `populate` the light (a setting, no age), the voice timing (500 ms), the
  joints (300 ms in the engine); the sleeping-name fixture the room (2 s),
  the voice's progress and the faces (500 ms), the joints; the real-service
  case the room and the light, the progress (500 ms), the joints, the
  playout and sound (200 ms).
- Nothing in the service changed. The deliberately stale and future inputs,
  the expiry after input loss (`wall + 2_000`), the restart that must not
  reuse old joints, and the pause's one-second look are all as before.

The limit that remains: a stall inside a fixture's own publications, after
an input with a short limit and before the clock read, longer than that
limit (200 to 500 ms), still fails. Two injected 1.3 s delays at
`fe8dd0ebd`, both in throwaway copies:
- Third's, after the first publication of each fixture (before the
  reorder): `lamp-voice` `48b3737d5` failed exactly the six, `fe8dd0ebd`
  passed all 34.
- The reviewer's, after the faces in the sleeping-name fixture's first step:
  the old order failed 6 of 34 and `fe8dd0ebd` failed the 2 `sleep_name`
  cases, the faces being 500 ms long.

With the reorder the faces are followed only by the joints' publication, so
that window is a single publication, not removed.

A second limit, found on ARM64 (#1196): the service places each bus input at
its injected `now` less its wall age, and the sleeping-name fixture moved
`now` on by exactly one 200 ms tick a step. A stall of more than a tick
anywhere in a step's publications made the room older by more than the clock
had moved, so it was placed before the previous step's room and refused as
out of order, long before its 2 s limit. The fixture now reads the wall clock
before publishing too, and when publishing took longer than a tick its clock
moves on by the excess, as a lamp's would. No placed time goes backwards
whatever the stall; below a 200 ms stall the clock moves exactly as before. A
300 ms stall after the room and a 250 ms stall after the joints, in a
throwaway copy, both pass with it; the 300 ms one failed before, with the
refusal seen on ARM64. A stall longer than an input's own limit is still
refused as stale, as it would be on a lamp. The activity `speech()` sends
has only 50 ms of slack: it is 250 ms old by design against its 300 ms
limit, so in the step after it a publishing stall of 250 ms or more is
refused, as it was before (the reviewer on #1198).

## Coordinated Serve-session fixtures (#823)

`ClockedServeFixture` coordinates the real relay process, fake Serve runtime,
prepared-player threads, optional fake recorder and delayed local transport.
All fourteen original `serve_session` identities use this explicit clock,
including their twenty-two attempts. Their complete test bodies, 114 assertion
macros and original fault inputs remain unchanged. Ordinary fake commands keep
their host clocks; control requires the explicit `fake-clock-serve-session`,
`fake-clock-serve-runtime`, `fake-clock-room-recorder` and
`fake-clock-slow-transport` commands.

Clock reads remain pure. Advances retain the original 2 ms poll boundaries and
require actual completed work and counted pipe, cue and room-input progress.
Prepared readiness and expiring grants still use the original channels, real
room datagrams and 100 ms lateness bound. A continuing relay's successful
SIGTERM send requires actual runtime signal observation at the next original
poll. A completed terminal relay stops further runtime steps, including after
forced kill. Runtime completion holds logical time during final metadata,
relay completion and child exit.
A terminal progress receipt does not prove an OS process or prepared worker has
exited. Bounded host guards, real kill/reap and thread completion remain separate.

The recorder adapter uses the existing parser and clock/wait entry point. Its
first progress receipt follows the actual `ready.json` write; the backend still
reads that file through its bounded parser under the original 15 s readiness
guard. The three recorder controls retain their full 6 s, 6 s and 23 s waits.
The optional declared 20 s capture end is not an exit receipt. A terminal
recorder acknowledgement freezes time until the backend observes its owned
`Child` exit; recording coverage and validity still use the original checks.
After relay completion, recorder and prepared-player work can drain at the
original 2 ms cadence, with recorder exit confirmed before player completion.

The delayed-transport control retains the entire 400 ms delay and 2300 ms first
preparation. Its wrapper neither consumes queued stdin nor fabricates stdout.
After acknowledging the completed delay it closes its control participant and
performs the original exec with the original arguments and inherited streams.
The controller separately requires a fresh ServeRelay registration from that
real relay before runtime bootstrap. The delay acknowledgement alone cannot
establish exec, relay readiness or child exit. Startup uses one cumulative host
deadline; later logical rounds cannot reset it, and a shorter preparation
deadline still wins. Recorder due times also bound each logical advance.

This migration covers the original Serve-session family. The six separate
direct `serve_relay` tests retain their existing path. Clock-controlled results
check ordering and the full configured windows; they do not measure hardware
latency or establish a result for the complete original 123-test inventory.

## Serve relay test coordinator

The six `serve_relay` cases use an explicit helper clock while retaining their
original test bodies, runner delays, fault settings and assertions. The relay
and its coordinator run on the test thread; the existing session body still
owns the real child, reader threads, Unix sockets, SIGTERM, process-group kill,
reap, metadata checks and final output. The helper replaces its runner sleep
with the full logical three-second boundary and advances the runtime and relay
only after completed polls, using the original two-millisecond interval.

A real Stop write is counted only after success. Closing the runner produces
actual EOF; the helper holds time until the relay's real stop-signal receipt.
The runtime then observes the actual signal through its existing handler.
Final runtime progress freezes time until the relay observes actual child
exit. The one-second run plus five-second allowance and the full ten-second
ignored-signal grace remain unchanged. Host control, cleanup and late-output
bounds remain active; a held post-stop boundary is checked against a ten-second
host budget before and after polls. The existing reader-enqueue barrier can
still wait up to thirty seconds before returning a failing poll; this is not a
strict ten-second abort deadline. No progress receipt substitutes for a process
status.

The explicit controlled serve command reports its complete received argv,
including its leading control pair, exactly once through the existing flushed
JSON stdout record. Only the validated suffix is parsed as runtime arguments.
Ordinary fake commands and the direct controlled library API retain their
existing argument reporting. The original serve suffix and child-versus-spawn
argv assertion remain unchanged. This implementation needs fresh paired and
native qualification before a speedup can be claimed.

## The reply-buffer capacity test on a paused clock (#823)

`provider_worker::tests::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`
played a twenty-second answer and a four-second hold in real time, 24 s. Its
coordinator ticks were already scenario steps (#1035). Its modeled speaker,
elapsed times and scripted service now read Tokio's clock, and the test runs
on a paused runtime, so it takes about 3 s with the same assertions and the
same values every run (`voice-test-fixtures.md`, "Speaking speed through a
small reply buffer"). It left the group, in all three filtersets, on
2026-10-16.

## Pilot capture stalls on the stepped fixture (#893)

Six `coordinator::tests::pilot_faults` capture-stall cases now select the
existing `Fixture::Stepped`, as `second_route` did (lampOS#890, #944):
`a_half_second_capture_stall_is_a_gap_and_the_voice_goes_on`,
`a_microphone_closed_by_a_stall_reopens_under_a_new_epoch_and_the_voice_goes_on`,
`a_stall_inside_a_question_abandons_it_and_the_voice_goes_on`,
`a_reopen_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on`,
`a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` and
`a_short_xrun_reopen_inside_a_question_abandons_it_and_the_voice_goes_on`.
On `Fixture::Threaded` their privacy peer ran on the host clock, and a loaded
host let it fall behind the 50 ms freshness check. Their assertions, stall
lengths and every production bound are unchanged.

The stepped capture peer sends the blocks a stall held back in one step.
When that burst fills the 64 KiB data socket, the block that did not fit is
no longer lost: it stays due and goes next step with its own acquisition
time, as the real capture worker never skips a sequence. A lost block left a
sequence hole that the coordinator rightly refuses as `invalid original
microphone read interval`. With the socket buffer cut to 40 KiB,
`a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` failed
that way before the change and passed after it.

Still separate: `provider_terminal`'s same cause is #975 (PR #982), and the
original-123 realtime-group denominator above stays frozen.

## Pilot SIGTERM raised on the stepping thread (#739)

`pilot_faults`'s `Act::Sigterm` used to send SIGTERM to the whole process.
Any thread could take that signal, and the handler could run late on a loaded
host. The stepped scenario clock kept moving in the meantime: the stop was
seen 0.27 to 1.7 s of scenario time after it was sent. That let
`sigterm_while_the_failure_notice_plays_stops_cleanly` play its notice to the
final sample, which failed 2 to 6 of 40 runs with 20 `yes` workers. The act
now raises the same SIGTERM on its own thread, which runs the installed
handler before `raise` returns. The production `Termination` path, every
assertion and every bound are unchanged. The three `sigterm_while_*` pilot
cases then passed 40/40 under 20 `yes` workers and 20/20 under SIGSTOP 80 ms
every 300 ms.

## Pilot faults all on the stepped fixture (#973)

#972 moved the six capture stalls above. #973 moves the last threaded
scenario, `rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across`,
so every `coordinator::tests::pilot_faults` scenario runs on the scenario
clock, and the threaded fixture, unused after, is gone. A loaded host cannot
starve a peer past the 50 ms privacy or 100 ms capture lease, because logical
time advances only after every peer has run. Production bounds, stall
lengths and assertions are unchanged.

A run's `started_us`/`finished_us` (first child spawned, every child
reaped) are taken on the host clock: they are real process events, and
`rapid_mute…` compares them across runs, whose logical times are their own.
Its cross-cycle privacy wait was already a separate mock thread. The
SIGTERM raised on the stepping thread is the section above (#739).


## pilot_faults leaves the group (#1128)

Since #972 and #973 every `coordinator::tests::pilot_faults` scenario runs on
the stepped fixture, and none waits on the real clock: the module's 45 tests
took 2.2 s one at a time in full run 38924461963 (29.7 s before). Its timing
assertions are on scenario time, except two host-clock upper bounds in
`sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss`:
the real metadata finish within `FINISH_WAIT` plus 250 ms, and SIGTERM to
every child reaped within 1 s. The module left the group, in all three
filtersets, on 2026-10-16, and runs on every pull request. On the Intel Mac
four copies of the module at once, each running four tests at a time,
passed 12 of 12 runs (540 test runs). The children they start and their
cleanup stay on the host clock, as for the other coordinator fixtures
outside the group. If one of the two host-clock bounds fails on a loaded
pull-request runner, that one test goes back into the group by its exact
name, not the module.

## early_end on one logical clock (#823)

`coordinator::tests::early_end`'s 13 conversations ran in real time: the
coordinator, three peer threads polling every millisecond, and the real
provider worker, Gemini session and scripted service on a runtime of their
own. Each lasted its script's `seconds` (4 to 8; the cut-in case runs twice),
93 s for the module. They now share one logical clock, all on the test's
thread:

- the coordinator and its capture, privacy and speaker peers on
  `scenario_clock`, the peers run inside its step, as in `pilot_faults`;
- the provider worker, its Gemini session and the scripted service on a
  paused current-thread Tokio runtime. Each step first advances it to the
  step's logical instant (`sleep_until`), its timers firing in order on the
  way, and then the peers run. In cleanup the logical instant stands and the
  runtime goes on 2 ms a step, so that the worker takes Stop;
- `provider_link.rs` reads `scenario_clock::monotonic_us` under `cfg(test)`,
  as `provider_worker.rs` does since #1035: the worker's link clock pairs its
  Tokio instants with the voice's microseconds, and in a scope both are
  logical. Unscoped tests and release builds read `lamp_ipc::monotonic_us` as
  before;
- what the service and the peers record (`Seen`) is stamped with
  `scenario_clock::instant()`, which is `Instant::now()` without a scope.

The child processes (`sleep 60`), their start barrier, the coordinator's
cleanup and the 12 s conversation watchdog stay on the host clock. The
assertions, the scripts, the service's timings and every production bound
are unchanged. `a_name_note_told_with_a_request_ended_early_does_not_survive_it`
runs on `#[tokio::test(start_paused = true)]`.

On one Intel Mac, one test at a time: the 19 tests took 93.17 s before and
16.26 s after; the name note 1.12 s and 0.01 s. Each conversation's summary
line (connections, activity starts and ends, audio blocks sent, requests
given up, answers begun, blocks played, last sound to the first block handed
to the speaker) is the same before and after, except that times differ by up
to 6 ms (602 ms where real time gave 601 to 604: the 2 ms step), and in the
cut-in case 94 blocks are played before "stop" cuts in, on every run, where
real time played 91 or 92 (not asserted). Six copies of the module at once
on 16 logical CPUs passed 24 of 24 runs in 23 to 32 s each, and their summary lines
were byte for byte those of an unloaded run.

So `coordinator::tests::early_end` left the group, in all three filtersets,
on 2026-10-16.

## endpoint_session's settled real workers (#823)

Three `coordinator::tests::endpoint_session` tests drive a real end-of-turn
worker that makes no progress during their four-second conversation:
`missing_model_session_ends_on_the_unchanged_silence_fallback` and
`duplex_missing_model_session_ends_on_the_unchanged_silence_fallback` (the
worker reports a missing model and exits) and
`a_worker_still_loading_leaves_the_baseline_window` (it sleeps for 6 s
without `Ready`). They ran the conversation in real time. Now
`run_settled_process` waits, on the host, until the worker's monitor shows
that state (`Failed`, or `Loading`), and then runs the conversation on the
stepped fixture. For the missing-model pair `Failed` is final, so the real
process cannot race the logical clock. `Loading` is the monitor's first
state, so for the still-loading test the wait returns at once. What keeps
that worker unchanged is its 6 s sleep against a scenario that takes under
a second on the host. The test holds only while the scenario takes under
6 s; the scope's watchdog is 12 s. The worker's
startup, exit and teardown stay on the host clock, and so do the assertions
on them. The missing-model tests already allowed a worker that fails "before
the loop first looks"; the wait makes that the only order. On the Intel Mac
the three took 4.42 to 4.51 s each before and 0.62 to 0.77 s after, one run
2.10 s. Six copies at once passed 24 of 24 runs.

`hung_worker_never_delays_capture_endpoint_or_session_end` stays on real
time: its assertions need the worker supervisor's 700 ms hang detection
(`turn-end-wire`, on the host clock) to happen during the conversation. The
module stayed in the group as a whole, for it and the module's other
real-process tests, until 2026-10-17 (below). It left the group itself later
that day, waiting on its worker's events on the stepped clock (below).

## Two barrier waits on a paused clock (#823)

`provider_worker::tests::resumed_question_survives_a_silent_service_and_the_assumption_is_reported`
waited out the session's barrier quiet (`AssumeAfterQuiet`, at least 1.9 s)
twice, and `provider_worker::tests::kept_start::with_no_first_answer_the_patience_ends_in_the_strict_timeout_and_no_end`
waited out the kept start's 2.5 s patience and then the strict wait, in
real time: 8.6 s for the two. Both are Tokio timers in the provider worker's
session, so the two tests now run on `#[tokio::test(start_paused = true)]`,
and their own elapsed checks (`waited`, `kept_since`, the hung-worker
bound) read `tokio::time::Instant`. The rig's `pump` and `kept_start`'s
`heard` measure their durations on Tokio's clock too, which is the host's
on an ordinary runtime, so every other test using them is unchanged. The
rig's heartbeats and leases stay on the host clock, which barely moves
while a paused test runs. Assertions and bounds are unchanged. On the Intel
Mac the two took 8.56 to 8.62 s before and 0.58 to 0.62 s after, passing
10 of 10 alone and 18 of 18 with six copies at once; all 148 of
`provider_worker`'s tests passed with the change. They left the
group, in all three filtersets, on 2026-10-16.

## The two review hearings on a logical clock (#823)

`provider_worker::tests::review_2026_10_14::review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer`
and `review_sound_a_name_heard_as_a_request_began_lets_out_the_answer_the_rule_would_hold`
waited four real seconds before saying a name on the bus, so that every
other worker in the test binary reads it as four seconds old: 9.6 s for the
two in full run 38924461963. They now run on `#[tokio::test(start_paused =
true)]` with `logical_clock()` (#1131), and `name_heard`'s wait ticks the
rig, so the four seconds pass in logical time. A worker dates a hearing from
its own clock when it reads it (`began = now - ago_ms`, `crate::bus_out`), so
another test's worker still reads it as four seconds old. The assertions and
the `AGO_US` of four seconds are unchanged. They left the group, in all
three filtersets, on 2026-10-17.

## The evaluator's clocked suites leave the group (#823)

`lamp-voice-eval`'s `serve_session`, `physical_loopback` and `serve_relay`,
with the evaluator's
`physical::mac::audit_grant_tests::audit_transport_that_outlives_its_bound_is_killed_and_never_certified`,
`lamp-records::records`'
`real_reporter_reloads_switch_and_policy_and_requires_a_matching_receipt`
and six of `lamp-memory::health`'s seven tests, already run on explicit
clocks (the sections above, the Records reporting fixture clock and Memory's
controlled fixture).

`final_dispatch` mostly does not (the reviewer on #1202). 12 of its 13 tests
build `PhysicalBackend::new`, on the system clock, over a `/bin/sh`
transport that sleeps; only
`a_session_end_first_seen_in_the_final_drain_withholds_and_is_kept` uses
`final_drain_clock`. Six keep short host-clock windows:
- `a_healthy_session_still_dispatches_once` sleeps to 200 ms ahead, then must
  start within `MAX_TRIGGER_LATENESS_US` (100 ms);
- `an_invalid_cue_first_seen_in_the_final_drain_withholds`,
  `a_relay_framing_error_first_seen_in_the_final_drain_withholds`,
  `a_transport_closed_during_the_final_drain_withholds` and
  `an_unobserved_event_in_the_final_drain_requires_revalidation` need the
  line written after the transport's `sleep 0.3` before the 800 ms target
  (500 ms to spare); the last then dispatches within 100 ms;
- `a_final_drain_that_exhausts_its_budget_requires_revalidation` relays
  1,024 of 2,000 lines within 300 ms, then dispatches within 100 ms.

They passed every run below and on ARM64. If one fails on a pull-request
runner, it goes back into the group by its exact name, as for
`pilot_faults` (#1162).

What is left of the 65 tests' time is real process work: 31.8 s one at a
time in full run 38924461963. Their other host bounds are far above what the
tests take: 30 s for each serve-fixture wait, the 10 s stop grace, 4 s for
each of Records' six status waits, 8 s for each of Memory's readiness, log
and exit waits and 3 s for its socket requests, and one window for a whole
fixture of 10 s (`final_drain_clock`'s delayed line) and 5 s (the audit
test).

Memory's seventh,
`the_environment_alone_does_not_enable_reporting_or_create_bus_data`, stayed
in the group by name: with no `--bus-root` the program makes no reporter, so
there is no report clock to step, and the test watched for 2.2 s, twice, that
nothing appears (4.4 s in that run). It left later that day (below).

They were qualified the way a pull request runs them, each test in its own
process, on the Intel Mac (16 cores) at `54be5904b`, release build: all 57
of the four suites one at a time (39 s), then 570 runs (ten each, shuffled)
32 at once, then 670 runs (the 570 and fifty of each single test) 48 at once,
then 870 runs (the 570 and fifty of each of Memory's six) 48 at once. All
2,167 passed. The slowest,
`serve_session::stale_duplicate_reordered_foreign_and_lost_cues_end_the_conversation`,
took 43.5 s in all in the second round and 85.8 s in the slower third,
against 3.9 s alone on ARM64. The Records test took at most 3.6 s over its six
waits, and Memory's slowest,
`sqlite_reports_retained_counts_and_stays_well_after_caller_refusals`, 8.5 s
over its many requests (0.035 s alone on ARM64). No test, assertion or
bound changed. They left the group, in all three filtersets, on 2026-10-17,
so pull requests run them again. The group kept 38 tests:
`capture_hold_worker` (5), `endpoint_session` (30, one of them `#[ignore]`d,
so 29 that run) and three named single tests. In that run it was 28 tests
and 47.9 s; `endpoint_session` has gained ten since, the fourth's
`with_asker_first_*` among them.

## The two head taps on the stepped clock (#1206)

`coordinator::tests::hand_controls`' `a_tap_on_a_quiet_lamps_head_plays_its_hm_as_a_turn_of_its_own`
and `a_tap_while_the_lamp_is_still_thinking_keeps_its_answer_and_does_not_hum`
date their tap 420 ms back: the hand's last step is 20 ms old when the loop
first polls it, and the tap's acquisition, which is what goes stale, is
82 ms old at the decision against its 100 ms freshness
(`TOUCH_INPUT_TTL_US`). On the host clock, whatever happens between making
the tap and that first poll has about 18 ms (the reviewer on #1207). The
ARM64 emulator takes longer: under podman/qemu on the Intel Mac, at `7ec04da05`, release, the
quiet-lamp test failed 5 of 5 runs at "the tap is judged". Native CI passes
both.

They now run on the stepped scenario clock (`run_loop_with(…,
Clocking::Stepped)`). The four fixture workers' sides are read on the test
thread before each 2 ms step, as their threads read them on the host clock,
and a Stop closes the child's input and waits for its exit, as
`provider_terminal` does. `HandControls::poll`, `fence_clear` and
`quiescent_observation` read `scenario_clock::monotonic_us` under
`cfg(test)`, as `coordinator.rs` does. Without a scope that is the host
clock, so the other `hand_controls` tests, every other test and the lamp's
build are unchanged. The bus publication and `restore`'s start stay on
`lamp_ipc::monotonic_us`. The assertions are unchanged. The other
`run_loop` callers (a fault on the first tick) and the ignored latency probe
stay on the host clock.

On the Intel Mac, a 200 ms or 2 s sleep injected just after the tap is
made, or 5 ms in every step, still passes both; on the host clock the 200 ms
sleep fails both. Under qemu, on the same commit with this change, both
passed 5 of 5 runs.

The owner and the reply were still made on the host clock before the
children started, and the owner's 500 ms listening window ran while they
did: under stress on an Apple Silicon Mac, 2 to 37 of 320 runs of the pair
failed ("listening_not_ready", or no tap judged), and a 500 ms sleep just
before the scope failed both every time (the seventh and the reviewer on
#1207). `run_loop_with` now takes them as a closure (`start`) and makes them
just before the controls, inside the scope; `run_loop` passes its callers'
as they were. With a 500 ms or 2 s sleep before the scope, both pass.

The other four loop tests in `hand_controls` had the same gap on the host
clock (#1220). Each pass of the loop checks physical privacy before it polls
the controls, and the fixture's privacy worker says nothing. So a stall
since `privacy_started` longer than privacy's freshness ended the loop on
"physical privacy unavailable" before the head's stop, sleep or touch was
seen. The reviewer saw `a_lamp_put_to_sleep_from_the_head…` fail 2 of 120
under stress on #1215; a 120 ms sleep right after `privacy_started` failed
it, `a_head_touch_stops…`, `a_head_touch_while_the_lamp_is_still_thinking…`
and `conversation_sends_hand_stop_before_a_provider_fault_queued_in_the_same_tick`
every time. Now `run_until_fault` and `run_until_error` take the owner and
reply as `start` and run stepped, and the same-tick hand stop enters a scope
before its owner is made, reads its peers on the test thread after the loop
as before, and takes their Stop through the scope's teardown pump. With a
120 ms or 1 s sleep there, all 16 pass. The assertions are unchanged.

Their teardown kept one host-clock budget (the reviewer, 07:07 on #705):
`SteppedPeer::read` waited for a stopped child's exit against the
coordinator's real shared cleanup deadline. Under heavy load that deadline
could be spent before the pump ran, so the wait timed out, the pump stopped
before the remaining peers, and they never saw Stop ("mock peer did not
receive Stop", or "verified cleanup failed"): 2 to 3 of 120 module runs. A
400 ms sleep just after the cleanup's deadline is set fails four tests that
way every time. The peer now waits for the exit against the scope's own
watchdog, as #1232's `MutePeer` does, so the cleanup finds the child gone;
with that sleep, all of `hand_controls` passes.

The reviewer found the same wait in three more stepped fixtures (#1235):
`second_route`'s and `pilot_faults`' `close_input` and `provider_terminal`'s
Stop. They now wait against the scope's watchdog too. A 400 ms sleep at the
start of the cleanup then left two `pilot_faults` tests failing:
`stalled_speaker_during_output_is_bounded_and_never_completes_the_answer` and
`stalled_speaker_under_vad_only_is_bounded_too`. Their `StopOnly` speaker read
one control message a step, so the cleanup's revocation held its Stop back a
step past the real deadline. It now reads what is queued in one step, as the
other peers do (it still installs nothing). With that sleep, all 74 tests of
the three modules pass.

Those stepped runs end on the fixture's silent privacy worker, but none
said so. `a_silent_privacy_worker_ends_the_loop_once_its_freshness_has_passed`
does (the reviewer on #1222): a quiet lamp, stepped, ends on "physical
privacy unavailable; not a user mute" after exactly `PRIVACY_FRESH_US`
(50 ms) of the loop's own time; the test allows 50 to 60 ms. It fails if
the startup arm is gone (the loop then never ends on privacy), if the loop
ends at once (2 ms), or if it waits twice the freshness (100 ms).
`Ran::ended_us` is the loop's end on the clock it ran on.
`run_loop_ringed` adds a ring worker whose side answers nothing, so the
trace holds the frames the ring was asked for (`ring_requested`):
`only_the_hums_own_ring_frames_are_marked_listening` (#1244) runs a tap that
hums and one refused while the lamp thinks.

## endpoint_session on pull requests, but for two (#823)

`coordinator::tests::endpoint_session` holds 30 tests now. One is
`#[ignore]`d (`real_worker_session_records_verdict_provenance` needs a built
`lamp-turn-end`), and nine are the fourth's `with_asker_first_*`, added after
the module went into the group, so no pull request had run them. Of the 29
that run, 27 run on pull requests again. Two stay in the group by name: `hung_worker_never_delays_capture_endpoint_or_session_end`
(6.4 s, the supervisor's 700 ms hang detection on the host clock, above)
and `endpoint_fixture_reaps_on_panic_before_and_after_handoff` (3.1 s of
real process reaping).

None of the other 27 uses the fixture's 100 ms ordering tolerance; only
the ignored test reaches it now (`run_process_with_privacy_tolerance`),
`hung_worker…` having moved to the stepped clock (below). The 27 still keep these host-clock
windows (the reviewer on #1203):
- `confirmed_endpoint_exit_keeps_the_restartable_mute`: its `exec sleep 60`
  worker is killed at the default 300 ms grace (`turn-end-wire`'s
  `client.rs`) and must be reaped within `EndpointRig::finish`'s 500 ms,
  so 200 ms to spare. It is the tightest, and the same 500 ms that
  lamp-4ace missed on 2026-10-16. **If it fails on a pull-request runner, it
  goes back into the group by its exact name.**
- `unconfirmed_endpoint_exit_fails_mute_cleanup_and_never_reopens_service`:
  the 1.5 s kill must not land within those 500 ms (1 s to spare).
- The three `run_settled_process` tests wait up to 5 s for the worker to
  settle, then run stepped. `a_worker_still_loading_leaves_the_baseline_window`
  also needs its stepped scenario to decide before the worker's `exec sleep 6`
  ends.
- Every stepped scenario has one 12 s host watchdog for the whole scenario
  (`Scope::enter`).

They were qualified the way a pull
request runs them, each test in its own process, on the Intel Mac (16 cores)
at `54be5904b`, release build: the 29 one at a time (25.9 s; the 27, 16.4
s), then ten of each, shuffled, 16 at once and again 32 at once, then twenty
of each mixed with the evaluator's 570 CPU-heavy runs, 48 at once. All 1,160
runs of the 29 passed, and all 570 of the evaluator's. On ARM64, run
38963119955 ran the 27 under the `pr` profile, and they passed. The slowest
under the mix,
`with_asker_first_quieter_talk_takes_no_turn_from_an_awaited_answer`, took
6.4 s against 2.5 s alone. No test, assertion or bound changed. They left
the group, in all three filtersets, on 2026-10-17. The group keeps 10
tests: `capture_hold_worker` (5), these two and three named single tests.

## A stopped fixture's exit waited for as an event (#1232)

`coordinator::tests::service_mute`'s
`active_physical_mute_cancels_owned_reply_then_reaps_without_completion` and
`repeated_verified_mute_cycles_keep_one_logger_and_never_open_audio_while_waiting`
failed in the reviewer's full one-thread run under load (#1223): "privacy
worker did not stop within 300 ms". The cooperative fixture's child (`cat`)
exits only when the test's peer closes its input after seeing Stop, and those
peers ran on their own threads. A stopped worker has 300 ms to exit
(`Worker::shutdown`, and the coordinator's shared cleanup grace), so a peer
thread the host did not run in time left a live child that was force-killed.
A 400 ms sleep in the peers' Stop handling failed both every time.

The exit is now waited for as an event, on the thread doing the stopping.
`active_mute` runs stepped (ready fixtures, the owner made in the scope); its
peers are read in the scope's pump, and on Stop the pump closes the child's
input and waits for its exit, bounded by the scope's watchdog, so the
coordinator's cleanup finds it gone. The service-privacy mock gives
`Worker::shutdown` a `cfg(test)` stop hook (`Worker::on_stop`) that does the
same right after Stop is sent; a cleanup of its own (`service_privacy_controls`)
still has the peer close the input on Stop, as before. The 300 ms bounds and
every assertion are unchanged, and a Stop that is never sent still fails. With
the 400 ms sleep in place, all six `service_mute` tests pass.

`service_privacy_controls`' own cleanup sends its Stops without `shutdown`,
so its three tests on the same mock kept the race (the ninth on #1233,
#1249). It now runs the worker's stop hook (`Worker::run_on_stop`) right after
each Stop. Its `stopping_child` fixture's hook takes the Stop, sends the ring's
Blanked receipts and closes the child's input there, instead of a peer
thread. With the 400 ms sleep, all 56 `service_*` tests pass (on
`lamp-voice` `3462d7858`); on #1233 alone the three failed.

## hung_worker on the stepped clock, waiting on its worker (#823)

`coordinator::tests::endpoint_session::hung_worker_never_delays_capture_endpoint_or_session_end`
ran its whole six-second conversation on the host clock (6.4 s), with the
100 ms fixture privacy tolerance, because its assertions need the real worker
supervisor's 700 ms hang detection, its 100 ms backoff and its respawn to
happen during the conversation. On the host clock the loop's own 500 ms result
timeout had to come first, with 200 ms to spare: a 400 ms stall of the loop
right after the first request fails it ("physical privacy closed or lost
freshness").

It now runs stepped (`run_policy_with_peers_held`, a `Hold` run before each
step). Logical time waits for the worker's real events in their real order:
when the first request is out, the hold notes the logical time; once logical
time is the hang bound (`limits.hang`) past it, the loop having given up at its
500 ms, the hold waits on the host until the supervisor reports `Recovering`,
which the loop's next pass records; on the next step it waits until the worker
has been respawned (`spawned >= 2`). Each wait has a 5 s host bound. The
assertions are unchanged; `session.elapsed < 8 s` now measures the real time
including those waits. With the same 400 ms stall after the first request it
passes; it takes about 1.45 s, and 30 copies at once passed 90 of 90 runs on
the Intel Mac. It left the group, in all three filtersets, on 2026-10-17.

## lamp-actions' light test on a clock it moves on (#823)

`lamp-actions::server`'s
`the_light_is_offered_when_its_owner_appears_and_withdrawn_when_it_goes`
took 4.1 s on ARM64 (4.2 s on the Intel Mac). Most of it was the server's
`LOOK_AGAIN`, the two seconds after which it looks again at what it can do,
waited out twice: once for the ring's owner to appear, once for it to go.

The server's two periods (`LOOK_AGAIN` and the health report's
`REPORT_EVERY`) are now read on a clock its caller gives
(`server::serve_with_clock`). `serve_reporting_with_schedule`, and so the
lamp, passes `Instant::now`. The test gives a clock that moves only when it
says, and it moves it by 2 s, the period docs/actions.md gives, only after
an event on the bus. To offer the light, that event is the owner's state
being there. To withdraw it, the event is that state having expired, by its
own real 600 ms. A caller's waits, the idle pause and the light's freshness
stay on the host's clock. Every assertion and the test's 5 s bounds are
unchanged. Because the move is a fixed 2 s, not `LOOK_AGAIN`, a longer period
still fails: with `LOOK_AGAIN` at 10 s, "never: the light is offered" (the
reviewer's P3 on #1273).

It takes about 1.4 s (3 of 3), and 30 copies at once passed 90 of 90 runs
on the Intel Mac. The control is a 400 ms sleep in the owner before its
first publish. With it the test passes (1.7 s). With it and the wait for the
owner's state removed, it fails "never: the light is offered", so the move is
ordered by the event, not by time. The old test passed with the same sleep
(4.1 s): it was in the group for its length, not for a bound a loaded
runner misses. It left the group, in all three filtersets, on 2026-10-17.

## The self-test's ring check on a clock its tests step (#1128)

`lamp-selftest`'s ring tests were the slowest on pull requests: in run
38997949448, `a_ring_that_draws_nothing_is_not_checked_not_failed` took
10.0 s, `a_colour_never_drawn_fails_naming_it` 3.8 s and
`every_colour_drawn_passes_and_the_persons_light_is_never_written` 3.1 s.
`ask_colours` waits on the host clock up to `DRAWN_WITHIN` (1 s) for each of
the ten colours to be drawn, then holds each one drawn for `HOLD` (300 ms).

The check now reads a small clock (`now` and `pause`); the lamp's is the
host's, as before (`ask_colours` and `Ring`'s `Check::run`). Those three
tests give it a stepped clock that stands still until the check pauses, and
then moves on only once the stand-in ring owner has drawn or refused every
colour asked so far (the last `light/test` it handled, against the bus's
latest). The 1 s for a refused colour and each hold take no real time, and a
colour is never missed for want of the owner's thread. The two tests where a
person asks for a light after a real time stay on the host clock. Every
assertion is unchanged.

The seven ring tests take 0.93 s together, one at a time, on the Intel Mac.
With a 1.2 s stall in the stand-in owner before each draw, the host-clock
version fails three tests and this one only the hold test, which is on the
host clock by design. With 400 ms, both pass. 30 copies at once, three
rounds: 90 of 90.

## The kept-note read gets the test's own bound (#1269)

`remember::tests::someone_known_keeps_to_their_own_and_is_answered_from_theirs_then_the_room`
failed once in a loaded `cargo test -p lamp-voice --lib` (the twelfth, while
reviewing #1261) and passed alone. Its last exchange, `theirs_too`, read the
person's kept notes with `FIRST_READ`, the lamp's real 300 ms for the first
session's note, and the test's stand-in memory is a thread that has to
accept, read and answer inside it. A host that did not run that thread in
time left the note the room's alone. A 400 ms sleep in the stand-in before it
answers `kept` failed the test every time.

`theirs_too` now takes its deadline from its caller. The voice's
`told_for_session` passes `FIRST_READ`, so the lamp's 300 ms is unchanged,
and the test passes the 2 s it already gave its other four exchanges, as
`DaysClient::told_for_session_within` does for the store tests. Every
assertion is unchanged; with the 400 ms sleep in place the test passes.

Other remember tests also read a stand-in with the lamp's 300 ms: the Days
search (`run_at(..., FIRST_READ, ...)`, twice), the rules-off note
(`DaysClient::told_for_session`) and the Days environment switch's child
process (`days_environment_child`, its recall and forget). None has been
reported failing, and they are left as they are. `rules`'
`the_fact_is_kept_on_the_bus_and_a_stalled_bus_never_holds_up_a_session`
asserts `took < 300 ms` on purpose: there the bound is what it checks.

## The first control message's ages on a clock the test moves (#1128)

`lamp-voice::first_control_message`'s only test (3.7 s, among the slowest
of the 17 October full run) slept 0.3 s, 1.7 s and 1.7 s so that three
messages would be that old when read: the first control message is taken
after 300 ms (its grace), a later one 1.7 s old is refused past the longest
hold-up (1.5 s), and a first data message gets no grace.

The transport's `Channel` now has `send_with(payload, now)` and
`receive_with(now)`, which read a clock given to them at the places `send`
and `receive` read the host's: the send's stamp, and in a receive both the
look before it (for how long since the last look) and the age after it.
`send` and `receive` call them with `monotonic_us`, so the lamp reads the
same clock at the same places. The test moves one clock for both sides by
exactly what it used to sleep, so the ages and the gaps between looks are as
before, and it takes no time. A second pair of channels still goes through
the plain `send` and `receive` on the host's clock (the fifth's ask).

Two deliberate breaks fail it as they fail the old test: `HELD_UP_US` at 2 s
fails the later message's check, and no first-message grace fails the first
check.

## Memory's timed-out mutation is sent before its caller gives up (#1280)

`lamp-memory::voice`'s
`a_timed_out_mutation_may_commit_but_cannot_seed_unlinked_journal_history`
failed on a loaded Intel Mac (load about 21) with a `Timeout` at
`voice.rs:118`, `Fixture::next`'s 5 s wait for the held write to be answered.
Its caller, `DaysClient::run`, was given 50 ms, and it checks that bound
before it writes the request. A client held up before writing gave up
without sending, or after marking the write uncertain but before the line
went out, so nothing was held and nothing was ever answered. A 400 ms sleep
in `run_for` right after `let started = Instant::now()` fails it every time
(at `voice.rs:403` on the base: with nothing sent, nothing is uncertain).
Before that line it passes with 50 ms too, since the deadline then starts
after the stall (the reviewer, #1286).

The caller's deadline is now 1 s, the test's own bound, not one the lamp
runs with (its tools allow up to `ASK_DEADLINE`, 1.5 s). The server's hold
still decides that the caller times out, and every assertion is unchanged.
With the 400 ms sleep it passes. Each test in its own process, 30 at once:
before, 16 of 90 passed (all 74 failures at `voice.rs:118`); after, 90 of 90,
and 48 of 48 at 16 at once. It takes about 3.9 s instead of 2.0 s, since the
caller now waits its 1 s twice.

## Memory's env-alone test waits for the program to say reports are off (#823)

`lamp-memory::health`'s
`the_environment_alone_does_not_enable_reporting_or_create_bus_data` (4.4 s
on ARM64, 6.1 s on the Intel Mac) checked that `LAMP_BUS_ROOT` alone, without
`--bus-root`, makes no health reports and no bus data. With no reporter there
is no report clock to step, so it watched for 2.2 s, twice, that nothing
appeared.

The program now says so once at start, from what it made: when no reporter
was made and no `--bus-root` was given, after the socket is taken,
`lamp-memory: health reports off: no --bus-root` (docs/memory.md). The test
waits for that line, an event, then checks as before that the process is
still serving, that the bus directory doesn't exist and that the log has no
`health delivery`. The program states the absence instead of the test
watching for it. That is a change of method, not of what is checked. A
scratch change that lets `LAMP_BUS_ROOT` alone set the bus root fails the
new test ("never logged …", 8 s) as it fails the old one (the bus directory
appears). So does the tenth's, which starts a reporter from `LAMP_BUS_ROOT`
and leaves the bus root unset: 5 of 5. A first version said the line beside
the reporter's decision rather than from it, and caught that only 6 times in
10 (the tenth's P2 on #1283). With a 400 ms sleep before the line, it passes. `lamp-memory::binary`'s
`a_killed_process_leaves_a_store_the_next_one_loads_and_a_socket_it_replaces`
counts each journal's lines exactly, and now counts the new one (5, not 4).

It takes about 1.8 s on the Intel Mac, almost all of it the program starting
twice in a debug build. It left the group, in all three filtersets, on
2026-10-17.

## The mute-while-waiting capture test reads its worker's own end (#1305's test)

`capture_hold_worker`'s `a_mute_while_the_microphone_is_waited_for_opens_nothing`
(#1305) failed one pull request's run, #1340's 39019956234, with "worker
fault capture: NotPermitted after 0 frames". It passed in the nine runs
around it. Once the microphone is denied, the worker's next open is refused
and it ends, sending `Fault { NotPermitted }` as it goes: the outcome the
test wants. But the test waited with `while !worker.is_finished() {
rig.service() }`, and `Rig::service` panicked on any worker fault. So if the
test read the fault before it saw the thread finish, it failed. A refused
send of the next authority, after the worker had ended, would have panicked
too. Since #1299 this binary runs on every pull request.

The rig now records a worker fault, as it already recorded a reference fault
and a refused send, when `tolerate_fault` is set, and this test sets it
before it waits. With `tolerate_fault` set, a refused receive on the control
or data socket ends that drain instead of the test: once the worker has
gone, Linux can refuse the receive itself (`capture.c.p: Connection
refused`), which is how `a_speaker_held_up_ends_the_capture_worker_unless_recovery_is_bounded`
failed (#1351, the reviewer on this pull request). A refused receive isn't
kept in `refused`, which holds the send's. After `join` it asserts the worker ended on `NotPermitted`,
from its result and from the recorded fault if one came. On Linux x86_64 the
test as it was failed 1 of 3 on its own; with a 400 ms sleep in the worker
right after it sends its fault, it failed 3 of 3, and this passes 3 of 3
either way. For #1351: a scratch `service()` right after that test joins its
worker, with no send due, receives on the gone sockets first; on
`lamp-voice` it failed 1 of 3 at the refused receive, and with this the
receive never fails. The whole binary passes 9 of 9.

## lamp-tasks' status waits read the kept value in slices (#1128)

`lamp-tasks::runtime`'s
`with_progress_a_value_from_the_start_each_task_read_at_once_then_on_schedule_and_the_words_stay_here`
(6.1 s) and `a_watched_task_whose_reads_fail_is_unread_not_gone_and_after_a_restart_too`
(5.6 s) were among the slowest in the full run of 17 October (39029608429).
Almost all of it was one wait: `next_status`'s single `recv(5 s)` for the
task's first status. A publisher looks for new subscribers at most every
200 ms (`Bus::rescan_every`), so a status published just after the test
subscribed reaches it only as the topic's kept value, which `recv` reads
when its wait on the socket is over: at the end of the 5 s. That's the bus
as designed; the test asked for the whole timeout at once.

`next_status` now receives in slices of 100 ms within the same 5 s bound, so
the kept value is read within about 100 ms. It is the same message, and the
same checks follow. On Linux x86_64 (release, as CI): the first test went from
6.1 s to 1.1 s (5 of 5 each; what's left is the diagnostic's real one-second
cadence), the second from 5.3 s (4 of 5; 0.15 s once) to 0.2 s, and the whole
`runtime` binary from 6.4 s to 1.4 s.

## Setup's light paints on a clock its tests run fast (#1128)

Three of `lamp-setup::light`'s tests were among the slowest in the full run
of 17 October (39029608429): `the_light_follows_setup_and_lets_the_ring_go_before_the_marker`
(4.0 s), `a_ring_freed_in_its_retry_window_is_not_opened_after_the_marker`
(3.8 s) and `the_press_after_a_failed_save_shows_blue_at_once` (2.7 s). They
waited out the painter's real fades (`FADE_IN` 1.5 s, `CHANGE` 0.8 s,
`FADE_OUT` 0.6 s) and slept through windows of their own.

The painter now reads a `Clock` (`now`, and `pause` for a frame) at the places
it read the host's: the start, each step's time, the retry's due time and
the frame's sleep. `Light::spawn`, `spawn_if` and `spawn_with_retry` pass
`HostClock`, which is `Instant::now` and `thread::sleep`, so the lamp is unchanged;
`spawn_with_clock` takes another. The three tests give the painter a clock
whose pause moves it on by exactly the frame and waits a fiftieth of that,
so the painter's time is its frames however the machine stalls. Each of
their own windows (the fade in before letting go, the fade out and 300 ms
after it, the 300 ms of nothing after the marker) is waited in that time.
A clock the test steps would not do: `release()` waits on the test's
thread for the painter to fade out.

The third's two bounds (#1128, 21:35): "blue at once" (`< 1.5 s`) is the
painter's own reaction, so it is now counted in its frames from the press to
the first blue one (12 here, against 30). `released.elapsed() < 2 s` asks
whether letting go fell through to `RELEASE_WAIT`, a wait on the host's
clock, so it stays there. The other three tests stay on `Light::spawn` and
the host's clock, so the lamp's own wiring keeps a full fade in and out.

On this Mac the three went from 4.07, 3.98 and 2.73 s to 0.12, 0.11 and
0.07 s. Three deliberate breaks fail the same check in the old tests and the
new: keeping the ring let go after a failed save (amber never shows),
opening a ring while fading out after the marker, and a 3 s `CHANGE` (blue
not at once). A 400 ms stall of the painter at its 30th frame, and of the
test's thread after the press, after the marker and after the ring was
freed, fails none of them.
