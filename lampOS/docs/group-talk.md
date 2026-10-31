# People talking around the lamp

What the lamp does when it is not alone with one person: several people
standing at it, or colleagues in conversation at the next desk. Status on
2026-10-13: four measures are on lamp-4ace and each has been seen to act
there, the last two only in a staged scene of played voices. With real
people talking around it the last two are not yet shown.

## What happens

The lamp ends a turn after 0.6 s of quiet and its answer begins 1.0 to 1.5 s
after that. Any speech it hears in between takes the turn: the answer is
dropped (`user_interrupted`) and the new speech becomes the next thing sent
to the model. Alone with one person that is right: they paused in the middle
of a thought and went on. In a group it means the lamp never gets a word in.

On lamp-4ace on 2026-10-13 people stood at the lamp for seven minutes,
talking to it and to each other (47 turns in the service's record):

- 21 of the 47 turns were replaced by more talk, 0.01 to 1.25 s after the
  end of speech was decided, all before any of the provider's answer had
  arrived.
- One of them asked "Why you not speaking up?".
- Twice the lamp answered a scrap of their talk to each other.

Later the same day, with two colleagues in conversation beside it, three
questions played to it by name in one scene ("Hey Lamp, what do you see in
front of you right now?", "Lamp, what is two plus two?") all went
unanswered: each was replaced by their talk before the lamp spoke.

How loud a voice is does not tell these apart. People standing at the lamp
are all as loud as the person speaking to it (a median of -15 to -29 dBFS
after cancellation that morning, either way).

## What is done about it

**Voices that are too faint are not speech** (`--faint-floor-db`, see
[serve-runtime.md](serve-runtime.md), "How loud an utterance was"). At
-40 dBFS this removed the phantom utterances from across the office. It
does nothing for people at the lamp.

**The level follows the room** (`crates/voice/src/room_floor.rs`,
`LAMP_FAINT_FLOOR_MAX_DB`). -40 was not enough in a working office: the
talk at the next desks arrived at a median of -27 to -35 dBFS, every
sentence of it lit the lamp up, and about thirty times in fifty minutes the
lamp answered. Set to -28 by hand it heard a third as much and answered
none of it. But a level right for that room at four in the afternoon turns
a quiet voice away from an empty one at eight. With the environment
variable set, the floor moves between `--faint-floor-db` and that highest
level. It follows the speech that was not for the lamp: what the detector
scores as speech outside any turn, and what was heard in turns that ended
by themselves with the lamp saying nothing; never a turn the lamp
answered, anything heard while it speaks, or a turn replaced by more talk.
It sits three decibels above the middle of that as it fades (by half in
about two and a half minutes), needs about three seconds of such speech to
move at all, and is back at its lowest within about a quarter of an hour
of a room emptying (six halvings take the most there can be under those
three seconds). Each move of a decibel or more is in the record as
`room_floor`, with `room_alone_dbfs`: where the room's talk alone would
put the floor before the two limits. That number is recorded with a fixed
floor too, so a room can be watched before the floor is let go in it.
Host tests for the rule (`room_floor::tests`).

**A question put to the lamp stays its own** (the character file,
[character.md](character.md)): "you will often be asked something and then
hear other people talking to each other before you have had the chance to
answer … answer it at the first pause, once, as if the talk in between had
not happened". The model hears everything in the session, so it can do
this. With that line alone and the two colleagues still talking, six
questions played by name 25 s apart: two answered within 2 s, one 11 s late
at the first pause, three not at all (18 turns were replaced by their talk
during the run). Before the line: none of the three above.

**In a busy room a just-asked lamp keeps its turn**
(`crates/voice/src/coordinator.rs`, `HOLD_FLOOR_US`; `Attention::busy` in
`crates/voice/src/body_link.rs`). The room counts as busy when the last three
things the lamp heard went unanswered: nothing it said, in a row, with less
than half a minute of quiet since. Then, from the moment a person's speech
is judged ended until the answer begins to play, a new sound does not take
the turn: the candidate is suppressed as the lamp's own playback would be,
and the service records `floor_held` once for that turn. If no answer has
begun to arrive 1.8 s after the speech ended, the hold lapses and the next
voice takes the turn as before, so a turn the model declines to answer
costs the room at most that. Once the lamp answers, the count is zero again
and the next three turns are handled as they always were.

It is tied to a busy room because of what it costs: speech that begins
during a hold is not sent to the model at all. For one person who pauses
and goes on, that would lose the start of their second half, and the lamp
would answer the first.

The first build of this ended the service the first two times it acted
(lamp-4ace, 12:22 and 12:28 on 2026-10-13, restarted by systemd four
seconds later each time, with its conversation gone). It dropped the
candidate with `suppress_prospective`, which checks the authority snapshot
against the time it is given, and it was given a time read a few
microseconds before that snapshot was issued. It now lets the candidate go
with `InputAdmission::release_prospective`, which consults no authority and
has its own test.

**In a busy room the lamp finishes the start of its answer**
(`KEEP_ANSWER_US`). A turn whose speech ended while the room was busy is
remembered, and for the first four seconds that the lamp is heard answering
it, what the microphone hears is not scored as speech: nothing over the
lamp's voice stops it and nothing starts a new turn. The service records
`answer_kept` once for the turn. Most answers are shorter than four
seconds; a story can be stopped once they have passed. The cost is that the
person who asked cannot stop it in those four seconds either.

**A short sound does not cut off an answer still to come
(`LAMP_WAIT_KEEPS_ANSWER=on`, #1123; off unless set, and set in the product's
voice unit).** On lamp-52e6 on
2026-10-16 (the sixth, 16:5x), short sounds with no words, 0.6 to 1.4 s
long, admitted 1.4 to 2.6 s after a question, ended 4 of 10 waiting answers
as `user_interrupted` on the backup voice, whose answers begin later. The
owner's session on lamp-4ace had dozens. A new sound took the turn as soon
as it began, before anything of it was known, and no words of it can be
had before then: the model hears them only once it takes the turn, which
is what ends the old answer. With the switch, in any room, while an answer
to someone is awaited and none of it has sounded (the person's input
ended, no provider audio, nothing playing), a new sound waits instead of
taking the turn at once (`while_answer_waits` in
`crates/voice/src/coordinator.rs`):
- it stays a pending candidate, its audio kept from its start (the mask is
  given the memory for it, `PlaybackMask::keep_waited_sound`: a candidate
  holds its 30 blocks of pre-roll and up to 1.2 s more, and with the usual
  128 blocks the oldest were gone and it was rejected,
  `retained_history_unavailable`, losing what was said; the reviewer and
  the first builder on #1130);
- 1 s after its trigger (`WAIT_KEEPS_FOR_US`), or 100 ms before its own
  decision deadline if that is sooner (`WAIT_DECIDES_BEFORE_US`), it is
  decided: let go if it has been quiet for 300 ms (`WAIT_QUIET_BLOCKS`),
  and otherwise it takes the turn as before, from its start;
- one that has ended before then is let go at once;
- once it has begun to wait it keeps these rules when the answer's audio
  arrives, for a cough is no less a cough then; but once the answer sounds
  (its audio waiting for the speaker or playing, as the duplex mask hears
  it), the mask scores a person's speech 0.0 until it has matched the
  lamp's sound, so the sound's pause from then on is the mask's silence.
  It is judged by its pause as it stood when the answer began to sound:
  ended or quiet for 300 ms, it is let go and the answer plays; still
  going, it is decided at once by the duplex rules for speech over the
  lamp, as without the switch, and stays decided (the first builder's
  re-check on #1130). So a cough still going when the answer begins can
  cut it, as before; one that had stopped cannot.

Nothing waits, and a new sound is decided as before, when its block comes
with no echo evidence (the mask keeps such a block only among its last 80
reads and 0.6 s, so a waited sound could not be held); when the turn is the
lamp's own (a hello, a look, a reminder, news, the head's "hm?": no input,
and a short "hi" or "not now" may answer it); when the answer has been
awaited more than 3 s since the person stopped (`WAIT_KEEPS_WITHIN_US`:
it may not come, and a "hello?" to a silent lamp must be heard; the backup
voice gives up a turn that has not begun in 3 s); when the lamp's name was
heard from the sound's start on (`name/rising` or `name/heard`: "Lamp, …
stop" is someone talking to it); and for a sound that begins within 1 s
after one was let go (`AFTER_LET_GO_US`: two in a row are more likely talk).

What it lets go, measured on the product's mask and hold
(`what_the_wait_lets_go_and_takes_by_length`): a sound of 0.75 s or less
(it has ended, or gone quiet for 300 ms, by its decision). From 0.8 s it
takes the turn. So of the sixth's 0.6 to 1.4 s sounds, the 0.8 s and longer
ones still cut the answer. What it costs:
- a real follow-up of 0.75 s or less ("No.", "Wait.") said while the
  answer is still to come is let go, and the lamp answers what was asked
  first; said again once the answer plays, it cuts in as before;
- a follow-up with a pause across its 1 s mark (0.7 s, 0.4 s of quiet,
  0.6 s; or 0.6 s, 0.5 s, 0.5 s) loses its first piece: the piece after the
  pause is taken at once, without it (`a_follow_up_with_a_pause_loses_only_its_first_piece`);
- a real new question in the gap is taken 1 s after its trigger, not at
  once.

Let go, the answer goes on; the record says `input_let_go_answer_waits`
with why (`quiet` or `ended`), and `answer_waits` once for each sound that
waited. Both voices are covered, as the backup voice takes its turns from
the same place (`coordinator/second.rs`). On the product's duplex hold a
candidate may wait 1.2 s for its decision, which is where the 1 s comes
from; on the standard one (200 ms) the wait ends 100 ms before the
deadline, so a sound is taken 100 ms later than before and never let go.
Tests, in `coordinator/tests/turn_taking.rs`, on the product's duplex mask
hearing each block with echo evidence, a full pre-roll, its hold, the real
admission and detector: the lengths above;
`while_an_answer_waits_a_short_sound_is_let_go_and_the_answer_goes_on`
(and taken at once with the switch off, on the same path);
`while_an_answer_waits_a_sound_that_goes_on_takes_the_turn_from_its_start`;
`a_waited_sound_needs_the_memory_the_wait_gives_the_mask`;
`once_the_answer_sounds_a_waited_sound_is_judged_by_its_pause_then` (the
answer really playing, its echo not yet matched);
`a_sound_heard_without_echo_evidence_does_not_wait`;
`a_sound_with_the_lamps_name_in_it_takes_the_turn`;
`the_wait_is_only_for_an_answer_to_someone_awaited_a_short_while`;
`the_wait_is_on_only_with_on_or_words`. The loop's own call is not driven
by a test (#1087).

**With `LAMP_WAIT_KEEPS_ANSWER=words`, a waited sound is judged by its
words too** (#1123 step 2, `crates/voice/src/wait_words.rs`; off unless
set, and the wait is `on`'s alone without the second voice's configuration,
`LAMP_VOICE_SECOND_CONFIG`). With `on`, a real short "No." or "Wait." is
let go with the cough (#1130's "most likely wrong"). With `words`,
everything `on` does, and:
- while a new sound may still wait (an answer to someone awaited unheard,
  within 3 s of the request's end), a speech-to-text stream is kept ready:
  the second voice's, as the stop words use it (`RelayEars::for_waits`).
  One takes 0.6 to 0.8 s to connect. It rests once no sound may wait and
  none is in hand, so a long answer keeps no idle stream open to the relay
  (`words_awaited`, the seventh's P3). It closes with a Stop, the mute, a
  capture gap or the microphone's privacy, beside the stop words' stream.
  Those `close` calls in the loop are driven by no test, as the stop
  words' aren't; `WaitWords::close` itself is
  (`closing_drops_the_sound_and_the_stream`). With privacy on, a sound in
  hand is dropped only by that call (the seventh's re-check).
- a waited sound's audio goes to it from its first kept block (its
  pre-roll), each block once, while it waits.
- its words are asked for once it has been quiet for 100 ms
  (`ASK_AFTER_QUIET_BLOCKS`, as the stop words ask), well before its
  endpoint (600 ms).
- words back with anything past those that ask nothing: it is decided at
  once, as a sound that went on is. With nothing playing it takes the turn
  from its start. Once the answer sounds, the duplex rules decide it, as
  for a sound still going then. The asker-first rule still applies after
  it.
- words back with nothing past those: `on` decides it, as before.
- those that ask nothing: fillers ("um", "uh", "hmm", `FILLERS`), hums of
  only m and h ("mm-hm"), laughs of only h, a and e ("haha", "heh"), and
  acknowledgements ("okay", "yeah", "yes", "right", "alright",
  `ACKNOWLEDGEMENTS`). So a laugh, or the asker's "Okay.", doesn't cut off
  the answer it waits for (the seventh's P2). "No" and "what" ask, alone
  or after them. A stop alone, or after them ("Okay, stop."), is a stop
  (next).
- words back that only tell the lamp to stop (`only_a_stop`): a stop
  phrase by the stop words' own list (`stop_words::stop_phrase`), and
  nothing more that asks. That covers "Stop.", "Okay, stop it, please.",
  "Hold on." and "Wait.". The sound is let go and never asked of the model,
  and the answer it waits for is stopped on the next pass, as a stop said
  over the lamp's speech stops it (`hand_control_cancel` with
  `spoken_stop`). It's recorded as `input_let_go_answer_waits` with `why:
  stop_said`, and as `wait_words` with `stop: true`. Before, "Stop." took
  the turn and went to the model as a request, which answered it (the
  reviewer's P3 on #1277). A stop with more asked ("Wait, what about
  tomorrow?", "Stop, what's the weather?") takes the turn as before, and
  "Don't stop." is no stop.
  A stop counts only when no speech came after its words were asked for:
  "Wait, … what about tomorrow?" with a comma's pause is asked for at
  the pause and comes back "Wait.", but the person has gone on, so it
  takes the turn from its start as any word does (the reviewer's P2 on
  #1352; `WaitWords::asked_through_us` against the loop's `last_speech_us`).
  A stop is most likely just before the answer begins, when `on` has
  already decided to take the sound because the answer started to sound.
  If that sound's words were asked for and it has been quiet since, it is
  still judged by its words: a stop stops the answer, and while they
  aren't back it waits for them up to the same deadline
  (`wait_words::then_once_decided`; the seventh's P2 on #1352). It waits
  only on a block the mask heard with echo evidence: a block without is
  kept only among the mask's last reads (80, and 0.6 s), so a sound held
  then loses its first reads and the loop's decision fails, ending the
  conversation (`coordinator::may_hold_once_decided`; the reviewer's P1 on
  #1352). So a stop whose words are still out when the answer sounds
  without echo evidence goes as `on` decided: the answer is not stopped.
  Two
  stops still take the turn as before, because `on` decides them before
  any words are asked for: one with the lamp's name ("Lamp, stop now."),
  and one still going at its 1 s mark (a slow "Okay, stop it, please.").
  `only_a_stop` reads them as stops; the loop never asks it (the
  seventh's P3 on #1352).
- where `on` would let it go and its words aren't back: it waits for them
  while it hasn't ended, up to 100 ms before its deadline, and is then let
  go as with `on`. A sound that has ended is never held. The next sound's
  start would find it still pending, which the admission refuses, and that
  ends the conversation
  (`with_words_a_sound_is_never_held_for_them_past_its_end`).

How long the words may take once asked for, measured on the product's mask
and hold (`how_long_the_words_may_take_by_length`):
- 490 ms for a sound of 0.5 s or less (it is let go at its endpoint);
- 450 ms at 0.6 s;
- 350 ms at 0.7 s;
- 300 ms at 0.75 s (these are let go as quiet at their 1 s mark, and wait to
  100 ms before their deadline).

Through the relay on lamp-4ace the words came 225 and 269 ms after the
audio (`stt-check`, #1047, in `stop-word.md`). Asked at the 300 ms of quiet
that `on` lets go on, they would have had 290 ms, and 250 down to 90 ms
for sounds of 0.6 to 0.76 s.

A 0.4 s "No." whose words come back in 100 ms is taken 210 ms after it
stops. Without the switch it was taken at once, on its trigger; with `on`
it was let go.

What it costs:
- **privacy:** the audio of any sound that waits goes to the second voice's
  speech-to-text (`LAMP_VOICE_SECOND_CONFIG`; on lamp-4ace the company
  relay, with Deepgram's `nova-3` behind it):
  - any sound the detector starts on in the 3 s after a request ends while
    its answer is unheard, asked or not: the person's "No.", a cough, a
    television, a colleague's words to someone else;
  - at most about 1.3 s of each: its 300 ms of pre-roll, and up to 1 s
    after it set the detector off.

  The stop words' privacy line (`stop-word.md`, "Privacy: what goes
  where"), accepted for lamp-4ace only, doesn't cover it, and it is to be
  decided for this too before home lamps;
- someone else's short words (a television, a colleague) in the gap take
  the turn as a follow-up would, where `on` let them go, unless they only
  laugh or acknowledge;
- a short "Yes." or "Okay." meant as an answer in the gap is let go, as
  with `on`;
- the lists of what asks nothing are English: with the speech-to-text in
  another language, a lone "Dạ." or "Ừ." counts as a word and takes the
  turn (the seventh's re-check). A lamp set to another language needs its
  own lists before `words` is turned on there;
- a sound with a pause of more than 100 ms in it is judged by its words
  before the pause: "Uh, … no" is let go, as with `on`;
- with no words back, a sound `on` lets go as quiet at its 1 s mark is let
  go up to 100 ms later;
- a sound that goes on (0.8 s and longer) but paused for 100 ms is taken
  sooner than with `on`, once its words come back.

Recorded:
- `wait_keeps_answer` at the start, with `words` and
  `words_asked_after_quiet_blocks`;
- `wait_words` once a sound, with:
  - `words`: the count of its words that ask something, or `null` if none
    came back in time;
  - `took`;
  - `asked_at_us` and `at_us`: from the ask to the decision.

The words themselves aren't recorded. The journal names the stream
`wait words`.

Tests:
- in `wait_words.rs`, on scripted ears: the stream's life, the filler
  count, and the decision;
- in `coordinator/tests/turn_taking.rs`, through the function the loop calls
  (`with_waited_words`), on the product's mask, hold, admission and
  detector: `with_words_a_short_no_while_an_answer_waits_takes_the_turn_from_its_start`,
  `with_words_a_cough_a_filler_or_no_words_in_time_are_let_go`,
  `with_words_a_sound_is_never_held_for_them_past_its_end`,
  `how_long_the_words_may_take_by_length`,
  `a_stream_for_words_is_kept_ready_only_while_a_sound_may_wait`,
  `with_words_a_lone_stop_while_an_answer_waits_stops_it_and_is_never_asked`,
  `with_words_a_stop_the_person_went_on_from_takes_the_turn_and_a_lone_one_stops`,
  `with_words_a_stop_just_before_the_answer_sounds_still_stops_it` and
  `with_words_a_sound_decided_without_echo_evidence_is_never_held_for_its_words`.
  The stop itself is the loop's spoken-stop path, which no test drives
  through the loop, as for the stop words.

The real relay is not driven by a test. The lamp check, on lamp-4ace with
the second voice configured: a question, then in the gap before its answer
"No." and "Wait." five times each, and five coughs. Count `wait_words`
(`took`, `words`, `null`) and `input_let_go_answer_waits`, with the time
from `asked_at_us` to `at_us`. Then five "Stop."s said just before the
answer begins, counting `stop_said` against them. `last_speech_us` is
the detector's score before the playback mask, so once the answer
sounds the lamp's own sound may count as speech after the ask, and such a
stop then goes as `on` decided. The rig drives that clock only from the
person's score, so only the lamp shows it (the reviewer's P3 on #1352).

**The lamp's own motors take no turn (`LAMP_MOTION_KEEPS_ANSWER=on`, #1712's
B; off unless set, and it needs `LAMP_BUS=on`).** A joint that moves makes a
sound the microphone takes for a voice. On lamp-4ace at 04:34 on 20 Oct the
thinking pose's elbow sank 2.5 s into a slow first answer, a line with no
words began with it, and it took the turn from the request in flight: the
answer was cut, and the 0-word line was held. That sound goes on for as
long as the joint moves, so the wait above takes it as a line that goes
on. With the switch, the voice reads the body's joints (`body/joint_states`,
about 12.5 times a second, `crate::body_motion`):
- a joint that moves, or is sent, 4 servo counts or more between two of the
  body's messages (50 counts a second) is moving, from the message before
  the one that showed it (`MOVED_RADIANS`; the thinking head moves at 50 a
  second and the base at 100, a held servo's jitter is a count or two);
- while an answer is awaited and none of it has sounded (the person's
  input ended, no provider audio), a new sound whose start (its first block
  to its trigger) began within a move, or 100 ms after one (`MARGIN_US`), is
  the lamp's own sound and is let go, as its playout would be. The answer
  goes on, and the service records `input_let_go_body_moving` with the
  move's span (`body_heard` in `crates/voice/src/coordinator.rs`);
- a move that began after the sound's start is not its: the lamp turning
  to a voice is the voice's;
- the lamp's name heard with it is still a call, as for the wait above;
- until the body has said where it was at the sound's trigger, the sound
  waits, at most 120 ms after it (`WAITS_US`), under the 200 ms the
  admission has to decide. A body silent 300 ms (not running, or no arm)
  holds nothing, and a line is heard as before;
- not while the lamp sounds: there the duplex hears what is its own, and a
  person cutting in over a nod still yields it.

Each message is dated by its wall-clock time on the voice's clock, since
the body's own `at_us` counts from its start. The cost: a person who
starts speaking just as the lamp moves, while it thinks, is let go at
that start; the rest of what they say begins a new line once the move is
over. A move is under a second.

**The asker's voice isn't the motors (`LAMP_MOTION_VOICE=record|on`, the
first builder on #1722, 21 Oct 03:27; off unless set, and it needs
`LAMP_ASKER_FIRST` on or `record` for the asker's level).** The rule above
was built on that one 04:34 line, whose level was never recorded. On
lamp-4ace on 21 Oct its 168 let-gos all fell on played lines (within 0.2 s
before to 0.3 s after one), and at 03:11 the rest of "my name is" was let
go four times as the body turned to the voice. The resting sway is
inaudible on mic2 (-84 dBFS, the same as stillness, 40 dB under speech);
the big moves (turning to a voice, standing for a question) are still to
be measured, with a person at the arm.
- A sound the rule finds begun as a joint moved is read on its own speech
  blocks, from its first read to the rule's decision, against the asker's
  level as the asker-first rule keeps it: the middle of their own speech
  blocks, away from the lamp's echo (`AskerFirst::levels_since`).
- With at least 6 of its own speech blocks, and their upper tenth no more
  than 6 dB under the asker's (`motion_voice::MARGIN`, asker-first's, until
  the big moves are measured), it is the asker's
  (`crate::motion_voice`). The upper tenth, as asker-first judges: a voice's
  loud blocks are its words, and a motor's whine is flat. On 6 to 10 blocks
  it's about the loudest, so this leans to keeping (a few loud motor blocks
  over a quiet hum pass it). `record` writes the middle too, so the bar can
  be set on either before `on` (Fourth's P3 2 on #2371).
- Decided once for each sound: a sound kept is not judged again when a
  later rule holds it and it's seen on more blocks (the reviewer's P3 1).
- `record`: it is let go as before, and `input_let_go_body_moving` also
  carries `asker_level`, `heard_level`, `heard_upper_tenth`, `blocks` and
  `would_keep`, so the bar comes from real lines.
- `on`: the asker's goes on to the rules after this one, as if the body
  were still (`input_kept_body_moving`, with the same levels).
- Without the asker's level (asker-first off, or too little of the request
  heard), or with too few blocks of its own, it is let go as before.
- No new wait: it judges the blocks the sound has when the rule decides,
  inside its wait for the body's word.
- **Reading `record`'s lines:** `lamp-voice-eval motion-voice SESSIONS
  --plays SCENES` lines each let-go up with the played lines and gives the
  bar that keeps the lines and lets the rest go (`voice-eval.md`).
- **The known cost of `on`:** a motor sound as loud as the asker is
  theirs too, as at 04:34 if that line was so loud. It goes on to the
  answer wait, which lets it go only if it ends or goes quiet first.

**Counting it** (#1866). On lamp-4ace after a played test, for both routes,
with `lamp-voice-eval` from the ARM64 build:

    journalctl -u lamp-voice --since '<test start>' -o short-iso \
      | sudo ./lamp-voice-eval waiting-cuts

`--since`/`--until 'YYYY-MM-DD HH:MM[:SS]'` filter the piped journal's own
stamps (they don't call journalctl); `--json` for JSON; `--sessions DIR`
for records elsewhere. The journal alone can't say it:
none of the wait's decisions, nor whether what cut an answer had words, nor
a turn's route, is a journal line. They are the session's records
(`/var/lib/lampOS/voice/sessions/<invocation>/metadata/events-*.jsonl`,
root's only), which the tool reads, never writes, and pairs with the
journal's runs by when each session's folder was made. The journal gives
each run's start, `second only`, `wait words` and the second route's own
lines. Per run (its route at start and wait mode: `off`, `on`, `words`) and
per route, it counts:
- `wait_held_a_sound` (`answer_waits`), and how each ended:
  `wait_let_go_quiet`, `wait_let_go_ended`, `wait_stop_said`,
  `wait_took_turn`; with `words`, what the words said
  (`wait_words_asking`, `_asking_nothing`, `_none_in_time`);
- `cut_waiting_by_words` and `cut_waiting_by_no_words`: an answer cut
  (`user_interrupted`) before any of its audio came, by a turn whose input
  transcript had a word or had none, and how many of those had waited
  (`_after_a_wait`); `short_follow_up_kept`: one with words under 80 speech
  blocks; `cut_while_answer_sounds` apart;
- stops (`stop_hand`, `stop_head`, `stop_spoken`), and speech that went on
  (`went_on_after_early_end`, and the second route's `second_goes_on`);
- times in ms (n, median, p90, max): `wait_hold_ms` (sound held to its
  decision), `wait_words_ms` (asked to decided), `awaited_before_cut_ms`
  (the request's end to the cut), `stop_dispatch_ms`,
  `wait_stop_to_cancel_ms`.

It prints counts, category names and numbers only, never a word heard or
said; a journal line near these (wait, stop, request, route, cut,
interrupt; `waiting_cuts::NEAR`) that it does
not know is counted as `unclassified`, unprinted. Not countable: a short
follow-up lost with `on` (it is let go unheard, so whether it had words is
never known), and a stop the person went on from (#1352: it takes the turn
as any word does, with no record of its own). Code:
`crates/voice-eval/src/waiting_cuts.rs`; tested in
`crates/voice-eval/tests/it/waiting_cuts.rs` on an invented journal excerpt and
sessions with no real words (`crates/voice-eval/tests/data/waiting-cuts/`).

**A line the lamp starts on its own isn't ended by the room's sound
(`LAMP_OWN_LINE_KEEPS=on`, #2220; off unless set).**
- **The fault:** on lamp-06c1 on 20 Oct (El, #806's step 7), two of three of
  Welcome back's hellos were dropped as `user_interrupted`. A sound the room
  made about 2 s in was admitted before any of the hello's audio
  (`provider_audio_seen: false`). The wait above is only for an answer to
  someone, so any sound ended the lamp's own line.
- **With the switch,** while a line the lamp started on its own has not
  sounded, a new sound waits instead of taking the turn
  (`while_own_line_waits` in `crates/voice/src/coordinator.rs`). Such a line
  answers no person's words, has had no audio arrive, and has nothing
  playing. It is one of:
  - a skill's say;
  - a reminder;
  - news the lamp tells unasked (`lamp_tells_news`);
  - a line of the setup walk (`lamp_leads_setup`).

  A written turn that follows a person's, a look's or a "Sorry?", is as
  before.
  - **The lamp's name** heard as the sound began (`name/rising`, up to
    `CALL_BEFORE_US` before its trigger) takes the turn at once, as before.
  - **A face turned to the lamp and speaking** (#2287's follow-up, the first
    builder at 17:49 on 20 Oct): a sound the eyes saw a face speak through,
    turned to the lamp, is decided as before, at once.
    - **What counts:** speaking and `facing` in at least half of the eyes'
      looks from the sound's first read to its last speech, plus 300 ms for
      the eyes' lag (`OWN_LINE_EYES_LAG_US`), and at least three looks
      (`SpeakingFaces::turned_speakers`). The looks after a short line
      don't count against it (the reviewer's P3 on #2308).
    - **Whose it is:** any face turned to the lamp and speaking, not
      checked as the person the line is for. At the desk that is almost
      always them, and their "not now" is no room's talk. The record says
      `own_line_theirs` once, and the journal says so.
    - **The body's motors:** a sound decided this way isn't taken for the
      body's motors by the rule after it (`LAMP_MOTION_KEEPS_ANSWER`), as
      the lamp's name isn't. Welcome back turns the head before its hello,
      so a "not now" begun as it turns would otherwise be lost there.
    - **The eyes' rate:** with a face in view they look about four times a
      second. A 0.6 s "Not now." then has two or three looks, at the edge
      of the three it needs. So a very short line may still wait as the
      room's; the lamp run should note the eyes' rate.
    - **The reader:** the coordinator reads the eyes' faces every 100 ms off
      the audio path (`speaking_faces::read_from`), only with either switch
      (`LAMP_OWN_LINE_KEEPS` or `LAMP_OWN_LINE_OVER_TALK`).
      It keeps passing ids and two booleans, and nothing past 30 s.
    - **It needs the eyes to judge speaking while the sound waits:**
      `LAMP_EYES_SPEAKING=on` with the mouth model running. With
      `--mouth-on-heard on` they judge it only once the voice hears a line,
      and a sound waiting here isn't heard yet. Without that reading, the
      person's sound waits as the room's does.
  - **With `LAMP_WAIT_KEEPS_ANSWER=words`,** a waiting sound's words are heard
    as the answer's wait hears them (`own_line_with_words`, through
    `with_waited_words`; the stream is kept ready while an own line hasn't
    sounded, `own_line_awaited`).
    - A lone stop ("Stop.", "Wait.") is let go, never asked of the model, and
      the line is stopped (`input_let_go_own_line` with `why` `stop_said`).
    - So is a lone put-off ("Not now.", "Later.", "Not yet.", "No thanks.";
      `wait_words::only_a_put_off`), but only in a room that isn't busy. In
      a busy room a put-off is as usual between colleagues ("No thanks." to
      a coffee, "In a minute.") as to the lamp, so it's the room's and
      waits (the reviewer's P3 on #2322). A face turned to the lamp and
      speaking is still decided as before, above.
    - Any other words are the room's: the sound waits and is let go as
      before. In a busy office two colleagues' talk has words too, and it
      mustn't end the hello (the reviewer's P2 on #2308). A face turned to
      the lamp and speaking is still decided as before, above.
    - The record's `wait_words` says `"for":"own_line"`, with the count of
      words only.
    - **Privacy:** with both switches, a waiting sound's audio before every
      hello, reminder, news and walk line goes to the relay for its words,
      room talk included. That is the line `switches.md` already names for
      `words`, still to be decided, now reached by more sounds.
  - **Any other sound** is let go once the line begins to sound, or 100 ms
    before the sound's own decision deadline (`WAIT_DECIDES_BEFORE_US`). The
    line goes on. The record says `own_line_waits` once for each sound
    that waits, then `input_let_go_own_line` with `why` (`sounded` or
    `deadline`), and the journal says it was let go.
  - **A sound let go stays that sound while it goes on** (#2602), however
    the wait ended while it was held: at its deadline (`deadline`), because
    the line began to sound (`sounded`), or going on from one let go
    (`goes_on`).
    - **At its deadline**, from El's run on lamp-06c1 at 22:21:49 on 21 Oct:
      played room talk was let go at +2.144 s. The hello's first audio came
      at +2.291 s, and the same talk, still playing, rose again as a new
      sound at +2.343 s and cut the hello as a cut-in.
    - **As the line sounded**, from El's run on lamp-06c1 at 08:17–09:08 on
      22 Oct (build 5378, with the deadline case): in two of three hellos
      the waiting talk was let go as `sounded` at the first audio, and came
      back as a new sound 56 ms and 67 ms after `provider_first_audio`,
      cutting the hello.
    - **What counts as the same sound** (`LetGoSound`): from the last block
      heard as speech when it was let go, each block of speech carries it
      on. Quiet as long as the wait's own (`WAIT_QUIET_BLOCKS`, 300 ms) ends
      it. A new sound under the same line's turn that begins before such a
      gap is that sound going on.
    - **It is let go again** once the line sounds (`input_let_go_own_line`
      with `why` `goes_on`), and kept again from there. Before the line
      sounds it waits as any sound does.
    - **Still a cut-in:** the lamp's name heard in it, or a face seen
      speaking through it turned to the lamp, as above. So is a sound after
      a 300 ms gap, and speech that starts after the first audio with
      nothing let go before it.
    - **Its cost:** someone who starts speaking while the room's talk let go
      goes on, with no gap, doesn't cut the hello in until there is one,
      unless they say the lamp's name or are seen speaking turned to it.
      The wait has no rule for a near or loud voice of its own, so a person
      close by with neither is heard only after a 300 ms gap.
  - **A sound that begins once the line sounds** is a cut-in over the lamp's
    voice, judged by the duplex rules as for any of its speech. So words said
    over the hello still stop it.
  - **A block the mask heard without echo evidence** (a duplex lamp's block
    with no `echo`) is decided as before, as the answer's wait does: such a
    block is kept only among the mask's last reads, so a sound held longer
    couldn't be held whole (the reviewer on #2242).
- **Its cost:** someone who answers the lamp before its hello sounds, without
  its name, and not seen speaking to it, is let go with the room. Without
  `words`, that includes "not now" and "stop", unless the eyes saw them
  speaking turned to the lamp. They then say it again over the line or after
  it, and said over it, it stops it. So the first builder keeps the switch
  off until a lamp shows the person's own "not now" ends the hello.
- **Tested** as functions:
  - `a_line_the_lamp_starts_waits_out_the_rooms_sound_until_it_sounds`, with
    the person's case;
  - `a_sound_let_go_at_its_deadline_stays_that_sound_past_the_own_lines_first_audio`,
    with the 22:21:49 timing, a gap, speech after the first audio, and the
    switch off;
  - `a_sound_held_until_the_own_line_sounds_stays_that_sound_past_its_first_audio`,
    with 22 Oct's 56 ms and 67 ms, each end of the wait (`deadline`,
    `sounded`, `goes_on`), the name, a turned face, a gap, speech after the
    first audio, and the switch off;
  - `with_words_a_stop_said_before_the_own_line_sounds_stops_it`: a stop and
    a put-off stop the line; a colleague's words wait;
  - `wait_words`' `a_put_off_is_only_what_puts_the_lamp_off`;
  - `speaking_faces`' `a_face_speaking_turned_to_the_lamp_is_told_apart_from_the_room`.

  The loop's own calls aren't driven by a test, as the wait's aren't. On a
  lamp:
  - played room noise over three hellos, before and after;
  - the person at the lamp saying "not now" before a hello sounds, with the
    eyes judging speaking: the hello ends;
  - with `words`, "stop" or "not now" before it sounds stops it, and two
    colleagues talking nearby don't, nor a colleague's "no thanks" in a busy
    room;
  - the cut-in test of a person speaking over one, unchanged.

**Talk over the lamp's own line in a busy room isn't a cut-in
(`LAMP_OWN_LINE_OVER_TALK=on`, #2602's design; off unless set).**
- **The fault:** on lamp-a63e on 22 Oct a skill's say ended `dropped` as
  `user_interrupted` under office talk. `LAMP_OWN_LINE_KEEPS` keeps an own
  line only until it sounds; once it is audible, talk over it is a cut-in.
  `LAMP_CUT_INS_NOT_TALK` doesn't reach this: it only stops the provider
  worker counting cut-ins toward its busy room.
- **With the switch,** while one of the lamp's own lines (the same lines as
  above: a skill's say, a reminder, news, its hello, a setup line) is
  sounding (it has reached the speaker: the speaker accepted some of it,
  `Reply::reached_speaker`, so a line paused since still counts; audio
  still queued, or handed over and not yet accepted, does not), talk over
  it is let go and the line goes on (`while_own_line_waits`, `why`
  `busy_room`) when all of these hold:
  - the room was busy as the line began
    (`body_link::Attention::busy_lately`: busy now, or within the last
    180 s, room_told's `BUSY_ENDS_AFTER_US`, borrowed, not measured). It is
    taken once per turn, before the line's own audio counts as an answer
    and clears `busy`;
  - the lamp's name wasn't heard as it began (`name_began_us`, up to
    `CALL_BEFORE_US` before its trigger). `lamp-name` says a name only once
    it is over, so the talk stays pending (`ForName`, the line playing on)
    until `OWN_LINE_NAME_LAG_US` (600 ms) after its trigger, or 100 ms
    before its decision deadline if sooner, and only then is let go. On
    lamp-52e6 on 22 Oct "Lamp, wait." was said 482 ms after it began, its
    talk had been let go on its first blocks, and the say went on `spoken`;
  - the eyes saw no face speaking through it turned to the lamp
    (`turned_speakers`, as above; the eyes' reader runs with either switch);
  - `lamp-name` heard no stop word as it began (`stop/heard`, only when
    `LAMP_STOP_HEARD` runs that listener).
- **Still stops it:** "Lamp, …", a person turned to the lamp and speaking, a
  head tap or the button, and any talk in a room that wasn't busy. An answer
  to a person keeps today's rules. Needs no other switch.
- **No pause either:** while such a line sounds (`busy_room_line`), the
  duplex mask doesn't yield to talk over it (`PlaybackMask::room_talk`):
  a yield withholds the line's audio, the speaker plays zeros, and on
  lamp-52e6 on 22 Oct the say ended `dropped`
  (`audio_written_with_playback_gaps_unscored`) though its talk was let go
  31 and 40 times. The talk is still heard as speech and judged as
  candidates for the name and a turned face (one with them takes the turn as
  before, with no yield first); a stop heard by its own listener
  (`stop/heard`) still yields. Fruitless counts are untouched: with no
  yield there is no resume.
- **Record:** `input_let_go_own_line` with `why` `busy_room`, one per
  candidate: talk going on is a new candidate every few blocks once the last
  is let go, each judged for the name and a face. The journal says `talk
  over the lamp's own line in a busy room, with no name or face turned to
  it, is let go; the line goes on` once a line.
- **The name's wait, its limits:** `lamp-name --not-while-speaking` is
  off in the product, and on lamp-4ace on 18 Oct the lamp's own "I've made
  the light warmer" scored 0.990 as the name. So in the 600 ms wait an echo
  of the lamp's own line with the name in it, over busy talk, stops the
  line, where before it was let go on the first blocks; turning that flag
  on would instead drop every "Lamp, wait" said over the lamp's own line.
  The 600 ms runs from the talk's trigger, so "Okay so, Lamp, wait", its
  name ending more than 600 ms in, is still let go. The cap at the decision
  deadline less 100 ms leaves about 100 ms on a half-duplex
  `Hold::STANDARD` lamp; the product is duplex.
- **Its cost:** a bare "Stop." in a busy room no longer stops a say. The
  name, a face turned to it, or a tap still does, and once the sixth
  builder's stop-words fix (#2863) is on, `stop/heard` covers the bare
  "Stop" again (`LAMP_STOP_HEARD` is off in the product today). Where the
  lamp answers office talk, `Attention` is rarely busy and the rule rarely
  bites.
- **The busy window:** the 180 s is borrowed from room_told
  (`BUSY_ENDS_AFTER_US`), not measured for this rule; it is to be measured
  on lamp-52e6 in the lamp run.
- **Tested:** `talk_over_the_lamps_own_line_in_a_busy_room_is_let_go` (the
  table: name, face, stop word, a room not busy, an answer to a person, not
  yet sounding, audio queued but never handed to the speaker, a line that
  sounded and paused, a block handed over and not yet accepted);
  `talk_over_the_lamps_own_line_in_a_busy_room_leaves_no_playback_gap`
  (the duplex harness: 200 blocks of talk over the line, each candidate let
  go, every block handed to the speaker in order with no yield; without
  `room_talk` the yields withhold it; a candidate not let go takes the turn;
  a forced yield still yields);
  `a_room_busy_lately_stays_so_for_three_minutes_after_it_was_last_busy`
  (30 s and 181 s); and on the loop,
  `talk_over_a_say_in_a_busy_room_is_let_go_with_the_switch`: the say ends
  `spoken` with the switch in a busy room, and `dropped` without it or in a
  room not busy. "Lamp, wait" over it is covered by the table only: the
  loop harness hears no name. A name said late:
  `a_name_said_late_in_talk_over_the_lamps_own_line_in_a_busy_room_still_stops_it`
  (pending on its first blocks, the name or a stop word said 600 ms after
  the talk began takes it as before, none by then lets it go, a short
  hold's deadline lets it go first).

**With `LAMP_ASKER_FIRST=on`, the person asked comes first while the
answer is awaited** (`crates/voice/src/asker_first.rs`, #829; off unless
set, and `record` to judge it without acting, as the product's voice unit
runs it). On lamp-4ace in the busy
office at 14:21 on 2026-10-16, "Hey Lamp, I got the job today!" had no
answer yet 4.7 s after it ended. Then "¿Qué?" from across the room was
taken as a new request and replaced it (`user_interrupted`): the floor
hold had lapsed at 1.8 s. A person who has been asked something goes on
with whoever asked, and leaves the talk across the room alone.

- **The asker's level** is the middle of their own turn's speech blocks,
  heard while they said it, none within 0.3 s of the lamp's sound
  (`ECHO_TAIL_US`): not the lamp's echo, nor talk before the turn. The
  service's `local_endpoint` record gives it as `asker_level`, on the
  face's scale, after cancellation. With fewer than six such blocks there
  is no rule for that turn.
- **The rule:** from the end of the asker's speech, for at most 5 s
  (`KEEPS_FOR_US`) and until the answer begins to play, new speech is
  judged on its own blocks, from its own start. It takes the turn as soon
  as its upper tenth is no more than 6 dB under the asker's level
  (`MARGIN`, 45 on the face's scale): the asker going on, at their usual
  level, is taken at the six blocks that begin any turn, as without the
  rule, and a soft first word ("um, and…") before it does not keep them
  out. Speech whose upper tenth is under that waits as long as its
  candidate can, kept as it is: until its decision deadline or its oldest
  block's read history (600 ms, `turn_taking::HISTORY_US`) is 0.1 s off,
  or the utterance has ended. Nothing of it is thrown away, so if it then
  takes the turn, because the lamp's name came or louder speech did, it
  begins at its start. In duplex that is about 0.26 s after it began
  (0.21 s after its trigger, the six blocks that begin a turn): a
  name announced later than that (the name part announces it 78 to
  639 ms after it begins, `name.md`) comes too late. Still well under the
  asker at its last chance, it takes no turn and the lamp keeps the
  answer: the candidate is let go as for the floor hold. While the rule
  waits on a line and the answer has not begun, **the answer's first block
  waits for the line's judgement**, no later than its last chance, so the
  line is judged in the clear. Over the reply's first 0.2 to 0.45 s on
  lamp-4ace the evidence can't tell a person from the lamp
  (`Hearing::fresh_blocks`), and the lamp's own voice let through by the
  canceller is as loud as a person on a fifth of its blocks
  (`duplex-listening.md`). Judged over the answer's start, the asker's own
  follow-up after a soft "um" was let go on that alone, and a line from
  across the room took the turn on the lamp's voice (the reviewer on
  #1197). Well under the asker at its last chance, the line is let go and
  the answer plays; otherwise it is judged as anything said over the
  lamp's voice. Should the hold's bound run out before the line is judged,
  the rule hears over the answer only the blocks the render does not
  explain, as the duplex rule hears a person over the lamp
  (`Hearing::heard(…).person()`); that guard is not shown by a test. Left to the duplex
  rule, a line from across the room was admitted a block into the answer
  and replaced it, as "¿Qué?" replaced the answer at 14:21. Judged at the
  answer's first block, the asker's own follow-up opening with a soft
  "um" was let go on that alone, and the asker was not heard at all (the
  reviewer on #1197). The service
  records `asker_first` once for the turn, with both levels and `acted`,
  and the journal says by how many decibels. In any room, busy or not.
- **Why it ends after 5 s:** an answer held back by the rule for whom it
  answers never plays, and someone who took their question back ("never
  mind") must be heard again; the floor hold stops at 1.8 s for the same
  reason. Ordinary answers begin 1.0 to 2.0 s after the asker stops
  (`connected-conversation-20261011.md`, 32 turns), so 5 s covers them.
  It rests on one sound, 14:21's at 4.7 s, and it can't tell a held
  answer from a slow one. A line the rule kept is kept until it pauses
  for 0.3 s (`LINE_PAUSE_US`), past the 5 s too, so it never takes the
  turn halfway through: a line across the room begun at 4.7 s was kept
  at its start and taken mid-line at 5 s, 0.29 s into it (the reviewer
  on #1092). It is kept for at most 2 s past the 5 s (`LINE_MOST_US`):
  steady talk across the room that never paused for 0.3 s kept the rule
  on with no end (the first builder on #1092), so it ends 7 s after the
  asker at the latest, though the line goes on. The rest of a line still
  going then takes the turn mid-line: a line at ±300 from 4.7 s to 8.7 s
  after the asker is kept to 7 s and taken 2.3 s into it, as a 2.6 s
  request (the reviewer on #1197). That is the cost of the bound.
- **What still takes the turn:** the asker, or anyone about as loud; and
  the lamp's name heard beginning after the asker's speech ended
  (`name/rising` on the bus, from the name part), however quiet.
- **Over the lamp's own voice nothing changes:** talk from across the room
  is mostly cancelled with the echo there, and the duplex rule judges
  what is left ([duplex-listening.md](duplex-listening.md)).
- **Its cost:** someone near the lamp who speaks much more softly than the
  asker is dropped, not held: they must say it again once the answer
  begins, or use its name. So is the asker's own softer follow-up after
  a loud first line (an excited "I got the job today!", then an ordinary
  "and…", is easily 6 dB down), and the asker leaning back from 30 to
  60 cm, about 6 dB. A soft "never mind" before an ordinary answer is
  dropped too, and the answer plays: the 5 s don't help there, and the
  asker can only stop it over the answer. The dropped speaker is given no
  sign. A person asked
  something while they answer someone else at least glances at them: a
  small glance toward them when the rule keeps the turn, once a turn, is
  a follow-up.
- **Latency:** speech at the asker's level is admitted when it would be
  without the rule. When a quiet line waits as the answer arrives, the
  answer's first block waits too, until that line's last chance: about
  0.26 s from the line's start at most, and the trace says
  `asker_first_answer_held`. Only speech whose upper tenth is under the margin
  waits, until about 0.26 s after it begins (its last chance), before it is
  admitted or dropped.
- **The 6 dB is not yet measured** on the voice's microphone. It is a
  start from the room microphone's rises in [ears.md](ears.md): 19.5 to
  26.5 dB over the room's quiet for people at the desk, 9.8 to 15.1 dB
  for people farther off, and those overlap (desk talk to someone else,
  14.7 to 23.5 dB). `LAMP_ASKER_FIRST=record` is how it is set: the rule's
  record with `acted` false, the turn taken as without the rule, beside
  each turn's `asker_level` and each request's `speech_level`. Recorded
  only, it never waits: the candidate is taken at once and followed, and
  the record is written where `on` would have decided, on the same own
  blocks (`judged`: `at_its_last_chance`). Under `on`, one judged after
  the hold's bound let the answer begin says so (`judged`:
  `after_its_answer_began`).

## What is and is not shown

- `body_link::tests`: when a room counts as busy, and that an answer or
  half a minute of quiet ends it. `admission::tests`: that a pending
  candidate can be let go without authority and an accepted stream cannot.
  The hold and the kept answer themselves are a few lines each in the
  service's main loop and have no test of their own.
- `asker_first::tests`: the margin, judging on the new speech's own
  blocks, the wait and the last chance, the 5 s, a kept line and its 2 s
  bound, the asker's own turn's
  level, the name, a new request beginning afresh, and the switch's
  values. In the service's own loop with an answer awaited
  (`coordinator::tests::endpoint_session`):
  - `with_asker_first_quieter_talk_takes_no_turn_from_an_awaited_answer`:
    a second line 16.5 dB under the first takes no turn, and `asker_first`
    is recorded once with 178 and 54, over 25 blocks or more. One 4.4 dB
    under it takes the turn; one 8 dB under it does not. Recorded only, or
    without the switch, the quiet one takes the turn.
  - `with_asker_first_a_soft_start_to_the_askers_follow_up_is_kept_whole`:
    the asker's follow-up opening with 0.2 s at ±300 reaches the provider
    as 164 blocks, as without the rule (124 when it was judged at 60 ms,
    the reviewer's probe on #1092).
  - `with_asker_first_the_asker_just_after_talk_across_the_room_is_heard_whole`:
    half a second at ±300, then the asker 0.1 s later. The first of it
    takes no turn; the turn taken holds all of the asker's speech, the
    rest of the talk across the room before it when that ran on into
    theirs (0.22 s of the asker was cut when judged on the half second
    before).
  - `with_asker_first_a_short_quiet_line_ended_while_waiting_ends_nothing`:
    a 15-block quiet line, then the asker 0.3 s or 0.65 s after it. The
    session goes on, and the asker's turn is taken (the first builder's
    P1 on #1092: one kept waiting past its read history was refused and
    left the detector mid-utterance).
  - `with_asker_first_the_rule_ends_after_its_time_though_no_answer_played`.
  - `with_asker_first_a_late_short_word_is_decided_while_its_reads_last`:
    a 0.2 s word at ±300, 4.0, 4.2 or 4.5 s after the asker's end, takes
    no turn and is recorded, and the session goes on (the reviewer's P1 at
    00:00: waiting past 0.8 s of reads ended the voice or refused it).
  - `with_asker_first_a_line_kept_near_the_five_seconds_is_kept_whole`.
  - `with_asker_first_record_judges_where_on_does`.
  - `with_asker_first_a_loud_burst_just_before_does_not_let_soft_speech_in`:
    four blocks at the asker's level, then soft speech 60 ms later, takes
    no turn: speech is judged on its own blocks.
  - `with_asker_first_the_lamps_echo_is_not_the_askers_level`: the lamp
    answers and falls silent, and the next request opens on 0.2 s of its
    echo at ±6,000 before the asker's own ±800. The asker's level is 118,
    the ±800, and a line at ±700 while that answer is awaited takes the
    turn. With the echo counted it was kept out.
  - `with_asker_first_a_line_the_answer_begins_over_is_let_go_and_the_answer_plays`:
    a line at ±300 the answer arrives 0.2 s into, with echo evidence as
    lamp-4ace's microphone gives it. The answer's first block waits for the
    line (`asker_first_answer_held`, under 0.3 s); the line is let go at
    its last chance, recorded with `acted` true, and the answer plays
    uncut. Without the rule it takes the turn before the answer comes.
    Recording only, as #1092 did, it was admitted a block into the answer.
  - `with_asker_first_the_askers_soft_start_over_the_answers_start_takes_the_turn`:
    the asker's follow-up opening with 0.2 s at ±300, then their own
    ±2,000, the answer beginning during the soft part: taken, with `on`,
    `record` and off alike, and nothing recorded (the reviewer's probe on
    #1197, where it was let go on its soft start).
  - `with_asker_first_the_answer_waits_for_a_line_judged_in_the_clear`:
    the same soft start, with the evidence unmatched for the answer's first
    0.3 s, as on lamp-4ace. The answer waits, the line is judged in the
    clear, and the asker takes the turn with `on`, `record` and off alike
    (the reviewer's 04:50 P2; judged over the unmatched blocks it was let go).
  Each fails with its change taken out. The name's part is shown by the
  rule's own test only.
- A staged busy room on lamp-4ace (`hold.sh`: three lines between two
  played voices, a question to the lamp by name, and the other voice
  starting a third of a second after the question; twice over). With the
  hold alone: the second question's answer began 1.32 s after the
  end-of-speech decision, under the other voice ("It's a beautiful, bright
  blue."), `floor_held` once, and that voice stopped it 1.2 s in. With the
  kept answer as well: both questions answered whole ("Four. Easy one.",
  "It's bright blue! Such a nice, cheerful color."), 1.40 and 1.28 s after
  the decision, `answer_kept` twice, and nothing said to the lines that
  were not for it. No restart in either run.
- That is played speech, one scene, twice. With the two colleagues really
  talking beside it, only the first two measures have been tried.

Not solved, and known:

- After those four seconds, and in a room that does not count as busy, a
  voice over the lamp is judged by the rules for being interrupted
  ([duplex-listening.md](duplex-listening.md)), and people who carry on
  talking beside it can stop it.
- A room is busy only after three things in a row went unanswered. The
  first question put to the lamp in a conversation that has just begun
  beside it is handled as it always was.
- The lamp cannot tell who is speaking to it except by what is said. Its
  eyes report where a face is, and nothing uses that to decide whose turn
  it is.
- The lamp's light and head still react to every one of the turns it does
  not answer, until the head settles after two.
