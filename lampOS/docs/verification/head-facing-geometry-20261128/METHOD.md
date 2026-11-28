# Sensor 4: frozen head-facing geometry evaluation

This is an offline evaluation of the existing YuNet detector's five landmarks
plus a fixed generic 3D face and OpenCV solvePnP. No new model was trained. It
does not install, enable or change a lamp feature. The accompanying evaluation
report contains the outcome; source controls alone cannot establish accuracy.

The Python measurement tools, model binaries, images, full plan, materialized
manifest and runtime configurations remain private. Repository contribution
rules reserve committed Python for model-making training code; this evaluation
does not qualify for that exception. File names below identify the retained
inventory, not downloadable public scripts.

## Method and source inventory

The frozen primary is `yunet_five-nose-ray-hfov70-minbox4-v2`. The source version
is `opencv-pnp-canonical-nose-ray-hfov70-minbox4-v2`.

| Retained file | Role | SHA-256 |
| --- | --- | --- |
| `detector.py` | YuNet input, decoding and face admission | `a31c9b098b45a1778324237458613182fa78ebadf035d0954217a5d3de01d334` |
| `geometry.py` | Canonical correspondences, PnP and angle conventions | `dad6f51b08af309e7546ba5f43f62e273a706873529555848cb3ae86d3dd2430` |
| `mesh_model.py` | Optional mesh control; not imported by primary | `c04d74f0f5835671a98a4b6ddf5215d71bd37d5ee165394bb069ac9b0b1ffb6a` |
| `replay.py` | Locked replay, unknowns, raw results and timings | `a188daca80f05fc5ca0ac0374d7a851af0418c9363221abe68dbad68012c2f45` |
| `flatten_pnp.py` | Hash-bound adapter to the scorer | `b9b5212e0216e43ab0d2b4b8f9f5f72b3d956f94e7f5d1d085b0c056363ba25e` |
| `score.py` | Tune-only cone selection and one test score | `fd767b0e84b02034ac58e0fa9256ddef80cbe7a3ce778f4a3c2d2e7c786f2d9f` |
| `derive_plan.py` | Deterministic ordered recipe expansion | `673f39c79e90ee486e0572703e87d8ce3cb0a5ecd302b6ec4c225a4c940bc8c9` |
| `local_replay.py` | Same-process runtime attestation | `02f1de0df244735684c16d97010ea988fad7e803b6ddbf63660babbc5e0d215f` |

The private inventory also retains the three original unit-test modules,
mathematical projection controls, integration and scorer controls, text licences,
the canonical OBJ and historical source-readiness records. That readiness
snapshot predates the study; its `NOT_RUN` fields and original test counts are
not the current evaluation or a new packaging-check result.

Synthetic input is 320×240 RGB. YuNet receives a padded 320×256 BGR float32 frame using
the frozen pixel-centre sampling rule. Score admission is 0.6 with NMS IoU 0.3;
there must be exactly one finite, unclipped full-frame box. The primary retains
the decoder's four-pixel box minimum. The legacy 7.5% width heuristic applies
only to separate optional controls, not this candidate.

The generic face uses MediaPipe's canonical centimetre coordinates transformed
by `diag(1,-1,-1)`, without recentering. The five correspondences are the mean
of eye corners 33/133, the mean of 263/362, nose 1 and mouth corners 61/291.
EPNP is followed by LM refinement (20 iterations, epsilon 1e-8). Nonfinite or
degenerate points, invalid depth or rotation, a back-facing normal, and
reprojection RMS above 5% of the face-box diagonal produce explicit unknowns.

PnP assumes 70° horizontal FOV, square pixels, principal point
`((W-1)/2,(H-1)/2)` and zero distortion. This matches the synthetic recipe by
construction; it is not a measurement of real lamp or Pointing'04 optics. Gold
and predictions use yaw to the right and pitch **up**, relative to the
anatomical nose-tip-to-camera ray. Raw camera Euler pitch has a different sign
and remains a separate output. No true pose, projected landmark or gold box
feeds the detector/PnP prediction path.

## Frozen roles, scoring and preparation

