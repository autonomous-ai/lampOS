# Tests for markers.py, reads2.py and qholds.py: plain python3, no dependencies.
# Run from .github/tools/coordination: python3 tests/test_markers.py
import json, os, subprocess, sys, tempfile, unittest
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from markers import read_markers, parse_marker, signer
from reads2 import two_reads
from qholds import standing

HEAD = "e2b57f1f48c0ffee00112233445566778899aabb"
OTHER = "0123456789abcdef0123456789abcdef01234567"
W = {"writer-a", "writer-b", "bot-account"}


def c(login, body, t="2026-11-03T08:00:00Z"):
    return {"login": login, "body": body, "created_at": t}


class Accounts(unittest.TestCase):
    def test_read_ok_from_a_non_writer_is_ignored(self):
        self.assertEqual(read_markers([c("stranger", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00")], W), "")

    def test_hold_from_a_non_writer_is_ignored(self):
        cs = [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00"), c("stranger", "HOLD " + HEAD + "\n\nSixth, 10:05")]
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD)

    def test_a_non_writer_cannot_clear_the_bots_hold(self):
        cs = [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00"), c("bot-account", "HOLD train 12 failed in x"),
              c("stranger", "READ-OK " + HEAD + "\n\nReviewer-lead, 11:00")]
        self.assertEqual(read_markers(cs, W), "HELD")

    def test_login_case_does_not_matter(self):
        self.assertEqual(read_markers([c("Writer-A", "READ-OK " + HEAD)], W), "READ-OK " + HEAD)

    def test_no_writers_fails_closed(self):
        self.assertEqual(read_markers([c("writer-a", "READ-OK " + HEAD)], set()), "")

    def test_cli_without_writers_prints_nothing(self):
        env = {k: v for k, v in os.environ.items() if k != "LAMP_COORD_WRITERS"}
        r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "markers.py")], input=json.dumps(c("writer-a", "READ-OK " + HEAD)) + "\n",
                           capture_output=True, text=True, env=env)
        self.assertEqual(r.stdout.strip(), "")

    def test_cli_with_a_writers_file(self):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("# fetched 1\nwriter-a\n"); path = f.name
        try:
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "markers.py"), "--writers", path],
                               input=json.dumps(c("writer-a", "READ-OK " + HEAD)) + "\n", capture_output=True, text=True)
            self.assertEqual(r.stdout.strip(), "READ-OK " + HEAD)
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "reads2.py"), "--writers", path, HEAD],
                               input=json.dumps({"login": "", "body": "TWO READS", "pr_body": True}) + "\n" + json.dumps(c("writer-a", "READ-OK " + HEAD)) + "\n",
                               capture_output=True, text=True)
            self.assertEqual(r.stdout.strip(), "short 1")
        finally:
            os.unlink(path)


class Parsing(unittest.TestCase):
    def test_thanks_for_the_read_ok_is_not_a_read(self):
        body = "Friday, 15:30: thanks for the READ-OK `e2b57f1f48`; nothing else changed."
        self.assertIsNone(parse_marker(body))
        items = [{"login": "", "body": "TWO READS", "pr_body": True},
                 c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"), c("writer-b", body)]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_a_possessive_read_ok_is_not_a_read(self):
        self.assertIsNone(parse_marker("Friday, 15:30: the Reviewer-lead's READ-OK e2b57f1f48 was on this head."))

    def test_bracketed_and_quoted_markers_do_not_count(self):
        self.assertIsNone(parse_marker("Friday, to Reviewer-two (HOLD 3e741c2201): new head"))
        self.assertIsNone(parse_marker("your 19:34 comment opens with `HOLD 73aa8b23f0`"))

    def test_bold_marker_counts(self):
        self.assertEqual(parse_marker("**READ-OK** `e2b57f1f48`")[:2], ("READ-OK", "e2b57f1f48"))

    def test_signatures(self):
        self.assertEqual(signer("READ-OK " + HEAD + "\n\nReviewer-lead, 15:24: fine"), "Reviewer-lead")
        self.assertEqual(signer("READ-OK " + HEAD + "\n\nSixth: fine"), "Sixth")
        self.assertEqual(signer("READ-OK " + HEAD + "\n\nlooks fine to me"), "")


class Quoted(unittest.TestCase):
    # 3 Nov 16:4x, Thirteen's HOLD on #5068: a quoted or fenced marker is no marker.
    ASK = {"login": "", "body": "Privacy. TWO READS: Reviewer-lead, Tests.", "pr_body": True}

    def test_an_authors_quoted_read_ok_on_line_one_is_not_a_read(self):
        body = "> READ-OK " + HEAD + "\n\nthanks, waiting for a second read"
        self.assertIsNone(parse_marker(body))
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"), c("writer-b", body)]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")
        self.assertIsNone(parse_marker("   > READ-OK " + HEAD))

    def test_a_read_ok_inside_a_code_fence_is_not_a_read(self):
        self.assertIsNone(parse_marker("```\nREAD-OK " + HEAD + "\n```\nFifth, 10:00: that is the form."))
        self.assertIsNone(parse_marker("~~~text\nHOLD " + HEAD + "\n~~~"))
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("writer-b", "```\nREAD-OK " + HEAD + "\n```")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_a_quoted_read_ok_on_line_two_of_a_signed_comment_is_not_a_read(self):
        body = "Fourth, 11:00: replying to the lead.\n> READ-OK " + HEAD + "\nI'll read it after the fix."
        self.assertIsNone(parse_marker(body))
        cs = [c("bot-account", "HOLD train 12 failed in lamp-voice"), c("writer-b", body)]
        self.assertEqual(read_markers(cs, W), "HELD")

    def test_a_marker_after_a_closed_fence_still_counts(self):
        self.assertEqual(parse_marker("Fourth, 11:00: the log:\n```\nok\n```\nREAD-OK " + HEAD)[:2], ("READ-OK", HEAD))

    def test_bold_and_backticked_first_lines_still_count(self):
        self.assertEqual(parse_marker("**READ-OK** `" + HEAD[:10] + "`")[:2], ("READ-OK", HEAD[:10]))
        self.assertEqual(parse_marker("`READ-OK " + HEAD[:10] + "`")[:2], ("READ-OK", HEAD[:10]))


