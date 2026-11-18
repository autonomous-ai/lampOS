"""Moving between presets: a path through the safe poses of the five joints.

A preset is a point in joint space (bus order: yaw, shoulder, elbow, roll, pitch,
counts). Between two presets:

1. the straight line, if it is certified safe;
2. otherwise a roadmap (PRM): the presets and random safe poses, joined by
   certified straight edges, searched with A*, then shortened wherever a
   straight shortcut is certified too.

An edge is certified by checking poses along it close enough together that no
point of the lamp moves more than STEP_MM between two of them (each joint's
lever times its turn, summed), against a margin raised by that much; the centre
of mass the same way, with its own bound. Nothing between two checked poses can
break a rule the checks did not see.

    uv run python plan.py      # plans between the presets; writes plan.json and plan.png
"""
from __future__ import annotations

import heapq
import json
import os
import time

import numpy as np
from scipy.spatial import cKDTree

import body
import march
import reach

STEP_MM = 3.0
TIP_STEP_MM = 0.5
# What a count of each joint costs: the base's turn is the most visible, the arm
# lifts its own weight, roll is unseen (bus order).
WEIGHT = np.array([1.5, 1.2, 1.0, 0.3, 0.8])
LO = np.array([reach.SERVO[j][0] for j in range(5)], float)
HI = np.array([reach.SERVO[j][1] for j in range(5)], float)

S, F = reach.STAND, reach.FOLDED
PRESETS = {
    # Folded, the head rests on its arm (1.5 mm in the CAD, under any margin): no
    # planner may end there. It plans to just above it, the head lifted, and the
    # body's own fold settles the rest, as it does today.
    "pre_fold": F + [0, 80, 80, 0, -250],
    "stand": S,
    "half_up": F + (S - F) * 0.5,
    "look_down": S + [0, 0, 0, 0, 600],
    "look_far_down": S + [0, 0, 0, 0, 840],
    "look_left": S + [-450, 0, 0, 0, 0],
    "look_right_down": S + [450, 0, 0, 0, 450],
    "reach_up": S + [0, 0, 600, 0, -150],
    "low_peek": F + [0, 300, 120, 0, -250],
}


