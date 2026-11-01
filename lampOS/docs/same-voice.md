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
- `cargo run --release -p lamp-voice-eval --example same_voice_pairs --
  MODEL.onnx INDEX.tsv [--end-silence-ms 600] [--pairs]` reads the bars on
  labelled clips (`file<TAB>voice`) as the voice prints them. Each clip is
  cut into lines by the voice's `TurnDetector` (its silence window alone),
  and each line is printed from its start, pre-roll included, at most
  `ASKED_AT`; lines under `LEAST_SAMPLES` are counted, not printed. It
  prints the spread of three groups: pieces of one clip (the join's
  question), the same voice in two clips, and another voice. It also gives
  how many fall on each side of `DIFFERENT_BELOW` and `SAME_FROM`, and the
  bar with the fewest pairs on the wrong side. Numbers only. In place of
  the index it takes a microphone's recording (`--bus`, `lamp-bus record
  DIR mic2/audio`'s `mic2_audio.jsonl`), cut by when each line was played
  (`--plays`, `start_unix_ms<TAB>end_unix_ms<TAB>voice` or the night
  kit's round `.json`, whose `rows` give `start`, `end` in Unix seconds
  and `voice`; `--offset-ms` for the two clocks). Each clip runs from 300 ms before its play to 700 ms
  after, never past the plays on either side. Keep the 300 ms lead:
  `afplay`'s start time is taken before its sound begins. It also counts
  the plays with no line heard, the blocks lost inside the clips, the
  lines too short to print, and the plays with no sound recorded at all,
  which are left out (only none recorded for every play stops the run). These are diagnostics, not proof the times
  line up: the first and last tend to rise when the times miss the sound,
  but a recording with no loss counts 0 lost at any offset. `at_unix_ms`
  is when a block was published, not when it was heard, so one constant
  `--offset-ms` can't take out a delay that varies.
  `tools/mic2-voice-table.sh` does the whole run from the computer that
  plays a night-kit round: `lamp-bus record` of `mic2/audio` on the lamp
  over ssh, the round, the clocks' offset from `date` before and after,
  then this table at 300 and 600 ms end silence. The recording is removed
  from the lamp and from that computer at the end.

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
and loading took 1.8 s. On lamp-4ace on 18 October, in the voice's own
child, a line's first 1.8 s came back a median of 235 ms after
it was asked, 206–535 ms over 52 prints (the same-voice A/B's check-on arm,
below).

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
- **At ordinary priority** (#1445). The voice runs at real-time
  round-robin priority 20 (`CPUSchedulingPolicy=rr` in lamp-4ace's unit and
  the product's), and a process it starts takes that policy. Before it
  loads its model, the child takes the ordinary policy at nice 10
  (`lamp_same_voice::ordinary_priority`, `NICE`). One that can't, ends, and
  after three such starts the check is off. **Why:** on lamp-4ace on
  18 October at 08:34, with the model set, the provider worker missed its
  controller's heartbeat (`provider controller heartbeat expired`) before
  the child was ready, and the restart guard muted the lamp. The child's
  load is 15.5 s of one of its eight cores there. At the voice's priority
  it shares the voice's own threads' time and starves everything at
  ordinary priority. **Shown on lamp-4ace at 09:31** with this fix
  (`e7351fcb1`) and the model set: no heartbeat expiry, `ps` showing the
  child `TS` at nice 10 beside the voice's `RR` 20 threads, and the model
  loaded in 5.3 s rather than 15.5 s (the first builder on #1445, 09:36).
  The same steps at 06:36, without the fix, had started cleanly by
  chance. Tests, Linux only:
  `the_child_takes_the_ordinary_policy_a_little_below_ordinary_priority`,
  and `tests/priority.rs`'s
  `the_spawned_child_does_not_keep_the_voices_real_time_class`: the real
  `lamp-same-voice serve`, started from a thread at round-robin priority
  20 where the machine allows a test that, is read while it waits to open
  a model that is a FIFO nobody writes. Where a test may not take a
  real-time class, as on GitHub's runners, it shows only the child
  lowering its own niceness before the model is read. And, through the
  worker's own loop,
  `a_child_still_loading_its_model_takes_nothing_from_the_worker` (a real
  child that never gets ready: every line is taken and answered as without
  the check; with the ask made to wait for the child, the service sees
  nothing more from the worker).
- **Only what was said with the lamp quiet.** The coordinator marks a
  `Start` that was admitted while the lamp's own reply could be heard: one
  that had sound or was playing (`over_the_lamps_voice`). Such a line, and a
  turn the lamp began itself, is never judged: with the lamp over it, the
  model hears the lamp (47.1% equal error, above).
  - **A line begun while a silent reply was in hand is judged** (#829,
    18 October). This was measured in lamp-4ace's same-voice A/B: six
    two-person scenes, three rounds with the check on
    and three without, played `say` voices (Daniel, Samantha) on mic2. The
    check-on arm ran 20:03–20:39, and its journal and session events were
    paired per turn.
    - Each scene's call came while the line before it, left unanswered,
      still had its turn open. So it was taken for one said over the lamp
      (`over_the_lamp`, a reply in hand), though the lamp had said nothing.
    - In the check-on arm, 64 lines were begun so, none was printed, and 63
      had no sound from the lamp; 55 began with nothing in hand, and 52 were
      printed.
    - With no call printed, the conversation took the next speaker's voice:
      Samantha's "And what about tomorrow?" after Daniel's call, then her
      umbrella line as the conversation's own.
    - When it judged, it was right 8 times of 8: six of Samantha's lines
      against her own print (0.48–0.60), and two of Daniel's against hers
      (0.14, 0.18).
    - `over_the_lamp` itself still marks a reply in hand, for the staff
      note (`crate::staff_note`).
  - **What a call printed this way costs:** its print is asked for at
    1.8 s and comes back a median of 235 ms later in the child, as any
    line's does (206–535 ms over the check-on arm's 52 prints).
    The call's reply never waits for it: a print not back at the verdict is
    kept for the conversation when it comes (`Voices::decided`).
- **Only a line of 1.5 s or more** (`LEAST_SAMPLES`). Its first 1.8 s
  (`ASKED_AT`: the request's 0.3 s pre-roll, then 1.5 s) are asked for as
  soon as they are heard, while the person is still talking or in the
  quiet that ends the line; a shorter line is asked for at its end. Asked
  for at 3 s, scene 3's two short lines were printed too late on
  lamp-4ace (below).
- **The conversation's voice** is the prints of the lines the lamp
  answered in it that were long enough to know, the last four (`KEPT`). A
  line answered as the conversation going on adds its print. One that
  began a conversation of its own (answered for its name, a face or
  anything but the conversation) starts them afresh with its own. A print
  that comes after its answer was let out still counts, unless the line
  was let out as the conversation going on without being judged and its
  print is another voice's: then the conversation's prints stay as they
  were and the conversation is over, as for a line held (below). A line
  answered as the conversation going on that is too short to know keeps
  them. One too short to know that began a conversation of its own leaves
  no voice known.
- **The rule.** A line's **highest** cosine against the conversation's
  prints, under 0.30 (`DIFFERENT_BELOW`), is another voice: no
  conversation is going on for that line, and only the name, a face, or close and turned to the
  lamp, let its answer out. The same voice, or no verdict, is as without
  the model. Each verdict is a journal line with its cosine: "another voice
  than the conversation's (cosine 0.12)", "the conversation's voice
  (cosine 0.91)".
- **Why the highest of several, not the last one alone** (the first
  builder's decision on #1445, 10:3x, after the Sixth's finding there).
  With only the last answered line kept, one mixed print, such as one
  with the lamp's tail in it, would become the only reference and set the
  bar for every line after it. With the highest of the last four, one
  mixed print can't. This was decided on lamp-4ace's 09:31 run of
  18 October, read at first as two of one speaker's lines (requests 49 and
  50, 0.14 and 0.24) held against a middling print. The first builder
  corrected that at 11:0x: 49 was Daniel and 50 Samantha, so both
  verdicts were right, and that run shows no harm from the last print
  alone. The fix stands as correct by itself. **Its cost:** another voice now passes if it is near
  any of the four, not only the last. So a kept print mixed towards the
  other person's voice would let that person's next line through as the
  conversation going on. Tests:
  `a_mixed_print_among_the_conversations_does_not_make_its_own_voice_another`
  (A's mixed print kept, then A's next line, far from it but near A's
  first, is the conversation's voice; B is still another; judged against
  the newest print alone it fails) and
  `the_conversation_keeps_its_last_prints_and_a_new_one_begins_afresh`.

**Its cost**, in the "would a person do this?" sense: B's direct follow-up
to the lamp's answer to A ("And what about tomorrow?", when 1.5 s or
longer) now needs B facing the lamp, or the name. A person at the desk
would usually answer it. That is why it is behind its own model path.

**The bar is 0.30**, one bar for every rule that reads a voice (the first
builder at 03:50 on Queue #1722): the conversation's own check above and
the same-one rules, `LAMP_GOING_ON_NOT_OWN` and
`LAMP_QUIET_GOING_ON_SAME_ONE`, which take a print under it against the
caller's as someone else's (no print, a line begun over the lamp's voice
or under 1.5 s, never is). Two bars for one question would let the older
check hold what the newer rules let through. **The evidence:** on
lamp-4ace's mic2 on 21 October at 02:29 the same voice with both clips
quiet scored down to 0.35 and other voices up to 0.29; 0.45 would have
held 3 of 20 of those same-voice pairs, 0.30 none
([verification](verification/same-voice-20261021/), #2354). On the sets
with pairs kept, 0.30 read none of 37 same-voice pairs of printed lines as
another and caught 27 of 29 others. On 18 October's labelled set (below)
1 of 950 same-voice pairs is under 0.30, the lowest at 0.27, so a rare own
follow-up is still held. **The cost** is the trade: more other voices
pass as the same and go on, so more talk between people is answered, the
case #829 was made for. That set's count of pairs of two at or over 0.30 is
not recountable (only its summary is kept): between about 220 (at 0.455,
its 7.5% equal error) and 1,269 (at 0.25) of 2,966. The 0.29 top for other
voices on 21 October is two synthetic voices in one room. Without `LAMP_SAME_VOICE_MODEL`, which
no lamp sets today, nothing changes.

**The bar was 0.45 before**, the equal-error bar of lamp-4ace's labelled set
(18 October, scored on lamp-4ace by the first builder with the Sixth's
script; [verification](verification/same-voice-20261018/README.md), #1485).
That set is 89 clips of Daniel, Samantha and Karen played on mic2 with the
lamp quiet, cut as the worker prints them: 950 same-voice pairs (median
0.62, lowest 0.27) and 2,966 of two (median 0.23, highest 0.69). Its equal
error is **7.5% at 0.455**, wrong about 1 in 13 times each way. Its lines
are all 1.5 to 3 s of speech (the Sixth's scene list on #1445), so none is
under 1.5 s, though the worker judges a line from 1.5 s as sent, about
0.6 s of speech with its pre-roll and end silence (below). The bar of
0.25 before it let 1,269 of the 2,966 pairs of two (43%) pass as one
voice. **The older recordings with the lamp quiet disagree** (below): their
same-voice median is 0.47 (lowest 0.10), so about half their same-voice
pairs fall under 0.45, and their equal error is 14.3% at 0.295. On the 19
of them cut again as the worker prints them, 0.45 takes 39 of the 75
same-voice pairs for two voices, 31 of them among the 38 with a line under
1.5 s, and 1 of the 96 pairs of two for one voice
(`verification/same-voice-early-20261018`). The labelled set is the
cleaner measurement, but only of lines 1.5 s or longer. At either bar
**the check stays off**: it is at chance with the lamp's voice in a
print, and if it's used at all it's one weak sign beside facing, lips
(#720) and the name (the first builder, #1445).

How the bar of 0.25 had come about, and why it was wrong: it was set on
synthetic voices off the lamp. With both
clips 1.5 s or longer and the lamp quiet, 0.25 took none of 37 same-voice
pairs for two voices and caught 19 of 29 pairs of two. Those were whole
clips, up to 6.2 s, and the voice prints only a line's first 1.8 s.
Cut as the worker prints them, with the pre-roll and, for a short line,
the end silence (18 October,
[verification](verification/same-voice-early-20261018/README.md)): at
3 s, 0.25 took none of the 37 for two voices (the lowest 0.28) and caught
22 of the 29; at 1.8 s, none (the lowest 0.32) and 22; at 1.5 s, 6 and
26. **On the lamp's own recordings it doesn't hold** (the Sixth's
script, run by the first builder on lamp-4ace on 18 October over every
recorded set of 13–15 October: 137 clips of played `say` voices on mic2,
the real CAM++, on #1445 at 11:0x). With the lamp quiet in both, the
equal error was 14.3% (at 0.295). At 0.25, 85 of 793 same-voice pairs
were read as two voices (11%) and 896 of 3,212 pairs of two as one
(28%). With the lamp sounding over one or both it was 47.9%, chance. No
bar separates them, so **the check stays off and is not to hold an
answer** (the first builder's decision). The labelled set (above) is the
cleaner measurement and set the bar at 0.45 then. (An earlier note here, that one voice's own lines
came out at 0.14 and 0.24 in the 09:31 run, was wrong: they were two
speakers, both judged right.)

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
  the bar too. At 0.30, 1 of 950 of one voice's own pairs does on the
  labelled set, whose lines are 1.5 s or longer, with the lamp quiet (at
  the earlier 0.45, about 1 in 13). With a line under 1.5 s, at 0.45 it
  was most of them: 31 of 38 on the older recordings (below). Before, one
  false "another voice" held one line; now it holds every unnamed line
  after it until the lamp speaks. **So before `LAMP_SAME_VOICE_MODEL`
  goes on anywhere,** #829's battery on lamp-4ace gets a TV line and a
  raised-voice line between a call and its follow-up. If those end the
  conversation, the fallback is to end it only when the other voice was
  near the lamp (`set_near`) or a second face is in view.
  - **A question in the conversation's own voice goes on** (#829, 19
    October). The cost above was measured on lamp-4ace in queue 22e's
    check-on arm (01:19–01:56).
    - In scene 5, Samantha's "Pepper", held as another voice (0.09–0.13),
      ended Daniel's conversation. His "Lamp, what about a name for a black
      kitten then?" (its bare "Lamp," never heard as the name) and "And
      which of those names is more popular?" were then held in all three
      rounds.
    - In scene 4, a television's line (0.34, 0.41) ended his, and his "And
      how long would that take by train?" was held.
    - Now, when lines held as another voice are all that ended the
      conversation since the lamp last spoke (`RoomTold::ended_only_by_another_voice`),
      a **question** in the conversation's own voice (likeness at or over
      the bar) goes on with it. A question here (`remarks::questions`, the
      first builder's ruling on #1636) is a `?`, a sentence that begins
      with a question word or a helping verb (after one "and", "so", "but"
      or "then" at most), or the lamp's name. The journal
      says `the conversation's voice (cosine …), a question after another
      voice's line: the conversation goes on`. Not `remarks::asks`, which
      takes any "you", "please", "thanks" or a doing word, all common in
      talk to another person (the reviewer on #1636).
    - **Still held:**
      - a statement in that voice: the caller's talk to the other person,
        scene 1's "Yes, and we should leave a bit early.", "Yes, and you
        should bring yours too.", "Okay, let's leave a bit early then.",
        "Thanks, I'll take the car.". It is talk past the lamp, so it ends
        the conversation for every voice (`RoomTold::own_voice_talked`;
        the journal says `… no question after another voice's line: talk
        past the lamp`), whether the model would have answered it or not;
      - any line after that or after talk left unanswered: #1030's 06:07
        scene, where Daniel's "But the client wants to meet in person."
        ends it before his "budget" question;
      - another voice's lines, compared with the conversation's prints:
        her own question right after "Pepper" ("Do you like that name?")
        is held;
      - a question whose print is not back at its verdict, which says
        `judged without the voice check: …`. It is no talk past the lamp,
        so the question after it, printed in time, still goes on.
    - **A statement whose print is late** (a short line, printed at its
      end, as lamp-4ace prints one of 1.5–1.8 s) is taken for talk past the
      lamp as well, so the question after it is held (the reviewer on
      #1636, 03:41). The journal says `no question after another voice's
      line, its voice not known in time: taken for talk past the lamp`. A
      line too short to print is judged as it was before the rule.
    - **Its costs** (the first builder's yes, 19 October, 02:10 and 03:07):
      - the caller asking the other person a question right after her line
        ("Do you really like Pepper?") is answered;
      - so is a statement that begins with a question word or a helping
        verb, or names the lamp: "When we get there, I'll call you.", "What
        I mean is we should leave a bit early.", "How about we leave a bit
        early.", "Should be fine by then, I think." (the reviewer on #1636);
      - a request to the lamp that isn't phrased as a question, in his voice
        right after her line ("Tell me another name.", or a "Lamp, …" whose
        bare name was missed, as in scene 5), is held, as before the rule,
        and now also ends the conversation.
    - **On the lamp** (the first builder's A/B, 19 October, 08:3x on #829):
      lamp-4ace, 07:20:43–08:35:13, the office empty, against queue 22's
      install (`cfce52401`). The build was `eda24bc9b`, this rule without the
      late-print arm above, scored by `score_ab_fourth.py`. Answered with the
      check / without:
      - the umbrella line, the one this check is for: 0/3 / 2/3. A's remark
        and the car: 0 times with the check;
      - the wanted lines: 15/15 / 14/15, scene 6 as expected both ways;
      - scene 1's "And what about tomorrow?", B's follow-up: 0/3 / 2/3, the
        expected cost of a second speaker's follow-up;
      - scene 2's "And in yen?": 2/3 / 3/3;
      - scene 4's "And how long would that take by train?": 3/3 / 2/3;
      - the busy openings: 0 both ways, as expected after 60 s of quiet.

      The voice print came 217–374 ms after it was asked, on 1.80 s of
      sound (the journal's lines, the check on).
    - **Tests:**
      `spoken_to::with_a_voice_check_a_question_in_the_conversations_voice_goes_on_after_another_voices_line`
      (scene 5 with her question and her line after it; scene 4; four
      statements held; #1030's scene with the model answering Daniel's
      statement), with #1030's `…_a_line_held_as_another_voice_counts_as_talk`
      unchanged;
      `spoken_to::with_a_voice_check_a_statement_whose_print_is_late_after_another_voices_line_ends_the_conversation`
      (#1030's scene with Daniel's statement printed after its verdict);
      `spoken_to::with_a_voice_check_a_question_whose_print_is_late_after_another_voices_line_leaves_the_conversation_going_on`
      (the same scene with a short question in its place);
      `remarks::question_tests::a_question_is_a_question_mark_or_a_question_word_first`;
      `room_told::tests::a_conversation_is_over_only_by_another_voice_until_talk_or_the_lamp`.
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
    conversation ends there. A's own short follow-up, in A's voice, is
    answered only if the model takes it for A's, and at the earlier bar of
    0.45 it mostly didn't. On lamp-4ace's October recordings, cut as the worker prints
    them (`verification/same-voice-early-20261018`), **0.45 takes 31 of
    the 38 same-voice pairs with a line under 1.5 s for two voices** at
    1.8 s (33 at 3 s), and 8 of the 37 longer ones (12 at 3 s). The bar of
    0.25 before it took 8 and none at 1.8 s (7 and none at 3 s), seven of
    the eight one clip, `cut-q-03` (1.45 s), against its own voice's long
    lines. So with the check on, A's own short follow-up would be held as
    another voice most of the time, and with this rule each such hold ends
    the conversation (the reviewers on #1445 and #1508). The labelled set
    that set the bar has no line under 1.5 s, so it says nothing of these.
    The clean cut of 15 October gave 7.7%. Judging the bar
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
  the first 3 s at the bar's own pairs and over all pairs together
  (above), but not for short lines: with a line under 1.5 s, an equal
  error of 21.7% against 18.2%, 8 same-voice pairs under 0.25 against 7,
  and 31 under 0.45 against 33. A line of 1.5 to 1.8 s as sent (0.6 to
  0.9 s of speech) is still asked for at its End, and on the lamp is
  likely judged without the check. If the child on the A733 takes
  longer than a line's head start and Gemini's time together, the journal
  says so and the next step is to wait for the print at the verdict up to
  a bound. Tests:
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
- **A late print of another voice ends the conversation** (the Seventh on
  #1445). On a lamp where a print still comes after its line's verdict,
  the line is let out as the conversation going on, and its print used to
  become the conversation's voice unchecked: B's, after B's follow-up, so
  scene 3 went on with B and A's remark was judged against B's voice. Now
  the late print of a line let out as the conversation going on is
  compared with the conversation's voice first. Another voice's (under
  0.30) is not taken; the journal says "another voice than the
  conversation's, known after its answer (cosine 0.12): the conversation
  is over", and at the next line's verdict the conversation ends, as for a
  line held as another voice (`RoomTold::another_voice_held`), that line
  included. It's taken then, and not at once, because the lamp's answer to
  the late line begins the count afresh (`RoomTold::spoke`). A line that
  began a conversation of its own, for its name or a face, still makes its
  late print the conversation's voice unchecked. Tests:
  `with_a_voice_check_whose_every_print_comes_late_a_late_other_voice_ends_the_conversation`
  (scene 3 with every print after its verdict: A's call and B's follow-up
  are answered and the other three held; with the late print taken
  unchecked, every line is played) and
  `a_late_print_is_compared_before_it_becomes_the_conversations_voice`.
- **A call begins a conversation of its own, even amid another's** (Eighth
  on #829). A line that called the lamp for itself, by its name heard, its
  head tapped or its words, starts the conversation's prints afresh once
  it's let out, even while another person's conversation is going on and
  before its own print has come. Its voice is never judged late against
  that person's, and starting afresh drops any late "another voice" of the
  conversation before. Before this, such a call was taken as the old
  conversation going on: its late print was another voice than the old
  speaker's, so it ended the caller's own conversation at the caller's next
  line. That was not what happened in the 09:34 scene 3 on 18 October: its
  journal has no "known after its answer" line, and its "And what about
  tomorrow?" (request 18) was judged another voice directly (the first
  builder on #829 and #1445). A line whose conversation ends this way says so at its
  verdict: "no conversation is going on for it: request N's voice, known
  after its answer, was another's". Tests:
  `with_a_voice_check_a_named_call_amid_anothers_conversation_begins_its_own`
  (B calls and follows up, A calls by name while B's conversation is going
  on and follows up, every print late: all four answered; with the call
  taken as B's conversation going on, A's follow-up is held),
  `a_new_conversation_ends_any_late_voice_of_the_one_before` and
  `a_call_takes_no_late_voice_from_before_it_at_its_verdict` (`Voices::at_verdict`:
  no late voice is taken at a call's verdict. The worker's passing of
  `called_for_itself` there isn't tested through its loop: the strict busy
  form and a name heard in the sound are process-wide settings no rig sets).
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

The journal and hung-child restart fixtures retain the worker from their actual
`Prints` constructor. After the original assertions, they close its channels,
wait for its completion event with a five-second diagnostic bound, then join
the worker. The restart case still uses `Prints::timed` and checks the same
answer timeout and second start. Unwinding also retains the join; a stuck OS
spawn or reap requires the enclosing test-process watchdog. This fixture
cleanup does not change production supervision or establish the cause of a
historical native leak.

## The caller's voice only (#2602, 21 October)

On lamp-4ace on 21 October at 20:06 (sv-ab, after a quiet-gated restart),
the check held none of the two-person scenes' talk: scene replies were 2 5 5
with the model, against 2 3 4 without it. Every scene line printed read as
the conversation's voice, at 0.55 to 0.84, while two voices were speaking;
at 18:33, with the conversation still the caller's alone, the other person
read 0.11 to 0.29. The conversation's voice had taken in the second person:
`Voices::adopt` kept the print of every line let out as the conversation
going on, and once one of theirs was let out before its print was back,
their voice was the conversation's too.

With `LAMP_SAME_VOICE_CALLER_ONLY=on` on the second route
(`Voices::callers_only`), the conversation's voice is its caller's only:
the prints of lines let out for themselves (the name, a call, a face turned
to it). A line let out only as the conversation going on is judged against
it and never added to it, so the second person's lines stay another voice
(a line let out for a face turned to the lamp is a new call, as for the
caller's face, so its speaker's voice becomes the conversation's: the
Reviewer's P2 on #2650, `a_caller_let_in_by_a_turned_face_takes_the_conversations_voice`)
(`with_the_caller_only_a_going_on_line_never_becomes_the_conversations_voice`).
A caller whose call was too short to print has no voice known for that
whole conversation, unlike without the switch, where the next going-on
line's print becomes its voice: the check then stays silent until the next
call, on the side of holding nothing, unless the eyes tie a follow-up to
the caller: a going-on line seen spoken by the call's one face and no other
(`seen_from_the_caller`, the worker's `spoken_by_the_caller_alone`) gives
a conversation with no voice its voice, and nothing else ever does (the
first builder on #2602, 21 Oct 21:19;
`a_short_call_takes_its_voice_from_the_callers_first_follow_up_seen_spoken`).
A call seen spoken by two faces names no caller, so nothing is taken. The
one line marked is the newest seen so: a later one seen before an earlier
one's print is back takes its place, and that earlier print is never taken,
on the side of holding nothing. The caller's face can also come from the
first going-on line's face turned to the lamp when none was seen speaking
the call (#2341), which may be someone who never called; and a moving mouth
is not the sound's source, so lips moving under a TV voice pass too.
Not yet run on a lamp.

## The asker's voice (#4306, 30 October)

The conversation's voice is for the rule, and a line let out as the
conversation going on can add to it (above). For the Policy the voice
keeps one more print: the asker's, the print of the line that began the
conversation (`Voices::like_asker`). It is the print of a line let out for
itself, or, with the caller's voice only, of the first going-on line seen
spoken by the caller when the call was too short to print. A line let out
only as the conversation going on never becomes it, so a second person
whose line was taken for the conversation's is never the asker. A
conversation of its own begins it afresh: a new call's print is the asker
from then on, and a call too short to print leaves none.

For each request, `attention/addressed` then says `like_asker`: the cosine
of the request's print against the asker's, from -1 to 1. It is read
without using up the line's one judgement, and is left out while no
conversation is going on (the voice's thread has no exchange with the
lamp's words in it from the last 45 s), while
either print is not known, and without the check. The prints stay in the
voice; only the number is told. No rule in the voice reads it. On
lamp-4ace's played rounds (#4306, 30 Oct) a follow-up and desk talk inside
a conversation looked alike to the Policy's seven questions; whose voice it
is should tell them apart, and the Policy's weight for it stays 0 until it
is fitted on a lamp's run with the sign ([policy.md](policy.md), "How
alike to the asker").

Shown in tests only (`like_asker_is_against_the_line_that_began_the_conversation_alone`,
`like_asker_is_told_only_in_a_conversation_and_high_in_the_askers_voice`):
no unit in `deploy/` sets `LAMP_SAME_VOICE_MODEL` or `LAMP_ADDRESSED`
yet, and the voice says `like_asker` only with both.

## Not shown

- **People.** Every voice was synthetic and played from one loudspeaker.
  Real voices at 0.5, 1 and 2 m, in Vietnamese and English, are needed
  before any threshold is chosen (asked for on #829).
- **The lamp.** No timing or memory on the Orange Pi.
- **A reference against PyTorch.** The filter banks were checked against
  their definition (a tone lights its band; the mean is off), not against
  torchaudio's numbers. The same voice scoring 0.95 says they are close.
- **Many voices.** Three voices, 47 clips.
