# Offline head-pose measurements

`head_pose_compare` is an offline producer for a dedicated 6DRepNet model.
It reuses the product JPEG decoder, full RGB frame and pinned YuNet detector.
It does not open a camera, use the bus, change a switch, score a dataset or
enable a product feature. No image, filename, source path or subject identity
is written to its JSONL output. Private images, lists, weights and results stay
outside git. This tool supplies measurements; it does not qualify a model.

Build from `lampOS/` so the workspace camera UAPI configuration applies:

```sh
cargo build --release -p lamp-eyes --example head_pose_compare
target/release/examples/head_pose_compare \
  /private/models/face_detection_yunet_2023mar.onnx \
  /private/models/pose.onnx "$POSE_ONNX_SHA256" \
  /private/images /private/image-list.json > /private/measurements.jsonl
```

Supply the SHA-256 of the **exported ONNX file**, not its source checkpoint.
There is no default checkpoint or implicit download. The caller records model
provenance, the export environment, licence evidence and the chosen ONNX hash
with the evaluation protocol before replay. This producer accepts one f32
input shaped `[1,3,224,224]` and one f32 rotation matrix output shaped `[1,3,3]`.
It uses the existing `tract-onnx` 0.23.8 dependency. Both the bounded ONNX bytes
and the pinned product detector are verified before their ONNX parser runs.
Model loading failures stop before any image replay. A successful build alone
does not establish that a particular exported graph runs on this runtime.

The image list is a JSON array of unique relative `.jpg` or `.jpeg` paths,
preserving the caller's order. It is limited to 1 MiB and 10,000 rows; each
image is limited to 8 MiB and the product decoder limits either side to 4,096
pixels. The pose model is limited to 256 MiB. Files must be stable regular
files, and resolved image paths must remain inside the canonical image root.
On Unix, final file opens refuse symlinks and use nonblocking opens to avoid
waiting on named pipes. Internal image symlinks can resolve to an ordinary
file inside the root. This is an offline tool for a caller-controlled input
tree, not a security boundary against concurrent ancestor-directory swaps.

## Frozen preprocessing

The metadata's `preprocess` field identifies
`full_rgb_cross_pad_02_resize224_center_even_bilinear_v1`:

1. Decode the original JPEG using `Picture::from_jpeg_full`. Feed its usual
   BGR working planes to the product detector. Keep the original decoded RGB
   `Frame` for the pose crop. The detector's padded input is 320 × 256; its
   populated image preserves the original aspect ratio, so it is 320 × 240
   only for a 4:3 source. Never use padded rows as real image pixels.
2. Require exactly one detected face. Require a finite, positive box wholly
   inside the populated working image, with width at least 0.075 of that
   image's width. Zero faces, multiple faces, clipped boxes and small boxes
   remain unknown. Do not select a largest face or substitute a landmark
   estimate. Detection eligibility is separate from pose correctness.
3. Map the selected box through the product's exact `Frame.scale` to the
   full RGB image. Truncate left, top, right and bottom toward zero. Set
   horizontal padding to `floor(0.2 * box_height)` and vertical padding to
   `floor(0.2 * box_width)`, using the truncated dimensions; clip the padded
   box to the full image. The axes deliberately follow the upstream demo.
   There is no eye-line rotation or landmark alignment.
4. Resize the shorter crop edge to 224 and the longer edge to
   `floor(224 * long / short)`. Center-crop 224 × 224. Center offsets match
   torchvision's `round((size - 224) / 2)` with ties to even.
5. Sample RGB with a Rust bilinear pixel-center sampler and clamped crop
   edges. It directly samples the centered output, avoiding a large resized
   intermediate. It does **not** reproduce PIL's integer quantization or
   antialiasing, and is not claimed to be PIL-bitwise or prediction-equivalent.
   Normalize floats by `/255`, then means `[0.485, 0.456, 0.406]` and standard
   deviations `[0.229, 0.224, 0.225]`, in RGB NCHW order.

