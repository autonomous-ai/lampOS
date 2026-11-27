# Offline current mouth baseline

`mouth_baseline_bench` runs the existing eyes components on local JPEGs. It
does not open a camera, connect to the bus, download anything, read labels,
score accuracy, or publish messages. Its records are **component replay**,
not the #8854 live journal export. They contain neither operating-system boot
IDs nor claims about live capture age or readiness.

The runner uses production `faces::Detector`, `Picture::from_jpeg_full`,
`mesh::Mesh`, `mesh::meshed_order`, `mesh::crop` / `crop_full`, `facing::yaw_deg`,
`bus_out::edges` / `Ids`, and `speaking::Mouths`. Every input runs YuNet. Only
the tallest detection, using production ordering even on a height tie, is
meshed; every visible detection is judged. Tracking uses observed clipped
boxes and source time, never a label or a supplied face box. The small adapter
around `judge_with_diagnostics` follows `bus_out::message`'s size and visible
landmark gates, with a test comparing every face's result and passing ID.

## Run

From the lampOS workspace, with dependencies already present:

```sh
export CARGO_TARGET_DIR=/private/tmp/third-speaking-face-8848/lampOS/target
export CARGO_BUILD_JOBS=2
export LAMP_EYES_SPEAKING=on
export LAMP_EYES_SPEAKING_STEADY=on
export LAMP_EYES_SPEAKING_ALL_FACES=off
export LAMP_EYES_SPEAKING_DROP_OUTLIER=off
export LAMP_EYES_SPEAKING_SKIP_JUMP=off
export LAMP_EYES_MOUTH_FULL_LOOK=off
cargo run --offline -q -p lamp-eyes --example mouth_baseline_bench -- \
  --root /absolute/local/input-root \
  --input frames.jsonl \
  --yunet models/face_detection_yunet_2023mar.onnx \
  --mesh models/face_mesh_v2.onnx > run.jsonl 2> run.stderr.jsonl
```

Use a new output filename for every run, and keep both output streams even
when the command fails. The runner never deletes or rewrites an input. Keep
the original manifests, JPEGs, models, intermediate files and run logs locally
with SHA-256 inventories; do not commit or upload face images.

The optional `--ort-runtime models/libonnxruntime.1.30.0.dylib` (or the pinned
Linux ARM64 library) selects the existing production ONNX Runtime loader.
Without it, the mesh uses tract. YuNet always uses tract. There is no runtime
discovery, auto-download or fallback. Both model files and any native library
must match the production byte counts and SHA-256 pins before loading. The
normal production loader also checks its pins.

The root must be absolute. CLI asset paths may be relative to it or absolute
inside it. The input manifest, every JPEG, both models and an optional runtime
must canonicalize under that root. A symlink into the root is allowed; a
symlink escaping it is refused. Parent traversal, control characters and
non-UTF-8 paths are refused. Regular-file checks and bounded reads reject
directories, oversized inputs and special files. Keep the input tree immutable
while running: this is a local offline reader, not a sandbox against a process
concurrently replacing ancestor directories or the native library.

## Switches and crop resolution

The runner reads and logs all six switches explicitly. `SPEAKING` and `STEADY`
must be `on`. `ALL_FACES`, `DROP_OUTLIER`, and `SKIP_JUMP` must be absent or
`off`. Unexpected values are errors, not silent defaults. It constructs
`Mouths` using production `speaking::from_environment` on startup and reset.

The current switch census has `LAMP_EYES_MOUTH_FULL_LOOK` **off**. Absent or
`off` uses the detector-resolution crop and mesh scale 1, and the run header
says `baseline: "current_production"`. `on` explicitly selects `crop_full`
from the decoded source frame, sets mesh scale to `1 / frame.scale`, and
reports `baseline: "current_judge_full_frame_variant"`. Do not pool those two
conditions as one baseline. Merely decoding the full JPEG does not enable
full-resolution mouth inference.

## Input

