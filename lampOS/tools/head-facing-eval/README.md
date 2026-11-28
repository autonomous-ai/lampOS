# Sensor 4 offline board replay

This kit uses public Pointing’04 photos. It does not open a camera, access live face records, publish to the bus, change a unit, or install a service. Run only through the lamp holder in the agreed quiet slot. Retain every input and output. Do not remove or overwrite an earlier result folder.

## Frozen producer and inputs

The already reviewed producer is `lamp-eyes` example `head_direction_compare` at #8856, commit `cee2e324742b53c2e6b9e24c41d372ce5d2c864f`. From that revision's `lampOS/` workspace, build off the lamp with:

```sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36 -p lamp-eyes --example head_direction_compare
```

Record the full source revision and executable SHA-256. The wrapper independently hashes the executable and checks the exact product YuNet and Face Mesh V2 assets. `pointing04-smoke.json` is the preserved 18-image smoke list, SHA-256 `30432e0c03a4fffab38c9362813160d5ccf7d7b33cff902cbb1ba87ac7638143`. `pointing04-smoke-sha256.json` records every original image hash. Resolve paths under the retained public Pointing’04 dataset root. The full previous manifest has 2,790 rows; this smoke list is not an accuracy sample or a held-out qualification.

## Run

Pass explicit local paths, with a new output directory whose parent already exists:

```sh
bash run.sh BINARY DETECTOR_ONNX FACE_MESH_ONNX PUBLIC_IMAGE_ROOT pointing04-smoke.json NEW_OUTPUT_DIRECTORY
```

GNU `timeout`, Bash and a SHA-256 tool are required on the board. This runs the producer at nice 15, with a 900-second limit and 5-second forced-stop grace. A timeout or failure keeps partial results and its nonzero exit code; it is not a completed measurement. The board holder chooses the safe slot and records concurrent load/thermal context separately. This is no hard CPU or thermal isolation claim.

The newer, separately gated producer adds optional `--crop full`; the wrapper's final `full` argument selects it. The reviewed #8856 binary only supports the default working crop. Do not use the full option with that binary. Default crop metadata is `product_picture_rotated_1_5`; full is `product_full_frame_rotated_1_5`. Compare modes only on identical input hashes and record the producer hash for each.

## Evidence and boundaries

Keep `results.jsonl`, `stderr.txt` and `run.json`, and hash them. Validate the closing row, exit code zero and exactly 18 ordered sample rows before summarizing. Report model load separately, first sample separately, then subsequent-sample p50/p95 and counts for decode, detector, crop/mesh, geometry and total. Repeated inputs are timing repeats, never additional labelled opportunities. Report processing errors and missing geometry explicitly.

Timing starts at file reading and ends at the produced measurement. It includes decode and inference; it excludes real capture and camera cadence. Source capture age stays null. Running on the board provides board processing latency only. No labelled real desk-facing episodes exist yet, so 0.5–1.5 m recall, real small-face recall and camera-to-result latency remain unmeasured. Head geometry is not shoulder orientation, gaze or verified speaking intention.

Public source: https://crowley-coutaz.fr/Pointing04/data-face.html. No public images are copied into Git, and no real people's captures leave their holder.
