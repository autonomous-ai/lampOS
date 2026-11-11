# Room sound classification (#1669): contract proposal

Status: the first builder accepted this topic and model direction on
[19 October at 09:02](https://github.com/autonomous-ai/lampOS/issues/1669#issuecomment-6258073073).
Third reviewed the typed contract on 19 October. A publisher now exists in
Ears behind `LAMP_EARS_CLASSES=on`, off unless set (on in the product unit since 9 Nov, #5748) and only with
`LAMP_PRIVACY_CONTROLS=on`, or `no-slide` with a stored yes (see "The publisher behind LAMP_EARS_CLASSES"
below). Third's privacy read of its two-second retention found no P1 or P2
(19 October); its scores still need the empty-room and busy-room
measurement, and the owner's word, before any unit sets it. The product's
`lamp-ears.service` sets it since 9 Nov (the lead's ask on #5748), with
`LAMP_EARS_CLASSES_MODEL=/opt/lampOS/ears/models/yamnet.onnx`, where
`models.json` places our own conversion's two files (#6232, below); a lamp
without them publishes `invalid_model`. Without the switch Ears publishes levels and sudden changes,
not music or laughter.

## Purpose and boundaries

Classify the existing room microphone (mic1, 16 kHz mono) locally. The
`ears/classes` output contains no PCM, recording, embedding, transcript or
person's identity. The separate opt-in Ears export in
[#1748](https://github.com/autonomous-ai/lampOS/pull/1748) can put raw PCM on
`mic1/audio` under privacy control; it does not authorize another topic or a
network upload to carry that PCM. A subscriber's recording/retention needs its
own authorization. The first classification output is measurement only. A
positive score is a model score, not a calibrated probability or permission to
move.

Keep `ears/level` and `ears/sudden` unchanged: the voice, expression, mood and
self-test already consume them. A sudden onset cannot distinguish speech,
music, laughter or a chair, so it must not stand in for semantic detection.

## Proposed topic: ears/classes, version 1

A `Latest` topic, nominally updated twice a second and expiring after 1.5 seconds.
An implementation must publish an unavailable state when it loses usable input;
a consumer must also expire it locally if the publisher disappears. The exact
model input length and hop will be recorded with the pinned model, not guessed
from a family name.

The proposed message carries:

- Status: `unavailable` or `scored`. Unavailable has a reason such as warm-up,
  privacy, device gap, overload, invalid model or own speech. It carries no scores.
- For a scored result: the original input window's start and end on the capture
  clock, inference completion time on that same clock, and capture epoch. A newer
  publish time must not make old audio look recent. Reject reversed/future times,
  windows over two seconds and results older than two seconds after input end.
- A pinned model/label-map identifier and separate finite scores in [0,1] for
  speech, music and laughter, and a door and footsteps (#2562, guard mode's
  arrivals by sound). They are multi-label: music and speech may coexist.
  Never choose a single winning label and discard evidence of speech.
- Whether the lamp's voice or its 500 ms tail touched the window, and whether
  that evidence was fresh. Unknown own-voice state is not evidence of silence.

The typed contract is `lamp_msgs::SoundClasses` (`TYPE=lamp_msgs/SoundClasses`,
version 1). Its JSON is either:

```json
{"status":"unavailable","reason":"privacy"}
```

or:

```json
{"status":"scored","model_id":"fixture-v1","window":{"start_us":1000000,"end_us":2000000,"completed_us":2100000,"capture_epoch":7},"scores":{"speech":0.9,"music":0.9,"laughter":0.2},"own_voice":"unknown"}
```

This example deliberately has concurrent speech/music scores and unknown own
voice; it is not permission for a skill to act. `check()` validates shape/ranges
and the raw-age bound. `check_at(now_us)` additionally rejects future or stale
results. Neither checks privacy authority, clock identity or current capture
epoch: those remain required at the publisher/consumer boundary. Unknown JSON
fields are refused, so an unavailable message cannot hide stale scores.
`door` and `footsteps` were added to version 1 (#2562): a message without
them, from an older `lamp-ears`, reads both as 0, no door and no footsteps. The
only publisher is Ears behind its switch, below.

There is no `safe_to_act` field. Consumer gates also require fresh voice state,
no active conversation/input, no hand/pause fence, and the relevant skill's
cooldown. Skills must recheck at the final output boundary and cancel when
speech begins; a quiet trigger alone does not guarantee a quiet delayed action.

## Capture and privacy contract that must be resolved first

The existing privacy path has 50 ms permits. Its queued level/sudden metadata
retains its original deadline: later heartbeats never extend it. A classifier
window plus inference cannot be passed through that path by relabelling old
samples with a newer permit.

Proposed separate bounded retention for classification:

1. Accept blocks only after `CaptureOwner::tick_while` completes its post-read
   privacy/fence checks. The classifier gets a nonblocking bounded handoff;
   inference and filesystem/model work never run on capture or the bus pump.
2. Retain at most two seconds only during an uninterrupted capture epoch. The
   oldest retained sample must also be at most two seconds old: window length,
   queueing and inference share that bound, rather than adding separate windows. A
   lease gap, mute, restart, lineage change, read gap, queue overflow or unknown
   authority purges the partial window and invalidates queued/in-flight jobs.
   Renewing after a gap cannot resurrect an earlier window.
3. A result remains tied to that epoch, the complete input window and a bounded
   deadline. Publication rechecks current authority and the cancellation fence;
   a muted/stale/replaced result is discarded. A later lease is not a new
   timestamp or provenance for the old input.
4. The compatibility capture loop without `LAMP_PRIVACY_CONTROLS=on` does not
   enable classification. Its missing capture boundary is not silently repaired
   by trusting voice/speaking metadata.

This additional retention contract is implemented behind the switch below,
without changing `Emission` or `PublicationGate`. It still needs a
privacy-owner read before any unit sets the switch.

## Model and measurement plan

YAMNet is one candidate: TensorFlow's maintained implementation classifies
521 AudioSet classes with MobileNet-v1, accepting 16 kHz mono audio. The existing
Rust inference stack and model/operator compatibility still need a concrete
check. The switch-gated publisher first pinned Qualcomm AI Hub's YAMNet ONNX
float export, release v0.59.0, for evaluation only. After the weights'
distribution review below ("Where the weights come from", READ-OK on #6119)
it pins our own conversion of Google's `yamnet.h5` instead (#6232), released
in `models-2026-11-11` and placed by `models.json`; the measurements on this
page were taken with Qualcomm's export, whose scores ours matches.

Primary sources:
- [TensorFlow YAMNet tutorial](https://www.tensorflow.org/hub/tutorials/yamnet)
- [TensorFlow model source](https://github.com/tensorflow/models/tree/master/research/audioset/yamnet)

Reuse the existing native inference/feature infrastructure where it fits. Pin
model bytes, label map and licence/source receipt before using scores. Do not
substitute an energy heuristic for a music/laughter classifier.

Provisional engineering targets, to report rather than claim achieved:
- Capture handoff under 1 ms per 20 ms block, independent of a slow classifier.
- Music onset to an available scored result at most 2 s at p95; laughter at most
  1.5 s at p95. Measure first use, repeated events and loaded dependencies.
- Report false music/laughter events per hour of office speech, misses and
  durations at each threshold. No accuracy target or production threshold is
  declared from synthetic examples or a short quiet-room clip.

The office recordings remain on the office Mac; the first builder runs the
scorer there and shares numeric summaries. Keep quiet speech, overlapping talk,
lamp playback, music with vocals, instrumental music and laughter as separate
labelled conditions. Training/calibration and held-out evaluation recordings
must not overlap. A zero observation duration is unmeasured, not zero false rate.

**Off the lamp** (#1669): `cargo run --release -p lamp-ears --example
score_wav -- yamnet.onnx FILE.wav...` scores 16 kHz mono files through this
publisher's own frontend and pinned model, every 500 ms, and says per file how
many windows clear Music sway's and Laugh glance's bars and how long music
holds. It is the model on the file's own sound, with no room or microphone, so
a lamp run's numbers are read against it, never replaced by it. On 21 October,
three free-to-use tracks (solo piano, a jazz vocal, an upbeat instrumental)
held music over 0.6 for 57 to 59 s of 60. A spoken line over the upbeat track
dropped it under 0.6 in a few windows, never past Music sway's 5 s gap.
Nine free-to-use laughs peaked at 0.00 to 0.61 on `Laughter`, which tops the
whole laughing family (Giggle, Snicker, Belly laugh, Chuckle) in every one. A
laugh lands high in one 0.96 s window and rarely in two, so only one of them
ever held two windows in a row over 0.3. In 16.8 minutes of people talking
(a read article, two conversations, an interview), laughter never went over
0.09. So Laugh glance takes **one window over 0.3**, no longer two over 0.5:
5 of the 9 laughs, and none of the talk (the tables are on #1669).

**Parked (21 Oct):** no skill reads `scores.laughter` now: the owner never
asked to listen for other people laughing, so the laugh glance is off and
kept (`skills/laugh-glance`, held out of the product by
`install::NOT_PRODUCT_SKILLS`). The score is still published. Install never
removes a skill, so a copy already put on a lamp by hand (#2399's trial) stays
and still acts with `LAMP_EARS_CLASSES=on` until its folder is removed.

**Through a lamp's microphone** (lamp-52e6, #2399, 21 Oct, `tools/joys-scores.sh`
with `KEEP_DIR`, from a speaker beside the lamp in a talking office): laughs
score about five times lower, 0.04 to 0.15, and nothing else (music, a line
over it, the office) went over 0.015 in 2,430 windows. So Laugh glance now
takes **one window over 0.05**: 2 of the 5 laughs, none of the rest. Music
reads as off the lamp in spells (up to 0.98) but dips under 0.6 between
phrases. Music sway's 0.6 for 20 s with a 5 s gap swayed once on piano and
on a jazz vocal (60 s and 22 s in), never on the empty office, and not on an
upbeat track whose 60 s held under the bar too often. Each skill keeps one
case made of that run's own scores.

The first of them, the music sway (`skills/music-sway`, #2834), is on in
the product since #5770. The product's `lamp-ears.service` sets
`LAMP_EARS_CLASSES` since 9 Nov (#5748), with YAMNet's two files from our own
conversion (below) in `models.json`. Music sway reads
`scores.music` and does nothing without this topic. It never sways to the
lamp's own music (#2833): a requested recording (#1757) plays while
`voice/state` is `listening`, which its `resting` condition holds back, and
that condition must have held 2 s, past the ears' last window of the music.

Only after this evidence do the `kind = "core"` skills use the topic: a small
rare music sway, and following a departing person with Nine's presence
contract (#1677). A glance at laughter was dropped (the lead on #1711, 21 Oct:
never asked for); `skills/laugh-glance` stays parked and off. None acts over the lamp's speech or a
person addressing it. Departure is not inferred from sound classification.

The offline voice-eval exporter recognizes `ears/classes` in its omitted-topic
counts. It does not export score payloads or make them behavioral evidence.

## Where the weights come from (#5748)

The review the first builder asked for on #5748 before any release of the
weights. Read on 10 November 2026 from each source's own page or licence file;
where a source says nothing, it says so here.

**The chain behind `yamnet-qai-v0.59.0`.**
1. **Google's YAMNet.** Trained by Google on AudioSet. Its Keras weights are
   `https://storage.googleapis.com/audioset/yamnet.h5`, the file
   [tensorflow/models research/audioset/yamnet](https://github.com/tensorflow/models/tree/master/research/audioset/yamnet)'s
   README downloads. That repository's
   [LICENSE](https://github.com/tensorflow/models/blob/master/LICENSE) is
   Apache 2.0 ("Copyright 2022 Google LLC") and names `/research` among the
   folders it covers. The README states no separate terms for `yamnet.h5`.
2. **A PyTorch port.** [w-hc/torch_audioset](https://github.com/w-hc/torch_audioset)
   (MIT, "Copyright (c) 2021 Haochen"; master `daed697ddc`) converts
   `yamnet.h5` with `tf_2_torch/convert_yamnet.py` and publishes the result as
   `yamnet.pth` (release v0.1). Its model takes one mel patch, `[1, 1, 96, 64]`,
   the input this publisher feeds.
3. **Qualcomm's export.** [qai-hub-models](https://github.com/quic/ai-hub-models)
   (BSD-3-Clause) v0.59.0, `models/yamnet/model.py`, loads that port
   (`YAMNET_PROXY_REPOSITORY` torch_audioset, `DEFAULT_WEIGHTS = "yamnet.pth"`,
   fetched from Qualcomm's own asset store) and exports the ONNX graph and its
   `yamnet.data`. Its README's licence section points only to torch_audioset's
   MIT licence. Qualcomm states no separate terms for the exported files, and
   the conversion itself runs in their tooling, not in a command we can repeat.

So the bytes we pin went through two conversions by others. Neither says
which `yamnet.h5` it took, and Qualcomm's step has no recorded command we can
repeat.

**The class names.** `yamnet_class_map.csv` sits in the Apache 2.0 tree
above. Its display names are AudioSet's ontology, which Google publishes under
[CC BY-SA 4.0](https://research.google.com/audioset/download.html). The
AudioSet labels and segments themselves are CC BY 4.0. The lamp ships no
label file. It ships five class indices and their names in the code and in
this document (Speech, Music, Laughter, Walk/footsteps, and the door group).

**Recommended: we convert it ourselves, from Google's file.**
- **Steps:** run torch_audioset's `convert_yamnet.py` at `daed697ddc` on
  `yamnet.h5`, then `torch.onnx.export` the same `[1, 1, 96, 64]` → `[1, 521]`
  model, with external data.
- **What the release records:** the sha256 of `yamnet.h5` as downloaded, the
  Python, TensorFlow, PyTorch and ONNX versions, and the exact commands.
- **The output** is a new `yamnet.onnx` and `yamnet.data`. Their sizes and
  sha256 replace the pins in `classes.rs` (`MODEL_ID` becomes ours) in the
  switch-on PR.
- **Equal or not:** `score_wav` runs on the 21 October clips (#1669) through
  both models. The new one ships only if its scores match Qualcomm's to
  within rounding. Otherwise the thresholds above were set on a different
  model, and that is said rather than shipped.
- **Why:** this takes Qualcomm's untraceable step out of the chain. Every
  remaining link then has a written licence: Apache 2.0 for Google's weights
  and code, and MIT for the conversion script.

**What the `models-2026-11-11` release carries beside the two files.**
- Apache 2.0's text, with a NOTICE that says:
  - the weights are Google's YAMNet, from `yamnet.h5` (link and sha256), and
    converted by us to ONNX (the commands above);
  - torch_audioset's MIT licence and copyright line, for the conversion
    script.
- The AudioSet attribution for the class names the lamp uses: "AudioSet
  ontology by Google, CC BY-SA 4.0", with the link. If the release ever
  carries `yamnet_class_map.csv`, that file keeps CC BY-SA 4.0.

**Not answered by any source:**
- whether Google's `yamnet.h5` download has terms of its own beyond the
  repository's Apache 2.0 (the README states none);
- what terms Qualcomm's asset store puts on its `yamnet.pth` and export.

The recommendation above doesn't depend on either.

**Our conversion (#6232, 11 November 2026).** Done as recommended, by
`lampOS/tools/yamnet-convert.sh WORKDIR` (every input pinned in it; the
Python half is `tools/yamnet-convert.py`, run with `python -I`):
- **Input:** `yamnet.h5` from the URL above, 15,296,092 bytes, sha256
  `13c3308955bbfaef262f175ac9c40e47b134573a93984f009220dd7cc12a1744`, the
  same from two downloads into separate folders at 14:00 and 16:25 (+07).
- **Sources:** torch_audioset at `daed697ddcd1d2f7304cc3c2b1b4295ac7e304e4`
  (its `YAMNet` module and `convert_yamnet.py`'s weight mapping; its
  `TFYamn` check is written for an older YAMNet API and isn't run), and
  Google's YAMNet code, tensorflow/models at
  `c14bf9ad91962cf189f9f58db2132c06247fcd53`, as the reference.
- **Tools:** Python 3.11.13, tensorflow 2.18.0, tf_keras 2.18.0, torch
  2.5.1 (`torch.onnx.export`, opset 17, TorchScript exporter), onnx 1.17.0,
  onnxruntime 1.20.1, h5py 3.12.1, numpy 1.26.4, pyyaml 6.0.2; made on an
  Apple Silicon Mac. Two runs gave byte-identical files.
- **Output:** `yamnet.onnx` 29,489 bytes, sha256
  `c7e501792262e7335b309e67ed68f8e99773a869719397b479593a82fa69657d`, and
  `yamnet.data` 14,917,924 bytes, sha256
  `26771b4915f21765ec6605f89cfb6da9121631c2e1c6a1e767d9ef95248cc6bd`;
  input `patch` `[1, 1, 96, 64]`, output `logits` `[1, 521]`. `classes.rs`
  pins them, `model_id` `yamnet-h5-lampos-2026-11-11`.
- **Against Google's Keras model** with the same `yamnet.h5`, on Google's
  own patches of five synthesized 4 s clips (silence, a 440 Hz tone, white
  noise, a chord progression with a kick, and a vowel-formant pulse train;
  no recording of anyone), 40 patches: the largest logit difference is
  6.1e-4 (on silence, whose logits are far below zero; 8.4e-5 on the rest),
  the largest score difference 2.4e-6; the script fails over 1e-3. Keras
  tops music at Music (0.984) and the vowels at Speech (0.981).
- **In lamp-ears' own code** (tract-onnx 0.23.8): the five scores it
  publishes on those patches are within 4.6e-6 of Keras's (Qualcomm's
  export: 4.3e-6). `score_wav` over the five clips (35 windows) prints the
  same scores to its two decimals with either model, and the same verdicts;
  silence scores speech 8.63e-8 and music 7.07e-15, as before. So the
  thresholds above, set on Qualcomm's export, hold. The model-backed test
  passes with it (load 98 ms, a patch 2.8 ms on the Mac). The 21 October
  clips (#1669) weren't on the Mac and were not rerun.

## The publisher behind LAMP_EARS_CLASSES

`lamp-ears` with `LAMP_EARS_CLASSES=on` (exactly `on`) and
`LAMP_PRIVACY_CONTROLS=on` publishes `ears/classes`
(`crates/ears/src/classes.rs`), and with `no-slide` only when the person's
yes to room sounds is stored (`room_sounds_yes`, read at the start and
every 2 s while it runs, so a forget stops it;
[privacy-switch.md](privacy-switch.md)). With the switch on and privacy
controls off, or `no-slide` without the yes, one journal line says no
classifier runs, and none does. Off, nothing below
exists: no queue, thread or model.

**Model.** `LAMP_EARS_CLASSES_MODEL` is the absolute path to `yamnet.onnx`,
with `yamnet.data` beside it. Both are pinned in the code by size and
SHA256 (graph 29,489 bytes `c7e50179…657d`, weights 14,917,924 bytes
`26771b49…c6bd`; `model_id` `yamnet-h5-lampos-2026-11-11`, our conversion
above). The product's `lamp-ears.service` names
`/opt/lampOS/ears/models/yamnet.onnx`, where `models.json` places both. They are read once,
checked, and parsed from the checked bytes in memory, so a file swapped
after the check is never loaded. A missing, relative or mismatched model
publishes `invalid_model` every 500 ms and retains no audio. Loading and
inference run on their own thread (`ears-classes`), never on capture or the
Bus pump. Scores are independent sigmoids of the logits for Speech (0),
Music (132) and Laughter (13), and Walk, footsteps (48) and a door for
guard mode (#2562; read by `lamp-skills` only with `LAMP_GUARD_SOUNDS=on`).
A patch none of whose 20 ms blocks rose 6 dB over the room's quiet (the
level floor of `ears/level`, once it is known) is not run through the
model: it is published as scored with every score 0, on time, as YAMNet
scores silence. A silent room then costs the classifier nothing
(lamp-52e6, #5770: 14.8 % of a core with classes on at rest, 6.5 % off);
music sway's 5 s gap and its 5 minutes under 0.3 to rearm read those zeros
as before. `door` is the highest of the door group: Door (348), Sliding door (351),
Slam (352) and Knock (353), since a knock on a door is AudioSet's Knock, not
Door. Tap (354) is left out: desk taps and typing raise it all day. The
indices are `yamnet_class_map.csv`'s, the same in qai-hub-models v0.59.0's
`labels/yamnet_labels.txt`.

**Frontend and windows.** As the offline probe: 16 kHz, FFT 512, periodic
Hann 400 centered in it, hop 160, magnitude, 64 HTK mel bands 125–7500 Hz,
`log(mel + 0.001)`, patches of 96 frames every 50 frames (500 ms). A capture
era's first patch reflects at its start and needs 15,456 samples; later
patches take 15,712 real samples. A window's `start_us`/`end_us` are whole
20 ms blocks on the capture clock (read-completion time minus 20 ms, and the
last block's read completion), so they cover the patch's full FFT extent.
`own_voice` is `present` for a window with a block captured while the
voice said, freshly, that the lamp was speaking (ears' speaking flag, the
one its sudden sounds use), so Laugh glance's and Music sway's `own_voice`
guards hold at the lamp's own voice (#1757's read, 22 Oct). Otherwise it is `unknown`, never
`clear`: the flag turns a stale voice state into "not speaking", which is
not evidence of a quiet lamp. A `voice/state` of `listening` while the lamp
plays something (#1757's music, its input kept open for "stop") is not
counted; the skills' `voice/state` `equals = "resting"` guards still hold
then.

**Handoff and retention.** After the capture owner's post-read lease and
fence checks, the same callback that feeds the PCM export offers the block
to a bounded queue of 25 blocks (500 ms): a copy of 640 bytes, no wait, no
allocation. Only a block whose own permit and fence still hold is copied.
A full queue drops the block and marks the window overloaded. The worker
keeps whole blocks of one era from the next patch's start: at most about
16,000 samples, plus what waits in the queue, under 1.5 s and never over
the 2 s cap. It purges (zero-filled, best effort as for the PCM export)
on a sequence gap, a block from another epoch or fence, the era's
authority lapsing (checked on every 20 ms wake), 100 ms without a block,
an overflow, the oldest retained sample passing 2 s, or an inference error.
A later block starts a new era; it never completes the old one. What is
made from the samples goes the same way: each patch's 96x64 log-mel
features (about a second of the room, roughly invertible) and the FFT's
buffers are zero-filled once the patch is scored, best effort as the
samples are; the model's own copy of its input is the inference library's
and is not.

On the privacy route, lamp-ears says once, after 10 s with no permission
from Voice (since its start or the last one), that the microphone stays
closed until one comes: muted, or Controls has no privacy source, as on a
lamp with no privacy slide in its pins (lamp-4ace on 20 October, 04:13).

**Publication.** The worker sends results to the existing Bus pump through
a queue of four (dropped when full). Before each publish the pump checks
that the era's epoch and fence still hold a live lease now and that the
result passes `check_at(now)` (two-second raw age); otherwise the result is
discarded. The header time of a scored message is the window's end, never
the publish time. Results are published under the era's current lease, not
the lease of their last block: a lease lasts 50 ms and inference can take
longer, but any lapse or mute closes the era, so "same epoch, live now"
means capture was never interrupted since the samples were admitted.

`unavailable` says `warming_up` until an era's first patch, `privacy` when
the era lost authority, `device_gap` on a gap or 100 ms of no blocks,
`overloaded` on overflow, age or a scored era a whole second late,
`invalid_model`, and `inference_failed`. It is said at once after scores and
otherwise every 500 ms (at most every 100 ms). With no blocks and no live
lease (a lamp muted from the start, or before the first grant) it says
`privacy` from 100 ms after it starts.

A laughter and speech timeline is a new kind of personal signal. Nothing
reads or records `ears/classes` yet; a subscriber that records it (the
journal, records, the cloud) gets its own privacy read first (Third on
#1923).

**Measured on the development Mac (Apple Silicon, host only, 19 October).**
Against the targets above:

| What | Target | Measured |
|---|---|---|
| Capture handoff per 20 ms block | under 1 ms, independent of the classifier | release: p50 42 ns, p95 84 ns, max 13 µs over 1,000 blocks; with the worker stalled and the queue full, p50 42 ns, max 125 ns (debug: p50 167 ns) |
| Per patch, frontend and inference | not set; a patch every 500 ms | release: p50 2.8 ms, max 3.1 ms over 20 patches; model load 75 ms (debug: 102 ms per patch) |
| Music onset to scored result, p95 | ≤ 2 s | not measured; by construction a sound is first fully inside a patch 0.5–1 s after onset, plus inference |
| Laughter, p95 | ≤ 1.5 s | not measured |

Silence scores equal the offline probe's (speech 8.63e-8, music 7.07e-15),
and streamed patches' features equal the offline frontend's over the whole
signal. Not measured: ARM64 inference time and CPU on a lamp, Linux ALSA
timing, onset latency on real sounds, and any accuracy or false rate.

Host tests (`cargo test -p lamp-ears classes`) cover the switch, frontend
bounds and values, the handoff cost and a full queue, unauthorized blocks,
streamed windows against the offline features, gap, mute, epoch, fence and
age purges, the worker's scores then `privacy` with the pump's recheck, and
pins refused before parsing. The model-backed test runs only with
`LAMP_EARS_CLASSES_TEST_MODEL` set to a present `yamnet.onnx`, and is
skipped otherwise.
