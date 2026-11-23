"""The reach map over all five joints, factorised (README.md, "Five joints").

Each rule depends on only some joints, so each is computed as its own table over
just those, and a pose is safe when every table says so:

- tipping: the centre of mass's distance from the base's axis does not change as
  the base turns, so it is a table over shoulder, elbow, roll and pitch;
- two parts not next to each other: their relative place depends only on the
  joints between them in the chain (the shade against the upper arm on pitch and
  roll alone); only a moving part against the fixed base needs all five;
- the table: a flat plane, so the base's turn does not change it.

Tables of up to four joints use the 3D map's grid (32 counts, 2.8 degrees); roll
and base yaw use 64 counts, and the one five-joint table (the shade against the
base) 64 counts on every joint but roll's 128. A bounding-sphere test skips
poses whose two parts are plainly apart.

    uv run python reach5.py [--margin-mm 2]

Writes lampOS/.cache/reach/map5.npz and report5.json.
"""
from __future__ import annotations

import argparse
import itertools
import json
import os
import time

import numpy as np
from scipy.spatial import cKDTree

import body
import reach

# Bus order: yaw, shoulder, elbow, roll, pitch.
J = {"yaw": 0, "shoulder": 1, "elbow": 2, "roll": 3, "pitch": 4}
# Each bone and the joint that turns it, in chain order.
LINK_JOINT = {"base_yaw": "yaw", "base_pitch": "shoulder", "elbow_pitch": "elbow",
              "wrist_pitch": "pitch", "wrist_roll": "roll"}
CHAIN = ["base"] + body.BONES


def axis(name: str, step: int) -> np.ndarray:
    lo, hi = reach.SERVO[J[name]]
    return np.arange(lo, hi + 1, step, dtype=float)


FINE = {"yaw": axis("yaw", 64), "shoulder": axis("shoulder", 32), "elbow": axis("elbow", 32),
        "roll": axis("roll", 64), "pitch": axis("pitch", 32)}
COARSE = {"yaw": axis("yaw", 64), "shoulder": axis("shoulder", 64), "elbow": axis("elbow", 64),
          "roll": axis("roll", 128), "pitch": axis("pitch", 64)}


def joints_between(a: str, b: str) -> list[str]:
    """The joints that move b relative to a (a before b in the chain)."""
    ia, ib = CHAIN.index(a), CHAIN.index(b)
    return [LINK_JOINT[CHAIN[k]] for k in range(ia + 1, ib + 1)]


def grid_counts(names: list[str], axes: dict[str, np.ndarray]) -> tuple[np.ndarray, tuple[int, ...]]:
    """Every pose over the named joints (the others at the stand), bus order."""
    mesh = np.meshgrid(*[axes[n] for n in names], indexing="ij")
    counts = np.tile(reach.STAND, (mesh[0].size, 1))
    for n, m in zip(names, mesh):
        counts[:, J[n]] = m.ravel()
    return counts, mesh[0].shape


def sphere(points: np.ndarray) -> tuple[np.ndarray, float]:
    c = (points.min(0) + points.max(0)) / 2
    return c, float(np.linalg.norm(points - c, axis=1).max())