One strict JSON object per line, with no blank lines or unknown fields:

```jsonl
{"id":1,"at_us":0,"jpeg":"frames/0001.jpg","reset":true,"head_moving":null}
{"id":2,"at_us":250000,"jpeg":"frames/0002.jpg","reset":false,"head_moving":null}
{"id":3,"at_us":0,"jpeg":"next/0001.jpg","reset":true,"head_moving":false}
```

| Field | Meaning |
| --- | --- |
| `id` | Unique unsigned 64-bit input ID, for an external join only. |
| `at_us` | Unsigned 64-bit source timestamp in microseconds. Used directly by the tracker and mouth window. |
| `jpeg` | Relative JPEG path under the root, not under the manifest's directory. Never emitted as text. |
| `reset` | Required boolean. The first row must be true. True clears tracks and mouth windows before this frame and begins a new numbered segment. |
| `head_moving` | Optional boolean or null. Omitted/null is unknown and passed as `None` to production, which excludes its mesh pairs as moving. True is moving. False is an explicit **oracle stationary-lamp condition**, marked on the row and counted in the header. |

Source time must strictly increase within a segment, but may restart with a
reset. IDs remain unique across resets. Long gaps are preserved, letting the
production tracker and windows expire naturally. A reset is a replay boundary,
not a simulated OS boot. Temporary face IDs restart after resets, so their
scope is `(segment, temporary_id)` within this run.

The runner does not sleep or resample. Supply the intended look cadence in
the manifest and retain the frame extraction method alongside it. Running
every frame of a high-frame-rate video changes a window's sample count from
the lamp's usual look cadence; it is not a claim of live scheduler equivalence.

Limits are 64 MiB per manifest, 100,000 rows, 8,192 bytes per row including its
newline, 4,096 UTF-8 bytes per path, and 16 MiB per JPEG. The production decoder
also bounds each image side to 4,096 pixels. Duplicate IDs, duplicate keys,
non-integer timestamps, missing required fields, labels, supplied boxes and
unannounced time reversal are errors.

## Output and denominators

Stdout is JSONL in this order:

1. `mouth_baseline_run`: schema 1, evidence/time bases, effective switches,
   baseline/crop condition, manifest hash, actual model/library hashes and
   sizes, hashes of the compiled runner and production source modules, host
   OS/architecture, selected engine, total inputs and head-condition counts.
2. One `mouth_baseline_frame` per input, in input order, with numeric row and
   input IDs, segment, source timestamp, reset and head condition, JPEG hash
   and byte count, decoded dimensions, detector fill, mesh scale, selected
   detector index, frame outcome, all detections, and actual host timing.
3. `mouth_baseline_complete`: input and emitted totals, no-detection and
   missing-mesh frame counts, segment count and total elapsed host time.

Each detection has its numeric ephemeral tracking ID, raw `(x, y, width,
height)` in detector pixels, normalized clipped `(xmin, ymin, xmax, ymax)`,
detector score, mesh status and the production numeric `MouthDiagnostic`.
The diagnostic includes speaking `state`, `score`, `from`, reason, mouth gap,
eye distance, window counts/span, yaw, jitter, window reset and head state.
Fixed state/reason strings accompany the numbers; there is no free-form input
text, path, person ID, landmark array, image or label in these records.

`mesh_status` is `read`, `not_selected`, `crop_unavailable` or
`mouth_unavailable`. A missing crop or valid mesh with no measurable mouth
passes `None` to the same current judge. A smaller face still receives the
production corner-based judgement. A mixed mesh/corner window follows the
production rules unchanged. An empty clipped box is retained with
`decision:null` and `skipped_reason:"empty_clipped_box"`, matching the bus's
decision skip while preserving the observed detection.

Frame `outcome` is `observed`, `no_detection` or `mesh_missing`.
**No-detection and missing-mesh inputs remain in the frame denominator.**
An empty face array is not an invented negative speaking decision. Warm-up,
size, yaw, jitter and steady-pair refusals retain production `unknown` and its
reason. No accuracy metrics, speaker assignment or ground-truth matching are
performed here; independent public labels are joined afterwards.

