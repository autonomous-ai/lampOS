# Reach: where each joint may go, at each pose

Today the body keeps each joint inside a fixed box around home (the product's
`--reach 150,15,1,250`: head pitch 150 counts up and 15 down, roll 1, base yaw
250, the arm only between folded and the stand). A fixed box is the wrong
shape both ways. It refuses safe poses: the head may tip far further down
while the lamp stands than 15 counts. And it lets unsafe ones through: lamp-0c89
fell on 2026-09-04 to a recording that kept every joint in its range
(`docs/v1-verdicts.md`, `docs/lean.md`).

This study maps the lamp's poses by whether the whole body is safe in them, so
that a joint's range can be read at any pose with the others held, and a path
checked as it goes. It is offline arithmetic and moves nothing. The body, its
rules and its owner decide whether any of it is ever used.

```sh
uv run python reach.py --step 32 --margin-mm 2    # about 5 minutes on a laptop
```

It reads the CAD from `lampOS/.cache/reach/lamp.glb`, a copy of
`robots/lamp/hardware/cad/glb/lamp.glb`. That file is in Git LFS; with LFS not
installed for the repository, copy the object from
`.git/lfs/objects/78/cf/78cfd2…` (its sha256 is the pointer's oid). It writes
`map.npz`, `report.json` and `slices.png` there.

## What makes a pose safe

A pose is refused when any of these fails (`reach.py` `judge`):

| Rule | From | Status |
|---|---|---|
| each servo inside its span | the old runtime's calibrated spans for the head and base; the arm from folded to 1,400 above it (`--stand`'s cap) | per-lamp limits not read yet (registers 9 and 11) |
| the centre of mass within 22 mm of the base's axis | the body's anti-tip rule, `cog.rs`, ported in `body.py` `Lean`: reproduces `cog.rs`'s tests (17.850 against 17.851 mm folded, 1.558 against 1.553 standing) | **masses are V1's estimates, 1.49 kg against 7 kg on the spec**: every tipping boundary moves when they are weighed |
| no two parts that are not next to each other in the chain closer than the margin | the CAD's own meshes, each part skinned to its bone, posed by `lamp.model.json`'s chain | the cable (`6_wire`) is left out: it bends across all five bones and the CAD does not say how far it may |
| no moving part below the table plus the margin | the same | |

Each part is a point cloud, one point per occupied 4 mm voxel, in its bone's own
frame. A distance is between voxel centres, so a touch reads as a millimetre or
two.

## Checked against a lamp

The one contact measured on a lamp: lamp-52e6 folded, only the head's pitch
moved, met the base going down at 2,010 counts and rose freely going up to
1,474 (`docs/body.md`, 2026-10-14). `validate_pitch` asks the model the same:

| pitch counts | 1474 | 1600 | 1771 | 1900 | 2010 | 2100 |
|---|---|---|---|---|---|---|
| clearance, this model (mm) | 13.7 | 12.4 | 1.8 | 1.7 | 1.1 | 0.8 |
| clearance, `lamp.model.json`'s pitch sign (mm) | 0.8 | 1.7 | 1.7 | 7.6 | 13.7 | 13.7 |

**`lamp.model.json`'s head pitch turned the wrong way** (its sign was "cad, until
measured"): as the file had it, the folded head touched the arm at fewer counts
and lifted away at more, the opposite of the lamp, and the camera's measured gaze
(more counts is down, `docs/body.md`, the gaze table) agrees with the lamp. The
model's sign is -1 now (the pitch-sign change, #7513), the virtual lamp's camera
and the SDK twin follow it, and this study uses the file as it is: the column
above is the old sign, kept as the reason.

The poses the lamp really takes read as it does (margin aside):

| Pose (lamp-4ace) | Least clearance | Above table | Off the axis |
|---|---|---|---|
| standing | 16.5 mm | 181 mm | 2.7 mm |
| half way up | 16.8 mm | 130 mm | 10.3 mm |
| folded | 1.5 mm (the head rests on its arm) | 68 mm | 18.0 mm |
| elbow folded, shoulder still up | 1.6 mm (a near touch) | 100 mm | 10.0 mm |
| shoulder folded, elbow still up | 13.7 mm | 68 mm | **37.0 mm: would tip** |

