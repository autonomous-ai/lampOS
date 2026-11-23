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


if __name__ == "__main__":
    unittest.main()
