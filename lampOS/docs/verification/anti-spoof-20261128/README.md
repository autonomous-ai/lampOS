# Single-model anti-spoof evaluation: not qualified (#8971)

28 November 2026, Fourth. [Vietnamese](README_vi.md),
[numeric evidence](numeric.json). This is an offline evaluation, with no product
code, training, lamp access, deployment or operating-point approval.

**MiniFASNetV2 does not meet zero object false presence and ≥95% real-person
retention on this set.** Calibration selects a threshold that rejects everyone.
The fixed argmax baseline admits printed faces and loses real people. Its host
latency also misses the declared p95 budget. Presence remains observational.

## Frozen procedure and exact model

The [protocol was posted before inference](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7242335162).
Its SHA-256 is
`8bd405357881e3c767309c635e25cd69ac0b399febc68778d2b31a94e8e75df2`.
The [calibration selection was posted before test scoring](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7242353570),
SHA-256 `84625408a02ee37fe7b79828bb5483e6b47ecac0d1dfd2a530bad5a03dbe8beb`.
No test threshold, model or input selection was changed afterwards.

One [Minivision checkpoint](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/tree/b6d5f04ad78778917853b25c778acef6d5626d15):
`2.7_80x80_MiniFASNetV2.pth`, 1,849,453 bytes, 434,560 parameters, SHA-256
`a5eb02e1843f19b5386b953cc4c9f011c3f985d0ee2bb9819eea9a142099bec0`.
Code/weights are in the Apache-2.0 repository; no separate weight restriction
was found. Exact-weight license clarification questions are unanswered. The
[author reports privately collected application data](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/issues/25#issuecomment-678321691),
with [about five million images per class](https://github.com/minivision-ai/Silent-Face-Anti-Spoofing/issues/25#issuecomment-678095382),
and no open-source training sets. Training overlap, consent and provenance
cannot be independently audited. This evaluation is not a distribution approval.

Use the publisher's 2.7 context expansion, edge shifting, inclusive crop and
linear resize to 80×80. Input is **BGR float32 0–255**, without division by 255.
Class 1 means real. Weights load as tensors only, strictly into the reviewed
architecture. There is one model, not the publisher demo's two-model ensemble.
The detector is the existing frozen YuNet at 0.6, not another RetinaFace model.

The baseline accepts an argmax of class 1. The calibrated rule also requires
`p_real ≥ t`; the grid is 0..1 in steps of .01 plus 1.0000001 (all reject).
On calibration only, require zero object false positives and no processing
unknowns, then maximize primary-real acceptance and break ties by lower t.
The selected t is **1.00**: 0/67 real scenes retained, 0/956 object scenes
accepted. Baseline calibration is 34/67 and 196/956 respectively. No declared
grid point qualifies. This does not prove failure of every possible model or
threshold family.

## Inputs and what the denominators mean

All **3,636 frozen 5b images** and their existing detections are reused from
[the static evaluation](https://github.com/autonomous-ai/lampOS/pull/8980).
Its source-group calibration/test split stays fixed. Previous static results
were already known; no claim that this is a new unseen dataset is made.
Primary-real held-out scenes total 67, with 45 detected by YuNet. Detector
misses remain in the end-to-end denominator. Adding a rejecting filter cannot
recover the 22 misses. Conditional retention is reported separately.

The external set has **23 source files and 129 views**, all test-only:

- [Axon Labs' CC-BY-4.0 public replay sample](https://huggingface.co/datasets/AxonData/Display_replay_attacks/tree/f5560f659e185ded62c2f09ff502426b0588406e):
  all five real photos and five display videos. Five predetermined frame
  positions per video (10/30/50/70/90%) give 25 correlated frames, not 25
  independent videos. The publisher's full paid dataset was not accessed.
- [Persona's filtered CC-BY-4.0 attack release](https://huggingface.co/datasets/withpersona/persona-fas-bench-v1/tree/726d025dc1d187f553a3faff3e438d879e7a57e8):
  all 13 available `physical_photo` images, not the full benchmark. The original
  card lists 14; the filtered release contains 13. One shows an operator's
  partial face behind the print. Before inference, its three views were marked
  unknown and retained; only **12** sources are unambiguous print negatives.

Each external source/frame is fit into 320×240 without enlargement, at relative
scales 1/.65/.4, with black letterboxing and JPEG Q85 4:2:0. Scale 1 is primary;
smaller views are diagnostics. These transformations are not measured lamp
optics, distance, illumination or JPEG quality. The external reviewed manifest
SHA-256 is `6b6baca67a7871f30f0b42beb3423a47630238ac4fb9b9e1ee5ff354920d1099`.

Publisher labels were checked against visible contents, not independently
attested captures. Identities and their independence are unverified. Rendered
people are not bona-fide human captures for PAD, and constructed 5b screens and
prints are not physical recaptures. The original static 5b ≥99% gate is unchanged;
this follow-up uses the lead's ≥95% real-retention bar. No images, identities,
raw per-example records or weights are published here.

## Held-out results

| Rule | 5b real scenes retained | Conditional on a detected real face | 5b object scenes accepted |
|---|---:|---:|---:|
| Existing YuNet ≥0.6 | 45/67 (67.2%) | 45/45 | 283/960 (29.5%) |
| Single MiniFAS argmax baseline | 27/67 (40.3%) | 27/45 (60.0%) | 152/960 (15.8%) |
| Frozen t=1.00 | 0/67 | 0/45 | 0/960 |

The baseline's 152 object errors comprise 74/268 constructed-display scenes,
77/268 constructed-print scenes and 1/308 public-object scenes. Other object
families have no accepted face, often because YuNet found none: this is not a
standalone anti-spoof success. It accepts 33/36 rendered people, a diagnostic
with no bona-fide liveness interpretation. Every processing row succeeded.

| External relative scale; baseline | Real photos retained | Display videos with any sampled frame accepted | Unambiguous print photos accepted |
|---|---:|---:|---:|
| 1.0, primary | 3/5 | 0/5 (0/25 sampled frames) | 12/12 |
| .65 | 0/5 | 0/5 | 0/12 |
| .4 | 0/5 | 0/5 | 0/12 |

The mixed-presence print source is accepted at scale 1, but that is not counted
as an empty-room error. Frozen t=1 rejects every external view, including all
five real photos at every scale. Zero attack acceptance alongside zero real
retention is not success. Five positive sources cannot establish fleet-wide
reliability, and repeated frames/scale variants are correlated.

All external real views had detections. At scale 1 their measured eye distances
fall in 15–30 px (three faces), 40–48 px (one), and 48–60 px (one); .65 falls in
15–30 (four) and 30–40 (one); .4 in 0–15 (three) and 15–30 (two). The numeric
evidence breaks all detected faces into 0–15/15–30/30–40/40–48/48–60/60–80/80+
pixel bins. These are observed landmark distances, not physical resolution
guarantees. No missing face is silently assigned a size.

## Latency and validation

Shared Intel iMac20,1/i7-10700K, macOS x86_64, Python 3.11.13, PyTorch 2.2.2,
OpenCV 4.10.0 and NumPy 1.26.4. Torch intra/inter-op are one thread. OpenCV was
asked for one but reports 16; no isolated-machine claim is made.

Boundary: **decoded RGB + supplied box → crop/tensor/model/softmax**. JPEG,
detector, camera, scheduling and publication are excluded. Construction is
172.705 ms, first face call 77.101 ms. All 1,434 later calls are retained:
median **10.812 ms**, p95 **19.719 ms**, max **83.276 ms**. The declared p95
≤10 ms host target **fails**. External detection separately has first 29.086 ms,
later median/p95 9.334/18.125 ms. Quantiles cannot simply be added into an
end-to-end quantile. ARM64/NPU, voice-load and actual presence delay are unmeasured.

Ten authored preprocessing/scoring controls pass. A separate scalar audit
(same builder, different implementation, no scorer import or inference rerun)
passes **15,794 assertions**, checking all hashes, sample alignment, probability
shape, calibration selection, held-out/source counts and latency percentiles.
There are no exact duplicate image bytes and no dropped rows. All 3,765 rows,
1,435 face outputs, source media, weights, protocols, scripts and timings remain
local. Public numeric evidence includes aggregate counts, source attribution
and script hashes only.

## Temporal alternatives: still unqualified

Blink evidence could use existing Blendshape V2 scores 9/10 (`eyeBlink`), with
one track's open→closed→open sequence, hysteresis and strictly increasing frame
times. The current expression reader is limited to one read every 2 seconds
(`crates/eyes/src/seems.rs::READ_EVERY`); it is not a validated blink sampler.
The mouth/mesh path and consent rules must be reviewed before any integration.
A passive blink can also occur in replay video; published experiments even
bypassed blink checks with manipulated prints
([Hofbauer et al., 2021](https://doi.org/10.1049/bme2.12015)). It cannot alone prove
physical presence.

Parallax would need measured camera translation, calibration and depth-sensitive
residual motion after compensating the camera/planar scene. Box-center movement
or mere head rotation is insufficient. Existing `speaking::JointHistory` binds
joint motion to capture time; it is not a calibrated depth/parallax estimator.
Flat moving screens, bent prints and missed/stale joint readings need controls.
Use only an already-occurring natural glance in a later authorized test; do not
add autonomous motion for this evaluation.

**Delay before presence is not measured for either alternative.** A blink rule
must wait for the completed sequence; a parallax rule waits for sufficient
natural motion and multiple valid frames. No event means no finite detection
guarantee. A configurable deadline N ends in unknown, never invented presence
or proven absence. Before choosing N, measure first-candidate→confirmed p50/p95,
unknown fraction and real retention, at candidate deadlines 2/5/10 seconds on a
fresh frozen temporal set. Those deadlines are proposed evaluation bins, not
qualified product limits. This set supplies neither labelled genuine blink
videos nor calibrated lamp-motion truth, so no such number is fabricated.