class NextLineSigner(unittest.TestCase):
    # The Reviewer-lead's note on #5068: a label on the next line is no reader.
    def test_a_label_on_the_next_line_is_not_a_name(self):
        self.assertEqual(signer("READ-OK " + HEAD + "\nTests: 12 passed"), "")
        self.assertEqual(signer("READ-OK " + HEAD + "\nWhy: the fence"), "")
        self.assertEqual(signer("READ-OK " + HEAD + "\nFail-first: yes"), "")
        self.assertEqual(signer("READ-OK " + HEAD + "\nlooks good: yes"), "")

    def test_names_on_the_next_line_still_sign(self):
        self.assertEqual(signer("READ-OK " + HEAD + "\nSixth, read at the head"), "Sixth")
        self.assertEqual(signer("READ-OK " + HEAD + "\nReviewer-lead: my P3"), "Reviewer-lead")
        self.assertEqual(signer("READ-OK " + HEAD + "\nThe first builder: lifted"), "first builder")
        self.assertEqual(signer("READ-OK " + HEAD + "\nTests, 10:02: 12 passed"), "Tests")

    def test_a_tests_line_does_not_make_a_second_reader(self):
        ask = {"login": "", "body": "TWO READS", "pr_body": True}
        items = [ask, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("writer-a", "READ-OK " + HEAD + "\nTests: 12 passed")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")


class TwoReads(unittest.TestCase):
    ASK = {"login": "", "body": "Privacy. TWO READS: Reviewer-lead, Tests.", "pr_body": True}

    def test_same_login_signed_and_unsigned_is_one_reader(self):
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("writer-a", "READ-OK " + HEAD + "\n\nrestating my read after the rebase")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_unsigned_before_signed_is_still_one_reader(self):
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nlooks right"),
                 c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_two_signed_names_from_one_login_are_two_readers(self):
        # Shared accounts: the Reviewer-lead and Sixth post from one login.
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("writer-a", "READ-OK " + HEAD + "\n\nSixth, 15:10")]
        self.assertEqual(two_reads(items, HEAD, W), "met 2")

    def test_unsigned_reads_from_two_logins_are_two_readers(self):
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD), c("writer-b", "READ-OK " + HEAD)]
        self.assertEqual(two_reads(items, HEAD, W), "met 2")

    def test_a_non_writer_read_does_not_count(self):
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("stranger", "READ-OK " + HEAD + "\n\nHeron, 15:10")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_a_read_on_another_head_does_not_count(self):
        items = [self.ASK, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                 c("writer-b", "READ-OK " + OTHER + "\n\nHeron, 15:10")]
        self.assertEqual(two_reads(items, HEAD, W), "short 1")

    def test_a_read_on_a_same_patch_earlier_head_counts(self):
        # 4 Nov: #5043's two reads were on e05764f95b, rebased to 91ccaf30bb with the same patch.
        items = [self.ASK, c("writer-a", "READ-OK " + OTHER[:10] + "\n\nReviewer-lead, 15:41"),
                 c("writer-b", "READ-OK " + OTHER[:10] + "\n\nHeron, 16:00")]
        self.assertEqual(two_reads(items, [HEAD, OTHER], W), "met 2")
        self.assertEqual(two_reads(items, HEAD, W), "short 0")

    def test_ask_from_a_non_writer_comment_is_ignored(self):
        self.assertEqual(two_reads([c("stranger", "TWO READS"), c("writer-a", "READ-OK " + HEAD)], HEAD, W), "none")


