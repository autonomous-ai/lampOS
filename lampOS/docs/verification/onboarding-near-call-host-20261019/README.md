# Onboarding near-call step, host verification (2026-10-19, #1803)

Host only (macOS, aarch64). No lamp, no device, no person. Toolchain
`cargo +1.96.0`, environment `RUSTC_WRAPPER=` (empty),
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`,
`-j2`, `--test-threads=1`, run from `lampOS/` in sequence.

| # | Command | Result |
|---|---|---|
| 1 | `cargo +1.96.0 fmt --all --check` | pass |
| 2 | `cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings` | pass, no warnings |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 onboarding:: near_call onboarding_storage stop_and_physical_mute_do_not_wait_for_onboarding_storage bus_out::tests::onboarding say::tests::onboarding` | 64 passed, 0 failed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-near-call -- --test-threads=1` | 7 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-msgs -- --test-threads=1 onboarding` | 1 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-skills -- --test-threads=1 onboarding` | 3 passed |
| 4 | `cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1` | 2369 passed, 0 failed, 12 ignored (lib 2129) |
| 4 | `cargo +1.96.0 test -j2 -p lamp-msgs -p lamp-near-call -p lamp-skills -- --test-threads=1` | 270 passed, 1 failed (see below) |
| 5 | `cargo +1.96.0 build -j2 --workspace` | pass |

The one lamp-skills failure in step 4 was a timing test under host load
(load average about 30): `the_tools_socket_lost_is_made_again_and_the_folder_gone_ends_lamp_skills`
(10 s socket read timeout). It passed three times alone, and the whole
`--test skills` binary then passed 105/105. A second full run instead timed
out `package::an_install_waits_while_another_run_holds_the_skills_folder`,
which passed alone. This change touches no lamp-skills code; only
`skills/onboarding/SKILL.md` text.

## What the tests show

`onboarding::near_flow_tests` (11), with a scripted bus and Unix clock:

- three accepted calls write `near-call.json` (0600, bar 19.4 from
  22.4/25.0/23.1) and only then mark `how_you_sound` done; the model's
  `setup_step how_you_sound done` is refused; a new voice cycle continues from
  the kept prompts and tries;
- a hearing begun at +6000 ms is accepted with its rise read at +6900 ms;
  +6001 ms is "the name wasn't heard"; evidence past +7000 ms is not buffered;
- the lamp speaking from 0.4 s before the hearing, or the prompt's own tail
  0.3 s before, spoils the try; speaking 1.1 s after does not;
- a 5 s Unix clock step during the try, or a source restart, spoils it;
- NaN, infinite, >1 and <0 scores, impossible `ago_ms`, and out-of-range or
  NaN levels are refused at the boundary; a valid 0.94 is "unsure"; an invalid
  message during a try spoils it;
- persisted tries are checked on every load and commit (NaN/negative rise,
  score 1.2 or 0.9, 6 prompts, more tries than prompts, a `done` without a
  bar); a hand-edited progress file fails storage initialization;
- three unanswered prompts, or calls 18.0/28.1/20.0 dB, end `failed` with no
  file (an earlier file is removed); five prompts at most;
- nobody there for 30 s gives an untaken prompt back; a try nobody answered is
  judged without a hearing and later evidence is not taken; the next prompt is
  asked when someone is back;
- skip leaves an earlier file as it was and ignores late evidence; redo clears
  prompts, tries and the generation;
- a held near-call write keeps 1000 loop ticks under 1 s and nothing is done
  before it completes.

`onboarding::storage::tests::the_near_call_bar_is_written_or_removed_only_on_the_worker`
and `coordinator::tests::pilot_faults::a_held_near_call_write_does_not_interrupt_capture_privacy_or_stop`
(the real coordinator with the worker held inside the near-call write: capture
accepted, listening ready, physical mute and SIGTERM reach every role).

## Latency path

Prompt playback known whole (coordinator, `invitation_finished`, Unix clock
read) -> bus drained every 20 ms into at most 256 numbers -> try judged in
memory at prompt end + 7.3 s -> after the third accepted try the bar's write
is submitted to the onboarding storage worker (write, fsync, rename there) ->
the next poll records `done` and the worker commits progress. The coordinator
never waits on a file on this path. Not measured on a lamp.

## Not shown

A lamp run with a person; the hold measured on a lamp (that the model's
"Yes?" is never heard during a try).

## Rebase and reply hold (2026-10-19)

Rebased onto `origin/seventh/onboarding-privacy-proof` (d7df17aac: #1792 on
lamp-voice d3870f543 with Third's review fixes). Conflicts were the docs and a comment line in `crates/voice/Cargo.toml`;
`lamp-near-call` was already a `lamp-voice` dependency upstream. Fourth's #1892
consumer: `Calibrated::fits` now also refuses a try scored outside 0.95..=1;
this step only keeps tries `near_try` accepted (score at least 0.95, scores
over 1 refused as invalid) and `valid_try` checks the same range on load, so
what it writes still fits. No API changed. The step runs after privacy is
done, skipped or `not_available` (privacy without a Controls receipt source),
as before.
`onboarding::near_flow_tests::the_step_follows_privacy_that_is_not_available`
pins the `not_available` case; an early close's retry keeps privacy led, so
this step waits for it.

The reply hold: while a try's window is open (prompt end .. +7 s), an
utterance heard in it ends unanswered (`onboarding_near_call`), on both routes.
`coordinator::tests::second_route::a_call_in_the_near_call_window_is_not_answered_on_either_route`
runs the real coordinator loop with Gemini Live and with
`LAMP_VOICE_ROUTE=second`: inside the window nothing is played and the turn's
outcome is `onboarding_near_call`; with the window closed before the call the
same call is answered (48 samples from that route). With the gate forced off
the test fails. `onboarding::near_flow_tests::the_reply_is_held_only_in_the_try_window_and_the_held_call_counts`:
held from the prompt's end through 7 s, not before nor after nor once judged,
and the held call is accepted.

## Re-stack on #1792 43fcedc90 (2026-10-19)

Re-stacked onto `origin/seventh/onboarding-privacy-proof` at 43fcedc90 (#1792
folded, on lamp-voice 473038541). The near-call step applied cleanly. The
reply hold conflicted only in `coordinator/tests/second_route.rs`, where
lamp-voice had added #1670's
`the_second_route_alone_gives_mood_what_was_heard_and_the_voices_phases`
at the same place; both tests are kept. The rig's new `second_hears` field
defaults to `None` in `Script::question`, so the near-call test takes it
unchanged; `near_window_ms` and the `TEST_NEAR_WINDOW` setup merged without
conflict. Same toolchain and environment as above.

| # | Command | Result |
|---|---|---|
| 1 | `cargo +1.96.0 fmt --all --check` | pass |
| 2 | `cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings` | pass, no warnings |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib onboarding -- --test-threads=1` | 69 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib near -- --test-threads=1` | 30 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib a_call_in_the_near_call_window_is_not_answered_on_either_route -- --test-threads=1` | 1 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-near-call -- --test-threads=1` | 7 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-msgs onboarding -- --test-threads=1` | 1 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-skills onboarding -- --test-threads=1` | 3 passed |
| 4 | `cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1` | 2431 passed, 0 failed, 12 ignored (lib 2191) |
| 5 | `cargo +1.96.0 build -j2 --workspace` | pass |

## The reviewer's P3s on #1964 (2026-10-20)

- The hold keeps only the name. `NearFlow::holds_reply` (behind
  `Onboarding::near_holds_reply`) needs the window open and the name heard in
  it, beginning no more than 0.5 s before the utterance's first speech; the
  coordinator also lets through an utterance with more speech than a call
  (`CALL_SPEECH_BLOCKS`, 1.2 s). "skip that" with no name, and "Lamp, can we
  skip this?", are answered.
- The coordinator test runs the step's rule: `TEST_NEAR` holds a
  `NearFlow::listening_for_test` (window and name hearing), in place of
  `TEST_NEAR_WINDOW`'s bare range.
  `a_line_that_is_more_than_the_name_is_answered_in_the_near_call_window`
  covers no name (0.6 s) and the name with 1.8 s of speech, on both routes.
  Mutations, each reverted: `holds_reply` forced false fails
  `a_call_in_the_near_call_window_is_not_answered_on_either_route`; the length
  check removed, or the name requirement removed, fails
  `a_line_that_is_more_than_the_name_is_answered_in_the_near_call_window`.
  The closed-window coordinator case is gone; the window's bounds are pinned by
  `the_reply_is_held_only_in_the_try_window_and_the_held_call_counts`, which
  also pins the name and its 0.5 s lead.
- `lamp_msgs::OnboardingStep` reads a step it does not know as `Unknown`
  (`#[serde(other)]`), so an older lamp-skills beside a newer voice still asks
  for the invitation; the say service checks the token.
  `onboarding_pending_snapshot_is_asked_once_and_retry_is_an_explicit_edge`
  asks for a `giving_way` invitation.
