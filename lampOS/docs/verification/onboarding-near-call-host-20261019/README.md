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

A lamp run with a person; the model staying quiet after the person says its
name; the attention rule's handling of the lamp's own reply during the step.
