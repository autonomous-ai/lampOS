# Speaking face: Light-ASD component candidate (#8848)

This is an independent, off-by-default benchmark candidate on the requested
simple-call base `0dbbfc95bbea6ddf397308e1a8ef1933c5d19dcb` (#8892). It runs
through `speaking_face_bench` in its own process. It is not connected to live
capture, voice admission, a renewing conversation window, personal memory or
product units. No merge, train entry or lamp installation is part of this step.
The owner's 27 November evening scope is **two or more faces**, last in the
sensor integration order: prove each sensor alone before combining it.

`LAMP_SPEAKING_FACE_SENSOR=on` is the only enable value. Off, the bench returns
before opening a model, manifest or media file. One visible face produces
`not_applicable`, without reading its media. At most eight tracks are inferred
per case; inference runs one at a time. A case with more tracks remains an
explicit `unknown` (`too_many_faces`), without opening its media. Future live capture and cancellation
integration require their own review and evidence; this bench is not that path.

## Model and provenance

The [official Light-ASD source](https://github.com/Junhua-Liao/Light-ASD/tree/ed38c232de5efe0261dbd68627c0ade7cdfe14eb)
and its MIT licence are retained with the conversion record. Source commit:
`ed38c232de5efe0261dbd68627c0ade7cdfe14eb`. The AVA-trained checkpoint is
`weight/pretrain_AVA_CVPR.model`, SHA-256
`d44bc3ea7baa8e0946fa3921311714a630ed8b90a1928fab0dbe30d918909317`.
The TalkSet-fine-tuned checkpoint is not used. The source's 94.06% AVA
validation mAP is a published ranking metric, not a measured lamp face-choice
percentage or a 95% qualification result.

The local conversion includes the trained audio-visual classifier head, without
training or threshold fitting. It exports ONNX opset 17, IR version 8, batch 1:

| Window | Bytes | SHA-256 |
|---|---:|---|
| 25 frames / 1 s | 4,103,414 | `a2386f3d087af52809901a568df504ebdbdbe6161d6d958b3fccae31ec50352d` |
| 50 frames / 2 s | 4,103,415 | `9c56974f3933cda59e5524e26aa9c5ef79e854ff78e43b09c0c1eb3adc5b1ef9` |

Both pins are enforced before parsing. Exporting the source's rank-four audio
`MaxPool3d(1,1,3)` needs an equivalent `MaxPool2d(1,3)` lowering: the original
unbatched operation and lowering were checked bit-for-bit, then complete outputs
were compared against the unmodified PyTorch source. Conversion scripts, original
and exported weights, parity inputs/outputs and hashes stay in the local evidence
archive; binaries and recordings are not committed.

The primary engine is the existing `tract-onnx 0.23.8`. An optional runtime path
uses the project's existing ARM64 ONNX Runtime 1.30.0 pin, verified before
loading, with one CPU thread and spinning disabled. It refuses targets without
an existing runtime pin; Intel Mac measurements use tract. A successful model
load is not a claim that the lamp has met its latency budget.

## Inputs and time

The model takes audio `[1,4*T,13]` and grayscale visual `[1,T,112,112]`, contiguous
float32. Visual values are 0..255; normalization is already in the graph.
Audio is mono signed PCM16 at 16 kHz in its original amplitude units. The Rust
frontend follows `python_speech_features 0.6`: 25 ms rectangular windows, 10 ms
hop, 512-point FFT, preemphasis 0.97, 26 mel filters, 13 orthonormal DCT-II
coefficients, lifter 22, and coefficient zero replaced by log spectral energy.
The AVA loader wrap-pads the 99/199 MFCC rows to 100/200. It does not normalize
PCM to -1..1 or zero-pad the feature rows. Independent synthetic golden values
test the frontend's beginning, middle and padded end.

The network has forward and backward GRUs and symmetric convolutions. A full
track's offline predictions see future context. This candidate's primary
comparison uses only the **newest output of a trailing 1 s or 2 s window**.
There is no future-frame smoothing. The newest frame is 40 ms before the audio
window's end; both boundaries are recorded. The first useful window needs that
much captured history. Processing duration must not hide this collection time.

The prepared public-data inputs contain tight annotated face crops resized to
112 square at 25 fps. This is the AVA component protocol with supplied tracks
and boxes, not proof of live detection/tracking quality. The current-mouth
comparison uses actual YuNet detections and the existing tracker; report that
input advantage explicitly. Public movie footage has no measured desk distance
or lamp lighting. Natural face pixel sizes and any declared resolution probes
cannot be relabelled as measurements at 0.6 or 1.2 m.

## Frozen face-choice rule

Before reading benchmark predictions, the rule was fixed at class-1 softmax
probability 0.5. Exactly one positive track selects that temporary track; no
positive returns `no_visible`; two or more return `unknown`. A missing score,
duplicate track or invalid number returns `unknown`. No largest-face, named
person, top-score or audio-only fallback is used. A separately reported forced
top-1 ranking diagnostic is not this rule and cannot replace its abstentions.

The evaluator keeps missing context in the denominator. A clear single-speaker
case answered `unknown` is a miss. Zero-speaker and overlap controls are reported
separately. One-face inputs are outside the revised sensor's opportunities.
Selecting public videos/timestamps uses only availability and independent labels,
never candidate scores. Public component results do not qualify the product's
real-person, on-body gate.

## Running the candidate

From the standalone `lampOS/` root:

```sh
LAMP_SPEAKING_FACE_SENSOR=on cargo run --release -p lamp-eyes \
  --example speaking_face_bench -- ROOT CASES.jsonl MODEL.onnx ASSET.json
```

An optional fifth argument is the already pinned ARM64 runtime library; this
does not install or obtain it. `ASSET.json` contains `frames`, `bytes` and
`sha256` exactly as pinned above. Each JSONL case has:

```json
{"id":1,"clock_id":"public-source-1","eyes_epoch":"track-set-1","input_start_us":0,"input_end_us":2000000,"target_frame_us":1960000,"context_frames":50,"audio_pcm_s16le":null,"faces":[{"track_id":"track1","gray_u8":null},{"track_id":"track2","gray_u8":null}]}
```

An available artifact replaces null with `{"path":"relative/path","sha256":"..."}`.
Audio is exactly `context_frames * 640` little-endian signed samples. A face is
exactly `context_frames * 112 * 112` unsigned grayscale bytes. Incomplete context
is null, never a frozen or repeated face presented as observed. A 25-frame model
takes the newest half of a 50-frame input and recomputes its own MFCC window.
Paths, bytes and SHA-256 are checked; canonical paths must stay under `ROOT`.
Gold labels and private words are refused by the input schema. Manifests have
at most 10,000 cases and 64 KiB per row.

Results identify the model and backend, source clock/eyes epoch, source frame
and audio boundaries, per-face scores, the chosen output, model load time, and
host feature/inference/total replay durations. **`ready_us` is always null.**
`replay_elapsed_us` is a host duration; it is never added to recorded timestamps.
These results follow the [corrected #8850 replay distinction](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7211763273)
but remain a component report, not a qualification manifest or lamp-live run.

The [current journal reader](speaking-face-sensor.md) still explains actual
mouth journals, when supplied. The original 2/88 evidence was lost; public
component replay and a fresh lamp count must be labelled separately. Every raw
input, derived file and run is retained locally with SHA-256 and a README under
the owner's overnight retention instruction. No recording is deleted or put on
GitHub. Numerical results and limits belong in the `EVAL` comment on #8848.