Reference formulas come from `demo.py`, `sixdrepnet/regressor.py` and
`sixdrepnet/utils.py` in upstream
[`thohemp/6DRepNet`, commit `464b2ba55c3d9b3d3b707c6271cf329a983ded20`](https://github.com/thohemp/6DRepNet/tree/464b2ba55c3d9b3d3b707c6271cf329a983ded20).
This records the chosen hybrid of demo crop geometry and RGB normalized
regressor input; it does not claim to duplicate every upstream demo step.

## Angles, unknowns and output

The producer applies upstream `compute_euler_angles_from_rotation_matrices`
to the row-major matrix: `sy = sqrt(R00² + R10²)`,
`pitch = atan2(R21, R22)`, `yaw = atan2(-R20, sy)`,
`roll = atan2(R10, R00)`. When `sy < 1e-6`, pitch becomes
`atan2(-R12, R11)` and roll becomes zero. Values are degrees; pitch and roll
can extend beyond ±90°. These are raw model axes, without camera-sign or
dataset-label calibration, a facing threshold, confidence or a success score.
A finite matrix must also have `RᵀR` within 0.01 of identity and determinant
within 0.01 of +1; otherwise the result is `invalid_rotation`, with no angles.
This tolerance is a numerical validity check, not a model confidence level.

The JSONL schema is `lamp-head-pose-comparison/1`. It contains:

- One `metadata` row with input count, list and model hashes, model byte sizes,
  runtime, preprocessing version, selection rule, tensor shapes, coordinate
  convention and separate detector/pose load times.
- One `sample` per input row, indexed from zero. It includes numeric box,
  image and crop geometry, `Frame.scale`, state, optional angles and timings.
  `face_count: null` means the detector was not run successfully; zero means
  it ran and found no face. A failed or ineligible row never disappears.
- A `complete` row with samples, attempted pose calls, unknown rows and
  processing errors. Unknown eligibility results are not processing errors.
  File, decode, detector, malformed-box, preprocessing, inference and invalid
  rotation failures are processing errors. All rows are still written before
  an error exit. Output failures or startup failures can prevent completion;
  reject a replay without its `complete` row.

Paths and arbitrary underlying error strings are not copied into JSON or the
producer's own stderr. Startup errors use fixed codes. The tool does not
configure or make guarantees about external tracing configured by its caller.

## Timing boundaries

There are no hidden warm-up images or inference calls. Model load includes
file reading, hash verification, graph parsing and runtime preparation, and
is reported separately. `index: 0` identifies the first input frame;
`pose_call: 0` identifies the first **attempted pose inference**, which can
occur later if earlier frames are unknown. Subsequent pose calls can be used
for a separately declared warm statistic; they do not by themselves prove
thermal steady state. Never pool first-call latency into an unnamed warm
number. Errors retain elapsed time for each attempted stage; unattempted
stages are null.

`decode_us` includes the original JPEG decode and creation of the product
working `Picture`. `detector_us` includes the product detector's tensor
packing, inference and box decoding. `preprocess_us` includes full-image crop
geometry, resampling and normalization. `inference_us` includes pose input
tensor packing, model execution and output extraction. `geometry_us` includes
rotation validation and Euler conversion. `total_us` runs from the start of
input reading, including path containment, through the final result, before
JSON serialization and output flushing. It also includes per-image file I/O
and selection overhead, so it need not equal the stage sum. It excludes
model load, list parsing and output. These are offline software timings,
not camera-to-result latency or an acoustic measurement.

The evaluation owner freezes split, sign convention and threshold using only
the allowed tuning set, retains unknowns in denominators, then evaluates the
held test set once. This producer has no access to labels and cannot make
that separation for the caller. Record the exact binary, source revision,
ONNX hash, runtime, hardware, thread configuration and preprocessing version
for every benchmark. Hardware latency remains unverified until the same
pipeline is timed on the authorized ARM64 board.

Focused synthetic unit checks (no model or dataset required):

```sh
cargo test -p lamp-eyes --example head_pose_compare
cargo clippy -p lamp-eyes --example head_pose_compare -- -D warnings
```

A separate model smoke mode needs the ONNX file but reads no image or list:

```sh
target/release/examples/head_pose_compare --synthetic-smoke \
  /private/models/pose.onnx "$POSE_ONNX_SHA256" > /private/synthetic.jsonl
```

It runs three fixed, already normalized tensors: zero, one and a gradient.
The gradient is `numpy.linspace(-2, 2, 224*224, dtype=float32)` laid out as
three NCHW planes `[grid, -grid, grid / 2]`. The separate
`lamp-head-pose-synthetic/1` schema reports each tensor's little-endian f32
SHA-256, raw rotation matrix, Euler angles and model-call time. It retains the
same finite/proper-rotation guard. Compare hashes before comparing numerical
outputs with a reference exporter. This mode checks runtime compatibility
and synthetic numerical parity; it does not test the detector, preprocessing
or prediction quality on photographs.
