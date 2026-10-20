# Trying to break the voice changes of 2026-10-14

Task 2 of the running list ([work-split.md](work-split.md)), by the second
builder. It adds tests and this page and changes nothing the lamp does.

**What became of it.** The first builder took the page and its tests onto
`lamp-voice` the same morning and mended there the five faults shown by a
test and the five found by reading. What was done for each, and
which three tests were changed and why, is under "What the lamp showed"
in [work-split.md](work-split.md). The page below is as the review wrote
it: where it says a test fails "today", or that something "is" so, that
was the code before those mends.

## What was looked at

Everything that changed on the night of 2026-10-14 for the four things
the task names, read end to end, with `git log -p` of each commit:

| What | Commits | Code |
|---|---|---|
| The name heard in the sound lets out an answer the rule would hold | `bd53393bd`, `c1d230a68` | `crates/voice/src/attention.rs`, `bus_out.rs` (`NameIn`), `provider_worker.rs` (`name_in`) |
| A second look at a call that got nothing | `407c55f26` | `crates/voice/src/second_look.rs`, `coordinator.rs` |
| The model told its name was heard while the person speaks | `7a07ebd9e` | `crates/voice/src/called.rs`, `crates/gemini/src/recovery.rs` (`try_note`), `provider_worker.rs` |
| The part that hears the name | `4dd0051fc`, `6e1392926` | `crates/name/src/{main,said,hear}.rs`, `bus_out.rs` (`MicOut`) |

`f9f336ac2` and `dd82d462b` came and went the same night; together they
change no code. The task says all of it is live on lamp-4ace. The voice
unit committed in `deploy/lamp-4ace/` does not set these switches
(`LAMP_ATTENTION`, `LAMP_SECOND_LOOK`, `LAMP_NAME_NOTE`, `LAMP_BUS`,
`LAMP_BUS_AUDIO`), so which of them are on was not checked here.

What was asked for: anything that could make the lamp speak when it
should not, stay silent to someone who called it, answer twice, go round
in a loop, or lose its link to the speech service.

The tests are in four files, all named for this review:

- `crates/gemini/tests/it/review_2026_10_14.rs`
- `crates/voice/src/provider_worker/tests/review_2026_10_14.rs` (the
  provider worker's own loop against the scripted service; the one line
  `mod review_2026_10_14;` was added at the end of the worker's test
  module to reach it)
- `crates/voice/tests/review_2026_10_14.rs`
- `crates/name/tests/it/review_2026_10_14.rs`

A test whose name begins `review_` shows a fault and fails today. It says
what should happen, so it passes once the fault is mended. A test whose
name begins `review_sound_` is an edge that was tried and held; it passes.

## Findings, most serious first

### 1. A note about one request is given to the next one, whenever it comes

**Speaks when it should not.** Test:
`review_a_note_for_a_request_given_up_is_not_carried_into_the_next_request`
in `crates/gemini/tests/it/review_2026_10_14.rs`.

What happens:

