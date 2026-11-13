# Tests for the default-on marker (11 Nov 16:4x, first builder): a PR that changes the product's defaults (lib.sh's
# COORD_PRODUCT_RE) trains only on a writer's `DEFAULT-ON <sha>` for its head or a same-patch head. markers.py's
# default_on_shas, and train-bot.sh's DRY=1 round and still-eligible.sh over test_lanes' repository and stand-in gh.
# Run from .github/tools/coordination: python3 -m unittest discover -s tests
import json, os, sys, unittest
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from markers import default_on_shas, read_markers
import test_lanes as tl
from test_lanes import HERE, run

HEAD = "e2b57f1f48c0ffee00112233445566778899aabb"
W = {"w1"}
UNIT = "lampOS/deploy/product/lamp-voice.service"


def c(login, body, t="2026-11-11T09:00:00Z"):
    return {"login": login, "body": body, "created_at": t}


class Marker(unittest.TestCase):
    def test_the_first_line_counts(self):
        self.assertEqual(default_on_shas([c("w1", "DEFAULT-ON " + HEAD)], W), [HEAD])

    def test_after_a_signature_it_counts_with_a_long_sha(self):
        self.assertEqual(default_on_shas([c("w1", "First builder, 16:40: DEFAULT-ON " + HEAD[:12] + ", on its row")], W),
                         [HEAD[:12]])

    def test_prose_quotes_and_a_non_writer_count_for_nothing(self):
        for body in ("my DEFAULT-ON is owed", "the first builder's DEFAULT-ON " + HEAD + " was on it",
                     "> DEFAULT-ON " + HEAD, "it opens with `DEFAULT-ON " + HEAD[:10] + "`", "DEFAULT-ON"):
            self.assertEqual(default_on_shas([c("w1", body)], W), [], body)
        self.assertEqual(default_on_shas([c("stranger", "DEFAULT-ON " + HEAD)], W), [])

    def test_it_is_no_read(self):
        # A DEFAULT-ON neither reads nor holds: the PR still needs its READ-OK.
        self.assertEqual(read_markers([c("w1", "DEFAULT-ON " + HEAD)], W), "")


class Gate(tl.Repo):
    # Only BotDryRound's helpers, not its tests (test_lanes runs those).
    bot = tl.BotDryRound.bot
    assertNothingChanged = tl.BotDryRound.assertNothingChanged

    def comments(self, n, *extra):
        h = open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip()
        lines = [{"login": "w1", "body": "READ-OK " + h, "created_at": "2026-11-04T01:00:00Z"}] + list(extra)
        self.fakefile("comments-%d.json" % n, "".join(json.dumps(x) + "\n" for x in lines))
        return h

    def product_pr(self, n=8, marker=None, login="w1"):
        h = self.pr(n, [UNIT])
        self.comments(n, *([{"login": login, "body": "DEFAULT-ON " + (marker or h), "created_at": "2026-11-11T09:00:00Z"}]
                           if marker is not False else []))
        return h

    def still(self, n):
        os.makedirs(self.state, exist_ok=True)
        return run(["zsh", os.path.join(HERE, "still-eligible.sh"), str(n)], self.work, env=self.env, check=False)

    def test_a_product_pr_without_the_marker_is_left_out(self):
        r, lines = self.bot(before=lambda: self.product_pr(marker=False))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#8 left out: it changes the product's defaults, and the first builder's DEFAULT-ON isn't on head", r.stderr)
        self.assertIn("dry: crate lane: would carry 9 onto " + self.base[:9], lines)
        self.assertNothingChanged()
        r = self.still(8)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("#8 changes the product's defaults", r.stdout)

    def test_with_the_marker_on_its_head_it_is_eligible(self):
        r, lines = self.bot(before=lambda: self.product_pr())
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertNotIn("product's defaults", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertEqual(self.still(8).returncode, 0)

    def test_a_marker_on_an_older_head_with_the_same_patch_carries(self):
        def before():
            old = self.pr(8, [UNIT])
            moved = self.gain("lampOS/docs/other.md")
            new = self.pr(8, [UNIT], base=moved)
            self.assertNotEqual(old, new)
            self.comments(8, {"login": "w1", "body": "DEFAULT-ON " + old, "created_at": "2026-11-11T09:00:00Z"})
            self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip())
                                               for n in (7, 8, 9)))
        r, lines = self.bot(before=before)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertNotIn("product's defaults", r.stderr)
        self.assertTrue(any(l.startswith("dry: crate lane: would carry 8") for l in lines), lines)
        self.assertEqual(self.still(8).returncode, 0)

    def test_a_marker_on_another_patch_does_not_carry(self):
        def before():
            old = self.pr(8, [UNIT])
            self.pr(8, [UNIT, "lampOS/skills/x/skill.toml"])
            self.comments(8, {"login": "w1", "body": "DEFAULT-ON " + old, "created_at": "2026-11-11T09:00:00Z"})
            self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip())
                                               for n in (7, 8, 9)))
        r, lines = self.bot(before=before)
        self.assertIn("#8 left out: it changes the product's defaults", r.stderr)

    def test_a_marker_from_a_non_writer_is_ignored(self):
        r, lines = self.bot(before=lambda: self.product_pr(login="stranger"))
        self.assertIn("#8 left out: it changes the product's defaults", r.stderr)
        self.assertEqual(self.still(8).returncode, 1)

    def test_skills_and_install_rs_count_too(self):
        for f in ("lampOS/skills/faq/knowledge.md", "lampOS/crates/update/src/install.rs"):
            def before(f=f):
                self.pr(8, [f]); self.comments(8)
            r, lines = self.bot(before=before)
            self.assertIn("#8 left out: it changes the product's defaults", r.stderr, f)

    def test_a_pr_touching_no_product_file_needs_none(self):
        r, lines = self.bot()
        self.assertNotIn("product's defaults", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.pr(8, ["lampOS/crates/update/src/round.rs"])
        self.assertEqual(self.still(8).returncode, 0)


if __name__ == "__main__":
    unittest.main()
