# Confirmed-person threshold evaluation, 28 November 2026

**Result: no qualifying configuration on the declared grid.** Raising face
confidence, requiring an unclipped/larger face, and adding a body-pose score did
not separate physically present people from depicted people at the required
retention. None of these results authorizes an active presence policy.

[Vietnamese](README_vi.md) · [Aggregate numbers](numeric.json) ·
[Task and EVAL](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241725349)

## What ran

Offline piece 5b against product source `16e5d147bd7dc31dbf61d1ef7e0b1f68516700c3`:
3,636 retained image cases, 1,808 calibration and 1,828 test. The replay completed
every row without a processing error. No lamp data, device access, training,
installation, merge, or product default changed. This measures a per-image
decision, not real sittings or an empty-room day/night interval.

| Denominator | Calibration | Test |
|---|---:|---:|
| Source groups, including render identities | 221 | 226 |
| Primary public people, visible annotated nose and both eyes | 67 | 67 |
| Primary frontal rendered people | 36 | 36 |
| All human conditions, including stresses | 840 | 852 |
| Object cases | 956 | 960 |
| Ambiguous-source cases, never scored as clean negatives | 12 | 16 |

435 licence-filtered COCO val2017 scenes were reviewed before model inference.
Seven scenes remained ambiguous; their 28 variants remain unknown. All source
variants stay in one split. Public identities are unknown: source separation does
not establish identity or pretraining separation. Render identities A12–A17
calibrate and A18–A23 test. There are no identical image-byte aliases. Conditions
of one source or render identity are correlated, not independent people.

The public people are varied scenes, not a representative sample of people at a
lamp. Twelve visible-face desk/table scenes were manually marked before inference,
six per split. The held-out baseline finds 5/6; the draft face rule finds 4/6. This
small subset cannot establish 99% desk retention. COCO chair/table/laptop category
co-occurrence was also recorded, but is not proof of a person sitting at a desk.

Inputs are 320×240 JPEG Q85, 4:2:0, never enlarged. Q85 is a declared test setting,
not measured camera quality. Public stresses are scale 0.65, a warm dim RGB gain
of `[0.35, 0.2905, 0.224]`, and translation toward either edge. These are artificial
transformations, not measured lux or natural occlusion. Primary public cases use
the unshifted scale-1 daylight transform only. Primary renders use 0.5/1/1.5 m and
two authored light presets, inherited from the prior sensor-4 diagnostic.

Negative families include public object scenes, 1,072 constructed print/display
cases, and 216 ceramic-bust/headphone/round-lamp renders. Prints/displays are 2D
raster compositions with authored frames, stands and backgrounds, not photographs
of real screens. Busts are low-poly cut avatar meshes on pedestals. These controls
expose a failure mode; they do not establish physical-scene coverage.

## Freeze and decision rule

The [protocol was posted before inference](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241581604).
The [calibration selection was posted before test scoring](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241656460).
The seven families contain 5,642 configurations. Select zero observed object false
presence on calibration, then maximize the smaller of the two primary positive
retentions; break ties by their combined accepted count and then the original
ascending grid order. Qualification requires zero object false presence, no
negative processing unknowns, and at least 99% retention in **each** primary
cohort. With these small denominators that requires 67/67 and 36/36. Even perfect
observed retention would not prove a 99% population rate.

| Parameter | Declared values |
|---|---|
| Face score | .60, .65, .70, .75, .80, .85, .90, .95, .99, 1.01 |
| Face height / filled-image height | 0, .02, .04, .06, .08, .10, .15, .20, .25, .30, .40, .50, 1.10 |
| Maximum clipped area fraction | 0, .001, .01, .05, .10, .25, .50, 1 |
| Mean body-keypoint score | 0 through .95 in steps of .05, then 1.01 |