1. Someone says "Hey Lamp". The name part hears it plainly. The worker
   sends the note ("your own ears caught your name, Lamp, just now ... it
   is said to you") inside that request's activity, beside its sound. On
   the lamp a hearing was on the bus 78 to 639 ms after the name began
   ([name.md](name.md)), and an utterance ends only after 600 ms of
   quiet, so the note is usually out before the request ends.
2. The request is then given up before its end. The coordinator throws
   out an utterance it judges a sound (`not_speech`: "Good night, Lamp."
   went this way in 5 of 8 played runs on 2026-10-13, before the faint
   floor's hold, and a played "Hello?" in 6 of 6 on 2026-10-14,
   [not-speech.md](not-speech.md)), or the microphone is closed. The
   worker abandons the request and closes its input. No activityEnd is
   written, by design, so the service is not asked to answer something
   nobody owns. The activity stays open.
3. The next thing anyone says, a second or ten minutes later, by the
   same person or by two other people talking, continues that activity.
   The service gets: the start, the given-up sound, the note, the new
   sound, the end.

The model is told the new request was said to it by name "just now". On
a running session, or a quiet room, nothing holds its answer. The test
shows it with the session set up both ways, as the lamp runs it
(`kept-start`) and without:

    ["activityStart", "audio of 1", "note", "audio of 2", "activityEnd"]

The same carry-over of a given-up request's sound is older and on
purpose (`open_activity` in the session's tests). The note makes it worse
because it is a claim about now. One case it helps: "Hey Lamp" thrown out,
then the question a moment later. A mend would keep that and drop the
rest; one way is for the note to be written only when its own request is
committed, at its end.

### 2. A name heard after a request's sound has ended lets out that request's answer

**Speaks when it should not.** Test:
`review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer`
in `crates/voice/src/provider_worker/tests/review_2026_10_14.rs`.

The rule counts a hearing for a request "at any time while it is in hand"
([attention.md](attention.md)), and in the worker "in hand" lasts until
the model's first sound.
That covers a name said at the start or in the middle of what was said.
It also covers a name said by someone else after the request's sound
ended, while its answer is awaited.

The sequence, in a busy room on a new session:

1. Two people talk. "How was your weekend?" is the session's first
   request. Its sound ends; the rule would hold the answer.
2. A second later, before the model's first sound, someone else begins a
   line with a name like the lamp's ("Hey Liam, the meeting moved.",
   "Hey man, ..."). The name part reports such lines about half the time
   at 0.9 ([name.md](name.md)). In a busy room that line does not take the
   turn while the first answer is awaited (`floor_held`, up to 1.8 s).
3. The hearing reaches the worker. The model's answer to "How was your
   weekend?" begins, and is let out: `its answer is played: the lamp's
   name was heard in the room's sound`.

The test runs the worker's own loop, the scripted service and a real bus.
The same request with no hearing is held
(`a_new_sessions_first_answers_to_talk_in_a_busy_room_are_held_and_later_ones_are_the_models`).
With the name heard as the request began it is played, as it should be
(`review_sound_a_name_heard_as_a_request_began_lets_out_the_answer_the_rule_would_hold`).

The second look has the same fault, wider: it counts a hearing until the
turn is over, which for a request that got nothing is four seconds or more
after its sound ended. See "Found by reading" below.

### 3. The lamp's own second look uses up a new session's protection

**Speaks when it should not.** Test:
`review_the_lamps_own_second_look_does_not_use_up_a_new_sessions_held_requests`
in `crates/voice/tests/review_2026_10_14.rs`.

The rule holds answers to a new session's first two requests in a busy
room, because that is where the answers to other people's talk were
(`FRESH_REQUESTS`). It counts every request that starts on the session,
the lamp's own written turns too.

1. A new session in a busy room. Its first request is a line to someone
   with a name like the lamp's. The name part hears it plainly (three of
   ten such lines reached 0.98 on lamp-4ace). The model rightly says
   nothing.
2. The voice takes a second look: a written turn of its own. The model
   rightly says nothing again.
3. The next line of the two people's talk is only the second thing the
   session has heard. It is counted as the third, the rule does not apply,
   and its answer is the model's to give. Without the rule, "How was your
   weekend?" was answered in all three runs ([attention.md](attention.md)).

A hello or a look counts the same way; the second look is what makes it
happen in a busy room.

### 4. A plain hearing just before a request is lost to a fainter one

**Silent to someone who called it**, when the model also missed the call.
Test:
`review_a_plain_hearing_just_before_a_request_is_not_lost_to_a_fainter_one_after_it`
in `crates/voice/tests/review_2026_10_14.rs`.

Before a request begins, the second look keeps only the newest hearing.
Inside a request it keeps the plainest. So "Hey Lamp" heard at 1.000,
then a faint 0.91 hearing 1.2 s later, then the request beginning 0.2 s
after that (the voice can be slow to be sure in a busy room or over its
own voice), leaves only the 0.91, under the 0.98 a second look needs. The
call gets nothing. Narrow: two hearings must fall in the 1.5 s before the
request begins.

### 5. A hearing can be said with a score over 1

**No effect on what the lamp does.** Test:
`review_a_hearing_is_never_said_with_a_score_over_one` in
`crates/name/tests/it/review_2026_10_14.rs`.

`said.rs` holds the first score of a hearing to 1 and not the later ones.
Nothing holds the model's output to 0..1 (`Hearer::load` checks only that
silence scores in range). A hearing said with 1.02 breaks its own type's
rule (`NameHeard::check`). The voice does not check it; another reader of
`name/heard` may. It only matters if `name.onnx` can give more than 1.

## Found by reading, not shown by a test

Each of these needs a small way in for a test that the code does not
have. None was added, by the task's rule.

