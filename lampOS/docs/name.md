# Hearing its name

`lamp-name` (crate `lamp-name`, `crates/name`) listens to what the lamp's
microphone hears for the lamp's name, "Hey Lamp" or "Lamp ơi", and says on
the bus when it heard it. It listens for nothing else and decides nothing.

## What it is for

The lamp answers when it is spoken to. The plainest sign of that is its
name, and the lamp could not hear it: the speech service writes down what
was said, and on lamp-4ace it wrote "Lamp" not once in 310 lines of one
day. It wrote "Milan", "Hey man", "Hey Mom", "Amen", or left the name out
([attention.md](attention.md)). So the text can show that the lamp was
called and can never show that it was not.

This part hears the name as a sound, on the lamp, before any text exists.
What it says is one sign among several for the part that decides whom the
lamp answers ([policy.md](policy.md)). It is not a gate: on the first
measurement it missed two or three calls in twenty and was set off by half
of the lines addressed to a similar name.

## The process

    lamp-name --models ABSOLUTE_DIR [--microphone mic1|mic2] [--least SCORE] [--quiet-below DBFS] [--step-ms 80|40|20|10] [--stop-least SCORE] [--not-while-speaking on|off] [--rescue-model ABSOLUTE_FILE --rescue-sha256 HEX [--rescue-gate SCORE]] [--bus-root ABSOLUTE_DIR]
    lamp-name check MODELS_DIR [--quiet-below DBFS] [--step-ms 80|40|20|10] [--stop-least SCORE] [--rescue-model FILE --rescue-sha256 HEX [--rescue-gate SCORE]] FILE...