The [Reviewer's recipe](https://github.com/autonomous-ai/lampOS/blob/3a8934944acba1c17f3cb0267aae778410f5809e/lampOS/tools/sensors/vision-freeze-20261128-v1.json)
has SHA-256 `f8b57acf50c5cac4f472326fbe5a73bf0df3b3aa348235ce453e42c0c8e4423a`.
Its ordered plan has SHA-256
`321a78927cd87c585d1cc7a53bc7fcbc7f8dc22bfcdbe08a03fb5b3b4eb31e8f`.
All 5,472 rows remain present: 2,736 unavailable train rows (A00–A11), 1,368
tune rows (A12–A17), and 1,368 test rows (A18–A23). Each evaluated role has
1,296 primary cases plus 72 separate boundary diagnostics. Training and
training-derived calibration remain held. Seven A00 development previews are
excluded from the study.

Each identity crosses three nose-to-camera distances (0.5/1/1.5 m), day and
warm-dim lighting, 18 head poses, and independent shoulder yaw 0°/75°. Head
labels do not qualify shoulder estimation. The reference facing cone is
`abs(yaw) <= 60`, `-20 <= pitch_up <= 25`, with inclusive boundaries.

Only tune-primary predictions select the integer cone: yaw limit 1–90°, lower
pitch −60–0°, upper pitch 0–60°. Selection maximizes the worst class/cell count,
then the worst class count, then total correct; ties minimize distance to the
reference cone and finally use lexicographic order. Every class needs at least
616/648 correct, and each class in every distance/light cell needs 103/108.
Unknowns remain failures in their original denominators. Boundary rows never
enter fitting or primary rates. A failed tuning bar stays failed; a later test
is diagnostic and cannot repair it.

Reproduction needs the exact retained plan, materialized manifest and permitted
input images, plus the pinned model and unchanged source. Raw predictions,
receipts, adapter inputs, fitted cone, test attempt and runtime bindings must
remain cross-bound by SHA-256. Missing or reordered cases and changed inputs
refuse scoring. Existing attempts must not be overwritten. Pointing'04's 2,790
previously exposed photographs are a separate diagnostic, with no fitting,
calibration or new-holdout claim; the photographs are not redistributed.

A private packaging check regenerated the plan with Python 3.11.13. Generation
exited successfully, but approximately 1e-16 differences in serialized matrix
entries changed its SHA-256 to
`3c5cdce6c53bbb995e341fd7eb9e3daefdffd39fe6f9abe830baf45c5cc491a1`.
The exact-hash gate refused it. Use the retained frozen bytes; do not relax the
scorer or substitute numerically close JSON. The original generation Python
version was not established by that check. The already-audited frozen plan and
materialized dataset were not changed.

The primary needs NumPy and OpenCV, without ONNX Runtime. The measured local
environment was Python 3.11.13, OpenCV 4.10.0 and NumPy 1.26.4 on x86_64. Its
core requests one OpenCV thread but the actual build reports 16. File-read and
decode through detector/PnP result timing is not camera capture-to-publication
latency or ARM qualification. Optional mesh controls have their own runtime
dependency and do not alter the primary result.

## Sources, licences and limits

The pinned YuNet model is 232,589 bytes, SHA-256
`8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4`.
The [official model directory](https://github.com/opencv/opencv_zoo/tree/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet)
explicitly applies its [MIT licence](https://github.com/opencv/opencv_zoo/blob/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet/LICENSE)
to all files there. The retained licence SHA is
`c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958`.
The model is not redistributed in this documentation.

The [canonical MediaPipe face](https://github.com/google-ai-edge/mediapipe/blob/2f787dbfe2cb65152d3be409aaff8484a4e8f21b/mediapipe/modules/face_geometry/data/canonical_face_model.obj)
is a 45,999-byte text asset, SHA
`8bac80443397e113f41a8b565ea72c59390bc031d9defab289dba7bc0c54e618`.
Its retained [Apache licence](https://github.com/google-ai-edge/mediapipe/blob/2f787dbfe2cb65152d3be409aaff8484a4e8f21b/LICENSE)
SHA is `8707eef0533987efc5b155d64761eeb6e20793f50b9bd1a68dad1cf4719d0ed8`.
OpenCV's separate [4.10.0 licence](https://github.com/opencv/opencv/blob/4.10.0/LICENSE)
snapshot is `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`.
A code licence is not automatically a licence for unrelated pretrained weights.

The [shared A00–A23 avatar recipe](https://github.com/autonomous-ai/lampOS/tree/3fb28e9e207091cf20835c7d64e8d3f9d56dcff7/lampOS/training/synthetic-avatars-20261128)
uses MPFB 2.0.18 commit `d9648f3257a0b9712281d466245f30d656250154`,
fixed macros and nine ordered facial targets. MPFB's program code is
GPL-3.0-or-later; its selected mesh, targets, rig and output assets are separately
CC0-1.0. Sections C/D of its [licence overview](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.md)
cover assets and scripted/rendered output. These terms support commercial use
of output from the selected CC0 inputs, without making the program or helper
code CC0. Overview snapshot SHA:
`5cefb60680cb9efd4550a2e65d719021cfbec9dad3658481d11d48ae863ce04d`;
[CC0 text](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.ASSETS.md)
SHA `f6089cba01cb570a24712b41ab8a586ccd3cc5ef53dc266ca50b95c288956d2c`;
[program licence](https://github.com/makehumancommunity/mpfb2/blob/d9648f3257a0b9712281d466245f30d656250154/LICENSE.CODE.md)
SHA `26c03bc049c15fc5b9e52d791a06f1a6bc32f89edc8f8c701ab0aadf66e85b82`.

The 42 selected external files have separate CC0 asset/header records: 38
members of the [system asset pack](https://static.makehumancommunity.org/assets/assetpacks/makehuman_system_assets.html)
and four members from the [glasses pack](https://static.makehumancommunity.org/assets/assetpacks/glasses01.html).
The asset seal SHA is
`bb5c47367f2b4971aaa519408c5667c71b7ae6ff8dec044b9e685b90b1e9c9ed`.
Two glasses with conflicting AGPL headers were excluded. No blanket licence
claim covers all community assets. The extracted headers are hashed; the
reviewed asset-pack HTML pages were not retained as hashed snapshots.

Blender 4.5.17 EEVEE Next, 32 samples, produced retained lossless PNGs from
locally authored scenes. Twenty-four distinct procedural neutral geometries
share one base topology and rig, six skins, three hair classes, one glasses
shape and one outfit. Only twelve identities have evaluation frames. Correlated
procedural choices are not independent human subjects or population coverage.
Static heads and two lighting presets do not reproduce real interactions,
gaze, measured lamp optics or joint head/shoulder truth.

## Packaging verification record

The attempted source package verified 26 exact copied files (328,080 bytes)
against its inventory and syntax-compiled the copied Python sources. Plan
generation exited 0, then its exact-SHA check failed as described above. The
remaining unit, mathematical, scorer and adapter checks did **not** run in
that attempt. Six original unit tests load models and were explicitly excluded
from the proposed no-inference suite. No model, study image, training job,
download or device was accessed while preparing these notes. Earlier evaluated
controls and final EVAL results have their own receipts; they are not replaced
by this incomplete packaging check.

[Evaluation results](README.md) · [Vietnamese results](README_vi.md)