class Geometry:
    def __init__(self, fine_m: float = 0.004, coarse_m: float = 0.008):
        self.fine = body.Cad(reach.GLB, voxel_m=fine_m)
        self.coarse = body.Cad(reach.GLB, voxel_m=coarse_m)
        self.trees = {p: cKDTree(self.fine.points[p]) for p in CHAIN}
        self.spheres = {p: sphere(self.fine.points[p]) for p in CHAIN}

    def frames(self, counts: np.ndarray) -> dict[str, np.ndarray]:
        w = self.coarse.bone_worlds(body.deg_from_counts(counts, reach.HOME))
        return {"base": np.broadcast_to(np.eye(4), (len(counts), 4, 4))} | w

    def pair(self, a: str, b: str, counts: np.ndarray, margin_mm: float, chunk: int = 20000) -> np.ndarray:
        """Least distance (mm) between parts a and b at each pose; poses whose
        bounding spheres are farther apart than 20 mm read as that gap."""
        out = np.empty(len(counts))
        ca, ra = self.spheres[a]
        cb, rb = self.spheres[b]
        pts_b = self.coarse.points[b]
        for s in range(0, len(counts), chunk):
            f = self.frames(counts[s:s + chunk])
            rel = np.linalg.inv(f[a]) @ f[b]
            centre = rel[:, :3, :3] @ cb + rel[:, :3, 3]
            gap = (np.linalg.norm(centre - ca, axis=1) - ra - rb) * 1000
            near = gap < max(20.0, margin_mm + 10)
            d = gap.copy()
            if near.any():
                r = rel[near]
                p = np.einsum("nij,kj->nki", r[:, :3, :3], pts_b) + r[:, None, :3, 3]
                d[near] = self.trees[a].query(p.reshape(-1, 3), workers=-1)[0].reshape(len(r), -1).min(axis=1) * 1000
            out[s:s + chunk] = d
        return out

    def above_table(self, b: str, counts: np.ndarray, chunk: int = 20000) -> np.ndarray:
        out = np.empty(len(counts))
        cb, rb = self.spheres[b]
        pts_b = self.coarse.points[b]
        for s in range(0, len(counts), chunk):
            m = self.frames(counts[s:s + chunk])[b]
            cy = (m[:, 1, :3] @ cb + m[:, 1, 3] - rb) * 1000
            d = cy.copy()
            near = cy < 30.0
            if near.any():
                r = m[near]
                d[near] = (np.einsum("nj,kj->nk", r[:, 1, :3], pts_b) + r[:, None, 1, 3]).min(axis=1) * 1000
            out[s:s + chunk] = d
        return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--margin-mm", type=float, default=2.0)
    args = ap.parse_args()
    t0 = time.time()
    geo = Geometry()
    lean = body.Lean()
    tables, log = {}, []

    def done(name, dims, shape, value, kind):
        tables[name] = dict(dims=dims, shape=shape, value=value.reshape(shape).astype(np.float32), kind=kind)
        log.append(f"{name}: {dims} {shape} {np.prod(shape)} poses, {time.time() - t0:.0f} s")
        print(log[-1], flush=True)

    # Tipping: yaw left out (it turns the centre of mass about the axis).
    dims = ["shoulder", "elbow", "roll", "pitch"]
    counts, shape = grid_counts(dims, FINE)
    done("tip_off_axis_mm", dims, shape, np.concatenate([lean.off_axis_mm(c) for c in np.array_split(counts, 50)]), "le_22")

    # Every pair of parts not next to each other, over the joints between them.
    for a, b in itertools.combinations(CHAIN, 2):
        if abs(CHAIN.index(a) - CHAIN.index(b)) == 1:
            continue
        dims = [n for n in J if n in joints_between(a, b)]
        five = len(dims) == 5
        counts, shape = grid_counts(dims, COARSE if five else FINE)
        done(f"clear_{a}__{b}", dims, shape, geo.pair(a, b, counts, args.margin_mm), "ge_margin")

    # The table, for each moving part past the swivel, over the joints before it but yaw.
    for b in ("elbow_pitch", "wrist_pitch", "wrist_roll"):
        dims = [n for n in J if n in joints_between("base", b) and n != "yaw"]
        counts, shape = grid_counts(dims, FINE)
        done(f"table_{b}", dims, shape, geo.above_table(b, counts), "ge_margin")

    axes = {"fine": FINE, "coarse": COARSE}
    np.savez_compressed(os.path.join(reach.OUT, "map5.npz"),
                        **{f"{k}": v["value"] for k, v in tables.items()},
                        **{f"axis_{g}_{n}": a for g, ax in axes.items() for n, a in ax.items()})
    meta = {k: {"dims": v["dims"], "shape": list(v["shape"]), "kind": v["kind"],
                "grid": "coarse" if len(v["dims"]) == 5 else "fine"} for k, v in tables.items()}
    json.dump({"margin_mm": args.margin_mm, "tip_mm": reach.CLIP_MM, "tables": meta, "log": log,
               "seconds": round(time.time() - t0)}, open(os.path.join(reach.OUT, "map5.json"), "w"), indent=1)
    print(f"all tables in {time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
