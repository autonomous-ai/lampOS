"""Asking the five-joint map (reach5.py's tables) about poses, and its report.

    uv run python query5.py            # writes lampOS/.cache/reach/report5.json

`Map5.safe(counts)` answers for any (N, 5) bus-order counts (yaw, shoulder,
elbow, roll, pitch): each table is read at its nearest grid point, and with
`careful=True` at the worst of the grid points around the pose on every axis it
uses, so a pose between grid points is refused if any neighbour is.
"""
from __future__ import annotations

import itertools
import json
import os

import numpy as np

import reach

J = {"yaw": 0, "shoulder": 1, "elbow": 2, "roll": 3, "pitch": 4}
TIP_BIT, HIT_BIT, TABLE_BIT, SERVO_BIT = reach.TIP_BIT, reach.HIT_BIT, reach.TABLE_BIT, reach.SERVO_BIT


class Map5:
    def __init__(self, folder: str = reach.OUT):
        meta = json.load(open(os.path.join(folder, "map5.json")))
        z = np.load(os.path.join(folder, "map5.npz"))
        self.margin = meta["margin_mm"]
        self.tables = []
        for name, m in meta["tables"].items():
            axes = [z[f"axis_{m['grid']}_{d}"] for d in m["dims"]]
            self.tables.append((name, m["dims"], axes, z[name], m["kind"]))

    @staticmethod
    def _index(axis: np.ndarray, v: np.ndarray, careful: bool) -> list[np.ndarray]:
        step = axis[1] - axis[0]
        x = np.clip((v - axis[0]) / step, 0, len(axis) - 1)
        if not careful:
            return [np.rint(x).astype(int)]
        return [np.floor(x).astype(int), np.ceil(x).astype(int)]

    def judge(self, counts: np.ndarray, careful: bool = False) -> np.ndarray:
        """A refusal code per pose: 0 safe, else the bits of the rules broken."""
        counts = np.asarray(counts, float)
        why = np.zeros(len(counts), np.uint8)
        for j, (lo, hi) in reach.SERVO.items():
            why |= np.where((counts[:, j] < lo) | (counts[:, j] > hi), SERVO_BIT, 0).astype(np.uint8)
        for name, dims, axes, value, kind in self.tables:
            choices = [self._index(a, counts[:, J[d]], careful) for d, a in zip(dims, axes)]
            worst = None
            for combo in itertools.product(*choices):
                v = value[tuple(combo)]
                if kind == "le_22":
                    v = -v  # larger is worse; flip so the minimum is the worst
                worst = v if worst is None else np.minimum(worst, v)
            if kind == "le_22":
                why |= np.where(-worst > reach.CLIP_MM, TIP_BIT, 0).astype(np.uint8)
            else:
                bit = TABLE_BIT if name.startswith("table_") else HIT_BIT
                why |= np.where(worst < self.margin, bit, 0).astype(np.uint8)
        return why

    def safe(self, counts: np.ndarray, careful: bool = False) -> np.ndarray:
        return self.judge(counts, careful) == 0


def joint_range(m: Map5, pose: np.ndarray, joint: str, step: float, careful: bool) -> tuple[float, float] | None:
    """The safe run of one joint around `pose`, the others held, in counts from it."""
    lo, hi = reach.SERVO[J[joint]]
    vals = np.arange(lo, hi + 1, step, dtype=float)
    c = np.tile(pose, (len(vals), 1))
    c[:, J[joint]] = vals
    ok = m.safe(c, careful)
    iv = reach.interval(vals, ok, pose[J[joint]])
    return None if iv is None else (iv[0] - pose[J[joint]], iv[1] - pose[J[joint]])


def main() -> None:
    m = Map5()
    report = {"margin_mm": m.margin, "poses": {}}
    # Over the coarse five-joint grid.
    z = np.load(os.path.join(reach.OUT, "map5.npz"))
    axes = [z[f"axis_coarse_{d}"] for d in J]
    shape = tuple(len(a) for a in axes)
    total, codes = 0, np.zeros(16, np.int64)
    for i, yaw in enumerate(axes[0]):
        mesh = np.meshgrid(*axes[1:], indexing="ij")
        c = np.column_stack([np.full(mesh[0].size, yaw)] + [g.ravel() for g in mesh])
        why = m.judge(c)
        codes += np.bincount(why, minlength=16)
        total += len(c)
    report["grid5"] = {"shape": shape, "poses": int(total), "share_safe": float(codes[0] / total),
                       "refused_by": {name: float(sum(codes[k] for k in range(16) if k & bit) / total)
                                      for name, bit in (("tip", TIP_BIT), ("self_collision", HIT_BIT), ("table", TABLE_BIT))}}
    # Each joint's range at the named poses, every joint now.
    named = {"stand": reach.STAND, "half_up": reach.FOLDED + (reach.STAND - reach.FOLDED) * 0.5}
    for name, pose in named.items():
        entry = {"counts": pose.round().tolist(), "safe": bool(m.safe(pose[None])[0]),
                 "safe_careful": bool(m.safe(pose[None], careful=True)[0])}
        for joint in J:
            step = 16.0
            iv = joint_range(m, pose, joint, step, careful=True)
            entry[joint] = None if iv is None else {"low": iv[0], "high": iv[1],
                                                    "low_deg": round(iv[0] * 360 / 4096, 1), "high_deg": round(iv[1] * 360 / 4096, 1)}
        report["poses"][name] = entry
    # Against the 3D map, where they overlap (yaw and roll at the stand's).
    z3 = np.load(os.path.join(reach.OUT, "map.npz"))
    S, E, P = np.meshgrid(z3["shoulder"], z3["elbow"], z3["pitch"], indexing="ij")
    c = np.tile(reach.STAND, (S.size, 1))
    c[:, 1], c[:, 2], c[:, 4] = S.ravel(), E.ravel(), P.ravel()
    ok5 = m.safe(c)
    ok3 = (z3["why"] == 0).ravel()
    report["vs_3d"] = {"poses": int(len(c)), "agree": float((ok5 == ok3).mean()),
                       "safe_in_3d_only": float((ok3 & ~ok5).mean()), "safe_in_5d_only": float((ok5 & ~ok3).mean())}
    json.dump(report, open(os.path.join(reach.OUT, "report5.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in report.items() if k != "poses"}, indent=1))
    for name, e in report["poses"].items():
        print(name, "safe" if e["safe"] else "REFUSED", {j: None if e[j] is None else (e[j]["low_deg"], e[j]["high_deg"]) for j in J})


if __name__ == "__main__":
    main()
