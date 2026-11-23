"""From A to B by recursive midpoints: when the straight line is blocked, find a
safe point near its middle, nudged off the line if it must be, and solve each
half the same way. Near the presets almost every straight line is certified
(plan.py: 199 of 200 random pairs), so this only works where something is in the
way, and falls back to nothing heavier.

Each edge is certified as plan.py certifies one (checks close enough that no
point moves more than STEP_MM between two, margins raised by half of it).
"""
from __future__ import annotations

import numpy as np

import lattice
import plan

RADII = (48.0, 96.0, 192.0, 384.0)  # counts a nudge goes off the middle, nearest first
TRY_EACH = 3  # nudges tried at each radius, the clearest first


def nudges(ch: plan.Checker, m: np.ndarray) -> list[np.ndarray]:
    """The middle itself, then points around it along the lattice's directions, at
    growing distances; at each distance the ones with the most clearance first."""
    out = [m]
    for r in RADII:
        cand = m[None] + lattice.DIRS * r
        cand = cand[((cand >= plan.LO) & (cand <= plan.HI)).all(axis=1)]
        if not len(cand):
            continue
        gap, off = ch.measure(cand)
        ok = (gap >= ch.margin + plan.STEP_MM / 2) & (off <= plan.reach.CLIP_MM - plan.TIP_STEP_MM / 2)
        order = np.argsort(-(np.minimum(gap - ch.margin, (plan.reach.CLIP_MM - off) * 4)))
        out += [cand[i] for i in order if ok[i]][:TRY_EACH]
    return out


def path(ch: plan.Checker, a: np.ndarray, b: np.ndarray, depth: int = 0, max_depth: int = 6) -> list[np.ndarray] | None:
    if ch.edge(a, b):
        return [a, b]
    if depth >= max_depth:
        return None
    for m in nudges(ch, (a + b) / 2):
        if not ch.safe(m, extra_mm=plan.STEP_MM / 2, extra_tip=plan.TIP_STEP_MM / 2)[0]:
            continue
        left = path(ch, a, m, depth + 1, max_depth)
        if left is None:
            continue
        right = path(ch, m, b, depth + 1, max_depth)
        if right is None:
            continue
        return left[:-1] + right
    return None


def plan_path(ch: plan.Checker, a: np.ndarray, b: np.ndarray) -> list[np.ndarray] | None:
    p = path(ch, np.asarray(a, float), np.asarray(b, float))
    return None if p is None else plan.shortcut(ch, p)
