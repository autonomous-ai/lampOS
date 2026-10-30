# Sounds that are not speech, and answers that are not answers

Two things the runtime now stops by itself, because telling the model to
stop them did not work. Status on 2026-10-13: on lamp-4ace the first has
been seen to act on a played sound, without a fault; the second was built
after it was seen happening and has not happened since. Both end a turn
without a successor, a path the service already had for an answer the
provider interrupts.

## A sound

The lamp's ears are a speech detector (`crates/voice/src/activity.rs`): six
blocks of 10 ms in a row scored as speech start an utterance. A cough, a
click or a bump can do that. The utterance goes to the model like any
other, and the model hears words in it. On lamp-4ace on 2026-10-13, in the
service's own record:

- Nine times in seven minutes the lamp said "Sorry, I missed that. Can you
  say it again?" to such a sound, most of them transcribed as "¿Qué?". Its
  owner, sitting beside it and saying nothing, reported it.
- With the character file changed to forbid that answer, it answered them
  otherwise: "¿Cómo te llamas?" (17 blocks of speech) got "I'm Lamp. What's
  your name?"; "¿Qué?" (11 blocks) got "Ah, hi! I see your desk."

Fourteen such sounds that the lamp answered that day had 11 to 33 blocks
scored as speech in the whole utterance, twelve of them under 25. The four
shortest things really said to it in the same records ("How you doing?",
"Okay.", "lungs", "Ouais.") had 34 to 41.

So: when an utterance ends having had fewer than 30 blocks scored as speech
(`SCRAP_BLOCKS` in `crates/voice/src/not_speech.rs`; 25 at first), the turn ends there
with outcome `not_speech`. Its end is not sent as a finished question, what
the provider may still answer is dropped, and the `local_endpoint` record
carries `not_speech` with the count. Two kinds of short utterance are never
treated so:

