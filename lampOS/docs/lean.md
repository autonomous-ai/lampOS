# Lean: static balance before a planned movement

`lamp-lean` computes the modelled centre of mass of five lamp joints and its
signed distance from the base's footprint after an inward margin. It is an
arithmetic library with no motor access, service or environment switch of its
own. Body uses its model for V1's anti-tip rule only
([`crates/body/src/cog.rs`](../crates/body/src/cog.rs), with the first
software's model `deploy/lamp-4ace/lean-first-software.json` and V1's own
limits; [body.md](body.md), "Not tipping over"). The footprint check below
(`check_pose`, `check_movement`, `check_clip`) has no caller: one must enable
it explicitly after measuring its unit and qualifying that integration.

The supplied geometry is a **wholly synthetic stand-in**, not lamp-4ace's
measurements or V1's estimated masses and limits. Passing with it proves only
arithmetic on that fixture. `measurement: "measured"` and the provenance label
are declarations by the data supplier, not independent verification.
`Model::has_measurements()` exposes that declaration; neither it nor a positive
clearance authorizes an actuator.

## What is taken from V1

This review inspected the task's source base `6a1d2745c` on 2026-10-14.
[V1 verdicts](v1-verdicts.md) records the lamp-0c89 fall on 2026-09-04: a
third-party recording kept its individual joints in range but tipped the unit.
That is the reason to check the whole body as well as each joint.

Keep V1's forward kinematics, mass weighting, preflight refusal and report of
the offending pose. Change these assumptions and failure behaviors:

