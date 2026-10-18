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

    lamp-name --models ABSOLUTE_DIR [--microphone mic1|mic2] [--least SCORE] [--quiet-below DBFS] [--bus-root ABSOLUTE_DIR]
    lamp-name check MODELS_DIR [--quiet-below DBFS] FILE...

| Option | What it is | If left out |
|---|---|---|
| `--models` | A directory holding `melspectrogram.onnx`, `embedding_model.onnx` and `name.onnx` | required |
| `--microphone` | Whose sound to listen to: the topic `mic1/audio` or `mic2/audio` | `mic2` |
| `--least` | The score from which the name counts as heard, from 0.5 to 1 | 0.9 |
| `--quiet-below` | A level in dBFS, from -90 to -30: the models rest while the last 2.03 s held no sample this loud ("Resting while the room is quiet") | off: the models always run |
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

A name that is said raises the score for several steps in a row. That is
one hearing (`src/said.rs`): it is said once, when the score falls again
or after 0.8 s, with the highest score it reached and how long ago it
began. For a second after it, scores are passed over, so the same call
is not heard twice. The score said is never over 1, whatever the models
gave (one over 1 fails the message's own check and would not be sent).

After sound was lost, or the microphone was closed for more than half a
second, the models start afresh: they need about two seconds of sound
before they give a score again. A hearing that was open at that moment is
said then, dated from its first high score (it was dated 0.8 s earlier
than that until 2026-10-14, which left a call 0.7 s, not 1.5 s, to count
for the request after it).

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
  `tests/quiet_models.rs` (ignored; it needs `LAMP_NAME_MODELS` and
  `LAMP_NAME_SOUND`) gave the same score on every step worked out, to the
  bit, on a laptop on 18 Oct: 70 of 770 steps worked out on 30 s of hiss,
  a 3.6 s line and 30 s more; 578 of 639 on four speech files between
  quiets of 1 to 4 s.
- `lamp-name check MODELS_DIR --quiet-below DBFS FILE...` rests as the
  program does, and says how many steps were worked out.

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
  and the report (four), resting (six), and two from the review of 14
  October. The models are not in the repository, so no test here runs
  them unless they are given (`tests/quiet_models.rs`, ignored);
  `lamp-name check` and the measurement above are what exercises them.
- **The classifier's training** is outside the repository and was one
  afternoon's work on generated speech. A version trained with similar
  names in its speech did worse on the lamp's microphone, not better.
