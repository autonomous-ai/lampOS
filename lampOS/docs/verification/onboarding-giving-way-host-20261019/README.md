# Onboarding giving-way step, host verification (2026-10-19, #1800)

Host only (macOS, aarch64). No lamp, no body, no device, no person. Toolchain
`cargo +1.96.0`, environment `RUSTC_WRAPPER=` (empty),
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`,
`-j2`, `--test-threads=1`, run from `lampOS/` in sequence.

| # | Command | Result |
|---|---|---|
| 1 | `cargo +1.96.0 fmt --all --check` | pass |
| 2 | `cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings` | pass, no warnings |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 onboarding` | 76 passed, 0 failed (7 of them `giving_way_flow_tests`) |
| 3 | `cargo +1.96.0 test -j2 -p lamp-msgs -- --test-threads=1 onboarding` | 1 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-skills -- --test-threads=1 onboarding` | 3 passed |
| 3 | `cargo +1.96.0 test -j2 -p lamp-body -- --test-threads=1` | 470 passed, 0 failed (`HAND_MOVED` moved to the crate root) |
| 4 | `cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1` | 2442 passed, 0 failed, 12 ignored (lib 2213) |
| 5 | `cargo +1.96.0 build -j2 --workspace` | pass |

## What the tests show

`onboarding::giving_way_flow_tests` (7), a fake body (`Felt::joints`,
`Felt::hand` as the bus would carry them, source `body`) on a scripted Unix
clock:

- the base moved 30 counts with its goal following, then the hand let go:
  `giving_way` done, `GIVE_DONE` said, one `nod` asked, kept across a restart;
  the model's `setup_step giving_way done` is refused by the worker and by the
  runner's mailbox;
- a hand still on the head is not done; two unfelt minutes fail with
  `GIVE_FAILED`, no move asked; a release from before the second invitation's
  end is not counted;
- a 10-count bump (under `HAND_MOVED`), 30 counts with the goal left behind
  (the body pushing back), a body restart mid-push, another publisher and
  evidence from before the invitation ended are not done; the same push then
  is;
- a 5 s Unix clock step over the try refuses it ("the clock jumped");
- without a body the step is `not_available` and nothing is offered; the
  keeper's pause during a try records `not_available` and a later push is
  ignored;
- after a skip or a redo, the same push changes nothing;
- with the outcome's progress write held on the storage worker, 1,000 loops
  of evidence and ticks take under a second, nothing is done or moved before
  the write lands, and done follows its release.

## Not shown

A lamp run with a person: a real hand on lamp-52e6 or lamp-4ace, whether the
body's `body/hand` and `body/joint_states` carry a gentle push as the tests
script it, and the time from the hand letting go to the lamp's word. No
coordinator test drives an onboarding invitation through the say service on
either route.