Families are each single parameter, face score+height+unclipped, that combination
plus body score, and that combination plus body score whose nose is inside the
same face box. Nose association uses the existing 0.2 keypoint floor. All face
constraints must hold for the **same** face. Clipping is against the filled image,
not black detector padding. Score/height/body boundaries use float32 as Rust does.
Values above 1 are explicit reject-all controls, not usable settings.

## Frozen held-out result

| Calibration-selected zero-FP family | Frozen cutoff | Calibration public / render | Test public / render | Test object FP |
|---|---|---:|---:|---:|
| Face score | ≥.95 | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Face height | ≥1.10 of image | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Body mean | ≥.70 | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Face score+height+unclipped | ≥.60; ≥1.10; 0 clipping | 0/67; 0/36 | 0/67; 0/36 | 0/960 |
| Face+body, with or without nose association | ≥.60; height ≥.30; no clipping; body ≥.45 | 1/67; 4/36 | 1/67; 6/36 | 3/960 |

Every selected family had 0/956 calibration FP; none passed positive retention.
Clipping alone had no zero-FP calibration candidate. The three held-out combination
FP were two prints and one display. No threshold changed after the freeze.
The full test grid is descriptive only and did not select an operating point.
Failure on this grid is not a proof about every possible classifier or threshold.

## Descriptive baseline and proposed settings

These points were on the original grid, not chosen for good test performance.
Fractions count accepted cases, not localization or identity accuracy.

| Held-out rule | Public people | Rendered people | Object FP | All human conditions |
|---|---:|---:|---:|---:|
| Existing YuNet floor .60 | 45/67 | 36/36 | 283/960 | 298/852 |
| Face score ≥.80 | 34/67 | 36/36 | 193/960 | 199/852 |
| Face ≥.80; height ≥.06; unclipped | 33/67 | 36/36 | 191/960 | 171/852 |
| Above + body ≥.20, nose in same face | 32/67 | 36/36 | 186/960 | 164/852 |
| Body mean ≥.20 alone | 65/67 | 36/36 | 629/960 | 673/852 |
| Body mean ≥.30 alone | 60/67 | 36/36 | 454/960 | 443/852 |

The draft face rule retains 171/298 of the human-condition cases found by the
existing floor, losing 127. It therefore does not establish the no-regression
intent either. Of the baseline's 45 primary public acceptances, 44 have a passing
face box containing the annotated nose; the draft face rule has 33/33. Headline
presence counts must not be relabeled as correct-person localization.

| Held-out negative family | Existing face floor | Draft face rule |
|---|---:|---:|
| Constructed display | 139/268 | 94/268 |
| Constructed print | 138/268 | 95/268 |
| Public object scene | 6/308 | 2/308 |
| Framed photo; game screen | 0/4; 0/4 | 0/4; 0/4 |
| Ceramic bust; headphones; round lamp | 0/36 each | 0/36 each |

277/283 baseline object acceptances and 189/191 draft-face acceptances are the
constructed prints/displays. Both models can correctly detect a depicted face or
body yet give the wrong answer to physical presence. Mean confidence across 17
keypoints is not a liveness probability. These results do not qualify movement,
moving-screen rejection or compensation for the lamp's own camera motion.

## Runtime, controls and latency

YuNet weights match the product hash; the offline adapter reproduces its
320×256 top-left-padded geometry and decode. The CPU OpenCV backend is not a
bit-parity assertion about tract. MoveNet is the approved public **Lightning
float16 TFLite v4**, not the shipped int16 NPU model. Independent images use a
whole-image square crop at 192×192, without carrying a previous image's track.

Two published head-decode fixtures agree within 0.000099 model pixels, four crop
fixtures match byte-for-byte, and three tracking fixtures agree within 8e-8
normalized coordinates. Together with geometry and two generated blank controls,
14 adapter checks pass. Nine scoring controls pass. The 77-check post-test audit
uses a separate scalar recount of every one of the 11,284 grid rows, verifies the
selection/tie order and all input/source hashes. This is independent code by the
same builder, **not an independent reviewer**. No full-model float/int16 parity
was measured: the numeric head fixtures lack their original input pictures.