- **A call answered with a spoken "..." gets no second look.** Silent to
  someone who called it. When an answer opens with no word in it, or
  with a function's name said aloud, the coordinator ends the turn with
  `cancel_reply(.., "nothing_said")` (`coordinator.rs`, about line 2559).
  `second_look.finished` is called only on the ordinary end of a turn
  (about line 3330), so this one is never offered a second look. The
  model has kept quiet this way to a question put to it by name: "The
  question put to it by name and the one that follows it got '...' in
  both runs" ([conversation-thread.md](conversation-thread.md), with a
  wording of the thread note since taken out), and "..." came back on a
  later character file, three times in one scene
  ([not-speech.md](not-speech.md)). Needs: the second look switched on
  other than from the environment, so that a coordinator test can turn
  it on.
- **The second look counts a hearing until the turn is over** (finding 2,
  in the coordinator). Speaks when it should not. A request that got
  nothing is over only when the service's four-second wait has run out,
  so a plain hearing in those seconds earns a second look, and the note
  then tells the model "you were being called". Where the attention rule
  held an answer, the coordinator cannot tell that from the model saying
  nothing, so the second look asks the model again in a turn of the
  lamp's own, which the rule always plays: a held answer to other
  people's talk can come back out. Needs: the end of the request's sound
  passed to `SecondLook`, and the second look switched on other than from
  the environment.
- **A second look can go to a session that never heard the call.**
  Speaks when it should not. The note says "what was just said to you".
  It is sent to whatever session is ready (`cloud.ready_session()`). If
  the link was renewed in between (the worker renews when idle after
  eight unanswered requests in a row, at most every fifteen minutes, and
  an unanswered call is one of the eight) or dropped and came back
  fresh, the session has not heard it, and is told to "answer it now".
  Needs: a coordinator test with the second look switched on.
- **`lamp-name` dates a hearing 0.8 s too early when sound stops in the
  middle of it.** `main.rs` closes an open hearing at a gap or a lost
  message with `hearings.tick(now + LONGEST)`, so `ago_ms` is 800 ms more
  than it was. A call then has 0.7 s, not 1.5 s, to count for the request
  that follows. Rare: the sound must stop while the name is being heard.
  Needs: the loop in `main.rs` taken apart from the models and the bus.
- **The worker says nothing if it cannot take hearings from the bus.**
  `NameIn::on(root).ok()` in `provider_worker.rs`. The coordinator's own
  `NameIn::from_environment` says so in the journal. On a lamp where it
  fails, the rule runs on the service's text alone, and nothing records
  it.

## Suspected, not shown

- **The lamp hearing its own name.** `MicOut` puts on the bus everything
  the voice is given after its echo canceller, also while the lamp is
  speaking, and nothing sets apart a hearing made then. The name part
  reported one hearing with nothing played, "while or just after the lamp
  answered" ([name.md](name.md)). If the lamp says "Lamp" in an answer and
  the canceller leaves enough of it, the next request within 1.5 s is
  taken as called, by the rule, the note and the second look. Needs the
  lamp's microphone.
- **A late answer after a second look.** The second look is sent when the
  service's four-second wait for an answer has run out. If the model's
  answer to the call comes after that, it is taken for the second look's
  answer, and the second look's own answer follows: the call answered
  twice. Whether the service ever answers after four seconds of nothing
  was not looked at.
- **"Hey Lamp," then a long pause, then the question.** If the question
  begins more than 1.5 s after the name was heard, as a new request, the
  name counts for neither the rule nor the second look on it. With the
  first part thrown out as a sound, its sound and the note (finding 1)
  are in the question's activity: the model is told it was called and the
  rule, on a new session in a busy room, holds what it says.
- **A silent second look is counted as talk the lamp left alone.**
  `room_told` counts each request that ends unanswered toward a busy room
  (three in a minute). A near-name line the model leaves alone, and its
  second look left alone too, count two.

## A trade-off, not counted as a fault

The words taken from the text as a call on that night (`c1d230a68`:
"Helen", "Mom", "Amen", "Weiland", first or after a greeting) are also how
ordinary talk opens: "Hi Mom, I'm still at work" on the phone, "Helen,
can you send it?". In a new session's first two requests in a busy room,
each lets an answer out. "Man," and "plan" were there before, and so was
"lamb" anywhere in the text. [attention.md](attention.md) already says
the text is read only as evidence; this is the cost of that, for the
first builder to weigh.

## Looked at and found sound

- **The note cannot end the link.** In `crates/gemini` an error from a
  command ends the connection, and the session refuses a note for a
  request that is not the open one. `Supervisor::try_note` checks the
  same thing first, on the same thread that ends and retires requests:
  the request must be the one submitted on the live connection, with its
  input open. `try_end` marks it ended at once, and the note and the end
  go through one queue in order. A note for a retired request, or behind
  a start that is kept back, is dropped without an error. Tried, both
  ways the session is set up:
  `review_sound_a_note_at_a_wrong_moment_is_refused_here_and_the_link_stays`.
