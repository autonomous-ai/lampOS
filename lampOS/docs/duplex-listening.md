# Hearing while speaking

Scope: `MAIN-050`. The duplex turn-taking mode and the capture-clocked echo
reference, both opt-in, on `lamp-a1-duplex`. Code:
`crates/audio/src/{duplex,drift,evidence}.rs`,
`crates/voice/src/{turn_taking,reference,audio_workers}.rs`, and one commit
of changes proposed to Main's coordinator, admission, options and wire.

The proposal, from "Answer" on, was measured on recordings, in tests or
under emulation before anything ran on a lamp, and is kept as written.
Since 2026-10-12 the mode runs on lamp-4ace, and what that showed and
changed is in the next section and in the dated subsections of "The
decision". No default changes: the mode is still chosen by flag.

## On lamp-4ace, 2026-10-12 and 13

`lamp-voice serve … --turn-taking duplex --microphone mic2 --reference-clock
capture`, with played prompts from another machine's loudspeakers a metre
away unless a line says otherwise. One evening, one room, one lamp.

- **It stops for a person speaking over it in about six of ten tries and
  then takes what they said.** Ten batches of ten played cut-ins, each
  four seconds into a story: 4 to 8 stopped for per batch. Whenever the
  words came through, the lamp answered them. A lone "Stop." is usually
  not heard at all. In the one recording of a person speaking over this
  lamp, replayed through the lab, the rule in force would have stopped it
  0.25 s after they began.
- **It rarely stops for its own voice now.** Over 24 ordinary replies each
  time: 4 pauses at first, 2 once the head kept still and the lamp's own
  first word was no longer read as a person, 1 with a longer run asked for
  in a reply's first second. Each is a pause of about 0.7 s and nothing of
  the reply is lost. One became an interruption, once, before the last two
  changes: the lamp stopped itself half a second into a story.
- **What decides a cut-in is the canceller's state, not the rule.** In a
  recorded session with a still head the canceller took the lamp's voice
  down by 60 to 72 dB and a played "Lamp, stop." passed through it
  untouched. In ordinary running the detector often scores a played voice
  near zero for as long as the lamp is loud, and nothing downstream can
  count what is not there.
- **What changed on the way**, each with its reason below: seven blocks to
  yield and ten in a reply's first second ("Seven blocks", "The first
  second of a reply"); nothing read into a block until the lamp's own
  sound has been matched recently ("The first moments of a reply"); a
  short word heard plainly is an interruption ("A short word, heard
  plainly"); an utterance stays open while the lamp has stopped for it
  (under "The decision"; it was a crash); a longer wait before going on
  (item 3 there); and a record of every block in the trace ("What was
  heard, block by block").
- **What the next reply says is the model's.** See "After the
  interruption" and [character.md](character.md).
- **Not known:** how a person at the lamp fares. Every count above but one
  is a loudspeaker.

## On lamp-4ace, 2026-10-14: what recordings of a word over the lamp show

(The evening of 2026-10-14, 18:00 to 19:35, played three times as the
lamp runs and three with the steady reference, gave 13 of 28 sentences and
5 of 17 stop words as it runs, with 3 pauses for nothing in 53 trials:
[steady-reference.md](steady-reference.md), "Played on lamp-4ace, off and
on". One more run, at 19:36, with the lamp's voice 3.9 dB quieter (its
soft volume from 75 to 62, -7.5 to -11.4 dB) so that it reaches the
second microphone further from that microphone's ceiling: 6 of 10
sentences, 0 of 6 stop words, no pause for nothing. One run, no sign that
it helps; the volume was put back.)

Played again that night, with nobody in the office, the checks gave what
they had given: the lamp stopped for six of ten sentences spoken over it,
0.5 to 2.2 s after the person began, and paused for a seventh and went
on. Of seven short words that asked it to stop or wait ("Stop.", "Lamp,
stop.", "Wait.", "Okay, stop.") it stopped for one, and of three sounds of
someone listening ("Mm-hm.", "Yeah.", "Uh-huh.") one stopped it.

To see why, the lamp's microphone was recorded while that was played:
four runs of 87 s, each three story requests with a word played four
seconds into the answer, one run of the same words into quiet, and two
more runs with the lamp's first microphone recorded beside the second
(`record_run.py`, `record_both.sh`; the sound the microphone gave, the
sound after the canceller and the sound sent to the speaker, with the
record of which speaker samples went with each block). What they show:

- **The canceller is WebRTC's AEC3** (the `sonora` port,
  [architecture.md](architecture.md)), not a simple one. What is weak is
  what it is given.
- **The echo does not stay where the canceller expects it.** Against the
  speaker samples the canceller was handed with each block, the lamp's own
  voice arrived 91 ms late sixteen seconds into a run and 129 ms late at
  thirty-nine, moving in steps of about 5.4 ms, and after that stepped
  back and forth by one such step every few seconds (two runs: 90.9 to
  131.4 ms and 85.6 to 131.6 ms; the canceller's own estimate followed
  from 80 to 128 ms). The second microphone is a USB device with a clock of its own
  (its endpoint is asynchronous); the speaker is another device.
- **Little of the echo in the second microphone is a linear copy of the
  speaker's sound.** Over 38 stretches of two seconds of the lamp's own
  speech, the most a fixed linear filter could have taken off, by the
  coherence of the two, was 1.8 to 14.3 dB, with a median under 7. A
  canceller's linear part has that much to work with; the rest of the
  silence it makes is its suppressor's, and a suppressor cannot tell a
  person from the echo it is removing. That is the "canceller's state"
  above.
- **The lamp's own voice is as loud in its microphone as a person is, and
  sometimes too loud for it.** Its own speech sat at -9.6 to -16.4 dBFS
  over two-second stretches in four runs, and 0.01 to 0.05% of samples
  were at the microphone's ceiling. The same played words into quiet
  arrived at -12.3 to -27.4 dBFS: two of the three synthetic voices
  mostly at -12 to -16 (one short "Stop!" at -23), and the third at -21
  to -27. In the played check that night none of the three words in that
  third voice made the lamp so much as pause.
- **The first microphone hears the echo in one place.** It is the input
  of the same USB card as the speaker, both on the board's clock, and
  over two runs the echo's place in it did not move by more than 0.1 ms.
  But it is a plain microphone a few centimetres from the speakers: the
  lamp's voice is at -13 to -17 dBFS in it and the loudest part of a line
  played from a metre away at -43, some 27 dB under. (The second
  microphone had the same line at -12, level with the lamp's own voice.) Its echo was no more linear by the same measure (8.6 to
  13.2 dB in 32 ms windows), and with one filter for a whole run nearly
  all of that was under 1 kHz; whether that is the path or a drift of
  less than a sample in how the two recordings were laid together is not
  settled.
- **The chips' "Auto Gain Control" switch is not it.** Both cards have one
  and both were on. With both off, nothing about the second microphone
  changed (levels, the moving echo), and the first became 11 dB quieter
  in everything. They were put back on.

Two ways of finding a person without the canceller were then tried on
these recordings, off the lamp, and neither is a basis for a rule:

| Tried | On the recordings |
|---|---|
| A linear canceller of our own for each frequency, guarded so a person cannot throw it off, and the energy it leaves where it took little away | It took nothing off the second microphone's echo (what it left was 3 dB under what came in in one run, level with it in one, 6 dB over it in one, and in the fourth it ran away) and 5 to 7 dB off the first's. At any level the played words passed, a third or more of the lamp's own speech passed too |
| No canceller: the energy in each of 20 bands against what the speaker's sound of that moment would put there, by a ratio learned as it speaks | At the levels where 10 of 14 words passed, the lamp's own speech passed 45 to 89 times a minute |
| WebRTC's AEC3 on the first microphone, with the speaker's sound at the one fixed delay it has there (tried on the evening of 2026-10-14, two runs, `both-a` and `both-noagc`) | The delay held to within 0.09 ms over 68 s. It took 24.7 and 25.4 dB of the lamp's voice off over all, 20.4 to 32.6 dB in two-second stretches (the second microphone's canceller: 11.7 and 13.0 dB over all, 4.8 to 64.6 dB). But a person is 25 to 30 dB under the lamp there before cancelling, so what was left of the lamp's voice was still louder than the played word in 6 of 7 cases (by 4.0 to 31.5 dB; the seventh 2.0 dB over). With the lamp's own start rule (speech score 0.8 for six blocks) it caught 0 of the 7 played items, with no false start in 1.4 minutes of the lamp speaking; on the same recordings the second microphone's rule caught 2 of 7 (both sentences) and started falsely 9.5 times a minute. Played words added in software to the lamp's speech (139 trials, one voice) said the same: "stop." 0 of 30 on the first microphone, 7 of 30 on the second |
| The name part (`lamp-name`, [name.md](name.md)), which listens for "Lamp" in the voice microphone's sound, as a sign someone is talking to the lamp over it (played on lamp-4ace on the evening of 2026-10-14, read from its `name/heard` records) | It heard the name in most story requests played into quiet, and in 2 of the 15 played lines that said "Lamp" over the lamp's voice ("Lamp, stop.", "Hey Lamp, how many legs..."), in one of which the lamp had already stopped itself. It hears the sound after the voice's canceller (`mic2/audio`), the same sound the voice decides on |

Not shown, and it matters more than any of this: **a person.** Every
word was a synthetic voice from a loudspeaker, and the note that came
with the second microphone says its chip treats a live voice differently
from a played one. What is left to try, in the order of what each would
tell: a person cutting in, recorded the same way; a reference that
follows the second microphone's clock, so that the echo stays in one
place and the canceller's linear part can be measured again; a
listener for the few words that mean stop, trained on recordings like
these of words under the lamp's own voice; and the lamp's own level,
which is at its microphone's ceiling.

(The canceller on the first microphone for the decision to stop alone was
on that list; it is the third row of the table above, and is not a basis
either while that microphone sits a few centimetres from the speaker: it
could serve only with the microphone moved or shielded from it, or the
lamp much quieter. The analysis, its scripts and outputs were kept in the
first builder's scratch folder, not in the repository.)

Nor is the first microphone a better echo reference for the second than
the speaker's own sound (same evening, same two runs, the measure above:
two-second stretches of the lamp's own speech, what a fixed linear filter
could take off the second microphone). From the speaker's sound: median
7.6 and 6.8 dB; from the first microphone: 6.8 and 6.6 dB, less in 23 of
26 stretches. The two agree below 1 kHz and the first microphone is much
weaker at 2 to 4 kHz (coherence 0.06 to 0.19 against 0.44 to 0.63). So what
the speaker's sound cannot explain in the second microphone is probably
not in the sound the speaker makes (guessed, not tested). One more thing
measured: following the second microphone's moving delay every half
second, rather than one delay a stretch, raised what the speaker's sound
explains from about 7 to about 11 dB median (three stretches lost 2.6 to
11 dB that way; the delay was picked piece by piece, which may flatter it).
So about 4 dB of what reads as "not linear" above is the echo moving within
two seconds, which is what the steady reference is for.

