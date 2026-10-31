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

The nightly full run is set for 19:00 UTC (02:00 in Vietnam) in
`.github/workflows/lampos-nightly.yml` on `main`: GitHub starts a scheduled
workflow only from the default branch. GitHub starts it late, though: the six
nightlies of 25 to 30 Oct began between 21:34 and 23:10 UTC (04:34 to 06:10 in
Vietnam), so look for a night's result after 06:00 there. It starts this workflow on
`lamp-voice` as a manual run with `full`, so the job is defined in one place.
By hand: Actions, "lampOS on ARM64", "Run workflow", branch `lamp-voice`,
tick `full`; or `gh workflow run lampos-arm64.yml --ref lamp-voice -f full=true`.

When a full run fails, a job opens an issue titled "lampOS: the full ARM64
run failed", or comments on the open one. Nothing the tests assert was
changed. The runners are unchanged (`ubuntu-22.04-arm`): larger runners cost
too much. A push to `lamp-voice`, or a manual run there, can instead go to the
internal Mac Studio (`.github/scripts/lampos-route.py`, the switch
`vars.LAMPOS_STUDIO`). On 31 Oct that switch is `off`, so every run is
GitHub-hosted.

Each runner compiles every test binary again (about 5 of its 10 minutes), so
pull requests and merges use one runner, where the realtime group was
skipped until 2026-10-17; those tests took
about a minute in parallel (run 38721013037: 4,399 tests, 55 s, 4 cores).
A merge cost one runner for about 11 minutes on 17 Oct (until 2026-10-15 it
cost four for about 15). On 31 Oct, with three times the tests, a push's ARM64
job took 25 to 27 minutes (the last five), and its whole run, signing and
publishing included, 28 to 30.

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

- `lamp-voice`'s `capture_hold_worker` tests, a module of its one integration
  binary since #1806 (`binary_id(lamp-voice::it) & test(/^capture_hold_worker::/)`; the evaluator's
  four, `serve_session`, `physical_loopback`, `serve_relay` and
  `final_dispatch`, and six of `lamp-memory::health`'s seven tests left on
  2026-10-17, below);
- one test of `lamp-voice`'s `coordinator::tests::endpoint_session`, named
  exactly, `endpoint_fixture_reaps_on_panic_before_and_after_handoff` (the
  module's other 28 left on 2026-10-17, `hung_worker…` later that day, and
  `coordinator::tests::early_end` and `coordinator::tests::pilot_faults` on
  2026-10-16, below);
- one single test that took 4 s or more outside those, named exactly:
  `lamp-health::it`'s (one binary since #1806)
  `program::between_two_healths_five_seconds_apart_the_program_is_not_taken_for_gone`
  (`lamp-actions::server`'s light test and `lamp-memory::health`'s
  `the_environment_alone_does_not_enable_reporting_or_create_bus_data` left
  on 2026-10-17, below);
- two tests of `lamp-skills::it`'s `skills::` (one binary since #1806),
  named exactly, that run the real
  program on the host's clock by design (#1806, Seventh's choice on
  2026-10-19): `asking_the_policy_on_the_hosts_clock_gives_up_after_its_wait`
  (the production 2.5 s give-up, and the late answer after it passed over)
  and `a_reading_pose_is_held_on_the_mood_socket_for_its_for_and_then_let_go`
  (a real one-second re-publication during a 1.5 s hold). They took 4.5 s
  and 3.6 s on ARM64. The reading pose's checks of a skill's definition,
  which need no clock, are a test of their own outside the group,
  `a_reading_pose_needs_a_known_mood_and_a_for_of_at_most_two_minutes`;