- **The second look is taken once.** Its own turn is not a heard request,
  so nothing is owed for it, whatever is heard while it is in hand; one
  not taken in three seconds is dropped; a new request drops it:
  `review_sound_a_second_look_is_owed_once_and_its_own_turn_owes_nothing`
  and the unit tests in `second_look.rs`. No loop was found.
- **The note is offered once for a request**, never on a faint or old
  hearing: `review_sound_the_note_is_offered_once_for_the_request_the_name_began`
  and the unit tests in `called.rs`.
- **The name counts for its own request only**, across a new session
  too, and the verdict is made once:
  `review_sound_a_name_heard_counts_for_its_own_request_only`, and through
  the worker's own loop,
  `review_sound_a_name_heard_as_a_request_began_lets_out_the_answer_the_rule_would_hold`.
- **One call is one hearing**, with the hold-off, a score that is not a
  number opening nothing:
  `review_sound_one_call_is_one_hearing_dated_from_its_first_high_score`.
- **The second look's written turn cannot break the worker or the
  session.** It is committed only when nothing is owed, by the same path
  and on the same conditions as a hello or a look, whose tests already
  cover it. The bus parts (`NameIn`, `MicOut`) never wait and drop their
  errors, so the bus cannot stop the voice. No way was found for the
  night's changes to lose the link.

## How the tests were run

On an Apple M2 Max laptop, macOS 26.6.2, the host's own toolchain (Rust
1.96.0, as `rust-toolchain.toml` pins), from `lampOS/`, on `second/voice-review`
at `origin/lamp-voice` `e93a828d1` with these tests added. Not on the
lamp, and not in a Linux container.

Each command once, in this order:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and the same for `lamp-gemini` and `lamp-name` | clean |
| `cargo build --workspace` | builds |
| `cargo test -p lamp-gemini` | library 226 passed, 2 ignored; `comparison_config` 10 and `wire` 12 passed; `review_2026_10_14` 1 passed, 1 failed: `review_a_note_for_a_request_given_up_is_not_carried_into_the_next_request` |
| `cargo test -p lamp-name` | library 3 and program 3 passed; `review_2026_10_14` 1 passed, 1 failed: `review_a_hearing_is_never_said_with_a_score_over_one` |
| `cargo test -p lamp-voice` | library 726 passed, 5 failed, 5 ignored (below); every file in `tests/` passed except `review_2026_10_14`: 3 passed, 2 failed, `review_the_lamps_own_second_look_does_not_use_up_a_new_sessions_held_requests` and `review_a_plain_hearing_just_before_a_request_is_not_lost_to_a_fainter_one_after_it` |

Of the five library failures in `lamp-voice`, one is this review's
(`review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer`).
The other four are older tests that fail on this laptop whenever the whole
library runs at once: `coordinator::motor::tests::stale_snapshot_does_not_revoke_newer_idle_authority`,
`unresolved_cleanup_removes_the_service_mute_restart_token`,
`native_head::native_head_replayed_receipt_and_changed_camera_grant_are_terminal`
and `privacy_does_not_hide_actual_discard_or_close_failure`. Each stops
with "owned child timed out" after its 2 s wait for a child process. How
many fail changes from run to run: 21 in an earlier full run, all of them
in `coordinator::motor` and `coordinator::optional_ring`. The same 21
failed, and nothing else, with `origin/lamp-voice` as it is (this review's
one line in `provider_worker.rs` taken out again): 708 passed, 21 failed.
Run one at a time (`cargo test -p lamp-voice --lib -- --test-threads=1
coordinator::motor optional_ring`), all 46 tests of those two groups pass.

So 5 tests fail because of a fault (one per finding), and 6 tests of edges
that were tried pass. The two tests in the worker's loop take about five
seconds each and take turns: each hearing is said four seconds old, so
that no other test's worker, which hears the same bus, can take it for its
own (see the notes at the top of that file).

## Not shown

- Anything on the lamp: its microphone, its echo canceller, the name part
  on its processor, the real speech service. Every test here runs on a
  laptop against a scripted service.
- How often any of these happens on the lamp. The findings say what can
  happen and in which order, not how often.
- Whether the real service, given a note inside an activity, treats it as
  the scripted one does (as part of the request).
- The coordinator's second-look wiring: it is switched on only from the
  environment, so no test here could turn it on.
