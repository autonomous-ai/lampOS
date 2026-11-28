# Shared face quality, inactive (#8971)

`eyes::quality::measure(face, picture, native_frame)` measures an already-decoded
capture. `FaceQuality::fit_to_compare(&ComparisonLimits)` applies caller-supplied
limits. This piece has no runtime caller, setting, bus field, inference, stored
image, comparison or presence change. [Vietnamese](face-quality_vi.md).

The [5b evaluation](https://github.com/autonomous-ai/lampOS/pull/8980) found no
qualified static presence threshold. Fit for comparison is not evidence of
liveness, identity, consent or permission to save. Presence remains observational
under the [lead's decision](https://github.com/autonomous-ai/lampOS/issues/8971#issuecomment-7241776080).

## Measurements and unavailable evidence

Score and raw box retain working-image coordinates. Height and clipped-area
fraction use the **filled image**, not black padding. Working eye separation is
the detector's landmark distance; native eye separation maps it through the
actual decode scale. This is an estimate, not proof of resolved eye detail, and
enlarging a crop cannot manufacture native pixels.

Absolute yaw reuses `facing::yaw_deg`, the existing five-mark heuristic, not
qualified physical head-pose truth. Piece 5a uses 6DRepNet yaw strata: its numeric
cutoffs cannot transfer directly to this different estimator.

`NativeLumaLaplacianV1` is population variance of the cardinal one-native-pixel
Laplacian of integer luma, sampled inside the raw source crop. It needs no resize,
interpolation or crop allocation. The bounded grid can alias textures; the value
depends on content, JPEG and lighting. No operating cutoff is qualified yet.

Missing native pixels or an absent blur limit gives `unavailable`. Invalid
dimensions, scale, buffers, marks or numeric limits give `invalid`. Every mark
must be inside both raw box and filled image; coincident eyes cannot supply yaw.
Other reasons are `low_score`, `small`, `turned`, `blurred`, `clipped`. The seven
bits deduplicate reasons. Limits include equality; unknown/invalid never passes.

Native layout checks cover dimensions, byte count, exact decoder scale and
filled size. They **do not prove buffers share an exposure**. Future callers
must bind face/picture/native frame to one capture, check freshness and privacy/
camera epoch, then recheck authority at comparison, saving and publication. They
must cap face batches and cancel stale work. This module grants none of that
authority and introduces no worker or retained frame.

## Every number and operating limit

| Number or limit | Meaning and source |
|---|---|
| 320×256 canvas | Existing `picture::{WIDTH, HEIGHT}`; filled area may be smaller |
| 4,096 maximum native side | Existing decoder bound, reused |
| 128 samples per axis, at most 16,384 stencils | Authored work bound, not an accuracy threshold |
| At least 2 stencils | One value has no measured variation; smaller crop is unavailable |
| Luma `(77R + 150G + 29B) >> 8` | Versioned integer definition; coefficients sum to 256 |
| Four neighbors minus 4×center | Cardinal neighbors one native pixel away |
| 7 reason bits | Fixed enum; no free text, names or pixels |
| Minimum score in [0,1] | Caller-supplied, no product default |
| Minimum native eye pixels, finite and >0 | Caller-supplied, separately validated |
| Maximum yaw estimate in [0°,90°] | Caller needs evidence in this estimator's units |
| Maximum clipped fraction in [0,1] | Caller-supplied; an unclipped policy requires 0 |
| Blur metric and finite nonnegative variance limit | Matching metric and validated cutoff required; absence is unavailable |

There is no default `ComparisonLimits` or environment switch. Unit-fixture
thresholds are arithmetic controls, not proposed settings.

## Reuse

V1's `faceid/recognizer.py` checks height, raw-box truncation and Laplacian
variance on an aligned 112-pixel crop; `faceid/geometry.py` rejects marks outside
box/image. Keep those geometry protections, and reuse Rust's decoder scale and
yaw helper. Change V1's missing-aligned-crop fallback from infinite sharpness to
unavailable. The native sampled blur metric differs from the aligned-crop one;
V1's cutoff cannot be reused. No legacy runtime or new model dependency is added.

## Verification and latency

Generated tests cover native/working scale, padding/clipping, unknown calibration,
invalid buffers/limits/marks, reasons, exact boundaries, flat/checkerboard
Laplacian values and bounded work. They do not measure recognition or liveness.

`cargo run --release -p lamp-eyes --example quality_measure` measures borrowed
decoded frame + supplied face → measurements. It excludes construction, JPEG,
detection, scheduling and publication. It reports the first call for each of
320×240, 1280×960 and 4096×4096 generated frames, then 500 single-face and 500
eight-face batch repetitions, without dropping outliers. Eight is benchmark load,
not a product batch limit. The host target is p95 ≤1 ms/face and ≤8 ms/batch.
ARM64 and voice-under-load remain unmeasured. The benchmark uses no model,
camera, device or external data.

Measured on the shared Intel iMac20,1/i7-10700K, macOS x86_64, 28 November 2026:

| Native frame | First call, ms | One-face p50 / p95, ms | Eight-face p95 / max, ms |
|---|---:|---:|---:|
| 320×240 | 0.250 | 0.225 / 0.335 | 2.644 / 6.334 |
| 1280×960 | 0.844 | 0.224 / 0.257 | 2.140 / 4.672 |
| 4096×4096 | 0.691 | 0.244 / 0.962 | 7.533 / 14.652 |

The p95 host targets passed. Worst single-face time was 1.901 ms. The first
call is per size, not a fresh process for each size. All 3,000 repeated timings
and three first-call timings remain local; [aggregate evidence and source
hashes](verification/face-quality-20261128/numeric.json) are public. A separate
scalar check recomputed all percentiles and verified the source hashes (27
checks). These synthetic full-frame boxes exercise the maximum stencil count;
they are not detection/recognition accuracy cases or isolated-hardware timings.

Mac `cargo test --locked --offline --release -p lamp-eyes --all-targets` passed
522 tests, including the 12 new quality tests. Five existing ignores remain:
three model/runtime-dependent checks, a manual people-fixture writer and a
diagnostic-cost report. Strict all-target Clippy and workspace formatting pass.
