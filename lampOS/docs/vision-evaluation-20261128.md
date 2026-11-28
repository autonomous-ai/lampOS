# Vision sets and conversion freeze, 28 November 2026

Rubric: **`vision-public-synthetic-20261128-v1`**. The executable recipe
is [vision-freeze-20261128-v1.json](../tools/sensors/vision-freeze-20261128-v1.json).
This freezes planned cases, splits, labels, bars and timing boundaries before
training/conversion results. It is not a claim that rendered media, AMI
selections or converted models already exist. Producers seal their exact
asset/row manifests as described below; missing material stays visible.

The owner's [16:32 direction](https://github.com/autonomous-ai/lampOS/issues/8964#issuecomment-7237307291)
authorizes public/synthetic vision work now. Monday's footage is a later
external test, **not a prerequisite for this work**. Include the
[OpenCV comparisons](https://github.com/autonomous-ai/lampOS/issues/8964#issuecomment-7237344737)
on the same cases. A result can pass this named dataset's bar without being
called a real-person lamp result. The historical [morning v2 rubric](sensor-evaluation.md)
and table keep their meaning; do not silently change their inputs or force
this recipe into v2's strict schema. Model training is authorized within
the source roles below. A dataset pass does not authorize merging, release
trains or product switch-on. The lead runs board legs.

## Source rights and roles

Use commercially permitted sources for training **and quantization
calibration**. Thresholds, crop/sign conventions, normalizers, model
selection and pruning are fitting too. Test inputs cannot influence them.

| Source | Frozen use | Evidence to retain |
|---|---|---|
| MakeHuman bundled assets / our renders | Train, tune, test in separate identity families | The [publisher](https://github.com/makehumancommunity/makehuman/blob/master/LICENSE.md) distinguishes CC0 bundled assets/output from application code and separately licensed community assets. Pin every mesh, material, hair, glasses and rig, with its own licence. |
| AMI | Train/tune/test by whole participant-connected session groups | The [official download page](https://groups.inf.ed.ac.uk/ami/download/) identifies CC BY 4.0 signals/transcription and the manual v1.6.2 annotations. Retain the exact downloaded asset licence, attribution and derivative description. Do not extend that statement to unrelated annotation packs. |
| Pointing'04 | Test-only diagnostic; never training, calibration or threshold/crop/sign fitting | Retain publisher terms, original images/labels and prior exposure. All 15 people were already evaluated in #8847; this is not a new unseen-person test. |
| Own TTS and Rhubarb visemes | Same split as voice/utterance/avatar family | Bind TTS model/voice terms and bytes. [Rhubarb's publisher](https://github.com/DanielSWolf/rhubarb-lip-sync/blob/master/LICENSE.md) identifies MIT software and user-owned generated lip-sync data; this does not license the input voice or avatar assets. |
| Random tensors / gray images | Conversion and timing smoke only | Not detection, pose, mouth, selection or population accuracy. |

Unknown or research/non-commercial-only rights do not enter training or
calibration for a shipping candidate. Software/weight licences and source
training-data rights are separate fields. Existing restricted public tests
can remain tests with their terms recorded. Keep actual people's media,
transcripts, identity joins and per-person rows local even when public at
the source; GitHub gets aggregates, hashes and source links.

## Freeze twice, without waiting for another approval

1. **Recipe:** this version fixes the role/case construction below. Common
   avatars and source clips keep the same role for every sensor/model.
2. **Materialized input:** before fitting, publish an aggregate seal and
   retain the exact local source inventory, licence records, complete
   ordered row list, labels and train/tune/test component map. Record failed
   rendering/download/preparation rows in that planned list. Freeze the
   300 calibration tensor hashes before conversion, then the selected
   model/config/thresholds before opening test predictions.

A producer may seal and proceed under these rules without waiting for
Monday or a new permission prompt. Changing a source, case, label, camera,
split, threshold or preprocessing after results creates a new version;
keep the previous one and mark any revisited test as exposed. A hash alone
is not proof that the split, source licence or label was checked.

Each manifest row binds: recipe/case ID, origin, role, identity/session/source
family IDs, original and derived file hashes, sample/frame bounds, geometry
and visibility truth, label basis, processing availability/failure, and
renderer/preprocessor/config hashes. Runs bind model weights/graph, complete
source/base/patch, actual executable, runtime/compiler/options, config,
input manifest, output and scorer hashes. The score records actual coverage
and source reuse, not only successful outputs.

## Shared synthetic identity and camera recipe

A00–A11 train; A12–A17 tune; A18–A23 test. Seed each avatar with 20261128
plus its numeric suffix. Freeze the full morph/mesh/material/accessory
parameters before fitting. Each group of six balances six skin materials,
three hair styles and glasses; these are rendered attributes, not inferred
properties of real people. All views, crops, expression/animation variants
and TTS/utterance families of an identity stay together. Shared MakeHuman
base assets are disclosed; six test avatars are not six human participants.

Use 320×240 RGB to match the current eyes input scale. The initial rig is
an explicit **proxy**: pinhole 70-degree horizontal field of view, centred
principal point, square pixels and zero distortion. The camera is below
the face (+10-degree subject elevation) and to one side (subject azimuth
-15 or +15 degrees by avatar parity). Distances 0.5/1.0/1.5 m are optical
centre to face origin. Day and warm-dim scene parameters are in the JSON;
they are rendering controls, not measured lamp lux or calibrated intrinsics.
Retain camera/world matrices and final pixel projections. An actual lamp
calibration can define a later version; its absence does not stop rendering.

The exact render label is relative to the face-to-camera ray, not a neck
joint's local Euler value. In camera coordinates x right/y up/z into scene,
let `l` be the unit ray from face origin to camera; `r=normalize(l×up)`,
`u=r×l`; let `h` be the head's outward forward unit vector.
Yaw is `atan2(h·r,h·l)`; pitch is
`atan2(h·u,sqrt((h·r)^2+(h·l)^2))`, in degrees, positive pitch up.
Keep the vectors/matrices so signs can be checked before model predictions.
Projected boxes, landmarks, occlusion and mouth tracks come from the
renderer; no detector output supplies its own gold.

## Sensor 4: heads, with independent shoulder variation

The 18 head poses in the JSON contain nine facing and nine away cases.
Facing gold remains inclusive `abs(yaw)<=60`, `-20<=pitch<=25`.
Cross all 24 identities, three distances, two lights, all poses and shoulder
yaw 0/75 degrees: **5,184 primary images** (2,592 train, 1,296 tune,
1,296 test). The test has **648 facing / 648 away**, 108/class in each of
the six distance/light cells. Pass requires **616/648 each class and
103/108 each class/cell**. Wrong, unknown, missing and unavailable primary
outputs fail. Report every identity, condition and shoulder-disagreement
slice plus yaw/pitch errors with their measured denominator.

The 288 separately fixed boundary images (12 poses × 24 identities) are
diagnostics, not extra primary successes. The current task is explicitly
**head-only**. An independent shoulder label is retained; do not relabel a
head result as passing the historical joint head-and-shoulder job.

Compare the frozen tract control, new neural FP32, its NPU conversion, and
OpenCV solvePnP using each declared landmark source. Pin generic 3D face
points, camera assumptions, PnP variant and signs before tests; no fitting
on Pointing'04. Time the full landmark-to-result path and the detector/mesh
it needs, not only the tiny solvePnP kernel.
YuNet's five marks remain a separately named five-point control; do not
invent a chin to call it the six-landmark mesh baseline.

Keep Pointing'04's **2,790 images** as a separate exposed public diagnostic,
with all per-subject counts, unknowns and the prior frozen reference/crop
conventions. A model tuned on it earlier is labelled accordingly. It cannot
be pooled with fresh rendered tests or used again to choose signs/cones.

## Sensor 5: one source among two or three faces

Each primary generated episode is **2.4 s, 60 RGB frames at 25 Hz and
38,400 mono samples at 16 kHz**. Keep exact audio/viseme/frame times and
actual emitting source. Use a causal declared context; output cannot use
future frames. Scripted visemes are synthetic truth, not independently
observed human articulation.

Use the JSON's seven type counts per distance/light cell. Train/tune/test
have **240/120/480 episodes**. Test cells each have 80: 24 slot-0 speakers,
24 slot-1, 16 slot-2 (three-face scenes), and 16 off-camera speakers.
This yields **384 visible / 96 no-visible**, with 192 two-face and 288
three-face episodes. Slots are left-to-right; freeze layouts and source
permutations using the JSON's row-order rotation, so voice or slot does not
encode the answer. Test requires
**456/480 overall, 76/80 in every cell, at least 365/384 clear-visible
recall and >=95% face-pick precision**. An explicitly correct `no_visible`
is different from unknown/missing. A wrong visible choice counts against
precision; zero choices does not manufacture a precision pass.

Non-speaking faces may move their mouths independently; own-source
background audio cannot reveal the target through a headset channel.
Keep ambiguous/multiple-source or unobservable cases as declared diagnostics
or primary failures, never exclusions chosen after scores. A frame/window
inside an episode is not a new independent utterance.

Compare Light-ASD/other declared neural candidates with a landmark-mouth
motion plus shared-audio-energy baseline. Match source episodes, context,
preprocessing and cutoff. Report each model's own declared window and any
difference; a T50 versus T25 comparison is not identical context.

For AMI, keep entire a/b/c/d session families and all recordings/cameras of
a participant in one connected component. Exclude the already viewed
ES2002/IS1000 components from new fitting/test selection in this version;
retain them as diagnostics. The JSON defines deterministic component
partitioning and hash-ranked eligible case selection. Seal at least
120 target-speaking and 120 other-speaking planned test windows before
predictions; retain unavailable inputs and exact label/guard rules.
Candidate windows are 2.4 seconds at three-second grid starts, with one
annotated lexical speaker covering at least one second. A is that target;
Q is another source with no target vocal event within the window plus
0.5-second guards. Malformed or ambiguous annotation stays explicit. Select
one camera observation per underlying episode; preserve its original PTS.
No overlapping windows or alternate-camera copies as independent cases.
Use the same room/mixed microphone for all face targets. Annotation-derived
speaker timing is a proxy unless independently checked against audible and
visible behavior. A single close-up A/Q result is not multi-face selection.
Unknown distance, lighting or physical A/V error stays unknown.

## Eyes and conversion controls

Tests owns exact YuNet, Face Mesh V2 and blendshape model/input pins.
Use the same source frames and exact preprocessed tensors for the CPU
control and converted backend. Separate import/FP32 changes, quantization
changes and end-to-end preprocessing changes. Preserve channel/layout,
range, crop/resize, fixed shape/context, quantization scale/zero-point,
output ordering and dequantization. A changed model is a new candidate,
not just a runtime comparison.

Quantization calibration uses **300 distinct training-derived tensors**
(50 per synthetic distance/light cell), never tune/test frames or
Pointing'04. Reused episode frames are disclosed. A synthetic random tensor
cannot establish accuracy or replace representative calibration inputs.
Both CPU and NPU see identical test IDs. Report raw tensor shape/nonfinite
failures, max/mean/p95 absolute error and RMSE, plus decision agreement,
correct-in-both, gains, losses and unknown-in-both.

Detection uses independent projected face boxes, one-to-one IoU >=0.5
matching, >=95% recall and precision, and **137/144** test no-face rejections.
The additional own-rendered no-face rows are 144 train / 72 tune / 144 test.
For mesh, pin six anatomical anchors (nose tip, chin, outer eye corners,
mouth corners) and their model/render correspondence before predictions:
>=95% planned faces must meet the JSON's normalized visible-anchor error
bar. Report visibility and missing correspondence rather than deriving
gold from CPU output; all-478-point CPU/NPU drift is a separate parity
measure. Blendshape conversion requires zero missing/nonfinite outputs and
max absolute coefficient drift <=0.05 on the identical inputs; downstream
head/mouth bars still apply. Coefficient agreement is not emotion truth.

Each candidate must meet its own labelled-set bar after conversion. Report
accuracy cost in counts and percentage points; unchanged accuracy with
different lost/gained cases is not exact parity. Public/rendered scores,
numerical conversion checks and actual lamp observations remain separate.

## NPU and OpenCV timing

The 2.945 ms sample-network mean in #8964 is a smoke result, not a face,
mesh, Light-ASD or full-pipeline result. Record evidence of actual backend
execution and any CPU operators/fallback. OpenCV DNN/TIM-VX availability
must be established by the kit, not inferred from a loaded VIPLite driver.

The board bar is **>=10 fresh decisions/s**, with full-frame service
p95 <=100 ms beside the voice. Sensor 5's separate **<=40 ms p95 fresh
decision-work** target is not met by dividing a T50 call by 50. Include
all required faces in the decision. Report the first usable output including
context filling, cold model load, queue wait, every processing stage and
actual publication. Keep frame capture-to-publication <=350 ms and source
age <=500 ms where those live timestamps exist; a replay has null readiness.
Frozen T50 conversion/parity work can proceed for the released Light-ASD
model. Its symmetric temporal layers and reverse GRU do not establish an
incremental stateful implementation. Record target-frame alignment and any
lookahead separately; time a complete-window replay as such. An incremental
model change needs its own source/weights and accuracy result on the same
sealed test, without tuning against that result.

Retain one cold run, 10 explicitly excluded but saved warmup frames,
at least 300 measured fresh frames and a five-minute sustained run with
temperature/frequency/load, repeated/dropped frames and deadlines. Show
CPU and NPU full-pipeline p50/p95/max on identical inputs and settings;
repeating a cached output is not fresh throughput. A short board slot may
report a partial result, not a sustained pass. Include fallback/error
behavior and the effect on the concurrent voice; no sensor may wait the
voice. Only the lead operates the lamp or stages self-contained kits.

## Storage and EVAL

Keep each complete dataset **below 5,000,000,000 bytes**, with shared assets
accounted for once and referenced by hash. Estimate size on a declared
training batch before producing the full fixed plan; use lossless video
as the canonical directly encoded render output. Do not silently reduce
resolution, clip counts or quality after seeing results. If the plan cannot
fit, retain the attempt and issue a revised pre-result recipe; do not delete
data or call a partial set complete. Build-cache cleanup is separate and
only its owner performs it. Retain raw media, every failed attempt, sources,
licence/attribution records, labels, inputs, model outputs, README and full
SHA-256 inventories.

Use the [vision EVAL template](../tools/sensors/vision-eval-template.md).
Report `SET_BAR_PASS/FAIL/NOT_RUN`, `CONVERSION_PASS/FAIL/NOT_RUN` and
`STREAM_PASS/FAIL/NOT_RUN` separately, with source/coverage limits.
These states describe this named dataset and measurement. They do not
retroactively qualify a morning v2 report, imply real-person accuracy or
authorize a combined-judge integration.

