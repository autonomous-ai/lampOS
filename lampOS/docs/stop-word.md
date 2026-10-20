# Hearing "stop" under the lamp's own voice

`stop.onnx` gives one score every 80 ms: how likely it is that someone has
just said "stop", "wait" or "hold on". It takes exactly the numbers
`lamp-name` already makes for the name's model, so on the lamp it is one
more small model on numbers that are there anyway.

This is task 3 of [the builders' list](work-split.md). The model was
trained and measured on a laptop (Apple M2 Max, macOS 26.6). Its graph
was run under tract 0.23.8 there and in a Linux ARM64 container. Nothing
here was run on a lamp, and no number here is the lamp's.

## What exists and what we take

**openWakeWord.** The lamp's name already uses its front end: a mel
spectrogram and Google's speech embedding, 96 numbers every 80 ms, fixed
(`hear.rs`, [name.md](name.md)). Its own models are a small network on
sixteen of those: Flatten, then Linear, LayerNorm and ReLU layers, then a
sigmoid ([train.py](https://github.com/dscripka/openWakeWord/blob/main/openwakeword/train.py)).
Its recipe trains on generated speech (Piper's LibriTTS-R voice through
`piper-sample-generator`), on look-alike phrases as hard negatives, and on
about 2,000 hours of other sound as precomputed features. It aims at
under 0.5 false accepts an hour with under 5 % misses, measured on about
5.5 hours of DiPCo
([README](https://github.com/dscripka/openWakeWord),
[notebook](https://github.com/dscripka/openWakeWord/blob/main/notebooks/automatic_model_training.ipynb),
[custom_model.yml](https://github.com/dscripka/openWakeWord/blob/main/examples/custom_model.yml)).
Its default weight on negatives rises to 1,500 against a positive. Its
"custom verifier" is a logistic regression per user on the same features,
asked only when the base model is above a threshold
([custom_verifier_models.md](https://github.com/dscripka/openWakeWord/blob/main/docs/custom_verifier_models.md)).

- Taken: the front end, unchanged; a small last model on sixteen
  embeddings; generated speech and look-alike phrases.
- Changed: plain ops only (no LayerNorm), so that tract runs it as it runs
  `name.onnx`.
- Not taken, for their licences: the 2,000 hours of negative features
  are CC BY-NC-SA 4.0
  ([dataset card](https://huggingface.co/datasets/davidscripka/openwakeword_features)).
  Piper's `en_US-libritts_r-medium` voice, which openWakeWord uses, was
  fine-tuned from the Lessac voice, whose recordings are licensed for
  research only
  ([model card](https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/libritts_r/medium/MODEL_CARD),
  [Lessac licence](https://www.cstr.ed.ac.uk/projects/blizzard/2013/lessac_blizzard2013/license.html)).
  Here the generated speech is Piper's `en_US-libritts-high`, trained
  from scratch on LibriTTS
  ([model card](https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/libritts/high/MODEL_CARD)).
- Not taken: a verifier per user. It needs each user's recordings.

**The embedding.** Lin and others, "Training Keyword Spotters with
Limited and Synthesized Speech Data", ICASSP 2020
([arXiv 2002.01322](https://arxiv.org/abs/2002.01322)). Trained on about
200 million two-second YouTube clips. A small model on it, trained on 3,220
synthetic clips, scored 92.6 % on Speech Commands' 35 words; on as many
real clips, 95.3 %. So a few thousand real words are enough to train on,
and generated speech carries part of the way. Nothing in it was trained on
two voices at once.

**Speech Commands** (Warden 2018, [arXiv 1804.03209](https://arxiv.org/abs/1804.03209),
CC BY 4.0). Version 2: 105,829 one-second clips of 35 words from 2,618
speakers; "stop" 3,872 times. Its validation and test lists are made from a
hash of the speaker's part of the file name, so a speaker is in one list
only ([input_data.py](https://github.com/tensorflow/tensorflow/blob/master/tensorflow/examples/speech_commands/input_data.py)).
Known results on its words, clean and alone: 88.2 % for its baseline,
98.7 % for BC-ResNet ([arXiv 2106.04140](https://arxiv.org/abs/2106.04140)).
None of that is under another voice. "Wait" and "hold" are not among its
words.

**How others hear a stop under their own playback.**

- Google: "To stop a timer, you don't need to say 'Hey Google.' Just say
  'Stop.'" (English only) ([Nest help](https://support.google.com/googlenest/answer/7028899?hl=en)).
  Since January 2022 a bare "stop" also cuts off the Assistant while it
  speaks on smart speakers and displays
  ([9to5Google](https://9to5google.com/2022/01/25/google-assistant-stop/)).
  How it is built is not published.
- Apple, HomePod (2018): its own sound is 30 to 40 dB louder than a voice
  across the room, and after its multichannel canceller the leftover echo
  is still 10 to 20 dB above the voice when it plays loud. A network
  trained on real echo estimates a mask for a Wiener filter and a
  suppression gain ([Apple ML Research](https://machinelearning.apple.com/research/optimizing-siri-on-homepod-in-far-field-settings)).
  That is the lamp's position, a little worse, and the answer was a
  better canceller, not a better word spotter.
- Amazon: training a wake word with music and TV mixed in at 0 to 40 dB
  under it cut misses by 30 to 45 % (Raju and others 2018,
  [arXiv 1808.00563](https://arxiv.org/abs/1808.00563)). Giving the network
  the playback signal as a second input ("implicit echo cancellation")
  cut misses in device-directed speech during playback by 56 % (Cornell
  and others, SLT 2022, [arXiv 2111.10639](https://arxiv.org/abs/2111.10639)).
- Taken: training under the other voice across the lamp's range, as the
  first of these did, and through a suppressor. Not taken, for now: the
  playback signal as an input. It is the largest gain reported, but the
  fixed front end has no place for it; it would be a new model and a new
  interface, not a second small model beside the name's.

**Short words under another voice.** Papers make their own mixtures of
Speech Commands with other speech (for example Shi and others,
Interspeech 2023,
[ISCA](https://www.isca-archive.org/interspeech_2023/shi23f_interspeech.html)).
No public set of short commands spoken over other speech was found. The
Microsoft AEC Challenge has real double talk
([repo](https://github.com/microsoft/AEC-Challenge)), but its recordings'
own licence is not stated and its near-end speech is not commands.

**Sources used here, and their licences.**

| Source | What it gives | Licence |
|---|---|---|
| Speech Commands v0.02 | "stop" from 2,618 speakers; 34 other words; background noise | CC BY 4.0 |
| MSWC, English ([Mazumder and others 2021](https://datasets-benchmarks-proceedings.neurips.cc/paper/2021/hash/fe131d7f5a6b38b23cc967316c13dae2-Abstract-round2.html)) | "stop", "wait", "stopped", "stopping", "stops", "waited", "waiting", "holding", and about 8,900 other words, cut from Common Voice sentences | CC BY 4.0 ([Hugging Face](https://huggingface.co/datasets/MLCommons/ml_spoken_words), not gated) |
| LibriSpeech dev, test ([openslr 12](https://www.openslr.org/12/)) and train-clean-100 ([Hugging Face](https://huggingface.co/datasets/openslr/librispeech_asr), not gated) | The other voice; talk to train against; "stop", "wait", "hold on" as read in books; near misses in sentences | CC BY 4.0 |
| Piper `en_US-libritts-high`, 904 voices | Generated phrases with the targets, near misses, short replies, long talk | Voice trained on LibriTTS, CC BY 4.0 ([openslr 60](https://www.openslr.org/60/)); voices repository MIT |
| AMI meeting corpus, 34 meetings ([licence](https://groups.inf.ed.ac.uk/ami/corpus/license.shtml)) | Ordinary talk, and the targets said in it; testing only | CC BY 4.0 |
| wav2vec 2.0 base, 960 h (torchaudio `WAV2VEC2_ASR_BASE_960H`) | Only to find where words are; nothing of it is in the model | MIT ([docs](https://docs.pytorch.org/audio/stable/generated/torchaudio.pipelines.WAV2VEC2_ASR_BASE_960H.html)) |

Not used: Common Voice itself (since October 2025 it is downloaded only
with an account at the Mozilla Data Collective, and the Hugging Face copies
are gated; MSWC reaches its speakers under CC BY 4.0); macOS voices. The
openslr mirror gave about 30 MB a minute, so train-clean-100 came from
the Hugging Face copy instead.

## Answer

On the laptop, on speakers it never heard, at the proposed threshold (a
score of 0.9 for one step):

- **Heard alone, it catches most of them**: 85 % of 736 real people's
  "stop" and 84 % of 263 real "wait" (Speech Commands, MSWC and
  LibriSpeech test speakers), and 95 % of 375 generated "hold on". "Stop
  it." counts as a stop; 19 of 19 generated were caught.
- **Plainly under a louder voice, it hears few, and no threshold mends
  it.** Level with a LibriSpeech reader (0 dB), 17 % of "stop" and 13 % of
  "wait"; 15 dB under, 4 % and 3 %. A far larger recogniser, wav2vec 2.0,
  wrote the reader's words and not the target's in such mixtures. The
  embedding the lamp uses was never trained on two voices at once, and
  everything after it can only work with what it gives.
- **Where the other voice is taken out first, half comes back.** Through a
  suppressor that is told the other voice exactly: 48 % of "stop" and of
  "wait" at 0 dB, 25 % and 18 % at -15 dB. The lamp's canceller is not
  told its echo exactly, so the lamp is somewhere between the two cases.
- **Talk sets it off** about 9 times an hour of LibriSpeech test readers
  (11.1 h), 3 times an hour of generated voices (0.9 h), and 7 times an
  hour of AMI meetings (24.1 h, away from any target said). "Don't stop",
  "bus stop" and "stop" inside a sentence set it off in 3 %, 3 % and 8 %
  of generated clips; "stopped" and "waited" cut from sentences in 56 % and
  60 %; look-alike single words ("Top.", "Spot.", "Gate.") in 36 %.
- **In real meetings** it heard 1 of the 28 "stop", 3 of the 39 "wait" and
  5 of the 22 "hold on" said in those 34 AMI meetings.
- **A hearing begins** a median 62 ms after the word ends (under another
  voice; 90 % within half a second), before the 48 ms the lamp needs to complete
  the chunk and its step's own time.
- **What this means for the lamp.** On its own it does not answer the
  task's case, a person 0 to 15 dB under the lamp's voice plainly mixed.
  It may help where the canceller has already taken most of the lamp's
  voice out, which is closer to the suppressed case. It is worth wiring
  only behind a switch, only while the lamp is speaking, and as a pause
  that the duplex rules then confirm or undo, so that a false alarm costs
  a moment and not the reply. Before that, ten minutes of the lamp's own
  microphone after the canceller say whether it is worth it ("Where it is
  most likely to be wrong on the lamp").
- **The model:** `models/stop/stop.onnx`, 124,135 bytes, 30,913 weights,
  SHA-256 `5fe07569e3d7c5adcdf496770deb306701b057ab8adce1d03ce901db3ce72d41`
  ([SOURCE.md](../models/stop/SOURCE.md)). Run inside `hear.rs` under
  tract 0.23.8, on the Mac and in a Linux ARM64 container, it gives the
  scores it gave in training's onnxruntime to within 6e-6.

## What was made

All of it is in `lampOS/training/stop/` and runs from nothing with one
command, `sh make.sh` ("How to make it again").

**The numbers it works on** (`features.py`) are made exactly as `hear.rs`
makes them. Chunk i is samples 1280 i to 1280 i + 1760, given to
`melspectrogram.onnx` on the 16-bit scale. Each number becomes x / 10 + 2.
Embedding k is of frames 8k to 8k + 76. The model gets the newest
sixteen, and its score for them is of the sound up to 1.982 s plus 0.08 s
a step. The two front-end files are the pinned ones; their SHA-256 is
checked before use.

One thing had to be found: the mel model ends by flooring every number at
80 dB under the largest **in the whole tensor it is given**. The lamp
gives it one chunk at a time. A batch of chunks would floor them all at
the batch's largest and give other numbers (up to 11.6 apart, on silent
chunks). Here the model is cut before that floor and runs on batches, and
the floor is put back chunk by chunk. Against a step-by-step copy of the
loop in `hear.rs`, the embeddings of 10.4 s of sound then differed by at
most 4.6e-5, on numbers up to about 60.

**Checked against the lamp's own code.** A scratch Rust program (not
committed) includes `crates/name/src/hear.rs` unchanged, under
`tract-onnx` 0.23.8 with default features off and the workspace's
`Cargo.lock`, with `stop.onnx` installed as `name.onnx`. It fed four files
20 ms at a time, as `lamp-name check` does: 1,185 scores. Against
`listen.py` (onnxruntime 1.31 and `features.py`) the largest difference was
6e-6 on the Mac and 6e-6 in the `lampos-dev:1.96` Linux ARM64 container.
The three models took 1.59 ms a step on the Mac and 2.10 ms in the
container, against 1.70 ms with a model the size of the first tried
(1536 x 128). `stop.onnx` has 30,913 weights and about 0.4 million
multiplications a step: almost nothing beside the front end.

**Who says what.** Every set is split by speaker. No speaker of a test
set is in any training set; this was checked set against set.

| Set | Train | Test |
|---|---|---|
| "stop", Speech Commands, aligned and cut | 3,014 clips, 1,230 speakers | 404 clips, 152 speakers (its testing list) |
| "stop", MSWC | 1,848 clips, 1,488 speakers | 319, 258 |
| "wait", MSWC | 1,615, 1,213 | 244, 189 |
| "stop", "wait", "hold on" read in LibriSpeech | 28 (dev) | 34 (test): 13 stop, 19 wait, 2 hold on |
| Generated phrases with a target (46 phrases, "Stop it." among them) | 9,000, 791 voices | 1,200, 113 other voices |
| Near misses, generated (61 phrases in 9 groups) | 3,000 | 1,400 |
| Near misses, MSWC ("stopped", "stopping", "stops", "waited", "waiting", "holding", "hold") | 5,190 | 979 |
| Other single words: Speech Commands' 34, MSWC's about 8,900 | 4,542 and 9,515 | 930 and 2,387 |
| Short replies ("Mm-hm.", "Yeah."...), generated | 600 | 200 |
| The other voice, read: LibriSpeech | dev-clean, dev-other: 10.5 h, 73 readers (11 kept back for validation) | test-clean, test-other: 10.7 h, 73 readers |
| The other voice, generated | 1,000 sentences, 1.5 h | 600 sentences, 0.9 h |
| More talk to train against: train-clean-100 | 25.6 h, 251 readers | |
| Ordinary talk: AMI meetings, headset mix | | 34 meetings, 24.1 h |

"Hold on" from real people is almost missing: two clips in the test
readers, and those in the AMI meetings. MSWC has "hold" but no "on" (it
keeps single words), so its "hold" clips are near misses here.

**Where a word is.** wav2vec 2.0 is forced to the known words of each
clip (`align.py`). A word's end is moved on to where its sound is 25 dB
under its loudest, within 0.15 s, so that a closing "p" or "t" is inside
it. A clip is the word and a quarter second each side, faded in and out
over 20 ms. Clips the forced alignment scored under 0.2 were dropped
(Speech Commands "stop": 110 of 3,872).

**Mixing** (`mix.py`, `make_data.py`). A level is an active level: the
mean power of the 10 ms frames within 30 dB of the loudest 5 %, in dBFS.
A word's level is over the word; the other voice's over the 2.6 s it
plays. "-10 dB" means the word 10 dB under the other voice. The other voice
was put at -30 to -10 dBFS (the lamp's own voice reaches -9.6 to -16.4 dBFS
in its microphone, [duplex-listening.md](duplex-listening.md)), and the
word 20 dB under it to 6 dB over; 12 % of words alone. Then, at random: a
room on the person (a decaying tail, 0.15 to 0.5 s); in 40 %, a suppressor
like a canceller's, which takes off in each time and frequency cell what
the other voice puts there, overdone or underdone by up to two times, with
a floor of -10 to -30 dB, and so takes some of the person too; the
microphone's band (high-pass 80 to 400 Hz, low-pass 3.5 to 7.5 kHz) in
half; a noise floor of -80 to -55 dBFS; clipping in 5 %.

**Labels.** Each example is 2.59 s, long enough for nine inputs 80 ms
apart. A target is placed so that it ends 0.02 to 0.10 s before the end of
the first input's sound. An input is a target when one ended 0.02 to 0.74
s before the end of what it sees. Every other short clip (other words,
near misses, short replies) is placed the same way by the end of its own
word, so that where a word sits says nothing of whether it is a target.
Long talk is placed anywhere and is all negative: 12 h of dev readers,
generated voices and two voices at once, and 25 h of train-clean-100
readers and two at once.

**Training** (`train.py`): 106,756 target inputs (of 198,178 made),
307,800 inputs of other words, 1.6 million of talk. Each batch of 1,024 is a quarter targets,
a quarter other words, half talk. A negative weighs three times a target.
Targets plainly under the other voice by more than 3 dB were left out: see
"What was tried". AdamW, one-cycle rate up to 0.002, 12,000 steps, dropout
0.2, a little noise on the inputs. Validation: Speech Commands' own
validation speakers under the 11 dev readers kept back, and an hour of those
readers alone.

**Measuring** (`evaluate.py`): every number in the tables comes from
streams of sound run through the front end and the model step by step, as
`lamp-name` would, and the rule below.

- Catches: each held-out target clip, its end at 3.2 s into 5 s of another
  voice, with the other voice at -14 dBFS. Alone, the word is at -20 dBFS.
  The other voice is a LibriSpeech test reader, or a held-out generated
  voice; the same stretch of it at every level. Each level is mixed
  plainly, and again through the suppressor at strength 1 and floor -20
  dB, which is told the other voice exactly (a canceller that knows its
  echo, which a real one does not). A catch is a hearing that begins
  between the word's start and 1 s after its end.
- False alarms: hearings an hour in every test reader's utterances played
  one after another, each at -14 dBFS (10.7 h); the same for the generated
  test voices (0.9 h); and in 34 AMI meetings as recorded (24.1 h), not
  counting hearings from 0.2 s before to 1.5 s after a transcribed "stop",
  "wait" or "hold on".
- Near misses: the share of clips with any hearing, each clip alone at
  -20 dBFS between 2 s of quiet and 1.5 s.

**The rule for a hearing** (`hearing.py`) is `said.rs`'s: a hearing begins
when the score has been at or over the threshold for N steps in a row; it
is over when the score falls, or 0.8 s after it began; then 1 s is passed
over. The one change: the moment counted is when the hearing begins, not
when it is said, because a stop is wanted at once.

## Measured

On the M2 Max laptop, once, with the model in `models/stop/`, by
`evaluate.py` over every test clip, reader and meeting
(`stop-catches.eval.json` and `stop-talk-ami-near.eval.json`, printed by
`tables.py`). "Real people" pools Speech Commands, MSWC and the
LibriSpeech test readers; "generated" is Piper's held-out voices. A
percentage is of the clips in that cell.

### Catches at 0.9, one step, under a LibriSpeech test reader

| Word | Speakers | alone | 0 dB | -5 dB | -10 dB | -15 dB | 0 dB, suppressed | -5 dB, suppressed | -10 dB, suppressed | -15 dB, suppressed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| stop | LibriSpeech | 2/13 (15 %) | 2/13 (15 %) | 1/13 (8 %) | 0/13 (0 %) | 0/13 (0 %) | 2/13 (15 %) | 3/13 (23 %) | 2/13 (15 %) | 1/13 (8 %) |
| stop | MSWC | 257/319 (81 %) | 54/319 (17 %) | 36/319 (11 %) | 23/319 (7 %) | 13/319 (4 %) | 140/319 (44 %) | 120/319 (38 %) | 95/319 (30 %) | 75/319 (24 %) |
| stop | Piper | 344/385 (89 %) | 88/385 (23 %) | 61/385 (16 %) | 48/385 (12 %) | 28/385 (7 %) | 240/385 (62 %) | 220/385 (57 %) | 175/385 (45 %) | 133/385 (35 %) |
| stop | Speech Commands | 370/404 (92 %) | 68/404 (17 %) | 45/404 (11 %) | 29/404 (7 %) | 17/404 (4 %) | 208/404 (51 %) | 171/404 (42 %) | 143/404 (35 %) | 106/404 (26 %) |
| stop | **real people** | 629/736 (85 %) | 124/736 (17 %) | 82/736 (11 %) | 52/736 (7 %) | 30/736 (4 %) | 350/736 (48 %) | 294/736 (40 %) | 240/736 (33 %) | 182/736 (25 %) |
| stop | **generated voices** | 344/385 (89 %) | 88/385 (23 %) | 61/385 (16 %) | 48/385 (12 %) | 28/385 (7 %) | 240/385 (62 %) | 220/385 (57 %) | 175/385 (45 %) | 133/385 (35 %) |
| wait | LibriSpeech | 8/19 (42 %) | 3/19 (16 %) | 3/19 (16 %) | 0/19 (0 %) | 1/19 (5 %) | 8/19 (42 %) | 6/19 (32 %) | 4/19 (21 %) | 4/19 (21 %) |
| wait | MSWC | 214/244 (88 %) | 30/244 (12 %) | 19/244 (8 %) | 11/244 (5 %) | 6/244 (2 %) | 119/244 (49 %) | 95/244 (39 %) | 63/244 (26 %) | 44/244 (18 %) |
| wait | Piper | 427/440 (97 %) | 154/440 (35 %) | 100/440 (23 %) | 70/440 (16 %) | 46/440 (10 %) | 337/440 (77 %) | 287/440 (65 %) | 235/440 (53 %) | 187/440 (42 %) |
| wait | **real people** | 222/263 (84 %) | 33/263 (13 %) | 22/263 (8 %) | 11/263 (4 %) | 7/263 (3 %) | 127/263 (48 %) | 101/263 (38 %) | 67/263 (25 %) | 48/263 (18 %) |
| wait | **generated voices** | 427/440 (97 %) | 154/440 (35 %) | 100/440 (23 %) | 70/440 (16 %) | 46/440 (10 %) | 337/440 (77 %) | 287/440 (65 %) | 235/440 (53 %) | 187/440 (42 %) |
| hold on | LibriSpeech | 0/2 (0 %) | 1/2 (50 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) |
| hold on | Piper | 358/375 (95 %) | 175/375 (47 %) | 114/375 (30 %) | 79/375 (21 %) | 54/375 (14 %) | 333/375 (89 %) | 312/375 (83 %) | 276/375 (74 %) | 223/375 (59 %) |
| hold on | **real people** | 0/2 (0 %) | 1/2 (50 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) |
| hold on | **generated voices** | 358/375 (95 %) | 175/375 (47 %) | 114/375 (30 %) | 79/375 (21 %) | 54/375 (14 %) | 333/375 (89 %) | 312/375 (83 %) | 276/375 (74 %) | 223/375 (59 %) |

The LibriSpeech words are cut from read sentences, not said as commands,
which is why so few are caught even alone.

### Catches at 0.9, one step, under a held-out generated voice

The other voice here is a generated voice, like the lamp's own.

| Word | Speakers | alone | 0 dB | -5 dB | -10 dB | -15 dB | 0 dB, suppressed | -5 dB, suppressed | -10 dB, suppressed | -15 dB, suppressed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| stop | **real people** | 629/736 (85 %) | 101/736 (14 %) | 59/736 (8 %) | 36/736 (5 %) | 15/736 (2 %) | 389/736 (53 %) | 307/736 (42 %) | 228/736 (31 %) | 145/736 (20 %) |
| stop | **generated voices** | 344/385 (89 %) | 63/385 (16 %) | 32/385 (8 %) | 17/385 (4 %) | 12/385 (3 %) | 236/385 (61 %) | 207/385 (54 %) | 162/385 (42 %) | 95/385 (25 %) |
| wait | **real people** | 222/263 (84 %) | 41/263 (16 %) | 18/263 (7 %) | 9/263 (3 %) | 5/263 (2 %) | 115/263 (44 %) | 93/263 (35 %) | 71/263 (27 %) | 46/263 (17 %) |
| wait | **generated voices** | 427/440 (97 %) | 113/440 (26 %) | 63/440 (14 %) | 27/440 (6 %) | 16/440 (4 %) | 316/440 (72 %) | 267/440 (61 %) | 206/440 (47 %) | 150/440 (34 %) |
| hold on | **real people** | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) | 1/2 (50 %) |
| hold on | **generated voices** | 358/375 (95 %) | 140/375 (37 %) | 57/375 (15 %) | 27/375 (7 %) | 15/375 (4 %) | 332/375 (89 %) | 312/375 (83 %) | 265/375 (71 %) | 184/375 (49 %) |

### Catches at 0.98, one step, under a LibriSpeech test reader

| Word | Speakers | alone | 0 dB | -5 dB | -10 dB | -15 dB | 0 dB, suppressed | -5 dB, suppressed | -10 dB, suppressed | -15 dB, suppressed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| stop | **real people** | 355/736 (48 %) | 39/736 (5 %) | 21/736 (3 %) | 18/736 (2 %) | 5/736 (1 %) | 146/736 (20 %) | 127/736 (17 %) | 89/736 (12 %) | 60/736 (8 %) |
| stop | **generated voices** | 244/385 (63 %) | 39/385 (10 %) | 29/385 (8 %) | 22/385 (6 %) | 11/385 (3 %) | 146/385 (38 %) | 120/385 (31 %) | 82/385 (21 %) | 58/385 (15 %) |
| wait | **real people** | 126/263 (48 %) | 6/263 (2 %) | 3/263 (1 %) | 2/263 (1 %) | 2/263 (1 %) | 52/263 (20 %) | 40/263 (15 %) | 23/263 (9 %) | 14/263 (5 %) |
| wait | **generated voices** | 352/440 (80 %) | 71/440 (16 %) | 42/440 (10 %) | 30/440 (7 %) | 22/440 (5 %) | 212/440 (48 %) | 183/440 (42 %) | 138/440 (31 %) | 96/440 (22 %) |
| hold on | **real people** | 0/2 (0 %) | 1/2 (50 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 0/2 (0 %) | 1/2 (50 %) | 1/2 (50 %) | 0/2 (0 %) |
| hold on | **generated voices** | 353/375 (94 %) | 124/375 (33 %) | 80/375 (21 %) | 56/375 (15 %) | 41/375 (11 %) | 306/375 (82 %) | 266/375 (71 %) | 226/375 (60 %) | 171/375 (46 %) |

A few generated phrases at 0.9, under a reader:

| Phrase | Alone | 0 dB | -10 dB | 0 dB, suppressed | -10 dB, suppressed |
|---|---:|---:|---:|---:|---:|
| Stop. | 23/23 | 9/23 | 3/23 | 14/23 | 11/23 |
| Stop it. | 19/19 | 4/19 | 2/19 | 13/19 | 9/19 |
| Lamp, stop. | 19/22 | 8/22 | 6/22 | 17/22 | 13/22 |
| Okay, stop. | 23/24 | 4/24 | 3/24 | 22/24 | 18/24 |
| Wait. | 16/16 | 6/16 | 4/16 | 12/16 | 9/16 |
| Hold on. | 25/26 | 12/26 | 2/26 | 25/26 | 19/26 |

### False alarms, and the threshold

Each row is one threshold and the number of steps in a row it must hold
for. The first seven columns are catches of real people's "stop", "wait"
and "hold on" under a reader; then hearings an hour where no target was said; then the
share of near-miss clips (all groups below) with any hearing.

| Score from / steps in a row | Real people, alone | Real people, 0 dB | Real people, -10 dB | Real people, -15 dB | Real people, 0 dB, suppressed | Real people, -10 dB, suppressed | Real people, -15 dB, suppressed | Readers alone, an hour | Generated voice alone, an hour | AMI meetings, an hour | Near misses with a hearing |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0.5/1 | 96 % | 40 % | 21 % | 15 % | 79 % | 62 % | 49 % | 57.92 (645 in 11.1 h) | 38.50 (36 in 0.9 h) | 62.60 (1506 in 24.1 h) | 1366/5945 (23.0 %) |
| 0.7/1 | 93 % | 31 % | 15 % | 10 % | 67 % | 50 % | 39 % | 27.21 (303 in 11.1 h) | 19.25 (18 in 0.9 h) | 29.06 (699 in 24.1 h) | 1005/5945 (16.9 %) |
| 0.8/1 | 91 % | 26 % | 12 % | 8 % | 59 % | 43 % | 32 % | 16.52 (184 in 11.1 h) | 10.70 (10 in 0.9 h) | 17.33 (417 in 24.1 h) | 850/5945 (14.3 %) |
| 0.9/1 | 85 % | 16 % | 6 % | 4 % | 48 % | 31 % | 23 % | 8.98 (100 in 11.1 h) | 3.21 (3 in 0.9 h) | 7.36 (177 in 24.1 h) | 560/5945 (9.4 %) |
| 0.95/1 | 73 % | 10 % | 4 % | 2 % | 34 % | 21 % | 15 % | 4.94 (55 in 11.1 h) | 2.14 (2 in 0.9 h) | 3.24 (78 in 24.1 h) | 338/5945 (5.7 %) |
| 0.98/1 | 48 % | 5 % | 2 % | 1 % | 20 % | 11 % | 7 % | 2.16 (24 in 11.1 h) | 1.07 (1 in 0.9 h) | 1.04 (25 in 24.1 h) | 129/5945 (2.2 %) |
| 0.99/1 | 33 % | 3 % | 1 % | 0 % | 14 % | 6 % | 3 % | 1.35 (15 in 11.1 h) | 0.00 (0 in 0.9 h) | 0.37 (9 in 24.1 h) | 56/5945 (0.9 %) |
| 0.995/1 | 19 % | 1 % | 0 % | 0 % | 7 % | 4 % | 2 % | 0.90 (10 in 11.1 h) | 0.00 (0 in 0.9 h) | 0.21 (5 in 24.1 h) | 21/5945 (0.4 %) |
| 0.5/2 | 95 % | 35 % | 18 % | 13 % | 73 % | 55 % | 44 % | 37.72 (420 in 11.1 h) | 27.81 (26 in 0.9 h) | 42.23 (1016 in 24.1 h) | 1001/5945 (16.8 %) |
| 0.7/2 | 90 % | 27 % | 12 % | 8 % | 60 % | 43 % | 33 % | 17.42 (194 in 11.1 h) | 13.90 (13 in 0.9 h) | 18.33 (441 in 24.1 h) | 691/5945 (11.6 %) |
| 0.8/2 | 86 % | 21 % | 9 % | 6 % | 50 % | 35 % | 26 % | 10.15 (113 in 11.1 h) | 4.28 (4 in 0.9 h) | 10.35 (249 in 24.1 h) | 512/5945 (8.6 %) |
| 0.9/2 | 72 % | 12 % | 5 % | 3 % | 38 % | 25 % | 18 % | 5.30 (59 in 11.1 h) | 2.14 (2 in 0.9 h) | 4.28 (103 in 24.1 h) | 280/5945 (4.7 %) |
| 0.95/2 | 57 % | 7 % | 3 % | 1 % | 26 % | 15 % | 11 % | 3.14 (35 in 11.1 h) | 1.07 (1 in 0.9 h) | 1.87 (45 in 24.1 h) | 145/5945 (2.4 %) |
| 0.98/2 | 36 % | 3 % | 1 % | 0 % | 15 % | 8 % | 4 % | 1.53 (17 in 11.1 h) | 1.07 (1 in 0.9 h) | 0.67 (16 in 24.1 h) | 43/5945 (0.7 %) |
| 0.99/2 | 24 % | 1 % | 0 % | 0 % | 9 % | 4 % | 2 % | 0.99 (11 in 11.1 h) | 0.00 (0 in 0.9 h) | 0.33 (8 in 24.1 h) | 15/5945 (0.3 %) |
| 0.995/2 | 13 % | 0 % | 0 % | 0 % | 4 % | 2 % | 1 % | 0.63 (7 in 11.1 h) | 0.00 (0 in 0.9 h) | 0.17 (4 in 24.1 h) | 6/5945 (0.1 %) |

**Proposed: 0.9, one step**, used as a pause the duplex rules confirm or
undo (see the wiring). A false alarm then costs about a second of
silence. If the lamp's voice after its canceller sets it off as these
voices did, that is three to nine such pauses an hour of the lamp
speaking (the generated voices and the readers); that is the first thing
to measure on the lamp. **Stricter: 0.98, one step**, if a hearing is to stop the lamp
outright: about one to two false alarms an hour of a voice alone, for half
the words heard alone and a fifth through the suppressor at 0 dB. Two
steps in a row instead of one cut false alarms by 30 to 40 % and words
caught by 15 to 25 %. `lamp-name check` reports hearings at 0.9,
its fixed threshold, which is the proposed one.

### Near misses

Each clip alone at -20 dBFS. "stopped", "waited" and the rest from MSWC
are one second cut out of a read sentence around that word; from Piper
they are whole sentences.

| Near miss | Clips | With a hearing at 0.9/1 | With a hearing at 0.98/1 |
|---|---:|---:|---:|
| libri: hold | 14 | 1 (7 %) | 0 (0 %) |
| libri: stopped | 11 | 3 (27 %) | 2 (18 %) |
| libri: stopping / stops | 5 | 1 (20 %) | 0 (0 %) |
| libri: waiting / waited | 19 | 4 (21 %) | 0 (0 %) |
| mswc: hold | 229 | 8 (3 %) | 3 (1 %) |
| mswc: holding | 193 | 0 (0 %) | 0 (0 %) |
| mswc: other words, one each | 2387 | 87 (4 %) | 16 (1 %) |
| mswc: stopped | 236 | 131 (56 %) | 28 (12 %) |
| mswc: stopping | 37 | 19 (51 %) | 6 (16 %) |
| mswc: stops | 29 | 14 (48 %) | 2 (7 %) |
| mswc: waited | 80 | 48 (60 %) | 7 (9 %) |
| mswc: waiting | 175 | 75 (43 %) | 8 (5 %) |
| piper: bus stop | 149 | 5 (3 %) | 1 (1 %) |
| piper: don't stop | 156 | 4 (3 %) | 1 (1 %) |
| piper: hold | 150 | 0 (0 %) | 0 (0 %) |
| piper: listening | 200 | 0 (0 %) | 0 (0 %) |
| piper: sounds like | 166 | 60 (36 %) | 30 (18 %) |
| piper: stop in a sentence | 156 | 12 (8 %) | 0 (0 %) |
| piper: stopped | 151 | 21 (14 %) | 8 (5 %) |
| piper: stopping / stops | 166 | 6 (4 %) | 2 (1 %) |
| piper: wait in a sentence | 154 | 38 (25 %) | 10 (6 %) |
| piper: waiting / waited | 152 | 7 (5 %) | 1 (1 %) |
| sc: other words, one each | 930 | 16 (2 %) | 4 (0 %) |

The near misses that hold the sound of a target inside a longer word
("stopped", "waited" cut from sentences) and the one-word look-alikes are
what it confuses most. Whole phrases with a target inside them ("Don't
stop.", "the bus stop") much less.

### In ordinary meetings

34 AMI meetings, 24.1 h of their headset mix as recorded. The targets said
in them, by the transcript, and how many began a hearing between the
word's start and 1 s after its end:

| Score from / steps in a row | stop (28) | wait (39) | hold on (22) |
|---|---:|---:|---:|
| 0.5/1 | 10 | 7 | 11 |
| 0.7/1 | 7 | 7 | 9 |
| 0.8/1 | 5 | 6 | 7 |
| 0.9/1 | 1 | 3 | 5 |
| 0.95/1 | 0 | 1 | 5 |
| 0.98/1 | 0 | 1 | 4 |
| 0.99/1 | 0 | 0 | 1 |
| 0.995/1 | 0 | 0 | 1 |
| 0.5/2 | 7 | 7 | 9 |
| 0.7/2 | 5 | 4 | 9 |
| 0.8/2 | 4 | 3 | 7 |
| 0.9/2 | 1 | 3 | 5 |
| 0.95/2 | 0 | 1 | 5 |
| 0.98/2 | 0 | 1 | 3 |
| 0.99/2 | 0 | 0 | 1 |
| 0.995/2 | 0 | 0 | 1 |

At 0.9, 177 hearings came away from any target said, 7.4 an hour. What was
said just before them is mostly ordinary words: "then if you need
something next up" (1.000), "to play around with it what's a good way of",
"is they've got this big s stop-list", "of um weight on". One that was a
target: "so it sounds like well well hold w on but" (0.994).

### How soon

From the end of the word (where its sound fades) to the step at which a
hearing begins, at 0.9, one step, under a reader. A negative time means
the score crossed before the very end of the word's sound. On the lamp,
each step's score comes 48 ms after the last sound it uses (when the
1,760-sample chunk is whole), plus the step's own time.

| Level | Caught | From the word's end to the hearing: median | 90th percentile |
|---|---:|---:|---:|
| alone | 1980 | -18 ms | 142 ms |
| 0 dB | 575 | 62 ms | 382 ms |
| -5 dB | 379 | 62 ms | 398 ms |
| -10 dB | 260 | 62 ms | 382 ms |
| -15 dB | 165 | 62 ms | 462 ms |
| 0 dB, suppressed | 1388 | 62 ms | 222 ms |
| -5 dB, suppressed | 1215 | 62 ms | 222 ms |
| -10 dB, suppressed | 994 | 62 ms | 302 ms |
| -15 dB, suppressed | 774 | 62 ms | 302 ms |

## What was tried, and what it showed

All on the same laptop, each model trained once (one seed). The test
columns are from a quarter of the test clips and a quarter of the readers
(`evaluate.py --every 4`), except for row 5. Catches are of real people's
words under a LibriSpeech test reader, at 0.9, one step; false alarms are
of test readers alone, an hour.

| Model | Trained on | Alone | 0 dB | -10 dB | -15 dB | 0 dB, suppressed | -10 dB, suppressed | Readers alone, an hour |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1. Flat, 20,000 steps | targets at every level from -20 to +6 dB; other words placed anywhere; the negatives it scored highest drawn more often from half way | 90 % | 38 % | 26 % | 17 % | | 55 %¹ | 106 |
| 2. Flat, 8,000 steps | as 1, targets 0 dB and over only | 94 % | 22 % | 12 % | 8 % | | 37 %¹ | 54 |
| 3. Flat, 8,000 steps | as 1, only the first four inputs after a word | 76 % | 36 % | 21 % | 16 % | | 50 %¹ | 83 |
| 4. Flat, 12,000 steps | other words cut and placed as the targets are; targets alone, through the suppressor, or plainly 3 dB and over; no extra draw of hard negatives | 90 % | 19 % | 11 % | 8 % | 57 % | 34 % | 26 |
| 5. As 4, 8,000 steps, negatives weighed 20 to 1 | | 68 %² | | | | 12 %² | | 6² |
| 6. As 4, with 25 h more talk to train against, 16,000 steps | | 88 % | 12 % | 6 % | 4 % | 42 % | 26 % | 16 |
| 7. As 6, the time filters (64 wide), 12,000 steps: **the model kept** | | 86 % | 16 % | 5 % | 2 % | 48 % | 27 % | 8 |

¹ Only -10 dB through the suppressor was measured then. ² On the
validation speakers (Speech Commands' validation list under the dev
readers kept back), not on the test sets; at the same score, model 4 gave
98 % alone, 53 % through the suppressor at 0 to 6 dB, and 29 false alarms
an hour there.

What each showed:

- **Words placed anywhere taught the model where a word sits, not what it
  is.** With model 2, one test utterance in ten scored over 0.9. The
  hearings came about a second after speech began after a pause,
  wherever that was: "well they could be worse off", "somehow i had grown
  nervous". So from 4 on every other word is cut and placed exactly as
  the targets are.
- **Drawing the hardest negatives more often made false alarms worse,
  every time**: in 1, on the validation readers at 0.98, from 10 to 43 an
  hour once the draw began. It was left off from 4 on.
- **A heavier weight on negatives buys quiet with recall** (5).
- **More talk to train against helped a little** (6 against 4): 16 false
  alarms an hour instead of 26 at 0.9, with the words caught falling too.
- **The time filters** (7), Conv over three embeddings at a time, gave as
  many words as 6 with half its false alarms at the same score, and fewer
  near misses (10 % of near-miss clips against 13 %). They are the model
  kept. They train about five times slower on the laptop's processor.
- **A word plainly under another voice is barely in these numbers.**
  wav2vec 2.0, a far larger recogniser than this, was asked what it heard
  in 15 of the training mixtures. Alone, it wrote "STOP", "WAIT", "HOLD ON
  A MINUTE". At 0 dB and -10 dB it wrote the reader's sentence, word for
  word, and at most a trace of the target ("STILL", "WA", "OLD"). Through
  the suppressor, where the other voice is taken out cell by cell, the
  same words come through far more often: the "suppressed" columns.

## Wiring it on the lamp: a proposal

`crates/name` and `crates/msgs` are not this task's to change, so this is
what the first builder would add. It is as small as it can be.

1. **The file.** `stop.onnx` goes beside the name's three models, in
   `/opt/lampOS/name/models/`. Nothing else is installed.
2. **A fourth model on the same numbers.** In `hear.rs`, load it with the
   same `load(path, &[1, CONTEXT, EMBEDDING])` as `name.onnx`. Where the
   sixteen embeddings are flattened for `name.onnx`, run `stop.onnx` on
   the same `flat` and give back its score beside the name's. That is
   exactly what the scratch check ran, so the scores are the ones in the
   tables. It costs 24 microseconds a step in the Linux ARM64 container on
   the Mac (20,000 runs under tract 0.23.8); the name's whole step is 9.2
   to 9.8 ms on the lamp.
3. **The rule.** The shape of `said.rs`, with two changes. A hearing is
   counted at the step where the score has been at or over **0.9** for
   **one** step, and it is said at once, at that step, not when the score
   falls again: a stop is wanted now. Then 1 s with no new hearing, as for
   the name. From the end of the word to that step took a median 62 ms
   under another voice, and 90 % within 0.46 s ("How soon"). On the lamp
   add the 48 ms until the chunk is whole and the step's own time.
4. **A switch, off unless set.** `--stop-least SCORE` on `lamp-name`. With
   it absent, the fourth model is not loaded and nothing changes.
5. **What it says.** A topic `stop/heard`, an event, carrying what
   `NameHeard` carries: `{ "score": f32, "ago_ms": u32 }`, `ago_ms` being
   how long before the message the run of high scores began. The type is a
   copy of `NameHeard` named `StopHeard`; message types are the third
   builder's (`crates/msgs`).
6. **Who acts on it.** The voice, the way it takes `name/heard` today
   (`NameIn` in `crates/voice/src/bus_out.rs`): a `StopIn` that, while the
   lamp's own reply is playing, pauses the speaker at once, as the duplex
   mode's own pause does, and lets the duplex rules decide: a person
   speaking, and the reply stops; nothing, and it goes on from where it
   paused ([duplex-listening.md](duplex-listening.md)). At any other time
   a hearing is passed over: a "stop" said to someone else while the lamp
   is quiet stops nothing, and the false alarms of ordinary talk measured
   here only matter while the lamp speaks.

## Wired, off unless set (#828, 18 October)

The proposal above, as built on `sixth/stop-model`:

- **`lamp-name --stop-least SCORE`** loads `stop.onnx` from `--models`
  beside the other three, refusing a file without the pinned SHA-256
  (`hear::STOP_SHA256`), runs it on the same sixteen embeddings every
  80 ms, and publishes `stop/heard` (`lamp_msgs::StopHeard`: `score`,
  `ago_ms`) at the step the score first reaches the bar, then nothing new
  for 1 s (`said::Stops`). It logs each hearing and `stops=` on the minute
  line. Without the option nothing is loaded and nothing changes.
- **`lamp-name check MODELS --stop-least 0.9 FILE…`** prints
  `FILE stop SECONDS SCORE` for each hearing it would publish: the tool for
  measuring it on replays before any lamp run.
- **`LAMP_STOP_HEARD=on`** (with the bus) in the voice: a hearing while the
  reply sounds (`reply_sounding`) makes the next block with something to
  yield begin a yield (`Interruption::force_yield`), whatever the duplex
  hearing says of it, **as heard plainly**: by the time `stop/heard` comes
  the word is over, and quiet after it commits the yield, so the lamp stops
  (the reviewer on #1555; proposal 6's pause-and-resume would have resumed
  after 0.4 s of quiet). A person going on still commits it; the deadline
  still resumes it. A forced yield that resumes is not counted as one of
  the reply's fruitless yields, and the reply's fallback after two of those
  does not drop a forced one, so "stop" said three times stops it the
  third time too. A false hearing then costs the reply, which is why the
  false stops an hour are measured before the switch goes on. A hearing at
  any other time yields nothing. Each is a `stop_heard` trace line with its
  score, whether the reply sounded, and whether the yield was taken.
- `stop.onnx` is in no models release yet, so `deploy/product/models.json`
  does not fetch it; it is placed by hand beside the name's models until
  it is.
- Tests: `said::tests::a_stop_word_is_said_at_the_step_it_reaches_the_bar_and_once_a_second`,
  `duplex::tests::a_forced_yield_begins_at_the_next_block_and_quiet_commits_it`, `coordinator::tests::duplex::a_stop_heard_by_the_listener_ends_the_reply_though_only_quiet_follows` (the reply ends on the forced commit: the coordinator cancels it as the hand's Stop, and the speaker is held from the commit to the cancel), `hear::tests::only_the_pinned_stop_model_is_taken`, `the_stop_bar_is_read_in_the_program_and_in_check`,
  the parse of `--stop-least`, and `lamp_msgs`' `StopHeard`.

## Where it is most likely to be wrong on the lamp, and how to see it quickly

In the order I would look:

1. **The canceller's leftover of the lamp's own voice.** The false alarms
   here were counted on clean voices. The leftover after AEC3 is a
   broken, gated copy of the lamp's voice; it may set the listener off
   more often, or less. With the voice publishing its sound
   (`LAMP_BUS_AUDIO=on`), let the lamp talk for ten minutes with nobody
   speaking and record it:

       lamp-bus record /var/tmp/stop-own mic2/audio --seconds 600

   Then, on the Mac, from `lampOS/training/stop/`:

       python from_bus.py /var/tmp/stop-own/mic2_audio.jsonl own.wav
       python listen.py ../../models/stop/stop.onnx own.wav --least 0.9 --run 1

   Each "heard" line is a false alarm in ten minutes of the lamp's voice.
   One or two is what the voices here gave; many more, and the threshold
   has to go up (the table under "False alarms, and the threshold").
2. **What the canceller leaves of a person.** Record the same way while
   the 2026-10-14 words ("Stop.", "Lamp, stop.", "Wait.", "Okay, stop.")
   are played four seconds into a story, then while a person says them.
   `listen.py` prints each hearing with its time. The "suppressed" columns
   are the guess for this; the plain columns are what to expect if the
   canceller leaves the lamp's voice whole.
3. **The microphone chip, and played words.** A loudspeaker's "Stop." is
   not a person's, and the second microphone's chip treats a live voice
   differently ([duplex-listening.md](duplex-listening.md)).
4. **The lamp's own "wait" and "stop".** It says them. Its own word
   through the canceller is a near miss that was never tested here.
5. **Level.** Training put the other voice at -30 to -10 dBFS and words
   from 20 dB under it to 6 dB over. The lamp's own voice reaches -9.6
   dBFS and touches the microphone's ceiling; clipped sound is outside
   most of what was trained.

**First look on lamp-4ace, 2026-10-14, 13:48 to 13:54 (the first
builder).** Seven played scenes of `shorts.sh`, each a story asked for and
then a short word from the Mac's loudspeaker about twelve seconds later,
were recorded from `mic2/audio` (after the canceller; voice in duplex
mode, no steady reference): 329.8 s. Played times were taken from the
scenes' `#PLAY_US` lines, on the same clock as the recording. `lamp-name
check` on the lamp, with `stop.onnx` as `name.onnx`, over the whole file
and over a window from 2.5 s before each word to 3 s after:

| Played word | Lamp's reply playing | Level before / loudest 250 ms (dBFS) | Highest score |
|---|---|---|---|
| "Stop." | yes | -36.0 / -18.5 | 0.24 |
| "Lamp, stop." | yes | -19.6 / -11.3 (touches the ceiling) | 0.36 |
| "Stop!" | no (the model's connection had failed; no story) | -75.6 / -13.4 | 0.54 |
| "Lamp, stop talking." | yes | -29.2 / -22.1 | 0.12 |
| "Stop." | yes | -68.3 / -63.1 | 0.34 |
| "Wait." | no (its reply had ended) | -85.6 / -22.4 | 0.04 |
| "Mm-hm." (not a target) | yes | -65.9 / -17.9 | 0.10 |

- **No word was heard.** None of the four "stop" that reached the
  recording, nor the "wait", came near 0.9. The plainest case, "Stop!"
  at -13.4 dBFS with the room quiet and the lamp silent, reached 0.54.
  The fifth "stop" never reached the recording: the stream stayed under
  -63 dBFS through it, so it says nothing about the model.
- **Two false alarms**, 0.94 and 0.92, in 5.5 minutes. Both began while a
  played request ("Hey Lamp, tell me a story about …") was being said;
  none came during the lamp's own voice.
- **A step took 6.98 ms** over the whole file on the lamp (4,098 steps).
- **What it means.** One look, of played words from one loudspeaker: not
  a rate. But it gives no reason to wire the listener now, and tonight's
  stop work goes on without it. What would change that: a person's live
  "stop" at the lamp, recorded the same way, and the same played words
  with the steady reference on.

**On the lamp, with no code change**, the lamp's own code and tract can run
the model over recordings: put `melspectrogram.onnx`,
`embedding_model.onnx` and `stop.onnx` renamed `name.onnx` in a directory
of their own and run `lamp-name check THAT_DIR own.wav`. It prints each
hearing at 0.9 (its threshold is fixed) and the highest score, and the
time a step took on the lamp.

## How to make it again

From `lampOS/training/stop/`, on a Mac or Linux machine with `uv`, about
45 GB of free disk for a while, and a network:

    sh make.sh

It downloads into `lampOS/.cache/stop/` (git-ignored) and checks each file
against [`downloads.sha256`](../training/stop/downloads.sha256), then:
unpacks (`unpack.py`); generates speech (`make_speech.sh`, `speak.py`);
aligns, cuts and splits by speaker (`clips.py`); builds the inputs
(`make_inputs.sh`, `make_data.py`); trains (`train.py`, which with no
options is this model's configuration); measures (`evaluate.py`) and
prints the tables (`tables.py`). Every step runs at the lowest priority
with two to six threads. On the M2 Max, shared with another session,
the generated speech took about an hour and a half, the alignment about
an hour, the inputs about an hour, training the kept model 30 minutes on
three threads, and the full measurement 32 minutes on six.

To look at a few files without the rest: `python listen.py
../../models/stop/stop.onnx FILE.wav --least 0.9 --run 1` prints each
hearing and the highest score; `--scores` prints every score.
`python from_bus.py` turns a `lamp-bus record` of `mic2/audio` into a WAV
file for it.

## Step 2: the request's first words (16 October)

A yield's request now begins at the person's first words said under the
lamp's voice, with `LAMP_FIRST_WORDS=kept` ([duplex-listening.md](duplex-listening.md#first-words-kept-828)).
It doesn't make a short stop word stop the lamp. A pause has no words to
judge, so that needs words heard during the pause. That's the next
section.

## A stop word heard in a yield (#828, step 2, 16 October)

Off unless `LAMP_STOP_WORDS=heard` in the voice's environment, with
`--turn-taking duplex` and the second voice's configuration
(`LAMP_VOICE_SECOND_CONFIG`); built by the fourth builder
(`crates/voice/src/stop_words.rs`).

The duplex rule stops the lamp when someone is plainly over it, and then
knows only sound and timing. A short "Stop." is over by the time the lamp
has stopped, so the rule took it for "mm-hm" and resumed. On lamp-4ace on
16 October at 05:39, two of four short stop phrases didn't stop the story.

With the switch:
- **A stream ready when a reply begins.** The coordinator opens a stream on
  the second voice's speech-to-text, through the relay on lamp-4ace, on a
  thread of its own (`RelayEars`). Connecting took 0.6 to 0.7 s on
  lamp-52e6, so it's done before it's needed. The stream is kept with
  `KeepAlive`, and closed when the reply ends.
- **A yield's audio to it.** The audio held for the yield, then each block
  while it is in force. The rule now waits for the words: stillness doesn't
  resume the reply until they come (`Interruption::await_words`). Its
  1.1 s deadline still resumes it whatever was heard.
- **The words asked for after 100 ms of stillness**
  (`FINALIZE_AFTER_STILL_BLOCKS`). Through the relay they came about 0.25 s
  after a stop phrase ended (the sixth builder, #828, 08:22), inside the
  400 ms of stillness after which the rule resumes. A block counts as still
  only 300 ms after the lamp stops (`clear_after_blocks`), so for a "Stop."
  that ends as the lamp yields the commit comes about 0.65 s after it: the
  lamp is silent from the yield, and only the decision comes later (the
  reviewer on #970).
- **The words wait for `CloseStream` when `Finalize`'s answer is short**,
  whatever is left after it (`SttStream::finished`). A request's line may
  end on the answer when what it leaves is quiet ([second-voice.md](second-voice.md),
  #1993), but over the lamp the canceller can take a stop word under any
  level bar: 51 of 88 had nothing over -28 dBFS left (#828).
- **A hand Stop, a head touch, mute or privacy closes it** (`close`): the
  duplex rule's yield, which a hand Stop leaves in force, is let be until
  it ends, not begun again and called unheard (the reviewer on #970).
- **A stop phrase commits the yield** (`stop_phrase`: "stop", "wait",
  "enough", "pause", "quiet", "hush", "shush", "hold on", "hang on", alone
  or among other words, but not right after "don't", "not" or "never":
  "Don't stop" is no stop, the reviewer on #970). The reply ends as for any interruption, and what
  was said goes to the provider, which answers ("Okay."). Any other words
  let stillness resume the reply as before, and no words resume it at the
  deadline.
- `duplex_policy` records `stop_words_after_still_blocks`. Each yield's
  outcome is recorded as `stop_words`, with `stop` and how many words, or
  `words: null` when it ended before they came; the words themselves aren't
  recorded. The journal says the same in one line a yield ("a yield's words
  came: 1 word, a stop phrase", "a yield ended before its words came").
  Counted against the played cut-ins, these show how often a one-word stop
  made the lamp yield at all: that half is the canceller's, not this one's
  (the first builder, 09:49: a lone "Stop." yielded 0 of 3 that morning).

### Privacy: what goes where

The first builder accepted this on lamp-4ace on 16 October at 09:49 (#828).
It is to be decided again before home lamps ship.
- **What:** the audio of a yield only. That's what was held for it (300 ms
  before its onset, or with `LAMP_FIRST_WORDS=kept` up to 1.6 s), then each
  10 ms block while it is in force, until the words are asked for. Usually
  a second or two said over the lamp's voice, after the echo canceller.
  Nothing of a reply with no yield, and nothing when the lamp is not
  speaking.
- **Where:** the second voice's speech-to-text, as `LAMP_VOICE_SECOND_CONFIG`
  configures it. On lamp-4ace that is the company relay
  (`stt_base_url`, with Deepgram's `nova-3` behind it), with the credential
  file the configuration names. It is the same service the second voice
  uses when it answers, so with this switch it also hears yields while
  Gemini is the active route.
- **When the stream is open:** from the start of each reply to its end. It
  is opened before it's needed (connecting takes 0.6 to 0.7 s) and carries
  no audio until a yield.
- **When it is closed:** at a reply's end; at a hand Stop, a mute or a
  capture gap (`hand::cancel`); on any tick where the microphone is not
  allowed, which covers the privacy switch; and on every way out of the
  conversation, the physical mute's and a lost privacy reading's among
  them, as the ears are dropped there.
- **How it is closed** (the reviewer's P1 on #970): apart from the queue of
  audio to the relay's thread. Each yield has a number, carried by every
  block and every result. Forgetting the yield, resting or dropping the
  ears changes the live number at once and wakes the thread past its
  queue. The thread checks the number under its lock as it hands each
  block to the stream, so once that has returned nothing more of the yield
  is handed on, however much is still queued. It then drops the yield's
  stream, which ends the socket's writer; audio handed on before the close,
  captured while the microphone was allowed, may still finish going. A
  block captured more than 3 s before it would be handed on never is.
  Dropping the ears waits up to 250 ms for the thread to end.
- **Whose words:** a result is taken only for the yield that asked for it
  (the reviewer's P2 on #970). Words that come for a yield after it was
  forgotten are dropped, never taken for the next yield's.
- **What is kept on the lamp:** the outcome per yield (`stop_words`, no
  words). Whatever the relay keeps is the relay's.
- **Not covered: `LAMP_WAIT_KEEPS_ANSWER=words`** (#1123 step 2,
  `group-talk.md`) sends something else to the same relay, through the same
  driver (`RelayEars::for_waits`):
  - **what:** the audio of **any sound** the detector starts on in the 3 s
    after a person's request ends, while its answer hasn't been heard yet
    and the lamp is silent. That is asked or not: the person's own "No.", a
    cough, a television, or a colleague's words to someone else. It goes
    from its 300 ms of pre-roll, each block once, until its words are asked
    for (100 ms after it stops) or until it is decided (at most 1 s after
    it set the detector off). So at most about 1.3 s of a sound.
  - **to which service:** the second voice's speech-to-text, as
    `LAMP_VOICE_SECOND_CONFIG` names it: Deepgram itself, or a relay at its
    `stt_base_url`. On lamp-4ace that is the company relay, with Deepgram's
    `nova-3` behind it, the same service as the stop words.
  - **when its stream is open:** in those 3 s while the answer is unheard,
    and while a sound is in hand; nothing is sent while no sound waits.
  - **closed:** as the stop words' stream is. **Kept:** `wait_words`, with
    a count and no words.

  The decision above doesn't cover it.

Tests:
- `stop_words::tests`: the phrases, and the driver: the held audio and each
  block until the words are asked for, asked once, taken once, forgotten
  when the yield ends; a yield without words unheard; closing drops the
  yield and the stream once, and the next reply has one ready again;
- `lamp-audio` `duplex::tests::a_yield_waiting_for_its_words_commits_on_a_stop_word_and_otherwise_resumes`
  (the rule);
- `coordinator::tests::duplex::a_stop_word_heard_in_a_yield_commits_it_and_other_words_resume_it`,
  through the production hearing, yield and commit, with scripted words. A
  stop phrase commits at the eleventh still block. Other words resume at the
  same block as without the switch. No words resume at the deadline, 110
  blocks from the yield.

- through the relay's real thread against a stand-in speech-to-text on an
  in-memory socket: `audio_queued_for_a_yield_never_goes_once_it_is_forgotten_rested_or_dropped`
  (a yield's audio queued while the thread is held never reaches the
  service once forgotten, rested or dropped; after a forget the next
  yield's goes; a block captured 4 s before is never sent) and
  `words_that_come_for_a_yield_after_it_is_forgotten_are_not_the_next_ones`
  (the first yield's words, held by the thread until after it was
  forgotten, are not the next yield's; its own are).

Not shown: the relay on a lamp in this path, and how long the pause grows
when the words are slow (at most to the deadline). The thread's retry after
a failed connection is not tested.

## A stop word heard over the lamp's voice (#828, 16 October)

Off unless `LAMP_STOP_WORDS=heard-always` in the voice's environment, with
`--turn-taking duplex` and the second voice's configuration; built by the
fourth builder (`crates/voice/src/stop_words.rs`, `Listen::Always`).

**Why** (the evidence is in
[verification/stop-yield-20261016](verification/stop-yield-20261016/README.md)):
with `heard`, words are heard only once the lamp yields, and a short "Stop."
said under the lamp's voice mostly makes no yield. In the first builder's
two recordings of 16 October at 01:51, replayed with `lamp-audio
replay-aec` and scored with `hearing_replay`, 2 of 9 cut-ins said over the
lamp made one ("Wait." and "Lamp, stop talking."). In the speech-like
blocks over the other seven, the lamp's own sound explained a median 96 to
99% of the microphone, so the rule took them for its own voice. No sign
per block told the two apart there: the residual after the canceller rose
47 to 74 dB over most cut-ins, but as much between the lamp's own words
(7 such blocks in a row at 20 dB fired 22 times in 143 s of the lamp
alone), and the canceller took off a median 13.7 dB over the cut-ins and
17.4 dB with the lamp alone.

With the switch:
- **Everything heard while the lamp speaks goes to the stream:** every
  block while the lamp's sound still reaches the microphone, or while it
  has stopped for someone, from the reply's first block to its end. A
  stream is asked for at the reply's first sound from the provider and
  closed when the reply ends. A reply held, or never answered, opens none
  (the first builder at 15:29: eleven in two minutes of office noise). The
  0.6 to 0.8 s the stream takes to connect overlaps the start of playout,
  and up to 3 s heard meanwhile is kept for it.
- **The words are read as they come:** each result the service gives,
  interim ones among them (`SttStream::take_words`). No stream is asked to
  finish.
- **A stop phrase the lamp did not say itself stops the reply,** once a
  reply (`stop_over`). What the lamp says, as its words are written
  (`LAMP_WORDS_US`, the last 6 s), is its own: the canceller lets its
  voice through, and "the fox had to wait" in a story stops nothing. The
  reply stops as for a hand Stop (`hand_control_cancel`, reason
  `spoken_stop`): the lamp falls silent and listens, and nothing is sent to
  the model. It is one of the local cancels (`receipt_history::LOCAL_CANCELS`,
  #1024), so the speaker's discard of the rest of the reply's audio is
  expected and ends nothing, as for a head tap (the first builder's P1 on
  #1047).
- **`LAMP_STOP_WORDS_HOLD=own-words`** (off unless set; #828 section 2):
  a stop heard is acted on only once the lamp's own words cover the block
  its result arrived in, or `HOLD_US` (150 ms, on the blocks' capture
  clock) after that block, whichever is first (`StopWords::with_hold`).
  The block is the one the relay's result arrived in, so after the words
  were said, by the relay's answer; the 150 ms is counted from there,
  which is what a person's stop is delayed by. Without it, at a reply's
  start with nothing queued, the lamp's text for its own "stop" can come
  after the speech-to-text's words for it, and its own "stop" stopped it.
  - **What covers it:** only a word of the lamp's stamped ahead of when it
    was written (the reply's audio queued before it), at or after that
    block. Text stamped as it was written can be a piece for audio played
    before the stop, since transcripts come in pieces, so it ends no hold
    (the reviewer on #1353 at 17:34: "And the owl said:" then "stop,
    little fox." 60 ms late).
  - At each block of the hold, a stop whose words the lamp's own then hold
    is let go ("a stop phrase heard was the lamp's own, let go after N ms
    held"). One that acts says so ("a held stop phrase acts after N ms
    held"). Several stops heard during one hold are judged together, from
    the first's block.
  - **Its delay:** a person's stop acts 150 ms later at most, measured on
    the 10 ms block clock in `a_persons_stop_is_held_at_most_150_ms`. It
    is 0 when the lamp's words are already stamped ahead (audio queued),
    and the full 150 ms otherwise, which is at a reply's start.
  - **Own words later than 150 ms** still stop the lamp, as without the
    hold: a person's stop is never held longer or dropped, and a false one
    ends the reply as a hand Stop does, with the lamp listening.
  - A reply that ends, or a microphone that closes, during the hold lets
    the held stop go.
  - **Counted:** with `LAMP_STOP_WORDS_COUNTS`, `reply.held` lists each
    held stop, with `waited_ms` and whether it `acted`. The end-of-reply
    line adds ", N stops held". A lamp run then shows whether the 150 ms
    is ever what acts (the first builder on #1353, 17:21).
- **A yield takes its usual course,** without waiting for words.
- `duplex_policy` records `stop_words_always`.

**Its privacy cost:** everything the microphone hears while the lamp
speaks goes to the second voice's speech-to-text (the company relay on
lamp-4ace), the lamp's own voice let through by the canceller with it, not
only a yield's second or two. That is beyond what the first builder
accepted for `heard` at 09:49, so it is a separate value, off unless set.
The stream is closed on every way out, as with `heard`.

**Its latency:** about 0.25 s from the end of a phrase to its words
through the relay (the sixth builder, #828, 08:22); interim words can
come sooner.

**The journal**, counts only, never the words:
- At start, "stop words: heard-always, …" with the speech-to-text's host,
  or why it isn't heard.
- Each stream: "a stream is ready, connected in N ms", or why not.
- Each reply's end, or its close by a stop: "the reply's N blocks went to
  the speech-to-text, its words asked for at P pauses, M results came
  back", with ", a stop phrase among them" when one stopped it. Then, from
  the stream's own thread, "the stream took N blocks and dropped M (too
  old, or a backlog), and was opened again K times; the last sent back A
  results with words, B without, C speech starts, D other messages", and
  how it ended.
  Those tell a service that said nothing from one whose results held no
  words, and both from results that weren't read.

**On lamp-4ace at 15:27** (the first builder, #1047): a played story
with "Lamp, stop." and then "Stop." said over it. "The reply's 455 blocks
went to the speech-to-text, 0 results came back", and no yield either
time. The stream connected, and the audio went to it. Nothing came back
that the stop words read.

**What `stt-check` showed** (the first builder on lamp-4ace, 16:59): a
short "Stop." (461 ms) got no interim result at all, only a speech start.
Its words came only from `Finalize`, 269 ms after its audio. A longer "Lamp,
stop talking please." (2,040 ms) got interims, and its final came from
`Finalize` 225 ms after its audio. heard-always never asked a stream to
finish during a reply, so 33 replies with sound since 16:20 had 0 results.

**So the words are asked for at each pause** (`FLUSH_AFTER_STILL_BLOCKS`,
200 ms): while the lamp speaks, after speech-like blocks (the coordinator's
detector at 0.5 or more), 20 blocks that aren't ask the stream for the words
heard so far with `Finalize`, and the stream goes on (`SttStream::flush`,
in order with the audio). Pauses between the lamp's own words ask too, and
their words are the lamp's, checked as before. How soon a "Stop." would
then stop the lamp is not measured: by the relay's 269 ms after the ask,
about 0.5 s after it's said, and with the half-second asks of the raw
source up to about 0.8 s. The end-of-reply line says how many pauses
asked.

**On lamp-4ace at 20:49 with that** (the first builder, #1047, 20:58):
"Lamp, stop." and "Stop." played over a story. The pauses asked for words
13 and 9 times, 0 results came back, and there was no yield. So:
- **How a stream ended is kept** (`Replies::ended`): the service's close
  code and reason, the socket ending with no close, or a failure. It is in
  the thread's end-of-reply line ("…; the stream was closed by the
  service, code N …").
- **A stream that ends during a reply is replaced at once.** The journal
  says "the stream ended during the reply after it sent back …; another is
  asked for", and what is heard while it reconnects (up to 3 s) goes to the
  new one. If the relay closes a stream once it has answered a `Finalize`,
  as it would `CloseStream`, every pause after the first was talking to
  nobody. The end-of-reply line says how many times one was opened again.
- **`LAMP_STOP_WORDS_AUDIO=/run/…/file.wav`** (an absolute path, off
  unless set): the last 10 s of each reply's stream audio, exactly as sent,
  written there at the reply's end (16 kHz mono 16-bit), for `stt-check`.
  Written over at each reply; deleting it is for whoever set it.
- **`LAMP_STOP_WORDS_COUNTS=/run/…/stop-words.json`** (an absolute path,
  off unless set; with `heard-always`): each reply's counts as JSON,
  written over at its end. Under `reply`: the blocks sent, the pauses its
  words were asked for at (with the raw or linear source, every half-second
  ask among them, `asked_every_ms`), the source heard (`source`:
  `processed`, `raw` or `linear`), the results, whether a stop phrase was heard,
  and what results heard (200 bytes each), each with its number in the
  reply (`n`, from 1) and whether it stopped the lamp: the reply's **last
  16 results**, and before them **every result whose words hold a stop
  phrase**, however early (the first 16 that do), in the order heard.
  Those are judged on the words alone, so the lamp's own "stop" or "wait"
  heard back through the canceller pins a place too: a story with 16 of
  its own early on can push a person's later stop out of the pinned ones,
  though not out of the last 16 (Second on #1306). A
  stop said 9 s into a story fell after the first 16 results the file
  kept before, so whether it came back could not be read (Fourth and the
  first builder on #828, 17 October 13:18 and 13:42). Under `stream`, added by the stream's thread as it
  rests: the blocks it took and dropped, how often it was opened again,
  and the last stream's results with and without words, speech starts,
  other messages and how it ended. On lamp-4ace on 17 October the journal
  could not be read after midnight's log sync, so a stop from the stop
  words could not be told from one by the duplex yield (the first builder
  on #1047). It holds what was said over the lamp's replies: for played
  tests.
- **`LAMP_STOP_WORDS_SOURCE=raw`** (off unless set; unset or `processed`
  is as before): the stream hears each block as the microphone recorded
  it, before the canceller, not the canceller's output the provider hears.
  On lamp-4ace on 16 October at 22:44 the canceller's output gave 16 and
  18 speech starts and no words at all over two replies with "Lamp, stop."
  and "Stop." played over them: when the lamp's own voice reaches the
  microphone far louder than the person, the canceller suppresses both
  (`lamp_audio::duplex`), so a short word said over the lamp is gone
  before the speech-to-text gets it (the first builder on #1047). The
  capture worker is asked for the raw blocks (`--raw-blocks`, only then)
  and sends each beside the cancelled one (`MicrophoneFrame::raw`).
  - The lamp's own words are in the raw blocks too. A stop word in what
    the lamp is saying is left out as before (`stop_over`), so "stop" in
    its own story stops nothing.
  - **The words are asked for every half second while the lamp sounds**
    (`CADENCE_BLOCKS`, 50 blocks sent; `StopWords::with_cadence`), as well
    as at each pause after speech. The pause is the detector's, and the
    detector scores the canceller's output, which over the lamp holds no
    speech: `pauses_asked` stayed near 0 and a "Lamp, stop." said over it
    was never asked for (the first builder at 02:29 and 02:57 on #1047).
    A pause after speech starts the half second again. Words said across
    an ask come back in two results: a stop word is one word, but "Hold
    on." split there is missed. The counts file then says
    `asked_every_ms: 500`.
  - `LAMP_STOP_WORDS_AUDIO` then writes the raw blocks, which is what the
    stream was sent.
  - With `heard` (yields only), a yield's first, held blocks are still the
    canceller's output: only what follows is raw.
  - Not offered: mic1. The first builder's recording there (22:55) had
    the played request only 8 dB over the floor, and no words even in the
    request. (The canceller's linear output, which could not be read out
    when this was written, is `linear` below.)
  - **Played three times, and no person's stop came through.** On
    lamp-4ace on 17 October at 01:46 (the first builder), 8 of 10 cut-ins
    stopped the story against 7 of 10 on the canceller's output, mostly by
    Gemini's own yield, and "Lamp, stop." alone never did. On lamp-52e6 on
    17 October at 06:53 and 07:20 (the sixth, the Mac's speaker at 45, 70
    and 90), the stream's words were the lamp's own story every time,
    never the person's, and the stop words stopped it 0 times: from raw
    mic2 the lamp drowns a person 1 to 2 m off. Louder, Gemini's yield
    took over (4 of 5), which isn't a stop. Next (the first builder, 07:07):
    the canceller's linear output, before its residual suppressor.
  - **On lamp-4ace on 19 October, 00:46–00:52** (`cfce52401`, ten played
    cut-ins), the stream's results were the lamp's own story word by word.
    "Stop." was heard once, as `"Stop. He"`, and stopped the lamp; "Lamp,
    stop." was never written (the first builder on #828, 01:29).
- **`LAMP_STOP_WORDS_SOURCE=linear`** (off unless set; #828, the first
  builder at 01:29 on 19 October): the stream hears the canceller's linear
  stage, before its residual-echo suppressor. That is the microphone,
  high-pass filtered, less the echo the canceller's adaptive filter
  predicts (`EchoProcessor::with_linear_output`).
  - **The trade it is for:** the canceller's output suppresses a person's
    short word along with the lamp's voice. The raw block keeps the word
    but carries the lamp's voice loudest, so the speech-to-text returns the
    lamp. Offline, Sixth's model on `aec_linear` caught about as many as on
    `aec_only` (the first builder, 01:29).
  - **It keeps much of the lamp's voice too.** On lamp-4ace's mic2 the
    linear stage took only 3.1 to 4.1 dB of it out, and a person 10 dB under
    the echo stayed 4 to 7 dB under (#828, 17 October 08:01;
    [serve-runtime.md](serve-runtime.md)). The scripted rig's test, where
    it is over 20 dB under the raw block, isn't a lamp's. Read the A/B
    expecting the lamp's voice in the stream.
  - **How it gets there:** the capture worker is asked for it
    (`--linear-blocks`, only then). It keeps the canceller's linear output,
    as it does for `LAMP_VAD_INPUT`, and sends each block's beside the
    cancelled one (`MicrophoneFrame::linear`). Once a minute it says on how
    many blocks it sent it.
  - **A block the canceller did not run for** (no reference yet, or none
    trusted) has no linear output. The stop words then hear the block
    handed on, which is the microphone's block as recorded.
  - **As with `raw`:** the words are asked for every half second while the
    lamp sounds, and `LAMP_STOP_WORDS_AUDIO` writes what the stream was
    sent. With `heard`, a yield's first, held blocks are still the
    canceller's output.
  - **Which source a reply heard** is in its counts (`"source"`:
    `processed`, `raw` or `linear`) and in the journal's start line.
  - **Timing:** the linear output is one canceller block (4 ms) ahead of
    the canceller's output.
  - **Cost (rule 7), on an x86_64 Mac (i7-10700K, release build):**
    - keeping the linear output adds 0.5 to 0.9 µs to the canceller's
      ~106 µs a block (three runs of 6,000 blocks, with and without
      alternating block by block);
    - each frame's 160 more samples take 1.3 µs to encode and add 856
      bytes, as the raw blocks do.

    Not measured on a lamp.

**`lamp-voice stt-check SECOND_CONFIG WAV`** is for finding out why, on
the lamp, with its own configuration and a recording made for the purpose
(16 kHz mono 16-bit; the words heard are printed). It streams the
recording in 10 ms blocks at its own pace, as the stop words do. Then,
1.5 s after the last block, it sends `Finalize`, and 2 s later
`CloseStream`. It prints everything the service sends back, by kind and
when (`Results` with `is_final`, `speech_final` and `from_finalize`,
`SpeechStarted`, and anything else), and the counts at the close. Run on
the canceller's output for a recorded reply with "Stop." in it
(`lamp-audio replay-aec`), and on a clear "Stop." beside it, it shows
whether results come at all, only after `Finalize`, or only at the close.

Tests: `stop_words::tests::a_stop_word_the_lamp_did_not_say_itself_is_heard_over_it`,
`listening_always_hears_the_lamps_whole_reply_and_stops_on_a_word_it_did_not_say`
(the driver) and `listening_always_gives_the_words_as_they_come_through_the_thread`
(the relay's real thread, against a stand-in speech-to-text that gives an
interim result), `with_the_raw_blocks_the_words_are_asked_for_every_half_second`,
`a_stop_that_closes_the_reply_is_in_its_counts`,
`the_lamps_words_play_after_the_audio_queued_ahead_of_them` (the stamp
the coordinator gives them, `plays_at_us`) and
`the_lamps_own_words_count_from_when_they_play`. The lamp's own words are
stamped with when they will play, the reply's queued audio ahead of them
(the voice holds up to 30 s of it): stamped as their text came, a story's
own "wait" read back from the raw microphone stopped the lamp (the
reviewer on #1047). `tests/capture_hold_worker.rs::stop_words_blocks_carry_what_was_asked_only_when_asked`
(the raw, linear and canceller's blocks, #828) is Linux only, in a binary the `pr` profile skips, and pushes to
`lamp-voice` use that profile too (`lampos-arm64.yml`). It runs in the
nightly full run at 02:00, or before then with `gh workflow run
lampos-arm64.yml --ref <branch> -f full=true`. The lamp's own words are
counted as its own from when their text arrives, even when they play up
to 30 s later. A person's "wait" early in a story whose later text says
"wait" is passed over, which costs a missed stop and never a false one.
Counting only words stamped at most a moment ahead would turn an
estimate that runs late into a false stop, since the play time is
estimated from the queued audio, not measured. On a stop, `close()`
writes the reply's counts before it revokes the stream, because the
stream's thread adds its own counts when it rests; with
`LAMP_STOP_WORDS_COUNTS` on `/run` that write takes well under a
millisecond.
Not exercised: the coordinator's loop taking the stop
(`spoken_stop`), and the relay with a person's voice under the lamp's.

## Not shown

- **The lamp.** No sound from the lamp's microphones went into training,
  and the only test on it is the one look of played words above. Its
  second microphone's chip treats a live voice
  differently from a played one, and no mixture made elsewhere copies it.
- **The canceller.** "Suppressed" here is a suppressor that is told the
  other voice exactly. AEC3 on the lamp estimates its echo from a
  reference whose timing moves ([duplex-listening.md](duplex-listening.md));
  its leftover is neither the plain nor the suppressed case here.
- **A person at the lamp.** Every voice was a recording or generated.
  Speech Commands and MSWC are people reading a word into their own
  microphones; nobody here was cutting in on a talking lamp, with the
  urgency that has.
- **"Hold on" from real people**: two test clips, and the 22 in the AMI
  meetings. The rest of "hold on" is generated, by held-out voices of the
  same generator that made the training ones.
- **The lamp's own words.** It says "wait" and "stop" itself; the other
  voice here never held a target. What its own "wait" does through the
  canceller is untested.
- **Timing on the lamp.** The cost of a step was measured on the Mac and in
  a container on it, not on the Orange Pi.
- **More than one training run.** Each model was trained once, with one
  seed; the differences between the models tried are of one run each.
- **The test sets' own limits.** LibriSpeech and AMI headsets are close to
  their talkers; LibriSpeech is read, not spoken. The false-alarm rates are
  of those recordings at -14 dBFS, not of a room.
