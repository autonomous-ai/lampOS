# Head direction: offline comparison

For [#8847](https://github.com/autonomous-ai/lampOS/issues/8847), the
`head_direction_compare` example compares the existing five-mark geometry
with an isolated copy of the closed [#8782](https://github.com/autonomous-ai/lampOS/pull/8782)
mesh geometry. It produces observations for development. It does not score
accuracy, decide that a person faces a screen or phone, or enable a sensor.
No product unit, bus field, voice policy or camera reader is changed.

The approved goal is at least 95% recall separately for lamp, screen and
phone head-facing targets on the Reviewer's shared, independently labelled
set: at least 80 held-out episodes per target and six speakers. Head
direction is distinct from eye gaze, speaker identity and intended
recipient. This offline tool alone cannot establish any of those goals.

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

Before runtime integration, the candidate needs its own default-off gate,
capture-time provenance, bounded work and cancellation, explicit unknown
handling and a reviewed export boundary. Existing `Faces::for_skills`
removes only pitch and looking; adding another head field without extending
that boundary would expose it to skills. The candidate must not overwrite
legacy fields, turn absence into away, hold a voice line, or publish gaze
or identity. Only aggregate outcome/timing counts and versioned coverage
belong in the fleet evaluator.

The team lead owns all lamp tests. Qualification needs the shared on-body
set, then the approved stable/candidate A/B/B/A comparison with the corrected
played kit and a real-person session. No voice-group score or latency
regression, both required readers, one sensor enabled per release, and
rollback on a drop. The framing prerequisite remains #8846; a pose model
cannot measure a head the camera does not see.
