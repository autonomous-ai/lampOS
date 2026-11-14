# Tests for rows.py (14 Nov, the lead: "a PR's lamp row comes before its train"): plain python3, no dependencies.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, subprocess, sys, tempfile, unittest
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from rows import rows

HEAD = "e2b57f1f48c0ffee00112233445566778899aabb"
EARLIER = "abcdef0123456789abcdef0123456789abcdef01"
OTHER = "0123456789abcdef0123456789abcdef01234567"
W = {"writer-a", "writer-b"}


def c(login, body):
    return {"login": login, "body": body, "created_at": "2026-11-14T02:00:00Z"}


class Row(unittest.TestCase):
    def test_a_row_first_line_counts(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016 %s on build 12701: the journal line" % HEAD[:10])], HEAD, W, "7016"), "row")

    def test_a_bold_row_counts(self):
        self.assertEqual(rows([c("writer-a", "**ROW** 7016 on build 12701")], HEAD, W, "7016"), "row")
        self.assertEqual(rows([c("writer-a", "  **ROW 7016 on build 12701**")], HEAD, W, "7016"), "row")

    def test_a_row_names_its_pr(self):
        # 14 Nov (the readers' P3s on #7034).
        for body in ("ROW #7016 on build 12701", "**ROW** #7016 on build 12701", "ROW **7016** on build 12701", "ROW 7016: x"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "row", body)
        for body in ("ROW 7017 on build 12701", "ROW 70160 on build 12701", "ROW #701 on build 12701",
                     "ROW %s on build 12701" % HEAD[:10], "ROW on build 12701"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_without_pr_a_row_never_counts(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016 on build 12701")], HEAD, W, None), "none")

    def test_a_failed_row_is_no_row(self):
        for body in ("ROW 7016 on build 12701: FAIL, the journal line", "ROW 7016 on build 12701: VOID (wrong kit)",
                     "Fifth, 09:51 GMT+7: lamp row for #7016 on lamp-52e6: FAIL"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)
        for body in ("ROW 7016 on build 12701: PASS, the earlier FAIL was the kit", "ROW 7016: VOID then PASS on a rerun",
                     "ROW 7016 on build 12701: no failure, fail-first test kept", "ROW 7016 " + "x" * 300 + " FAIL"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "row", body)

    def test_switch_on_needs_a_real_row(self):
        ok = c("writer-a", "READ-OK %s\n\nSixth, 10:02: no row needed." % HEAD)
        self.assertEqual(rows([ok], HEAD, W, "7016", switch_on=True), "none")
        self.assertEqual(rows([ok, c("writer-b", "ROW 7016 on build 12701: PASS")], HEAD, W, "7016", switch_on=True), "row")

    def test_row_must_be_the_word_opening_the_comment(self):
        for body in ("ROWS for later", "row 7016 on build 12701", "Fifth: ROW 7016 on build 12701",
                     "Here is the line.\nROW 7016 on build 12701"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_a_quoted_or_fenced_row_does_not_count(self):
        for body in ("> ROW 7016 on build 12701", "```\nROW 7016 on build 12701\n```", "```ROW 7016 on build 12701"):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_lamp_row_for_this_pr_counts(self):
        body = "Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6 at build 12690, the journal line below."
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "6907"), "row")
        self.assertEqual(rows([c("writer-a", body.replace("lamp row", "Lamp Row"))], HEAD, W, "6907"), "row")

    def test_lamp_row_for_another_pr_does_not(self):
        body = "Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6."
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "690"), "none")
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "6908"), "none")
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, None), "none")

    def test_lamp_row_past_300_characters_or_quoted_does_not(self):
        self.assertEqual(rows([c("writer-a", "x" * 300 + " lamp row for #6907")], HEAD, W, "6907"), "none")
        self.assertEqual(rows([c("writer-a", "Thanks.\n> lamp row for #6907 on lamp-52e6")], HEAD, W, "6907"), "none")


class NoRowNeeded(unittest.TestCase):
    def test_in_a_read_ok_on_the_head(self):
        body = "READ-OK %s\n\nSixth, 10:02: docs only, no row needed." % HEAD
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "no-row-needed")
        # Either sha may be the shorter.
        self.assertEqual(rows([c("writer-a", "READ-OK %s No row needed." % HEAD[:10])], HEAD, W, "7016"), "no-row-needed")
        self.assertEqual(rows([c("writer-a", "READ-OK %s no row needed" % HEAD)], HEAD[:12], W, "7016"), "no-row-needed")

    def test_on_an_earlier_head_given_with_the_same_patch(self):
        body = "**READ-OK** `%s`\n\nSixth, 10:02: tests only, no row needed." % EARLIER
        self.assertEqual(rows([c("writer-a", body)], [HEAD, EARLIER], W, "7016"), "no-row-needed")

    def test_on_another_head_does_not(self):
        body = "READ-OK %s\n\nSixth, 10:02: no row needed." % OTHER
        self.assertEqual(rows([c("writer-a", body)], [HEAD, EARLIER], W, "7016"), "none")

    def test_a_read_ok_with_no_sha_does_not(self):
        self.assertEqual(rows([c("writer-a", "READ-OK, no row needed.")], HEAD, W, "7016"), "none")

    def test_outside_a_read_ok_does_not(self):
        for body in ("Sixth, 10:02: on %s, no row needed." % HEAD,
                     "HOLD %s: no row needed, but a P2" % HEAD,
                     # A marker later in the comment is not a first-line READ-OK.
                     "Sixth, 10:02, READ-OK %s: no row needed." % HEAD):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_quoted_or_fenced_does_not(self):
        for tail in ("\n> no row needed", "\n```\nno row needed\n```"):
            body = "READ-OK %s\n\nSixth, 10:02: read." % HEAD + tail
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_a_row_is_preferred(self):
        cs = [c("writer-a", "READ-OK %s no row needed" % HEAD), c("writer-b", "ROW 7016 on build 12701")]
        self.assertEqual(rows(cs, HEAD, W, "7016"), "row")


class Writers(unittest.TestCase):
    def test_a_non_writer_never_counts(self):
        for body in ("ROW 7016 on build 12701", "lamp row for #7016 on lamp-52e6", "READ-OK %s no row needed" % HEAD):
            self.assertEqual(rows([c("stranger", body)], HEAD, W, "7016"), "none", body)

    def test_no_writers_fails_closed(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016 on build 12701")], HEAD, set(), "7016"), "none")

    def test_the_pr_body_never_counts(self):
        self.assertEqual(rows([{"login": "writer-a", "body": "ROW 7016", "pr_body": True}], HEAD, W, "7016"), "none")

    def test_the_cli(self):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("# fetched 1\nwriter-a\n"); path = f.name
        try:
            def cli(body, *heads):
                return subprocess.run([sys.executable, "-I", os.path.join(HERE, "rows.py"), "--writers", path, "--pr", "7016", *heads],
                                      input=json.dumps(c("writer-a", body)) + "\n", capture_output=True, text=True).stdout.strip()
            self.assertEqual(cli("ROW 7016 on build 12701", HEAD), "row")
            self.assertEqual(cli("READ-OK %s no row needed" % EARLIER, HEAD, EARLIER), "no-row-needed")
            self.assertEqual(cli("READ-OK %s no row needed" % EARLIER, HEAD), "none")
            self.assertEqual(cli("READ-OK %s no row needed" % HEAD, "--switch-on", HEAD), "none")
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