| Option | What it is | If left out |
|---|---|---|
| `--models` | A directory holding `melspectrogram.onnx`, `embedding_model.onnx` and `name.onnx` | required |
| `--microphone` | Whose sound to listen to: the topic `mic1/audio` or `mic2/audio` | `mic2` |
| `--least` | The score from which the name counts as heard, from 0.5 to 1 | 0.9 |
| `--not-while-speaking` | `on`: a hearing that began while `voice/state` said `speaking`, or within 0.6 s after, is not said ("Not its own voice") | off: every hearing is said |
| `--quiet-below` | A level in dBFS, from -90 to -30: the models rest while the last 2.03 s held no sample this loud ("Resting while the room is quiet") | off: the models always run |
| `--step-ms` | 80, 40, 20 or 10: a step's score is the highest of 80 / `--step-ms` grids of steps, each this much later than the one before, so a name that falls between two steps of 80 ms is scored where it lies ("Where the steps fall", #694). The models' work grows about as many times | 80: one grid, as always |
| `--wake` | With `--step-ms` under 80, a score from 0.01 to 0.5: the later grids run their models only while the first grid's name score is over it, and briefly after, scoring the steps just before as they wake, so a name they hear best comes up to 0.4 s late ("Woken grids", #694). Refused at 80 | off: every grid always runs |
| `--rescue-model` | A second name model of `name.onnx`'s shape, outside the models directory: where `name.onnx` reached `--rescue-gate` at a step or in the 1 s before it, that step's score is the higher of the two ("A second model to rescue a call", #1546). Refused unless its SHA-256 is `--rescue-sha256` | off: no second model |
| `--rescue-sha256` | The rescue model's SHA-256, 64 lower-case hex digits; required with `--rescue-model` | — |
| `--rescue-gate` | A score from 0.05 to 0.95 | 0.5 |
| `--stop-least` | A score from 0.5 to 1: also run `stop.onnx` (beside the others, its SHA-256 checked) on the same embeddings, and publish `stop/heard` at the step its score first reaches this, then nothing new for 1 s ([stop-word.md](stop-word.md), #1555). In `check`, a `FILE stop SECONDS SCORE` line per hearing | off: `stop.onnx` is not loaded |
| `--near` | A score from 0.05 to under `--least`: also say each hearing that reaches this lower bar on `name/near`, when it is over, with its highest score; `name/heard` and `name/rising` are as without it. `LAMP_NAME_NEAR=on` in its environment, with no `--near`, is `--near 0.5` (the voice's second route reads it beside a line's first word, [second-voice.md](second-voice.md)) | off |
| `--bus-root` | The bus's directory | `LAMP_BUS_ROOT`, or `/run/lampos-bus` |

It is its own process: with it gone the lamp talks on as before, and
nothing waits for it. A wrong option prints the usage and exits with 2.
Models that cannot be loaded, or that do not give a score for silence,
end it with 1 before it listens to anything.

| Topic | Type | |
|---|---|---|
| `mic2/audio` (or `mic1/audio`) | `lamp_msgs/Audio` | Reads: what the microphone hears, as the voice publishes it |
| `name/rising` | `lamp_msgs/NameHeard` | Publishes: each hearing's first sign, as soon as the score reaches its bar, with the score so far (#782) |
| `name/heard` | `lamp_msgs/NameHeard` | Publishes: a score and how long ago the name began, once for each hearing, when it is over |
| `name/near` | `lamp_msgs/NameHeard` | Publishes, with `--near`: each hearing that reached the lower bar, when it is over, its highest score and how long before it reached that bar; its window is from then to the message's time. Never a call alone |
| `diagnostics` | `lamp_msgs/Diagnostic` | Publishes: how it is, about once a second, as `name` |

**Said twice (#782).** A hearing is said on `name/heard` when the score
falls again, with the highest score it reached: 0.40 to 0.56 s after the
score reached the bar for "Hey Lamp," on lamp-4ace on 15 October. That was
0.08 to 0.19 s after a 600 ms end-of-speech window had already ended the
call. So each hearing is also first signed on `name/rising`, as soon as
the score reaches the bar, with the score so far. What only needs to know
the name is being said (a call's wait, `--call-wait-ms` in the voice) reads
`name/rising`; what weighs how sure the hearing was (the voice's rule for
whom it answers, its 0.999 and 0.95) keeps reading `name/heard`. A first
sign with no final word was a near miss. `Hearings::rising` in `said.rs`;
its test is `a_hearing_is_first_signed_as_it_begins_and_said_when_it_falls`.

**Under its bars (#3923).** A line that starts with "Lamp" is sometimes
heard as "Ma'am, …" or "Blake, …", and its run of scores reaches neither
bar. Whatever the switches are, a run of scores from 0.05
(`UNDER_BARS_LEAST`) whose best stayed under `--near` (under `--least`
without it) is one journal line when it is over, never published:
`under its bars: best=0.310 began_ms_ago=240`, beside `near its name:
score=… began_ms_ago=…` for a run that reached the near bar. So a missed
call shows whether it scored about 0.3 or about 0. Runs are held off for
1 s after each, as hearings are, which in a room that talks all day is
still a line about every 1.1 s: so at most 10 are written a minute
(`UNDER_BARS_A_MINUTE`), and the next line written after some were not
ends `not_written_before=N`. `UnderBars` in `main.rs`; its tests are
`a_run_under_both_bars_is_one_line_and_one_at_the_near_bar_is_none` and
`under_its_bars_is_ten_lines_a_minute_at_most`.

**It keeps no sound.** Samples are held for about a fifth of a second,
until they have been turned into the numbers its models work on, and of
those it holds the last two seconds' worth. With `--quiet-below` it also
holds the newest 2.11 s of samples in its memory, to start the models on
when a sound comes. Nothing is written down and there is no option to.
Its log holds counts, times, scores and levels.

## Where the sound comes from

The voice owns the microphone. With `LAMP_BUS_AUDIO=on` in its unit, as
well as `LAMP_BUS=on`, it publishes what it hears on `mic2/audio`
(`crates/voice/src/bus_out.rs`, `MicOut`): the sound after its echo
canceller, 16 kHz, one channel, 20 ms to a message, each with a count
that shows when a message was lost. With the microphone closed (the
privacy switch, or a voice that is not permitted to listen) there is no
sound to publish and none is.

That switch is the voice's own and is off unless someone sets it. It is
the one topic that carries sound: with it on, any program that runs in
the lamp's group can hear the room. The always-on voice writes no sound
down (it refuses its recording option), and this does not change that: a
topic is not a file, and nothing on the lamp records it. But it is a door
that was not there, and it is why it has a switch of its own.

## How it hears

Three small models, run every 80 ms (`src/hear.rs`). The design is
openWakeWord's; the last model is ours.

| Model | Takes | Gives |
|---|---|---|
| `melspectrogram.onnx` | The newest 0.11 s of sound | Eight frames of 32 numbers, one for every 10 ms |
| `embedding_model.onnx`, Google's speech embedding | 76 frames, 0.76 s | 96 numbers that say what kind of speech sound that was |
| `name.onnx`, trained by us | The newest sixteen of those, 1.28 s | One score: how likely it is that the name was just said |

The first two are fixed and know nothing of lamps. The third is a small
classifier trained on generated speech of people saying "Hey Lamp" and
"Lamp ơi", and of people saying other things. The files are not in the
repository; they are installed beside the program, and
`lamp-name check` runs them over a file of sound where they are.

### Where the steps fall (`--step-ms`, #694)

The second model takes 76 frames at a time, a new one every 8 frames
(80 ms), so where those steps fall on a word moves its score, and by a lot
for a call near the bar. On lamp-75c9's own microphone (21 Oct, a laptop
playing "Hey Lamp, my name is Alex." in Piper voices, the lamp's
`mic2/audio` recorded), each line moved by 0, 10, … 70 ms scored, at 80 ms:
one 0.82 0.04 0.10 0.09 **0.96** 0.28 0.14 0.09; another 0.85 0.63 **0.97**
0.92 0.96 0.56 0.70 0.24. A strong call is above 0.97 wherever it falls.
That is what lost lamp-4ace's 03:11 call, not the 10–90 ms of the first
word the microphone path clips (#694).

With `--step-ms`, `80 / STEP_MS` grids run on the same sound, each
`--step-ms` later than the one before (`hear::Grids`). A step's score is the
highest of the first grid's for it and each later grid's newest, which is
of the step before and whole before the first grid's. So there is still
one score every 80 ms, and the hearing, the rescue's gate and the minute
line count steps as before; the first step after a start has the first
grid's alone. Frames are 10 ms, so only `--step-ms 10` tries every place a
word can fall; 20 tries every other one.

The 15 lines, each at the 8 places (hits at 0.9, of 40 a condition):

| `--step-ms` | after a quiet minute | 1 s after another line | after the lamp's reply | `check` on a laptop, ms a step |
|---|---|---|---|---|
| 80 | 27 | 15 | 38 | 4.6 |
| 40 | 30 | 20 | 40 | 6.6 |
| 20 | 32 | 24 | 40 | 13.4 |
| 10 | 32 | 32 | 40 | 22.9 |

At 10 the score no longer depends on where the word falls; the 8 misses
left in each of the first two are one voice the model scores 0.65–0.80
wherever it lies, whose clean file scores 0.84: its reach, not the step.
Off (80) until its cost is measured on a lamp beside a voice turn.

**Woken grids (`--wake`, #694).** On lamp-75c9, beside a voice turn, `--step-ms
10` took 76% of a core against 9% at 80. Almost all of a step is the second
model: on a laptop, 2.14 ms of it, against 63 µs for the first and 11 µs for
the name's. So a shared first model saves nothing. With `--wake SCORE` the
later grids make only their frames until the first grid's name score goes
over SCORE. Then they make the embeddings they passed over, from the frames
kept for them (the newest 20, `hear::KEPT`), score their last five
positions (`hear::BACK`), and stay awake for 8 steps after the score was
last over the bar (`hear::AWAKE_STEPS`). A score given late goes into the
next step (`Grids::first`), so a name a later grid hears best is heard up
to 0.4 s after it was said, plus the time the catch-up takes, within the two
bounds below; past them it is let go.

The later grids run on a thread of their own (`hear::Woken`), so the first
grid's steps, and its own hearing, never wait for them: a wake's catch-up
took 0.95 to 1.16 s of one core on lamp-75c9. Done on lamp-name's own loop,
as #2759 first did, it held back the first grid. Three of four played calls
that 80 ms alone heard were lost (22 Oct, 07:36–08:31: 11.9% of a core at
80, 79.9% at 10, 15.3% woken; 11 wakes in 18 minutes with nobody saying the
name). `check` and the tests wait for the thread at each block
(`Hearer::waiting`), so a file scores the same every time.

Two bounds keep the thread honest (reviewer-two on #2759):
- **At most a second and a half waits for it** (`hear::MOST_QUEUED`, 75
  blocks): a wake's catch-up takes about a second on lamp-75c9, and at half
  a second 55 overruns came of 35 wakes in 18 minutes (22 Oct 10:3x).
  If it falls further behind, what waits is let go, and the later grids
  start afresh at the first grid's next whole step, so each keeps its place.
  A forget goes ahead of anything waiting. The minute line counts these as
  `overruns=`.
- **A late score never hears a call twice.** A later grid's score that comes
  late, for a step where the first grid heard a call (`--least` or more
  within six steps of it), leaves the name and the rescue out; a stop word
  the first grid heard (`--stop-least`) leaves the stop out. One for a call
  the first grid missed is heard, once, if it comes while the first grid's
  steps around it are still kept: 48 steps (`hear::RECENT`), so up to about
  3.4 s late. Older, or of work let go by an overrun, it is let go whole,
  even a call only it heard. Without this, one played call's fine score, a
  second late and past the hold-off, opened a second hearing.

`check` and the real-model test wait for the thread, so they show what
the grids hear, not how far behind the thread falls on a lamp. That is the
lamp run's to measure (`overruns=`, `longest_block_ms`).

Even at its worst place, each of the 15 lines above took the first grid's
score over 0.04. In 30 s of Piper talk with no name, it never went over
0.01, and in a held synthesised chord never over 0.02. So `--wake 0.03`
wakes on every line wherever it falls, and not on that talk. On the 120
(lines × places), woken at 0.03 scored each as highly as always awake (104
of 120 at 0.9, against 80 at 80 ms). The real models' test is
`step_models::woken_grids_hear_what_the_grids_always_awake_hear`. With
three positions back, warm1 moved 10 ms lost its 0.96, which was four back.

Its cost, `check`'s whole run on a laptop:

| sound | 80 ms | 10 ms woken at 0.03 | 10 ms always awake |
|---|---|---|---|
| 90 s of talk, no name | 2.44 s | 2.90 s | 19.05 s |
| 60 s near silence | 1.65 s | 1.95 s | 12.58 s |
| the 120 recorded calls | 25.1 s | 82.1 s | 139.4 s |

So about 1.2 times 80's work while no name is near, and 10's while one is.
A real room wakes it more often than clean files do: 4ace's office heard
the name at 0.9 or more 227 times in a day. Off until its CPU on a lamp is
measured.
With `--wake`, the minute line also says `wakes=` (times the later grids
woke that minute) and `woken_steps=` (the steps they were awake for), so a
room's cost is read from the journal.

A name that is said raises the score for several steps in a row. That is
one hearing (`src/said.rs`): it is said once, when the score falls again
or after 0.8 s, with the highest score it reached and how long ago it
began. For a second after it, scores are passed over, so the same call
is not heard twice. The score said is never over 1, whatever the models
gave (one over 1 fails the message's own check and would not be sent).

After sound was lost, or the microphone was closed for more than half a
second, the models start afresh. Both are read from the blocks themselves:
a skipped count, or half a second or more between two by the voice's own
monotonic clock (the bus message's `at_us`), or a new publisher (the voice
restarted). Not by when lamp-name got to them, since a woken grid's
catch-up can keep blocks waiting, and not by a wall clock, which a lamp
with no battery clock has set 30 to 70 s after boot (#2759). Then the models start afresh: they need about two seconds of sound
before they give a score again. A hearing that was open at that moment is
said then, dated from its first high score (it was dated 0.8 s earlier
than that until 2026-10-14, which left a call 0.7 s, not 1.5 s, to count
for the request after it).

## A second model to rescue a call

The installed `name.onnx` almost never hears the name where it was not
said, but through lamp-4ace's speaker and canceller it hears few bare
"Lamp," calls. A model trained on the lamp's own recordings
(`training/name/lamp.py`, [bare-name.md](bare-name.md)) hears far more
calls, and far more that were not. With `--rescue-model` both run on the
same sixteen embeddings (`src/rescue.rs`): a step's score is
`name.onnx`'s, unless `name.onnx` reached the gate at that step or in the
twelve before it (1 s), and then it is the higher of the two. So
`name.onnx` says where a call may be, and the second model only how sure
it is there. Everything after (the bar, a hearing, `name/rising`) judges
that score as it judged `name.onnx`'s.

Lost sound and a gap close the gate: the steps are counted again from
them, and no score before them is within a second of one after. A rest
(`--quiet-below`) counts as the time it lasts, since its steps are counted
though not worked out: one of 1 s or more closes the gate, and after a
shorter one a gate reached just before it still opens the first steps
after it, as on the same sound with no rest, where those steps are quiet.
The file is given with its SHA-256, and another file under its name is
refused at the start; the model is built from the bytes that were hashed,
not read again, and is checked by a score from 0 to 1 for silence.

On the second sitting's recordings (18 Oct, 99 played calls, 18.9 min of
held-out negatives: the lamp's answers, the room, near and mid lines, a
quiet room), with the gate at 0.5: 51 calls heard at 0.98 against
`name.onnx`'s 35, and no false hearing at 0.98 for either. At 0.3, 54
calls and 3.2 false an hour; at 0.1, 66 and 6.3. The trained model alone
heard 82, with 104.6 false an hour. One sitting of played voices: a sign,
not proof. The trained model is the office's and stays on lamp-4ace; none
is published.

The journal says the rescue at the start (`rescue=PATH@GATE:SHA12…`),
`rescued=yes` on a hearing one of whose steps reached the bar only by the
second model, and `rescued_steps` in the minute line. `lamp-name check`
takes the same three options. On a laptop (i7-13650HX, `check` over two
recordings, five runs each) a step took a median 2.07 ms with the second
model against 2.04 without: about 30 µs. On lamp-4ace it is measured as
`stop.onnx` was (#1555).

## Resting while the room is quiet

Run every 80 ms all day, the models cost lamp-4ace 750 steps of 9.3 ms a
minute: about 12% of one core with nobody speaking, and the board warmed
to 75 °C overnight, past the 70 at which it slows itself (#705). With
`--quiet-below DBFS` they rest while the room is quiet (`src/quiet.rs`).

- **What it rests on.** A score is of the newest 2.03 s of sound and of
  nothing before them (25 chunks for the first model, whose frames make
  the sixteen embeddings the last one takes: `hear::WINDOW`). A step
  whose 2.03 s held no 20 ms block with a sample at the bar or above is
  of a quiet room, and is not worked out. It is counted as `rested`.
- **The same scores.** The newest 2.11 s are kept. At the first block at
  the bar, the models start afresh on them, from the step where the
  first score whose 2.03 s hold that block begins. Steps stay counted
  from the last start, gap or lost sound, so that score and every one
  after it is the number the models would have given had they never
  rested. They rest again with the block that ends the last window to
  hold the sound, and a hearing still open then is said.
- **The cost.** The start at each sound after a rest: at most 25 steps of
  the first model and 16 of the second, before the next live step. On a
  laptop that block took 32 ms against a step's 2.1 ms on average (the
  longest block with no rest, 2.5 to 2.9 ms), about fifteen steps' work, so about 0.14 s at lamp-4ace's 9.3 ms a step, while a
  score reaches the bar some 0.3 s or more into "Hey Lamp". Blocks that
  come meanwhile wait in the bus's socket: about 7 of them, against the
  queue `net.unix.max_dgram_qlen` allows; one dropped is lost sound, and
  the models start afresh. **The break-even:** each sound costs that
  start and 2.03 s of steps, so sounds over the bar less than about 3.3 s
  apart (a ticking clock, keys, a drip) cost more than the switch off:
  in a stand-in probe (Jarvis on #1440), one 20 ms sound every 2.1 s cost
  1.71 times the work, every 3 s 1.20, every 4 s 0.91, every 10 s 0.36.
  The bar is for a room whose quiet is quiet.
- **Read on the lamp.** The minute line's `longest_block_ms` is the
  longest one block took: with the switch on, the start after a rest, on
  the lamp's own processor, to read beside `sound_lost`.
- **Setting the bar.** The minute line says each 80 ms step's loudest
  sample as `loudest_db_p50` and `loudest_db_p99`, with the switch on or
  off. The bar goes at least 10 dB under the quietest call's loudest
  20 ms, and over the empty room's level, both read on the lamp first.
- **Tested** by `quiet.rs`'s tests, on a stand-in for the models whose
  score is a checksum of exactly its own 2.03 s: every score after a
  rest is the one with no rest
  (`a_score_after_a_rest_is_the_score_without_one`); exactly the steps
  whose 2.03 s held a block of sound are worked out
  (`exactly_the_steps_whose_window_held_sound_are_worked_out`); the
  models say when they rest (`the_models_say_when_they_rest`). Starting
  one sample off the steps, one step late, or resting a step late each
  fails one of them. With the real models,
  `tests/it/quiet_models.rs` (ignored; it needs `LAMP_NAME_MODELS` and
  `LAMP_NAME_SOUND`) gave the same score on every step worked out, to the
  bit, on a laptop on 18 Oct: 70 of 770 steps worked out on 30 s of hiss,
  a 3.6 s line and 30 s more; 578 of 639 on four speech files between
  quiets of 1 to 4 s.
- `lamp-name check MODELS_DIR --quiet-below DBFS FILE...` rests as the
  program does, and says how many steps were worked out.

## Not its own voice

The lamp's voice reaches its own microphone through the canceller, and its
name in the lamp's own words can be heard: on lamp-4ace on 18 Oct (11:52)
the installed model heard the lamp's "There you go, I've made the light
warmer for you" at 0.990. Most readers of `name/heard` and `name/rising`
take any hearing (#705, step 4: the voice marks the request in hand as
called, tells the model so and starts its call's wait; only the body
looks at whether the lamp speaks, and as the message arrives). With
`--not-while-speaking on` (`src/not_self.rs`), `lamp-name` reads
`voice/state` and passes over a hearing that began while it said
`speaking`, or within 0.6 s after (a score reaches the bar a little after
its sound, and the room rings on). Its first sign and its final word are
judged once, together. A voice unheard for 2.5 s, or gone, is not speaking.
Each one passed over is a journal line ("passed over a hearing that began
while the lamp spoke", with its score), no sound, and the minute line counts
them (`passed_over_in_all`). Any hearing that began within 3 s after the
lamp stopped speaking, passed over or said, gives `after_speech_ms` (0
inside a reply), so a lamp's own hearings say how far the 0.6 s tail has to
reach: the tail is not measured yet. It counts from the first score at the
bar, not from the sound, so a quick bare "Lamp" begun about 0.1 s after a
reply can still be passed over. A hearing said at its first sign is said at
its final word too, and one passed over is passed over at both, even when
the voice is heard again between them.

**What it costs:** a person who calls the lamp by name while it speaks, or
within 0.6 s after, is not heard as calling it. Their words still reach
the voice as a cut-in does (the duplex yield and the stop word do not read
the name), without the name's sign: they lose the policy hold's override,
the model's "you were called" and the body's look-up at the name. The lamp
check plays a person's "Lamp, …" over a reply with the switch off and on:
the lamp must still stop, answer and turn to them.

**Tested** by `not_self.rs`: a hearing inside a reply is passed over at
both its signs, and one just after the reply and its tail goes out
(`a_hearing_inside_a_reply_is_passed_over_and_one_after_it_is_not`); a
reply still going covers, a silent or gone voice does not. With no tail,
no pairing of the two signs, or no end to an unheard reply, a test fails.
Not shown: the lamp. The check is the same played scene as 18 Oct's
(a "Lamp" in the lamp's own reply) with the switch off and on.

## How it says it is

| Level | Message | When |
|---|---|---|
| `ok` | | It is listening, or the microphone is closed and there is nothing to listen to |
| `warn` | `no sound reaches it: the voice is not putting the microphone's sound on the bus` | Nobody publishes the topic it listens to |
| `error` | `its models stopped working: ...` | A model failed on sound it was given. It listens no more until it is started again |

With it: whether sound is coming, how many times it has heard the name,
how long a step takes it, and how much sound was lost on the way. With
`--quiet-below`, a minute of rest reports `ms_a_step` 0.0 (no step was
worked out), and the start after a rest counts in the next steps'
`ms_a_step`: read it beside the minute line's `rested`.

## Measured

On what lamp-4ace's own microphone recorded on 2026-10-14
([attention.md](attention.md), "Hearing its name in its own sound"): six
recordings of 86 s, 44 lines played from a loudspeaker in eleven
synthetic voices with nobody in the office. This program was run over
each whole recording from start to end, 20 ms at a time, as it would
hear it live, on a development Mac. A call counts as heard if a hearing
began between 0.3 s before the line was started and 1 s after it ended.

| Score from which it counts | "Hey Lamp" (20) | A similar name (10) | Office talk (10) | "Lamp ơi" (2) | Heard with nothing being played |
|---|---|---|---|---|---|
| 0.9, before the echo canceller | 18 | 5 | 0 | 2 | never, in 520 s |
| 0.9, after it, which is what it is given on the lamp | 17 | 6 | 0 | 2 | never |
| 0.999, before | 11 | 3 | 0 | 0 | never |
| 0.999, after | 9 | 2 | 0 | 0 | never |

The 520 s include the lamp's own answers to the lines and the quiet
between them. What sets it off is a line that begins with a name like
its own: "Hey Lan", "Hey Liam", "Hey Lance", "Hey Mom", "Hey man". A
step took 2.3 ms on the Mac.

## On the lamp

On lamp-4ace since 04:12 on 2026-10-14, as `lamp-name.service` under the
lamp's ordinary user, kept to the board's two fast processors and a
little below ordinary priority, with `LAMP_BUS_AUDIO=on` in the voice's
unit. The three model files are in `/opt/lampOS/name/models`.

- **It starts in under half a second**: the models were loaded and run
  once on silence in 366 ms.
- **The sound arrives whole.** `mic2/audio` came 49.96 times a second,
  20.0 ms apart on average and 22.7 at most, 0.2 ms old on arrival, none
  missed (over 4 s, with `lamp-bus hz`). Over five minutes the part
  itself counted 750 steps a minute and no sound lost.
- **A step costs it 9.2 to 9.8 ms** for each 80 ms of sound: about an
  eighth of one fast processor, all the time the microphone is open.
- **Live, it hears what it heard in the recordings.** The same 44 lines
  played to the running lamp, which answered them as it does:

| Score from which it counts | "Hey Lamp" (20) | A similar name (10) | Office talk (10) | "Lamp ơi" (2) | Heard with nothing being played |
|---|---|---|---|---|---|
| 0.9 | 17 | 5 | 0 | 1 | once, in about 5 minutes (0.988) |
| 0.999 | 8 | 2 | 0 | 0 | never |

  A hearing was said on the bus 78 to 639 ms after the name began.

With it the lamp's health has six parts reporting.

## Not shown

- **A bare "Lamp".** It was trained on "Hey Lamp" and "Lamp ơi". Of eight
  played lines that began "Lamp," and nothing before it, on 2026-10-14,
  it reported a hearing in none
  ([conversation-latency.md](conversation-latency.md)). One syllable is
  little to go on, and the study found the same of every spotter it tried.
  A model of the same shape that also hears a bare "Lamp" was trained and
  measured off the lamp, to be tried in place of `name.onnx`:
  [bare-name.md](bare-name.md).
- **Resting on the lamp.** The bar is not yet set: the room's level and
  the quietest call at mic2 are to be read on lamp-4ace with the switch
  off, and the saving and a call's `name/rising` time measured with it
  on, with `sound_lost` staying 0 across starts and lamp-4ace's
  `net.unix.max_dgram_qlen` written down.
- **A person.** Every voice was synthetic and played through a
  loudspeaker near the lamp. Real voices, accents, distance and a full
  office are untried, and are what the classifier has to be trained for.
- **Enough lines to tell 17 from 18.** Forty-four lines, four of them
  Vietnamese.
- **What its one stray hearing was.** No sound is kept, so what the
  lamp's microphone held at that moment cannot be gone back to. It was
  between two played lines, while or just after the lamp answered.
- **Its own tests are small**: the rule for a hearing (five), the options
  and the report (four), resting (six), not its own voice (five), and two
  from the review of 14 October. The models are not in the repository, so no test here runs
  them unless they are given (`tests/it/quiet_models.rs`, ignored);
  `lamp-name check` and the measurement above are what exercises them.
- **The classifier's training** is outside the repository and was one
  afternoon's work on generated speech. A version trained with similar
  names in its speech did worse on the lamp's microphone, not better.
