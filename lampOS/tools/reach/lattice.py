"""Moving between presets on a lattice: A* over fixed directions, each step a jump.

From a node the search tries a fixed set of directions in joint space (plan.py's
alternative to its random roadmap): +-1 on each joint, and the diagonals the lamp
moves along, the fold (shoulder and elbow together) and the head keeping its aim
as the elbow turns (elbow with pitch). Along each direction it jumps STEP counts
at a time, up to K_MAX steps while every node on the way is safe, and adds the
first and the farthest as successors.

A node is checked with its margin raised by the farthest any point of the lamp
can move in one step of that direction (each joint's lever times its turn), and
its centre of mass the same way, so the step from a checked node to the next is
safe by construction; no samples between nodes are needed.

    uv run python lattice.py     # the same transitions as plan.py; writes lattice.json
"""
from __future__ import annotations

import heapq
import json
import os
import time

import numpy as np

import march
import plan
import reach

STEP = 16.0  # counts a step on every joint (1.4 degrees)
K_MAX = 8
DIRS = []
for j in range(5):
    for s in (1, -1):
        d = np.zeros(5)
        d[j] = s
        DIRS.append(d)
for s in (1, -1):
    DIRS.append(np.array([0, s, s, 0, 0], float))          # the fold
    DIRS.append(np.array([0, 0, s, 0, s], float))          # elbow and pitch together
    DIRS.append(np.array([0, 0, s, 0, -s], float))         # elbow against pitch
DIRS = np.array(DIRS)


class Lattice:
    def __init__(self, ch: plan.Checker):
        self.ch = ch
        # Per direction: how far a point can move, and the centre of mass, in one step.
        self.move_mm = (np.abs(DIRS) * STEP * ch.lever_mm).sum(axis=1) * march.RAD_PER_COUNT
        self.tip_mm = (np.abs(DIRS) * STEP * ch.tip_rate).sum(axis=1)
        self.cache: dict[tuple, tuple[float, float]] = {}
        self.expanded = 0

    def key(self, q):
        return tuple(np.rint(q).astype(int))

    def measure(self, qs: np.ndarray):
        new = [q for q in qs if self.key(q) not in self.cache]
        if new:
            gap, off = self.ch.measure(np.array(new))
            for q, g, o in zip(new, gap, off):
                self.cache[self.key(q)] = (g, o)
        return np.array([self.cache[self.key(q)] for q in qs])

    def ok(self, qs: np.ndarray, d: int) -> np.ndarray:
        inside = ((qs >= plan.LO) & (qs <= plan.HI)).all(axis=1)
        m = self.measure(qs)
        return inside & (m[:, 0] >= self.ch.margin + self.move_mm[d]) & (m[:, 1] <= reach.CLIP_MM - self.tip_mm[d])

    def successors(self, q: np.ndarray):
        """Along each direction, the longest jump of K_MAX, 4, 2 or 1 steps whose
        straight edge is certified (plan.Checker.edge: checks close enough that no
        point moves more than STEP_MM between two), so a narrow passage a coarse
        node check would close stays open."""
        out = []
        for u in DIRS:
            for k in (K_MAX, 4, 2, 1):
                t = q + u * STEP * k
                if ((t >= plan.LO) & (t <= plan.HI)).all() and self.ch.edge(q, t):
                    out.append(t)
                    break
        return out

    def search(self, a: np.ndarray, b: np.ndarray, max_expand: int = 400, max_s: float = 120.0):
        """A* from a to b: lattice nodes about a; the goal joined by a certified
        straight edge (plan.Checker.edge) once within reach. Gives up past
        `max_expand` nodes or `max_s` seconds."""
        t0 = time.time()
        start, goal = self.key(a), b
        best = {start: 0.0}
        came: dict[tuple, tuple] = {}
        heap = [(plan.cost(a, b), start)]
        closed = set()
        while heap and self.expanded < max_expand and time.time() - t0 < max_s:
            _, k = heapq.heappop(heap)
            if k in closed:
                continue
            closed.add(k)
            q = np.array(k, float)
            self.expanded += 1
            near = np.abs(goal - q).max() <= STEP * K_MAX
            if near and self.ch.edge(q, goal):
                path = [goal, q]
                while k in came:
                    k = came[k]
                    path.append(np.array(k, float))
                return path[::-1]
            for s in self.successors(q):
                ks = self.key(s)
                nc = best[k] + plan.cost(q, s)
                if nc < best.get(ks, np.inf):
                    best[ks] = nc
                    came[ks] = k
                    heapq.heappush(heap, (nc + plan.cost(s, goal), ks))
        return None


def main() -> None:
    t0 = time.time()
    ch = plan.Checker()
    queries = [("stand", "pre_fold"), ("pre_fold", "stand"), ("look_far_down", "pre_fold"), ("reach_up", "low_peek"),
               ("look_left", "look_right_down"), ("low_peek", "look_far_down")]
    results = []
    for a, b in queries:
        pa, pb = plan.PRESETS[a], plan.PRESETS[b]
        lat = Lattice(ch)
        before = ch.checked
        t = time.time()
        if ch.edge(pa, pb):
            path, how = [pa, pb], "straight line"
        else:
            path = lat.search(pa, pb)
            how = "no path found" if path is None else f"lattice A*: {len(path)} nodes, {lat.expanded} expanded"
            if path is not None:
                path = plan.shortcut(ch, path)
                how += f", {len(path)} after shortcuts"
        entry = {"from": a, "to": b, "how": how, "seconds": round(time.time() - t, 1), "poses_checked": ch.checked - before}
        if path is not None:
            dense = np.vstack([np.linspace(p, q, 40) for p, q in zip(path, path[1:])])
            gap, off = ch.measure(dense)
            entry.update(path=[p.tolist() for p in path], legs=plan.describe(path),
                         least_clearance_mm=round(float(gap.min()), 1), most_off_axis_mm=round(float(off.max()), 1),
                         cost=round(sum(plan.cost(p, q) for p, q in zip(path, path[1:])), 0))
        results.append(entry)
        print(f"{a} -> {b}: {how} ({entry['seconds']} s, {entry['poses_checked']} poses); " + " | ".join(entry.get("legs", [])) +
              (f"; least clearance {entry['least_clearance_mm']} mm, most off-axis {entry['most_off_axis_mm']} mm, cost {entry['cost']}" if path is not None else ""))
    json.dump({"step": STEP, "k_max": K_MAX, "directions": DIRS.tolist(), "results": results,
               "seconds": round(time.time() - t0)}, open(os.path.join(reach.OUT, "lattice.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
