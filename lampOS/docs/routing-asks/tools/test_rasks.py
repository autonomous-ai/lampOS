import json, os, tempfile, unicodedata, unittest
import rasks

POLICY = "".join(f"## {r} x\nSource: docs/tasks.md:1953\n\n"
                 for r in list(rasks.RULES) + ["P6"]) + (
    "## Order of application\n\n## Scoring contract\n\n## Known weaknesses\n")
VI = "Mấy giờ rồi nhỉ?"

def v2():
    return [{"id": i, "ask": f"ask {i}", "expect": "answer", "kind": "k", "why": "w"}
            for i in range(1, 101)]

def v3():
    return [dict(c, rule="P1") for c in v2()]

def codes(errors):
    return {e.split(": ")[1] for e in errors}

class CheckTest(unittest.TestCase):
    def run_check(self, cases, twins=()):
        return rasks.check(cases, v2(), POLICY, [list(t) for t in twins])

    def test_check_passes_minimal_valid(self):
        self.assertEqual(self.run_check(v3()), [])

    def test_check_flags_changed_ask(self):
        for bad in ("ask 7 ", unicodedata.normalize("NFD", VI)):
            base, cases = v2(), v3()
            base[6]["ask"] = VI; cases[6]["ask"] = bad
            errs = rasks.check(cases, base, POLICY, [])
            self.assertEqual(codes(errs), {"ask-changed"}); self.assertTrue(errs[0].startswith("7: "))

    def test_check_flags_v2_schema_breaks(self):
        for mutate, code in [
            (lambda c: c.pop("why"), "v2-field"),
            (lambda c: c.update(kind="other"), "v2-field"),
            (lambda c: c.update(lang="en"), "type"),
            (lambda c: c.update(rule="P2", acts=["light"]), "type"),
            (lambda c: c.update(doors=["ask"]), "type"),
            (lambda c: c.update(rule="P2", acts="alarm"), "type"),
        ]:
            cases = v3(); mutate(cases[0])
            self.assertIn(code, codes(self.run_check(cases)))

    def test_check_flags_rule_mismatch(self):
        for patch in [
            {"rule": "P9"}, {"rule": "P2"}, {"rule": "P1", "acts": "light"},
            {"rule": "P3", "expect": "hand_over", "doors": ["task"]},
            {"rule": "P7b", "expect": "hand_over", "doors": ["answer", "ask"]},
        ]:
            cases = v3(); cases[0].update(patch)
            self.assertIn("rule", codes(self.run_check(cases)))
        ok = v3(); ok[0].update(rule="P7b", expect="hand_over", doors=["task", "answer"])
        self.assertEqual(self.run_check(ok), [])

    def test_check_flags_ids_and_new(self):
        self.assertIn("ids", codes(self.run_check(v3()[:-1])))
        extra = {"id": 101, "ask": "x", "expect": "answer", "kind": "k", "why": "w", "rule": "P8"}
        self.assertIn("new", codes(self.run_check(v3() + [extra])))
        self.assertEqual(self.run_check(v3() + [dict(extra, new=True)]), [])
        old = v3(); old[0]["new"] = True
        self.assertIn("new", codes(self.run_check(old)))

    def test_check_flags_non_nfc_new_ask(self):
        extra = {"id": 101, "ask": unicodedata.normalize("NFD", VI), "lang": "vi",
                 "expect": "answer", "kind": "k", "why": "w", "rule": "P1", "new": True}
        self.assertEqual(codes(self.run_check(v3() + [extra])), {"nfc"})

    def test_check_flags_bad_pre(self):
        for pre in (["glasses"], ["after:50"], ["after:999"]):
            cases = v3(); cases[18]["pre"] = pre      # id 19
            self.assertIn("pre", codes(self.run_check(cases)))
        cases = v3(); cases[18]["pre"] = ["face", "after:17"]
        self.assertEqual(self.run_check(cases), [])

    def test_check_allows_p8_on_any_id(self):
        cases = v3(); cases[0]["rule"] = "P8"
        self.assertEqual(self.run_check(cases), [])

    def test_check_flags_twins_differing_in_acts(self):
        cases = v3()
        cases[14].update(rule="P2", acts="volume")   # id 15
        cases[15].update(rule="P2", acts="light")    # id 16
        self.assertIn("twins", codes(self.run_check(cases, twins=[(15, 16)])))

    def test_check_reports_uncomparable_twin_group(self):
        cases = v3()
        cases[82].update(rule="P3", expect="hand_over")   # id 83, no doors
        errs = self.run_check(cases, twins=[(83, 84)])
        self.assertTrue(any(e.startswith("83: twins:") for e in errs), errs)

    def test_check_flags_twins_diverging(self):
        cases = v3(); cases[82].update(rule="P7a")    # id 83, twin 84 still P1
        self.assertIn("twins", codes(self.run_check(cases, twins=[(83, 84)])))
        self.assertEqual(self.run_check(v3(), twins=[(83, 84)]), [])

    def test_check_flags_untraceable_policy(self):
        for text in (POLICY.replace("## P4b x", "## Q x"),
                     POLICY.replace("## Order of application", "## Order"),
                     POLICY.replace("## Scoring contract", "## Scoring"),
                     POLICY.replace("## Known weaknesses", "## Weak"),
                     POLICY.replace("## P8 x\nSource: docs/tasks.md:1953", "## P8 x\nNew ruling, awaiting owner")):
            self.assertIn("policy", codes(rasks.check(v3(), v2(), text, [])))

    def test_check_allows_relabel_to_drop_doors_and_acts(self):
        base, cases = v2(), v3()
        base[0].update(expect="hand_over", doors=["task"])   # v2 handed over, v3 answers
        base[1].update(acts="light")                         # v2 named a tool, v3 needs none
        self.assertEqual(rasks.check(cases, base, POLICY, []), [])

    def test_check_accepts_provisional_ruling(self):
        text = POLICY.replace("## P8 x\nSource: docs/tasks.md:1953",
                              "## P8 x\nNew ruling, provisional (lamp team to confirm): why")
        self.assertEqual(rasks.check(v3(), v2(), text, []), [])