The reference that follows the second microphone is built, off unless
set, with a rig that reproduces the moving echo and a replay option for
these recordings: [steady-reference.md](steady-reference.md).

## Answer

- **The lamp can keep hearing while it speaks, on one of the two lamps
  recorded.** Where a person reaches the microphone about as loudly as the
  lamp's own voice does (`4ace`), a synthetic overlap at the person's
  recorded level stopped the speaker in 31 of 33 held-out trials and
  became an interruption in 24. Where the lamp's own voice arrives 22 dB
  louder than the person (`a0ae`), the canceller removes the person with
  the echo and the mode hears almost none of them: no held-out trial of 75
  stopped the speaker, at any level. There it behaves as the conservative
  baseline does, and its trace says how far apart the two levels are.
- **Its own voice did not become a turn in any replay.** In 176 s of
  recorded own speech across four sessions the lamp stopped for its own
  voice twice, both in one development reply, and never lost a reply to
  it. In the 64 s that were held out it did not stop once. Each such stop
  is a pause of about half a second, after which the reply goes on from
  the sample it stopped at.
- **A long session no longer ends at eleven minutes.** The fixed reference
  margin is used up by the measured difference between the speaker's and
  the microphone's clocks after 66,326 blocks. The capture-clocked
  reference ran four virtual hours at that difference with every counter
  intact, and five minutes under real pacing before the host stalled. It
  still faults at once on real loss.
- **The 200 ms target is met for a loud interruption and missed for an
  ordinary one.** From the start of speech to the decision to stop took a
  median of 240 ms at the person's recorded level and 110 ms at 6 dB
  above it, held-out. About 30 ms more of audio is already with the
  speaker at that moment.