`host_started_us` and `host_completed_us` are actual `Instant` elapsed times
from the start of this invocation's run, including initial model loading in
their origin. `host_elapsed_us` is their difference. Stage times cover frame
read/hash, decode, detector, crop/mesh and tracking/judging. Frame timings
exclude JSON serialization and output flush; completion time includes earlier
output writes. They measure this host and engine, not device latency. Source
timestamps never serve as host latency. There is no live `frame_age_ms` field:
the diagnostic-only age setter receives zero internally, which is stripped
from output rather than misrepresented as a measured age.

A run is usable only with exit status zero, a completion record and equal
input/emitted counts. Corrupt JPEGs, model/runtime pin failures, model loading
or inference errors, invalid mesh tensor data and input/output errors fail
with a fixed JSON diagnostic on stderr. It contains a code and, where known,
a numeric row; underlying error strings and panic payloads are suppressed.
Prior output rows remain available for diagnosis but are an incomplete run,
not a reduced scoring denominator. The runner does not fabricate records for
frames it cannot decode.

## Checks

```sh
rustfmt --edition 2024 --check crates/eyes/examples/mouth_baseline_bench.rs
CARGO_TARGET_DIR=/private/tmp/third-speaking-face-8848/lampOS/target \
  CARGO_BUILD_JOBS=2 cargo check --offline -q -p lamp-eyes --example mouth_baseline_bench
CARGO_TARGET_DIR=/private/tmp/third-speaking-face-8848/lampOS/target \
  CARGO_BUILD_JOBS=2 cargo test --offline -q -p lamp-eyes --example mouth_baseline_bench
CARGO_TARGET_DIR=/private/tmp/third-speaking-face-8848/lampOS/target \
  CARGO_BUILD_JOBS=2 cargo clippy --offline -q -p lamp-eyes --example mouth_baseline_bench -- -D warnings
```

Tests cover strict parsing, switch rejection, input text suppression,
root/symlink confinement, regular files and byte bounds, time gaps/reset/ID
rules, production adapter parity for multiple faces and missing meshes,
geometry gates, unknown versus stationary head, and track/window resets.
Production-environment tests run in child **processes** with fixed switches;
they do not mutate process-global environment variables during parallel tests.
File fixtures and their hashes are retained under the host temporary directory
in `mouth-baseline-tests-*` directories, with no test cleanup.

This runner excludes live camera grants, joint freshness reconstruction,
voice-heard scheduling, steady-rate scheduling, still-face filtering, empty-room
change skipping, bus publication, identity recognition and end-to-end speaker
or turn attribution. Its component decisions and host costs do not validate
those paths or a lamp's behavior.

### Local verification, 27 November 2026

The eight focused tests, formatting, host compile and strict example clippy
passed on macOS x86_64 with the target directory and two-job limit above.
Two real-model smoke runs used the same retained 26-frame manifest under tract:
the production crop and the explicitly marked full-frame variant each emitted
26 frames, including two no-detection frames. The other 24 inputs repeated an
existing local test JPEG in three reset-separated eight-look segments. In the
production-crop run, unknown and moving head conditions remained unknown after
warm-up; explicit stationary head settled to `no`. This is a plumbing/gating
check on repeated stills, not public-footage accuracy or speaking recall.

Negative CLI checks covered an unwanted label, a corrupt model, an escaping
symlink, a missing initial reset, incompatible switches, and an unsupported
ORT target. A corrupt JPEG failed at its numeric row with no completion record
or input-text leakage. No production ORT library is pinned for macOS x86_64,
so native ORT inference was not exercised. No device or Linux execution was
performed. Raw inputs, output streams, check logs and SHA-256 inventories were
retained locally under `/private/tmp/mouth-baseline-8848-evidence-20261127`;
the runner itself never removes inputs or intermediates.