- one more of `lamp-skills::it`'s `skills::`, named exactly,
  `asking_the_policy_a_hello_made_and_not_noticed_rests` (#1829, the
  reviewer's P3 on 2026-10-19): it skips the program's clock for the run
  itself, but waits up to 20 s of real time for the program to start and
  subscribe, and once missed that under a load of about 30;
- two tests of `lamp-voice`'s `hand_controls`, named exactly, that count
  the input pump's real wakes for a second by design (#1806, #1569 row 35,
  2026-10-19): `with_the_wait_an_idle_pump_wakes_five_times_a_second_and_stops_at_once`
  (at most 10 passes waiting, and a wake brought round within 100 ms;
  the off-mode observer checks actual park arguments and bounded progress) and `with_the_wait_an_owner_polling_every_2_ms_does_not_wake_the_pump`.
  They took 2.6 s and 1.3 s on ARM64; beside 8 busy cores and four SQLite
  stores syncing on a Mac, the first's stop took 103 ms to 1.09 s in 18 of 20
  runs. That was the pump letting go of its subscriptions on disk, not its
  wake: beside four `F_FULLFSYNC` writers a woken stop took a median of 225
  to 234 ms while the wake's next pass came within 1.5 ms. So since
  2026-10-20 the wake is judged by that pass (a wake ignored: about 176 ms,
  every run). The 2 ms tick's count can still fall short beside busy cores
  and syncs together (26, 30 and 74 passes in 3 of 25 runs). A later 31-pass
  failure remains a failed observation, with no measured cause claimed.
  The fixture now starts counting only after the pump observes its first
  completed drain following subscriptions, with a two-second readiness bound.
  A held-start control rejects readiness before entry and requires it after
  release. Readiness and wake failures stop and join the owned worker before
  asserting; a stuck join is left to the outer test watchdog rather than
  detaching a live pump. The two-second stop limit is checked after joining.
  The off-mode fixture records the duration passed to each actual park
  call, after it returns, in an instance-local `cfg(test)` observer. It
  requires progress since the sample began, allowing at most two further
  diagnostic seconds after the sample, before stop; every recorded park
  argument must be exactly 2 ms. Stop and join
  complete before the receipt is checked. The one-second pass count remains
  diagnostic output, not a 100-passes/s host or hardware guarantee. The
  waiting ceiling, 100 ms wake check, two-second stop bound and production
  waits are unchanged; the previously documented late-wake false-positive
  remains. Observer atomics do not wait or allocate, and do not exist in
  ordinary builds. The separate head-touch bus-to-poll fixture also waits
  for that first subscribed drain. Its readiness and receipt watchdogs each
  allow two seconds; the 21-sample median latency limit stays 100 ms. A
  test-owned guard stops and joins the worker before reporting a failure,
  including an assertion unwind. A missing receipt records the round,
  publication durations, source ages at publication, receipt wait and health
  counters. These diagnose the failure; they do not extend message freshness.
  The b333/1cad Mac run's head-touch receipt failure and 192.48 ms owner-poll
  mute observation remain failures, alongside four privacy-reader failures
  (combined Voice 4,084/6/21). The identical-tree ARM64 run passed 7,497 cases
  with 53 reported skips. Neither observation establishes a host-load cause
  or erases the other;