Host: Intel iMac20,1 / i7-10700K / 64 GiB, macOS 26.6.2, x86_64, Python 3.11.13,
LiteRT 1.0.1 with reported XNNPACK CPU delegate, NumPy 1.26.4, OpenCV-headless
4.10.0.84, Pillow 10.4.0. OpenBLAS and LiteRT request one thread; OpenCV requests
one but reports 16. The shared host was not isolated from other work.

Timing is **decoded RGB to output features**; file/JPEG decode and model
construction are excluded. It includes YuNet input/decode, MoveNet crop/model
and face-feature arithmetic. Construction was 34.839 ms; the first case was
22.226 ms. No study warmup was discarded. The remaining 3,635 cases have warm
median/p95 17.952/21.890 ms, maximum 42.719 ms. MoveNet invocation alone has
median/p95 7.025/8.278 ms. None is ARM64 NPU, camera-to-bus, audio-path or
voice-under-load latency.

## Source permissions and limits

The [exact MoveNet float16/v4 publisher page](https://raw.githubusercontent.com/tensorflow/tfhub.dev/master/assets/docs/google/models/movenet/singlepose/lightning/tflite/float16/lite/4.md)
licenses the weights under Apache 2.0. Its model card identifies COCO train2017
and fitness-video training and COCO val2017 evaluation. This study reuses that
published validation domain; it does not claim unseen people or a new independent
generalization benchmark. YuNet follows the MIT licence in the
[OpenCV Zoo model directory](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
The earlier avatar assets are CC0; object geometry is authored.

[COCO's terms](https://raw.githubusercontent.com/cocodataset/cocodataset.github.io/master/dataset/termsofuse.htm)
license annotations under CC BY 4.0; image licences are separate. This subset
selected annotation licence IDs 4, 5, 7, 8 (CC BY 2.0, CC BY-SA 2.0, no known
copyright restrictions, US government works), retaining per-image metadata and
attribution locally. It includes no selected NC/ND entries. No images, models,
private trace, names or source pictures are committed or redistributed here.
Only aggregate evidence is public. All inputs, failed preparation attempts,
outputs, source receipts and hashes remain local.

The declared JPEG/light transformations are not lamp measurements. Real-person
labels have a single-builder visual audit; ambiguity is retained, and no
operator-attested empty room or actual sitting is represented. More variants of
the same scene do not increase the number of independent people. No confidence
interval here claims population zero FP or 99% retention.

## Consequence for piece 1

Keep active confirmation disabled. A detector score cannot be treated as a
person's presence, even with a same-face body association. Continue bounded
measurement and shared comparison-quality plumbing, but do not silently promote
failed cutoffs to defaults. Temporal evidence must handle multiple faces in one
frame, expired history, privacy/camera epochs and camera motion; moving screens
also need explicit negatives. Tests/First own the physical-scene ROW, including
night/day empty-room events and retained real sittings. Recognition size/yaw and
blur limits remain separate work, not results of this study.

## Evidence hashes

Full hashes, family slices and distributions are in [numeric.json](numeric.json).

| Artifact | SHA256 |
|---|---|
| Frozen protocol | `63d61cf665dc58d595d5a15fc51e42ef82602f752991e292959997ac1f7f4eac` |
| Input manifest | `f3847a4ff01434d031407e428e3c22896800676c29259b0e560846d47072e89f` |
| Replay predictions | `50336d0f284aabf85bd64837b03dad0efa7e14a8bbc05f54aa568172d6a9564b` |
| Selected thresholds | `93ed2c275eb04c2eb5ed5d01e27acc463b94d861eb8e52085b0e942f6617c4df` |
| Private full aggregate | `e0a59af005c4ea5dce2ecdb7b4c942d7ff962e003bcafe99eae9aa1503f2c7e9` |
| Public aggregate | `5c486e2301baaebbbc983e1305171bc531a555395c177f7bbfc34f177059b6c5` |
