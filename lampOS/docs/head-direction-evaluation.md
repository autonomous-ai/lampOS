# Head direction: comparison and runtime observation

For [#8847](https://github.com/autonomous-ai/lampOS/issues/8847), the
`head_direction_compare` example compares the existing five-mark geometry
with the checked geometry from the closed
[#8782](https://github.com/autonomous-ai/lampOS/pull/8782) draft. A separate,
default-off runtime observer counts its availability using meshes the eyes
already computed. Neither path scores accuracy, decides that a person faces
a screen or phone, or changes a product unit, bus field or voice policy.
The observer adds no camera reader, inference or worker thread.

The pure geometry moved from `examples/head_direction/geometry.rs` to
[`src/head_direction/geometry.rs`](../crates/eyes/src/head_direction/geometry.rs)
without changing its math. The
[`head_direction_compare` example](../crates/eyes/examples/head_direction_compare.rs)
now imports that shared implementation; this does not relabel the earlier
offline results or qualify runtime behavior.

The approved goal is at least 95% recall separately for lamp, screen and
phone head-facing targets on the Reviewer's shared, independently labelled
set: at least 80 held-out episodes per target and six speakers. Head
direction is distinct from eye gaze, speaker identity and intended
recipient. Neither the offline comparison nor runtime availability counts
establish any of those goals.

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

The mesh uses the existing rotated 1.5-times face crop from the working
picture and the existing tract `Mesh` implementation. It does not use the
full-resolution crop, a second camera, a new model or a new dependency. The
candidate retains #8782's canonical points and geometry, with additional
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

The frozen candidate above processed all 2,790 public Pointing'04 images
on a macOS x86-64 host, with the product's 320-by-256 working-picture
decoder, pinned models and rotated crop. No angle fit, target classifier or
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
The mesh improves this development angle comparison but remains unqualified
for the approved three-target recall and on-body latency gates.

## Qualification and fleet integration remain separate

Use the Reviewer's #8850 manifest and scorer, not a second scorer in this
tool. Freeze model, crop, calibration and thresholds before the held-out
set. Count unknowns, timeouts, missed visible faces and wrong subjects as
failures on answerable trials; expected no-face trials separately test safe
abstention. Report each target's recall, confusion counts, coverage and
participant/session/condition slices. The public Pointing'04 pose labels
are development references, not desk-specific screen/phone truth.

The older local Pointing'04 benchmark used MediaPipe's transformation matrix,
an unrotated 1.6-times crop, and fitted each threshold on the observations it
reported. A missed away face counted as a correct rejection. Its reported
91–92% is neither a result for this implementation nor a held-out three-class
qualification score. Preserve it under that original method and denominator.

The runtime observer above supplies a default-off, bounded availability and
source-age prerequisite. It does not supply labelled per-case output,
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
