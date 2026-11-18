"""Where each joint may go, at each pose: a map of the lamp's safe poses over
shoulder x elbow x head pitch, and each joint's range at a pose with the others
held (README.md). Offline.

    uv run python reach.py [--step 32] [--margin-mm 3]

Writes lampOS/.cache/reach/map.npz (the grid and why each pose is refused),
report.json (each joint's range at the named poses, against today's fixed box)
and slices.png (the three planes through the standing pose).
"""
from __future__ import annotations

import argparse
import itertools
import json
import os

import numpy as np
from scipy.spatial import cKDTree

import body

OUT = os.path.join(body.LAMPOS, ".cache", "reach")
GLB = os.path.join(OUT, "lamp.glb")

# The lamp the map is made for: lamp-4ace's standing and folded poses (cog.rs's
# tests), bus order (yaw, shoulder, elbow, roll, pitch). Its home, from which
# lamp.model.json's degrees count: the stand's head and base, the folded arm.
STAND = np.array([2015, 2657, 2602, 2043, 1772], float)
FOLDED = np.array([2015, 2048, 2047, 2043, 1950], float)
HOME = np.array([2015, 2048, 2047, 2043, 1772], float)
# What each servo may do at all: the old runtime's calibrated spans for the head
# and base (crates/body/tools/generate_clips.py RANGE), the arm from folded up to
# the most a stand may be from folded (body.md, `--stand`: 1,400), a little below.
SERVO = {0: (847, 3275), 1: (FOLDED[1] - 48, FOLDED[1] + 1400), 2: (FOLDED[2] - 48, FOLDED[2] + 1400),
         3: (621, 3254), 4: (1464, 3071)}
# The rules a pose must keep.
CLIP_MM, LOOK_MM = 22.0, 16.0  # cog.rs: off the axis while moving; leaning back, held
# Parts next to each other in the chain touch at their joint by design.
CHAIN = {("base", "base_yaw"), ("base_yaw", "base_pitch"), ("base_pitch", "elbow_pitch"),
         ("elbow_pitch", "wrist_pitch"), ("wrist_pitch", "wrist_roll")}
PARTS = ["base"] + body.BONES
PAIRS = [(a, b) for a, b in itertools.combinations(PARTS, 2) if (a, b) not in CHAIN]
# Today's fixed box, the product's `--reach 150,15,1,250` around home (pitch up,
# pitch down, roll, yaw; deploy/product/lamp-body.service), the arm only between
# folded and the stand.
TODAY = {"pitch_up": 150, "pitch_down": 15, "roll": 1, "yaw": 250}
# Refusal codes, a bit each.
SERVO_BIT, TIP_BIT, HIT_BIT, TABLE_BIT = 1, 2, 4, 8


def validate_pitch(cad_fine: body.Cad, cad_coarse: body.Cad) -> dict:
    """The one contact measured on a lamp, asked of the model: lamp-52e6 folded
    (cog.rs LAMPS; bus order yaw 2035, shoulder 2053, elbow 2069, roll 2035, its
    home pitch 1771), only the head's pitch moved. Measured: more counts met the
    base at 2,010; fewer rose freely to 1,474 (docs/body.md, 2026-10-14)."""
    folded = np.array([2035, 2053, 2069, 2035, 1771], float)
    pitch = np.arange(1450, 2500, 5.0)
    c = np.tile(folded, (len(pitch), 1))
    c[:, 4] = pitch
    least, _ = clearance(cad_fine, cad_coarse, body.deg_from_counts(c, folded))
    at = {int(p): round(float(least[np.argmin(np.abs(pitch - p))]), 1) for p in (1474, 1600, 1771, 1900, 2010, 2100, 2250, 2400)}
    # What the measurement asks: free (clear) all the way up to 1,474, and touching by 2,010
    # and past it. The folded head rests a voxel or so from its arm in the CAD, so
    # "touching" is read as under 2 mm and "free" as more than that, growing upward.
    agrees = at[1474] > 5.0 and at[1600] > at[1771] and at[2010] < 2.0 and at[2100] < 2.0
    return {"measured": "lamp-52e6 folded: more counts met the base at 2010; fewer free to 1474",
            "model_clearance_mm_by_pitch": at, "agrees": bool(agrees)}


