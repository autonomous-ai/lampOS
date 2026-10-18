# The name, heard as a bare "Lamp"

A name model for `lamp-name` that hears "Hey Lamp", "Lamp ơi" and a bare
"Lamp" at the start of a sentence ("Lamp, what time is it?"), and is made
not to be set off by "Lan", "Liam", "lamb", "land", "plan", "lab", "Sam" or
"ma'am" in the same place, by ordinary talk, or by the lamp's own voice. It
has the shape of the model `lamp-name` runs now and can replace it.

This is task 5 of [the builders' list](work-split.md); the gap it answers
is in [conversation-latency.md](conversation-latency.md) and
[name.md](name.md). It was trained and measured on a laptop (Apple M2
Max, macOS 26.6) and its graph run under tract 0.23.8 there and in a Linux
ARM64 container. Nothing here was run on a lamp, and no number here is the
lamp's.

## What exists and what we take

**The lamp's own name model.** `lamp-name` runs openWakeWord's mel and
embedding models and then `name.onnx`, "a small classifier trained on
generated speech of people saying 'Hey Lamp' and 'Lamp ơi', and of people
saying other things" ([name.md](name.md)). A hearing is a score of 0.9 or
more (`--least`, default 0.9) for one step or more, said when the score
falls or after 0.8 s, then 1 s passed over (`crates/name/src/said.rs`). The
file lives on the lamp in `/opt/lampOS/name/models/`; its training "is
outside the repository and was one afternoon's work on generated speech",
and a version trained with similar names "did worse on the lamp's
microphone" ([name.md](name.md), [attention.md](attention.md)). On the
lamp's own microphone it heard 17 of 20 played "Hey Lamp", 5 of 10 similar
names, and none of eight lines that began with a bare "Lamp,"
([conversation-latency.md](conversation-latency.md)).

- Taken: the same input and rule, so that the new file can replace
  `name.onnx` with no code change; the training and measuring of task 3
  ([stop-word.md](stop-word.md)), reused as described under "What was
  made".
- Changed: a bare "Lamp" is a call; near misses are in the training, said
  in the same place as the name; the microphone's lost onset and a
  loudspeaker's muffling are done to the speech.

**Short names are known to be hard.** Picovoice advises against wake
words of fewer than six phonemes, as harder to detect and more often
set off, and suggests putting "Hey" or "OK" before a short word
([Picovoice](https://picovoice.ai/docs/tips/choosing-a-wake-word/); seen in a
search summary, the page did not load). "Lamp" is four phonemes; "Hey
Lamp" seven. Apple let people say just "Siri" from iOS 17 (2023), at first
in English in four countries only
([iDownloadBlog](https://www.idownloadblog.com/2023/06/06/ios-17-siri-invocation/)),
and describes a low-power first detector followed by later checks
([Apple ML Research](https://machinelearning.apple.com/research/voice-trigger)).
Amazon's selectable wake words include "Computer" and "Echo"
([TechRadar](https://www.techradar.com/home/smart-home/how-to-change-alexa-wake-word)).
No published false-accept numbers by wake-word length were found.

- Taken: a second check is what makes a short name workable elsewhere;
  here the lamp already has one (the policy that weighs the name with
  where the person looks and what was said, [policy.md](policy.md)).

**Speech of Vietnamese speakers of English, and of "Lamp ơi".** None was
found that a shipped model can be trained on:

| Source | What it has | Why not for training |
|---|---|---|
| L2-ARCTIC | 4 Vietnamese speakers of English | CC BY-NC 4.0 ([site](https://psi.engr.tamu.edu/l2-arctic-corpus/)) |
| Speech Accent Archive | Many accents | BY-NC-SA 4.0 ([site](https://accent.gmu.edu/)) |
| Common Voice (accent labels) | Some Vietnamese accents | An account is needed since October 2025 |
| EdAcc ([HF](https://huggingface.co/datasets/edinburghcstr/edacc)) | 10 Vietnamese speakers of English in 5 conversations, about 148 min | CC BY-SA 4.0: whether share-alike reaches a trained model is unsettled, so it is used for testing only |
| Piper `vi_VN-vais1000-medium` | A Vietnamese voice | Fine-tuned from the Lessac voice (research-only data) |
| Piper `vi_VN-vivos-x_low` | 65 Vietnamese voices | VIVOS is CC BY-NC-SA 4.0 |
| Piper `vi_VN-25hours_single-low` | A Vietnamese voice | Its data's licence is "Unknown" |
| MMS-TTS Vietnamese | A Vietnamese voice | CC BY-NC 4.0 ([model card](https://huggingface.co/facebook/mms-tts-vie)) |

- Taken instead: the English LibriTTS voices given the phonemes espeak's
  Vietnamese voice reads from the English words ("Lamp" becomes /laːmp/,
  "the" /tɛ/, "down" /zɔwn/), for a third of all lines: a stand-in for a
  Vietnamese accent, not one. "Lamp ơi" is said by the same voices from
  phonemes: /læmp əːj/, /lɛmp əːj/ and /laːm əːj/. Vietnamese itself comes
  from Google FLEURS (read Vietnamese, CC BY 4.0), as talk the name must
  not hear.

**Sources used, and their licences.**

| Source | Used for | Licence |
|---|---|---|
| Piper `en_US-libritts-high`, 904 voices | Calls, near misses, the lamp-like replies (as in task 3) | Trained from scratch on LibriTTS, CC BY 4.0 |
| Piper `en_GB-cori-medium`, `en_US-ljspeech-medium` | Testing only: a British voice, and two more female voices for the lamp's own | Trained from scratch on public-domain recordings (LibriVox; LJ Speech) |
| MSWC English ([HF](https://huggingface.co/datasets/MLCommons/ml_spoken_words)) | "lamp" from 253 speakers; "hey", "lamb", "land", "plan", "lab", "Sam", "Liam", "Lan", "lam", "madam", rhymes | CC BY 4.0 |
| Google FLEURS Vietnamese ([HF](https://huggingface.co/datasets/google/fleurs)) | Vietnamese talk: 9.1 h to train against, 3.0 h to test | CC BY 4.0 |
| LibriSpeech, Speech Commands, MSWC other words, AMI | As in task 3 | CC BY 4.0 |
| EdAcc, Vietnamese speakers only | Testing only | CC BY-SA 4.0 |

## Answer

On the laptop, on voices it never heard, at the threshold `lamp-name`
already uses (a score of 0.9 for one step):

- **"Hey Lamp" and "Lamp ơi" are heard about as well as by a model
  without a bare "Lamp"**: 94 % and 92 % of generated calls read as
  English, 87 % and 76 % of those read with a Vietnamese accent.
- **A bare "Lamp," is heard, not always.** 86 % of generated calls read
  as English, 53 % read with a Vietnamese accent, 51 % of real people's
  "lamp" cut out of their sentences (MSWC, 51 clips). With the
  microphone's lost onset and a loudspeaker's muffling together: 72 %,
  42 % and 63 %. With someone else talking 15 dB under: 53 %, 33 %, 25 %.
  The same recipe without bare calls hears 1 % of them, as the lamp's
  model heard none of eight.
- **A voice from another generator is heard far less.** Cori, a British
  voice made by another Piper model and never trained on: "Hey Lamp" 67 %,
  "Lamp ơi" 5 %, a bare "Lamp" 16 %. The model leans on the one generator
  it learned from (LibriTTS voices), and real people's single word "lamp"
  is at 51 %. Real calls, recorded at the lamp, are what it needs next.
- **One syllable is little to go on, and it shows in what sets it off.**
  At the start of a line, "Lamb" sets it off in 30 % of generated clips,
  "Lan" and "Lâm" in 25 %, "lab" 11 to 21 %, "land" 9 to 17 %, rhymes
  ("Camp", "Lump") 3 to 14 %; "Sam", "Liam", "ma'am", "plan", "hey" and
  "lamp" inside a sentence 0 to 2 % (and one of six real "Liam").
  Without bare calls, "Lamb", "lab", "land" and the rhymes were 0 to 2 %:
  that is the price of the bare name.
- **Talk sets it off** 3.3 times an hour of LibriSpeech readers, 10 times an
  hour of AMI meetings, 23 times an hour of Vietnamese (FLEURS) and 36
  times an hour of Vietnamese speakers talking English (EdAcc). Vietnamese
  is the hard part, and it was before the bare name too (the comparison
  model: 17 and 48 an hour): "Lamp ơi" said the Vietnamese way is
  /laːm əːj/, and "làm" is one of the most common Vietnamese words.
- **The lamp saying its own name**: "Lamp here, what can I do for you?" in
  the lamp-like replies set it off 24 times in 64. Any hearing while the
  lamp itself is speaking must be passed over.
- **At 0.98** the bare name is heard in 59 %, 31 % and 18 % (English
  reading, Vietnamese reading, real word), "Lamb" falls to 5 %, and talk
  to 1.3 an hour of readers, 3.2 of meetings, 11 of Vietnamese and 16 of
  EdAcc.
- **The model**: `models/name/name.onnx`, 124,135 bytes, SHA-256
  `f376e964711a7460d248f8b48b2c0f07b03f73c5adf270a67536e68b04202030`. It
  has the shape of the current `name.onnx` and replaces it with no code
  change; run inside `hear.rs` under tract 0.23.8 it gives training's
  scores to within 6e-6 (Mac and Linux ARM64).
- **Proposed**: try it in place of the current file at 0.9, as a sign for
  the policy and not a gate, and first run `lamp-name check` with it over
  the lamp's own recordings of calls. The lamp's current file could not be
  found to measure beside it ("The lamp's current name.onnx").

## What was made

`lampOS/training/name/` holds what is the name's own; the rest is task 3's
code in `lampOS/training/stop/`, imported from there and not copied
(`shared.py` puts that folder on the path after this one):

| From training/stop, unchanged | From training/stop, with one addition | The name's own |
|---|---|---|
| `features.py` (the front end exactly as `hear.rs`), `align.py` (where a word is), `mix.py` (levels, mixing, a room), `hearing.py` (the rule of `said.rs`) | `store.py` (a set of clips may live in another folder), `train.py` (`--inputs` for another folder of inputs, `--leave-out-way` to leave out one way of calling), `make.sh` (`STOP_ONLY_CLIPS=1` stops after the shared sets) | `phrases.py`, `speak.py`, `microphone.py`, `clips.py`, `make_data.py`, `evaluate.py`, `tables.py`, `make.sh` |

The training environment is task 3's (`training/stop/pyproject.toml` and
its `uv.lock`). Task 3's results do not change: its additions are off by
default.

**What is said.** Each call is made by one generated voice in two pieces:
the name ("Hey Lamp", "Lamp", or "Lamp ơi" as /læmp əːj/, /lɛmp əːj/,
/laːm əːj/) with a full stop, question mark, exclamation mark or comma,
then, in three of four, one of 33 questions after a pause of 30 to 350 ms.
So where the name ends is known exactly. The near misses are made the
same way in the same place: "Lan", "Hey Lan", "Lan ơi", "Liam", "Lamb",
"Land", "Plan", "Lab", "Sam", "Ma'am", "Lâm", "Lâm ơi", other names ("Hey
man", "Hey Mom", "Nam", "Em ơi", "Lana"...), rhymes ("Camp", "Ramp",
"Lamps", "Lump"...), and "lamp" inside a sentence ("Turn on the lamp").
A third of every kind is read as espeak's Vietnamese voice reads English.

| Set | Train | Test |
|---|---|---|
| Generated calls, three ways | 12,000 by 791 voices, of which the 1,685 by 113 of them are kept for validation | 1,500 by 113 other voices, and 450 by a British voice (Cori) |
| Generated near misses, 12 kinds | 9,000 | 2,200, and 450 by Cori |
| "lamp", MSWC, real people, the word alone | 202 clips | 51 clips |
| Near misses, MSWC, real people | 6,714 clips | 1,279 clips |
| "lamp" inside a sentence, LibriSpeech | 17 utterances | 7 |
| Other words (Speech Commands, MSWC) | 4,542 and 9,515, as in task 3 | |
| Lamp-like replies, female voices | 1,500 | 2,150 (LibriTTS women, Cori, LJ Speech) |
| Talk read with a Vietnamese accent, generated | 2,400 lines, 3.4 h | 600 lines, 0.8 h |
| Vietnamese, FLEURS | 9.1 h | 3.0 h |
| English talk, LibriSpeech | 10.5 h dev and 25.6 h train-clean-100 | 10.7 h test |
| Vietnamese speakers of English, EdAcc | | 583 segments, 1.3 h |
| Ordinary talk, AMI meetings | | 34 meetings, 24.1 h |

**What the lamp's microphone does** (`microphone.py`), done at random to
the person's voice: the first 60 to 140 ms after each silence of 0.25 s
taken down by 20 to 40 dB (half the time), a small loudspeaker's band,
resonance and saturation (two in five), and a room (three in ten). These
are guesses from the task's description, not measured from the lamp.

**Examples, labels and training** are task 3's. An example is 2.59 s,
nine inputs 80 ms apart; an input is a call when the name ended 0.02 to
0.74 s before the end of what it sees. Every short clip, call or not, is
placed with its word ending at the same moment after quiet or other sound,
so that where a word sits says nothing of whether it is the name. Around
the person: only the microphone's floor (two in five), other talk 10 to 30
dB under (one in three), 0 to 10 dB under (one in ten), or noise. Long
talk is all negative: 13 h of LibriSpeech readers, generated voices and
replies, the accented talk and FLEURS, alone or two at once. The model is
task 3's "time" shape, 64 filters, 12,000 steps, negatives weighed 3 to 1:
89,649 call inputs, 272,781 inputs of other words, 565,500 of talk. The
trainer's default `--lowest -20` left out the 17 % of call inputs that
had other talk 20 to 30 dB under them, the nearly quiet ones; that was not
meant, and it is how this model was made.

## Measured

On the M2 Max laptop, once, with the model in `models/name/`, by
`evaluate.py` over every held-out clip, reader and meeting, and printed by
`tables.py`. A call is caught when a hearing begins from the start of the
line to 1 s after the name ends. The voice is at -20 dBFS after 2 s of
quiet. "Onset lost": the first 100 ms after each silence 30 dB down.
"Loudspeaker": microphone.py's loudspeaker at the middle of its ranges.
"Both": the two. "Talk around": a LibriSpeech test reader 15 dB under.
"English reading" and "Vietnamese reading" are the LibriTTS test voices
(the second the stand-in accent; it also takes in a third of Cori's
lines); Cori, a British voice from another Piper model, has a row of its
own. The trade-off table counts every generated call together.

### Calls, at 0.9 for one step (the threshold `lamp-name` uses now)

| Way of calling | Voices | as recorded | onset lost | loudspeaker | both | talk around |
|---|---|---:|---:|---:|---:|---:|
| "Hey Lamp, ..." | generated, English reading | 277/294 (94 %) | 277/294 (94 %) | 282/294 (96 %) | 280/294 (95 %) | 244/294 (83 %) |
| "Hey Lamp, ..." | generated, Vietnamese reading | 230/263 (87 %) | 220/263 (84 %) | 224/263 (85 %) | 221/263 (84 %) | 179/263 (68 %) |
| "Hey Lamp, ..." | generated, a British voice (Cori) | 63/94 (67 %) | 57/94 (61 %) | 55/94 (59 %) | 52/94 (55 %) | 30/94 (32 %) |
| "Lamp ơi, ..." | generated, English reading | 325/354 (92 %) | 319/354 (90 %) | 327/354 (92 %) | 315/354 (89 %) | 274/354 (77 %) |
| "Lamp ơi, ..." | generated, Vietnamese reading | 152/199 (76 %) | 152/199 (76 %) | 153/199 (77 %) | 153/199 (77 %) | 128/199 (64 %) |
| "Lamp ơi, ..." | generated, a British voice (Cori) | 5/103 (5 %) | 5/103 (5 %) | 6/103 (6 %) | 6/103 (6 %) | 6/103 (6 %) |
| "Lamp, ..." | generated, English reading | 295/345 (86 %) | 301/345 (87 %) | 281/345 (81 %) | 247/345 (72 %) | 183/345 (53 %) |
| "Lamp, ..." | generated, Vietnamese reading | 111/209 (53 %) | 109/209 (52 %) | 111/209 (53 %) | 88/209 (42 %) | 69/209 (33 %) |
| "Lamp, ..." | generated, a British voice (Cori) | 14/89 (16 %) | 11/89 (12 %) | 15/89 (17 %) | 13/89 (15 %) | 11/89 (12 %) |
| "Lamp, ..." | real people (MSWC), the word alone | 26/51 (51 %) | 32/51 (63 %) | 25/51 (49 %) | 32/51 (63 %) | 13/51 (25 %) |

### Calls, at 0.98 for one step

| Way of calling | Voices | as recorded | onset lost | loudspeaker | both | talk around |
|---|---|---:|---:|---:|---:|---:|
| "Hey Lamp, ..." | generated, English reading | 264/294 (90 %) | 267/294 (91 %) | 263/294 (89 %) | 261/294 (89 %) | 215/294 (73 %) |
| "Hey Lamp, ..." | generated, Vietnamese reading | 207/263 (79 %) | 204/263 (78 %) | 198/263 (75 %) | 195/263 (74 %) | 161/263 (61 %) |
| "Hey Lamp, ..." | generated, a British voice (Cori) | 38/94 (40 %) | 35/94 (37 %) | 34/94 (36 %) | 38/94 (40 %) | 16/94 (17 %) |
| "Lamp ơi, ..." | generated, English reading | 314/354 (89 %) | 303/354 (86 %) | 309/354 (87 %) | 296/354 (84 %) | 246/354 (69 %) |
| "Lamp ơi, ..." | generated, Vietnamese reading | 143/199 (72 %) | 139/199 (70 %) | 142/199 (71 %) | 132/199 (66 %) | 115/199 (58 %) |
| "Lamp ơi, ..." | generated, a British voice (Cori) | 0/103 (0 %) | 0/103 (0 %) | 1/103 (1 %) | 5/103 (5 %) | 4/103 (4 %) |
| "Lamp, ..." | generated, English reading | 205/345 (59 %) | 199/345 (58 %) | 183/345 (53 %) | 149/345 (43 %) | 123/345 (36 %) |
| "Lamp, ..." | generated, Vietnamese reading | 65/209 (31 %) | 45/209 (22 %) | 63/209 (30 %) | 36/209 (17 %) | 44/209 (21 %) |
| "Lamp, ..." | generated, a British voice (Cori) | 3/89 (3 %) | 2/89 (2 %) | 4/89 (4 %) | 5/89 (6 %) | 4/89 (4 %) |
| "Lamp, ..." | real people (MSWC), the word alone | 9/51 (18 %) | 18/51 (35 %) | 16/51 (31 %) | 15/51 (29 %) | 8/51 (16 %) |

### The trade-off

Each row is a threshold, one step. Calls are of all generated voices; near
misses are the share of all near-miss clips below (not the lamp's own
replies) with any hearing; then hearings an hour where nobody called.

| Score from / steps in a row | "Hey Lamp" | "Lamp ơi" | "Lamp," | "Lamp," both | Near misses, any | LibriSpeech test readers, an hour | the lamp's kind of replies, generated, an hour | FLEURS Vietnamese, an hour | generated talk, Vietnamese accent, an hour | EdAcc, Vietnamese speakers of English, an hour | AMI meetings, an hour |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.5/1 | 97 % | 82 % | 86 % | 81 % | 32.7 % | 18.77 (209 in 11.1 h) | 28.73 (41 in 1.4 h) | 75.77 (233 in 3.1 h) | 5.74 (5 in 0.9 h) | 137.74 (179 in 1.3 h) | 43.40 (1044 in 24.1 h) |
| 0.7/1 | 94 % | 77 % | 80 % | 71 % | 21.7 % | 10.33 (115 in 11.1 h) | 19.62 (28 in 1.4 h) | 53.98 (166 in 3.1 h) | 3.45 (3 in 0.9 h) | 83.11 (108 in 1.3 h) | 24.57 (591 in 24.1 h) |
| 0.8/1 | 91 % | 75 % | 75 % | 65 % | 16.3 % | 7.36 (82 in 11.1 h) | 16.12 (23 in 1.4 h) | 37.40 (115 in 3.1 h) | 3.45 (3 in 0.9 h) | 62.33 (81 in 1.3 h) | 17.92 (431 in 24.1 h) |
| 0.9/1 | 88 % | 73 % | 65 % | 54 % | 9.4 % | 3.32 (37 in 11.1 h) | 8.41 (12 in 1.4 h) | 23.41 (72 in 3.1 h) | 3.45 (3 in 0.9 h) | 36.17 (47 in 1.3 h) | 10.14 (244 in 24.1 h) |
| 0.95/1 | 83 % | 72 % | 56 % | 44 % | 5.3 % | 2.42 (27 in 11.1 h) | 4.91 (7 in 1.4 h) | 16.58 (51 in 3.1 h) | 2.30 (2 in 0.9 h) | 23.85 (31 in 1.3 h) | 6.53 (157 in 24.1 h) |
| 0.98/1 | 78 % | 70 % | 42 % | 30 % | 2.7 % | 1.26 (14 in 11.1 h) | 2.10 (3 in 1.4 h) | 10.73 (33 in 3.1 h) | 2.30 (2 in 0.9 h) | 16.16 (21 in 1.3 h) | 3.16 (76 in 24.1 h) |
| 0.99/1 | 74 % | 68 % | 31 % | 20 % | 1.7 % | 0.72 (8 in 11.1 h) | 2.10 (3 in 1.4 h) | 8.45 (26 in 3.1 h) | 0.00 (0 in 0.9 h) | 7.70 (10 in 1.3 h) | 1.95 (47 in 24.1 h) |
| 0.995/1 | 69 % | 66 % | 21 % | 12 % | 1.0 % | 0.36 (4 in 11.1 h) | 0.70 (1 in 1.4 h) | 7.15 (22 in 3.1 h) | 0.00 (0 in 0.9 h) | 5.39 (7 in 1.3 h) | 1.12 (27 in 24.1 h) |
| 0.999/1 | 58 % | 61 % | 6 % | 2 % | 0.4 % | 0.18 (2 in 11.1 h) | 0.00 (0 in 1.4 h) | 3.90 (12 in 3.1 h) | 0.00 (0 in 0.9 h) | 0.77 (1 in 1.3 h) | 0.58 (14 in 24.1 h) |

Two steps in a row instead of one, at 0.9: false alarms a fifth to a
quarter fewer (readers 2.60 an hour, meetings 8.23, FLEURS 17.23, EdAcc
30.01) and calls 5 to 10 % fewer ("Hey Lamp" 82 %, "Lamp ơi" 73 %,
"Lamp," 60 %).

### Near misses, by kind

Each clip alone, as recorded and with both of the microphone's effects.
"Generated" are lines said by the test voices with the word where a name
goes ("Lamb chops tonight?", "Hey Lan, what time is it?"); "real" are
MSWC's words, cut as the calls were.

| Near miss | Clips | as recorded, 0.9/1 | both, 0.9/1 | as recorded, 0.98/1 | both, 0.98/1 |
|---|---:|---:|---:|---:|---:|
| Lan (generated) | 235 | 59/235 (25 %) | 55/235 (23 %) | 37/235 (16 %) | 22/235 (9 %) |
| Liam (generated) | 247 | 1/247 (0 %) | 6/247 (2 %) | 0/247 (0 %) | 1/247 (0 %) |
| Liam (real) | 6 | 1/6 (17 %) | 1/6 (17 %) | 0/6 (0 %) | 0/6 (0 %) |
| Lâm (generated) | 204 | 50/204 (25 %) | 56/204 (27 %) | 23/204 (11 %) | 27/204 (13 %) |
| Lâm (real) | 5 | 2/5 (40 %) | 3/5 (60 %) | 0/5 (0 %) | 0/5 (0 %) |
| Sam (generated) | 204 | 1/204 (0 %) | 0/204 (0 %) | 1/204 (0 %) | 0/204 (0 %) |
| Sam (real) | 36 | 0/36 (0 %) | 1/36 (3 %) | 0/36 (0 %) | 1/36 (3 %) |
| hey (real) | 292 | 0/292 (0 %) | 2/292 (1 %) | 0/292 (0 %) | 0/292 (0 %) |
| lab (generated) | 208 | 23/208 (11 %) | 11/208 (5 %) | 5/208 (2 %) | 4/208 (2 %) |
| lab (real) | 42 | 9/42 (21 %) | 4/42 (10 %) | 3/42 (7 %) | 0/42 (0 %) |
| lamb (generated) | 240 | 73/240 (30 %) | 47/240 (20 %) | 13/240 (5 %) | 13/240 (5 %) |
| lamb (real) | 14 | 7/14 (50 %) | 3/14 (21 %) | 0/14 (0 %) | 2/14 (14 %) |
| lamp in a sentence (generated) | 202 | 5/202 (2 %) | 10/202 (5 %) | 1/202 (0 %) | 3/202 (1 %) |
| lamp in a sentence (real) | 7 | 0/7 (0 %) | 0/7 (0 %) | 0/7 (0 %) | 0/7 (0 %) |
| land (generated) | 219 | 19/219 (9 %) | 8/219 (4 %) | 0/219 (0 %) | 0/219 (0 %) |
| land (real) | 322 | 55/322 (17 %) | 43/322 (13 %) | 5/322 (2 %) | 4/322 (1 %) |
| ma'am (generated) | 225 | 1/225 (0 %) | 5/225 (2 %) | 0/225 (0 %) | 0/225 (0 %) |
| ma'am (real) | 23 | 0/23 (0 %) | 0/23 (0 %) | 0/23 (0 %) | 0/23 (0 %) |
| other names (generated) | 213 | 21/213 (10 %) | 24/213 (11 %) | 9/213 (4 %) | 11/213 (5 %) |
| other names (real) | 1 | 0/1 (0 %) | 0/1 (0 %) | 0/1 (0 %) | 0/1 (0 %) |
| plan (generated) | 244 | 1/244 (0 %) | 3/244 (1 %) | 0/244 (0 %) | 1/244 (0 %) |
| plan (real) | 302 | 5/302 (2 %) | 12/302 (4 %) | 0/302 (0 %) | 2/302 (1 %) |
| rhymes (generated) | 209 | 29/209 (14 %) | 33/209 (16 %) | 8/209 (4 %) | 12/209 (6 %) |
| rhymes (real) | 236 | 8/236 (3 %) | 14/236 (6 %) | 3/236 (1 %) | 3/236 (1 %) |
| the lamp's own replies (generated) | 2150 | 35/2150 (2 %) | 56/2150 (3 %) | 13/2150 (1 %) | 18/2150 (1 %) |

The near misses at 0.9 that matter most are the ones that sound like it
and come where a name comes: "Lamb", "Lan", "Lâm", "lab", "land". A
colleague named Lan or Lâm would set it off about one time in four.

### Beside the same recipe without a bare "Lamp"

The lamp's own `name.onnx` could not be had. This is the stand-in: the
same data and training with the bare calls left out, so trained on "Hey
Lamp" and "Lamp ơi" as name.md describes the lamp's. Measured the same way.

| | This model, 0.9 | Without a bare "Lamp", 0.9 | This model, 0.98 | Without, 0.98 |
|---|---:|---:|---:|---:|
| "Hey Lamp", generated, all | 88 % | 90 % | 78 % | 84 % |
| "Lamp ơi", generated, all | 73 % | 73 % | 70 % | 71 % |
| "Lamp,", generated, all | 65 % | 1 % | 42 % | 0 % |
| "Lamp,", with both microphone effects | 54 % | 1 % | 30 % | 0 % |
| "Lamp,", real word (MSWC) | 51 % | 0 % | 18 % | 0 % |
| "Hey Lamp", Cori (another generator) | 67 % | 73 % | 40 % | 52 % |
| "Lamp ơi", Cori | 5 % | 2 % | 0 % | 2 % |
| "Lamp,", Cori | 16 % | 2 % | 3 % | 1 % |
| "Lamb" at the start, generated | 30 % | 0 % | 5 % | 0 % |
| "Lan", generated | 25 % | 28 % | 16 % | 21 % |
| Near misses, any | 9.4 % | 4.5 % | 2.7 % | 3.1 % |
| LibriSpeech readers, an hour | 3.32 | 3.68 | 1.26 | 1.62 |
| The lamp-like replies, an hour | 8.41 | 1.40 | 2.10 | 1.40 |
| Vietnamese (FLEURS), an hour | 23.41 | 17.23 | 10.73 | 10.41 |
| Generated talk, Vietnamese accent, an hour | 3.45 | 2.30 | 2.30 | 1.15 |
| Vietnamese speakers of English (EdAcc), an hour | 36.17 | 47.71 | 16.16 | 30.01 |
| AMI meetings, an hour | 10.14 | 9.93 | 3.16 | 5.53 |

The comparison model does no better on Cori's voice: the weakness on a
voice from another generator belongs to the way both were trained, and
very likely to the lamp's current model too, trained on generated speech
alone.

### Two things looked at more closely

- **What in the lamp-like replies sets it off.** At 0.9, 35 of the 2,150
  replies; 24 of them were the 64 saying "Lamp here, what can I do for
  you?" (the name, at the start of a sentence). The rest, one to four
  each, were other lines ("Let me think about that for a second.").
- **What in Vietnamese sets it off.** At 0.9, 68 of the 857 FLEURS test
  lines (8 %) had a hearing. Of the 63 lines with "làm", 16 %; of the 32
  with "nam", 19 %; "lan" 0 of 11. It is spread over ordinary speech more
  than one word, and twice as likely where "làm" is.

## What went wrong on the way, and was kept

- **The first model was trained on silent Vietnamese.** FLEURS stores
  32-bit floats; read as 16-bit integers, libsndfile gave silence, and 9 h
  of "Vietnamese talk" in training were quiet. That model fired on
  Vietnamese speakers of English (EdAcc) 71 times an hour at 0.9, and on
  real FLEURS lines its highest score reached 0.35 in half of them and
  0.96 in one in ten. FLEURS is
  now read as floats, and 3.4 h of generated talk read with the Vietnamese
  accent were added as talk that is not a call, because the accented calls
  had no accented talk against them. Retrained the same way, the model in
  this document: EdAcc 36 an hour, readers 6.5 to 3.3 an hour, the bare
  call at 86 % (from 83 %).
- **The same with no bare calls** (the comparison model) shows the price
  of the bare name: near misses that end like "Lamp" ("Lamb", "lab",
  "land", the rhymes) go from 0 to 2 % to 9 to 30 %.
- **Every model here is one training run** with one seed.

## The lamp's current name.onnx

It could not be measured beside this one. It is not in the repository on
any branch (`git grep name.onnx` over `lamp-voice`, `lamp-second` and the
rest finds only the code that loads it), and it is not on this machine
(a Spotlight search for `.onnx` files and `find` for `name.onnx` under
`~/code`, `~/harnesses` and `/private/tmp` found only copies made here).
[name.md](name.md) says it is installed on lamp-4ace in
`/opt/lampOS/name/models/` and that its training "is outside the
repository"; [attention.md](attention.md) calls it "a study on 2026-10-14,
kept outside the repository". No SHA-256 of it is recorded.

What stands in for it here is the same recipe as the new model with the
bare calls left out (`--leave-out-way 3`): trained, as name.md describes
the current one, on "Hey Lamp" and "Lamp ơi". It is not the lamp's file.
To put the lamp's file in the same tables, copy it off the lamp and run,
from `lampOS/training/name/`:

    python evaluate.py /path/to/the/lamps/name.onnx --only calls,near
    python evaluate.py /path/to/the/lamps/name.onnx --only talk,ami,edacc
    python tables.py ../../.cache/name/runs/name-calls-near.eval.json ../../.cache/name/runs/name-talk-ami-edacc.eval.json

(the result files are named after the model file's name).

## Wiring it on the lamp

It has the shape of the current model, so steps 1 to 3 and 5 change no
code; step 4 is for whatever takes `name/heard`, if it does not already
pass over hearings while the lamp speaks:

1. Keep the current file: `mv /opt/lampOS/name/models/name.onnx /opt/lampOS/name/models/name-hey.onnx`.
2. Copy `models/name/name.onnx` to `/opt/lampOS/name/models/name.onnx`.
3. Leave the threshold at its default, 0.9, one step: the rule of
   `said.rs` does not change. For fewer false hearings at the cost of
   bare calls, `--least 0.98` ("The trade-off").
4. Have whatever takes `name/heard` pass over hearings while the lamp's
   own reply is playing: the lamp saying "Lamp here, ..." sets it off.
5. Restart `lamp-name.service`. To go back, put `name-hey.onnx` back.

**One command on the lamp, before any of that**: put the two front-end
files and this `name.onnx` in a directory of their own and run
`lamp-name check THAT_DIR FILE.wav ...` over recordings of the lamp's own
microphone (the 2026-10-14 ones in attention.md, or new ones made with
`lamp-bus record DIR mic2/audio --seconds 600` and turned into WAV with
`training/stop/from_bus.py`). It prints each hearing at 0.9 and the
highest score per file. `listen.py` (training/stop) does the same on the
Mac at any threshold.

## Where it is most likely to be wrong on the lamp

1. **A bare "Lamp" after the microphone's lost onset.** The first tenth
   of a second of "Lamp" is most of its "l". The loss here is a guess at
   the size and shape of what the lamp does. The rows "onset lost" and
   "both" are where to look first, with the lamp's own recordings of
   calls that begin after a silence.
2. **A Vietnamese speaker's "Lamp", and "Lâm", "Lan".** Every accented
   line here is a stand-in: English voices given Vietnamese phonemes. A
   colleague called Lan or Lâm sets it off (the table above); that will
   happen in this office.
3. **The lamp's own voice.** The stand-in is LibriTTS women, Cori and LJ
   Speech reading lamp-like replies, some with the word "Lamp" in them.
   Gemini's "Kore" voice through the loudspeaker and the canceller was not
   heard. Ten minutes of the lamp talking with nobody near, recorded and
   run through `lamp-name check`, says how often it sets itself off.
4. **Played calls.** The 2026-10-14 checks played generated voices from a
   loudspeaker; the loudspeaker's muffling here is a guess too.

## On the lamp's own recordings

On lamp-4ace's mic2, through its speaker and canceller, this model heard
the bare call in one voice of three (#705, 18 Oct, 12:59, per-call windows
of `lamp-name check`): Daniel's two bare calls reached 0.82 and 0.57,
Samantha's and Karen's four stayed at 0.02 to 0.30, and its "Hey Lamp"
swung from 0.01 to 0.89. The installed file heard all six "Hey Lamp" at
0.97 to 1.00 and no bare call over 0.21. So the generated voices do not
carry over to this microphone, and lowering the bar would buy one voice.

`training/name/lamp.py` learns from the lamp's own recordings instead, and
judges a model only on voices it never heard:

- `python lamp.py lines VOICES.txt`: from `say -v '?'`'s output, the play
  list for the scenario player (`Voice|text|seconds`): per English speaking
  voice (up to twelve, macOS's novelty voices left out, taken in turn from
  each English locale), five bare calls and three "Hey Lamp", 7 s each so the
  lamp can answer, and three negatives (the word inside a sentence; "lamb",
  "lab", "camp", "Lan"), 5 s each; "Lamp ơi" by a Vietnamese voice if
  there is one. About 15 minutes.
- `python lamp.py train --front DIR --plays PLAYED.tsv RECORD.jsonl
  [--negatives RECORD.jsonl…] [--compare NAME.onnx…]`: the recordings
  (`lamp-bus record DIR mic2/audio --max-mb 160`, whose messages carry their
  own times, so no anchor is needed; the default 64 MiB stops after about
  19 minutes) and each played line's start (`UNIX_MS`, voice,
  kind). A call's steps from 0.35 s to 1.6 s after it began are its own,
  the rest of it to 4 s (its words, the lamp's answer) neither, and every
  other step a negative, with `--negatives` (an empty office, the lamp's
  stories) all negative. It trains one model a fold of voices on the other
  folds, and without one stretch of the voiceless negatives, and scores each
  call and each stretch only by the model that never heard it. False
  hearings are told apart by source (near, mid and other lines, the gaps
  with the lamp's own answers, the quiet recording):
  per voice, the calls heard at 0.5, 0.9 and 0.98, and false hearings an
  hour of negatives by `said.rs`'s rule, beside the same for each
  `--compare` model. Then one model on every voice, written as `name.onnx`.
- `--rescue NAME.onnx [--gates 0.1,0.3,0.5]`: the two models together, one
  table a gate G. A step's score is the given model's (the installed one),
  or, where that model reached G at this step or in the 1 s before it in the
  same recording, the higher of it and the trained folds' held-out score.
  So the installed model gates, and the trained one only rescues a call the
  installed one half heard. The table is what `lamp-name` would say running
  both models on each step with `said.rs`'s rule on that score.
- The model is `train.py`'s "flat" shape on `[1, 16, 96]`, fitted with
  scikit-learn's MLP, the scaling folded in: `lamp-name check` runs it as
  it runs the installed file (shown on a stand-in, 18 Oct), and its ONNX
  matches the fit to 2e-7.
- `python lamp.py dry-run --front DIR WAV…` runs it all on speech files and
  made-up plays; its numbers mean nothing.

**The first sitting** (lamp-4ace, 18 Oct, 15:38 to 16:02, trained on the
office Mac; the recordings and the model stay there): 108 of 135 played
lines were recorded (the recorder stopped at its 64 MiB default), and of
the twelve voices `lines` took, eight were macOS's novelty voices. Over 99
calls, with each voice held out, the model trained here heard 68 at 0.98,
against 17 for the installed file and none for the bare-name one; but it
heard 61.8 false calls an hour of negatives at 0.98, against none for the
installed file, before the negatives were held out by stretch and counted
by source. So a model of the lamp's own recordings hears voices it never
heard far better, and its negatives are the next thing to bring down. The
script now takes speaking voices only, holds the voiceless negatives out
by fold, and says where false hearings come from.

**The second sitting** (lamp-4ace, 18 Oct, 19:06 to 19:35, the first
builder's): 135 lines by twelve speaking voices and a Vietnamese one, all
recorded (`--max-mb 160`), and five quiet minutes. Over 99 calls the
trained folds heard 82 at 0.98, the installed file 35 and the bare-name one
6; but at 0.98 the trained folds heard 104.6 false calls an hour of
held-out negatives (18.9 min), the installed file none and the bare-name
one 6.3. Of the trained folds' false calls, 18 came from 1.7 min of near
lines, 9 from the 11.6 min of gaps (the lamp's own answers, the room), 6
from mid lines and none from the quiet minutes. Not for the lamp. Next,
`--rescue` on the same recordings, with the installed file as the gate: a
gate G that keeps false calls at 0.98 under about 2 an hour while calls
heard rise well above 35 of 99 goes forward; if none does, more near lines
and more of the lamp's own answers as negatives, in another sitting.

Not shown: a model fit to install. Played voices only, nothing of a
person; recording people waits for the owner's yes.

## How to make it again

From `lampOS/training/name/`, with `uv`, a network and about 60 GB of disk
for a while:

    sh make.sh

It runs training/stop's `make.sh` as far as the sets both tasks use, then
downloads its own (each checked against
[`downloads.sha256`](../training/name/downloads.sha256)), generates the
calls, near misses, replies and accented talk (`make_speech.sh`), gathers
its sets (`clips.py`), builds the inputs (`make_inputs.sh`), trains this
model and the one without bare calls, measures both and prints the tables.
On the M2 Max shared with other sessions: the generated speech about an
hour, the MSWC alignment 20 minutes, the inputs 30 minutes, each model 30
to 150 minutes on three threads (the second training ran beside other
work), the measurements about 50 minutes.

## Not shown

- **The lamp.** Nothing here was played to the lamp or recorded by it.
  The microphone's lost onset and the loudspeaker's muffling are guesses
  from the task's words.
- **The lamp's current name.onnx**, which could not be found. The
  comparison is with a stand-in trained the way name.md describes it.
- **A real Vietnamese speaker saying "Lamp" or "Lamp ơi".** Every accented
  call is an English voice given espeak's Vietnamese phonemes. The only
  real Vietnamese speakers of English (EdAcc) were heard as talk, never
  calling the lamp.
- **A real "Hey Lamp" or "Lamp ơi" by anyone.** The real calls are 51
  "lamp" words cut out of read sentences; no person said "Lamp," as a
  call. The generated calls come from the same voice generator as the
  training calls, with other voices.
- **Gemini's voice.** The lamp's own voice is stood in for by LibriTTS
  women, Cori and LJ Speech reading lamp-like lines.
- **More than one training run**, and anything about thresholds between
  the ones in the tables.
- **The cost on the lamp.** The model is task 3's shape and size (30,913
  weights; 24 microseconds a run in the Linux ARM64 container on the Mac);
  on the lamp it replaces a model of about the same cost, but that was
  not measured there.