class IoTest(unittest.TestCase):
    def test_save_keeps_vietnamese_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "x.json")
            rasks.save(p, [{"id": 2, "ask": VI}], about="a")
            with open(p, "rb") as f:
                raw = f.read()
            self.assertIn(VI.encode("utf-8"), raw); self.assertNotIn(b"\\u", raw)
            self.assertTrue(raw.endswith(b"}\n"))
            self.assertEqual(json.loads(raw)["about"], "a")
            self.assertEqual(rasks.load(p), [{"id": 2, "ask": VI}])
            rasks.save(p, [{"id": 2, "ask": VI}])
            self.assertEqual(rasks.load(p), [{"id": 2, "ask": VI}])

    def test_blind_strips_labels_and_hides_ids(self):
        cases = v3()
        cases[1].update(lang="vi", ask=VI)
        cases.append({"id": 101, "ask": "x", "expect": "hand_over", "doors": ["task"], "kind": "k",
                      "why": "w", "rule": "P4", "new": True, "pre": ["face"], "acts": "light"})
        rows, key = rasks.blind(cases, seed=1)
        self.assertTrue(all(set(r) <= {"id", "ask", "lang"} for r in rows))
        self.assertEqual([r["id"] for r in rows], list(range(1, 102)))
        self.assertEqual(sorted(key.values()), list(range(1, 102)))
        self.assertNotEqual([key[str(r["id"])] for r in rows], list(range(1, 102)))
        by_real = {key[str(r["id"])]: r for r in rows}
        self.assertEqual(by_real[2], {"id": by_real[2]["id"], "ask": VI, "lang": "vi"})
        self.assertEqual(by_real[101], {"id": by_real[101]["id"], "ask": "x"})
        self.assertEqual(rasks.blind(cases, seed=1), (rows, key))
        self.assertNotEqual(rasks.blind(cases, seed=2)[1], key)

    def test_blind_sample_draws_n(self):
        rows, key = rasks.blind(v3(), seed=3, n=20)
        self.assertEqual([r["id"] for r in rows], list(range(1, 21)))
        self.assertEqual(len(set(key.values())), 20)
        self.assertEqual(rasks.blind(v3(), seed=3, n=20), (rows, key))
        with self.assertRaises(ValueError): rasks.blind(v3(), seed=3, n=101)

def lab(i, expect="answer", doors=None, acts=None, rule="P1"):
    r = {"id": i, "expect": expect, "rule": rule, "confidence": "high"}
    if doors: r["doors"] = doors
    if acts: r["acts"] = acts
    return r

