# The same voice (#829)

Whether two things said to the lamp are the same voice. The question
comes from two people at the lamp: when B follows up on A's question
("And what about tomorrow?"), or A and B go on talking to each other after
the lamp answered, the busy-room rule now takes anyone who speaks within
45 s of an answer as the conversation going on
([attention.md](attention.md)). A face turned to the lamp cannot stand in
for it. On lamp-4ace on 15 October, 6 of the 9 answers let out because a
conversation was going on had nobody facing, all in one live exchange
([#829, 21:23](https://github.com/autonomous-ai/lampOS/issues/829); its
evidence is with pull request #864, `docs/verification/two-people-20261015/`).

## What exists

`crates/same-voice` (`lamp-same-voice`), built by the fourth builder on 15
October 2026: a library, a command to measure with, and since 16 October
the voice's child (below, "In the voice"). Nothing on the lamp runs it
until its model is put there and the voice is told where.

- **The model:** CAM++ from 3D-Speaker (Apache-2.0), as sherpa-onnx exports
  it (`3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, 29,596,978
  bytes, SHA-256
  `357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b`, from
  the `speaker-recongition-models` release of k2-fsa/sherpa-onnx). It has
  7.2 M weights and reports 0.73% EER on VoxCeleb1-O. It is not in the
  repository.
- **The front end:** 80 Kaldi filter banks a frame, computed in Rust as
  `torchaudio.compliance.kaldi.fbank` computes them with no dither, with
  the utterance's mean taken off each band, as 3D-Speaker does.
- **The run:** tract 0.23.8, pinned as in `lamp-name`, with the number of
  frames left open, so a clip of any length runs.
- `lamp-same-voice pairs MODEL.onnx LIST.tsv` prints the cosine of every
  pair of clips and how long each took.

## Measured (fourth builder's Intel Mac, 2026-10-15)

47 played lines from lamp-4ace's runs of 14 October, as the second
microphone gave them to the voice after the canceller: Daniel 28, Karen 9,
Samantha 10, all synthetic (`say`) from the Mac's loudspeaker. Clips with
the lamp sounding through more than half of either clip are counted apart
([verification](verification/same-voice-20261015/README.md)).

| Pairs | Same voice | Different | Equal error |
|---|---|---|---|
| The lamp quiet in both | 75 | 96 | 6.5% (cosine 0.29) |
| … the shorter clip 1.5 to 3 s | 16 | 29 | 0 errors |
| … the shorter clip under 1.5 s | 38 | 67 | 7.7% |
| The lamp sounding over one or both | 195 | 349 | 47.1% |

With the lamp quiet, two 6 s lines of the same voice scored a median 0.95.
With the lamp speaking over one of them, its own voice is what the model
hears: no better than chance. So the same voice can tell a follow-up after
the lamp has finished from another person's, and cannot tell who speaks
over it.

A clip took 75 ms for 1.5 s and 280 ms for 6 s on one thread of the Mac,
and loading took 1.8 s. The lamp's A733 has not been timed.

## In the voice (#829, 16 October)

With `LAMP_SPOKEN_TO=on`, a line said within 45 s of the lamp's last words
is taken for the conversation going on, whoever says it. On lamp-4ace on
16 October at 06:07, right after the lamp answered Daniel's "Hey Lamp, how
much is one hundred dollars in euros?", Samantha said "Anyway, if we drop
the trip we save almost half." to him. It rode on his conversation, and
only the model's own silence left it unanswered. With the model's path in
the voice's environment, `LAMP_SAME_VOICE_MODEL=/absolute/path.onnx` (the
worker's `--same-voice-model PATH`, after `--spoken-to`), a line in a voice
plainly not the conversation's is no part of it
(`crates/voice/src/same_voice.rs`):

- **Its own process.** The provider worker starts `lamp-same-voice serve
  MODEL`, installed beside `lamp-voice`, as it starts, so the model loads
  while the link opens. The child reads each line's sound on standard input
  and answers its print on standard output, one line of JSON
  (`lamp_same_voice::serve`). It takes one line at a time: a line asked for
  while it is busy, loading or being started again gets no print. A child
  that ends, or answers something that is not an answer to the line asked,
  is started again after 5 s. Without the child the rule is as without the
  model.
- **Only what was said with the lamp quiet.** The coordinator marks a
  `Start` that was admitted while a reply was in hand (`over_the_lamp`).
  Such a line, and a turn the lamp began itself, is never judged: with the
  lamp over it, the model hears the lamp (47.1% equal error, above).
- **Only a line of 1.5 s or more** (`LEAST_SAMPLES`). Its first 1.8 s
  (`ASKED_AT`: the request's 0.3 s pre-roll, then 1.5 s) are asked for as
  soon as they are heard, while the person is still talking or in the
  quiet that ends the line; a shorter line is asked for at its end. Asked
  for at 3 s, scene 3's two short lines were printed too late on
  lamp-4ace (below).
- **The conversation's voice** is the last voice the lamp answered that was
  long enough to know. A print that comes after its answer was let out
  still becomes it. A line answered as the conversation going on that is
  too short to know keeps it. One too short to know that began a
  conversation of its own (answered for its name, a face or anything but
  the conversation) leaves no voice known.
- **The rule.** A cosine under 0.25 (`DIFFERENT_BELOW`) between the line's
  print and the conversation's is another voice: no conversation is going
  on for that line, and only the name, a face, or close and turned to the
  lamp, let its answer out. The same voice, or no verdict, is as without
  the model. Each verdict is a journal line with its cosine: "another voice
  than the conversation's (cosine 0.12)", "the conversation's voice
  (cosine 0.91)".

**Its cost**, in the "would a person do this?" sense: B's direct follow-up
to the lamp's answer to A ("And what about tomorrow?", when 1.5 s or
longer) now needs B facing the lamp, or the name. A person at the desk
would usually answer it. That is why it is behind its own model path.

**The bar** is the measurement above, synthetic voices only: with both
clips 1.5 s or longer and the lamp quiet, 0.25 took none of 37 same-voice
pairs for two voices and caught 19 of 29 pairs of two. Those were whole
clips, up to 6.2 s, and the voice prints only a line's first 1.8 s.
Cut as the worker prints them, with the pre-roll and, for a short line,
the end silence (18 October,
[verification](verification/same-voice-early-20261018/README.md)): at
3 s, 0.25 took none of the 37 for two voices (the lowest 0.28) and caught
22 of the 29; at 1.8 s, none (the lowest 0.32) and 22; at 1.5 s, 6 and
26. A minute of two real people at the lamp is to set it.

**What else the voice does with it** (the reviewer on #1030):
- **Once a line.** A line's voice is judged, and said in the journal, once,
  while its answer's verdict is still to come, not on every chunk of the
  answer.
- **Another voice is talk.** A line held because it is another voice
  counts as talk between people, for a busy room and for the two-lines
  rule. In the 06:07 scene with the model answering Samantha, her
  "Anyway, …" then counts, and Daniel's "Did you get my message about
  the budget?" after his own unanswered line is held.
- **One such line ends the conversation** (`RoomTold::another_voice_held`,
  17 October). It counts as the two lines left alone do, at once: two
  people are talking, so whoever speaks next needs the name or a face,
  until the lamp speaks again. Without it, the check only moved the
  error in the first builder's battery (scene 3: Daniel asks the weather
  by name, Samantha says "And what about tomorrow?", then "I should bring
  an umbrella then."). With the umbrella line held, Daniel's own "Yes, and
  we should leave a bit early.", to her, came with the lamp quiet, in the
  conversation's voice, inside 45 s, and would have been answered. A
  sound let through (`End { sound: true }`) is no one's voice: held, it
  ends nothing (the reviewer on #1415). **Its cost:** with one person
  talking to the lamp, someone else's line near it, held as another voice,
  ends that person's conversation too. Its sharpest form: the lamp asks
  "Shall I start it?", someone else's line comes between, and the asker's
  own "Yes, please start it." is held, since a reply to the lamp's
  question is only let through while a conversation is going on. And it
  needn't be a second person (the Seventh on #1415): a TV's line of 1.5 s
  or more, or the asker's own line shouted or whispered, can fall under
  0.25 too, which was set on synthetic voices at one level. Before, one
  false "another voice" held one line; now it holds every unnamed line
  after it until the lamp speaks. **So before `LAMP_SAME_VOICE_MODEL`
  goes on anywhere,** #829's battery on lamp-4ace gets a TV line and a
  raised-voice line between a call and its follow-up. If those end the
  conversation, the fallback is to end it only when the other voice was
  near the lamp (`set_near`) or a second face is in view.
  - **Off the lamp, on the battery's lines** (made with `say` in its two
    voices, 16 kHz, the real model, `lamp-same-voice pairs`): against
    Daniel's "Hey Lamp, … Hanoi", Samantha's umbrella line (1.58 s) has a
    cosine of 0.13, her "Okay, I will book the car for eight." (2.14 s)
    0.16 and her "Yes, I read it this morning…" (3.07 s) 0.12. Daniel's
    own lines are 0.75 to 0.86, and her "And what about tomorrow?"
    (1.19 s) 0.16. These are clean clips, not through the lamp's speaker
    and microphone.
  - **On the lamp the short follow-up is judged too.** The 1.5 s bar
    counts the request's audio as the worker gets it: the coordinator's
    0.3 s pre-roll first, then the line, then the end silence (0.6 s in
    the product's and lamp-4ace's units; the reviewer on #1415). So a
    1.19 s line arrives as about 1.8 to 2.1 s, and is judged. At 0.16 it
    is held as another voice, which is the scene's own expectation for B's
    follow-up without a face or the name, and with this rule the
    conversation ends there. A's own short follow-up, in A's voice, stays
    answered unless the model takes it for another: 7.7% of same-voice
    pairs under 1.5 s on lamp-4ace's October recordings. Judging the bar
    on a line's own speech instead would keep a 1.19 s line unjudged, and
    would put a 1.58 s one such as the umbrella line just under it.
  - Test: `with_the_switch_and_a_voice_check_another_voice_ends_the_conversation`
    (scene 3 through the worker's own loop at the lengths the lamp sends,
    with and without the check, and the umbrella line alone between the
    call and A's remark, which only this rule holds) and `room_told`'s
    `with_the_switch_one_line_held_as_another_voice_ends_the_conversation`
    (with a sound let through, which ends nothing).
- **A print not back at its answer's verdict** (lamp-4ace, 18 October, at
  `6ea956900` with the check on; the first builder on #829, 06:54). A line
  is asked for as soon as 3 s of it is heard, or else at its End, the
  moment its input ends at Gemini. A shorter line's print then races
  Gemini's first output, and on the lamp it lost: scene 3's "And what
  about tomorrow?" (about 2.1 s as sent) and the umbrella line (about
  2.5 s) were judged without the check and played, with no word in the
  journal. B's late print then became the conversation's voice, since a
  print that comes after its answer still does. The journal now says,
  for a line long enough to know that is judged without the check, which
  print was missing ("judged without the voice check: its voice print was
  not back at its answer", "… the conversation's voice print was not back
  at its answer", or "… no voice of the conversation is known", when
  waiting for the line's own would change nothing). It says when the child
  was too busy to take a line ("no voice print: the same-voice child was
  busy (loading its model or printing another line)"), as it is for the
  15.5 s it loads after each start. And it says how long each print took
  from its ask ("its voice print came N ms after it was asked (S s of
  sound)"), so a run with the check on times the child on the lamp.
  **The fix:** a line is asked for once 1.8 s of it is heard (`ASKED_AT`),
  not at 3 s. Every line of scene 3 is 1.8 s or more as sent, so each is
  asked for while it is still heard: B's follow-up 0.29 s before its End,
  the umbrella line 0.68 s before, and their prints get that, plus
  Gemini's own time to its first output, to come back. On lamp-4ace's
  October recordings the first 1.8 s told the voices apart no worse than
  the first 3 s (above). A line of 1.5 to 1.8 s as sent (0.6 to 0.9 s of
  speech) is still asked for at its End, and on the lamp is likely judged
  without the check. If the child on the A733 takes longer than a line's
  head start and Gemini's time together, the journal says so and the
  next step is to wait for the print at the verdict up to a bound. Tests:
  `with_a_voice_check_whose_prints_take_the_lamps_time_the_scene_is_judged_with_it`
  (the scene at the lamp's lengths with a stand-in child that answers a
  line asked for while it is heard at once and one asked at its end after
  its verdict, as on the lamp: with the fix A's call is answered and the
  four lines after it are held; asked for at 3 s, it gives the lamp's
  `[true, true, true, false, false]`),
  `a_line_is_judged_from_its_first_1_8_s_when_it_is_long_enough_and_said_with_the_lamp_quiet`,
  `a_print_is_coming_from_its_first_sample_until_it_comes` and
  `a_line_judged_without_the_check_says_which_print_was_missing`. The
  worker's journal line itself is not checked by a test.
- **The child is supervised, with bounds.** One that gives no answer to a
  line within 2 s (`ANSWER_WITHIN`) is killed and started again, with a
  journal line. One that ends 3 times in a row before it is ready
  (`MOST_UNREADY`: a missing program or model, a broken file) is not
  started again, and the journal says once that the check is off.
- **Updates don't install it yet.** `lamp-update` puts `lamp-turn-end`
  beside `lamp-voice` (`crates/update/src/install.rs`), and not
  `lamp-same-voice`. Until it does, after each update the binary is copied
  beside `lamp-voice` by hand, or the check is off after its three
  starts. That's for the update's owner.

Tests: `same_voice::tests` (which lines are judged and when, the
conversation's voice, a line judged once, a late print, a refused one and a
busy child, the flag, the child's answers, a child that never gets ready
and one that hangs after it is ready, with stand-in programs),
`lamp_same_voice::tests::serving_answers_each_request_with_its_print_or_why_not_until_its_input_ends`,
and through the worker's own loop
`with_the_switch_and_a_voice_check_another_voice_is_no_part_of_the_conversation`
and `with_the_switch_and_a_voice_check_a_line_held_as_another_voice_counts_as_talk`.
The child with the real model, its timing and memory on the A733, and
real voices are not shown.

## Not shown

- **People.** Every voice was synthetic and played from one loudspeaker.
  Real voices at 0.5, 1 and 2 m, in Vietnamese and English, are needed
  before any threshold is chosen (asked for on #829).
- **The lamp.** No timing or memory on the Orange Pi.
- **A reference against PyTorch.** The filter banks were checked against
  their definition (a tone lights its band; the mean is off), not against
  torchaudio's numbers. The same voice scoring 0.95 says they are close.
- **Many voices.** Three voices, 47 clips.