- **Every positive is synthetic.** They are recorded loudspeaker prompts
  added to the microphone in software. No recording of a person speaking
  over a lamp exists yet, and the microphone board's own processing may
  treat real overlap differently. See [what is not established](#not-established).

## Turning it on, and off

```sh
lamp-voice directed /absolute/private/provider.json 40 /new/private/run \
  --turn-taking duplex --reference-clock capture
```

| Flag | Values | Default | Effect |
|---|---|---|---|
| `--turn-taking` | `vad-only`, `v1-baseline`, `duplex` | `vad-only` | `duplex` is `v1-baseline` with hearing |
| `--reference-clock` | `fixed`, `capture` | `fixed` | `capture` makes the echo reference follow the microphone's clock |

`LAMP_FIRST_WORDS=kept` in the voice's environment: a yield's request
begins at the person's first words, as far back as 1.6 s ("First words
kept", below). Off by default; not on any lamp.

`LAMP_STOP_WORDS=heard` in the voice's environment, with the second voice's
configuration: the words said in a yield are heard, and a stop phrase
commits it ([stop-word.md](stop-word.md#a-stop-word-heard-in-a-yield-828-step-2-16-october)).
Off by default; not on any lamp.

`LAMP_STOP_WORDS=heard-always`, with the same: everything heard while the
lamp speaks is heard, and a stop phrase the lamp did not say itself stops
the reply as a hand Stop does ([stop-word.md](stop-word.md#a-stop-word-heard-over-the-lamps-voice-828-16-october)).
Off by default; in no lamp's unit (played on lamp-4ace and lamp-52e6, where
no person's stop came through yet).

`LAMP_STOP_WORDS_SOURCE=raw`, with either: the stop words hear the
microphone's block before the canceller, and the capture worker is given
`--raw-blocks` in its argv, only then. `LAMP_STOP_WORDS_SOURCE=linear`: they
hear the canceller's linear output, before its residual-echo suppressor,
and the capture worker is given `--linear-blocks`, only then
([stop-word.md](stop-word.md#a-stop-word-heard-over-the-lamps-voice-828-16-october)).
Off by default.

`LAMP_STOP_WORDS_HOLD=own-words`, with `heard-always`: a stop heard is
acted on only once the lamp's own words, stamped ahead of when they were
written, cover the block its result arrived in, at most 150 ms later, so the lamp's own "stop" whose text comes late stops nothing
([stop-word.md](stop-word.md#a-stop-word-heard-over-the-lamps-voice-828-16-october)).
Off by default.

The two are separate because a long session needs the second under every
turn-taking policy. With neither flag the supervisor's argv to both audio
workers is byte for byte what it was, and a worker's periodic
`reference_clock` report has no new field. Capture frames do carry echo
observations in every mode; this is not byte-identical microphone metadata
or proof of unchanged processing cost.

The fallback is `--turn-taking v1-baseline`. Inside the mode there are further conservative paths,
none of them a setting:

| When | The mode does |
|---|---|
| A microphone block arrives without echo evidence | Masks that block exactly as `v1-baseline` does |
| A candidate's six onset blocks were not all heard with evidence | Decides it by the `v1-baseline` rules |
| A reply has twice stopped for something that turned out to be nothing | Stops yielding for the rest of that reply and counts a person over the lamp as silence, as `v1-baseline` does |
| A prospective fallback prefix outlives the mask's bounded original-read history | Rejects that candidate with `retained_history_unavailable` and `history_loss=expired` or `evicted`; preserves the old reply and drains the rejected utterance to its endpoint |
| Hearing is older than 100 ms | The mask no longer withholds output; the real conversation still faults on its mandatory capture heartbeat, so this is not a healthy microphone recovery |

Original input validation remains mandatory. A new invalid read, unknown
identity, cached timestamp mismatch, privacy/owner change or invalid capture
lineage is not relabelled as normal history aging. The mask exposes a typed
history-loss distinction only for elapsed history or a sequence it actually
evicted. Constant-size retirement bounds do not reconstruct the exact old
interval and cannot authorize its audio: an unverifiable fallback candidate
is rejected. Complete echo-evidence prefixes retain their existing path.
No 700 ms/1.1 s admission limit, 80-read history capacity, detector threshold,
reference clock, 40 ms reference freshness check or opt-in flag changes.

## What the lamps recorded first

Three measurements from bundles already on the Mac set the order of the
work. Lamp `4ace` is `lamp-4ace-varied-12io918n` (102 s, six turns); `a0ae`
is `lamp-rust-v1-ten-evidence-20261011T070623Z` (151 s, ten turns).

| | `4ace` | `a0ae` |
|---|---|---|
| Speaker clock against microphone clock | about +98 ppm | about +66 ppm |
| Echo delay 7 s into the session | 122 ms | 119 ms |
| Echo delay 45 s in, and after | 177 ms, then 167 ms to 183 ms | 179 ms, then 168 ms to 180 ms |
| Canceller removed, median | 52 dB | 52 dB |
| Cancelled signal above -45 dBFS during own speech | 18 % of blocks | 18 % of blocks |
| Detector starts (six blocks at 0.80) on the lamp's own speech | 20 in 56 s | 21 in 43 s |
| A person at the microphone, before cancellation | -18 dBFS | -33 dBFS |
| The lamp's own voice at the microphone, before cancellation | -14 dBFS | -10 dBFS |

The clock figures have a resolution of one block and read as 50 ppm to
150 ppm. The delay growth of 55 ms is in no queue either device reports:
the speaker queue stays at 456 frames and the capture backlog at 16
throughout. Its cause is not known.

So: fed the cancelled signal, the speech detector fires on the lamp's own
voice every two to three seconds, and the leaked voice is as loud as a
person. Neither the detector's score nor a level can carry an interruption
decision. What can is what the render explains.

## What V1 does, and what is kept

Pinned V1 (`d5efe9d7b`, `hal/drivers/voice/voice_service.py`) discards
capture while its own speech plays, discards a short tail after it, and
clears its detector before a fresh question. As the Lamp profile
configures it (`HAL_LIVE_MODE=false`), it does not listen for a person
over itself. `v1-baseline` is this repository's
version of that: a mask over the intervals in which accepted speaker audio
is sounding, plus 100 ms.

Duplex keeps all of it. `PlaybackMask::duplex()` is the same mask, fed the
same speaker receipts, holding the same intervals. Whenever it lacks the
evidence to do better, that mask is what decides, block by block. What it
adds is the evidence.

| V1 safeguard | In duplex |
|---|---|
| Own speech is not a turn | Kept: the lamp's own voice reaches the detector as silence |
| A tail after playback | Kept as the mask's 100 ms, and extended: nothing counts as "in the clear" until the render has been silent for 300 ms |
| Detector state cleared around playback | Kept by the mask for masked blocks; heard blocks never raise the detector on the lamp's voice in the first place |
| A person over the lamp is discarded | **Changed**: heard, when the evidence can separate them |

## The decision

One capture block is 10 ms. For each, the capture worker now sends the
`EchoEvidence` for that very block with the microphone frame. The mask
reads it together with the detector's score (`Hearing::heard`):

| The block is | When |
|---|---|
| `Still` | Not speech-like, and the render has been silent for 300 ms |
| `Clear` | Speech-like, and the render has been silent for 300 ms |
| `Under` | Not speech-like while the lamp is sounding |
| `Own` | Speech-like while the lamp is sounding, and the render accounts for it |
| `Over` | Speech-like while the lamp is sounding, and the render does not account for it |
| `Unknown` | Speech-like while the lamp is sounding, with no alignment to judge by |

Speech-like is a detector score of 0.5 or more. The render accounts for a
block when it explains more than 75 % of the microphone's energy before
cancellation, or when more than 80 % of the cancelled signal has to be
playback by energy accounting. Neither a score nor a level decides
anything alone: a score of 0.99 with the render explaining the block is
the lamp. The 300 ms is the longest the microphone took to fall back to
its floor after a reply stopped, 270 ms over sixteen recorded stops, and
one block.

The turn detector is not changed. It is given a different score: 1.0 for
`Over`, 0.0 for `Own` and `Unknown`, and the block's own score otherwise.
Its rule, six consecutive blocks at 0.80, then starts on a person and not
on the lamp. While the lamp has something to yield, the first `Over` block
of a run is given as 0.0 too (as many blocks as the yield takes more than
the detector's six), so that the detector's six end on the block that
yields and a candidate never arrives ahead of its yield.

A run of blocks becomes at most three moments (`Interruption`):

1. **Yield.** Seven consecutive `Over` blocks while the lamp has something
   to say. The coordinator hands nothing more to the speaker. It cancels
   nothing and drops nothing: the reply, its owner and its queue stay.
   Ten in the first 1.2 s after an answer's audio arrives, which is its
   first second of sound, and again after a resume. (Six blocks until
   2026-10-13; see "Seven blocks" below. The lab tables further down were
   measured with six.)
2. **Commit.** Six `Clear` blocks after the yield: the lamp is silent, the
   room has gone quiet of it, and speech-like input remains. This is a
   hearing decision, not addressee evidence or completed input admission.
   Or, since 2026-10-13, forty consecutive `Still` blocks after a yield
   in which at least ten `Over` blocks were **plain**: the render explained
   no more than 20% of the microphone. A short word that the lamp's own
   voice cannot account for is a person telling it something, "stop" as
   likely as anything, and the lamp does not talk over that. See "A short
   word, heard plainly" below.
3. **Resume.** Forty consecutive `Still` blocks with fewer plain blocks
   than that, or 1.1 s without a commit. The coordinator hands the speaker
   the next sample of the reply.
   (Fifteen blocks and 600 ms until 2026-10-12. On lamp-4ace a played
   "Lamp, stop. What is two plus two?" over a reply made the lamp yield
   0.5 s after the clip began, resume 0.5 s later, in the breath after the
   first word, and yield again 0.3 s after that before it committed. A
   lamp that has been spoken over now waits 400 ms of stillness before it
   goes on.)

While a yield is open, what the lamp hears is one utterance. The detector
is never given a score it would count towards the end of speech (anything
it hears is given as at least 0.35, its own boundary for silence), so it
cannot close the utterance before the yield is settled. It used to be able
to: a yield can stay open for 1.1 s and the detector waits 600 ms for the
end of speech. On lamp-4ace on 2026-10-13 a sound over the lamp was
followed by 640 ms of nothing and then by speech; the detector had closed
the utterance 40 ms before, the commit found nothing current to admit, and
the process ended with `directed candidate decision lost its identity`.
Now that is one request: the first sound, the pause and what came after.

The candidate the detector raised at the yield is held, not decided, while
the yield is open. A hearing commit still requires current privacy,
ownership, capture lineage, deadline and original-read evidence. Only then
does the candidate go through the ordinary path (`commit_admitted_input`):
the 30-block prefix includes the six onset blocks (normally 240 ms before
the first onset block), the old reply is cancelled as
`user_interrupted`, a new turn is admitted, and the retained audio is the
start of the request. On resume it is rejected as policy, inside its own
budget, so the microphone is not left draining.

**A reply that yields to nothing twice stops yielding, for a while.** Two
yields in one reply that resumed (`FRUITLESS_YIELDS`) make the mask fall
back to the conservative baseline: no yield, and a person over the lamp
scores 0. Since #1712 (the first builder's yes at 04:40 on 20 Oct) one of
them is forgiven after 20 s of the reply with no yield
(`FORGIVE_FRUITLESS_US`), so at most two count in any 20 s. A noise that
trips it more often still falls back as before, but a long story is never
deaf for its rest. On lamp-4ace at 03:45 that night, a Hindi line and "What
was that?" each yielded and resumed 0.76 s later, at margins near 0 dB
from a speaker across the desk, and "Hey Lamp, what's the weather in
Hanoi?" 30 s later made no request at all. A forced yield (a stop word)
that resumes is no fruitless yield. The session's `duplex_policy` event
gives both numbers (`fruitless_yields_before_fallback`,
`fruitless_yield_forgiven_after_ms`). Each forgiving is traced on the block
it happens (`duplex_fruitless_forgiven`, with `forgiven_so_far`), and
`duplex_summary` counts them (`fruitless_forgiven`):
`fallen_back_at_end` says only whether the last reply had fallen back when
the session ended (Eighth on #2048).

**With `LAMP_CUT_IN_KEEPS_LISTENING=on` (#2602, off unless set), a person
who keeps trying is still heard within those 20 s.** On the Mac's replay
of the coordinator's real path (22 Oct), a third line 3 to 19 s after two
asides made no yield and no candidate, and the story played on over it;
21 and 30 s after, it was heard. With the switch, a reply that has fallen
back still yields to a voice over it that keeps going: 30 blocks in a row
heard as a person over the lamp (`KEEPS_TRYING_BLOCKS`, 300 ms), then the
yield's own onset, about 0.4 s in all. That person is scored to the
detector as before the fallback, so the line commits and stops the story
as any cut-in does. Only `Over` blocks count, and only after the lamp's
own sound has been found again (`MATCHED_BLOCKS`), so the lamp's echo is
never one; a word or a cough is shorter. A voice that keeps going and is
never a line (a television) may cost at most two more fruitless yields
(`KEEPS_TRYING_YIELDS`) before 20 s gives one back; then the reply falls
back as without the switch. `duplex_policy` gives both numbers
(`keeps_listening`).

**With `LAMP_CUT_IN_NEAR_ASKER=on` (#3967, off unless set), talk well
under the person being answered is the room's.** Once a yield has
silenced the lamp, its talk in the clear is compared, before cancellation,
with the level of the person the reply answers (`asker_first`'s middle of
their own line, measured with `LAMP_ASKER_FIRST` at `record` or `on`): the
only place the two can be compared, since over the lamp's own voice the
canceller takes much of a quiet voice with the echo. From its sixth such
block (`NEAR_ASKER_BLOCKS`), talk whose middle level is more than 6 dB
(`NEAR_ASKER_DB`) under the asker commits nothing
(`Interruption::quiet_talk` in `lamp-audio`): the yield resumes at
stillness or its deadline as one that came to nothing, counted as any
fruitless yield, and `duplex_resume` gives `under_asker_db`. The asker, or
anyone about as close, still cuts in at once, and a stop word or a forced
yield still commits. With no asker's level (a line the lamp began itself,
one too short to measure, or another turn's), nothing changes. On
lamp-52e6 on 29 Oct (8474) two people talking with office noise 7 dB under
the asker cut a long answer within about 40 s (`duplex_commit` 343 ms
after its yield, `hearing_margin_db` 3.9).

**With `LAMP_CUT_IN_COUNTS_REAL=on` (#2602, off unless set), only the
lamp's own voice counts as a fruitless yield.** A resume counts toward
`FRUITLESS_YIELDS` only when its yield's evidence window points at the
lamp's own voice (`YieldEvidence` in `turn_taking.rs`). The window is
bounded: the consecutive `Over` blocks that made the yield (its onset run,
7 or 10 blocks) and every block heard while the yield is in force, through
the block that settles it, at most 120 blocks (1.2 s). In it, `Own` and an
`Over` block that is not plain (the render explains more than 20% of it)
are the lamp's own voice; a plain `Over` block and `Clear` are a person;
`Unknown`, `Under`, `Still` and a block without evidence are neither
(Third on #2602: `Unknown` or words that never came cannot show the lamp's
own voice made it yield). The resume counts only when the lamp's own voice
outnumbers a person; a tie or no evidence does not count there. Any other
resume (not forced) counts on a bound of its own instead
(`OTHER_FRUITLESS_YIELDS`, three; the Reviewer-lead's P2 on #2779). What
bounds what:

- Two resumes of the lamp's own voice (`FRUITLESS_YIELDS`), or three that
  were not its own voice (`OTHER_FRUITLESS_YIELDS`), make the mask fall
  back for the rest of the reply, as before. The two counts are separate,
  not summed: one of the lamp's own voice and two others leave neither at
  its bound, so a fourth yield is still made, and the resume after it
  falls back whichever count it adds to.
- The 20 s with no yield gives one back to each count (`FORGIVE_FRUITLESS_US`),
  and a commit clears both.
- With `LAMP_CUT_IN_KEEPS_LISTENING` too, a voice that keeps going costs at
  most two more of either count before the 20 s gives one back.

So two asides heard plainly leave a third line within 20 s heard, the
lamp's own voice twice still falls back, and a television, a radio or
someone else's short words pause a story at most three times in 20 s
(with the keeps-listening switch, five). Mixed with the lamp's own voice,
the two bounds together allow at most four pauses (one of its own voice,
two others, and one more that falls back), and with the keeps-listening
switch at most eight (three of its own voice, four others, and one more).
The 30/40/110-block bounds, the
own-voice cap of two and the 20 s are unchanged. `duplex_policy` gives the
window (`fruitless_counts_own_voice_only`). It stays off until `tools/played/cutin.sh`
has measured the false yields and the asides (#2602, piece 1).

What that means for a person in the room:

- Someone who starts a new question over the lamp is heard from their
  first word, the lamp falls silent, and their whole sentence reaches the
  model.
- Someone who says "mm-hm" quietly makes the lamp pause for about 0.7 s
  and carry on. Nothing of the reply is lost or repeated. Said plainly
  over the lamp it is an interruption like any other word: the lamp stops,
  the word goes to the model, and what happens next is the character
  file's (`docs/character.md`: carry on for an acknowledgment, stop for
  "stop").
- The lamp's own voice, when the evidence misjudges it, costs the same
  pause and nothing else.

This is no judgement of who is being addressed. Two people talking to each
other over the lamp for longer than the confirmation takes will interrupt
it, and so will a television. The mode knows that a sound is speech-like
and is not the lamp. It knows nothing else, and its acceptance basis in
the trace, `directed_session_duplex`, says so.

### Seven blocks

Counted on lamp-4ace on 2026-10-12 and 13 from the `duplex_hearing` trail,
with played prompts: 548 s in which the lamp had something to yield and 29
cut-in clips, each rule replayed open loop over the same blocks (after a
rule fires it is held off for 1.2 s). The sessions themselves ran with six
blocks and then with ten.

| Consecutive `Over` blocks | Clips it stopped for, of 29 | Stops with no clip playing | Per minute of lamp speech |
|---|---|---|---|
| 5 | 20 | 13 | 1.6 |
| 6 | 20 | 7 | 0.9 |
| 7 | 20 | 6 | 0.8 |
| 8 | 17 | 6 | 0.8 |
| 9 | 17 | 3 | 0.4 |
| 10 | 16 | 2 | 0.25 |
| 12 | 15 | 2 | 0.25 |

A stop with no clip is the lamp's own voice let through by the canceller
at the end of a word, where the render's share of the block drops for six
to nine blocks. It costs a pause of about 0.7 s in the middle of a
sentence. Ten blocks ran on the lamp for two batches and removes most of them,
at the price of four cut-ins in twenty that six would have stopped for. A
lamp that does not stop when spoken to is the worse fault, so the rule is
seven: every clip that six stopped for, and the one yield the six-block
rule made on the lamp's own voice in the recording of a person
(`dual-121930-owner-voice-both-mics`, 80 s in) is gone. That recording
holds a run of sixteen where the person speaks over the lamp; seven yields
one block later than six.

Two things the count showed that no run length fixes:

- **Nine clips in 29 were not stopped for by any rule.** In those the
  detector scored the played voice under 0.5 for as long as the lamp was
  loud, so there was nothing to count. A lone played "Stop." or "Lamp,
  stop." was missed in eight of ten tries. One recorded session with the
  head still shows the other side of it: the canceller took the lamp's own
  voice down to the microphone's floor (60 to 72 dB), a played "Lamp,
  stop." came through it untouched and the lamp yielded about 0.1 s after
  the first sound. The canceller's state, not the rule, decides which of
  the two a cut-in meets. Whether a person at the lamp fares as a played
  voice does is not known.
- **The head moving while the lamp speaks goes with the false stops.**
  With the body running: 8 stops with no clip in 6.8 minutes of lamp
  speech under the six-block rule. With the body stopped, over ten
  stories: none. One evening, one room, and the sessions differ in more
  than the body; it is a lead, not a measurement. The head now turns back
  to the person when the answer arrives, before the first word, so that
  its largest move is over when the lamp starts to speak (`docs/body.md`).

Counting `Over` blocks in a window instead of in a row was tried on the
first 362 s of the same data: 8 of the last 12 and 10 of the last 20
behaved like eight and ten in a row to within one clip. A run was kept
because the turn detector starts on six blocks in a row: with a run its
start can be made to end exactly on the yield, and with a window it
cannot.

### The first second of a reply

With the head still and the first moments of a reply set aside (next
section), the lamp still stopped for nobody twice in six long stories and
three times in ten more replies. Three of those five were within 0.6 s of
the lamp starting to speak, where the canceller is still settling and lets
more of the lamp through for longer: runs of 8, 9 and 12 `Over` blocks.
Ten blocks instead of seven were worth four missed cut-ins in twenty when
they applied everywhere. They now apply only while the lamp has had
something to say for 1.2 s or less, counted from the answer's audio
arriving (it plays about a quarter of a second later), and again from the
moment a reply is resumed. A person is that much harder to hear in the
lamp's first second and no harder after it.

### The first moments of a reply

When the lamp starts to speak, the render is sounding but its echo has not
reached the microphone, and the alignment the evidence holds is the one the
last reply left. For 0.2 s to 0.45 s on lamp-4ace the render explained
none of each block. The canceller has not converged then either, the
lamp's first word comes through it, the detector scores it as speech, and
the block is a person over the lamp by every rule above, and a plain one.
Most of the yields the lamp made with nobody speaking were there. With the
rule for a short word heard plainly, one of them became an interruption:
half a second into a story the lamp stopped itself, sent its own first
words to the model and said "Sorry, I missed that."

A block now says nothing about who is speaking (`Unknown`) unless the
render explained at least half of the microphone, at the alignment in use,
within the last 40 blocks (`EchoMatch.alignment_age_blocks`,
`Hearing.fresh_blocks`). A block the render does explain sets that age to
zero, so in the middle of a reply every block is fresh, and a person who
starts over the lamp there is heard as before. A person who starts in the
first moments of a reply is not heard over it until the lamp's own sound
has been matched once. Someone who goes on speaking just before the lamp
starts is a different case and is not affected: they are heard in the
clear, before there is anything to yield.

That was not enough. In an hour of ordinary replies the lamp once stopped
itself 0.34 s into "I'm doing great!", sent its own words to the model and
said "Sorry, I missed that. Could you please repeat it?" The render had
explained the first two blocks of that reply, which made every later block
fresh, and then none of the next twenty. So the mask also counts
(`turn_taking::MATCHED_BLOCKS`): since the lamp last had nothing to say,
or last went on after a pause, the render must have explained at least
half of the microphone on fifteen blocks before any block can be a person
over it. Until then such a block is `Unknown`.

### A short word, heard plainly

Until 2026-10-13 a yield became an interruption only if speech went on
once the lamp was silent, which takes at least 300 ms after the yield. A
lone "Stop." is over by then. In the last batch of played cut-ins before
this rule the lamp paused for both "Lamp, stop." and "Stop.", waited, and
went on with its story.

What tells such a word from the lamp's own voice misjudged as a person is
how much of the block the render explains. Over 855 s of lamp sound on
lamp-4ace, 72 runs of seven or more `Over` blocks, counting the blocks of
each run (and of the 300 ms after its seventh) of which the render
explained under 20%:

| | Runs | With ten or more such blocks |
|---|---|---|
| While a played voice was speaking over the lamp | 58 | 27 |
| With no clip playing | 14 | 2 |

Ten of those fourteen held none at all, and two more held two and seven:
the render still explained 30% to 75% of them, as it does when the
canceller lets the lamp through at the end of a word. The two that would
have stopped the lamp were 160 ms and 170 ms of something speech-like
that the render did not explain at all,
in an office at night with nobody in it; what they were is not known. At
that rate the lamp gives up a reply to a sound that was no person about
once in seven minutes of its own speech. Among the played clips, "Stop."
by one voice held 19 such blocks and "Lamp, stop." by another held none:
the rule makes a short word work when it is heard well, and changes
nothing when it is not.

### After the interruption: what the model says

Stopping the speaker is half of an interruption. The other half is the next
reply, and that is the model's. On lamp-4ace on 2026-10-12, with
`gemini-3.8-live` and played prompts, "Lamp, stop. What is two plus two?"
was played 4 s into a story:

| Character prompt | Runs | Service reported the turn `interrupted` | Reply answered the question | Reply went on with the story |
|---|---|---|---|---|
| "If someone cuts in while you are speaking, drop what you were saying and go with them." | 3 | 3 | 0 | 3 |
| The same prompt, where the service had already completed the story's turn | 2 | 0 | 2 | 0 |
| "…you have been interrupted… never finish the sentence you were in, never pick the story up again… answer only what they just said." | 5 | 5 | 5 | 0 |

In the first row the input transcript held the question each time, and the
reply took up the story from about where the service's own account of the
interrupted turn ends, 2.1 s to 2.4 s after the end-of-speech decision. In
the third the reply was "That's four." after 1.5 s to 1.7 s. Ten runs, one
question, one played voice: enough to change the prompt, not to call the
behavior of the service understood. The character prompt is deployment
configuration (`instruction_file` in the provider configuration); the one
lamp-4ace runs is `deploy/character/lamp.txt`, described in
[character.md](character.md). A deployment that writes its own needs the
stronger rule.

### What was heard, block by block

In serve mode the trace carries a `duplex_hearing` event for every second
in which the lamp had something to yield, and one more when that ends. It
holds one character per capture block in four strings of the same length:
`heard` (0 still, 1 clear, 2 under, 3 own, 4 over, 5 unknown), and in
tenths the detector's score, the share of the capture the render explains
and the share of the cancelled signal that must be playback; `-` marks a
block that was not judged. Three more strings carry the level of every
block before and after cancellation, a letter for each 3 dB down from full
scale (`a` is 0 to -3 dBFS), and the share of the cancelled signal the
render explains. With them come three levels before
cancellation, in dBFS: the loudest block of that second
(`loudest_capture_db`: with the lamp speaking alone, its own voice at its
microphone) and the two slow averages the margin is made of, a person
heard with the lamp silent (`person_db`) and the lamp's own voice where
the canceller let it through (`own_db`). Three more strings say what the
render was doing under each block (#4035): `render_level`, the render at
the matched alignment over that 10 ms block, lettered like the levels;
`render_drop`, how far it is under its own peak over the last 150 ms, a
letter for each 3 dB (`c` and on is 6 dB or more down: the lamp has just
stopped a word and the room still rings with it); and `alignment_age`, the
blocks since that alignment last held, in tens (`4` is the 40-block bar
past which a block is unknown). They are there to show whether the render
was falling at each yield that nobody asked for, as a guard against a
word's tail would assume; nothing decides on them yet. No audio and nothing audio can be rebuilt from.
It is there because a yield that comes late, or never, cannot be explained
from the yield events alone. A finite directed session does not write it:
it can record the audio itself.

### The limit: how loud the person is against the lamp

Every stage after the canceller sees only what the canceller left. When
the lamp's own voice reaches its microphone far louder than the person,
the canceller takes the person out with the echo, and no evidence
downstream can find them. This is a property of where the speaker and the
microphones sit and how loud the reply is, not of a threshold.

`Margin` measures it: a slow average of the level of blocks heard in the
clear against the level of blocks that were the lamp's own voice, both
before cancellation. It is in every `duplex_yield`, `duplex_commit` and
`duplex_resume` event and in `duplex_summary` as `hearing_margin_db`. On
`4ace` it settled between -3 dB and -5 dB and on `a0ae` near -22 dB. The
results below
are the only calibration it has: most overlaps were heard at -6 dB and
above, few at -12 dB and below.

### What the hold costs the shared code

A candidate that began over the lamp waits for the lamp's sound to die
away: about 3 blocks still queued, 30 of silence, 6 in the clear. The
standard admission budget is 200 ms and 20 blocks, and with it the same
person is turned away for capacity before the lamp is silent
(`the_standard_hold_cannot_keep_a_candidate_through_a_yield`). So in this
mode only:

| Bound | Standard | Duplex |
|---|---|---|
| Decision budget after the trigger | 200 ms | 1.2 s |
| Oldest retained block | 600 ms | 1.6 s |
| Blocks held while deciding | 20 | 120 |
| Queue length threshold before appending microphone PCM | 64 entries | 164 entries |
| Total command capacity including the endpoint's one `End` slot | 65 entries | 165 entries |

The duplex column was 700 ms, 1.1 s, 70, 114 and 115 until the yield was
lengthened on 2026-10-12. The transport's bound on the age of input audio,
`lamp_gemini::MAX_INPUT_AGE`, went from 1 s to 2.5 s with it: a request
accepted at the end of its budget brings audio up to 1.6 s old, and at 1 s
the older bound was already under the 1.1 s this mode could retain.

It is still one candidate, one identity, one deadline that nothing renews,
and one bounded list. The queue bound grows by exactly the 50 blocks the
hold grew by, because the accepted request is that much longer; the
standard bound is unchanged and tested to be. The terminal marker already
had this one-slot allowance; making it explicit does not admit additional
PCM or raise a threshold. A second terminal append at total capacity is
refused without growing the queue.

## Stopping the speaker

Two delays, reported apart.

**Detection**: from the first sample of the person's speech to the yield.
Six blocks is the floor, 60 ms. The evidence looks back 120 ms, so the
person has to fill enough of that window to stop the render explaining
it. Measured in the lab on `4ace`, held-out, 33 trials a level:

| Person against recorded level | Median | 90th percentile | Worst | Within 170 ms |
|---|---|---|---|---|
| +6 dB | 110 ms | 332 ms | 930 ms | 24 of 33 |
| 0 dB | 240 ms | 610 ms | 1,000 ms | 13 of 31 that stopped |
| -6 dB | 345 ms | 850 ms | 1,180 ms | 6 of 22 that stopped |

**Stop**: from the yield to a silent speaker. The coordinator's dispatch
returns without sending in the same pass of its loop in which the yield
was decided (`a_person_over_the_lamp_stops_the_speaker_...`: the block that
established the person is not followed by a chunk). What plays on is what
the speaker already has: at most one chunk of 10 ms in hand, and the
456 frames, 19 ms, that ALSA reported queued throughout both recorded
runs. That is about 30 ms. It is arithmetic on recorded queue depths, not
a measurement of sound stopping.

While the lamp is yielded the speaker is given nothing, so it keeps its
clock with zeros and reports `playback_gap`, as it does when a reply is
starved. The raw accepted-zero receipts, cursors and timestamps remain
unchanged; no gap is automatically excluded from starvation.

Each coordinator yield has a bounded observation slot and an attempted
`duplex_yield_end` record with its original owner, start/end coordinator
timestamps, source-read/frame identities, missing-evidence marker and
outcome. Outcomes distinguish resume, hearing `commit_decision`, evidence
discontinuity, capture-lineage change, stale hearing, reply completion,
ownership change and cancellation/session termination. `commit_decision`
can still be followed by a rejected prefix; only `input_admitted` records
actual admission. A missing evidence block followed by resumed capture
closes the interval instead of leaving the trace open. This bookkeeping
never authorizes input or output, and keeps only one interval independently
of event-buffer flushes.

These intervals concern coordinator withholding, not DAC silence or
acoustic cutoff. Correlation requires matching owners and actual cursor/time
boundaries; missing or dropped metadata leaves attribution unknown. Existing
metadata backpressure and process death can still prevent a terminal record
from being retained. How the speaker worker behaves over half a second of it on
a device is not established; the longest starvation gap recorded so far
is 160 ms.

Against the target of 200 ms from a clearly established onset to stopped
output, taking 170 ms of detection plus 30 ms of stop: met in 24 of 33
trials at 6 dB above the recorded level, 13 of 33 at that level and 6 of
33 at 6 dB below. The onset here is the first block of the prompt as the
detector first heard it in the clear, which is earlier than "clearly
established" by any reading; counted from the yield, the stop is the
30 ms. In the slow trials the decision was waiting for its evidence: six
blocks in a row that the render did not explain had not come sooner.

Request identity through the stop: the held candidate keeps its identity
and deadline from trigger to decision (`the same candidate, not a later
one`), the old reply ends through `cancel_reply`, and the new turn is a new
owner with a new request. Nothing duplex adds can write to the speaker
after that: the old reply's audio left with the old reply. Stale
completions, late bursts and reconnects after an interruption are for the
existing cancellation path to refuse, as after any interruption. Duplex
enters that path and adds no way round it. I added no test of those cases
and changed none.

## The reference clock

The canceller needs one render block for each capture block. The speaker
and the microphones are separate USB devices, and the speaker accepts
about 98 ppm more than one block in that time on `4ace`. `ReferenceAssembly`
counted block against block with a margin that starts at two and may not
leave one to eight. Through the production calls, in virtual time
(`the_fixed_margin_is_used_up_by_the_clock_difference_measured_on_the_lamps`):

| Speaker against microphone | Fixed margin ends in | After |
|---|---|---|
| +98 ppm | `ReferenceBacklog` | 66,326 blocks, eleven minutes |
| -98 ppm | `ReferenceLate` | between 5,000 and 25,000 blocks |

No recorded session is long enough to have shown it.

`--reference-clock capture` puts accepted render frames in a bounded ring
and reads one block out per capture block at a rate within 500 ppm of one
frame per frame (`lamp_audio::drift`). The rate follows how full the ring
is, slowly: a clock difference is slow and steady and is absorbed; loss is
fast and is not.

- **Every accepted sample is used once, in order.** The read is a
  16-tap windowed-sinc interpolation. At exactly one frame per frame the
  output is the input bit for bit. Off rate, a tone stays a tone to better
  than -65 dB up to 3 kHz and -45 dB at 7 kHz.
- **No counter is reset.** `accepted_through`, `analysed_through` and
  `capture_blocks_processed` remain the counts they were. After twenty
  virtual minutes at ±98 ppm, accepted minus analysed equals what waits in
  the ring, to the frame.
- **No silence is made up, and the ring is bounded.** It holds ten blocks
  at most: five kept in hand, the four that can be waiting after the
  longest hold-up a reference survives, and one. Dry or over is a fault.
- **Loss faults exactly as before.** Packets stop: `ReferenceLate` within
  1,002 to 1,005 blocks of the same schedule in either mode. A skipped
  packet is `CursorGap`. A flood is `ReferenceBacklog`. A difference of
  2,000 ppm, which no crystal has, is not followed.
- **Four virtual hours** at +98 ppm and at -66 ppm through the real render
  path: 1,440,000 blocks each, no fault, learned rate 98.0004 ppm and
  -65.9994 ppm (`four_virtual_hours_capture_clocked`, ignored by default;
  29 s in a release build).

The ring keeps about 45 ms of reference in hand where the fixed margin
kept 20 ms at the start. The canceller is told, through the same delay
hint it already takes.

The worker's `reference_clock` report, once a second, gains a `drift`
object in this mode only: the rate in force, the learned difference, and
the frames waiting. A reference fault carries it too.

### Paced by the real clock

`crates/voice/examples/reference_pace.rs` runs the production reference
calls and the real canceller in one process, fed by a second process that
sends one accepted-render packet per block of a clock running fast by a
chosen amount, over the same datagram transport. Both are paced by the
host's real clock. There is no audio device, no USB clock and no ALSA: it
is this software under real pacing and scheduling, on a desktop.

**No run reached the length it was asked for.** This Mac stops every
process for 40 ms to 65 ms a few times in five minutes, whether the
process sleeps or spins, and a reference older than 40 ms is a fault by
design in either mode. Runs in a round started together and shared those
stalls.

| Round | Mode | Speaker fast by | Ran | Ended with | Why |
|---|---|---|---|---|---|
| 1, ring of eight blocks | capture | 400 ppm | 20.9 s | `ReferenceBacklog` | The ring: capture loop woken 27 ms late |
| 1 | capture | 98 ppm | 20.9 s | `ReferenceBacklog` | The same, at the same block |
| 1 | fixed | 400 ppm | 126.6 s | `ReferenceBacklog` | The margin, used up by the clock difference |
| 1 | fixed | 98 ppm | 235.2 s | `QueueTiming` | Host: loop woken 65 ms late |
| 2, ring of ten | fixed | 400 ppm | 127.9 s | `ReferenceBacklog` | The margin, used up |
| 2 | capture | 400 ppm | 138.1 s | `ReferenceLate` | Host: no packet for 46 ms |
| 2 | capture | 98 ppm | 18.1 s | `ReferenceLate` | Host: no packet for about 40 ms |
| 2 | fixed | 98 ppm | 258.1 s | `ReferenceLate` | Host: no packet for 50 ms |
| 3, spinning | fixed | 400 ppm | 56.9 s | `ReferenceLate` | Host: no packet for 49 ms |
| 3 | capture | 400 ppm | 56.9 s | `ReferenceLate` | Host: the same block, 52 ms |
| 3 | capture | 98 ppm | 296.9 s | `ReferenceLate` | Host: no packet for 43 ms |

What that establishes:

- **The fixed margin runs out under real pacing as it does in virtual
  time.** At 400 ppm, after 126.6 s and 127.9 s. Six blocks of margin at
  that rate is 150 s; jitter takes the rest. At the measured 98 ppm it
  would be about eleven minutes, and no run on this host lasted that long.
- **The capture-clocked reference held for five minutes at the measured
  difference.** 29,693 blocks, the learned rate 98.8 ppm, the ring between
  963 and 1,886 frames of its 2,400, through capture wake-ups up to 29 ms
  late. It ended when the host kept the speaker process from sending for
  43 ms.
- **At 400 ppm it outlived the fixed margin**, 138.1 s against 127.9 s in
  the same round, with the ring at 2,077 frames at most and the rate at
  473 ppm of its 500 ppm limit, still settling.
- **The first round found a defect.** With a ring of eight blocks and five
  kept in hand, a capture loop woken 27 ms late found more reference
  waiting than there was room for. The fixed margin survived the same
  moment. The ring is ten blocks now, and
  `a_capture_loop_kept_waiting_catches_up_without_a_fault_in_either_mode`
  is that stall in virtual time: it fails at eight. A stall of 35 ms is a
  fault in both modes, because the oldest packet waiting is then 45 ms
  old.
- **Cost**: 76 µs to 132 µs of work per 10 ms block for the reference calls
  and the canceller on silence, and 2.8 MB to 3.1 MB resident, on this
  host.

It does not establish a run of more than five minutes. That needs a
quieter host or a lamp.

## The lab

`crates/voice/examples/duplex_lab.rs` replays a recorded session's
microphone and accepted render through the real canceller, detector and
evidence observer in the recorded interleave. With `--duplex` it closes
the loop: a yield stops the recorded render after three more blocks,
the microphone keeps the recorded echo for twelve and then holds a stretch
of the same session's room noise; a resume plays on from where the render
stopped; a commit skips to the end of that reply. How the echo dies away
is that model, not a measurement. The lab runs `Hearing` and
`Interruption` themselves: the mask's fallback after two fruitless yields
and the admission's hold are not in it. Those are in the production-path
tests.

**Negatives** are the recordings as they are. Nobody speaks over the lamp
in any of them, so every yield is a mistake.

**Positives** are synthetic. A prompt from the same session, as the lamp's
own microphone recorded it while the lamp was silent, is added to the
microphone signal during a reply, before cancellation, one overlap per
reply and per trial, at five levels from 6 dB above its recorded level to
18 dB below, at three random positions. They are loudspeaker speech
recorded once through the lamp's microphone and mixed in software.

| Session | Lamp | Role | Replies | Used |
|---|---|---|---|---|
| `lamp-4ace-varied-12io918n` | `4ace` | development and held-out | 7 | even replies to choose the thresholds, odd replies held out |
| `lamp-rust-v1-ten-evidence-20261011T070623Z` | `a0ae` | development and held-out | 10 | the same split |
| `lamp-connected-extended-20261011-01` | `4ace` | development | 10 | all |
| `lamp-connected-plain-20261011-01` | `4ace` | **held-out** | 8 | all; not opened for any decision before the rule was frozen |

The thresholds were chosen on the development halves of the first two
sessions, on negatives and positives together, before the closed loop
existed. The closed loop was then run on development data only. It showed
a weakness described below, and the rule was **not** changed in response:
it was frozen as committed at `feb560767` and evaluated once on the
held-out material. Every trial, failed ones included, is in
`docs/verification/duplex-lab-20261011/`.

### Own voice

The recordings as they are, closed loop. A stop here is the lamp stopping
for itself.

| Session | Part | Own speech | Stops | Replies lost | Pause |
|---|---|---|---|---|---|
| `4ace` six turns | development | 43.6 s | 2 | 0 | 480 ms, 470 ms |
| `4ace` six turns | held-out | 10.8 s | 0 | 0 | |
| `a0ae` ten turns | development | 21.9 s | 0 | 0 | |
| `a0ae` ten turns | held-out | 18.7 s | 0 | 0 | |
| `4ace` extended | development | 46.3 s | 0 | 0 | |
| `4ace` plain | held-out | 34.4 s | 0 | 0 | |

Both stops are in one reply, 220 ms of it apart: the render stopped
explaining the microphone for a stretch, and the lamp paused twice in a
row. Against the detector alone, which started on the lamp's own voice 41
times in 99 s of the first two sessions, that is the difference the
evidence makes. Two stops in 176 s is too few to give a rate.

### A person over the lamp, `4ace`

Synthetic overlaps. "Stopped" is a yield during the overlap; "interrupted"
is a commit. Delays are from the first block of the prompt's speech.

Held-out: odd replies of the six-turn session and all of the plain one.

| Level | Trials | Stopped | Median delay | Interrupted | Median delay | Paused and resumed | Never heard |
|---|---|---|---|---|---|---|---|
| +6 dB | 33 | 33 | 110 ms | 30 | 520 ms | 3 | 0 |
| 0 dB | 33 | 31 | 240 ms | 24 | 580 ms | 7 | 2 |
| -6 dB | 33 | 22 | 345 ms | 17 | 690 ms | 5 | 11 |
| -12 dB | 33 | 14 | 390 ms | 9 | 680 ms | 5 | 19 |
| -18 dB | 33 | 8 | 505 ms | 4 | 725 ms | 4 | 25 |

Development: even replies of the six-turn session and all of the extended
one.

| Level | Trials | Stopped | Median delay | Interrupted | Median delay | Paused and resumed | Never heard |
|---|---|---|---|---|---|---|---|
| +6 dB | 42 | 42 | 140 ms | 38 | 530 ms | 4 | 0 |
| 0 dB | 42 | 40 | 230 ms | 30 | 555 ms | 10 | 2 |
| -6 dB | 42 | 30 | 280 ms | 20 | 565 ms | 10 | 12 |
| -12 dB | 42 | 15 | 440 ms | 10 | 700 ms | 5 | 27 |
| -18 dB | 42 | 8 | 650 ms | 4 | 700 ms | 4 | 34 |

Held-out and development agree. The person's recorded level on this lamp
is 3 dB to 5 dB below the lamp's own voice at the microphone, so the rows
are margins of about +2, -4, -10, -16 and -22 dB.

### A person over the lamp, `a0ae`

| Part | Trials | Stopped | Interrupted |
|---|---|---|---|
| Development, five levels | 75 | 5 | 0 |
| Held-out, five levels | 75 | 0 | 0 |

The person's recorded level here is 22.5 dB below the lamp's own voice,
so the loudest row is a margin of about -16 dB. The result is no better
than the `4ace` rows at that margin and somewhat worse: it is the same
limit, reached by geometry and volume and not by a setting. On this lamp,
at this volume, a person cannot interrupt by voice in this mode. They are
heard once the reply ends.

### What the closed loop showed, and what was left alone

**A pause after the first words ends the yield.** Of the overlaps that
stopped the speaker at the recorded level, 7 of 31 held-out and 10 of 40
in development were resumed and not committed, and none of those was
committed later. In the development cases opened, the person had gone on
speaking for about 300 ms after the stop, and then 15 blocks in a row
scored below 0.5 just as the room became quiet enough to count: a pause
in the prompt, or its end. The lamp then spoke over the rest of it. A
person who says a name, pauses and asks their question would meet this.

**The obvious repair is not safe on this evidence.** Counting speech the
render does not explain, heard after the stop, towards the commitment
would have kept those overlaps. But one of the two stops for the lamp's
own voice was followed by fifteen such blocks, so the same change would
have cancelled a reply for the lamp's own voice. With two examples of one
and a handful of the other, all from replay, choosing between them would
be choosing a gate on echo negatives. It was not done.

**Software noise suppression** was turned on for one comparison
(`duplex_lab --noise-suppression`), on the development material of `4ace`
only, 42 trials a level:

| | Off | On |
|---|---|---|
| Stops for the lamp's own voice in 89.9 s | 2 | 4 |
| Replies lost to it | 0 | 0 |
| +6 dB: stopped, interrupted | 42, 38 | 42, 39 |
| 0 dB: stopped, interrupted | 40, 30 | 40, 37 |
| 0 dB: median delay to the stop | 230 ms | 120 ms |
| -6 dB: stopped, interrupted | 30, 20 | 34, 28 |

In replay it lets the person be heard sooner and more often, and the lamp
stops for itself twice as often. Four stops are still too few for a rate.
The recordings were made with it off and whatever the microphone board
does is in both columns. This is a replay comparison on development data
and it selects nothing.

## First words kept (#828)

Off unless `LAMP_FIRST_WORDS=kept` in the voice's environment, and nothing
without `--turn-taking duplex` (built by the fourth builder on 16 October
2026, for #828 step 2; no worker flag).

The rule yields only once a run of blocks is plainly someone over the lamp.
Before that, under the lamp's voice, a person's first words are taken for
the lamp's own sound, and the request began 300 ms before the onset
(`PRE_ROLL_BLOCKS`). So those words were never sent. On lamp-4ace on
16 October, "Lamp, stop. What is two plus two?" was sent as its tail and
heard as "vestido". In the first builder's two recordings of 01:51, five
requests said over a story began 1.2 to 2.5 s into what was said.

With the switch, a yield's request begins at the person's first words:
- While the lamp speaks, the detector keeps 1.6 s of blocks
  (`REACH_BACK_BLOCKS`), each with its own detector score before any mask.
- When the rule yields, the request begins 200 ms (`REACH_LEAD_BLOCKS`)
  before the earliest block of that 1.6 s the detector scores 0.35 or more
  (`SILENCE_PROBABILITY`).
- It never begins earlier than a block heard without echo evidence, because
  every block of a candidate is checked against what was heard before it is
  let through. It never begins later than without the switch.
- The hold's decision deadline is still counted from the trigger, and its
  retained age from the usual pre-roll. The lead is room the hold keeps
  besides them (`Hold::keeping_first_words`).
- At the commit, first words older than 2.0 s (`FIRST_WORDS_AGE_US`) are
  dropped from the front, and the drop is recorded as
  `input_prefix_sanitized`, reason `first_words_too_old`. The provider
  refuses audio older than 2.5 s (`lamp_gemini::MAX_INPUT_AGE`), which
  leaves 0.5 s for the handoff. Only the lead is dropped, never the usual
  pre-roll or the onset, and what is sent keeps its own capture times. The
  reviewer on #936 found a late but legal decision committing a lead 2.52 s
  old, on which the provider worker ended.
- The mask remembers 320 blocks of hearing instead of 128, and the provider
  queue's bound grows by the lead.
- Every other start, and every start without the switch, is as before.
- `duplex_policy` records `first_words_kept_blocks` (160). Each
  `input_candidate`'s `first_read_at_us` shows where its audio began.

### A line begun as the lamp's sound ends (#1722)

Off unless `LAMP_REPLY_END_REACH=on`, and nothing without
`LAMP_FIRST_WORDS=kept` (built by the fourth builder on 20 October 2026,
for the first builder's 06:56 on #1722).

- **The fault:** a person who starts as the lamp's reply ends, and leads
  with the name, lost the name. Under the lamp's last words the rule takes
  their voice for the lamp's own sound and doesn't yield. The line starts
  only once the lamp has been quiet 300 ms (`clear_after_blocks`), with the
  usual 300 ms before its onset, after the name. On lamp-4ace on 20 October,
  "Hey Lamp, ask Codex to write a haiku about coffee.", said as the
  lighthouse story ended, reached speech to text as its last 8 words. It was
  held for no name in 3 of the last 4 hourly hand-overs. lamp-name heard
  nothing under the story's end, so the name's own hit can't anchor it.
- **With the switch,** a start while the lamp's sound has been quiet under
  640 ms (`QUIET_BLOCKS_CEILING`, which the duplex rule's 300 ms falls
  inside) begins 200 ms (`REACH_LEAD_BLOCKS`) before the earliest onset of
  the kept 1.6 s. An onset here is the detector's own: a run of six blocks
  (`START_BLOCKS`), each scored 0.8 or more (`START_PROBABILITY`) by its own
  score. So the line begins where it would have, had the lamp not been
  sounding (`TurnDetector::reach_back_to_onset`, `reaches_past_reply`).
- **The lamp's own last words aren't taken.** The canceller leaves them
  scored under the onset's bar, so they never make a run, even scored where
  a yield's reach (0.35) would take them. At most the 200 ms lead before the
  person's onset goes with the line.
- **And only within the line's own stretch.** The onset is looked for back
  from the usual start, as far as a pause as long as the one that ends a
  line (the end-of-speech wait, 600 ms on lamp-4ace). Now and then the
  canceller leaves the lamp's last words scored as an onset (#828's replay),
  and words before such a pause aren't this line's (the reviewer's P3 on
  #2096).
- **The rest is as for a yield's first words:** never past a block heard
  without echo evidence, never later than without the switch, the same 2.0 s
  first-words age at the commit, and the same hold. A line begun 640 ms or
  more after the lamp's sound, and every line without the switch, starts as
  before.
- `duplex_policy` records `reply_end_reach_within_blocks` (64) with it on.
  As before, `input_candidate`'s `first_read_at_us` shows where the audio
  began.

Measured on the first builder's two recordings of 01:51, replayed with
`lamp-audio replay-aec` ([the evidence](verification/first-words-20261016/README.md)):
of six yields over a story, four requests that began 1.2 to 1.7 s late
would begin within 0.43 s of the line's start, and a fifth gains 0.7 s.
Its cost is up to 1.6 s of the lamp's own suppressed voice at the front of
a request: the sixth, which was not late, would begin 0.9 s early, on the
lamp's own voice.

What it does not do: tell the person's first words from the lamp's own
sound. It reaches back over whatever the detector scores as speech, and
the canceller takes out much of a person's speech under the lamp's voice.
A start found after the 1.6 s is still late.

### A line's sound begins before its first word (24 Oct)

Off unless `LAMP_SOFT_START=on`, and nothing without `LAMP_FIRST_WORDS=kept`
(the first builder, 24 October 2026, for the lost first word after a pause
on #3126). The product and lamp-4ace units set it since 25 October, after
lamp-4ace's third A/B (#3411).

- **The fault:** "Lamp, what time is it in London?", said with a pause after
  the name, was heard as "Time is it in London?". The owner's own lines lost
  their first word the same way. A line starts on six blocks scored 0.8 or
  more over the faint floor and keeps the 300 ms before that. The detector
  can take longer than that to be sure of a word. q03's "what", at -11 dBFS
  within 50 ms of its onset, started its line 278 ms in. So the line's sound
  began 12 ms before the word, and speech to text dropped it ("Did I just
  ask you?", four times of four that evening). With 171 ms of quiet before
  the "what", the same kind of line was heard whole.
- **With the switch,** an ordinary start (not a yield's, nor a line begun as
  the lamp's sound ends) keeps 300 ms more than the usual 300 ms
  (`SOFT_START_LEAD_BLOCKS`, `TurnDetector::soft_begin`). That's 600 ms before
  the start's sixth block.
  - **Why 300 ms:** in 60 lines recorded on lamp-4ace on 24 and 25 October,
    a start came 58 to 458 ms after its speech began. Every line with 100 ms
    or more of its sound before the word was heard whole.
  - **Why a fixed lead, not the detector's scores:** they dip inside such a
    word, so they are no guide to where it began. A first version reached
    back along them, 200 ms more and through gaps under 100 ms. It stopped
    short on q03 and q08, which lost their "what" again (the first builder's
    A/B, 25 Oct, 01:58).
- **Bounds:**
  - Additional lead never crosses a block the coordinator marks as not
    silent (`lamp_silent`). A block is silent only when the echo evidence
    does not say the lamp is sounding, no busy answer is kept, and
    `reply_sounding` is false: no reply audio still to hand over, accepted,
    queued or within the playback mask's tail. With echo evidence, the
    sounding guard requires 30 quiet 10 ms blocks (300 ms). Without it,
    the mask's existing fallback tail is 100 ms (`TAIL_US`), not 300 ms.
    These are software evidence bounds, not a measured acoustic tail.
  - The reply's playback token alone is not the test. It remains held until
    audio is retired, even after that audio is handed over and quiet, and
    on 25 October it kept every line begun after an answer, or after a bare
    "Lamp." whose answer waited, from a soft start.
  - It never begins later than the usual start. If pending reply PCM still
    makes the onset's block not silent, it keeps the ordinary prefix rather
    than returning empty audio, which admission refuses and which ended the
    conversation (the reviewer's P2 on #3392). Once the PCM is handed over
    and the mask and echo guards are clear, a retained playback token does
    not prevent the extra lead. The ordinary 300 ms prefix is never
    shortened by these guards, so it can still contain samples that would
    block additional lead.
  - What it costs: 300 ms more of the room before each line goes to speech
    to text. The lead does not stop at a pause, so a name said less than
    about half a second before the question may come with it ("Lamp. What
    time…"), which the second route reads as a call with its line.
- `input_candidate`'s `first_read_at_us` shows where each line's sound began.
  With `mic2/audio` recorded off the bus, which is on the voice's clock, the
  line's onset can be set against it.

## Listening gaps (#828)

Off unless `LAMP_LISTEN_GAPS=on` in the voice's environment, and nothing
without `--turn-taking duplex` (17 October 2026, the first builder's
proposal on #828).

**Why:**
- On lamp-4ace a short "stop" said on top of the lamp's speech is never
  heard over its echo.
- Every yield of the first builder's ten cut-ins on 17 October came where
  the lamp's speech left a pause.

**Two parts, both behind the switch:**
- **The gap** (`crate::listen_gaps`). At a sentence end in a reply that has
  sent at least 2 s (`AFTER_FRAMES`), the next words wait until 250 ms
  (`GAP_US`) after the first quiet chunk.
  - A sentence end is a run of 80 ms (`LOW_RUN_CHUNKS`) of chunks under
    RMS 300 (`LOW_RMS`), then a chunk over it.
  - Each one is held once. None while a yield is pending: the mask hands
    nothing over then anyway.
  - The mask holds the next chunk back in `dispatch_speaker_chunk`. The
    speaker writes real zeros through its zero-only API while speech is
    owed, as for a provider pause, so the canceller's reference and the
    render's coverage stay exact.
  - The speaker's `playback_gap` for those zeros is recorded with
    `listen_gap: true` and not counted as a delivery gap. Each gap is
    recorded as `listen_gap`.
  - Zeros begun in a hold that run on more than 100 ms past its end
    (`OVERRUN_US`) waited for late words: a delivery stall begun in the
    hold. They are counted as a delivery gap when they resume, and recorded
    with `listen_gap_overran_us`. The hold's end counts, not the next
    words' going: those go only once there are words, so late ones are let
    go late (Nine's P3 on #1264).
- **Hearing in the lamp's own silence** (`Hearing::silence_heard`). A gap of
  zeros alone gave the yield nothing to hear:
  - Once the 120 ms render window at the echo's alignment is silent, the
    evidence had no match.
  - A speech block was then `Unknown`, or `Clear` after 30 quiet render
    blocks, and neither starts a yield.
  - Now the evidence says when the render at the echo's alignment is silent
    (`EchoEvidence::silent_at_echo`, the alignment's age), apart from having
    no alignment at all.
  - With the switch, speech there, with an alignment no older than
    `fresh_blocks`, is `Over`. Nothing of the lamp's is in the window its
    echo would come from then, only the room's tail, which the canceller
    has.

**A probe, a model and not lamp data** (described on #828):
- **The model:** the real `EchoObserver` and `Hearing::EVALUATED`, with a
  synthetic voice as the render and a person 7 dB under its echo.
- **A 250 ms gap of zeros:** the person got 5 to 6 `Over` blocks in a row,
  then `Unknown`. No yield.
- **The same gap with a faint sound above the silence bar:** `Over`
  throughout, 7 in a row at 60 to 120 ms.

`duplex_policy` records `listen_gaps` (after 2,000 ms, a quiet run of 80 ms,
a gap of 250 ms, silence heard).

**Tests:**
- `listen_gaps::tests`: one gap at a sentence end after 2 s, held to 250 ms
  from its first quiet chunk; none in the first 2 s, for a pause under
  80 ms, or where the reply's own pause was long enough; each sentence end
  its own; a new reply counts again; zeros begun before the hold or after
  the next words aren't its, and zeros that ran on 300 ms past it are a
  delivery gap too.
- `coordinator::tests::a_listening_gaps_zeros_are_no_delivery_gap_unless_they_ran_past_its_hold`:
  started and resumed in a hold, the reply keeps no playback gap and a
  clean outcome; one that ran on is counted once it resumes. It fails
  with the resumed count taken out.
- `endpoint_session::a_long_answers_sentence_end_is_held_quiet_once_only_with_the_switch`:
  through the coordinator's loop, one hold of at least 200 ms with the
  switch and none without, the speaker handed the same samples either way.
  It fails with the hold taken out.
- `turn_taking::tests::speech_in_a_listening_gap_yields_only_with_the_switch`
  and `duplex::tests::speech_in_the_lamps_own_silence_is_over_it_only_with_the_switch`.
- `tests/it/evidence.rs`'s `a_pause_in_a_reply_is_said_to_be_silent_at_the_echo`:
  through the real canceller. It fails with the new field left unset.

**What it may cost, and isn't shown yet:**
- The hearing applies in the reply's own pauses too. The room's tail or the
  detector firing on the canceller's residue there could yield for nothing.
- The pause may sound stilted.
- The first builder's lamp check counts both: the ten cut-ins plus
  "Lamp, stop." and "Stop." five times each, on and off, the yields with
  nobody speaking, and the story by ear.

## Tests

Deterministic, no device, no recording:

```sh
cargo test --locked -p lamp-audio --lib -- drift duplex evidence
cargo test --locked -p lamp-voice --lib -- reference turn_taking admission options coordinator::tests::duplex
cargo test --locked --release -p lamp-voice --lib -- four_virtual_hours --ignored
```

What the production-path tests in `coordinator/tests/duplex.rs` hold,
through the real hearing, observation, decision, commit and dispatch
functions:

| Test | Holds |
|---|---|
| `the_lamps_own_voice_through_a_whole_reply_...` | 300 blocks of leaked own voice at a score of 0.99: no yield, no candidate, a chunk to the speaker every block |
| `a_person_over_the_lamp_stops_the_speaker_then_interrupts_...` | Yield on the seventh block; nothing dispatched and nothing cancelled while held; commit; one unbroken run of retained audio from before the first word; one `user_interrupted`; a new request |
| `a_brief_acknowledgment_costs_a_pause_and_none_of_the_reply` | A 46-block pause; same owner; every sample of the reply reaches the speaker once, in order |
| `a_yield_that_is_never_settled_is_given_up_at_its_deadline` | Resume at 60 blocks; the candidate ends as policy, not deadline or capacity |
| `the_standard_hold_cannot_keep_a_candidate_through_a_yield` | Why the hold is longer |
| `a_reply_that_twice_yields_to_nothing_masks_like_the_baseline_...` | The third time, no yield and no candidate |
| `with_keeps_listening_a_third_line_soon_after_two_asides_stops_the_story` | `LAMP_CUT_IN_KEEPS_LISTENING=on`: a third line 5, 10 or 30 s after two asides yields, commits and ends the story as `user_interrupted` |
| `without_keeps_listening_a_third_line_within_twenty_seconds_is_not_heard` | The switch off: 5 and 19 s after, no yield and no candidate; 21 s after, heard (the forgiven yield) |
| `with_keeps_listening_the_lamps_own_voice_is_never_a_cut_in` | With the switch: 3,000 blocks of the lamp's own voice, a 25-block word, and 1,000 blocks of voice unexplained before the lamp's sound is found again after a third aside, none a yield |
| `with_keeps_listening_a_voice_that_is_never_a_line_pauses_the_story_twice_more` | With the switch: four tries that resume cost two more yields, then none |
| `talk_well_under_the_asker_is_the_rooms_and_resumes_the_answer` (`turn_taking.rs`) | `LAMP_CUT_IN_NEAR_ASKER=on`: talk 8 dB under the asker resumes the answer, 2 dB under commits; no asker's level, or the switch off, commits |
| `with_counts_real_two_plain_asides_leave_a_third_line_heard` | `LAMP_CUT_IN_COUNTS_REAL=on`: two asides heard plainly, then a third line 5 s after, which commits and ends the story; off, not heard |
| `with_counts_real_the_lamps_own_voice_twice_still_falls_back` | With the switch: two yields whose windows the render mostly explains count, and a third line 5 s after makes no yield |
| `with_counts_real_unknown_alone_is_not_the_lamps_own_voice` | With the switch: two yields with plain onsets and 60 `Unknown` blocks after don't count as the lamp's own voice, and the third line commits |
| `with_counts_real_a_sound_that_is_never_a_line_stops_pausing_after_three` | With the switch: three yields that resume and are not the lamp's own voice fall back (`OTHER_FRUITLESS_YIELDS`); a fourth line within 20 s makes no yield and no candidate |
| `with_backchannel_holds_a_backchannel_resumes_the_reply_on_either_rule` | `LAMP_BACKCHANNEL_HOLDS=on` with `LAMP_STOP_WORDS=heard` ([stop-word.md](stop-word.md), #2939): "Mm-hm.", "Yeah." and "Ừ." (Vietnamese) after a plain word then quiet, and before 250 ms of speech once the lamp is silent, resume with no turn; off, both commit as before |
| `with_backchannel_holds_anything_else_commits` | With the switch: "Stop.", "Wait, I", "What was that?" and "Why?" commit on both rules; no words resume 500 ms after they were asked for (quiet) or at the yield's deadline (speech); off, no words commit as before |
| `with_backchannel_holds_a_backchannel_whose_words_come_late_still_resumes` | With the switch: a 250 ms "Yeah." on the speech rule whose words come 350 ms after the ask is held to them and resumes (#2959) |
| `with_backchannel_holds_an_answer_to_the_lamps_question_commits` | With the switch: "Yes.", "Sure." or "Mm-hm." after the lamp's "Shall I go on?" commit as answers; after a statement, or after a question the lamp has since finished a sentence after, they resume (#2959) |
| `with_backchannel_holds_and_heard_always_a_backchannel_resumes_the_reply` | `LAMP_BACKCHANNEL_HOLDS=on` with `LAMP_STOP_WORDS=heard-always` (#2939, lamp-52e6 23 Oct): "Yeah.", "Okay.", "Right.", "I see.", "Sure.", "Mm-hm." and "Uh-huh." resume the reply on both rules; off, each commits |
| `with_asides_hold_an_aside_resumes_the_reply` | `LAMP_ASIDES_HOLD=on` with `LAMP_BACKCHANNEL_HOLDS=on` and `heard-always` (#2939, lamp-52e6 25 Oct): "Hmm, interesting.", "Interesting.", "Wow." and "Oh, nice." resume the reply on both rules; with the asides switch off each commits; "Interesting, tell me more." commits |
| `with_backchannel_holds_the_five_from_lamp_52e6_resume_even_with_a_face_turned` | With `heard` and the switch (#2939, lamp-52e6 23 Oct): "Yeah.", "Okay.", "Right.", "I see.", "Sure.", "All right.", "Right, right.", "Oh, I see.", "Okay, sure.", "Uh huh." and "Mm hm." resume on both rules, so no turn reaches the busy-room rule's "someone is turned to the lamp"; the duplex rule takes no face |
| `with_backchannel_holds_and_heard_always_only_the_yields_own_final_words_decide_it` | With `heard-always` and the switch (reviewer-two's P2 on #3200): an interim "Okay." then the final "Okay, what was that?", a "Yeah." from before the yield then "What was that?", and a final "Okay." then "What was that?" commit on both rules; a final "Okay." after its interim resumes; an interim alone is held to the bound and resumes with no words; off, each commits |
| `with_backchannel_holds_a_wordless_yield_resumes_the_reply` | With `heard-always` and the switch (lamp-52e6 on #3200): a yield with no words resumes at the deadline on the speech rule (held 730 ms) and 500 ms after the ask on the quiet rule, with no turn; off, both commit as before |
| `with_backchannel_holds_a_question_after_a_pause_commits_though_its_first_word_came_back_empty` | With `heard` and the switch (lamp-52e6 on 24 Oct): "Wait", a 300 ms pause at which its words are asked for and come back empty, then the question in the clear: committed at the question's sixth block; with no question after it, resumed 500 ms past the ask; "Stop." commits at its words; off, the first word commits as before |
| `with_backchannel_holds_a_yield_that_ends_before_its_words_resumes_the_reply` | With the switch: a yield that ends before its words came is recorded as unheard and resumed, with no commit, no turn and nothing left yielding; with `heard` too |
| `with_backchannel_holds_words_after_a_wordless_resume_start_nothing` | With `heard-always` and the switch: words that come 800 ms after the ask, once the hold has resumed the reply, are not the yield's: no commit, no turn, nothing yielding |
| `with_backchannel_holds_a_yields_real_words_still_commit_a_turn` | With `heard-always` and the switch: "Can you turn the light down?" commits as a turn on both rules |
| `with_backchannel_holds_and_heard_always_a_request_still_cuts_in` | With `heard-always` and the switch: "Lamp, wait.", "What was that?", "Yeah, but why?" and "Okay tell me more" commit on both rules |
| `with_backchannel_holds_two_backchannels_leave_the_reply_yielding` | With the switch: two backchannels the story goes on after cost no fruitless yield, and a third line 5 s after yields and commits as a sentence |
| `without_echo_evidence_duplex_decides_as_the_conservative_baseline_does` | Same masked blocks, candidates and rejections as `v1-baseline` |
| `a_microphone_that_stops_arriving_does_not_hold_the_reply_back` | Dispatch resumes after 100 ms without a block |
| `a_person_who_speaks_once_the_lamp_is_silent_is_an_ordinary_turn` | Six blocks and admitted, as ever |
| `first_words_older_than_the_provider_takes_are_dropped_before_the_commit` | Paced on the scenario clock, 40 ms a block: first words 2.5 s before the commit are trimmed to 2.0 s; the onset and what followed are kept, in one run; the drop is recorded |
| `with_first_words_kept_a_yields_request_begins_at_the_words_said_under_the_lamp` | With `LAMP_FIRST_WORDS=kept`, the request begins 20 blocks before the first words said under the lamp's voice, through the same yield and commit; without it, 30 blocks before the onset |
| `with_the_reply_end_reach_a_line_begun_under_the_lamps_last_words_keeps_its_name` | With `LAMP_REPLY_END_REACH=on`, a name said under the lamp's last sound, with no yield, begins its line 20 blocks before its onset; without the switch, 300 ms before the trigger after the lamp's sound; the lamp's last words scored 0.6 aren't reached back to |
| `with_the_reply_end_reach_the_lamps_words_before_a_pause_are_not_taken` | The lamp's last words scored 0.9, then a 610 ms pause with a 600 ms end-of-speech wait, then the name: the line begins at the name, not in them |
| `with_the_reply_end_reach_a_line_long_after_the_lamps_sound_starts_as_before` | A line begun 640 ms after the lamp's sound starts as without the switch, though words said under that sound are in the pre-roll |
| `a_start_reaches_past_the_reply_only_as_the_lamps_sound_ends` | Only with the switch, echo evidence and the lamp's sound quiet under 64 blocks |

## Verification

On `bc2f1a7df`, each from a source-only export of that commit.

| Where | Command | Result |
|---|---|---|
| Host, x86_64 macOS | `cargo fmt --all -- --check` | clean |
| Host | `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | clean |
| Host | `cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=1` | 1,374 passed, 0 failed, 6 ignored |
| Host | `cargo build --locked --offline --release -p lamp-voice -p lamp-voice-eval` | built |
| Emulated Linux/ARM64 | `cargo clippy --locked --offline -p lamp-audio -p lamp-voice --all-targets --target aarch64-unknown-linux-gnu -- -D warnings` | clean |
| Emulated | `cargo test ... --release -p lamp-audio --lib -- drift duplex evidence --test-threads=1` | 21 passed |
| Emulated | `cargo test ... --release -p lamp-voice --lib -- reference turn_taking admission options coordinator::tests::duplex --test-threads=1` | 100 passed, 1 ignored |
| Emulated | `cargo build ... --release -p lamp-voice --bins --examples`, then `file` | `lamp-voice`, `reference_pace` and `duplex_lab` are aarch64 ELF |

The four-hour test ran on the host in a release build at `b09f69767`;
`reference.rs` and `drift.rs` are the same in `bc2f1a7df`.

The host does not compile `audio_workers.rs` or the worker branch of
`main.rs`: they are Linux-only, and only the emulated build covers them.
The workspace suite ran on the host only. Emulation here is user-mode
emulation of test binaries on an x86_64 machine: a build and a functional
run, not device timing.

## Integration

One commit holds every change to files that are not mine,
`6f30dadfc`: `coordinator.rs`, `admission.rs`, `options.rs`, `wire.rs`,
`main.rs`, the new `coordinator/tests/duplex.rs` and one line of
`coordinator/tests/provider_terminal.rs`. On Linux it builds only together
with `feb560767`, which gives the capture worker the entry point `main.rs`
calls and the field the wire gained. The tested head is `bc2f1a7df`, on
Main's `6d81ab49f`; none of the files in the proposal changed there since
it was written.

| File | Change |
|---|---|
| `options.rs` | `TurnTaking::Duplex`, `ReferenceClock`, `--reference-clock` for the session and for the capture worker only |
| `wire.rs` | `MicrophoneFrame.echo` (optional, defaulted), `ReferenceTiming.drift` (optional, omitted when absent) |
| `admission.rs` | `Hold` in place of three constants; `InputAdmission::with_hold`; `AcceptanceBasis::DirectedSessionDuplex`. `default()` is the standard hold |
| `coordinator.rs` | Hears each block before admission and gives the detector the gated score; dispatch returns while yielded; the hold and queue bound by mode; `duplex_policy`, `duplex_yield`, `duplex_commit`, `duplex_resume`, `duplex_summary` events |
| `main.rs` | Usage text; the capture worker is started with its options |

Decisions that are Main's: whether a 700 ms hold and a 114-entry queue are
acceptable in this mode; whether `duplex` should imply
`--reference-clock capture`; and every default.

Agent 2's `activity.rs` and endpoint code are untouched. The seam with
them is one number per block: the score the detector is given.

## Not established

- **Anything on a lamp.** No device run of either flag. Emulated ARM64 is a
  build and a test run, not device time, and nothing here is acoustic.
- **A person.** Every positive is a loudspeaker prompt mixed in software.
  A voice in the room may differ in level, spectrum and onset. The team
  reports that speech replayed through a loudspeaker sounds muffled on this
  microphone board where a direct voice is clear, so replayed prompts may
  understate or misstate what a person does.
- **The microphone board's own processing during overlap.** Mixing in
  software assumes the path from room to samples treats a person and the
  lamp's echo independently. The board is reported to do its own echo
  filtering, beamforming and noise control. If it does, real overlap may
  reach the canceller differently from the sum of two recordings, in
  either direction. Only a recording of a person over a lamp can say.
- **Noise suppression on.** The recordings were made with software noise
  suppression off, and the one comparison above is a replay of development
  material. Whether it helps or compounds on a lamp is not measured.
- **Where the 55 ms of delay growth comes from**, and whether the
  canceller's re-alignment during it is what produces the leaks.
- **Who is being addressed.** Not attempted.
- **The acknowledgment pause as a person experiences it.** Half a second
  of silence is the cost in samples. Whether it reads as attentive or as a
  stutter is for a room, not a replay.
- **A long session on a device with the capture clock.** Virtual hours and
  a paced process check are software. USB clocks also wander with
  temperature; the controller follows a change of 200 ppm over a minute in
  test, and no device has been asked to.
