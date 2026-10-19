# Room sound classification (#1669): contract proposal

Status: the first builder accepted this topic and model direction on
[19 October at 09:02](https://github.com/autonomous-ai/lampOS/issues/1669#issuecomment-6258073073).
Third reviewed the typed contract on 19 October; classifier retention still
needs validation before a live classifier uses it. Ears publishes levels and
sudden changes, not music or laughter.
This proposal reserves no production behaviour and changes no deployment unit.

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
  speech, music and laughter. They are multi-label: music and speech may coexist.
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
fields are refused, so an unavailable message cannot hide stale scores. There
is no runtime publisher in this checkpoint.

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

This is a proposed additional retention contract, not a change already made to
`Emission` or `PublicationGate`. It needs a privacy-owner read before a live
collector is installed. Offline model/measurement work can proceed independently.

## Model and measurement plan

YAMNet is one candidate: TensorFlow's maintained implementation classifies
521 AudioSet classes with MobileNet-v1, accepting 16 kHz mono audio. The existing
Rust inference stack and model/operator compatibility still need a concrete
check; no model is selected or advertised as running yet.

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

Only after this evidence do the three `kind = "core"` skills use the topic:
a small rare music sway, a glance at laughter, and following a departing person
with Nine's presence contract (#1677). None acts over the lamp's speech or a
person addressing it. Departure is not inferred from sound classification.

The offline voice-eval exporter recognizes `ears/classes` in its omitted-topic
counts. It does not export score payloads or make them behavioral evidence.