- two more of `lamp-skills::it`'s `skills::`, named exactly, that start the real
  program twice on the host's clock, a restart between, and wait 800 ms of
  real time for a nudge that must not come (#1837):
  `the_program_keeps_a_skill_off_across_a_restart` and
  `the_program_forgets_a_snooze_at_a_restart`;
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

What a pull request runs: `--profile pr`, the group one at a time, over the
crates its diff changes, every crate that depends on them, and lamp-cloud,
lamp-voice-eval and lamp-update, whose tests read other crates' files
(`.github/scripts/lampos-pr-crates.py`, the project lead's proposal, 19 Oct).
It runs every crate when the diff touches `Cargo.lock`, any `Cargo.toml`,
`crates/msgs`, `bus` or `ipc`, `deploy/`, `skills/`, `.config/`, the docs
tests read, the script, the workflow or any other path. A push to
`lamp-voice` runs every crate. Everything one test at a time, as the full run:
`cargo nextest run --release --workspace -j 1`. `cargo test` is unaffected
and runs everything.

## Every run runs the group (#823)

Once most of the group had moved onto test clocks, 9 tests were left, and 8
once memory's env-alone test leaves too (#1283). They wait on the real clock
by design: `capture_hold_worker` plays the production capture loop in real
10 ms ticks, `lamp-health`'s `program` test runs the real program's 5 s
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
preserves its existing wait and final-drain ordering. Reads never move time.
The runtime and relay are polled 2 ms after a round that moved anything and,
after one that moved nothing, at the fake model's 10 ms tick, which is
unchanged (#1806): an output that lands just after an idle poll is seen up to
about 8 ms later.

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
(`tests/it/rest_replay.rs`), and in `tests/runtime.rs`
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

The no-pause equivalence fixture publishes the optional keeper setting and
constructs its service before `populate`, then steps immediately with the
returned wall clock. This keeps setup work before the fresh input batch;
it does not remove the publication-time window below or change any expiry,
freshness, clock-set or no-pause equivalence checks.

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

The fixture also gives the provider's capture-time conversion and Gemini's
scripted connector one shared acquisition clock. The scenario pump advances
it before running the provider; reads are pure, and reconnects keep that same
clock. Audio waiting in the sender or session actor therefore ages with its
capture schedule, including during a host scheduling pause. The existing
2.5 s input-age limit still applies to that elapsed acquisition time. Only
this fixture installs the thread-local conversion guard and the connector's
explicit capture clock; ordinary tests and production keep the system clock.
The `fixture-clock` feature is enabled only by Voice's dev-dependency.

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

## The self-test's head move on a clock its tests step (#1128)

`lamp-selftest`'s head-move tests drive the check against a simulated body
on its own thread. They took 26 s on Linux (30.6 s once #1340's wait for a
still joint came in), the slowest on pull requests after the ring check
(the 17.5 s test alone, run 39014782754). Each move eases out over 0.4 s,
allows 2 s to reach and 3 s to come back, with a 20 ms step.

The check now reads the crate's `Clock` (`now`, `pause`), as the ring check
does: `one_move`, `moves` and #1340's `settled`. On the lamp it is the
host's (`Check::run` passes `Host`), and the same reads happen. The tests
give it a stepped clock, and the simulated body runs on the host's clock
until the check first pauses, so its report and first readings come as on a
lamp. After that it takes one step for each pause, and the clock moves on
only once it has. The body moves by steps, not by time, so each 20 ms of the
check is one 20 ms step of the body, without the wait. The body's report
wait (`REPORT_WITHIN`) and the silent body's 1.5 s stay on the host's clock.
#1340's own test of `settled` passes the host's clock too, since its
shaking head drifts by real time.

Stepping it first showed #1329, a real hazard: a move's start was read
while the head still settled after the last move, and the merged test
passed by 0.1 count of `NEAR`. Stepped, it failed every time. #1340 waits
for a still joint first. `settled` has to read the same clock as the move:
on the host's, the stepped body stands still during the wait and seems
settled where it isn't. At #1373 (19 October, Linux x86_64 in podman) the ten
head tests took 6.5 s (3 of 3), and the crate passed 59 of 59. #1340 then
added an eleventh, the axis that is not fitted. On ARM64 (run 39370639743,
20 October) the four Linux-only move tests took 2.2, 1.6, 1.4 and 1.4 s,
where the full run of 19 October (39337741436) had 17.4 s and 6.7 s for the
two slowest.

The simulated body counts each request's lease in its own 20 ms steps, from
the lease's own length, not against the host's clock: stepped, a 200 ms
lease judged by real time outlasted the whole move, so a check that asked
once and never renewed passed where a lamp's body would have dropped its
request (the reviewer's P2 on #1373). With the lease in steps, that check
fails both move tests again.

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

## A voice fixture's first `sh` gets 30 s to start (#1317)

The reviewer saw timed fixture starts fail in full local `cargo test -p
lamp-voice` runs, three times on 17 October and each time in a run of
consecutive tests: 8 `ring_trace_tests` once, 10 `pilot_faults` another
time, each failing its own "fixture exec marker timed out". Every one passed
alone. `fixture_exec_barrier` (`cfg(test)`) waits for a fixture's first `sh`
to print one byte, and it had the lamp's `WORKER_STARTUP_TIMEOUT` (3 s) as
its bound. On a quiet run of the whole crate here every such wait was under
50 ms, so the failures look like stretches where the whole machine stalled
beside other builders' work.

The fixtures (`Worker::command_fixture` and the cooperative fixture) now give
it `FIXTURE_EXEC_WAIT`, 30 s, as the serve fixtures have. Only a fixture that
never starts fails there, and it then fails in 30 s instead of 3. Nothing
the lamp runs changes, nor does any assertion. The barrier's own tests keep
their bounds (20 ms for its timeout, and a wrong marker fails at once).

With the fixture's `sh` made 4 s slow to print its marker,
`ring_trace_renewal_without_receipt_never_claims_success_and_times_out`
fails on the old bound with the reviewer's message (at 3.00 s) and passes on
the new one (4.0 s).

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

The reviewer's P3 on #1402 found what that stall probe missed. A stall of
the test's thread between the marker and freeing the ring could let the
whole fast fade out (12 frames, about 12 ms) pass before the ring was free.
Then a painter that opens a ring while fading out was no longer caught: 0 of
10 at a 20 ms and at a 40 ms stall, where the host clock's 600 ms had room.
The test now holds the fast clock (`Fast::hold`: the painter waits at its
next frame) from before the marker until the ring is free. That is safe,
since no ring is open and letting go returns at once. So all of the fade out
comes after both. That painter is now caught 10 of 10 at stalls of 0, 20,
40 and 400 ms there. The right one passes under a 400 ms stall 10 of 10, and
all six tests pass 20 of 20.

## The ring's startup deadline on a clock its tests move once the child is up (#1128)

Three of `lamp-voice`'s `process::ring_startup_tests` took 3.0 s each in the
17 October full run (39029608429), waiting out `WORKER_STARTUP_TIMEOUT` (3 s)
in `Worker::wait_until_ready`:
- `permanently_nonprivate_peers_timeout_without_connecting_or_chmod`;
- `missing_socket_deadline_is_unchanged_and_does_not_lose_the_child`;
- `connected_but_unready_ring_times_out_and_remains_owned`.

That wait now reads its deadline through `startup_now()` at its three places:
setting the deadline, and its two checks. On the lamp (any build that isn't a
test build) it is `Instant::now()`, so the lamp's code is unchanged. A test
build adds a thread-local amount (`startup_clock`, zero unless a test moves
it), and the wait runs on the thread that started it.

Each test's own tick moves that clock on 100 ms, but only once what the test
is about has happened: the child started, its peers bound, or its peer
connected. So the 3 s deadline passes in about 30 ticks, and a child slow to
start never finds it already gone; the host's clock still counts as well.
`missing_socket_deadline_is_unchanged` measures the wait on the same clock,
so it still finds at least the full 3 s.

On this Mac the three went from 3.54, 3.52 and 3.52 s to 0.70, 0.72 and
0.62 s; most of what's left is the fixture's first run of a fresh script.
A deadline set to end at half the bound fails
`missing_socket_deadline_is_unchanged` at the same check, old and new. A fake
ring 400 ms slow to start leaves all ten `ring_startup_tests` passing.

The first builder's ask on #1412:
`the_startup_wait_reads_the_hosts_clock_unless_this_thread_moves_it` pins
that the wait reads the host's clock until the test's own thread moves it,
and that another thread's move never reaches it. So every other test starts
its workers on the host's clock as before, and the other seven
`ring_startup_tests` start the ring on it. A test build whose clock ignored
the move fails it.

## The Policy's test rigs ask for a report every 20 ms (#1128)

The Policy says how it is on the bus once a second (`Diagnostic`, from
`second()`), and in the same look it looks again for its decisions'
publisher and for a part listening for the lamp's name. The judges rig
(`tests/it/judges.rs`) waited for the next of those reports after publishing
its cues, so each of its seven tests waited about a second. Several of the
`policy`, `hello` and `progress` tests wait for a report, too.

`policy::Options` now has `report_every`. `lamp-policy`, `lamp-policy-judges`
and the examples pass `policy::REPORT_EVERY`, the lamp's second, so the lamp
is unchanged. The four rigs in `crates/policy/tests` pass 20 ms. The voice's
`hello_asks` and `progress_news` never wait for a report, so they keep the
lamp's second.

At 20 ms the judges rig's handshake had to become strict. It took the first
report with `log_written` of 1 or more after its cues, and at 20 ms that
report can be sent at the end of the very turn the cues landed in, after
that turn had read the voice and the room. The rig now subscribes after
publishing the cues and takes the second report on that subscription, with
`log_written` still 1 or more. A report goes at most once a turn, at its
end, so the first one there was sent after the cues, and the turn that ends
with the second began after them. With no handshake at all, the judges tests
still passed 140 of 140 (four at a time), so none of them is sensitive to it
today. The strict one keeps the guarantee the second-long wait gave.

On this Mac, one at a time: `judges` 7.4 s to 0.6 s, `policy` 21.0 s to
15.9 s, `progress` 9.0 s to 7.6 s, `hello` 6.9 s to 5.1 s. The rest of
those is the lamp's own real-time windows (the pause, `NAME_BEFORE`,
`WORDS_FOR`), which this leaves alone. `the_policy_says_how_it_is_once_a_second_and_that_it_decides_without_a_model`
passes at 20 ms, so it checks what the report says, not how often; since
#1511, `the_lamps_policy_reports_once_a_second` pins `REPORT_EVERY` at 1 s.

## Waits the tests chose themselves (#1128, 18 October)

A read of the 26 slowest tests left after the earlier changes (the 13:00
full run, 39102474827) found most of them waiting on a bound the test
chose itself: a production constant passed as its own deadline, a fixed
sleep, or a tool's whole `--seconds`. Each was cut without touching what
the lamp runs and without dropping a check; #1530's withdrawal may now take
up to its `WAIT` where it was looked at after 600 ms. Each test's comment
says why. #1530, #1531, #1535 and #1536 were probed with a deliberate break
that the test still fails; the others with runs under load, a stall or a
slowed start. ARM64 seconds, before (the full run, or the other PR's run of
the same hour) and after (the PR's own run):

| PR | Test | It waited on | Now | ARM64 s |
|---|---|---|---|---|
| #1525 | skills' two host-clock tests | the skill put in after the start, found by `RELOAD_EVERY` (2 s) | put in before the start on the host's clock only | 6.52 → 4.52, 5.62 → 3.63 |
| #1526 | bus `hz`, `record`, `keep-logs` | a fixed `--seconds` of 1.5, 2.5 and 2 | 0.5, 0.75 and 0.6, counted once each tool listens | 2.84 → 1.83, 2.61 → 0.86, 2.01 → 0.61 |
| #1527 | records' full-pipe write | the fixtures' `HOST_WAIT` (4 s) as its deadline | its own 200 ms | 4.01 → 0.20 |
| #1528 | motor preflight, child never exits | `WORKER_STARTUP_TIMEOUT` (3 s) | its own 1 s | 3.01 → 1.01 |
| #1529 | provider_pair's abandoned turn | `KEPT_START_PATIENCE` (2.5 s) | a 500 ms patience set through `successor_start`; `provider_worker`'s own test holds the 2.5 s | 2.79 → 0.77 |
| #1530 | progress_news' withdrawal | three fixed 600 ms sleeps | polls `waiting()` until let go, within `WAIT` | 2.14 → 0.94 |
| #1531 | lamp-name's lease | two halves of 1 s against a 300 ms lease | halves of 0.5 s, the same cadence | 2.01 → 1.01 |
| #1535 | hello_asks, nobody answering | `ASK_FOR_US` (2 s) on the host's clock | the voice's clock stepped to `ASK_FOR_US − 1` and `ASK_FOR_US` | 2.06 → 0.06 |
| #1536 | the transport's held-up receiver | sleeps of 400 ms and 1.7 s | one clock stepped by the same, via `send_with` and `receive_with` | 2.11 → 0.004 |

#1506 is the one change in the batch to what the lamp does: setup's address watcher ends
when the join does, so five `lamp-setup::serve` tests no longer wait out
its one-second sleep, and "set up" comes up to a second sooner (docs/setup.md).

Kept as they are, each for its own reason:

- `service_privacy`'s GPIO test waits the privacy worker's `UNAVAILABLE_WAIT`
  three times: a mute-safety bound.
- skills' host-clock tests keep their 2.5 s give-up and their one-second
  pose: that is what they check.
- `lamp-health`'s alive test keeps the lamp's own three-second lease, as its
  comment says.
- `lamp-surround`'s serve test keeps `--seconds 1`: whole seconds only, and a
  SIGTERM sent just after the page answers can reach the process before its
  handler is registered.
- `provider_worker`'s burst test keeps the rig's 2 ms tick: the rig publishes
  the coordinator's leases on every pass, so a tighter loop would flood the
  worker's control socket.
- `lamp-settings`' client deadline tests keep their 1 s: a shorter one cuts the
  peer's margin under load, which #1280 showed fails.
- voice-eval's `serve_session` tests spend their time in lockstep rounds,
  over 32,000 of `POLL_US` (2 ms) for a scenario of about 65 s of logical
  time. A shorter plan or a driver that jumps to the next thing due would cut
  it. That is voice-eval's own design, proposed on #1128.

## The Policy's own waits, short in its tests (#1806, 20 October)

Five of `lamp-policy::policy`'s tests waited out two of the loop's own waits
in the full run of 19 October (39337741436). Four waited for the lamp's words
after its first sound (`outcome::WORDS_FOR`, 1 s):
`what_the_lamp_did_after_each_decision_is_written_beside_it` 1.8 s,
`a_lamp_that_spoke_before_it_was_decided_is_written_with_a_time_less_than_nothing`
1.5 s, and `a_piece_taken_up_after_the_lamps_sound_but_sent_before_it_is_answered_by_it`
and `a_session_stamped_minutes_ago_is_timed_by_its_taking_up` 1.2 s each.
`how_the_room_is_and_who_the_eyes_see_are_taken_from_the_bus` (2.3 s) slept
2.1 s for the eyes' list to outlast `SEEN_FOR` (2 s).

The two are now a `policy::Timing { words_for, seen_for }` in `Beside`.
`Timing::LAMP` and the default are the lamp's 1 s and 2 s, and `lamp-policy`'s
own `main` passes `Timing::LAMP`, so the lamp is unchanged.
`the_loops_own_waits_are_the_lamps_unless_a_test_says_otherwise` holds those
values. The second judge's tracker (`--local-judge`) takes the same
`seen_for`.

The five tests use `QUICK`: 400 ms for the words, and 1 s for the list. The
tests send their words within milliseconds of the lamp's first sound, so
400 ms still takes them all in. In the room test the second utterance comes
150 ms after the list, and the test sleeps 1.1 s before the third.
On this Mac the five took 8.05 s and now take 4.85 s, each run
from its own build: 1.81 to 1.22, 1.51 to 0.96, 1.20 to 0.69, 1.21 to 0.66
and 2.32 to 1.32. With the loop's list read on `SEEN_FOR` in place of its own
`seen_for`, the room test fails (the third decision still saw the faces).

Left as they are, for what each is there to show:
- `hello`'s office scenes are timed to the rig's 400 ms quiet. Halved, the
  lull would sit 50 ms inside it on a busy runner.
- `decide`'s picture test needs its 1.1 s gap. It is what makes "the new frame
  is sent with its own age" a check: with a gap of 100 ms, an age measured
  from the old frame would pass `< 500 ms` too.
- `judges_program` keeps `--seconds 2`. The child's time starts at its
  start, and inside it the test waits for the ready report, sends a line,
  and waits for a decision with a 1 s deadline.

## Voice fixtures under a full parallel library run (#1569, 22 October)

The full x86 Voice library shares one process; passing a case alone does
not establish that its fixtures are isolated. The requested-audio Skills
fake and `DeclineMusic` now begin their original three-second accept
watchdog at an explicit dispatch rendezvous. Time spent constructing the
provider, or waiting for another fixture's guard, cannot use that watchdog.
The long memory-hit fake prepares its 150-entry reply before either real
60 ms call and acknowledges readiness separately for each request.

The timer-following-answer and busy-room-reminder cases already wait for
provider/Policy events. They now opt into the existing paused fixture
clock, so host scheduling does not expire their original logical authority.
The music-RPC Stop fixture takes the existing process conversation guard
through worker shutdown and restoration; it cannot replace another test's
stored question. Its Stop deadline and the own-answer assertions remain.

The coordinator recording fixture drives the real loader and Skills socket
worker with the existing scenario clock. It consumes each real Bus publication
at an explicit observation event, then steps its original wire stamp and cache
bounds together while the worker is parked. Production construction still
starts its independent monitor; runtime tests still assert that cache contention
cancels. Recording tests retain an independent real 250 ms handoff assertion
alongside their logical receipt and trace checks.

[The verification record](verification/voice-full-run-events-20261022/README.md)
keeps the unchanged full-run failures, controlled before/after stalls,
scheduled conversation overlap, negative controls and complete x86/ARM64
counts. The injected stalls are reproduction patches only; no fixed sleep
is added to the submitted tests. These are fixture measurements, not
acoustic or device latency evidence.

## Body successor-socket ownership and fixture teardown

`dropping_reaped_predecessor_does_not_remove_active_successor_sockets`
still starts a real ready successor, drops the reaped predecessor, and
requires the predecessor directory gone while every successor peer remains
and the successor remains ready and unreaped. Its later fixture teardown
explicitly kills only that owned successor and requires an actual reap,
truthful non-clean exit, no IPC cleanup error, all peer paths removed, and
the successor directory removed on drop.

That teardown does not assert a graceful Stop inside the product's unchanged
20 ms inhibit-acknowledgement deadline. Cooperative stop receipts and clean
replacement remain covered by
`actual_child_uses_same_worker_loop_and_stop_reaps_before_clean_replacement`;
`hung_owned_child_is_killed_and_reaped_after_missing_inhibit` still checks the
failed-acknowledgement path. No runtime bound or cleanup policy changes.
The recorded 2/3 fixed observations remain their original failed/passed
outcomes; separating the fixture's contracts establishes no historical cause.

## Unplug-gap phases share the worker's explicit timeline

`a_gap_in_samples_withdraws_ready_and_only_an_unsafe_one_fails` drives the
existing production `Shared::tick` and real kept-bus publication on one
controlled monotonic timeline, as the existing worker tests do. Safe samples
remain 20 ms apart; worker ticks use the actual `PERIOD` constant. Each bus
result is read before the timeline advances in 10 ms steps. Both Ready
proofs must still take at least `SETTLE_US` (500 ms), each phase has a 2 s
logical limit and a separate 2 s host watchdog, a Failed phase is rejected
early, and the unsafe sample must report the moved joint. The latch is asserted after
every observed step. Diagnostic labels distinguish first proof from proof after
the gap.

The runtime Adapter, atomic sample encoding, worker thread and 100 ms
freshness/500 ms settling rules are unchanged. This case tests functional
temporal behavior through the existing worker seam and real bus; it does not
measure host thread scheduling, physical settling or actuator latency. The
original three failed observations remain failed and do not establish a
cause. Slow worker-period, shortened-settling and gap-as-failure controls
must remain negative before the candidate is qualified.

The separate `the_enabled_adapter_publishes_its_latch_and_joins_after_stop`
case retains enabled-thread integration coverage. It waits for the actual
worker's Folding publication and latch, checks safe/unsafe encoding through
`Adapter::sample`, observes its stopped Failed publication and checks the latch at both
publication endpoints, then joins the worker and removes the bus root. Publication waits
and the post-join check retain two-second host bounds; the outer test runner
must still bound an actually stuck join. It needs no uninterrupted half-second
of host-scheduled safe samples. No physical motion or latency is measured.

### Body-push fixture publication cadence (26 October 2026)

The two second-route body-push scenarios publish the first held push at once,
then refresh `body/hand` every 100 ms of scenario time. This follows the topic's
promised cadence; normal production Body uses every fourth 20 ms step (80 ms).
The fixture previously rewrote the Latest bus file on every 2 ms scenario pump
step. A counter-only negative on 2747c72a7 observed 4,582 publications and
failed the new bounded-count assertion after successful scripted mute/cleanup.
This demonstrates excessive fixture work, not a proven cause of every earlier
host-watchdog failure. Full and isolated watchdog failures keep their original
source and outcome in the verification record.

The two existing cases each retain their unpushed control. They still require
scripted PhysicalMute, Stop/exit observation for every child, no forced shutdown,
correct search/picture cancellation and no unauthorized speaker output. Pushed
runs require 2–111 real publications across eleven logical seconds; unpushed
runs require zero. The 24-second host watchdog, cleanup deadlines, topic freshness
and `since_us` semantics are unchanged. This is test scheduling only, with no
runtime, acoustic-latency or physical-hand acceptance claim.

## Session-reader rollback fixtures prove their injected boundary

`failed_private_rollback_disables_only_metadata` requires evidence that grant
checkpoint 12 and rollback checkpoint 0 both ran. Its test-only observer keeps
the first apply error, including its kind and OS error, and prints the actual
report before asserting each original report field separately. The unchanged
empty event file and private sentinel checks remain. An earlier verified
private fallback is a different outcome, not a successful observation of this
late failure.

The focused Linux rollback fixtures use a thread-local, pinned filesystem root
at their own private fixture directory. The common ancestor and no-follow
component walk still checks real owners, modes, ACLs and identities at and
below that root. This deliberately excludes host `/` and `/tmp` ancestry from
these focused cases; every unscoped security fixture still uses the original
root and path walk. The scope rejects paths outside its component prefix,
checks the named root against its pinned identity, cannot move between
threads, and restores the default on unwind. It needs no chroot capability or
process-wide environment change.

A separate real early-refusal case leaves a readable voice sibling after
reader preparation. It requires that checkpoint 12 was not reached, preserves
the original refusal diagnostic, and checks a degraded but initialized writer
with private metadata. It retains the production-bounded finish report and
uses the existing test-only completion event for eventual data assertions;
the production finish bound is not extended. Another control retains refusal
of an unsafe root mode, a real ACL and an escaped path. No production privacy
validation, grant or rollback action, report policy or finish bound changes. These tests establish
neither the cause of earlier CI failures nor physical-lamp acceptance.

`replaced_record_never_changes_an_outside_sentinel` also enters that pinned
fixture-root scope immediately after constructing its fixture. The guard stays
alive through reader preparation, file creation, the record's replacement by
a symlink, the original grant-refusal assertion and the unchanged sentinel
check. This case exercises the replacement boundary below its owned root;
host ancestry remains covered by the unscoped fixtures. No timing or error
injection is added, and no historical failure cause is inferred.

`only_the_new_session_is_shared_after_one_pinned_lookup` uses the same owned
root boundary for its positive grant. It retains both iterations, the single
lookup, the original acknowledged/non-degraded report assertion and its
500 ms finish bound, new-session/event modes and groups, the unchanged old
session and the private sentinel. It checks sharing below the fixture root;
the real host's ancestor metadata is not an input to that focused assertion.
This does not identify the cause of an earlier unavailable-reader report.

The positive grant, failed-group lookup and unsupported-ACL final-data fixtures
retain the report from the production-bounded finish, then observe the existing
test-only writer-completion event before asserting final health and file modes.
The event is sent only after the writer loop and file finish have returned.
Their original health predicates remain, including writer-failure rejection;
failures print both reports. The 500 ms production finish budget is unchanged,
and the five-second completion observer is a test guard, not a fixed sleep.
An unchanged-Main full run failed the failed-group health predicate without
printing its report fields; this change does not infer which field caused it.

`an_owned_fixture_root_ignores_only_changes_above_its_boundary` checks that
scope deterministically. Inside a private top-level directory, it changes an
owned ancestor's mode after preparation: a boundary including that ancestor
refuses the grant, while the inner fixture boundary permits it. An invalid
voice mode below the inner boundary still refuses the grant. Original private
rollback/event modes, groups and sentinel checks remain, and RAII restores
the controlled modes on unwind. No global ancestor, production check, retry,
error allowance or finish deadline changes.

`shared_session_writer_projects_every_segment_without_private_values` also
keeps the owned fixture-root guard through preparation, projection and finish.
All original positive projection assertions and the 500 ms finish bound remain
unchanged. This isolates the focused sharing check from host ancestor metadata;
it does not establish the cause of an earlier unavailable-reader report.
