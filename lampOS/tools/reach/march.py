"""A joint's safe range at a pose without a grid: march along the joint, each step
as long as the clearance proves safe (conservative advancement, "sphere tracing").

At a safe pose the nearest gap between two parts is d mm (less the margin), and a
turn of the joint by an angle a moves no point farther than r * a, r being the
farthest any point moved by that joint lies from the joint's own origin. So a turn
of up to d / r cannot close the gap: the march takes it, and measures again. Near
a limit the steps shrink to a count. Tipping and the table step the same way,
each with its own bound. Nothing between two measured poses can be missed, which
a plain binary search over the joint cannot promise when the safe values are not
one run (the shade passing a corner of the arm).

    uv run python march.py      # each joint's range at the standing pose, with its cost
"""
from __future__ import annotations

import itertools
import json
import time

import numpy as np
from scipy.spatial import cKDTree

import body
import reach

J = {"yaw": 0, "shoulder": 1, "elbow": 2, "roll": 3, "pitch": 4}
LINK_JOINT = {"base_yaw": "yaw", "base_pitch": "shoulder", "elbow_pitch": "elbow", "wrist_pitch": "pitch", "wrist_roll": "roll"}
CHAIN = ["base"] + body.BONES
PAIRS = [(a, b) for a, b in itertools.combinations(CHAIN, 2) if abs(CHAIN.index(a) - CHAIN.index(b)) > 1]
RAD_PER_COUNT = 2 * np.pi / 4096


class Body:
    def __init__(self, voxel_m: float = 0.004, margin_mm: float = 2.0):
        self.cad = body.Cad(reach.GLB, voxel_m=voxel_m)
        self.trees = {p: cKDTree(self.cad.points[p]) for p in CHAIN}
        self.lean = body.Lean()
        self.margin = margin_mm
        self.evaluations = 0

    def measure(self, counts: np.ndarray) -> dict:
        """Everything the march needs at one pose (bus-order counts)."""
        self.evaluations += 1
        c = np.asarray(counts, float)[None]
        w = self.cad.bone_worlds(body.deg_from_counts(c, reach.HOME))
        frames = {"base": np.eye(4)} | {k: v[0] for k, v in w.items()}
        world = {p: self.cad.points[p] @ frames[p][:3, :3].T + frames[p][:3, 3] for p in CHAIN}
        gap = min(self.trees[a].query((world[b] - frames[a][:3, 3]) @ frames[a][:3, :3])[0].min() for a, b in PAIRS) * 1000
        table = min(world[p][:, 1].min() for p in ("elbow_pitch", "wrist_pitch", "wrist_roll")) * 1000
        off = float(self.lean.off_axis_mm(c)[0])
        # Each joint's lever: the farthest point it moves from its own origin.
        lever = {}
        for k, bone in enumerate(body.BONES):
            origin = frames[bone][:3, 3]
            moved = np.concatenate([world[p] for p in body.BONES[k:]])
            lever[LINK_JOINT[bone]] = float(np.linalg.norm(moved - origin, axis=1).max()) * 1000
        return {"gap": gap, "table": table, "off": off, "lever": lever}

    def tip_rate(self, counts: np.ndarray, joint: str) -> float:
        """A bound on how fast the centre of mass leaves the axis as `joint` turns,
        mm a count: the distal mass share times its lever, doubled for slack."""
        h = 8.0
        a, b = np.array(counts, float), np.array(counts, float)
        a[J[joint]] -= h
        b[J[joint]] += h
        d = self.lean.off_axis_mm(np.stack([a, b]))
        return max(abs(d[1] - d[0]) / (2 * h) * 2.0, 1e-3)

    def march(self, pose: np.ndarray, joint: str, direction: int, limit: float) -> tuple[float, int]:
        """From a safe `pose`, turn `joint` in `direction` (+1 or -1) until a rule
        would break or the servo's span ends; the last count proved safe."""
        q = np.array(pose, float)
        start = self.evaluations
        while True:
            m = self.measure(q)
            gap = min(m["gap"], m["table"]) - self.margin
            tip = reach.CLIP_MM - m["off"]
            if gap < 0 or tip < 0:
                raise ValueError(f"not safe at {q}: gap {gap:.1f} mm, tip room {tip:.1f} mm")
            lever = max(m["lever"][joint], 1.0)
            by_gap = gap / (lever * RAD_PER_COUNT)
            by_tip = tip / self.tip_rate(q, joint)
            step = np.floor(min(by_gap, by_tip))
            room = (limit - q[J[joint]]) * direction
            if room <= 0:
                return q[J[joint]], self.evaluations - start
            if step < 1:
                return q[J[joint]], self.evaluations - start
            q[J[joint]] += direction * min(step, room)


def main() -> None:
    b = Body()
    pose = reach.STAND.copy()
    grid3 = {"shoulder": (-337, 271), "elbow": (-507, 837), "pitch": (-308, 876)}
    out = {}
    t0 = time.time()
    for joint in J:
        lo, hi = reach.SERVO[J[joint]]
        t = time.time()
        down, n1 = b.march(pose, joint, -1, lo)
        up, n2 = b.march(pose, joint, +1, hi)
        out[joint] = {"low": down - pose[J[joint]], "high": up - pose[J[joint]],
                      "low_deg": round((down - pose[J[joint]]) * 360 / 4096, 1),
                      "high_deg": round((up - pose[J[joint]]) * 360 / 4096, 1),
                      "evaluations": n1 + n2, "ms": round((time.time() - t) * 1000),
                      "grid_3d": grid3.get(joint)}
        print(joint, json.dumps(out[joint]))
    print(f"all five joints: {b.evaluations} evaluations, {time.time() - t0:.2f} s")


if __name__ == "__main__":
    main()