- one that began within four seconds of the lamp's own last sound: "Yes."
  and "No." answer what the lamp just asked. (This was ten seconds at
  first. On the lamp each sound it answered then excused the next one,
  eight seconds later.) But not in the 4 s after a hand or the lamp's head
  stopped it (`AFTER_STOP_US`, `hand_stop` or `head_stop`), while it has
  not spoken again: its words then were cut, not answered. Then a short
  utterance is a sound, and is not sent as a request, even with
  `LAMP_SHORT_SOUNDS` and even said close to the lamp (a hand on the head
  is close by nature; `not_speech::dropped`). Once the lamp has answered
  again ("Want another?"), a short "Yes." is taken as always. On lamp-4ace on 16 October from
  16:32:16 to 16:32:34 the owner tapped the head to stop a story six
  times. Each time, within 1 to 3 s, the tap's own sound was heard
  ("¿Quién?", "un", "¿Qué?"), excused, sent to the model, and the story
  began again (the first builder, #829, 16:33). The record's `not_speech`
  then says `after_stop`, and the journal "a short sound just after the
  lamp was stopped is not taken as a request". Real words, 30 blocks or
  more, are taken as always. A real short word in those 4 s goes too, in
  every mode, `quiet-room-checked` among them: someone who stops the lamp
  by mistake and says "Go on." gets nothing, and says it again after the
  4 s or at more length. `not_speech.after_stop` in the record counts what
  that costs. A Stop pressed while the lamp was not speaking opens no 4 s
  (Second on #1107). With `LAMP_STOP_TOLD=on` (off until it is played on
  lamp-4ace), the first request after such a Stop, on the session of the
  answer it cut and within two minutes (`TELL_STOP_WITHIN_US`), says so in
  its Start (`after_stop`, and in its `input_admitted` record). The model
  is told beside it, in the lamp's "(For you to know, not to answer: …)"
  form, that the person stopped it, that what it was saying is over, and
  to say nothing about the stop (`STOPPED_NOTE`; the journal: "the model
  is told the person stopped it"), so that it doesn't begin the story
  again. A request that took it and is then thrown out as a sound, the
  tap's own thump, gives it back, and the next request tells it
  (`not_speech::StopToTell`; the first builder on #1148). The second route
  gives the same note as a system message before the person's words.
- one that took the turn from the lamp's own answer (a quick "stop" over
  it), or from something that had itself been said (the short second half
  of a sentence after a pause). At first any turn that took over from
  another was let through, and so was a sound that followed a sound: two
  of 25 and 26 blocks, a second apart, got "I'm here if you need
  anything." Taking over from a sound now excuses nothing.

What it does not catch: sounds of 30 blocks and more (two of the fourteen
had 31 and 33; the real "How you doing?" had 34, so there is little room
left between them), a sound in the
four seconds after the lamp speaks, and anything long enough to be speech
that is not (a quiet voice across the room at a median of -33 dBFS was 60
blocks). The character file carries the rest: what seems to be in a
language other than English or Vietnamese is a noise, and is not answered
([character.md](character.md)). The lamp's light still shows that it heard
something for the length of the sound.

### A short phrase thrown out as a sound, and the floor that did it

"Good night, Lamp." was thrown out as a sound in 5 of 8 played runs on
2026-10-13. Blocks under the faint floor ([character.md](character.md),
`--faint-floor-db`, -28 dBFS on lamp-4ace) are not counted as speech
whatever the detector makes of them, and that was applied to each 10 ms
block alone. A `not_speech` record now carries how the last two seconds
of blocks were judged (`last_blocks`: twentieths of the detector's score,
the level, and how many blocks the detector scored as speech and the
floor turned away). The first one recorded: the detector scored 50 blocks
of the phrase as speech; it began at about -20 dBFS and trailed to about
-30; 28 of the 50 were turned away for being 1 to 4 dB under the floor,
and the 22 left were under the 30 an utterance needs.

So a block under the floor is now still counted for a while after a run of
blocks over it (`crates/voice/src/room_floor.rs`, `Near`): six blocks in a
row, as many as begin an utterance, and then 400 ms (`LAMP_FAINT_HOLD_MS`
in the voice unit; unset or 0, each block is judged alone as before). A
voice across the room has a block at the floor now and then and no such
run.

On lamp-4ace with the hold at 400 ms, none of 7 short phrases was thrown
out (six "Good night, Lamp." and a "Thank you, Lamp."), utterances with a
middle level of -28 to -31 dBFS and a loudest tenth of -23 to -25 among
them, and four lines played at a tenth of the volume were not admitted at
all.

A stricter run was then tried, each of the six blocks 3 dB clear of the
floor, to make it harder for a colleague's voice at the floor to earn the
hold. Replaying the one recorded phrase, it made no difference (49 blocks
counted either way), and on the lamp the next four short phrases passed.
The fifth did not: its loud start was four blocks 3 dB over, one just
over, two more and one just over, eight in a row over the floor and never
six in a row at the stricter level, and it was thrown out with 9 and 12
blocks counted. The stricter run was taken out again. One recording had
been too little to choose it by.

Not shown: the thing the floor is for, colleagues' voices from across a
working office, which nothing played here stands in for. With the hold, a
voice whose loud moments sit just over the floor is let in where before
only its loud blocks were counted; how often that happens in the real
office is to be read from the `not_speech` records and the count of turns
admitted on a working day.

### A word thrown out as a sound: "Hello?"

The gate was built for sounds, and it costs a word. On 2026-10-14, with
nobody in the office, the check of eight lines said to the lamp without
its name ([character.md](character.md)) gave seven answers: the first
line, a played "Hello?", ended as `not_speech`. Played six more times,
nine seconds apart, it was thrown out six times in six. The detector
scored 16 blocks of it as speech in the check and 23, 18, 20, 19, 20 and
22 in the six; 30 are needed. Three blocks of the first were under the
faint floor and none of the others.

That is a synthetic voice at a quick rate (`say` at 185 words a minute),
and the word lasts about a fifth of a second by the detector. The same
clip was sent on as speech in each of the ten earlier runs of that check
in which it was played into quiet, all of them before the gate existed,
and the lamp answered it in nine. The service wrote it as "Hello." in
five of the ten and as "albero", "al loro", "Claro.", "alarm" and "Hola."
in the other five: for a word this short the text is no better a judge
than the count.

So the count of blocks cannot tell this word from the sounds the gate is
for: fourteen sounds had 11 to 33 blocks, and this word has 16 to 23.
Lowering the number lets the sounds back in. What might tell them apart
is how the blocks were judged, not how many there were, and until now
only an utterance that was thrown out recorded that. An utterance taken
as said with fewer than 80 blocks of speech (`SHORT_BLOCKS`) now carries
the same `last_blocks` in its `local_endpoint` record, so a working day
leaves both kinds side by side: the short things people said, which were
answered, and the sounds, which were not. They are numbers about the
sound, two seconds of scores and levels, and nothing of the sound itself.

Not shown: a person's "Hello?". The four shortest things really said to
the lamp on 2026-10-13 had 34 to 41 blocks, so a word said at a person's
pace may well pass where this clip does not. Nothing here has measured
it. The new field is not covered by a test.

**Measured on lamp-4ace on the evening of 2026-10-14** (nobody in the office;
played from the Mac's loudspeaker a metre away; `noises.sh` and
`shortwords.sh` with the played-test tools):

- *Noises at an idle lamp:* the fourteen macOS system sounds, a knock and a
  bang, three times each, 8 to 14 s apart. The lamp said nothing after any
  of the 48. The detector took six tonal ones (Glass, Hero, Submarine, Blow,
  Bottle, Funk) for a voice 16 times and the lamp began to listen; the gate
  threw out 15 of those records here as sounds, with 12 to 26 blocks of
  speech and an unbroken run of 2 to 18 blocks scored 0.9 or more.
- *Short words into quiet, not after the lamp's own:* "Hello?", "Hi.", "Hey.",
  "Yes.", "No.", "Thanks.", "Okay.", "Lamp?", "Sorry?", "What?", "Hey Lamp.",
  "Good morning." in three voices, 14 s apart: 6 of 36 answered, 4 more
  heard and not answered, 26 not heard at all. Of the 26, 13 reached the
  gate and were thrown out as sounds (12 to 29 blocks of speech, runs of 3 to
  19), and 13 never started an utterance: the detector did not take them
  for speech.

So neither the count nor the longest sure run tells these words from those
sounds: every threshold that keeps "Hi." keeps "Glass" too. The next thing to
try is to let a short utterance in a quiet room go to the model with a note
that it may not be words said to the lamp, and that if it is not it says
nothing (`not_for_me`), since the model hears the sound itself. The same two
played sets measure it: words answered against noises answered.

**Built that evening** (`LAMP_SHORT_SOUNDS=quiet-room`, off unless set;
`not_speech::sent_anyway`): an utterance the gate would throw out goes to the
model after all while the room is not busy. Its first try on lamp-4ace
(22:27, voice `c2ac775f5`) showed a side effect: the short words the model
left alone counted as unanswered talk, the room was taken for busy after
three, and from then on even "Good morning." was held back by the busy-room
rule ([attention.md](attention.md)). So a turn let through only to be judged
no longer counts toward a busy room, answered or not (`body_link::Attention::
not_counted`); an answer still clears the count. Measured again after that.

**What the played noises did with it** (lamp-4ace, 2026-10-14, the noise set
of 23:23:56 to 23:34:20, voice `9cdd727f5`, empty office at night): the
tonal alert sounds the detector takes for a voice were now sent on. Twelve
requests by 23:31:09, none answered, each 13 to 38 blocks of speech, ten of
them under the gate's 30, all closed after the four seconds the provider
waits. The model said nothing to any of them, as it should. Three other parts
of the lamp still took each one for talk:

- The provider's own room judgement ([room_told](../crates/voice/src/room_told.rs))
  counted each as something said and left alone. Three of them inside 37 s
  (38, 17 and 13 blocks) made the room busy at 23:24:39, and the session was
  told "the office around you is busy, with people talking to each other". It
  stayed busy until 23:55:14, through the next two played tests.
- The run of unanswered requests that renews the session after eight
  counted them too: the session was renewed at 23:52:48.
- The body went to its thinking pose for each one, rising to full height for
  four seconds.

So the coordinator now marks such a request when it ends (`ProviderInput::End`
`sound`, recorded as `let_through_as_sound` on its `local_endpoint`). Left
unanswered, the provider counts it as it counts a sound it never saw: not in
the room's run, not in the five quiet minutes, not in the renewal run, and its
journal line reads "a sound (request N) got no answer", which the played-test
quiet gate does not count. The body keeps the pose it had while the sound was
heard instead of thinking, and an answer brings it back as usual. The ring
still shows its waiting light for those four seconds.

The played sets of that night ran before the change was installed:

- *Noises* (23:23:56 to 23:34:20, the run above): 0 of 48 answered; 15 taken
  for a voice and listened to, the same six tonal sounds as before (Glass,
  Hero, Submarine, Blow, Bottle, Funk).
- *Short words* (23:38 to 23:47): **0 of 36 answered**, 14 heard. The journal
  says why: "its answer is held back: a busy room, and nothing shows it was
  called", and for "Hey Lamp." "the lamp's name was heard in a busy room and
  nobody is turned to the lamp; the model is not told". At 23:52:48 the
  session was renewed after eight unanswered requests.

So that run measured the side effect, not the switch. It is to be run again with
the change installed.

Again with the change installed (voice `dcc2e6b1e` from 00:23:54 on
2026-10-15, the same empty office at night, nobody in the camera's view):

- *Noises* (00:25:48 to 00:36:20): 0 of 48 answered; 13 taken for a voice.
- *Short words* (00:37 to 00:47): **2 of 36 answered** ("Hello?" got "Oh,
  hello! There you are. What's on your mind?"; "Lamp?", heard as "Plan.",
  got "Oh, hi! What's up?"); 21 heard, 13 not. This time no sound made the
  room busy: the model itself left the first short words alone, three of
  them inside a minute counted as talk not for the lamp, the room was busy
  from 00:37:49, and the session, told so, left the rest alone too, "Good
  morning." three times among them.

A short word from across a dark, empty office, with nobody in view, is
arguably one the lamp should leave alone; and the first played set, without
the switch, which got 6 of 36 answered, ran in the evening.
The switch stays on (it answers no noise), and short words are parked
until the V2 MVP works end to end (the owner, 2026-10-15: "wide first.
deep later"). Next time: with a person at the lamp, in view.

## Said close: loud blocks count

An utterance said close to the lamp is judged by the gate with its loud
blocks counted as speech (`not_speech::scrap`). Close means the room
microphone rose 18 dB or more over its quiet during it (`room/sound`,
sampled every 100 ms by the coordinator, `room_floor::Rise`; the same sign
as `LAMP_ATTENTION=busy-near`). Loud means a block at about -31 dBFS or
louder (`LOUD_LEVEL`, 128 on the face's scale) that the detector did not
call speech, while the lamp was not speaking. Such an utterance is a sound
only with fewer than 16 speech and loud blocks together
(`CLOSE_SCRAP_BLOCKS`), not the usual 30. Everything after the gate still
judges it: attention, the name, the Policy. Without the bus it is never
close, and the gate is as before. In the 4 s after a hand or the head
stopped the lamp, it is not counted close: the tap's own thump is said
close by nature, and with its loud blocks counted it was kept (the
reviewer on #1107). The utterance's record says so: `said_close`
in `not_speech` and in `speech_level` is its loud blocks when it was close,
`null` when it was not.

Why: on lamp-4ace on 2026-10-16 from 17:13 the owner at the desk was dropped
as a sound six times (`not_speech`, turns 5, 7 and 9 to 12). They had 16, 19,
19, 17, 26 and 14 blocks counted, and 0, 0, 4, 5, 27 and 26 loud blocks not
called speech. In two of them the detector scored 15 and 20 loud blocks under
0.5, one clipping; only 0 to 12 blocks were under the floor. Loud blocks
alone, with 30 needed, would have kept only two of the six. A count of 16
alone would have lost the 14-block one. The two together keep all six, the
lowest at exactly 16.

Not skipped outright (Ten on #1137): a knock, a cough or a mug set down by
the lamp rises as much as a voice, and is a few blocks, the kind of sound
`SCRAP_BLOCKS` was made for on 2026-10-13. One with up to about 12 speech
blocks and a few loud ones stays a sound. A longer bang close to the lamp
can pass, and is then judged by what comes after the gate.

**For whoever tunes the detector next:** it under-scores a loud voice at the
desk. The counts above are from the word-free trace the first builder kept
for 17:13 to 17:16.

## A room sound let through and answered (#800)

On lamp-52e6 on 2026-10-15 at 13:03 (#775's verification, turn 6) a room
sound of about 0.7 s was admitted (`directed_session_duplex`, which with
`--turn-taking duplex` is the basis of every admission, not a judgement),
ended by the silence fallback (`silence_fallback`, `short`: fewer than 80
blocks of speech never reach Smart Turn), transcribed as a one-word question
in Spanish, and answered 1.9 s later. Nobody had spoken to the lamp. The
seventh builder saw the same sound answered all day there.

Why it went through: with `LAMP_SHORT_SOUNDS=quiet-room` a sound (fewer
than 30 speech blocks, not over the lamp, not within 4 s of its own words) is
sent on, marked `sound`, whenever the room is not busy, and "busy" is only
the last three turns gone unanswered within 30 s (`body_link::Attention`).
Nothing after that looks at what was heard: the busy-room rule
(`attention.rs`, `first_output_busy`) plays in any room not known to be
busy, and the check on the answer's own words passes any text with a
letter in it.

**Counting it on a lamp's records:**

    lamp-voice short-turns SESSIONS

reads the session records under SESSIONS (`<session>/metadata/events-*.jsonl`)
and prints one line for every short turn (`crates/voice/src/short_turns.rs`):
one let through as a sound (`let_through_as_sound`), one thrown out as a
sound (`not_speech`), and any other with fewer than 80 speech blocks. Each
line has its speech in ms, its median and upper-tenth level, how its end was
decided, whether an answer reached the speaker and how the turn ended, and
what was heard as kinds only: how many words, whether it was whole one of
the short things people say to the lamp (`short_turns::SHORT_WORDS`: "yes",
"stop", "what", "go ahead", …), and whether it had marks English does not
(`¿`, `¡`, an accented letter). Never the words. A heard text (the
session's input transcript) is joined to the turn admitted last before it.
Then counts: let through, let through and answered, and of those how many
were heard as a short word, with other marks, or as nothing; thrown out;
short and taken as said, and answered.

**Checked (`LAMP_SHORT_SOUNDS=quiet-room-checked`, off unless set).** It
lets a sound through in a quiet room as `quiet-room` does, and passes the
provider worker `--sounds-checked`. There the rule for whom the lamp answers
(`crate::attention`, which needs `LAMP_ATTENTION` on) takes a request whose
end came marked `sound` as one to check, and plays its answer only when:
- what was heard is, whole, one of the short things said to the lamp
  (`not_speech::SHORT_WORDS`: "yes", "stop", "what", "go ahead", …);
- or the room microphone rose 18 dB or more over its quiet during it, said
  close to the lamp (this needs the ears, read with `LAMP_ATTENTION=busy-near`);
- or someone is turned to the lamp (the eyes are read for it whatever the
  room);
- or the lamp began the turn itself;
- or what was heard has a word in it, the lamp's own last line was a
  question, and a conversation is going on: the person's answer to it
  (lamp-4ace, 17 October 16:11, "Yes. Please start it." heard as "bien";
  [attention.md](attention.md#a-short-sound-let-through-in-a-quiet-room-800)).

Otherwise the answer is held, as a held answer always is (its sound and
words are not let out, a call in it is answered and not run), with the
reason "a short sound, and nothing shows it was said to the lamp", and the
first-output record says so. A sound let through is never counted against
the room. What the 4 s after the lamp's own words and speech over the lamp
excuse from being a sound is not changed, so a "yes" to a read-back (#736)
is not a sound at all. Tests: `a_short_sound_is_answered_only_when_something_shows_it_was_said_to_the_lamp`
(`attention.rs`), `a_sound_heard_as_another_language_is_held_and_a_short_word_is_answered`
(the worker's own loop against the scripted service: "¿Qué?" held, "Yes."
answered, and both answered without the switch),
`the_checked_value_lets_sounds_through_and_has_the_worker_check_them` and
`a_short_word_is_one_said_to_the_lamp_whole` (`not_speech.rs`).

**On the second voice** (when Gemini is down, [second-voice.md](second-voice.md)),
the worker takes the same flag and checks a marked sound too (the reviewer
on #818, 20:11). That route has no ears and no eyes, so only the heard words
can show the sound was said to the lamp: one not heard, whole, as a short
word ends quietly after its words, and its chat is never asked. A sound said
close or facing the lamp but not heard as a short word is held there, where
the Live route would answer it. One with a word in it, right after the
lamp's own question in a conversation going on, is judged as a line there
too: the worker reads the same thread (`worker::lamp_just_asked`). Tests:
`a_short_sound_not_heard_as_a_short_word_is_held_when_sounds_are_checked`
(`second_provider/tests.rs`: "¿Qué?" held with the chat never asked, "Yes."
answered, and "¿Qué?" answered unchecked or unmarked),
`a_checked_sound_with_a_word_right_after_the_lamps_question_is_answered`
(the same file: "bien" answered when the lamp has just asked) and
`both_workers_take_the_flag_that_checks_sounds`.

**Not shown.** No recorded sound was played: the repository holds none of
the office's. Whether the input transcript always comes before the first
of the answer (in 24 of 24 answers measured it did, `attention.md`) decides
whether a real short word is held for want of its text. How often a real
short word is said to the lamp out of the blue, not after its own words, is
not counted either; `short-turns` on a lamp's records says.

## Misheard in a conversation, asked again (#926)

On lamp-4ace on 2026-10-16 at 05:39 a person who had asked for a story cut
in with "No no no, tell me a joke instead." The service heard it as "हिंदू
के स्थान" and the lamp answered in Hindi, about places in Liechtenstein; an
earlier cut-in heard as "23" got "Four." A cut-in is never a sound, so the
rule above did not hold either.

With `LAMP_TRANSCRIPTION_LANGUAGE=en-US` (any language tag; the provider
worker's `--transcription-language`), the service's transcript of what it
hears is given that language as a hint (`inputAudioTranscription.languageCodes`),
so English said with an accent is less often written in another script (#926:
"tell me the fact about eagle" came back as Devanagari on lamp-4ace). Only the
transcript: the reply's language is not set by it, though how the model
replies under the hint is not measured yet (`transcript_probe` prints the
reply). Unset, or not a tag `lamp_gemini` takes (said once, and no hint; a
hint the session still refuses is said and dropped), the setup is as before
and the voice starts. Open before it is turned on: whether the hint keeps
accented English in Latin, and whether it lets a real mishear through. With
the hint, a cut-in misheard as another language (05:39 on 16 Oct) may come
back in Latin letters and pass `OTHER_SCRIPT` while the model, which answers
from the audio, still answers in that language; those cut-ins are replayed
too before it goes on (the reviewer on #1367).

With `LAMP_OTHER_LANGUAGE=sorry` in the voice's unit (the provider worker's
`--ask-unclear`, and only with `LAMP_SPOKEN_TO=on`'s attention rule), while a
conversation is going on (`Thread::going_on`: the lamp said something in the
last 45 s), what was heard for a turn is judged when its answer begins
(`attention::first_output_busy`, `not_speech::unclear_in_conversation`):

- mostly letters of another script than Latin: misheard (`OTHER_SCRIPT`);
- one number alone, when the lamp's last line was not a question: a
  fragment (`ONLY_DIGITS`); "23" answering "how old are you?" is an answer,
  and several ("1 2 3") are someone counting, held only as a cut-in's
  fragment;
- for a cut-in over the lamp's own answer (the coordinator marks its end
  `ProviderInput::End { cut_in: true }` when the turn took over, also when it
  had already ended at admission), no common English word at all
  (`not_speech::ENGLISH_WORDS`, a small list, not a language detector):
  misheard, its start lost (`CUT_IN_FRAGMENT`). On lamp-4ace English cut-ins
  over a story came back as "vestido", "miel miel", "mueve mueve mueve",
  "Venga, nos vemos luego." (answered "See you later!"); "Stop.", "Wait.",
  "Okay.", "No." and "Wait, wait, wait." pass.

The worker says `Unclear` once, when the verdict is given. The trace says
why an asking was dropped (`ask_again_dropped`: another turn began, or 5 s
went by) and what held a due one back (`ask_again_waiting`, `held_by`, once
each time that changes). The service goes on generating into a held turn (lamp-4ace, 08:00:
`provider_idle` false for 10 s), so once the person has finished the
coordinator cancels it, as a stop does, and the asking follows. While the
person goes on, the trace says once what keeps it open
(`ask_again_turn_open`), and the asking is let go after 10 s. A hand's Stop,
the mute or a capture gap drops a pending asking with its turn, and an
`Unclear` for a turn no longer in hand is never taken.

Such a turn's answer is held and dropped, as a sound's is, the worker says
`ProviderOutput::Unclear` before its `TurnComplete`, and once the turn is
over the coordinator asks again with a written turn of the lamp's own
(`speak_up::ASK_AGAIN_NOTE`): the model says only "Sorry?" in the
conversation's language. Someone speaking first takes the turn and the
asking is dropped; so is one not begun within 5 s. Otherwise languages written in
the Latin script are not told apart; outside a conversation nothing changes; on
the second route nothing is asked again yet.

## A spoken cut-in, told (#828)

On lamp-4ace on 19 October at 02:59 (queue 22f, take 6), "Okay, okay, that
is enough." was heard right and committed over the lamp's story. The answer
was the story again, from about where its playback was cut, in new words: a
new generation, not old audio. A touch or the stop button tells the model
what it was saying is over (`LAMP_STOP_TOLD`, above). A spoken cut-in told
it nothing, and with the story cut mid-sentence in its context, "Okay, okay"
read as "go on". In the first builder's count of the 49 take-6 files since
16 October (03:15 on #828,
`~/lamp-voice-data/scenarios/2026101[6-9]T*cutin*-6.txt`), 24 were heard
right (20 acknowledged, 3 silent, this one the story again), and 25 were
misheard (19 silent, 6 the story went on or something else): the misheard
ones are `ASK_AGAIN_NOTE`'s case (above).

With `LAMP_CUT_IN_TOLD=on` (the provider worker's `--cut-in-told`, right
after `--going-on-asks`), the request that commits a spoken cut-in over the
lamp's own audible answer (its Start's `over_the_lamps_voice`) is told
beside what is heard, in the form of the lamp's other notes:
"(For you to know, not to answer: they spoke while you were speaking, and
you stopped there. Answer what they said; go back to what you were saying
only if they ask you to.)" (`crate::cut_in_told::NOTE`).

- **Not after a stop:** the first request after a hand or head stop is told
  the stop instead.
- **Not over a silent reply:** a line begun while a reply was in hand but
  made no sound is no cut-in.
- **One note per request:** a request sends only its newest note, so this one
  goes with the others due on it, the name heard among them (`add_note`).
- **The Live route only.** It needs no other switch. Off unless set.
- **Its cost:** a few words more beside such a request, and nothing waits on
  it. That the model then answers the cut-in rather than go on is what the
  ten cut-ins, A/B, three rounds each, will show.
- **Tests:** `provider_worker::tests::with_the_switch_a_cut_in_over_the_lamps_answer_is_told_so`
  (told with the switch, not without it, not over a silent reply, the stop's
  note instead after a stop),
  `provider_worker::tests::review_2026_10_14::review_sound_a_cut_in_that_names_the_lamp_is_told_both_notes_together`
  (the name heard too: both notes go out as one), `cut_in_told::tests` and
  `provider_worker::tests::going_on_asks::cut_in_told_is_read_after_going_on_asks`.

## A misheard cut-in's "hm?" (#828)

Asked to say "Sorry?" after a misheard cut-in (`ASK_AGAIN_NOTE`, above), the
model mostly didn't. Of the 59 cut-in scene files since 16 October whose
cut-in turn finished `unclear` (the Fourth's reader,
`askagain_replies_fourth.py`, #828, 19 October 05:04; its split is rough at
the edges, and it calls one story line an answer):
- 13 got "Sorry?";
- about 40 got the story going on, mostly mid-sentence;
- 2 or 3 got an answer to what was really said (two jokes, "is green.");
- 2 got silence.

Tonight's queue 22f no-rt takes 2 and 3 are two of them: the cut-in held as
misheard, the asking played 0.14 s and 0.18 s later, and its words the story
again. The answer held for the misheard line is usually the story going on,
and it stays in the model's context as said. A turn the model answers can't
be trusted to say only "Sorry?" while that story is there (the first builder,
05:07 on #828).

With `LAMP_MISHEARD_HUM=on` (the provider worker's `--misheard-hum`, right
after `--cut-in-told`, and only with `LAMP_OTHER_LANGUAGE=sorry`), no model
turn is made for a misheard cut-in.
- **Which lines:** the worker says such a turn `Unclear` with `cut_in`, when
  its end was a cut-in over the lamp's answer (`ProviderInput::End { cut_in:
  true }`).
- **The hum:** once the turn is over (cut as for the asking), the
  coordinator owes the lamp's "hm?", the head tap's sound that
  `LAMP_UNCLEAR_HUM` uses, with the listening face. It plays under that hum's
  rule: never over someone, never more than 2 s late, and not if the lamp
  spoke since. It is what a person or a pet does when they didn't catch you.
- **The model is told, not asked.** The first request begun within 10 s
  of the hold (`misheard_hum::answers_the_hum`), once, is told beside what
  is heard, with the lamp's other notes (`add_note`): "(For you to know, not
  to answer: they spoke while you were speaking, and you stopped there, but
  you did not catch what they said. This is what they say now. Answer it; go
  back to what you were saying only if they ask you to.)"
  (`crate::misheard_hum::NOTE`). It says nothing of the "hm?": a person who
  goes on talking takes the turn first, and then no hum plays.
- **Everything else as before:** a line held as unclear that was no cut-in
  is asked again with `ASK_AGAIN_NOTE`, and so is everything with the
  switch off. The Live route only.
- **The journal:** `request N: a misheard cut-in: the lamp's "hm?" N ms
  after it was held, and no model turn`; `… its "hm?" was not played (why)`
  when it was owed and dropped, `(cancelled)` by a Stop, the mute or a gap; `… no "hm?" (another turn began)`, or
  another why, when someone took the turn before it was owed; and the
  worker's `request N: the model is told it didn't catch the cut-in before
  it`. `Unclear` carries `cut_in` only when the lamp hums, so without the
  switch the wire is as before. The trace has `unclear_hum_owed` with
  `"after":"misheard_cut_in"`, and `unclear_hum` with `misheard_turn`.
- **Rule 7:** no model round trip. In the loop test the hum's turn begins
  2 ms after the hold. The time in the journal runs from the hold to the
  hum's turn begun; its sound reaches the speaker after the playout start
  (`speaker_first_write` in the trace), not yet measured on a lamp.
- **Its cost:** the 2 or 3 in 59 the model answered right from the audio,
  despite the transcript, now get "hm?", and the person says it again.
- **Tests:**
  `coordinator::tests::second_route::with_the_switch_a_misheard_cut_in_hums_and_sends_no_model_turn`
  (the hum with the listening face and no written turn; a line held as
  unclear that was no cut-in, and the switch off, asked again as before),
  `provider_worker::tests::sounds::with_the_switch_a_misheard_cut_in_is_said_as_one_and_the_next_request_is_told`
  (said as a cut-in, the next request told once and the one after it
  nothing; a line held as unclear that was no cut-in, a cut-in heard right,
  and the switch off, told nothing), `misheard_hum::tests` and
  `provider_worker::tests::going_on_asks::misheard_hum_is_read_after_cut_in_told`.

## Music the detector takes for speech (#2602)

The detector takes tonal music for speech at any level. On lamp-c0a4, a 6 s
tune of two-tone notes over a 65 Hz hum, played from a Mac, opened a turn
admitted 914 ms in (Thirteen on #2602). `tests/it/music_turns.rs` feeds six
tunes synthesised in the test (nothing recorded from people) block by block
through the voice's own detector, at three levels each:

| | turns opened | ≥30 speech blocks (past the length gate above) |
|---|---|---|
| Piper's "Stop. Please stop talking." | 1 | 1 (89 blocks) |
| 6 tunes × 3 levels (peak 0.5, 0.15, 0.05) | 17 of 18 | 17 of 18 (205 to 824 blocks: the whole tune) |

The only one that opened nothing was the beat with a bass, at the lowest
level. With `LAMP_MUSIC_TURNS_WAVS=DIR` the test writes the tunes there as
16 kHz WAVs, to play in a room or score off the lamp.

**`LAMP_SPOKEN_TO=on` already holds such a turn's answer** (the product unit
and lamp-4ace set it): music shows no sign it was said to the lamp, so its
answer is held as not spoken to. A gate on `ears/classes` at the answer
(#2726, closed) would have changed nothing there. Holding the turn itself
before it is admitted can't use the classes either: a candidate is decided
within 200 ms (`admission::DECISION_BUDGET_US`), and YAMNet's first window
of the turn's own sound comes about 1.2 s in. What a music turn still moves
is whatever reacts to admission itself, such as a gesture ended "by
conversation" (#2668).

Off the lamp, the pinned YAMNet (`lamp-ears`'s `score_wav`) scored music
over 0.6 in every window of four of the tunes, in 6 of 16 of the plucked
arpeggio's, and in none of a bright line with vibrato. In 40.8 s of Piper's
speech, none of 80 windows went over 0.02. Over the held chords, at the
music's level and 6 and 12 dB over it, 26 of 27 Piper lines reached a speech
score of 0.06 or more, against at most 0.02 in the tunes alone.

## An answer with no word in it

Told to stay quiet, the model sometimes says so aloud: an answer whose
transcript is "..." and whose sound is a second or two of nothing. To the
runtime that is an answer. The lamp shows its speaking face, counts the
turn as answered, and whoever speaks next is taken to be interrupting it.
A wording of the thread note produced sixteen of these in two scenes
([conversation-thread.md](conversation-thread.md)); with that gone they
came back on a later character file, three in one scene, to lines between
two other people.

The provider's transcript of an answer arrives before the answer is played
(the first sound is held 350 ms; on the lamp the "..." came about 180 ms
after it). When the first words of an answer to arrive have no letter or
digit in them and none of it has been played, the turn ends there with
outcome `nothing_said`. It is not played, and for the purpose of knowing
whether the room's talk has been for the lamp
([group-talk.md](group-talk.md)) a turn counts as answered only if the
lamp was heard in it. An answer that begins "..." and goes on to words is
lost with it. None was seen.

## A function's name said aloud

With seven functions declared (2026-10-13, the memory's three added), the
model sometimes writes a call as words and does not make it: in one test
three answers were "not_for_me", played through the speaker to two people
talking to each other. The same guard now ends an answer that opens with
a function's name (a word of lower-case letters with an underscore in
it, or the service's own way of writing a call, such as
`print(default_api.not_for_me())`) before any of it is played, with
outcome `nothing_said` and a `function_named` record. Its words are not
kept as something the lamp said (`Thread::lamp`), so they are neither
told to the next session nor taken for a conversation going on. On
lamp-4ace, in the next run in which it happened, "not_for_me()" came
twice as an answer's text and neither was played.

A call written as words is kept nowhere, on either route
(`not_speech::bare_call`; lamp-4ace, 2026-10-16 17:12, the lamp's words were
`not_for_me{}`):

- `voice/said` (`bus_out::BusOut::said`, the one place both its senders go
  through): an answer that opens with a function's name publishes none of
  its words, and a call later in the answer is left out wherever it comes,
  even split across pieces (" not_for" then "_me{}"): a piece that is, or
  with the ones held before it makes, a call so far is held back and never
  published (`not_speech::CallsLeftOut`). A call split inside its first
  word (" not" then "_for_me{}") still leaves its tail in.
- The thread: the same, a call later in an answer left out and the rest
  kept.
- The backup voice: a reply that is `not_for_me…` written as words ends the
  turn in silence, as the call does, with no announcement; any other call
  written as words is left out unsaid, and not run.
- The second route, a call narrated as words
  (`not_speech::opens_with_a_call_aloud`; lamp-06c1, 30 Oct: 13 replies
  opened "Calling not_for_me." or "Calling recall about the vet
  appointment.", 8 with no call in their round, each kept in the thread as
  the lamp's line and copied by the next replies): a reply's opening is held
  only until it can't be "calling " and a function's name (one with an
  underscore, or remember, recall or forget), so another reply's first piece
  waits a token at most. One that is has that sentence dropped: never said,
  never kept in the thread or the next round's messages. With nothing else
  in its round, a narrated `not_for_me` is the decline (unless the line
  called the lamp by name to a face, #2191), and any other is asked once
  more, told to call the function (`CALL_ALOUD_NOTE`). Beside a call or
  other words, those go on. "Calling your sister is a good idea." is said.

## Tests

Neither rule has a test of its own: both are a few lines in the service's
main loop, on a cancellation path that existing coordinator tests cover for
the provider's own interruptions. The whole `lamp-voice` host suite passes
with them (`cargo test -p lamp-voice -- --test-threads=2`; one test of a
motor worker's two-second start timed out in one such run and passed alone
three times in three).

On lamp-4ace, `sounds2.sh`: a question, then eight short sounds eleven
seconds apart, each more than four seconds after the lamp's last word (a
played "Ha.", "Mm." and "Uh.", a synthetic throat-clearing, four of the
Mac's alert sounds), then a second question. The two questions were
answered and none of the eight sounds was. One ("Ha.") was taken for the
start of speech and ended as `not_speech` 0.64 s later; two ("Mm.", heard
as "M&M", and an alert sound) were long enough to be sent on and the model
left them unanswered; five were never taken for speech at all. No restart,
and the turns after the dropped one went as usual. These are played and
synthetic sounds. A person's cough beside the lamp, which is what was
reported, has not been tried on purpose.