def clearance(cad_fine: body.Cad, cad_coarse: body.Cad, deg: np.ndarray, chunk: int = 2000) -> tuple[np.ndarray, np.ndarray]:
    """For each pose: the least distance (mm) between any two parts not next to
    each other, and the lowest point of the moving parts above the table (mm).
    Distances are between voxel centres (each cloud one point per occupied voxel),
    so a touch reads as a millimetre or so: `validate_pitch` gives the model's
    reading at the one measured contact."""
    trees = {p: cKDTree(cad_fine.points[p]) for p in PARTS}
    least = np.full(len(deg), np.inf)
    lowest = np.full(len(deg), np.inf)
    for s in range(0, len(deg), chunk):
        d = deg[s:s + chunk]
        worlds = cad_coarse.bone_worlds(d)
        frames = {"base": np.broadcast_to(np.eye(4), (len(d), 4, 4))} | worlds
        for a, b in PAIRS:
            # b's coarse points carried into a's own frame, where a's tree sits.
            rel = np.linalg.inv(frames[a]) @ frames[b]
            pts = np.einsum("nij,kj->nki", rel[:, :3, :3], cad_coarse.points[b]) + rel[:, None, :3, 3]
            dist = trees[a].query(pts.reshape(-1, 3), workers=-1)[0].reshape(len(d), -1).min(axis=1)
            least[s:s + chunk] = np.minimum(least[s:s + chunk], dist * 1000)
        for name in ("elbow_pitch", "wrist_pitch", "wrist_roll"):
            m = worlds[name]
            y = np.einsum("nj,kj->nk", m[:, 1, :3], cad_coarse.points[name]) + m[:, None, 1, 3]
            lowest[s:s + chunk] = np.minimum(lowest[s:s + chunk], y.min(axis=1) * 1000)
    return least, lowest


def judge(counts: np.ndarray, lean: body.Lean, cad_fine, cad_coarse, margin_mm: float, tip_mm: float = CLIP_MM) -> dict[str, np.ndarray]:
    """Every rule, for (N, 5) bus-order counts."""
    servo_ok = np.ones(len(counts), bool)
    for j, (lo, hi) in SERVO.items():
        servo_ok &= (counts[:, j] >= lo) & (counts[:, j] <= hi)
    off = lean.off_axis_mm(counts)
    back = lean.lean_back_mm(counts)
    least, lowest = clearance(cad_fine, cad_coarse, body.deg_from_counts(counts, HOME))
    why = (np.where(servo_ok, 0, SERVO_BIT) | np.where(off <= tip_mm, 0, TIP_BIT)
           | np.where(least >= margin_mm, 0, HIT_BIT) | np.where(lowest >= margin_mm, 0, TABLE_BIT))
    return {"why": why.astype(np.uint8), "off_axis_mm": off, "lean_back_mm": back, "clearance_mm": least, "above_table_mm": lowest}


def interval(axis_values: np.ndarray, ok: np.ndarray, at: float) -> tuple[float, float] | None:
    """The run of safe values along one axis that holds `at` (the nearest grid
    point), as (low, high); None when `at` itself is not safe."""
    i = int(np.argmin(np.abs(axis_values - at)))
    if not ok[i]:
        return None
    lo = hi = i
    while lo > 0 and ok[lo - 1]:
        lo -= 1
    while hi < len(ok) - 1 and ok[hi + 1]:
        hi += 1
    return float(axis_values[lo]), float(axis_values[hi])


