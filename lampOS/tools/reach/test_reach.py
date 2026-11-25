"""Checks for the reach tools (run: python3 -m unittest test_reach, from this folder). No CAD pass: a few seconds."""
import json
import os
import tempfile
import unittest

import numpy as np

import body
import query
import reach


class Lean(unittest.TestCase):
    def test_the_bodys_own_numbers_for_lamp_4ace(self):
        # README.md's table for lamp-4ace's poses (cog.rs's own tests give 17.851 and 1.553 mm at their counts).
        lean = body.Lean()
        off = lean.off_axis_mm(np.array([reach.FOLDED, reach.STAND]))
        self.assertAlmostEqual(off[0], 18.0, delta=0.2)
        self.assertAlmostEqual(off[1], 2.7, delta=0.2)

    def test_heavier_masses_move_the_centre_the_same_way_for_every_pose(self):
        text = json.load(open(body.LEAN_JSON))
        text["base"]["mass_kg"] *= 2
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "lean.json")
            json.dump(text, open(path, "w"))
            heavy = body.Lean(path)
        poses = np.array([reach.FOLDED, reach.STAND])
        self.assertTrue((heavy.off_axis_mm(poses) < body.Lean().off_axis_mm(poses)).all())


class Query(unittest.TestCase):
    axes = {"shoulder": np.array([0.0, 10.0, 20.0]), "elbow": np.array([0.0, 10.0]), "pitch": np.array([0.0, 10.0, 20.0, 30.0])}

    def grid(self, ok):
        why = np.ones((3, 2, 4), np.uint8)
        for (i, j, k) in ok:
            why[i, j, k] = 0
        return {**self.axes, "why": why}

    def test_runs_are_the_maximal_safe_stretches(self):
        self.assertEqual(query.runs(np.array([0.0, 1, 2, 3, 4]), np.array([1, 1, 0, 1, 1], bool)), [(0.0, 1.0), (3.0, 4.0)])
        self.assertEqual(query.runs(np.array([0.0, 1]), np.array([0, 0], bool)), [])

    def test_a_range_is_the_run_that_holds_the_pose_and_an_unsafe_pose_has_none(self):
        grid = self.grid([(1, 0, 1), (1, 0, 2), (1, 0, 3), (0, 0, 2), (2, 0, 2)])
        self.assertEqual(query.range_at(grid, (10, 0, 20)), {"shoulder": (0.0, 20.0), "elbow": (0.0, 0.0), "pitch": (10.0, 30.0)})
        self.assertEqual(query.range_at(grid, (10, 0, 20), shrink=5)["pitch"], (15.0, 25.0))
        self.assertEqual(query.range_at(grid, (10, 10, 20)), {"shoulder": None, "elbow": None, "pitch": None})

    def test_the_export_holds_the_runs_for_each_shoulder_and_elbow(self):
        grid = self.grid([(1, 0, 1), (1, 0, 2)])
        out = query.export(grid, 0.0)
        self.assertEqual(out["safe_pitch_runs"][1][0], [[10.0, 20.0]])
        self.assertEqual(out["safe_pitch_runs"][0][0], [])


class Rules(unittest.TestCase):
    def test_a_refusal_has_its_own_bit_and_the_bits_do_not_overlap(self):
        bits = [reach.SERVO_BIT, reach.TIP_BIT, reach.HIT_BIT, reach.TABLE_BIT]
        self.assertEqual(len(set(bits)), 4)
        self.assertEqual(sum(bits), 15)

    def test_the_head_pitch_sign_is_the_models_own(self):
        self.assertEqual(body.SIGN_FIX, {})
        model = json.load(open(body.MODEL_JSON))
        self.assertEqual(next(j for j in model["joints"] if j["name"] == "pitch")["sign"], -1)