class Checker:
    """Rules at many poses at once (march.py's measure, vectorised), and the bounds
    that space a certified edge's checks."""

    def __init__(self, margin_mm: float = 2.0):
        self.b = march.Body(margin_mm=margin_mm)
        self.margin = margin_mm
        rng = np.random.default_rng(1)
        sample = rng.uniform(LO, HI, size=(400, 5))
        lev = np.array([[m["lever"][j] for j in march.J] for m in map(self.b.measure, sample)])
        # The farthest any point lies from each joint's origin over the sample, with slack.
        self.lever_mm = lev.max(axis=0) * 1.15
        h = 8.0
        rates = []
        for j in range(5):
            a, c = sample.copy(), sample.copy()
            a[:, j] -= h
            c[:, j] += h
            rates.append(np.abs(self.b.lean.off_axis_mm(c) - self.b.lean.off_axis_mm(a)) / (2 * h))
        self.tip_rate = np.max(rates, axis=1) * 1.5  # mm of centre of mass a count, per joint
        self.spheres = {}
        for p, pts in self.b.cad.points.items():
            c = (pts.min(0) + pts.max(0)) / 2
            self.spheres[p] = (c, float(np.linalg.norm(pts - c, axis=1).max()))
        self.checked = 0

    def measure(self, counts: np.ndarray, chunk: int = 300, far_mm: float = 40.0) -> tuple[np.ndarray, np.ndarray]:
        """Least clearance (to another part or the table, mm) and off-axis (mm) per pose.
        A pair whose bounding spheres are more than `far_mm` apart is read as that gap
        without asking its points: far from any margin, so no answer changes."""
        counts = np.asarray(counts, float)
        self.checked += len(counts)
        gap = np.empty(len(counts))
        off = self.b.lean.off_axis_mm(counts)
        cad, trees, sph = self.b.cad, self.b.trees, self.spheres
        for s in range(0, len(counts), chunk):
            c = counts[s:s + chunk]
            w = cad.bone_worlds(body.deg_from_counts(c, reach.HOME))
            fr = {"base": np.broadcast_to(np.eye(4), (len(c), 4, 4))} | w
            g = np.full(len(c), far_mm)
            for a, b in march.PAIRS:
                rel = np.linalg.inv(fr[a]) @ fr[b]
                (ca, ra), (cb, rb) = sph[a], sph[b]
                between = (np.linalg.norm(rel[:, :3, :3] @ cb + rel[:, :3, 3] - ca, axis=1) - ra - rb) * 1000
                near = between < far_mm
                if near.any():
                    r = rel[near]
                    p = np.einsum("nij,kj->nki", r[:, :3, :3], cad.points[b]) + r[:, None, :3, 3]
                    d = trees[a].query(p.reshape(-1, 3), workers=-1)[0].reshape(len(r), -1).min(1) * 1000
                    g[near] = np.minimum(g[near], d)
            for part in ("elbow_pitch", "wrist_pitch", "wrist_roll"):
                m = fr[part]
                y = np.einsum("nj,kj->nk", m[:, 1, :3], cad.points[part]) + m[:, None, 1, 3]
                g = np.minimum(g, y.min(1) * 1000)
            gap[s:s + chunk] = g
        return gap, off

    def safe(self, counts: np.ndarray, extra_mm: float = 0.0, extra_tip: float = 0.0) -> np.ndarray:
        counts = np.atleast_2d(counts)
        inside = ((counts >= LO) & (counts <= HI)).all(axis=1)
        gap, off = self.measure(counts)
        return inside & (gap >= self.margin + extra_mm) & (off <= reach.CLIP_MM - extra_tip)

    def edge(self, a: np.ndarray, b: np.ndarray) -> bool:
        """Is the straight segment a -> b certified safe?"""
        d = np.abs(b - a)
        move_mm = float((d * self.lever_mm).sum() * march.RAD_PER_COUNT)
        tip_mm = float((d * self.tip_rate).sum())
        n = int(np.ceil(max(move_mm / STEP_MM, tip_mm / TIP_STEP_MM, 1)))
        t = np.linspace(0, 1, n + 1)[:, None]
        # Between two checks a point moves at most STEP_MM: each check keeps half of it more.
        return bool(self.safe(a + (b - a) * t, extra_mm=STEP_MM / 2, extra_tip=TIP_STEP_MM / 2).all())


def cost(a: np.ndarray, b: np.ndarray) -> float:
    return float(np.linalg.norm((b - a) * WEIGHT))


def roadmap(ch: Checker, presets: dict[str, np.ndarray], n: int, k: int, seed: int = 3):
    rng = np.random.default_rng(seed)
    # Only around the presets (their box, 120 counts wider), not every servo's span:
    # shorter edges, and the poses a move between presets passes near.
    pts = np.array(list(presets.values()))
    lo, hi = np.maximum(pts.min(0) - 120, LO), np.minimum(pts.max(0) + 120, HI)
    cand = rng.uniform(lo, hi, size=(n * 4, 5))
    ok = ch.safe(cand, extra_mm=STEP_MM / 2, extra_tip=TIP_STEP_MM / 2)
    nodes = np.vstack([np.array(list(presets.values())), cand[ok][:n]])
    tree = cKDTree(nodes * WEIGHT)
    edges: dict[int, list[tuple[int, float]]] = {i: [] for i in range(len(nodes))}
    tried = set()
    for i in range(len(nodes)):
        _, nbrs = tree.query(nodes[i] * WEIGHT, k=k + 1)
        for j in nbrs[1:]:
            key = (min(i, j), max(i, j))
            if key in tried:
                continue
            tried.add(key)
            if ch.edge(nodes[i], nodes[j]):
                c = cost(nodes[i], nodes[j])
                edges[i].append((j, c))
                edges[j].append((i, c))
    return nodes, edges, len(tried)


