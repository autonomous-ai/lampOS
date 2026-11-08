# Tests for markers.py, reads2.py and qholds.py: plain python3, no dependencies.
# Run from .github/tools/coordination: python3 tests/test_markers.py
import json, os, shutil, subprocess, sys, tempfile, unittest
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from markers import read_markers, parse_marker, signer, merge_reviews, lifted_commits, held_note, ignored_reads
from reads2 import two_reads, body_names_unasked
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


def r(login, body, t, state="COMMENTED"):
    """A review as lib.sh fetches it: submitted_at as created_at."""
    return {"login": login, "body": body, "created_at": t, "state": state}


class Reviews(unittest.TestCase):
    """4 Nov (#5212 part 1): reads posted as GitHub reviews count as comments do."""

    def test_a_review_only_read_ok_counts(self):
        items = merge_reviews([], [r("writer-a", "READ-OK " + HEAD + "\n\nEl, 14:10: privacy re-read", "2026-11-04T07:10:58Z")])
        self.assertEqual(read_markers(items, W), "READ-OK " + HEAD)

    def test_a_review_hold_after_a_comment_read_ok_holds(self):
        cs = [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00", "2026-11-04T03:00:00Z")]
        rs = [r("writer-b", "HOLD " + HEAD + "\n\nEl, 10:30: one P2", "2026-11-04T03:30:00Z")]
        self.assertEqual(read_markers(merge_reviews(cs, rs), W), "HELD")

    def test_a_comment_hold_after_a_review_read_ok_holds(self):
        rs = [r("writer-b", "READ-OK " + HEAD + "\n\nEl, 10:00", "2026-11-04T03:00:00Z")]
        cs = [c("writer-a", "HOLD " + HEAD + "\n\nSixth, 10:30: one P2", "2026-11-04T03:30:00Z")]
        self.assertEqual(read_markers(merge_reviews(cs, rs), W), "HELD")

    def test_an_earlier_review_hold_is_cleared_by_the_same_readers_later_comment_read_ok(self):
        rs = [r("writer-b", "HOLD " + HEAD + "\n\nEl, 09:00: one P2", "2026-11-04T02:00:00Z")]
        cs = [c("writer-b", "READ-OK " + HEAD + "\n\nEl, 10:00: fixed", "2026-11-04T03:00:00Z")]
        self.assertEqual(read_markers(merge_reviews(cs, rs), W), "READ-OK " + HEAD)

    def test_an_empty_approved_review_counts_for_nothing(self):
        items = merge_reviews([], [r("writer-a", "", "2026-11-04T03:00:00Z", "APPROVED"), r("writer-a", "  \n", "2026-11-04T03:01:00Z", "APPROVED")])
        self.assertEqual(items, [])
        self.assertEqual(read_markers(items, W), "")

    def test_a_pending_review_and_a_non_writers_review_count_for_nothing(self):
        rs = [r("writer-a", "READ-OK " + HEAD, None, "PENDING"), r("stranger", "READ-OK " + HEAD, "2026-11-04T03:00:00Z")]
        self.assertEqual(read_markers(merge_reviews([], rs), W), "")

    def test_a_review_read_ok_is_one_of_two_reads(self):
        items = [{"login": "", "body": "TWO READS", "pr_body": True}] + merge_reviews(
            [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00", "2026-11-04T03:00:00Z")],
            [r("writer-b", "READ-OK " + HEAD + "\n\nEl, 11:00", "2026-11-04T04:00:00Z")])
        self.assertEqual(two_reads(items, HEAD, W), "met 2")

    def test_a_review_read_ok_lifts_its_readers_queue_hold(self):
        q = [c("writer-b", "HOLD a13408fb8d\n\nEl, 09:40: lamp check pending", "2026-11-04T02:40:00Z")]
        pr = merge_reviews([], [r("writer-b", "READ-OK a13408fb8d\n\nEl, 10:30: this clears my HOLD.", "2026-11-04T03:30:00Z")])
        self.assertEqual(standing("a13408fb8d" + "0" * 30, q, pr, W), "")

    def test_a_dismissed_reviews_read_ok_counts_for_nothing(self):
        rs = [r("writer-a", "READ-OK " + HEAD + "\n\nEl, 10:00", "2026-11-04T03:00:00Z", "DISMISSED")]
        self.assertEqual(merge_reviews([], rs), [])
        self.assertEqual(read_markers(merge_reviews([], rs), W), "")

    def test_a_review_does_not_shift_a_lifted_unsigned_comment_hold(self):
        cs = [c("writer-b", "READ-OK " + HEAD + "\n\nFourth, 09:00", "2026-11-04T02:00:00Z"),
              c("writer-a", "HOLD " + HEAD, "2026-11-04T03:00:00Z")]
        rs = [r("writer-b", "El, 09:30: a note, no marker", "2026-11-04T02:30:00Z")]
        with tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False) as f:
            f.write("1234\twriter-a/unsigned#1\te2b57f1f\thttps://example.invalid/1\n"); path = f.name
        try:
            self.assertEqual(read_markers(cs, W, pr="1234", lifted_path=path), "READ-OK " + HEAD)
            items = merge_reviews(cs, rs)
            self.assertTrue(items[1].get("review"))
            self.assertEqual(read_markers(items, W, pr="1234", lifted_path=path), "READ-OK " + HEAD)
        finally:
            os.unlink(path)

    def test_an_unsigned_review_hold_is_keyed_on_its_time(self):
        rs = [r("writer-a", "HOLD " + HEAD, "2026-11-04T03:00:00Z")]
        items = merge_reviews([c("writer-b", "READ-OK " + HEAD + "\n\nFourth, 09:00", "2026-11-04T02:00:00Z")], rs)
        self.assertEqual(read_markers(items, W, pr="1234", lifted_path=os.devnull), "HELD")
        with tempfile.NamedTemporaryFile("w", suffix=".tsv", delete=False) as f:
            f.write("1234\twriter-a/unsigned-review#2026-11-04T03:00:00Z\te2b57f1f\thttps://example.invalid/1\n"); path = f.name
        try:
            self.assertEqual(read_markers(items, W, pr="1234", lifted_path=path), "READ-OK " + HEAD)
        finally:
            os.unlink(path)

    def test_an_unsigned_review_hold_lifts_on_its_accounts_read_ok_saying_so(self):
        items = merge_reviews([c("writer-a", "READ-OK " + HEAD + "\n\nthis clears my HOLD", "2026-11-04T04:00:00Z")],
                              [r("writer-a", "HOLD " + HEAD, "2026-11-04T03:00:00Z")])
        self.assertEqual(read_markers(items, W), "READ-OK " + HEAD)

    @unittest.skipUnless(shutil.which("zsh"), "needs zsh")
    def test_lib_sh_merges_reviews_in_time_order_and_fails_closed(self):
        """lib.sh's coord_pr_comments with a stub gh that prints what `--jq` would (comments, then reviews)."""
        d = tempfile.mkdtemp()
        try:
            cm = [c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00", "2026-11-04T03:00:00Z")]
            rv = [r("writer-b", "HOLD " + HEAD + "\n\nEl, 10:30", "2026-11-04T03:30:00Z"), r("writer-a", "", "2026-11-04T03:40:00Z", "APPROVED")]
            for name, rows in (("comments", cm), ("reviews", rv)):
                with open(os.path.join(d, name), "w") as f:
                    f.write("".join(json.dumps(x) + "\n" for x in rows))
            gh = os.path.join(d, "gh")
            with open(gh, "w") as f:
                f.write('#!/bin/sh\ncase "$2" in\n*/comments*) cat "$STUB/comments";;\n'
                        '*/reviews*) [ -n "$FAIL_REVIEWS" ] && exit 1; cat "$STUB/reviews";;\nesac\n')
            os.chmod(gh, 0o755)
            env = dict(os.environ, PATH=d + os.pathsep + os.environ["PATH"], STUB=d, TOOLS_DIR=HERE, SP=d, PYTHON=sys.executable)
            out = os.path.join(d, "out")
            cmd = ["zsh", "-c", 'source $TOOLS_DIR/lib.sh; coord_pr_comments 5080 "$1"', "x", out]
            self.assertEqual(subprocess.run(cmd, env=env).returncode, 0)
            with open(out) as f:
                items = [json.loads(line) for line in f if line.strip()]
            self.assertEqual([x["login"] for x in items], ["writer-a", "writer-b"])
            self.assertEqual(read_markers(items, W), "HELD")
            env["FAIL_REVIEWS"] = "1"
            self.assertNotEqual(subprocess.run(cmd, env=env).returncode, 0)
            self.assertFalse(os.path.exists(out))
        finally:
            shutil.rmtree(d)


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

    def test_a_body_that_names_two_reads_anywhere_asks(self):
        # 7 Nov (#5635, the PM): fail closed. #5572 and #5580 merged on one read with TWO READS in their bodies'
        # prose; a false match only costs a second read.
        for body in ("Turning it on in homes is a separate default-on PR (TWO READS), later.",
                     "Per joining.md, privacy pull requests need TWO READS.",
                     "## Tests\n\n- TWO READS across a comment and a review;",
                     "This PR needs TWO READS (privacy).", "Readers: TWO READS (privacy and tests)",
                     "Privacy, so **TWO  READS** on this one."):
            self.assertEqual(self.ask(body), "short 1", body)

    def test_an_opt_out_line_clears_a_body_that_only_names_it(self):
        for body in ("Per joining.md, privacy pull requests need TWO READS.\n\nONE READ: names the rule, asks nothing.",
                     "- TWO READS across a comment and a review;\n\n**ONE READ** (tests only)"):
            self.assertEqual(self.ask(body), "none", body)
        # An opt-out never clears a line that asks.
        self.assertEqual(self.ask("TWO READS: Reviewer-lead, Tests.\n\nONE READ: a slip."), "short 1")
        # "ONE READ" in prose is no opt-out.
        self.assertEqual(self.ask("Needs TWO READS, not ONE READ."), "short 1")

    def test_a_read_ok_line_is_no_opt_out(self):
        # The Reviewer-lead's P2 on #5636: "- ONE READ-OK covers the docs half" opted a privacy PR out.
        for body in ("Privacy: TWO READS.\n- ONE READ-OK covers the docs half",
                     "Privacy needs TWO READS.\nONE READ-OK is in already."):
            self.assertEqual(self.ask(body), "short 1", body)
        self.assertEqual(self.ask("Privacy needs TWO READS.\nONE READ - tests only."), "none")

    def test_an_opt_out_quoted_or_fenced_is_no_opt_out(self):
        # El's P3 on #5636: a "> ONE READ:" line quoted from another PR, or one inside a fence, opted the body out.
        for body in ("Privacy needs TWO READS.\n\n> ONE READ: tests only (quoted from #5600).",
                     "Privacy needs TWO READS.\n\n```\nONE READ: tests only.\n```",
                     "Privacy needs TWO READS.\n\n~~~text\nONE READ: tests only.\n~~~"):
            self.assertEqual(self.ask(body), "short 1", body)
        # Outside the fence it still opts out.
        self.assertEqual(self.ask("Privacy needs TWO READS.\n\n```\ncode\n```\nONE READ: tests only."), "none")

    def test_an_opt_out_in_indented_code_or_a_longer_fence_is_no_opt_out(self):
        # El's P3s on #5643: four spaces or a tab make indented code, and a fence closes only on a line of its own
        # character, at least as long, with nothing after it.
        for body in ("Privacy needs TWO READS.\n\n    ONE READ: tests only.",
                     "Privacy needs TWO READS.\n\n\tONE READ: tests only.",
                     "Privacy needs TWO READS.\n\n````\n```\nONE READ: tests only.\n````",
                     "Privacy needs TWO READS.\n\n```\n```python\nONE READ: tests only.\n```",
                     "Privacy needs TWO READS.\n\n~~~\n```\nONE READ: tests only.\n~~~"):
            self.assertEqual(self.ask(body), "short 1", body)
        for body in ("Privacy needs TWO READS.\n\n   ONE READ: tests only.",
                     "Privacy needs TWO READS.\n\n  - ONE READ: tests only.",
                     "Privacy needs TWO READS.\n\n````\n```\n````\nONE READ: tests only.",
                     "Privacy needs TWO READS.\n\n```\nx\n```  \nONE READ: tests only."):
            self.assertEqual(self.ask(body), "none", body)

    def test_no_mention_asks_nothing(self):
        for body in ("Nothing about reads.", "two reads, lowercase", "ONE READ: tests only."):
            self.assertEqual(self.ask(body), "none", body)

    def test_opted_out_is_said(self):
        # The bot's log says a body that names TWO READS and opts out, once per head.
        self.assertTrue(body_names_unasked("Privacy needs TWO READS.\n\nONE READ: tests only."))
        for body in ("TWO READS: Reviewer-lead, Tests.", "Per joining.md, privacy needs TWO READS.", "no ask here",
                     "ONE READ: tests only."):
            self.assertFalse(body_names_unasked(body), body)

    def test_unasked_cli(self):
        def run(body):
            line = json.dumps({"login": "", "body": body, "pr_body": True})
            return subprocess.run([sys.executable, "-I", os.path.join(HERE, "reads2.py"), "--unasked"], input=line + "\n",
                                  capture_output=True, text=True, check=True).stdout.strip()
        self.assertEqual(run("This PR needs TWO READS (privacy).\nONE READ: tests only."), "unasked")
        self.assertEqual(run("This PR needs TWO READS (privacy)."), "")
        self.assertEqual(run("Nothing about reads."), "")


class ReadAskNamesTwoReads(unittest.TestCase):
    # 7 Nov (#5635): a writer's READ ASK that names TWO READS anywhere asks, not only a comment opening with it.
    def reads(self, *cs):
        return two_reads([{"login": "", "body": "A body.", "pr_body": True}] + list(cs), HEAD, W)

    def test_read_ask_with_two_reads_on_its_second_line(self):
        ask = c("writer-b", "Twelve, 02:40, to **the Reviewer-lead**: READ ASK " + HEAD[:10] + "\n\nPrivacy: TWO READS, "
                "the PM names the second.")
        self.assertEqual(self.reads(ask, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00")), "short 1")
        self.assertEqual(self.reads(ask, c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 15:00"),
                                    c("writer-b", "READ-OK " + HEAD + "\n\nSixth, 15:20")), "met 2")

    def test_read_ask_with_an_opt_out_line(self):
        ask = c("writer-b", "READ ASK " + HEAD[:10] + " to the Reviewer-lead; the rule says TWO READS for privacy.\n"
                "ONE READ: tests only.")
        self.assertEqual(self.reads(ask, c("writer-a", "READ-OK " + HEAD)), "none")

    def test_a_read_ok_that_mentions_a_read_ask_asks_nothing(self):
        # The Reviewer-lead's P3 on #5636: only a comment whose first line is a READ ASK asks.
        self.assertEqual(self.reads(c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead: read at the READ ASK; one "
                                                 "read here, not TWO READS.")), "none")
        self.assertEqual(self.reads(c("writer-b", "Sixth, 10:00: the READ ASK above names TWO READS by mistake.")), "none")

    def test_a_non_writer_read_ask_is_ignored(self):
        self.assertEqual(self.reads(c("stranger", "READ ASK " + HEAD[:10] + "\n\nTWO READS"), c("writer-a", "READ-OK " + HEAD)),
                         "none")

    def test_other_comments_naming_two_reads_ask_nothing(self):
        # Only a READ ASK or a first line asks: a reader's note naming the rule is not an ask.
        self.assertEqual(self.reads(c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead: one read, not TWO READS.")),
                         "none")


class QueueAsks(unittest.TestCase):
    """7 Nov (the PM, after Twelve's promote table): a writer's READ ASK on the Queue that names TWO READS asks for the PR
    it names, by its head (or a same-patch head) or by its number on the first line. #5647 and #5650 were asked TWO READS
    only on the Queue and merged on one read."""
    BODY = {"login": "", "body": "A launcher fix.", "pr_body": True}
    ONE = c("writer-b", "READ-OK " + HEAD + "\n\nJarvis, 06:06, the security read")

    def reads(self, queue, pr="5650", heads=HEAD, extra=()):
        return two_reads([self.BODY, self.ONE] + list(extra), heads, W, queue=queue, pr=pr)

    def test_a_queue_read_ask_by_head_names_two_reads(self):
        q = [c("writer-a", "READ ASK " + HEAD + "\nTWO READS. SubDarren, 06:05, to the **Reviewer-lead** and **Jarvis**: #5650.")]
        self.assertEqual(self.reads(q), "short 1")
        self.assertEqual(self.reads(q, extra=[c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 08:00")]), "met 2")

    def test_a_queue_read_ask_by_number_names_two_reads(self):
        q = [c("writer-a", "El, 05:18, to **Faye**: **READ ASK (restate) [#5650](https://example.invalid/5650)** at `"
               + OTHER[:10] + "`. TWO READS: Jarvis restates, and a second reader.")]
        self.assertEqual(self.reads(q), "short 1")

    def test_a_queue_read_ask_on_a_same_patch_earlier_head(self):
        q = [c("writer-a", "READ ASK " + OTHER[:10] + "\nTWO READS, security.")]
        self.assertEqual(self.reads(q, heads=[HEAD, OTHER]), "short 1")

    def test_a_comma_signed_queue_read_ask(self):
        # 7 Nov (the second reviewer's HOLD on #5653): signed with a comma, not a colon, before READ ASK.
        for first in ("Sixth, READ ASK to the **Reviewer-lead**: [#5650](https://example.invalid/5650)",
                      "Friday, 17:51, READ ASK to **Thirteen** and **Heron**: [#5650](https://example.invalid/5650)"):
            self.assertEqual(self.reads([c("writer-a", first + "\nTWO READS.")]), "short 1", first)
        # Prose naming a READ ASK after a comma but not as the marker still asks nothing.
        self.assertEqual(self.reads([c("writer-a", "Faye, 07:28, the hour: #5650's READ ASK was posted, TWO READS")]), "none")

    def test_a_dash_signed_queue_read_ask(self):
        # 7 Nov (#5653's P3s): signed with an em dash, an en dash or a spaced hyphen before READ ASK.
        for first in ("Twelve — READ ASK #5650", "Twelve – READ ASK #5650", "Second - READ ASK #5650",
                      "Second - **READ ASK** `" + HEAD[:10] + "`"):
            self.assertEqual(self.reads([c("writer-a", first + "\nTWO READS.")]), "short 1", first)
        # Prose mid-line, or a hyphen inside a word, still asks nothing.
        for first in ("Faye, 07:28 the hour. #5650 had a READ ASK naming TWO READS", "the pre-READ ASK note for #5650"):
            self.assertEqual(self.reads([c("writer-a", first + "\nTWO READS.")]), "none", first)

    def test_queue_shas_takes_only_asking_first_lines(self):
        # 7 Nov (#5653's P3s): `reads2.py --queue-shas FILE` gives lib.sh's coord_same_patch the shas on Queue first lines
        # QUEUE_ASK takes, so both read one regex.
        from reads2 import queue_shas
        q = [c("writer-a", "Sixth, READ ASK to **El**: `" + HEAD[:10] + "`\nTWO READS."),
             c("writer-a", "Twelve — READ ASK " + OTHER + "\nTWO READS."),
             c("writer-a", "the hour #5650 had a READ ASK abcdef1234 naming TWO READS"),
             c("writer-a", "READ ASK #5650\nat 1234567abc, TWO READS.")]
        self.assertEqual(queue_shas(q), [HEAD[:10], OTHER])
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as f:
            f.write("".join(json.dumps(x) + "\n" for x in q)); qpath = f.name
        try:
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "reads2.py"), "--queue-shas", qpath],
                               capture_output=True, text=True, check=True)
            self.assertEqual(r.stdout.split(), [HEAD[:10], OTHER])
        finally:
            os.unlink(qpath)

    def test_a_queue_read_ask_on_an_older_commit_of_the_pr(self):
        # 7 Nov (the second reviewer's HOLD on #5653): asked at OTHER, then a fix pushed on top (another patch). The ask
        # counts by the PR's commits, but a READ-OK on OTHER still does not count as a read on HEAD.
        q = [c("writer-a", "READ ASK " + OTHER[:10] + "\nTWO READS.")]
        self.assertEqual(two_reads([self.BODY, self.ONE], [HEAD], W, queue=q, pr="5650", commits=[OTHER, HEAD]), "short 1")
        self.assertEqual(two_reads([self.BODY, self.ONE], [HEAD], W, queue=q, pr="5650"), "none")
        old = c("writer-a", "READ-OK " + OTHER + "\n\nReviewer-lead, 08:00")
        self.assertEqual(two_reads([self.BODY, self.ONE, old], [HEAD], W, queue=q, pr="5650", commits=[OTHER, HEAD]), "short 1")

    def test_the_cli_reads_the_pr_commits_file(self):
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as f:
            f.write(json.dumps(c("writer-a", "READ ASK " + OTHER[:10] + "\nTWO READS.")) + "\n"); qpath = f.name
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("writer-a\nwriter-b\n"); wpath = f.name
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write(HEAD + "\n" + OTHER + "\n"); cpath = f.name
        try:
            lines = "".join(json.dumps(x) + "\n" for x in (self.BODY, self.ONE))
            def run(*extra):
                return subprocess.run([sys.executable, "-I", os.path.join(HERE, "reads2.py"), "--writers", wpath, "--queue", qpath,
                                       "--pr", "5650", *extra, HEAD], input=lines, capture_output=True, text=True, check=True).stdout.strip()
            self.assertEqual(run("--pr-commits", cpath), "short 1")
            self.assertEqual(run(), "none")
        finally:
            os.unlink(qpath); os.unlink(wpath); os.unlink(cpath)

    def test_queue_comments_that_do_not_ask_this_pr(self):
        for body in ("READ ASK " + OTHER[:10] + "\nTWO READS, for #5651.",           # another PR's head
                     "READ ASK [#5651](https://example.invalid) TWO READS",       # another PR's number
                     "READ ASK " + HEAD + "\nOne read, the Reviewer-lead.",       # no TWO READS
                     "READ ASK " + HEAD + "\nPrivacy needs TWO READS.\nONE READ: tests only.",   # opted out
                     "Faye, 07:28: the hour. #5650 had a READ ASK naming TWO READS on the Queue."):  # not a READ ASK line
            self.assertEqual(self.reads([c("writer-a", body)]), "none", body)
        # A non-writer's Queue READ ASK asks nothing.
        self.assertEqual(self.reads([c("stranger", "READ ASK " + HEAD + "\nTWO READS")]), "none")
        # #56500 is not #5650.
        self.assertEqual(self.reads([c("writer-a", "READ ASK [#56500](x) TWO READS")]), "none")

    def test_without_a_queue_nothing_changes(self):
        self.assertEqual(two_reads([self.BODY, self.ONE], HEAD, W), "none")

    def test_the_cli_reads_the_queue_file(self):
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as f:
            f.write(json.dumps(c("writer-a", "READ ASK " + HEAD + "\nTWO READS.")) + "\n"); qpath = f.name
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("writer-a\nwriter-b\n"); wpath = f.name
        try:
            lines = "".join(json.dumps(x) + "\n" for x in (self.BODY, self.ONE))
            def run(*extra):
                return subprocess.run([sys.executable, "-I", os.path.join(HERE, "reads2.py"), "--writers", wpath, *extra, HEAD],
                                      input=lines, capture_output=True, text=True, check=True).stdout.strip()
            self.assertEqual(run("--queue", qpath, "--pr", "5650"), "short 1")
            self.assertEqual(run(), "none")
        finally:
            os.unlink(qpath); os.unlink(wpath)


class LiftedByCommit(unittest.TestCase):
    """6 Nov (#5534): lifting words naming a full commit; an unsigned READ-OK never lifts a signed HOLD (#5542)."""
    OLD = "1d3d9711f8ce871e21483fd840f57b3a5570d174"

    def test_the_lifting_words_name_a_full_commit(self):
        self.assertEqual(lifted_commits("My HOLD on " + self.OLD + " is lifted."), [self.OLD])
        self.assertEqual(lifted_commits("My HOLD on `" + self.OLD + "` is lifted."), [self.OLD])
        self.assertEqual(lifted_commits("This lifts my HOLD on `" + self.OLD[:10] + "`."), [self.OLD[:10]])
        self.assertEqual(lifted_commits("my P2 is fixed; HOLD lifted"), [])
        self.assertIsNone(lifted_commits("No P1 or P2. My HOLD stands on " + self.OLD + "."))

    def test_an_unsigned_hold_on_a_full_commit_is_lifted(self):
        # The Reviewer-lead on #5521: "HOLD <full>  Read at Fifth's ask." then "READ-OK <new>  My HOLD on <full> is lifted."
        cs = [c("writer-a", "HOLD " + self.OLD + "  Read at the author's ask. The base went in by train."),
              c("writer-a", "READ-OK " + HEAD + "  My HOLD on " + self.OLD + " is lifted. - **The rebase:** clean.")]
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD)

    def test_an_unsigned_read_ok_never_lifts_a_signed_hold(self):
        # The reviewer's P3 on #5542: on a shared account, reader X's signed HOLD stood alone on the commit, and reader
        # Y's unsigned "my HOLD on <A> is lifted" cleared it. Now it stands (fail closed).
        cs = [c("writer-a", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."),
              c("writer-a", "READ-OK " + HEAD + "\n\nMy HOLD on `" + self.OLD + "` is lifted: the P2 is fixed.")]
        self.assertEqual(read_markers(cs, W), "HELD")
        self.assertEqual(held_note(cs, W), HEAD + " writer-a/reviewer-lead=" + self.OLD[:10])

    def test_a_signed_hold_is_lifted_by_its_own_signer(self):
        cs = [c("writer-a", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."),
              c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 11:00: my HOLD on `" + self.OLD + "` is lifted.")]
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD)
        # Another signer on the same account lifts nothing.
        cs[1] = c("writer-a", "READ-OK " + HEAD + "\n\nSixth, 11:00: my HOLD on `" + self.OLD + "` is lifted.")
        self.assertEqual(read_markers(cs, W), "HELD")

    def test_negated_lifting_words_lift_nothing(self):
        # The reviewer's P3 on #5542.
        for words in ("my HOLD on " + self.OLD + " is not lifted.", "My HOLD on " + self.OLD + " isn't lifted.",
                      "my HOLD on " + self.OLD + " is not yet cleared.", "I won't lift my HOLD on " + self.OLD + ".",
                      "Not lifting my HOLD on " + self.OLD + " yet.", "I never lift my HOLD on " + self.OLD + " on words."):
            self.assertIsNone(lifted_commits(words), words)
            cs = [c("writer-a", "HOLD " + self.OLD + "  Read at the author's ask."), c("writer-a", "READ-OK " + HEAD + "  " + words)]
            self.assertEqual(read_markers(cs, W), "HELD", words)
        for words in ("my HOLD on " + self.OLD + " is lifted.", "I lift my HOLD on " + self.OLD + "."):
            self.assertEqual(lifted_commits(words), [self.OLD], words)
            cs = [c("writer-a", "HOLD " + self.OLD + "  Read at the author's ask."), c("writer-a", "READ-OK " + HEAD + "  " + words)]
            self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD, words)
        # A "not" in an earlier sentence doesn't negate the lift.
        self.assertEqual(lifted_commits("Not a P1. My HOLD on " + self.OLD + " is lifted."), [self.OLD])

    def test_a_hold_carried_over_a_rebase_and_lifted_on_the_new_commit_leaves_the_old_one_standing(self):
        # Intended (fail closed): the lift names only B, so the HOLD on A stands, and --held (held_note) names it.
        cs = [c("writer-a", "HOLD " + self.OLD + "  Read at the author's ask."),
              c("writer-a", "HOLD " + OTHER + "  Carried over the rebase."),
              c("writer-a", "READ-OK " + HEAD + "  My HOLD on " + OTHER + " is lifted.")]
        self.assertEqual(read_markers(cs, W), "HELD")
        self.assertEqual(held_note(cs, W), HEAD + " writer-a/unsigned#0=" + self.OLD[:10])

    def test_a_read_ok_that_doesnt_say_lifted_clears_nothing(self):
        cs = [c("writer-a", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."), c("writer-a", "READ-OK " + HEAD + "\n\nThe P2 is fixed.")]
        self.assertEqual(read_markers(cs, W), "HELD")
        cs = [c("writer-a", "HOLD " + self.OLD + "  Read at the author's ask."), c("writer-a", "READ-OK " + HEAD + "  Read again on " + self.OLD + ".")]
        self.assertEqual(read_markers(cs, W), "HELD")

    def test_only_the_same_login_lifts(self):
        for hold in ("HOLD " + self.OLD + "  Read at the author's ask.", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."):
            cs = [c("writer-a", hold), c("writer-b", "READ-OK " + HEAD + "  My HOLD on " + self.OLD + " is lifted.")]
            self.assertEqual(read_markers(cs, W), "HELD", hold)

    def test_only_a_hold_on_the_commit_it_names(self):
        for hold in ("HOLD " + self.OLD + "  Read at the author's ask.", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."):
            cs = [c("writer-a", hold), c("writer-a", "READ-OK " + HEAD + "  My HOLD on " + OTHER + " is lifted.")]
            self.assertEqual(read_markers(cs, W), "HELD", hold)

    def test_another_readers_hold_on_the_same_account_is_never_touched(self):
        # One account signs for two readers, both holding the commit: an unsigned lift can't say whose, so both stand.
        cs = [c("writer-a", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."),
              c("writer-a", "HOLD " + self.OLD + "\n\nSixth, 10:05: a privacy P2."),
              c("writer-a", "READ-OK " + HEAD + "\n\nMy HOLD on " + self.OLD + " is lifted.")]
        self.assertEqual(read_markers(cs, W), "HELD")
        # Signed, it lifts only its own: Sixth's stands.
        cs[2] = c("writer-a", "READ-OK " + HEAD + "\n\nReviewer-lead, 11:00: my HOLD on " + self.OLD + " is lifted.")
        self.assertEqual(read_markers(cs, W), "HELD")
        cs.append(c("writer-a", "READ-OK " + HEAD + "\n\nSixth, 11:10: the privacy P2 is fixed."))
        self.assertEqual(read_markers(cs, W), "READ-OK " + HEAD)

    def test_a_queue_hold_is_lifted_by_a_full_commit_and_only_its_own(self):
        q = [c("writer-b", "HOLD " + self.OLD + "\n\nSeventh, 09:40: lamp check pending.", "2026-11-03T02:40:00Z")]
        lift = lambda sha: [c("writer-b", "READ-OK " + self.OLD + "\n\nSeventh, 10:30: my HOLD on " + sha + " is lifted.", "2026-11-03T03:30:00Z")]
        self.assertEqual(standing(self.OLD, q, lift(self.OLD), W), "")
        self.assertNotEqual(standing(self.OLD, q, lift(OTHER), W), "")

    def test_a_held_pr_with_a_read_ok_is_noted_with_its_standing_holds(self):
        cs = [c("writer-a", "HOLD " + self.OLD + "\n\nReviewer-lead, 10:00: one P2."), c("writer-b", "READ-OK " + HEAD + "\n\nSixth, 10:30")]
        self.assertEqual(held_note(cs, W), HEAD + " writer-a/reviewer-lead=" + self.OLD[:10])
        self.assertEqual(held_note(cs[:1], W), "", "no READ-OK: nothing to note")
        self.assertEqual(held_note(cs[1:], W), "", "not held")
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("\n".join(sorted(W)) + "\n"); wf = f.name
        try:
            run = subprocess.run([sys.executable, "-I", os.path.join(HERE, "markers.py"), "--writers", wf, "--held"],
                                 input="".join(json.dumps(x) + "\n" for x in cs), capture_output=True, text=True)
            self.assertEqual(run.stdout.strip(), HEAD + " writer-a/reviewer-lead=" + self.OLD[:10])
        finally:
            os.unlink(wf)


class IgnoredReads(unittest.TestCase):
    """6 Nov (the PM's ask on #5542): a READ-OK from a login that isn't a writer is named, not skipped in silence."""

    def test_a_non_writers_read_ok_is_named(self):
        cs = [c("stranger", "READ-OK " + HEAD + "\n\nReviewer-lead, 10:00"), c("stranger", "READ-OK " + HEAD + "\n\nagain"),
              c("writer-a", "READ-OK " + HEAD + "\n\nSixth, 10:30"), c("stranger", "HOLD " + HEAD), c("stranger", "> READ-OK " + HEAD)]
        self.assertEqual(ignored_reads(cs, W), ["READ-OK " + HEAD[:7] + " by stranger ignored: not a writer"])
        self.assertEqual(ignored_reads(cs[2:3], W), [])
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("\n".join(sorted(W)) + "\n"); wf = f.name
        try:
            run = subprocess.run([sys.executable, "-I", os.path.join(HERE, "markers.py"), "--writers", wf, "--ignored"],
                                 input="".join(json.dumps(x) + "\n" for x in cs), capture_output=True, text=True)
            self.assertEqual(run.stdout.strip(), "READ-OK " + HEAD[:7] + " by stranger ignored: not a writer")
        finally:
            os.unlink(wf)


if __name__ == "__main__":
    unittest.main(verbosity=1)