def draw_slices(sh: np.ndarray, el: np.ndarray, pi: np.ndarray, why: np.ndarray, path: str) -> None:
    """The three planes of the map through the standing pose, each pose coloured
    by the first rule it breaks, today's fixed box drawn over them."""
    from PIL import Image, ImageDraw

    colour = {0: (90, 190, 110), TIP_BIT: (220, 70, 60), HIT_BIT: (240, 160, 40), TABLE_BIT: (70, 120, 220),
              SERVO_BIT: (160, 160, 160)}

    def first(code: int) -> int:
        for bit in (SERVO_BIT, TABLE_BIT, HIT_BIT, TIP_BIT):
            if code & bit:
                return bit
        return 0

    i, j, k = (int(np.argmin(np.abs(a - v))) for a, v in ((sh, STAND[1]), (el, STAND[2]), (pi, STAND[4])))
    planes = [("shoulder", "elbow", sh, el, why[:, :, k], (FOLDED[1], STAND[1]), (FOLDED[2], STAND[2])),
              ("shoulder", "head pitch (+ is down)", sh, pi, why[:, j, :], (FOLDED[1], STAND[1]),
               (HOME[4] - TODAY["pitch_up"], HOME[4] + TODAY["pitch_down"])),
              ("elbow", "head pitch (+ is down)", el, pi, why[i, :, :], (FOLDED[2], STAND[2]),
               (HOME[4] - TODAY["pitch_up"], HOME[4] + TODAY["pitch_down"]))]
    cell, pad = 6, 40
    width = sum(len(p[2]) * cell + 2 * pad for p in planes)
    height = max(len(p[3]) for p in planes) * cell + 2 * pad + 40
    img = Image.new("RGB", (width, height), "white")
    d = ImageDraw.Draw(img)
    ox = 0
    for xname, yname, xs, ys, plane, xbox, ybox in planes:
        h = len(ys) * cell
        for a in range(len(xs)):
            for b in range(len(ys)):
                x0, y0 = ox + pad + a * cell, pad + h - (b + 1) * cell
                d.rectangle((x0, y0, x0 + cell - 1, y0 + cell - 1), fill=colour[first(int(plane[a, b]))])

        def px(v, axis):
            return (ox + pad + (v - xs[0]) / (xs[1] - xs[0]) * cell) if axis == 0 else (pad + h - (v - ys[0]) / (ys[1] - ys[0]) * cell)

        d.rectangle((px(min(xbox), 0), px(max(ybox), 1), px(max(xbox), 0), px(min(ybox), 1)), outline=(0, 0, 0), width=2)
        sx = STAND[{"shoulder": 1, "elbow": 2}[xname]]
        sy = STAND[{"elbow": 2}.get(yname, 4)]
        d.ellipse((px(sx, 0) - 4, px(sy, 1) - 4, px(sx, 0) + 4, px(sy, 1) + 4), fill=(0, 0, 0))
        d.text((ox + pad, pad + h + 6), f"{xname} {xs[0]:.0f}..{xs[-1]:.0f} ->", fill="black")
        d.text((ox + 4, 8), f"^ {yname} {ys[0]:.0f}..{ys[-1]:.0f}", fill="black")
        ox += len(xs) * cell + 2 * pad
    d.text((8, height - 18), "green safe | red tips (22 mm) | orange parts collide | blue hits the table | "
           "black box: today's fixed range | dot: standing", fill="black")
    img.save(path)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--step", type=int, default=32, help="grid step, counts (32 is 2.8 degrees)")
    ap.add_argument("--margin-mm", type=float, default=3.0, help="least clearance between parts and above the table")
    ap.add_argument("--tip-mm", type=float, default=CLIP_MM, help="the most the centre of mass may be off the axis (the body's is 22)")
    ap.add_argument("--lean", default=body.LEAN_JSON, help="the masses and centres, in lean-first-software.json's form (the measured ones go here)")
    ap.add_argument("--out", default=OUT, help="the folder map.npz, report.json and slices.png are written to")
    args = ap.parse_args()
    lean = body.Lean(args.lean)
    cad_fine = body.Cad(GLB, voxel_m=0.004)
    cad_coarse = body.Cad(GLB, voxel_m=0.004)
    check = validate_pitch(cad_fine, cad_coarse)
    print("validation:", json.dumps(check))

    sh = np.arange(SERVO[1][0], SERVO[1][1] + 1, args.step)
    el = np.arange(SERVO[2][0], SERVO[2][1] + 1, args.step)
    pi = np.arange(SERVO[4][0], SERVO[4][1] + 1, args.step)
    S, E, P = np.meshgrid(sh, el, pi, indexing="ij")
    counts = np.tile(STAND, (S.size, 1))
    counts[:, 1], counts[:, 2], counts[:, 4] = S.ravel(), E.ravel(), P.ravel()
    print(f"{S.size} poses: shoulder {len(sh)} x elbow {len(el)} x pitch {len(pi)}, step {args.step}")
    r = judge(counts, lean, cad_fine, cad_coarse, args.margin_mm, args.tip_mm)
    shape = S.shape
    why = r["why"].reshape(shape)
    out = args.out
    os.makedirs(out, exist_ok=True)
    np.savez_compressed(os.path.join(out, "map.npz"), shoulder=sh, elbow=el, pitch=pi, why=why,
                        **{k: v.reshape(shape).astype(np.float32) for k, v in r.items() if k != "why"})

    ok = why == 0
    named = {"stand": STAND, "folded": FOLDED,
             "half_up": FOLDED + (STAND - FOLDED) * 0.5}
    report = {"lamp": "lamp-4ace (cog.rs poses), CAD robots/lamp/hardware/cad/glb/lamp.glb, masses V1's estimates",
              "step_counts": args.step, "margin_mm": args.margin_mm, "tip_mm": args.tip_mm, "lean": os.path.basename(args.lean),
              "share_safe": float(ok.mean()),
              "refused_by": {name: float(((why & bit) > 0).mean()) for name, bit in
                             (("servo", SERVO_BIT), ("tip", TIP_BIT), ("self_collision", HIT_BIT), ("table", TABLE_BIT))},
              "today_box": TODAY, "validation": check, "poses": {}}
    for name, pose in named.items():
        i = int(np.argmin(np.abs(sh - pose[1])))
        j = int(np.argmin(np.abs(el - pose[2])))
        k = int(np.argmin(np.abs(pi - pose[4])))
        entry = {"counts": pose.round().tolist(), "safe": bool(ok[i, j, k])}
        for joint, axis_values, line, at in (("shoulder", sh, ok[:, j, k], pose[1]), ("elbow", el, ok[i, :, k], pose[2]),
                                              ("pitch", pi, ok[i, j, :], pose[4])):
            iv = interval(axis_values, line, at)
            entry[joint] = None if iv is None else {"low": iv[0] - at, "high": iv[1] - at,
                                                    "low_deg": round((iv[0] - at) * 360 / 4096, 1),
                                                    "high_deg": round((iv[1] - at) * 360 / 4096, 1)}
        report["poses"][name] = entry
    json.dump(report, open(os.path.join(out, "report.json"), "w"), indent=1)
    draw_slices(sh, el, pi, why, os.path.join(out, "slices.png"))
    print(json.dumps({k: v for k, v in report.items() if k != "poses"}, indent=1))
    for name, entry in report["poses"].items():
        print(name, "safe" if entry["safe"] else "REFUSED",
              {j: (None if entry[j] is None else (entry[j]["low_deg"], entry[j]["high_deg"])) for j in ("shoulder", "elbow", "pitch")})


if __name__ == "__main__":
    main()