The last row is why the body folds the elbow first (`docs/body.md`, "the elbow
folds first"): the map refuses the other order for tipping, and lets the elbow-
first path through with the shade a near touch from the swivel. The folded pose
itself is refused at a 2 mm margin, since its head rests on the arm: the map is
for moving, and folding and waking stay the body's own.

## What it found (lamp-4ace's poses, margin 2 mm, 2.8° grid)

- **Head pitch, standing:** from the servo's top (27° up) to about 77° down
  before the shade meets the arm. Today: 13° up, 1.3° down.
- **The arm:** the safe poses are a band along the fold, the shoulder and the
  elbow moving together. It goes on past the stand. Tipping closes it on
  both sides when the two diverge, which a fixed box per joint cannot express.
- **Elbow, standing:** −45° to +74° with the rest held; **shoulder:** −30° to +24°.
- Of the whole grid, 28% of poses are safe; 51% would tip (by V1's masses), 37%
  have parts colliding (most with the head tipped far down), 3% reach the table.

A grid answers exactly only at its points. Between them a part can move 8 to
16 mm, more than the margin, so the grids here are an overview, never the
safety answer: twice they called a pose safe that is not (below).

## Five joints (`reach5.py`, `query5.py`)

Each rule depends on only some joints, so each is its own table over just
those, and a pose is safe when every table says so. Tipping does not change as
the base turns (shoulder, elbow, roll, pitch); the shade against the upper arm
depends on pitch and roll alone; only a moving part against the fixed base
needs all five. Tables of up to four joints use the 2.8° grid; the shade
against the base, the one five-joint table, 5.6° (roll 11°). 19 minutes; 35% of
the 11M five-joint poses are safe, 51% tip, 25% collide, 2% reach the table.

## A joint's range without a grid (`march.py`)

From a safe pose, the nearest gap between two parts proves how far a joint may
turn before any point closes it (its lever times the turn), so the march takes
that step and measures again (conservative advancement, "sphere tracing"); the
centre of mass steps the same way with its own bound. Steps shrink to a count
at a limit, and nothing between two measured poses can be missed, which a plain
binary search over a joint cannot promise. At the standing pose:

| Joint | Range (counts) | Degrees | What stops it | Checks |
|---|---|---|---|---|
| base yaw | −1168 · +1260 | −102.7 · +110.7 | the servo's span | 87 |
| shoulder | −345 · +272 | −30.3 · +23.9 | tipping | 30 |
| elbow | −541 · +797 | −47.5 · +70.0 | tipping | 49 |
| head roll | −1422 · +1211 | −125.0 · +106.4 | the servo's span | 94 |
| head pitch | −308 · +883 | −27.1 · +77.6 | the servo's span · the shade meets the arm | 31 |

Base yaw and roll meet nothing in the CAD; the cable is their likely real limit.

## Checked (offline)

- The centre of mass in Python equals the body's own Rust on 10,000 random
  poses (`crates/body/examples/cog_eval.rs`): at most 4e-14 mm apart, the same
  verdict at 22 mm on every pose.
- Every count along each joint at the standing pose (9,569 poses): the march
  called no unsafe count safe, and stopped 0 to 2 counts short of the truth.
- The grids against that truth: the 3D grid allowed the elbow to +837 (its
  nearest point has the shoulder 15 counts higher, where the centre of mass is
  21.9 mm off the axis against 23.1 mm at the real pose), and the 5D grid head
  pitch to +80° against +77.6°.

## Moving between poses: tried, and set aside

`plan.py` (a roadmap with A*), `lattice.py` (A* over fixed directions, each a
jump) and `midpoint.py` (recursive midpoints, nudged off the line when blocked)
were tried on presets and on random poses near them. The certified straight
line already joins all six preset pairs, and 199 of 200 random pairs. For the
blocked ones, midpoints found a path for six of six, usually one waypoint and a
small "get clear first" move, in 1 to 5 s, but one took 654 s; the roadmap and
the lattice missed the cramped case. A shortest path is not what makes a lamp
move like a living thing either. So the map is a safety check, not a planner:
motion comes from presets, clips and the motion model, and the check passes a
step or cuts it at its last safe point.

## Not yet known, before any of it moves a lamp

1. **The masses.** Weigh the parts, or read the centre of mass from the
   servos' loads at rest in several poses, and give `lamp-lean` measured
   geometry. The tipping boundaries are only as good as these.
2. **The cable** through the neck and the head's roll: how far each may turn
   before it pulls. The body's `--pitch-window` floor (1,670) may be one such
   limit; the CAD has no answer.
3. **Each lamp's own spans and calibration** (homes and folded poses differ by
   a few degrees per lamp).
4. **A slow sweep on a lamp**, one joint at a time under the lock, its force
   held low and its load logged, against the map's boundaries.
5. **Pinch:** a hand between the shade and the arm near a boundary.

Then the body could take its ranges from the check instead of `--reach`:
`check_pose` (safe, and by how many millimetres), `cut_step` (a step cut at its
last safe point, as `cog::limit_lean_back` already cuts leaning back),
`range_at` (the march), and an offline check of a whole move file. In Rust, with
each part as a few dozen spheres instead of point clouds, a pose check is
estimated at 10 to 50 µs on the lamp's processor (not measured): a step check
fits a 20 ms tick.

## Using a map

- `python3 rejudge.py map.npz new.npz --lean MEASURED.json --tip-mm 22 --margin-mm 2` judges the same 3D map again with other
  masses, limit or margin in seconds: the parts-against-parts pass (15 minutes) does not depend on them.
- `python3 query.py map.npz SHOULDER ELBOW PITCH [--shrink COUNTS]` gives each joint's safe range at a pose (the others held);
  `--export out.json` writes the map for a page or a test. A grid is an overview (above): keep `--shrink` at 48 counts or more,
  past the 40 counts the 3D grid overshot the exact march at the standing pose.
- `node examples/twin/try.js --real --keyring DIR --reach-map examples/twin/reach-map-4ace.json --reach-shrink 48` (from `sdk/js`) puts the same
  guard on the page of a real lamp: the page only, the body's own limits unchanged; the first map is the one in use.
- The try-it page's guard (`sdk/js/examples/twin/reach-guard.js`, `server.js --reach-map reach-map-4ace.json`) checks the
  page's own goals against such a file before sending them. The page only; the body's limits are unchanged.
- The map is lamp-4ace's geometry and home, with V1's estimated masses, until each lamp's own are measured.
- `lean-base-heavy-estimate.json` is a second set of masses, plainly an ESTIMATE and not measured: the base 6.0 kg and the arm and
  head 1.0 kg (7 kg in all, the spec's figure), as `reach-map-4ace-base-heavy.json` was judged with
  (`rejudge.py map.npz out.npz --lean lean-base-heavy-estimate.json`, then `query.py out.npz --export ... --label ...`). With the
  base that heavy tipping stops binding; the limits that remain are contacts (parts against parts, the table) and each servo's span
  from the old runtime's calibration (the servos' own stored limits are not read), and the cable is not modelled. The page takes
  several maps (`--reach-map a.json,b.json`) and a picker switches between them.