class BotHold(unittest.TestCase):
    def test_cleared_by_a_real_read_ok(self):
        cs = [c("writer-a", "READ-OK " + OTHER + "\n\nReviewer-lead, 10:00"), c("bot-account", "HOLD train 12 failed in lamp-voice"),
              c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 11:00")]
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD)

    def test_cleared_by_a_signed_marker_later_in_the_comment(self):
        cs = [c("bot-account", "HOLD train 12 failed in lamp-voice"),
              c("writer-a", "Reviewer-lead, 11:00, READ-OK " + HEAD[:10] + ": the fix is in.")]
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD[:10])

    def test_not_cleared_by_a_quoted_read_ok(self):
        cs = [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00"), c("bot-account", "HOLD train 12 failed in lamp-voice"),
              c("writer-b", "Fourth, 11:00: the Reviewer-lead's READ-OK e2b57f1f48 was on this head."),
              c("writer-b", "Fourth, 11:01: as `READ-OK e2b57f1f48` said, nothing changed.")]
        self.assertEqual(read_markers(cs, W), "HELD")

    def test_not_cleared_by_a_read_ok_without_a_commit(self):
        cs = [c("bot-account", "HOLD train 12 failed in lamp-voice"), c("writer-a", "READ-OK\n\nReviewer-lead, 11:00")]
        self.assertEqual(read_markers(cs, W), "HELD")


class QueueHold(unittest.TestCase):
    Q = [c("writer-b", "HOLD a13408fb8d: lamp check pending\n\nSeventh, 09:40", "2026-11-03T02:40:00Z")]

    def test_holds_the_pr_whose_head_starts_with_the_sha(self):
        self.assertEqual(standing("a13408fb8d" + "0" * 30, self.Q, [], W), "2026-11-03T02:40:00Z")

    def test_another_head_is_not_held(self):
        self.assertEqual(standing(HEAD, self.Q, [], W), "")

    def test_lifted_by_the_same_accounts_later_read_ok_saying_so(self):
        pr = [c("writer-b", "**READ-OK** a13408fb8d\n\nSeventh, 10:30: the lamp check passed; this clears my HOLD.", "2026-11-03T03:30:00Z")]
        self.assertEqual(standing("a13408fb8d" + "0" * 30, self.Q, pr, W), "")

    def test_not_lifted_by_another_account_or_an_earlier_comment_or_plain_words(self):
        head = "a13408fb8d" + "0" * 30
        self.assertNotEqual(standing(head, self.Q, [c("writer-a", "READ-OK a13408fb8d\n\nReviewer-lead, 10:30: HOLD lifted", "2026-11-03T03:30:00Z")], W), "")
        self.assertNotEqual(standing(head, self.Q, [c("writer-b", "READ-OK a13408fb8d\n\nSeventh, 09:00: HOLD lifted", "2026-11-03T02:00:00Z")], W), "")
        self.assertNotEqual(standing(head, self.Q, [c("writer-b", "Seventh, 10:30: this clears my HOLD.", "2026-11-03T03:30:00Z")], W), "")

    def test_a_queue_hold_from_a_non_writer_is_ignored(self):
        q = [c("stranger", "HOLD a13408fb8d", "2026-11-03T02:40:00Z")]
        self.assertEqual(standing("a13408fb8d" + "0" * 30, q, [], W), "")


class HoldsLifted(unittest.TestCase):
    def test_a_hold_closed_in_words_needs_a_row(self):
        cs = [c("writer-a", "HOLD " + HEAD + "\n\nSecond, 10:00: one P2"), c("writer-b", "READ-OK " + HEAD + "\n\nFourth, 10:30"),
              c("writer-a", "Second, 11:39: my P2 is fixed, this clears my HOLD.")]
        self.assertEqual(read_markers(cs, W, pr="1234", lifted_path=os.devnull), "HELD")
        with tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False) as f:
            f.write("1234\twriter-a/second\te2b57f1f\thttps://example.invalid/1\n"); path = f.name
        try:
            self.assertEqual(read_markers(cs, W, pr="1234", lifted_path=path), "READ-OK " + HEAD)
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main(verbosity=1)


class BodyAsks(unittest.TestCase):
    # 4 Nov 22:1x (the PM, #5212): a PR body asks for two reads only on a line that asks it, never by quoting the rule.
    def ask(self, body):
        return two_reads([{"login": "", "body": body, "pr_body": True},
                          c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00")], HEAD, W)

    def test_lines_that_ask(self):
        for body in ("TWO READS: Reviewer-lead, Tests.",
                     "Privacy.\n\nTWO READS: Fourth (coordinator) and the first builder (privacy).",
                     "Privacy. TWO READS: Reviewer-lead, Tests.",
                     "## Lamp check\n\nok\n\n## TWO READS\n\nSixth and the first builder.",
                     "**TWO READS**: Fourth and the first builder.",
                     "TWO READS"):
            self.assertEqual(self.ask(body), "short 1", body)

    def test_bodies_that_only_name_the_rule(self):
        for body in ("Turning it on in homes is a separate default-on PR (TWO READS), later.",
                     "Per joining.md, privacy pull requests need TWO READS.",
                     "## Tests\n\n- TWO READS across a comment and a review;",
                     "Today it is left out as TWO READS short, because its body names the phrase."):
            self.assertEqual(self.ask(body), "none", body)
