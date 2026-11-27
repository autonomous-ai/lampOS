# Head direction: comparison and isolated facing observation

**Sensor 4 draft, 2026-11-27:** the runtime schema/2 and crop-selection
changes below are implemented on the isolated simple-call base
`0dbbfc95bbea6ddf397308e1a8ef1933c5d19dcb`. The Reviewer's v2 scoring contract
is frozen at `31c290d5bb9dc845679098a837a4d0ffd5059cb4`. Native Mac and
Linux tests, strict Clippy and formatting pass; native ARM64 and board
measurements are still required. This candidate is a **head-only
proxy**, with shoulders unknown, and cannot qualify the joint head-and-
shoulders job alone. Product defaults remain unchanged.

For [#8847](https://github.com/autonomous-ai/lampOS/issues/8847), the
`head_direction_compare` example compares the existing five-mark geometry
with the checked geometry from the closed
[#8782](https://github.com/autonomous-ai/lampOS/pull/8782) draft. A separate,
default-off runtime observer counts its availability using meshes the eyes
already computed. The sensor 4 extension adds fixed-rule facing counts to
that same observation boundary. Neither path establishes accuracy, decides
that a person faces a screen or phone, or changes a product unit, bus field
or voice policy.
The observer adds no camera reader, inference or worker thread.

The pure geometry moved from `examples/head_direction/geometry.rs` to
[`src/head_direction/geometry.rs`](../crates/eyes/src/head_direction/geometry.rs)
without changing its math. The
[`head_direction_compare` example](../crates/eyes/examples/head_direction_compare.rs)
now imports that shared implementation; this does not relabel the earlier
offline results or qualify runtime behavior.

The earlier head-target goal is at least 95% recall separately for lamp, screen and
phone head-facing targets on the Reviewer's shared, independently labelled
set: at least 80 held-out episodes per target and six speakers. Head
direction is distinct from eye gaze, speaker identity and intended
recipient. Neither the offline comparison nor runtime availability counts
establish any of those goals. These three targets are historical: the
published v2 contract below replaces them with jointly labelled head-and-
shoulder `facing`/`away` classes. The observer's head-only `not_facing`
counter is not automatically the v2 joint `away` result.

## Frozen v2 head-and-shoulders contract (2026-11-27)

Use the [Reviewer's frozen #8885 contract](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7221984822),
source `31c290d5bb9dc845679098a837a4d0ffd5059cb4`, in
`tools/sensors/adapter-v2.md` and `docs/sensor-evaluation.md`. Manifest and
run both use `version: 2`; the manifest uses
`rubric_id: "sensor-team-2026-11-27-v2"`. This scorer version is separate
from `lamp-head-direction-observer/2`, which remains a count-only journal
format with no per-trial gold or live readiness.

Label `head_target` and `shoulders_target` independently as `lamp`, `away`
or `unknown`, keeping `look_target` separate. Head expected/output classes
are `facing` and `away`: both known toward lamp means facing; with both
components known, at least one away means away. Collect at least 120 cases
per class, with the bar >=114/120 for each, including at least 20
head-toward/shoulders-away and 20 head-away/shoulders-toward cases. An
unknown component needs independent adjudication
(`head_gold_adjudicated: true`) to enter answerable accuracy; otherwise
use expected `unknown` and a fault diagnostic. Independent gold does not
turn an unknown prediction into a correct result. Primary missing,
unknown or late results fail; safe fault abstention is separate.

Qualification requires at least six real holdout speakers, independent
frozen labels and disjoint development/holdout people and sessions. Use
verified source clocks (`clock_verified: true`, measured error <=5000 us)
and pin the producing executable with `producer_binary_sha256`, alongside
source `build_sha`, model/config hashes and `source_patch_sha256` for any
uncommitted source, including untracked source. Source READ-OK, a public
benchmark or an invented adapter fixture supplies none of that evidence.

Sensor 4's v2 timing bar is source capture to usable result p95 <=100 ms
locally or <=300 ms on a server, with 500 ms expiry; opportunity-to-capture
and opportunity-to-result remain separate. The observer keeps its own
stricter 250 ms rejection ceiling unchanged. A replay's `ready_us` is
**null** and only `replay_elapsed_us` may describe host duration; it cannot
claim live association, framing, admission or playback outcomes. Invented
adapter fixtures check the contract only, not real data or qualification.

Keep every original input, annotation, failed run, derived result and
previous score privately, with immutable inventory/README hashes and
retention consent. The owner's **keep all data, never delete** instruction
supersedes old deletion dates: v2 uses `retention: "keep_all"` and rejects
`delete_after_unix_s`. Historical v1 deletion fields are not instructions
to delete evidence. Public reports contain aggregates, provenance and
limits, never real-person recordings, words, private paths or per-person
records.

The compatible metadata extension at
`ea4a33ac7780df93f421681b2ffe9039d71133b0`
([Reviewer record](https://github.com/autonomous-ai/lampOS/issues/8850#issuecomment-7222264639))
permits null consent/clock error/distance/bearing for public, generated or
invented development data, and unknown light/noise with an unverified
clock. It changes no v2 quality bar or real-person holdout requirement.
It does not create shoulder gold or capture timestamps for these static
public photos. The earlier invented adapter check remains pinned to its
actual `31c290d5` source and executable, rather than relabelled as a run of
this later scorer.

## Default-off runtime observer

`lamp-eyes --head-direction-observer on|off` accepts exactly `on` or `off`
and defaults to `off`. No product unit enables it. Enabling this explicit
observation option does not enable the mouth model, pitch/gaze, a camera
permission, a bus publication or a policy consumer. With it off, no observer
is constructed. With it on, the
[`Observer`](../crates/eyes/src/head_direction/mod.rs) borrows the current
processing look's picture, detections and existing mesh results; it does
not infer another mesh, copy a frame for analysis, change the look cadence
or choose a person for the voice.

The observer is called around the existing capture open/read and at the
existing look's geometry boundary in
[`main.rs`](../crates/eyes/src/main.rs). Its
[`freshness` guard](../crates/eyes/src/head_direction/freshness.rs) checks
capture/source identity, permission generation, sequence, monotonic source
time and source age again after the borrowed geometry. Capture metadata and
tickets are local only. A close clears the observer's active receipt; a
pending look is counted as cancelled. These sampled checks do not implement
an atomic revoke/publish fence, cancel existing inference or authorize a
pose export. A future raw-pose publisher and its consumers still need their
reviewed final authority boundary.

### Journal record and denominator

At most once per 60 seconds, when no look is pending, the existing main loop
may write one `lamp-eyes: head_direction_observer {JSON}` journal line. A
delayed loop produces a longer `window_us`; this is elapsed duration, not an
event timestamp or a fixed wall-clock minute. After a report, its counters
reset. This is one bounded, fixed-schema record, not a per-look trace or
durable history recorder:

| Field | Meaning |
| --- | --- |
| `schema`, `method` | Fixed `lamp-head-direction-observer/1` and `mesh_head_3a9ae0692_checked`. |
| `window_us`, `frames`, `looks` | Elapsed reporting duration, existing camera frames observed, and existing processing looks begun. Frames and looks are different denominators. |
| `outcomes` | One primary outcome per completed or cancelled processing look. Availability means usable geometry passed this observer's gates, not correct direction. |
| `frame_rejections`, `look_rejections` | Counts by the guard's fixed reason enum, at frame receipt and final look acceptance respectively. The same source failure can appear at both stages; do not add these populations together. |
| `age_bin_exclusive_upper_us`, `working_crop_source_age_us`, `full_crop_source_age_us` | Fixed histogram edges and counts, split by existing crop and driver timestamp point. Only fresh, available geometry enters them. |
| `saturated` | A counter reached its integer limit. Such a window is not a complete count. |

The table above describes the original schema/1 observation prerequisite
(#8865). Historical schema/1 records do not contain facing classifications
and must not be backfilled as successful or failed sensor 4 observations.

The processing-look outcomes stay separate: `available`, `no_face`,
`multiple_faces`, `filtered_face`, `detection_skipped`, `decode_failed`,
`detector_failed`, `too_small`, `outside_frame`, `clipped_face`,
`mesh_not_configured`, `mesh_deferred`, `mesh_unavailable`,
`invalid_geometry`, `rejected` and `cancelled`. Multiple detections are
ambiguous even when the existing still-face filter later removes all but
one. Missing or deferred meshes do not fall back to five marks. A failed
final freshness/permission check makes the look `rejected`, with its fixed
reason, rather than accepting its geometry or no-face outcome.

Guard reasons distinguish missing/changed permission, absent or mismatched
capture/source, unknown or invalid clock/timestamp/domain/point, timestamps
before open or in the future, regressed time/sequence, duplicate sequence,
stale or superseded observations, invalid age configuration and exhausted
generation. See `Reason` for the exact closed vocabulary. No driver message,
identity, face box, image, landmarks, angle, source timestamp or per-look
identifier is included in this JSON. The temporary angles are discarded.
Zero looks mean no processing opportunities were observed in that window;
they do not demonstrate successful sensing.

### Sensor 4 facing counts (schema/2 draft, 2026-11-27)

The extension retains the explicit `--head-direction-observer on|off`
switch and the same bounded report interval. The planned schema is
`lamp-head-direction-observer/2`; its method identifier remains
`mesh_head_3a9ae0692_checked`. It adds a `facing` object with three separate
methods:

| Method | Fixed rule on an eligible, fresh look |
| --- | --- |
| `legacy_yaw` | Existing YuNet yaw and existing edge allowance. No legacy pitch or gaze is enabled. |
| `mesh_yaw` | Mesh yaw with exactly the same existing edge allowance; this isolates the geometry change. |
| `mesh_yaw_pitch` | The same mesh yaw rule and pitch inclusively between -20 and +25 degrees. |

The yaw allowance is inherited, not fitted: 60 degrees at image centre,
multiplied by `1 + 0.8 * from_middle` and capped at 90 degrees.
`from_middle` is the selected face centre's absolute horizontal offset,
normalised to 0 at the image centre and 1 at an edge. These are fixed
development rules, not a calibrated lamp/screen/phone classifier. The
classification changes no public face field, voice verdict or actuator.
In the simple-call base the voice's own call/window rule continues to
decide admission; observation does not add a camera condition to it.

Each method has numeric `facing`, `not_facing` and an `unknown` reason map.
Unknowns use the existing closed processing-outcome vocabulary plus
`invalid_face`; the fixed `unknown.available` key is always zero. For each
method, the two known counts plus all unknown
reason counts equal `looks` in an unsaturated completed report. A final
freshness or permission rejection makes every method unknown with reason
`rejected`; the detailed guard reason remains in `look_rejections`.
Cancellation also remains unknown. Do not add the three methods' counts
together: they are comparisons of the same processing looks.

The methods retain independent eligibility. Legacy yaw may remain known
when the mesh is absent, deferred or invalid, or when only the candidate's
stricter box-in-frame gate rejects a look. A clipped detector box does not
change the historical legacy result if its required marks are measurable
and inside the frame. Therefore legacy known counts can exceed
`outcomes.available`; no mesh failure is a fallback to legacy evidence for
the candidate.

The separate `facing.shoulders_unknown_no_association` count records one
unknown shoulder result for each finished or cancelled look. The NPU's
existing MoveNet shoulders are 2D COCO positions and confidence scores;
their response supplies no source capture timestamp, frame sequence or
verified association with the eyes' selected person. Shoulder width,
left/right order and image bearing do not establish torso orientation.
There is no new pose inference, shoulder fallback or claim of measured
head-and-shoulder agreement. This is a head-only proxy and cannot qualify
the v2 joint job alone. In particular, a measured head facing the lamp
with unknown shoulders cannot be promoted to joint `facing`; independent
gold labels do not create a missing shoulder observation. Missing, stale,
ambiguous or unframed heads also remain unknown, never correctly away.

Existing `outcomes`, rejection counters, source-age histograms and
`saturated` remain separate; schema/2 adds the fixed `invalid_face` outcome
for malformed dimensions, boxes or detector scores. The age histograms
still describe fresh, available candidate geometry only; they are not per-method recall or
classification-cost histograms. The record exports fixed enums and counts,
not angles, face identities, landmarks, images or per-look timestamps.

For a labelled answerable episode, an unknown is an unsuccessful sensing
opportunity and remains in the recall denominator. Expected no-face cases
test safe abstention separately. Runtime `looks` have no independent truth
labels: a `facing` count is a prediction, not a correct answer, and multiple
frames of one episode must not inflate the episode denominator.

### Source-age evidence is not latency qualification

The observer reuses the camera's exclusive source-age ceiling: **less than
250,000 microseconds** at acceptance. This is an admissibility ceiling,
not the approved p95 **100 ms or less** source-to-usable-output budget.
Each histogram has exclusive upper edges of 25,000, 50,000, 100,000,
150,000, 200,000 and 250,000 microseconds. Its `start_of_exposure` and
`end_of_frame` arrays stay separate: an end-of-frame timestamp is not an
exposure start. Working-picture and full-frame crop arrays also stay
separate; the observer never enables the full crop itself.

Available looks older than the 100 ms budget remain in the upper bins;
looks at or beyond the 250 ms ceiling remain as `stale` rejections, without
a histogram entry. Unknown timestamps and other rejected outcomes also
remain counted. A percentile computed only from accepted histogram samples
would omit these tails and failures; it cannot establish whole-path p95,
recall or a qualified sensor. Existing processing looks are not independent
labelled head-turn, utterance or lamp/screen/phone opportunities.

The added observer overhead target is **p95 below 1 ms**, including checks
and geometry on the existing path. On-lamp overhead, periodic journal cost,
voice interference, thermals and the overall source-to-usable-output budget
have not yet been measured. No timing target is marked passed by this step.
The fleet recorder/upload, a live per-case adapter to the reviewed scorer,
a reviewed raw-pose export fence and sensor qualification are not implemented.

## Run without a lamp

Use the exact detector and mesh files pinned by
`deploy/product/models.json`. The existing loaders check their byte counts
and SHA-256 before parsing. Do not substitute the MediaPipe task bundle for
the product's ONNX model.

Keep a JSON array of relative JPEG paths outside Git, for example:

```json
["frames/001.jpg", "frames/002.jpg"]
```

From `lampOS/`:

```sh
cargo run --offline --locked --release -p lamp-eyes \
  --example head_direction_compare -- \
  DETECTOR.onnx MESH.onnx /path/to/approved/images /path/to/image-list.json \
  > /path/to/private/comparison.jsonl
```

For the sensor 4 comparison extension, an optional final suffix selects
the crop: `--crop working` or `--crop full`. Omitting it means `working`.
This option belongs to the offline example, not `lamp-eyes`; it does not
change a product unit or runtime mouth configuration.

The list is limited to 1 MiB and 10,000 distinct relative paths. Absolute
paths, parent traversal, repeated paths and non-JPEG extensions are refused.
Resolved images must stay beneath the supplied root. Each image is limited
to 8 MiB and the existing decoder's 4,096-pixel side bound. Use immutable
input files during a run. No image, crop, landmark array or input path is
written to the output; these files and their numeric per-image observations
remain private development artifacts, not fleet health reports.

## What is compared

Both methods use the same decoded `Picture`, pinned YuNet detector and
selected face. Selection is the product's largest-height mesh choice, not a
claim that this is the speaking person. The output includes face count and
the selected box so the canonical scorer can identify a wrong-subject case.

The original comparison uses the existing rotated 1.5-times face crop from
the working picture and the existing tract `Mesh` implementation. The
sensor 4 extension adds optional `--crop working|full`, default `working`:
`working` keeps that path; `full` uses the existing `crop_full` path on the
same decoded source frame, not a second camera or a second exposure. Its
`crop` metadata is `product_picture_rotated_1_5` for `working` and
`product_full_frame_rotated_1_5` for `full`; the comparison schema remains
`lamp-head-direction-comparison/1`. The runtime can likewise use the
same-frame full crop when its existing `full_mouth` configuration requests
it; the observer does not turn that configuration on. Match crop, decoding
and model settings before comparing offline and runtime results. Neither
mode introduces a new model or dependency. The candidate retains #8782's
canonical points and geometry, with additional
finite, overflow, length and degeneracy checks. Its coordinate signs and
depth assumptions still require validation on real images and on the body.

The legacy angles come from `facing::yaw_deg` and `facing::pitch_deg`, with
the same face-width and visible-mark gates as the existing publisher. Yaw
is signed toward picture-right using the publisher's sign rule. Pitch is
positive down. Legacy pitch is explicitly **counterfactual**: the product's
pitch/gaze switch remains off. Its unread `+90` sentinel becomes an explicit
`legacy_sentinel` with a null pitch, not an observed phone-down angle.

The candidate additionally abstains when the detected face box crosses a
frame edge, even if its eyes and nose are inside. This conservative
eligibility check is not proof that the whole head is visible or unoccluded;
the reference labels must still catch those misses. Legacy angles retain
their original visibility rule for a faithful comparison.

No candidate measurement falls back to the marks when its mesh is unread.
`no_face`, `too_small`, `outside_frame`, `clipped_face`, `invalid_geometry` and processing
failures remain distinct. Candidate and baseline angles keep separate
states. A processed image is not necessarily a measured pose or a correct
answer. The math fixtures establish coordinate and invalid-input behavior,
not sensor accuracy.

## Read the output

The JSONL stream begins with `lamp-head-direction-comparison/1` metadata:
model hashes, image-list hash, algorithm identifier, selection/crop rules,
runtime and model-load time. Each input ordinal then has one `sample` row,
including failed reads or decoding. Rows contain only declared metadata,
numeric observations and fixed states; the private list maps ordinals back
to input files. The stream ends with `complete`, the expected sample count
and a processing-error count. Processing errors make the command exit
unsuccessfully after preserving those rows. Missing detections are still
observations; their evaluation verdict belongs to the reference scorer.
Startup and output failures print fixed error codes, never supplied paths,
model-loader messages or malformed input values.

Record the code revision, executable hash, input manifest and exit status
beside an experiment. A truncated stream or absent completion record is
incomplete. Image rows are not independent interaction opportunities:
repeated frames from one labelled episode must not increase its denominator.

Timing is **file-read start to comparison result**, with decode, detector,
crop/mesh and geometry stages separately measured. Model loading is separate.
An attempted stage retains its duration even when it fails; unattempted
stages are null, not zero-latency measurements.
`capture_age_ms` is null because files provide no verified live capture
clock. These host times cannot establish the approved on-body p95 ≤100 ms
capture-to-usable-output budget, sampling delay, source freshness, thermal
behavior, camera shutdown or end-to-end voice latency.

## First product-pipeline comparison (2026-11-27)

This historical run processed all 2,790 public Pointing'04 images
on a macOS x86-64 host, with the product's 320-by-256 working-picture
decoder, pinned models and rotated **working-picture** mesh crop. It did
not exercise the runtime's optional full-frame mouth crop and does not
establish parity with every product configuration. No angle fit, target classifier or
threshold search was applied. Input hashes were unchanged after the run;
the stream had every ordinal, a completion record, no processing errors and
no standard-error output. Dataset images and per-image observations remain
outside Git.

These are **mean absolute angle errors on common measured inputs**, not
classification accuracy. The two methods have different abstention rules,
so each axis uses only the inputs measured by both methods for this paired
comparison:

| Axis | Common measured inputs | Five marks | Mesh candidate |
| --- | ---: | ---: | ---: |
| Yaw | 2,719 | 14.87 degrees | 11.69 degrees |
| Pitch | 1,320 | 29.02 degrees | 18.20 degrees |

Across all 2,790 inputs, the marks measured 2,776 yaw values and 1,349
pitch values. The candidate measured both axes on 2,719 inputs (97.46%
measurement coverage): 48 clipped faces, 13 no-face detections, 9 invalid
geometries and 1 too-small face remained unknown. Neither unknowns nor
unmeasured axes are correct outcomes. Three images produced two detections;
largest-height selection was retained and no observation was dropped on
that basis. Correct-subject qualification still requires the shared labels.
Legacy pitch remains a counterfactual because its product switch is off.

Host file-read-to-result timing was 56.05 ms median and 64.31 ms p95
(nearest rank, 2,790 inputs). This includes the shared detector and mesh,
excludes model loading, and is not source-capture-to-output latency on a
lamp. The experiment executable SHA-256 was
`7525617203652c76bfe561370434b89ac8c9f57e729d9c3ad3edcad7a08f83ca`.

The [dataset description](https://crowley-coutaz.fr/Pointing04/data-face.html)
defines the filename's vertical and horizontal pose labels. This
development analysis uses `yaw = -pan`, `pitch = -tilt` for the candidate's
camera-right/down convention. All 15 participants and both series were
used as development evidence; none is represented as an untouched holdout.
Pointing'04's marked head directions are not desk-specific lamp, screen or
phone labels. The source reference is *Estimating Face Orientation from
Robust Detection of Salient Facial Features*, Pointing 2004, in the
[workshop proceedings](https://crowley-coutaz.fr/FGnet/reports/Pointing04-Proceedings.pdf).
The mesh improves this historical development angle comparison but did
not qualify the then-current three-target recall or on-body latency gates.
It also supplies no joint head/shoulder truth for the current v2 job.

## Sensor 4 fixed-rule development comparison (2026-11-27)

The rules were frozen before binary analysis. The public-label proxy is
the optical-axis cone `abs(yaw) <= 60`, `-20 <= pitch <= 25`, using
`yaw = -pan` and `pitch = -tilt` from Pointing'04. Separately compare
legacy yaw and mesh yaw with the same inherited image-edge allowance,
then mesh yaw plus pitch, so the geometry and rule changes remain visible.
This first fixed-rule comparison selected no thresholds, and its cone is
not a labelled desk-interaction target. The separate bounded development
calibration below preserves this comparison unchanged. In particular, an
away head is not automatically a screen or phone label.

Pointing'04 supplies no real speaking, desk-distance or shoulder-orientation
labels for this task. All participants and both series already form
development evidence; this analysis creates no untouched holdout. Face-size
slices describe pixels, not metres. The lead confirms no labelled real
episodes are currently available. The Reviewer's frozen v2 scorer now
supplies the joint head/shoulder contract; the missing evidence is the
independently labelled real collection, a shoulder-capable candidate and
live timing, not an unpublished schema. The fixed-cone public analysis
remains a head-only development comparison.

The retained 2,790-image working-crop run gives the following fixed-rule
results. There are 810 inside-reference and 1,980 outside-reference images;
every missing or unreadable result remains in its true class denominator.
These are optical-axis proxy recalls, not the joint v2 sensor score.

| Fixed prediction rule | Inside-reference correct | Outside-reference correct | Unknown |
| --- | --- | --- | --- |
| Stock yaw with its edge allowance | 558/810 (68.89%) | 1,049/1,980 (52.98%) | 14 |
| Mesh yaw with the same allowance | 809/810 (99.88%) | 53/1,980 (2.68%) | 71 |
| Mesh yaw with that allowance and fixed pitch | 796/810 (98.27%) | 674/1,980 (34.04%) | 71 |

These rows preserve the selected-largest-face replay. A second conservative
view marks the three multiple-detection images unknown: correct counts are
unchanged, while unknowns become 17/74/74. This still does not prove that
the labelled person's face was selected. The pitch candidate's 95%
participant-cluster bootstrap intervals are 95.93–99.88% inside and
30.10–38.28% outside (15 participants, 5,000 resamples, both series kept).
Development reuse and correlated pictures prevent a qualification claim.

The lower angle error does not establish the required two-class behavior.
The inherited allowance and model geometry admit many outside-reference
poses. No threshold was fitted to these results. The candidate is not
qualified for addition to the interaction judge.

Small-face coverage is insufficient: one detected box is under 24 working
pixels wide, none are 24–48, and 96 are 48–80 with only two inside-reference
examples. All original images are 384×288 and working pictures 320×240.
Detected box width is neither ground-truth face extent nor physical distance.
Missing detections stay in the overall denominator, outside these box slices.

All 2,790 original image hashes and retained producer inputs were verified.
Eight synthetic denominator/boundary checks passed. The actual Rust f32
classifiers reproduced all 16,740 method/view decisions exactly, including
unknowns; no f32 boundary discrepancy was found. This is code-to-analysis
agreement, not independent accuracy evidence.
The numeric analysis result SHA-256 is
`ef44edab2dc7fe73ca537d5dd78b078947d5cda058fe4bfa651f0277bb5bd118`;
the original measurement JSONL SHA-256 is
`1b0742e40cd4ac23a73bed615524a13d5fc15682f4de67312516a8988e971518`.
The complete script, fixed protocol, per-input outcomes, slices, source
hashes and README remain retained locally with the public raw dataset.
No images, participant rows or private captures are added to this repository.

### Explicit full-frame crop rerun

The new host producer reran all 2,790 original images with both explicit
crop modes, with zero processing errors in each. Working-mode non-timing
sample fields exactly reproduce the retained run. Each replay also passes
16,740 actual Rust-versus-analysis decision comparisons with zero differences.
Full mode changes only
mesh angle/state values; detection boxes, face counts and legacy marks are
identical. The source image is 384×288 here, so this is not a simulation of
normal desk distances or the lamp's full-resolution capture.

| Full-frame crop prediction | Inside-reference correct | Outside-reference correct | Unknown |
| --- | --- | --- | --- |
| Stock yaw with its edge allowance | 558/810 (68.89%) | 1,049/1,980 (52.98%) | 14 |
| Mesh yaw with the same allowance | 810/810 (100.00%) | 60/1,980 (3.03%) | 71 |
| Mesh yaw with that allowance and fixed pitch | 798/810 (98.52%) | 684/1,980 (34.55%) | 71 |

The conservative multiple-detection view leaves these correct counts
unchanged and has 17/74/74 unknowns. The mesh's 71 raw unknowns are all in
the outside-reference class. No new independent cases or holdout were
created, and these figures do not qualify the joint sensor.

Both runs used the retained host executable SHA-256
`00bb1ab935ae94820549bd6c6a191a3d7ba4943475f4572e2ec7927c7b233e9d`.
Working JSONL SHA-256:
`4cfb1ccb9b6f73e9bd31c2a979129be8de8e7222e81a1aaf7266656f21e99c7f`.
Full JSONL SHA-256:
`285a0230495ce14da751e7d173b414a38fa487ea229419d3d17d2b408a548d39`.

Observed host file-read-to-result times are retained separately:

| Crop | Model load | First sample total | Subsequent total p50 / p95 | Subsequent samples |
| --- | --- | --- | --- | --- |
| Working | 638.328 ms | 245.964 ms | 207.348 / 282.283 ms | 2,789 |
| Full | 552.979 ms | 172.305 ms | 109.648 / 233.520 ms | 2,789 |

This shared host was also running build checks, with more overlap during
the working run. These durations do not establish a crop speedup or a
before/after latency improvement. Both readiness and source capture age
remain unmeasured; file timing must not be added to an invented capture clock.

| Evidence | Status and required boundary |
| --- | --- |
| Fixed-proxy class recall and unknown coverage | Measured above on the retained public development run; this is not the v2 joint score or a held-out result. |
| Working/full crop comparison and small-face stress | Both crop modes completed above on the same public inputs. Native small-face two-class support remains insufficient; no physical-distance or synthetic-small-face claim. |
| First and repeated host classification cost | Standalone optimized host measurement: first 1.045 µs, repeated median 0.087 µs, p95 0.125 µs over 1,000 samples, below the 1 ms added-classification target. Boundary is legacy yaw plus three classifiers; excludes model/geometry/guard/report work and is elapsed duration, not measured CPU time or board capture latency. |
| Source freshness and permission regressions | Focused 48 passed / 1 ignored; full Mac 567 passed / 6 ignored and Linux 569 passed / 6 ignored. Strict all-target Clippy and formatting passed on both; the capture-generation, sequence, clock, age and revocation tests remain. The new ignored classifier measurement was run explicitly in the separate optimized host harness. |
| Board offline processing replay | Requested from the lead for after 22:55 local, initially the historical `cee2e324` example in working mode; no result recorded. It cannot establish new classification behavior, capture age or real-person recall. |
| Real desk distance, speaking and shoulder cases | Unmeasured; no substitute labels or inferred physical distance. |
| Live board capture-to-usable-result p95 <=100 ms | Unmeasured; require original capture clock/point, missed and stale outcomes, and final permission check. |

Pending and unmeasured entries are not passes. Keep raw inputs and derived
observations with their holder; only reviewed numeric aggregate results
belong in this document. No sensor joins the combined interaction judge
before independent qualification.

## One bounded development calibration round (2026-11-28)

Following the first comparison, a separate protocol was frozen before one
threshold search on the same retained public outputs. It contains 3,564
variants: working/full crop, original/no edge allowance, yaw bounds from
10 to 60 degrees in steps of 5, lower pitch bounds from -40 to 0 in steps
of 5, and upper pitch bounds from 5 to 45 in steps of 5. Reference labels,
geometry eligibility, size/clipping checks and unknown handling stayed fixed.
The protocol SHA-256 is
`2c08701642ded99ebb29f4d5a30f9e2934f38658697e6cfec2f7924b56dca05b`.
This is a new development comparison, not a change to the three runtime
observer rules or to their original measurements.

For each of 15 folds, both series from one participant were excluded from
parameter selection. The remaining 14 selected the rule maximizing outside
recall, subject to inside correct × 100 >= inside opportunities × 95 and a
frozen tie order. The primary procedure selected both crop and thresholds;
two secondary procedures fixed the crop first. Selection used only the
conservative single-detection view. Raw largest-face results are retained
as descriptive evidence and did not influence selection. All participants
had already been used for development, so this is development cross-validation,
not an untouched holdout or head-and-shoulder qualification.

| Participant-out selection procedure | Inside correct / wrong / unknown, of 810 | Inside recall | Outside correct / false-facing / unknown, of 1,980 | Outside recall |
| --- | --- | --- | --- | --- |
| Select crop and thresholds | 757 / 53 / 0 | 93.46% | 1,224 / 680 / 76 | 61.82% |
| Fix working crop, select thresholds | 768 / 41 / 1 | 94.81% | 1,237 / 670 / 73 | 62.47% |
| Fix full crop, select thresholds | 750 / 60 / 0 | 92.59% | 1,217 / 689 / 74 | 61.46% |

The primary procedure misses both 95% development targets. False-facing is
680/1,980 (34.34%); its 76 unknowns are separate failures, not false-facing
predictions. These pooled predictions describe the selection procedure:
each excluded participant can have a different configuration. Four distinct
configurations were selected, all without the edge allowance, with working
crop in eight folds and full crop in seven. Excluded-participant inside
recall ranges from 68.52% to 100%; outside recall from 46.97% to 78.79%.
This is not the score of one fixed, deployable configuration.

Against the fixed v1 working-crop rule under the same conservative view,
the primary procedure loses 39 formerly correct inside cases and recovers
none; outside it loses 59 and recovers 609. Outside correct rises from
674 to 1,224, while inside correct falls from 796 to 757. The fixed full-crop
v1 comparator is retained separately. Better outside rejection in this
bounded round does not satisfy the per-class targets or the joint task.

The one configuration selected on all 15 participants is full crop, no
edge allowance, yaw 60 degrees and pitch [-25,+15]. Its optimistic in-sample
figures are 772/810 (95.31%) inside and 1,230/1,980 (62.12%) outside. The same
participants selected those parameters; these figures cannot replace the
cross-validated procedure figures or qualify a sensor. The runtime observer
retains its original yaw/edge/pitch rules; no fitted threshold is enabled.

The fixed diagnostics also distinguish an angular error from a constant
bias. With full crop, inside-reference pitch has signed median +1.10 degrees
and absolute median/p90 7.38/15.08 degrees; pitch-only outside cases have
signed median -0.54 and absolute median/p90 23.14/40.36 degrees. Opposite
errors can cancel in a signed median. An aggregate pitch error is not a
measured constant offset to subtract, and these diagnostics did not trigger
a second search. Small-face and physical-distance support remain unchanged.

The single Rust f32 grid invocation evaluated 19,887,120 decisions across
both visibility views. All 33,480 actual-source baseline comparisons match
v1. The saved prediction matrix agrees with all 641,520 participant count
cells. Independent verification checked all 46 reported choices against the
frozen ranking, 22,320 selected predictions, 32 class-transition groups and
11,340 lost/recovered rows. Five standalone Rust boundary tests, five
synthetic retention tests and the selection checks pass. No second grid,
new inference, label change or model training was performed.

The initial invocation completed once before a source review identified
that the reusable wrapper could overwrite a failed run. The exact executed
wrapper and successful outputs are preserved. The reusable entry point now
uses an exclusive attempt guard; the existing attempt lock is explicitly
labelled post-execution. No pre-execution protection is retrospectively
claimed, and the measurements were not repeated to test that correction.

All original inputs, outputs and source evidence remain retained, alongside
this round's protocol, helpers, matrix, counts, selected predictions and
independent audit. The final 50-artifact manifest has SHA-256
`78178c69a0879575b94af4d42ab6639a9ac8e19f2f414229133fa923bb916a29`;
the numeric result JSON is
`a2ee767b2e6df1d97a6a7e60d51f0a137716269790627367225c6a806999a91d`.
The standalone Rust classification source is
`76c7c8abe19bb0a759dd9b8973a70476396f2c14087164f80e419ac059723daf`
and its executable is
`e42785d1f8b407ddb38d0ace0fb5447d31ab8decdd220e30c7e9f9d673aaf5ee`.
This is a local analysis executable, not a new runtime binary. This round measures no new latency: the earlier host
file-processing timings retain their original boundaries and limitations.
Live source-capture-to-result timing, associated shoulders, independent joint
labels, physical desk distance and small-face coverage remain unmeasured or
unsupported. No threshold from this round is adopted into voice admission.

## Qualification and fleet integration remain separate

Use the frozen v2 #8850/#8885 contract above, not a second qualification
scorer in this tool. Freeze model, crop, calibration, thresholds, labels
and eligibility before the real-person holdout. Count unknowns, timeouts,
missed visible faces and wrong subjects as failures on answerable trials;
expected unknown fault trials separately test safe abstention. Report both
joint classes, each head/shoulder disagreement direction, confusion counts,
coverage and participant/session/condition slices. Pointing'04's pose
labels are development references, not joint head/shoulder gold. The
head-only observer cannot pass this job by renaming `not_facing` to `away`
or substituting the head direction for the missing shoulder component.

The older local Pointing'04 benchmark used MediaPipe's transformation matrix,
an unrotated 1.6-times crop, and fitted each threshold on the observations it
reported. A missed away face counted as a correct rejection. Its reported
91–92% is neither a result for this implementation nor a held-out three-class
qualification score. Preserve it under that original method and denominator.

The runtime observer and proposed schema/2 counts supply default-off,
bounded availability, fixed-rule predictions and source-age evidence.
They do not supply labelled per-case output,
fleet recording/upload, or the reviewed raw-pose export boundary needed by
a future sensor. Existing `Faces::for_skills` removes only pitch and looking;
adding another head field without extending that boundary would expose it
to skills. The candidate must not overwrite legacy fields, turn absence
into away, hold a voice line, or publish gaze or identity. Only reviewed
aggregate outcome/timing counts and versioned coverage belong in the fleet
evaluator; the local observer journal is not that integration.

The team lead owns all lamp tests. Qualification needs the shared on-body
set, then the approved stable/candidate A/B/B/A comparison with the corrected
played kit and a real-person session. No voice-group score or latency
regression, both required readers, one sensor enabled per release, and
rollback on a drop. The framing prerequisite remains #8846; a pose model
cannot measure a head the camera does not see.
