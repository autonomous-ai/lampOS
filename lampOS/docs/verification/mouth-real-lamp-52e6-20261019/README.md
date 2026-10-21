# Real-face mouth check, lamp-52e6, 19 October 2026 (#720)

Result: **not accepted for desk-distance use**. No speech model or threshold
was changed during this trial. The person's name, pictures and voice were not
recorded. Distances below are the person's reported placement, not independently
measured camera-to-face distance. Only the largest detection was scored; no
image was retained to independently verify its identity or camera framing.

The trial ran 08:00:57–08:10:30 +07. Only lamp-eyes was restarted. Body, Voice
and Expression were unchanged. The original eyes `22aa4eaa25ccb03e6226f11772cab8f4892b714e1aeb7690829ae625a1d5484f`
was restored at 08:10:30, with no remaining drop-in and the service active.
The journal showed just the deliberate install/restore service starts, no
failure/restart between them; `NRestarts=0` after restoration.

## What ran

- Source: #1470 at `2f2cb60668efb91297acd84c4f50ae2a8ba9d8be`.
- Native ARM64 artifact: run `39123665425`, `lampos-arm64-2f2cb60668efb91297acd84c4f50ae2a8ba9d8be`.
- lamp-eyes SHA-256: `c844623e6c7ecdb29f7dd48b71181d24fa051eb1439fa24254865da953149430`, verified against the artifact's SHA256SUMS and again on the lamp.
- Model SHA-256: `09d24504cdc51240b6ccd8c457040c7cb61ab3485e0e08696dd44ce52ed8131a`, the canonical export in `models/mouth/SOURCE.md`.
- `--mouth-model`, `--mouth-runtime` using the already installed `libonnxruntime.so.1.30.0`, `LAMP_EYES_SPEAKING=on`.
- No `--mouth-on-heard`: silence must be measured too. No CPU affinity changed.

## Results

The first two seconds of each bus phase are excluded to let the rolling window
fill. The numbers below are sample counts, not independently labelled windows.

| Phase | Duration | Bus messages | After 2 s | Median eye separation | Mesh median / p90 |
| --- | --- | --- | --- | --- | --- |
| Reported 0.5 m, lips closed | 30 s | 119 | 111 unknown | 14 px | 38 / 39 ms |
| Reported 25–30 cm, lips closed | 10 s | 39 | 31 unknown | 17 px | 38 / 39 ms |
| Same requested close position, reading aloud | 30 s | 119 | 78 unknown, 33 yes | 23 px | 39 / 40 ms |

The score's current minimum average eye separation is 24 px
(`crates/eyes/src/speaking.rs::LEAST_EYES_PX`). The silent phases never reached
that pixel size. The reading phase sometimes did, and all 33 classified samples
were `yes` from mesh, but 78/111 remained unknown. This does not meet the sheet's
90% acceptance bar, and there is no valid silent `no` result from these phases.
Face size changed between phases (mean box height 0.169, 0.200, 0.264), so this
is not a controlled silent-versus-speaking accuracy comparison. It is also not
evidence to lower the 24 px gate without a new controlled check.

The 1 m, lips-apart and turned-face phases were not run after the nearer phases
failed eligibility. CPU, temperature, a baseline accuracy A/B and acoustic
latency were not measured. The mesh time includes crop/rotation/inference as
reported by lamp-eyes, not the entire look or speech-onset latency.

## Method and raw numeric files

A scratch Python collector on the Mac ran the same artifact's `lamp-bus tap
humans/faces/tracked --seconds N` through SSH. It decoded each header and JSON
payload, and retained only timestamps, numeric bounding boxes, confidence,
turned and `speaking.{state,score,from}`. IDs, names and other fields were
excluded before writing. `*.jsonl` is that numeric projection, not the full bus
payload. The person explicitly signalled ready before each timed phase.

`*-journal.txt` contains only mouth-open and look-timing lines in the bus
phase's wall-time interval, ending one second after the last message. Thus its
mesh-run count can exceed the bus message count. `*-summary.json` records the
bus counts, mean largest-face height, timings and parse/journal outcomes. The
collector stays outside this Rust repository, under `/tmp/fifth-720-trial/` on
the test Mac; no measuring Python is committed here.

Next: inspect framing and the pixel-size gate separately from lip motion,
then repeat matched silent/speaking phases at verified distances. No claim
about 1 m performance or physical safety follows from this check.