def astar(nodes, edges, s: int, g: int) -> list[int] | None:
    h = lambda i: cost(nodes[i], nodes[g])
    best = {s: 0.0}
    came = {}
    heap = [(h(s), s)]
    while heap:
        _, i = heapq.heappop(heap)
        if i == g:
            path = [i]
            while i in came:
                i = came[i]
                path.append(i)
            return path[::-1]
        for j, c in edges[i]:
            nc = best[i] + c
            if nc < best.get(j, np.inf):
                best[j] = nc
                came[j] = i
                heapq.heappush(heap, (nc + h(j), j))
    return None


def shortcut(ch: Checker, pts: list[np.ndarray]) -> list[np.ndarray]:
    """Skip waypoints wherever the straight segment past them is certified."""
    out, i = [pts[0]], 0
    while i < len(pts) - 1:
        j = len(pts) - 1
        while j > i + 1 and not ch.edge(pts[i], pts[j]):
            j -= 1
        out.append(pts[j])
        i = j
    return out


def describe(path: list[np.ndarray]) -> list[str]:
    """Each leg as which joints lead (moved most, in their weighted counts)."""
    names = list(march.J)
    legs = []
    for a, b in zip(path, path[1:]):
        d = b - a
        order = np.argsort(-np.abs(d))
        legs.append(", ".join(f"{names[k]} {d[k]:+.0f}" for k in order if abs(d[k]) >= 20))
    return legs


def main() -> None:
    t0 = time.time()
    ch = Checker()
    for name, p in PRESETS.items():
        gap, off = ch.measure(p[None])
        print(flush=True, *[f"preset {name:16} safe={bool(ch.safe(p)[0])} clearance {gap[0]:5.1f} mm off-axis {off[0]:5.1f} mm"])
    queries = [("stand", "pre_fold"), ("pre_fold", "stand"), ("look_far_down", "pre_fold"), ("reach_up", "low_peek"),
               ("look_left", "look_right_down"), ("low_peek", "look_far_down")]
    results = []
    roadmap_built = None
    for a, b in queries:
        pa, pb = PRESETS[a], PRESETS[b]
        entry = {"from": a, "to": b}
        if ch.edge(pa, pb):
            entry.update(how="straight line", path=[pa.tolist(), pb.tolist()], legs=describe([pa, pb]))
        else:
            if roadmap_built is None:
                t = time.time()
                nodes, edges, tried = roadmap(ch, PRESETS, n=300, k=10)
                roadmap_built = (nodes, edges)
                print(f"roadmap: {len(nodes)} nodes, {sum(len(v) for v in edges.values()) // 2} certified edges of {tried} tried, {time.time() - t:.0f} s")
            nodes, edges = roadmap_built
            idx = {n: i for i, n in enumerate(PRESETS)}
            path = astar(nodes, edges, idx[a], idx[b])
            if path is None:
                entry.update(how="no path in the roadmap")
            else:
                pts = shortcut(ch, [nodes[i] for i in path])
                entry.update(how=f"roadmap: {len(path)} nodes, {len(pts)} after shortcuts",
                             path=[p.tolist() for p in pts], legs=describe(pts))
        if "path" in entry:
            dense = np.vstack([np.linspace(np.array(p), np.array(q), 40) for p, q in zip(entry["path"], entry["path"][1:])])
            gap, off = ch.measure(dense)
            entry.update(least_clearance_mm=round(float(gap.min()), 1), most_off_axis_mm=round(float(off.max()), 1),
                         cost=round(sum(cost(np.array(p), np.array(q)) for p, q in zip(entry["path"], entry["path"][1:])), 0))
        results.append(entry)
        print(f"{a} -> {b}: {entry['how']}; " + " | ".join(entry.get("legs", [])) +
              (f"; least clearance {entry['least_clearance_mm']} mm, most off-axis {entry['most_off_axis_mm']} mm" if "path" in entry else ""), flush=True)
    json.dump({"presets": {k: v.tolist() for k, v in PRESETS.items()}, "results": results,
               "poses_checked": ch.checked, "seconds": round(time.time() - t0)},
              open(os.path.join(reach.OUT, "plan.json"), "w"), indent=1)
    print(f"{ch.checked} poses checked, {time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