| Inspected V1 source | What changes here |
|---|---|
| [Stability calculation, lines 141–157](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/recording_stability.py#L141) puts each link's mass at its joint origin. | Each moving part and the fixed base have their own COM vector. Head roll can affect balance when the head's COM is away from its axis. |
| [URDF, lines 10–16](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/robots/lamp/urdf/lamp.urdf#L10) describes estimated masses and zero inertial origins. Its degree assumption conflicts with the default normalized [HAL positions](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/follower/hal_follower.py#L53). | Use explicit metres, kilograms and encoder-angle radians with measured zeros and axes. The old 22, 31.6 and 17.7 figures are not accepted as calibrated millimetres. |
| [Recording timing, lines 55–80](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/recording_timing.py#L55) checks authored frames, then interpolates joint angles. | Safe endpoints do not establish a safe nonlinear COM path. Continuous checks need an interval bound. |
| [Recording loading, lines 681–695](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/animation_service.py#L681) bypasses that check for a short clip or missing timestamps. [The gate, lines 186–208](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/recording_stability.py#L186) passes unavailable geometry or missing first-frame joints. | Invalid geometry and incomplete input cannot produce a safe result. A caller must cover every path into its motor writer. |

The [V1 tests](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/test/test_recording_stability.py)
offer useful extension and refusal-report cases. Their legacy thresholds and
pass-through expectations are not physical oracles. Yaw preserves radial COM
distance in the old check but can change clearance in an asymmetric footprint.

## Units, frames and geometry

Positions and lengths are **metres**, masses **kilograms**, rotations
**radians**. The fixed base frame lies on the support plane, `z = 0` at the
desk contacts, with **x forward, y left, z up**. It is right-handed; gravity is
assumed down. These match [ROS REP-103](https://raw.githubusercontent.com/ros-infrastructure/rep/master/rep-0103.rst).

The [schema](../crates/lean/src/data.rs) puts these facts in one JSON file:
`schema_version`, `measurement`, a provenance `label`, counterclockwise
`footprint_m`, inward `margin_m`, fixed `base`, and five `links` in **physical
serial order**. The base has `mass_kg` and `com_m`. Each link names its joint:

| Field | Meaning |
|---|---|
| `origin_m` | This joint's origin in the preceding link's frame; the first uses the fixed support-plane frame. The vectors encode lengths and lateral offsets. |
| `rpy_rad` | Fixed rotation `Rz(yaw) * Ry(pitch) * Rx(roll)`, stored as `[roll, pitch, yaw]`. |
| `axis` | Unit vector in that rotated joint frame, signed so increasing counts turn by the right-hand rule. |
| `angle_zero_rad` | Encoder-angle value at the physical reference pose used to measure this joint frame. |
| `mass_kg` | Mass of this rigid moving assembly alone. |
| `com_m` | Its COM in its own frame after this joint rotates; not the next joint's position or necessarily half a link length. |

The [model](../crates/lean/src/model.rs) translates to each joint, applies its
fixed rotation, then rotates about `axis` by
`encoder_angle_rad - angle_zero_rad`. It transforms the local COM and weights
all parts, including the base, by mass. Every mass must appear exactly once.
Static COM needs no inertia tensor.

Public arrays use increasing servo-ID order, independently of physical order:

| Index | Servo ID | Lean / message joint | Body calibration name |
|---:|---:|---|---|
| 0 | 1 | `base_yaw` | `base_yaw` |
| 1 | 2 | `shoulder` | `base_pitch` |
| 2 | 3 | `elbow` | `elbow_pitch` |
| 3 | 4 | `head_roll` | `wrist_roll` |
| 4 | 5 | `head_pitch` | `wrist_pitch` |

V1's topology puts head pitch before head roll, so that chain uses indices
**`[0, 1, 2, 4, 3]`**. IDs do not establish topology, axes or physical zeros.

`Angles` matches [Body's published convention](../crates/body/src/bus_out.rs):
`r = (count - 2048) * TAU / 4096`. A turn is 4096 counts; 4095 is the largest
single-turn count. `Present_Position` already includes the servo's homing
adjustment: do not subtract it again. Encoder radians are not automatically a
link's angle from vertical or CAD zero; the measured geometry supplies that
relationship. Count **offsets** convert by `TAU / 4096`, without subtracting 2048.

Legacy `NormalizedPosition` in `[-100,100]` is dimensionless. Its caller must
use Body's calibrated denormalization and preserve count quantization. Do not
apply its software `drive_mode` again to a published encoder angle.

## The footprint and margin

The support polygon is the convex hull of actual desk contact patches, not the
decorative outline or an assumed radius about the yaw axis. The inward margin
erodes that polygon; an invalid or collapsed inset is unusable. The coordinate
origin and yaw axis can be away from its centre and need the same measured frame.

`clearance_m` is **signed Euclidean distance to the inset polygon**: positive
inside, zero on its boundary, negative outside. It includes the physical margin.
Minimum normalized edge slack has the same sign, but differs from Euclidean
distance outside a corner. The small numerical reserve for a positive decision
does not cover measurement error or motion.

If a desk edge clips a foot, only its supported contact patch counts. Establish
supported patches before taking their hull and applying the margin. A margin
cannot make an assumed contact real. Repositioning the lamp, changing pads or
using a soft surface invalidates the corresponding support assumptions.

## Measurements the first builder needs

Record unit ID, hardware revision, date, frame photographs, instruments and
reading spread with the provenance. Weighing the assembled lamp alone cannot
identify its individual moving masses and COM vectors.

1. **Inventory rigid assemblies.** Unpowered and supported, identify the fixed
   base and five moving assemblies. Allocate each screw, motor case, driven
   attachment, electronics, camera and carried cable portion exactly once.
   A motor case and its driven attachment can belong to different links.
   Flexible cable tension is an external force that adding cable mass does
   not model.
2. **Weigh each assembly.** Tare the kitchen scale with its fixture. Put the
   whole assembly on it without another support carrying weight; repeat and
   convert grams to kilograms. Compare the sum with a separate whole-lamp
   weighing and reconcile missing or duplicated hardware. Retain raw readings,
   tare and scale resolution. Replacement parts or ballast require remeasurement.
3. **Locate each local COM.** Place the unpowered piece on two narrow, level
   supports distance `L` apart, one on the tared scale and the other at the
   same height. If the support at `L` reads equivalent mass `m_B` and the
   piece's mass is `m`, its COM coordinate from the other support is
   `L * m_B / m`. Nothing else, including cables, may carry weight. Reorient
   the separately supported piece safely for the other axes and express all
   three coordinates in its marked frame. A supported balance-edge check can
   corroborate them. Do not balance a powered lamp near tipping.
4. **If disassembly is unavailable, identify the limits of reaction measurements.**
   A rigid level platform with known support locations, one on the scale,
   permits whole-lamp reaction measurements in a fixed, securely supported pose.
   Subtract the empty platform's reaction, including tare uncertainty. Repeat
   orthogonally to estimate horizontal COM. This checks the combined model at
   that pose; it does not identify every link mass or 3D COM. Separately weighed
   subassemblies or verified design data are still required. Unidentified
   values stay stand-ins.
5. **Measure joint frames.** Mark axis centres and a reproducible reference
   pose. With a ruler, measure joint-to-joint vectors, heights from the support
   plane and lateral offsets. Use measured direction vectors or a rigid
   labelled fixture to establish frames: two separated axis points give its
   direction, and two nonparallel frame directions establish orientation/RPY.
   Record the right-hand axis sign against increasing counts and the count
   at the reference pose; convert it to `angle_zero_rad`. Confirm scale and
   any gearing against a known ruler-triangle or angle reference. CAD axes,
   V1 signs, normalized calibration and count midpoint are not measurements
   of the assembled lamp's zero.
6. **Measure support.** Trace contacting pads and frame origin on paper.
   Measure supported extreme points, confirm that included pads actually touch
   under load, and record their convex hull counterclockwise. Include desk-edge
   position and overhang. An inscribed approximation of a curved patch is
   conservative; an outer rectangle can invent support. Record unevenness,
   compression and desk slope; the model assumes a horizontal plane.
7. **Account for uncertainty and validate independently.** Retain repeated
   reading spread and instrument resolution. Choose a physical margin covering
   uncertainty plus an explicitly justified operating reserve, not the old
   threshold. Compare predicted COM with whole-lamp reaction measurements in
   securely supported folded, standing and intermediate poses. Disagreement
   calls for correcting mass allocation, measurements and frames. Do not seek
   a real fall as a calibration point.

## Continuous paths need more than frames

Both endpoints can be inside while an intermediate COM is outside. Fixed time
sampling can miss that interval; [OMPL's motion-validation documentation](https://ompl.kavrakilab.org/core/stateValidation.html)
describes this limitation. A bound between evaluations is needed to cover an
entire specified path.

For joint `j`, upper-bound each downstream COM's radius by summing intervening
origin-vector lengths and its local COM-vector length. Weight those bounds
by mass and divide by total mass to obtain `R_j`. Then
`|COM(q) - COM(p)| <= sum(R_j * |q_j - p_j|)` with angles in radians.

Signed Euclidean distance is 1-Lipschitz: COM movement cannot reduce clearance
by more than that movement's length. See [Coiffier and Béthune, page 2](https://gcoiffier.github.io/pdf/1lip.pdf).
Inside the coordinatewise endpoint box, the weighted distances to its two
endpoints sum to `S = sum(R_j * |delta_j|)`; one is at most `S/2`. Hence
`min(endpoint clearances) - S/2` bounds clearance below throughout the box.
This is an arithmetic derivation, not a measured lamp result.

Subdivide an inconclusive interval along its **actual** specified path.
Arithmetic joint midpoints work for common-scalar linear/smoothstep motion,
but their smaller diagonal boxes miss mixed-progress corners of arbitrary
asynchronous motion. For clamped Expression interpolation, evaluate the true
point on the original keyframe interval and then clamp; do not interpolate
already clipped endpoints. Work-limit exhaustion means unresolved, not safe.
This mathematical bound does not qualify servo tracking, lag or overshoot.

## Where Body and Expression would call it

The first builder owns integration. Preflight a standing transition or clip
before its first movement, then check the combined target at the motor boundary.
Body rounds final goals in [body.rs](../crates/body/src/body.rs) before
`send_goals`, which it calls from four places today. Combine head and arm into all five
canonical coordinates. When the arm is not written, use fresh observed arm
positions, not zeros or an assumed posture.

The helper and rest, relaxation, cancellation and fault callers need explicit
decisions too: each is one of `send_goals`' callers in `body.rs`, the startup
hold among them. Rejecting every write from an already outside pose
could prevent holding or stopping a push. Lean does not invent a recovery
movement. The separate guarded controller has preflight and goal boundaries
in [control.rs](../crates/body/src/control.rs), currently lines 759, 882 and
909; it is not the complete live Body loop.

[Expression offsets](../crates/msgs/src/expression.rs) are additive encoder
radians in canonical order. Check an isolated clip over its absolute Body
posture, Expression posture, actual weight and component limits, preserving
cubic smoothstep. `check_clip` covers the first through last authored keyframe
and requires a repeating cycle to close. It does **not** check admission from
the current pose to the first offsets, exit back to a posture, or a hold while
the base posture changes. Those entry/exit paths and interruption blends must
be composed and checked separately by the owner. Zero-posture or full-weight
checks do not cover intermediate weights, changed postures, overlapping clips
or live gaze. Passing arbitrary samples to `check_movement` only establishes
its declared joint-linear interpolation, not an unsampled composed curve.

Today's [Body adapter](../crates/body/src/expression_in.rs) accepts only yaw,
head roll and head pitch under `LAMP_EXPRESSION_BODY=on`, ignoring shoulder,
elbow and requested gaze. An abstract five-joint catalogue check is separate
from that applied mask. Body's older recorded clips and `Standing.pose` use
`[head_pitch, head_roll, base_yaw, shoulder, elbow]`: reorder to
`[v[2], v[3], v[4], v[1], v[0]]` before canonical conversion.

## Dependencies and alternatives

The crate reuses **glam 0.33.12**, pinned with default features disabled and
only `std` and `f64`. Its [manifest](https://raw.githubusercontent.com/bitshifter/glam-rs/0.33.12/Cargo.toml)
declares MIT OR Apache-2.0 and Rust 1.68.2 minimum, compatible with lampOS's
1.91 minimum. That configuration adds no normal transitive dependencies.
The [published archive](https://static.crates.io/crates/glam/glam-0.33.12.crate)
matched its crates.io SHA-256
`37bfe73dc8ec21f54d181e5a60554327ec3f1c600c455a75ccbfc65d2106a6c4`.
It supplies vector and rotation operations; model validation and the balance
contract remain this crate's responsibility.

[nalgebra](https://www.nalgebra.rs/docs/user_guide/points_and_transformations/)
offers suitable typed rotations/isometries, but was not already a dependency.
[k](https://docs.rs/k/0.32.0/k/struct.Chain.html) adds general URDF/COM machinery
beyond this fixed chain. Cached `ndarray` handles arrays rather than rotations.
[geo](https://docs.rs/geo/latest/geo/) supplies general hull/distance operations;
this bounded convex footprint has a narrower interface. Drake's
[COM API](https://drake.mit.edu/doxygen_cxx/classdrake_1_1multibody_1_1_multibody_plant.html)
is a reference for transformed mass weighting; its
[equilibrium solver](https://drake.mit.edu/doxygen_cxx/classdrake_1_1multibody_1_1_static_equilibrium_problem.html)
also models contact forces, nonpenetration and joint limits. No dynamics
solver or native robotics runtime is introduced.

## API and bounded work

Construct `Model` once from `Geometry` or `Model::from_json`. Unknown/missing
JSON fields and invalid geometry are errors; no default geometry is loaded.
`Angles::new([f64; 5])` validates complete encoder radians, while
`Angles::from_encoder_counts([u16; 5])` converts complete single-turn counts.
Neither silently wraps, clamps or fills a missing joint. Small numerical
values alone cannot reveal a caller's unit mistake; the caller must uphold
the radians/counts contract.

| Call | Coverage and result |
|---|---|
| `check_pose(angles)` | All five joint transforms, mass-weighted 3D COM, signed `clearance_m`, and the declared measurement status. `within_margin()` requires clearance greater than the 1 nm numerical reserve. |
| `check_movement(&[Step])` | Every supplied pose plus continuous joint-linear paths between them. A common monotone easing of all joints has that same geometric path. Time begins at zero; no shortest-angle wrapping is inferred. |
| `check_clip(base, Clip { ... }, sample_every_us)` | One authored smoothstep cycle on a fixed absolute base pose, a fixed Expression posture and weight, explicit component limits and an explicit applied-joint mask. It evaluates `base + mask * clamp(posture + weight * clip_offset, -limit, limit)`. Every keyframe and grid point is checked before adaptive subdivision. |

The two path calls return `MovementCheck`: prescribed `checked_steps`, total
`evaluated_poses`, the worst **evaluated** pose/time/clearance, a conservative
`minimum_clearance_bound_m` for the full specified interpolation, and:

- `WithinMargin`: the bound exceeds the numerical reserve everywhere.
- `OutsideMargin`: an actual evaluated pose has negative clearance.
- `Unresolved`: no outside pose was observed, but the bounded work cannot
  establish the reserve. Treat this as refusal for a proposed movement.

A negative bound alone does not establish an actual outside pose, and the
worst evaluated sample need not be the exact minimum of the continuous path.
Every prescribed pose is checked even after finding an outside sample. An
invalid input returns `Err`, not an empty successful report. A one-pose
movement is allowed; it establishes only that pose. A clip requires at least
two frames. Repeats with unequal first/last raw offsets are refused, including
differences on masked joints, rather than hiding a discontinuous seam.

| Input/work | Hard bound |
|---|---|
| Geometry JSON / provenance label | 64 KiB / 1–256 bytes, nonblank |
| Physical chain | Exactly five named links, each canonical joint once |
| Coordinates / mass | Finite components within ±2 m; each part 0–100 kg, fixed base strictly positive; total at least 0.001 kg |
| Rotations / axes | Finite radians within ±π; each finite axis within 0.000001 of unit length, then normalized |
| Footprint | 3–32 strictly convex counterclockwise vertices; input edges at least 1 µm; no repeated/collinear/crossed vertices; finite margin 0–1 m and a nondegenerate inset |
| Movement | 1–10,000 steps, strictly increasing microseconds from zero, at most 600 seconds |
| Clip | 2–64 keys, strictly increasing microseconds from zero, at most 10 seconds; sample interval 1–100,000 µs; conservative grid-plus-key count at most 10,000 before allocation |
| Clip values | Every offset/posture finite within ±0.35 rad even on ignored joints; limits 0–0.35 rad; weight 0–1 |
| Adaptive check | At most 100,000 total pose evaluations and 20 subdivision levels per interval |

Pose evaluation allocates nothing and performs no I/O. A path stores at most
10,000 sampled poses; a clip also stores at most 10,000 time entries. The
bounded recursive stack has no worker threads, queue or retries. The geometry
constructor clips a polygon in bounded storage (at most 66 temporary vertices).
Exhausting a depth/evaluation bound cannot create a positive decision. These
are work bounds, not wall-clock deadlines; near-boundary work can cost much
more than the ordinary 1,000-step fixture timed below. A future output owner
must keep large preflights out of its immediate feedback loop.

The single [stand-in data file](../crates/lean/geometry.stand-in.json) describes
five invented 0.2 m rods, each 0.1 kg at its midpoint, and a 0.5 kg fixed base.
Its square support extends ±0.1 m with a 0.01 m margin. It exists to make the
arithmetic independently checkable, not to estimate a production unit:

| Canonical angles (radians) | COM (metres) | Clearance |
|---|---|---:|
| `[0, 0, 0, 0, 0]` (standing fixture) | `[0, 0, 0.25]` | +0.09 m |
| `[0, π/2, -π, 0, π]` (folded fixture) | `[0.06, 0, 0.09]` | +0.03 m |
| `[0, π/2, 0, 0, 0]` (level arm fixture) | `[0.16, 0, 0.09]` | −0.07 m |

The folded fixture's rods intersect. None is a collision-qualified lamp pose.
For standing, the vertical moment is `0.1*(0.1+0.3+0.5+0.7+0.9) = 0.25 kg m`
and total mass is 1 kg. For level arm, the horizontal moment is
`0.1*(0.1+0.3+0.5+0.7) = 0.16 kg m`, beyond the 0.09 m inset edge.

## Run the finite catalogue check

The [read-only example](../crates/lean/examples/check.rs) requires three explicit
arguments: geometry JSON, an Expression catalogue JSON, and five comma-separated
single-turn encoder counts in canonical order. It limits file reads to 64 KiB
and 128 KiB. This command uses only the invented standing fixture:

```sh
cargo run -p lamp-lean --release --example check --offline --locked -- \
  crates/lean/geometry.stand-in.json crates/expression/behaviours.json \
  2048,2048,2048,2048,2048
```

It reports the baseline and each behaviour in each of its declared voice
states, with both masks (`all_five_joints` and `current_body_head_and_yaw`),
using a 20,000 µs grid plus every keyframe. At task base `6a1d2745c`, the five
behaviours produce 16 behaviour/state/mask checks. `actuation_qualified` is
always false, including when the supplied label says measured. The JSON states
what the isolated-cycle result does not cover. Exit 0 means only that all
these arithmetic checks passed; exit 1 means outside, unresolved or a clip
error; exit 2 means invalid arguments/files/JSON or an output error.

**First command on the lamp:** First builds this example for its target,
names the executable `lean-check`, and places it beside the stand-in JSON and
the exact checked Expression catalogue. From that directory:

```sh
./lean-check geometry.stand-in.json behaviours.json 2048,2048,2048,2048,2048
```

Expect `declared_measurement:"stand_in"`, baseline COM `[0,0,0.25]` within
floating-point tolerance, +0.09 m clearance, 16 `within_margin` clip results,
`model_within_margin_all_checks:true`, `actuation_qualified:false`, and exit 0.
This checks the target build's arithmetic and never moves anything.

Before future Body integration, repeat with the fully measured geometry,
actual catalogue and **all five freshly observed counts**. The five 2048
values above are synthetic inputs, not a prescribed lamp pose. Compare the
calculated COM with independent, securely supported scale readings as described
above. First qualifies measurement uncertainty, final-command composition,
tracking and recovery before enabling any new motor gate. The most likely
physical error is the masses or their allocation, followed by local COMs,
encoder zeros/axis signs and the loaded contact footprint.

## Gate and timing evidence

Development machine: macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K at
3.80 GHz, 16 logical CPUs, 64 GiB RAM. Rust/Cargo 1.96.0; compiler commit
`ac68faa20c58cbccd01ee7208bf3b6e93a7d7f96`. Commands run from `lampOS/`,
using `/Users/autonomous/.cargo/bin/cargo` on this machine:

```sh
cargo fmt --all --check
cargo clippy -p lamp-lean --all-targets --offline --locked -- -D warnings
cargo test -p lamp-lean --all-targets --offline --locked
cargo build --workspace --offline --locked
cargo tree -i lamp-lean --offline --locked
cargo +stable clippy -p lamp-lean --all-targets --target aarch64-unknown-linux-gnu --offline --locked -- -D warnings
```

All six commands passed. Native tests: 63 passed (10 footprint, 30 pose,
15 movement/clip, three catalogue, five example), zero failed, two timing
tests normally ignored and explicitly run below. Formatting and clippy emitted
no diagnostics. The new crate has no dependent workspace crate. The base
workspace built before this implementation, including the pinned `6a1d2745c`
base (52.95 seconds). The installed `+stable` compiler has the same 1.96.0
commit and includes the ARM64 standard library; this check is not ARM execution.

A 727-file source-only copy outside the parent repository, at
`/private/tmp/lamp-fourth-lean-final-jwpt8hc9`, passed the same 63 tests in the
existing local Podman Linux VM. Source and public dependency caches were mounted
read-only; no parent files, credentials, `.git` directory, network or host
target artifacts were available to the build. The VM runs Linux
6.12.13-200.fc41.x86_64 with six configured CPUs and 8 GiB RAM; its name
`lampos-arm64` does not change this **x86_64 execution**. The cached image has
Rust/Cargo 1.96.0. Exact command:

```sh
/usr/local/bin/podman run --rm --network none \
  -v /private/tmp/lamp-fourth-lean-final-jwpt8hc9:/source:ro \
  -v /Users/autonomous/.cargo/registry:/root/.cargo/registry:ro \
  -v /Users/autonomous/.cargo/git:/root/.cargo/git:ro \
  -w /source localhost/lampos-arm64-cross:latest \
  /bin/sh -c 'CARGO_TARGET_DIR=/tmp/lean-target cargo test -p lamp-lean --all-targets --offline --locked --target x86_64-unknown-linux-gnu'
```

The release example command above exited 0 and produced the expected 16
positive stand-in checks on the Mac. Running that same executable and files
with `2048,3072,2048,2048,2048` exited 1 with baseline clearance −0.07 m.
Supplying `2048,4096,2048,2048,2048` exited 2 and produced no result JSON.
These are intentional refusal cases, not failing gate tests.

**Failures retained:** no crate test or clippy stage failed. An initial worker
format command could not find `rustfmt` on `PATH`; its explicit Cargo-bin path
passed. The first sandboxed `sysctl` hardware read was denied; the authorized
read-only retry returned the CPU/memory identity above. Review added regressions
for actual-time clamping, non-midpoint smoothstep, physical-to-canonical radius
indexing, and separate depth/evaluation exhaustion before the final gate.

The preimplementation development targets were below 1 ms per pose and 10 ms
per ordinary 1,000-step movement check. One finite release invocation ran
`arithmetic_cost`; a second ran `inconclusive_cost`:

```sh
cargo test -p lamp-lean --release --test it cost::arithmetic_cost --offline --locked -- --ignored --nocapture
cargo test -p lamp-lean --release --test it cost::inconclusive_cost --offline --locked -- --ignored --nocapture
```

The first command initially ran before `inconclusive_cost` existed, so its
actual selector was `--test cost` without `arithmetic_cost`; there was exactly
one ignored test then. The commands above reproduce each workload separately,
avoiding concurrent benchmark interference. Each times its first call and
30 additional calls, using `Instant`, resident inputs and no cache eviction:

| Boundary | First (ms) | Repeated median (ms) | p95 (ms) | Worst (ms) |
|---|---:|---:|---:|---:|
| JSON parse plus model validation/construction | 0.061448 | 0.004308 | 0.006650 | 0.008095 |
| One static pose after model construction | 0.000667 | 0.000209 | 0.000281 | 0.000305 |
| Complete ordinary 1,000-step movement, including result allocation | 0.277907 | 0.203836 | 0.240004 | 0.256831 |
| One 1.2-second built-in repeating clip at 20 ms, 61 pose evaluations | 0.024176 | 0.015126 | 0.015775 | 0.017979 |
| Deliberately inconclusive two-step path, all 100,000 allowed evaluations | 19.921472 | 19.950070 | 21.012518 | 21.340156 |

The ordinary movement used sinusoidal small angles at 20 ms intervals and
required exactly 1,000 evaluations; the clip used the fixture's fixed baseline
and all-five mask. Inputs/model setup are excluded except in the explicitly
named construction row. The last row also includes two result assertions;
every result was `Unresolved`. Model setup, file I/O, process startup, transport,
Body composition and motor response are not inside the path timings. These are
first calls in their test process, not CPU-cold or disk-cold measurements.
Both ordinary targets passed on this Mac. The work-cap case demonstrates why
that ordinary-path target is not a universal deadline. No lamp timing follows.

## lamp-4ace, 14 October (first builder)

Taken onto `lamp-voice` unchanged (`bbbd82fc9`). On the first builder's Mac
its 63 tests pass and clippy is clean; built for the lamp, the same tests
pass under emulation. The example and the timing test were then run on
lamp-4ace itself at 11:05, beside the running voice and body. Arithmetic
only: nothing here commands a motor.

| What was run on the lamp | What the handoff said to expect | What the lamp showed |
|---|---|---|
| `./lean-check geometry.stand-in.json behaviours.json 2048,2048,2048,2048,2048` | clearance +0.09 m, 16 `within_margin`, all checks true, not qualified, exit 0 | The same, exit 0 |
| The level-arm counts `2048,3072,2048,2048,2048` | exit 1 | exit 1 |
| A count of 4096 | exit 2, no JSON | exit 2, nothing on standard output |

Timing on the lamp, the cost test in release (`Instant` around each call,
inputs resident; first call, then 30 more: median, p95, worst, in ms):

| Boundary | First | Median | p95 | Worst |
|---|---|---|---|---|
| Parse the geometry and make the model | 0.113 | 0.016 | 0.057 | 0.066 |
| One pose | 0.026 | 0.0006 | 0.002 | 0.003 |
| A 1,000-step path | 1.506 | 0.582 | 1.481 | 1.505 |
| A 1.2 s clip, 61 evaluations | 0.117 | 0.040 | 0.109 | 0.112 |
| All 100,000 evaluations spent, unresolved | 63.0 | 46.9 | 47.3 | 47.3 |

The whole example program once, from start to exit with its two file reads
and 16 checks, took 7.4 ms. One pose is far inside the 1 ms the handoff set;
a path that runs out its work cap is not, and belongs off the path to a
first movement, as the handoff says.

**The lamp's postures.** The body says its five motor readings on
`body/joint_states` (radians; counts are 2048 + radians x 4096 / 2pi). At
rest at 11:08: `2023,2052,2047,2053,1950`. Standing, as it does when someone
speaks, at 11:06: `2023,2558,2503,2053,1820`; at 11:11
`2014,2629,2603,2053,1771`.

**A file from the first software's model, and why it is still a stand-in.**
[`deploy/lamp-4ace/lean-first-software.json`](../deploy/lamp-4ace/lean-first-software.json)
is the first software's
[body description](https://github.com/autonomous-ai/lampOS/blob/bbbd82fc9/robots/lamp/urdf/lamp.urdf)
in this check's form, made by
[`crates/lean/tools/from_first_software.py`](../crates/lean/tools/from_first_software.py):
joint positions and axes as drawn by the maker, each estimated mass at its
joint, and zero positions at the middles of the motor ranges in the first
software's calibration kept on lamp-4ace
(`/root/bootstrap/rollback/hal.previous/calibration/robots/hal_follower/hal.json`,
dated 2026-08-21). Given the same angles, this check and the first
software's own function put the centre of mass in the same place to within
0.03 mm over 42 poses (the two rest and standing readings and 40 drawn at
random); [a test](../crates/lean/tests/it/first_software.rs) keeps four of them.
That shows the conversion is right. It does not make the file the lamp:

- The masses add up to 1.49 kg; the lamp's own specification
  (`robots/lamp/README.md`) says 7 kg, and its parts list has an optional
  steel plate of about 3 kg in the base. Most of the weight is unaccounted for.
- The first software's limit cannot be carried over. Its motors ran in
  -100..100 of each calibrated range (`use_degrees` is false and is never set),
  and its tipping check read those values as degrees; so its 22 mm limit, the
  31.6 mm of the clip that tipped a lamp and the 17.7 mm of its worst shipped
  clip belong to that scale only. Scored that way, the lamp's rest posture
  comes to 19.1 mm and standing to 13.1 mm: the first software would have
  played both, and warned on rest (above 85 % of its limit).
- Nobody has measured the base. The footprint in the file is the 100 mm circle
  inside the 200 mm square that the specification gives for the whole lamp,
  drawn as 32 sides, at most 0.48 mm stricter than the circle. With it every
  pose tried is far inside (rest +81.7 mm, standing +98.0 mm), which says
  nothing until the masses are real.

So the file stays `stand_in` and the check qualifies nothing. What it adds:
the lamp's true joint geometry and zeros in this check's form, so that when
the parts are weighed and the base drawn ("Measurements the first builder
needs" above), only masses, centres and the outline change. The motions lampOS
makes today are small beside the first software's dances: standing up moves
the shoulder and elbow about 510 and 455 counts together, and Expression's
nods move the head 20 to 40 counts. The body uses this file only for V1's anti-tip
rule, with V1's own limits ([body.md](body.md), "Not tipping over"). That qualifies nothing.

## What this does not establish

The criterion describes static gravity balance on rigid horizontal contacts.
It does not establish resistance to acceleration, a shove, cable pull,
slipping, pad deformation, desk tilt, collision or torque failure.
[Contact-stability research](https://scaron.info/publications/tro-2016.html)
treats friction and dynamic constraints separately from geometric support.
A bounded mathematical path does not establish that physical servos follow
it together without lag or overshoot.

Fourth has not measured the lamp's dimensions, masses, COM offsets, zero/sign
calibration, loaded footprint, operating margin, folded/standing safety,
end-to-end response time or resistance to tipping. Body uses this file only
for V1's anti-tip rule ([body.md](body.md)). A provenance label cannot supply
those missing measurements.

Not shown.
