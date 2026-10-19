# Onboarding `how_you_sound` sentence part, host verification (2026-10-20, #1803)

Host only (macOS, aarch64). No lamp, no device, no person. Toolchain
`cargo +1.96.0`, environment `RUSTC_WRAPPER=` (empty),
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`,
`-j2`, `--test-threads=1`, run from `lampOS/` in sequence, on top of the
near-call window change (8476918d8).

| # | Command | Result |
|---|---|---|
| 1 | `cargo +1.96.0 fmt --all --check` | pass |
| 2 | `cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings` | pass, no warnings |
| 3 | `cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 onboarding near_call sentence` | 88 passed, 0 failed |
| 4 | `cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1` | 2440 passed, 0 failed, 12 ignored (lib 2199) |
| 5 | `cargo +1.96.0 build -j2 --workspace` | pass |

## What the tests show

`onboarding::near_flow_tests`, the sentence tests at its end (scripted bus,
Unix clock and line evidence):

- after three accepted calls the bar is written and the name part is done,
  but the step is not: the sentence prompt follows (invitation attempt 6);
  its 15 s window opens at the prompt's end and the near-call gate stays shut;
- a line said close with 9 words for 3 s of speech ends the step `done`;
  `progress.json` has no field for the sentence beyond `sentence_prompts`;
- a line not said close is coached "a little louder" once, then a good one
  is `done`; one word for 4 s of speech is coached "a little closer", and a
  line whose words never came back then ends it `failed`
  (`SENTENCE_FAILED`: "I'll get used to your voice as we talk"), with the
  name part's bar still in place;
- with no bar (nobody called the name), the sentence part still decides the
  step (`done`, no `near-call.json`); a kept `done` needs `sentence_prompts`
  1 or 2;
- a line ending past the window is not taken; nothing heard is judged 3 s
  after the window ("louder"); a new voice cycle writes the bar again and
  asks again ("say another sentence"); with both prompts spent and none
  judged it fails; redo clears `sentence_prompts`; a skip ends the step
  `skipped` and late lines and words change nothing.

`coordinator::tests::second_route::a_sentence_in_its_window_is_counted_and_not_answered_on_either_route`:
on the second route alone, the line's End reaches the route, its words come
back (9 counted), the turn ends `onboarding_sentence` with nothing played, and
no record holds any of its words; on Gemini Live it ends at the answer's first
audio. Past the window the same line is answered on both.

## Not shown

A lamp run with a person; whether the 1 s-a-word coverage and the near
rule's bar suit real sentences said from a chair. On Gemini Live the model's
own session hears the sentence, as it hears every line.
