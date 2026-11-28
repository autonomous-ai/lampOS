# EVAL: method versus rendered heads

28 November 2026, follow-up to [the original failed PnP evaluation](README.md).

**SET_BAR: FAIL for both candidates. No qualification, training, product change or device access.** The unchanged PnP method has substantial error on real-photo labels; original 6DRepNet has much smaller error there but much larger error on these renders. This supports both a method limitation and a render-to-photo distribution gap. It does not prove the render labels wrong, isolate the cause of the gap, or establish lamp accuracy. The existing geometry audit still stands. 6DRepNet is one published baseline, not a claim about the best available model in 2026.

## Frozen comparison and denominators

The [final protocol](https://github.com/autonomous-ai/lampOS/issues/8847#issuecomment-7240908034) was published before new dataset inference. We reused PnP's original synthetic predictions, ran its unchanged core once on 2,000 real photographs, and independently ran the frozen 6DRepNet adapter once on both sets. There was no new fitting, threshold selection, training, or test exclusion. All 36 real annotations outside ±90° on any native Euler axis remain included. Separate detector instances produced identical detections on all 2,000 real inputs; 6DRepNet did not depend on PnP succeeding.

The locked selected cone is `abs(yaw) <= 63`, `-3 <= pitch_up <= 10`. The predeclared reference diagnostic is `abs(yaw) <= 60`, `-20 <= pitch_up <= 25`. The synthetic gold class uses the reference cone. With exact gold ray angles as predictions, the narrow selected cone catches only **360/648 (55.56%)** facing cases, while rejecting 648/648 away; the reference oracle is 648/648 both ways. This is a decision-rule mismatch for a calibrated estimator, not a mathematical upper bound for every biased predictor. Neither cone was changed after observing results.

There are 1,296 primary render cases, 648 per class, plus 72 boundary diagnostics. Qualification needs at least 616/648 in each class and 103/108 in each distance/light/class cell. Unknowns fail in their original class denominator; they are never correct away results. Correlated renders are not independent people. Real labels provide no measured distance, light, intrinsics or exact nose-to-camera ray, so no real distance/light cells or lens-facing qualification are invented.

## True synthetic nose-ray test

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 302/648 (46.60%) | 445/648 (68.67%) | 620/648 (95.68%) | 260/648 (40.12%) | 43 |
| 6DRepNet / renders | 311/648 (47.99%) | 500/648 (77.16%) | 526/648 (81.17%) | 334/648 (51.54%) | 40 |

Locked-cone cells; every row fails for both methods:

| Distance / light | PnP facing / away / unknown | 6DRepNet facing / away / unknown |
| --- | ---: | ---: |
| 0.5 m / day | 49/108 · 91/108 · 2 | 47/108 · 93/108 · 0 |
| 0.5 m / warm dim | 57/108 · 83/108 · 9 | 51/108 · 84/108 · 8 |
| 1 m / day | 49/108 · 84/108 · 6 | 51/108 · 88/108 · 6 |
| 1 m / warm dim | 46/108 · 68/108 · 5 | 53/108 · 88/108 · 5 |
| 1.5 m / day | 45/108 · 63/108 · 8 | 54/108 · 75/108 · 8 |
| 1.5 m / warm dim | 56/108 · 56/108 · 13 | 55/108 · 72/108 · 13 |

Boundary-only results are PnP 17/48 facing and 16/24 away, and 6DRepNet 15/48 and 18/24, with zero unknowns. They never enter primary rates or fitting. PnP primary unknowns are 36 no-face, four ambiguous-face and three back-facing solutions; 6DRepNet has the same detector misses and ambiguities, without the three PnP-specific failures.

## Common camera-normal diagnostic

Both sides use the physical face normal relative to the optical axis, not the off-axis ray to the lens. For physical CV rotation R, `h = R * [0,0,-1]`, `yaw = atan2(h.x,-h.z)`, `pitch_up = atan2(-h.y,hypot(h.x,h.z))`. Real gold classes comprise 1,514 inside and 486 outside the reference cone; synthetic camera-normal classes are 648 each, but need not contain the same rows as ray classes.

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 34/648 (5.25%) | 606/648 (93.52%) | 606/648 (93.52%) | 347/648 (53.55%) | 43 |
| 6DRepNet / renders | 235/648 (36.27%) | 520/648 (80.25%) | 500/648 (77.16%) | 354/648 (54.63%) | 40 |
| PnP / AFLW2000 | 1185/1514 (78.27%) | 210/486 (43.21%) | 1451/1514 (95.84%) | 118/486 (24.28%) | 120 |
| 6DRepNet / AFLW2000 | 408/1514 (26.95%) | 436/486 (89.71%) | 1412/1514 (93.26%) | 359/486 (73.87%) | 87 |

Real unknowns are PnP 120/2,000 and 6DRepNet 87/2,000: both have 52 ambiguous, four clipped and 31 no-face inputs; PnP adds 33 back-facing solutions. There were zero processing exceptions, missing inputs or dropped rows in all three new runs.

## Native rotation error, measured outputs only

| Method / set | Measured / planned | Yaw MAE | Pitch MAE | Roll MAE | Mean SO(3) error |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / renders | 1253/1296 | 11.06° | 23.48° | 18.31° | 19.44° |
| 6DRepNet / renders | 1256/1296 | 15.38° | 20.59° | 16.57° | 21.45° |
| PnP / AFLW2000 | 1880/2000 | 7.49° | 14.94° | 10.28° | 17.21° |
| 6DRepNet / AFLW2000 | 1913/2000 | 3.13° | 6.09° | 4.72° | 5.56° |

The native convention is `M = Rz(roll) Ry(yaw) Rx(pitch)`. Physical CV rotation is `D Mᵀ D`, where `D = diag(1,-1,-1)`. This source-derived convention was frozen before inference and checked against the upstream drawing algebra and mirror-bundled rotation code. Euler differences are absolute, unwrapped and unclamped. SO(3) geodesic error provides a separate rotation comparison; its acos argument is clipped only for floating-point safety. The table is conditional on measured outputs, so its coverage must accompany every error claim.

Real predicted-ray versus camera-label agreement is a separate **cross-frame proxy**. It supplies no ray angle-error or facing-ground-truth claim:

| Method / set | Locked facing | Locked away | Reference facing | Reference away | Unknown |
| --- | ---: | ---: | ---: | ---: | ---: |
| PnP / AFLW2000 | 535/1514 (35.34%) | 345/486 (70.99%) | 1422/1514 (93.92%) | 176/486 (36.21%) | 120 |
| 6DRepNet / AFLW2000 | 172/1514 (11.36%) | 443/486 (91.15%) | 1115/1514 (73.65%) | 417/486 (85.80%) | 87 |

## CPU timing and adapter

| Method / set | First call, ms | Later median, ms | Later p95, ms | Later calls |
| --- | ---: | ---: | ---: | ---: |
| PnP / renders | 14.288 | 13.450 | 18.815 | 1367 |
| 6DRepNet / renders | 211.032 | 168.749 | 228.435 | 1367 |
| PnP / AFLW2000 | 12.863 | 8.248 | 13.299 | 1999 |
| 6DRepNet / AFLW2000 | 149.299 | 154.442 | 247.282 | 1999 |

Timing starts at decoded RGB and ends after detector, crop, pose and output geometry. File reading, hashing, decoding and model construction are recorded separately in [the aggregate JSON](method-comparison-numeric.json). The first call is cold; later calls have no extra warmup. Historical synthetic PnP timings came from the original process and exclude the newly derived camera/native algebra. Render timing includes the 72 boundary cases; primary accuracy does not. Early detector abstentions remain in all-call timing. Pose-called timing is separately available in the JSON.

Host: Intel iMac20,1, i7-10700K, 64 GiB, macOS 26.6.2; Python 3.11.13, NumPy 1.26.4, OpenCV 4.10.0, Pillow 10.4.0, ONNX Runtime 1.18.1. ORT used CPU, batch one, sequential execution and one intra/inter thread. OpenBLAS was set to one; OpenCV requested one thread but reported 16 with GCD. This shared host was not isolated. These are descriptive Mac measurements, not ARM latency, camera-to-publication latency or a voice-impact result.

6DRepNet used the independent identical YuNet detector (score 0.6, NMS 0.3, four-pixel minimum, exactly one unclipped box), 20% cross-axis padding from the upstream demo, RGB PIL bilinear Resize(224)/CenterCrop(224) and ImageNet normalization. No gold box or landmark entered prediction. This new PIL adapter differs from the earlier Rust bilinear benchmark and does not establish product parity. The FP32 batch-one opset-13 ONNX export is 157,316,333 bytes; the source checkpoint is 157,315,191 bytes.

## Provenance, licences and reproducibility limits

[Original 6DRepNet source](https://github.com/thohemp/6DRepNet/tree/464b2ba55c3d9b3d3b707c6271cf329a983ded20) is MIT; the publisher [explicitly applies MIT to its fine-tuned weights](https://github.com/thohemp/6DRepNet/issues/27#issuecomment-1214677538). Its [README](https://github.com/thohemp/6DRepNet/blob/464b2ba55c3d9b3d3b707c6271cf329a983ded20/README.MD) describes 300W-LP training and AFLW2000 evaluation; the training source initializes a RepVGG-B1g2 ImageNet backbone. Training/test image or identity overlap was not independently audited. The checkpoint filename is not evidence that AFLW2000 trained it. [YuNet, canonical-face and selected render-asset terms](METHOD.md) are unchanged.

All 2,000 JPEG/MAT pairs came from the anonymous [AFLW2000-3D mirror, version 1](https://www.kaggle.com/datasets/mohamedadlyi/aflw2000-3d), after the original HTTP endpoint returned 404 and HTTPS timed out. ZIP SHA-256 is `97b7b41068f1757dc82ac5f01ad2bc5d15f0f83bffab96ee56607537e3cd7f90`, 87,473,487 bytes. Its checksum differs from the original TFDS ZIP checksum: publisher-byte authenticity is not asserted. The original [AFLW terms](https://www.tugraz.at/institute/icg/research/team-bischof/learning-recognition-surveillance/downloads/aflw/) restrict use to noncommercial research and restrict redistribution; mirror metadata does not supersede them. This is a research diagnostic, with no product-data clearance claimed. Images, annotations, per-image predictions, scripts and private paths are not published. AFLW citation: *Annotated Facial Landmarks in the Wild: A Large-scale, Real-world Database for Facial Landmark Localization*, ICCV workshop, 2011.

Generated controls passed: 21 adapter checks, eight scoring checks, 11 matrix-convention checks, four generated tensors matching the official PyTorch model within 0.001°, and three RGB preprocessing arrays matching the official transform exactly. A separately implemented scalar recount passed 53 checks without importing the scorer, opening images or calling a model. It checked row/image-hash/order completeness, all primary class/cell counts, native Euler MAE, proxy suppression and identical real detector outputs. This is an independent implementation check by the same builder, not an independent reviewer sign-off. The reviewer still needs to read the evidence.

| Artifact | SHA-256 |
| --- | --- |
| Frozen comparison protocol | `94a984770b293455c87b13d3614c937e2110f348c0fc528e3f57bfc6ca78061d` |
| Real adapter manifest | `47e466c7bdec8f9570eb07aa001544b37f5179876b6855a776cfb096ebaa3739` |
| Synthetic adapter manifest | `4fbe3bf5f311299459effad2929630cd1621c72b279b1006259685bf550f95d8` |
| 6DRepNet checkpoint | `9bdcd0e072df0d21e92288b40ea1d1b3cf7173c44728ab9c45c4d673f361e018` |
| 6DRepNet ONNX export | `8b9236159f30c7285d8b8e0b1f0cf5bbf2506f35a256e4cb62119ed7a9ffa1c6` |
| Private complete report | `2fbb7c8e9e699def4efff935a729e7fd332de8fb8b279acd50aed51f2c3e5f8b` |
| PnP real predictions | `f3b250b6715c2a063edad03364cc42b5b234815b3865c043660601db7793a232` |
| 6DRepNet real predictions | `283bf05fdd1419a78161946ed782ae8908c61159f3a15e6116d6c0c0f4a577c2` |
| 6DRepNet synthetic predictions | `1ba9cb7144cbd799efc16f02d3d9956e4b3cdc8b75da4b309a3878ef97bbe1ac` |


The private protocol also binds every source/runtime/input hash, mirror record, oracle and earlier exposure. Reproduction requires lawful access to those exact inputs and retained source; the public aggregate JSON alone cannot reproduce inference. Neither previous failures nor missing metadata are replaced by new estimates.

[Original EVAL](README.md) · [Vietnamese comparison](method-comparison_vi.md) · [Aggregate numbers](method-comparison-numeric.json)