class ReviewTest(unittest.TestCase):
    def test_unblind_maps_ids(self):
        out = rasks.unblind([lab(1), lab(2)], {"1": 9, "2": 4})
        self.assertEqual([r["id"] for r in out], [4, 9])
        with self.assertRaises(ValueError): rasks.unblind([lab(3)], {"1": 9})
        with self.assertRaises(ValueError): rasks.unblind([lab(1), lab(1)], {"1": 9})

    def test_diff3_verdicts(self):
        base = [
            {"id": 1, "ask": "a", "expect": "answer"},
            {"id": 2, "ask": "b", "expect": "hand_over", "doors": ["answer", "ask"]},
            {"id": 3, "ask": "c", "expect": "hand_over", "doors": ["ask"]},
            {"id": 4, "ask": "d", "expect": "answer", "acts": "volume"},
            {"id": 5, "ask": "e", "expect": "answer"},
        ]
        a = [lab(1), lab(2, "answer", rule="P7a"), lab(3, "hand_over", ["ask"], rule="P3"),
             lab(4, acts="volume", rule="P2"), lab(5, acts="time", rule="P2")]
        b = [lab(1), lab(2, "answer", rule="P7a"), lab(3, "hand_over", ["task", "ask"], rule="P4b"),
             lab(4, acts="light", rule="P2"), lab(5, acts="time", rule="P2")]
        rows = rasks.diff3(base, a, b)
        self.assertEqual([r["verdict"] for r in rows], ["keep", "change", "decide", "decide", "keep"])
        self.assertEqual(rows[1]["v2"]["doors"], ["answer", "ask"])
        self.assertEqual(rows[1]["a"]["doors"], ["answer"])
        with self.assertRaises(ValueError): rasks.diff3(base, a[:-1], b)

    def test_agree_rates_and_misses(self):
        spec = [{"id": 1, "expect": "answer"},
                {"id": 2, "expect": "hand_over", "doors": ["ask", "task"]},
                {"id": 3, "expect": "hand_over", "doors": ["ask"]},
                {"id": 4, "expect": "hand_over", "doors": ["answer", "task"]}]
        rev = [lab(1), lab(2, "hand_over", ["task"]), lab(3, "hand_over", ["task"]), lab(4)]
        got = rasks.agree(spec, rev)
        self.assertEqual(got, {"n": 4, "expect_rate": 0.75, "door_rate": 0.75,
                               "misses": {"expect": [4], "door": [3]}})
        self.assertEqual(rasks.agree(spec, rev[:2])["n"], 2)
        self.assertEqual(rasks.agree(spec, rev[:2])["expect_rate"], 1.0)
        with self.assertRaises(ValueError): rasks.agree(spec, [lab(9)])
        with self.assertRaises(ValueError): rasks.agree(spec, [])

    def test_agree_rejects_hand_over_without_doors(self):
        spec = [{"id": 1, "expect": "hand_over", "doors": ["ask"]}]
        with self.assertRaisesRegex(ValueError, "1"):
            rasks.agree(spec, [lab(1, "hand_over")])


class CliTest(unittest.TestCase):
    def agree_exit(self, spec, reviews, partial=False):
        with tempfile.TemporaryDirectory() as d:
            v3p = os.path.join(d, "v3.json"); rasks.save(v3p, spec)
            keyp = os.path.join(d, "key.json")
            with open(keyp, "w") as f:
                json.dump({str(c["id"]): c["id"] for c in spec}, f)
            paths = []
            for n, rows in enumerate(reviews):
                p = os.path.join(d, f"r{n}.json"); rasks.save(p, rows); paths.append(p)
            argv = ["agree", v3p, *paths, "--key", keyp] + (["--partial"] if partial else [])
            with open(os.devnull, "w") as null:
                import contextlib
                with contextlib.redirect_stdout(null):
                    return rasks.main(argv)

    def spec(self):
        return [{"id": i, "ask": f"a{i}", "expect": "answer"} for i in range(1, 21)]

    def test_agree_cli_passes_full_agreement(self):
        rows = [lab(i) for i in range(1, 21)]
        self.assertEqual(self.agree_exit(self.spec(), [rows, rows]), 0)

    def test_agree_cli_fails_below_threshold(self):
        good = [lab(i) for i in range(1, 21)]
        bad = [lab(i, "hand_over", ["ask"]) if i <= 2 else lab(i) for i in range(1, 21)]   # 90%
        self.assertEqual(self.agree_exit(self.spec(), [good, bad]), 1)

    def test_agree_cli_fails_when_both_miss_a_case(self):
        one = [lab(1, "hand_over", ["ask"])] + [lab(i) for i in range(2, 21)]   # 95%, at the bar
        self.assertEqual(self.agree_exit(self.spec(), [one, one]), 1)

    def test_agree_cli_partial_allows_a_sample(self):
        sample = [lab(i) for i in range(1, 6)]
        self.assertEqual(self.agree_exit(self.spec(), [sample]), 1)
        self.assertEqual(self.agree_exit(self.spec(), [sample], partial=True), 0)