class SharedTable(unittest.TestCase):
    """pose-table.json is the answers every reader of reach-map-4ace.json must give. This is the Python port, written apart from the page guard's."""

    HERE = os.path.dirname(os.path.abspath(__file__))
    table = json.load(open(os.path.join(HERE, "pose-table.json")))
    map = json.load(open(os.path.join(HERE, "..", "..", "sdk", "js", "examples", "twin", "reach-map-4ace.json")))

    @classmethod
    def nearest(cls, axis, value):
        return min(range(len(axis)), key=lambda i: abs(axis[i] - value))

    @classmethod
    def safe_at(cls, pose):
        a = cls.map["axes"]
        pitch = pose["pitch"]
        if pitch < a["pitch"][0] or pitch > a["pitch"][-1]:
            return False
        runs = cls.map["safe_pitch_runs"][cls.nearest(a["shoulder"], pose["shoulder"])][cls.nearest(a["elbow"], pose["elbow"])]
        return any(lo <= pitch <= hi for lo, hi in runs)

    @classmethod
    def safe(cls, pose, shrink):
        around = [pose] + [{**pose, j: pose[j] + sign * shrink} for j in pose if shrink > 0 for sign in (-1, 1)]
        return all(cls.safe_at(p) for p in around)

    @classmethod
    def range_of(cls, pose, joint, shrink):
        if not cls.safe(pose, 0):
            return None
        if joint == "pitch":
            a = cls.map["axes"]
            runs = cls.map["safe_pitch_runs"][cls.nearest(a["shoulder"], pose["shoulder"])][cls.nearest(a["elbow"], pose["elbow"])]
            low, high = next((lo, hi) for lo, hi in runs if lo <= pose["pitch"] <= hi)
        else:
            axis = cls.map["axes"][joint]
            at = lambda i: cls.safe_at({**pose, joint: axis[i]})
            i = cls.nearest(axis, pose[joint])
            if not at(i):
                return [pose[joint], pose[joint]]
            lo = hi = i
            while lo > 0 and at(lo - 1):
                lo -= 1
            while hi < len(axis) - 1 and at(hi + 1):
                hi += 1
            low, high = min(axis[lo], pose[joint]), max(axis[hi], pose[joint])
        return [min(pose[joint], low + shrink), max(pose[joint], high - shrink)]

    # The page guard's restrict, ported on its own terms (degrees in, degrees out) for the table's restrict cases.
    DEG = 360 / 4096
    FOLDED = {"shoulder": 2048, "elbow": 2047}
    HEAD_HOME = 1772

    @classmethod
    def counts(cls, deg):
        return {"shoulder": cls.FOLDED["shoulder"] + deg["shoulder"] / cls.DEG, "elbow": cls.FOLDED["elbow"] + deg["elbow"] / cls.DEG,
                "pitch": cls.HEAD_HOME - deg["pitch"] / cls.DEG}

    @classmethod
    def deg_of(cls, joint, c):
        return (cls.HEAD_HOME - c) * cls.DEG if joint == "pitch" else (c - cls.FOLDED[joint]) * cls.DEG

    @classmethod
    def line_distance(cls, pose, joint):
        if cls.safe(pose, 0):
            return 0
        a = cls.map["axes"]
        if joint == "pitch":
            runs = cls.map["safe_pitch_runs"][cls.nearest(a["shoulder"], pose["shoulder"])][cls.nearest(a["elbow"], pose["elbow"])]
            return min([float("inf")] + [(lo - pose["pitch"]) if pose["pitch"] < lo else pose["pitch"] - hi for lo, hi in runs])
        return min([float("inf")] + [abs(v - pose[joint]) for v in a[joint] if cls.safe({**pose, joint: v}, 0)])

    @classmethod
    def distance(cls, pose):
        if cls.safe(pose, 0):
            return 0
        a = cls.map["axes"]
        best = float("inf")
        for i, sh in enumerate(a["shoulder"]):
            for j, el in enumerate(a["elbow"]):
                base = max(abs(sh - pose["shoulder"]), abs(el - pose["elbow"]))
                if base >= best:
                    continue
                for lo, hi in cls.map["safe_pitch_runs"][i][j]:
                    best = min(best, max(base, lo - pose["pitch"] if pose["pitch"] < lo else pose["pitch"] - hi if pose["pitch"] > hi else 0))
        return best

    @classmethod
    def restrict(cls, joints, joint, deg, shrink=48):
        if joint not in ("shoulder", "elbow", "pitch"):
            return deg, None
        here = {n: joints[n]["deg"] for n in ("shoulder", "elbow", "pitch")}
        goals = {n: joints[n].get("goal_deg", joints[n]["deg"]) for n in here}
        now, held, goal = cls.counts(here), cls.counts({**goals, joint: here[joint]}), cls.counts({**goals, joint: deg})
        if cls.safe(held, 0):
            lo, hi = cls.range_of(held, joint, shrink)
            a, b = sorted((cls.deg_of(joint, lo), cls.deg_of(joint, hi)))
            if a <= deg <= b:
                return deg, None
            import math
            return (math.floor(b * 10) / 10 if deg > b else math.ceil(a * 10) / 10), "held_at_edge"
        fg, fh = cls.line_distance(goal, joint), cls.line_distance({**held, joint: now[joint]}, joint)
        closer = fg <= fh if fg != float("inf") and fh != float("inf") else cls.distance(goal) <= cls.distance(now)
        return (deg, None) if closer else (round(here[joint] * 10) / 10, "refused_further")

    def test_the_port_gives_the_tables_restrict_answers(self):
        for c in self.table["restrict_cases"]:
            joints = {n: {"deg": v} for n, v in c["pose_deg"].items()}
            got, kind = self.restrict(joints, c["joint"], c["goal_deg"])
            self.assertAlmostEqual(got, c["deg"], places=6, msg=c["name"])
            self.assertEqual(kind, c["kind"], c["name"])
        for c in self.table["restrict_all_cases"]:
            asked = {n: {"deg": v, "goal_deg": c["targets"].get(n, v)} for n, v in c["pose_deg"].items()}
            out, notes = {}, []
            for joint, deg in c["targets"].items():
                got, kind = self.restrict(asked, joint, deg) if joint in asked else (deg, None)
                out[joint] = got
                if kind:
                    notes.append(kind)
                if joint in asked:
                    asked[joint] = {**asked[joint], "goal_deg": got}
            self.assertEqual(notes, c["notes"], c["name"])
            for k, v in c["out"].items():
                self.assertAlmostEqual(out[k], v, places=6, msg=c["name"])

    def test_a_pose_at_the_runs_edge_is_safe_and_not_safe_with_the_margin(self):
        edge = next(c for c in self.table["cases"] if c["name"] == "stand_head_at_edge")
        self.assertTrue(self.safe(edge["pose"], 0))
        self.assertFalse(self.safe(edge["pose"], 48))
        self.assertTrue(edge["safe"] and not edge["safe_with_margin_48"])

    def test_the_port_gives_the_tables_answers(self):
        self.assertGreaterEqual(len(self.table["cases"]), 12)
        for c in self.table["cases"]:
            self.assertEqual(self.safe(c["pose"], 0), c["safe"], c["name"])
            self.assertEqual(self.safe(c["pose"], self.table["margin_counts"]), c["safe_with_margin_48"], c["name"])
            for joint in ("shoulder", "elbow", "pitch"):
                for shrink in (0, 48):
                    got = self.range_of(c["pose"], joint, shrink)
                    want = c[f"ranges_shrink_{shrink}"][joint]
                    if want is None:
                        self.assertIsNone(got, f"{c['name']} {joint} {shrink}")
                    else:
                        self.assertEqual([round(v, 3) for v in got], want, f"{c['name']} {joint} {shrink}")

    def test_a_per_lamp_export_names_its_lamp_and_lays_its_counts_on_the_maps(self):
        axes = {"shoulder": np.array([0.0, 10, 20]), "elbow": np.array([0.0, 10]), "pitch": np.array([0.0, 10, 20, 30])}
        grid = {**axes, "why": np.zeros((3, 2, 4), np.uint8)}
        out = query.export(grid, 48.0, "label", "lamp-75c9", (2076, 2045), 1772)
        self.assertEqual(out["unit"], "lamp-75c9")
        self.assertEqual(out["frame"]["offset"], {"shoulder": 28.0, "elbow": -2.0, "pitch": 0.0})
        self.assertTrue(query.for_unit(out, "lamp-75c9"))
        self.assertFalse(query.for_unit(out, "lamp-4ace"))
        self.assertFalse(query.for_unit(query.export(grid, 48.0), "lamp-75c9"))
        for f in self.table["frame_cases"]:
            frame = query.frame_of(f["unit"], tuple(f["folded"]), f["head_home"])
            self.assertEqual(query.to_map_counts(frame, f["lamp"]), {k: float(v) for k, v in f["map"].items()}, f["unit"])
        self.assertEqual(query.to_map_counts(None, {"shoulder": 1.0, "elbow": 2.0, "pitch": 3.0}), {"shoulder": 1.0, "elbow": 2.0, "pitch": 3.0})
        with self.assertRaises(ValueError):
            query.export(grid, 0.0, "", "lamp-75c9")


if __name__ == "__main__":
    unittest.main()
