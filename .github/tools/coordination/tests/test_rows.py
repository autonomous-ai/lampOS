# Tests for rows.py (14 Nov, the lead: "a PR's lamp row comes before its train"): plain python3, no dependencies.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, subprocess, sys, tempfile, unittest
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
from rows import rows, missing

HEAD = "e2b57f1f48c0ffee00112233445566778899aabb"
EARLIER = "abcdef0123456789abcdef0123456789abcdef01"
OTHER = "0123456789abcdef0123456789abcdef01234567"
W = {"writer-a", "writer-b"}


def c(login, body):
    return {"login": login, "body": body, "created_at": "2026-11-14T02:00:00Z"}


# 14 Nov 15:40 (the lead): a row names the lamp and the build in its first 400 characters, and quotes the journal.
J = "\n```\nlamp-52e6 lamp-voice[812]: listening_ready\nlamp-52e6 lamp-voice[812]: heard \"set a timer\"\n```"
WHOLE = " on lamp-52e6 build 12701: PASS 3/3" + J


class Row(unittest.TestCase):
    def test_a_row_first_line_counts(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016 %s on lamp-52e6 build 12701: PASS" % HEAD[:10] + J)], HEAD, W, "7016"), "row")

    def test_a_bold_row_counts(self):
        self.assertEqual(rows([c("writer-a", "**ROW** 7016" + WHOLE)], HEAD, W, "7016"), "row")
        self.assertEqual(rows([c("writer-a", "  **ROW 7016 on lamp-52e6 build 12701**" + J)], HEAD, W, "7016"), "row")

    def test_a_row_names_its_pr(self):
        # 14 Nov (the readers' P3s on #7034).
        for body in ("ROW #7016", "**ROW** #7016", "ROW **7016**", "ROW 7016:"):
            self.assertEqual(rows([c("writer-a", body + WHOLE)], HEAD, W, "7016"), "row", body)
        for body in ("ROW 7017", "ROW 70160", "ROW #701", "ROW %s" % HEAD[:10], "ROW"):
            self.assertEqual(rows([c("writer-a", body + WHOLE)], HEAD, W, "7016"), "none", body)

    def test_without_pr_a_row_never_counts(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016" + WHOLE)], HEAD, W, None), "none")

    def test_a_failed_row_is_no_row(self):
        for body in ("ROW 7016 on lamp-52e6 build 12701: FAIL, the journal line" + J, "ROW 7016 on lamp-52e6 build 12701: VOID (wrong kit)" + J,
                     "Fifth, 09:51 GMT+7: lamp row for #7016 on lamp-52e6 build 12701: FAIL" + J):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)
        for body in ("ROW 7016 on lamp-52e6 build 12701: PASS, the earlier FAIL was the kit" + J,
                     "ROW 7016 on lamp-52e6 build 12701: VOID then PASS on a rerun" + J,
                     "ROW 7016 on lamp-52e6 build 12701: no failure, fail-first test kept" + J,
                     "ROW 7016 on lamp-52e6 build 12701 " + "x" * 300 + " FAIL" + J):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "row", body)

    def test_failed_voided_and_fails_are_no_row(self):
        # 14 Nov (the second reader's P2 on #7041).
        for body in ("ROW #7041 abc FAILED on build 5", "ROW #7041 abc VOIDED", "ROW #7041 abc: FAILS the timer line",
                     "ROW #7041 abc: VOIDS the earlier run"):
            self.assertEqual(rows([c("writer-a", body + WHOLE.replace(": PASS 3/3", ""))], HEAD, W, "7041"), "none", body)
        for body in ("ROW #7041 abc: FAILOVER kept the voice up", "ROW #7041 abc: FAILED then PASS on a rerun"):
            self.assertEqual(rows([c("writer-a", body + WHOLE)], HEAD, W, "7041"), "row", body)

    def test_switch_on_needs_a_real_row(self):
        ok = c("writer-a", "READ-OK %s\n\nSixth, 10:02: no row needed." % HEAD)
        self.assertEqual(rows([ok], HEAD, W, "7016", switch_on=True), "none")
        self.assertEqual(rows([ok, c("writer-b", "ROW 7016" + WHOLE)], HEAD, W, "7016", switch_on=True), "row")

    def test_row_must_be_the_word_opening_the_comment(self):
        for body in ("ROWS for later", "row 7016", "Fifth: ROW 7016", "Here is the line.\nROW 7016"):
            self.assertEqual(rows([c("writer-a", body + WHOLE)], HEAD, W, "7016"), "none", body)

    def test_a_quoted_or_fenced_row_does_not_count(self):
        for body in ("> ROW 7016" + WHOLE, "```\nROW 7016 on lamp-52e6 build 12701\n```", "```ROW 7016" + WHOLE):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7016"), "none", body)

    def test_lamp_row_for_this_pr_counts(self):
        body = "Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6 at build 12690, the journal line below." + J
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "6907"), "row")
        self.assertEqual(rows([c("writer-a", body.replace("lamp row", "Lamp Row"))], HEAD, W, "6907"), "row")

    def test_lamp_row_for_another_pr_does_not(self):
        body = "Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6 build 12690." + J
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "690"), "none")
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "6908"), "none")
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, None), "none")

    def test_lamp_row_past_300_characters_or_quoted_does_not(self):
        self.assertEqual(rows([c("writer-a", "x" * 300 + " lamp row for #6907" + WHOLE)], HEAD, W, "6907"), "none")
        self.assertEqual(rows([c("writer-a", "Thanks.\n> lamp row for #6907 on lamp-52e6 build 12690")], HEAD, W, "6907"), "none")


# A journal block that names no lamp (a lamp named in the block's first 400 characters names the row's lamp).
JN = "\n```\nlamp-voice[90]: listening_ready\n```"


class Parts(unittest.TestCase):
    """14 Nov 15:40 (the lead): a ROW missing the lamp, the build or the journal lines doesn't count."""
    def test_the_example_counts(self):
        body = "ROW #7031 0a997fa7f3 on lamp-c0a4 build 13031: PASS 3/3\n```\nlamp-c0a4 lamp-voice[90]: said \"done\"\n```"
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row")

    def test_the_fifth_row_shape_counts(self):
        body = ("Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6, installed **12987** (lamp-voice `70474a2102`, its "
                "ARM64 run's own build), kit timers-3, slot 10:00.\n\n```\nVERDICT PASS 3/3\nline 1 heard \"set a timer\"\n```")
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "6907"), "row")

    def test_todays_short_forms_do_not(self):
        self.assertEqual(rows([c("writer-a", "ROW #7031 350a59035d")], HEAD, W, "7031"), "row-incomplete lamp,build,journal")
        self.assertEqual(rows([c("writer-a", "ROW 7031 0a997fa7f3a341c25dc17f81d3e80aab095c0982 on a test lamp (Orange Pi, build 13007)")],
                              HEAD, W, "7031"), "row-incomplete lamp,journal")

    def test_each_missing_part(self):
        for body, want in (("ROW #7031 on build 13031: PASS" + JN, "lamp"),
                           ("ROW #7031 on lamp-c0a4: PASS" + JN, "build"),
                           ("ROW #7031 on lamp-c0a4 build 13031: PASS, all three lines heard", "journal")):
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row-incomplete " + want, body)

    def test_what_names_a_lamp(self):
        # 14 Nov (both readers' P2 on #7062): the fleet's lamps count; a part's name doesn't.
        fleet = ("c0a4", "52e6", "06c1", "4ace", "75c9", "cb94", "9adf", "a0ae", "0c89")
        parts = ("body", "eyes", "ears", "seen", "peek", "keep", "name", "test", "face", "abcd")
        for lamp, ok in ([("lamp-" + x, True) for x in fleet] + [("lamp-" + x, False) for x in parts]
                         + [("lamp-voice", False), ("lamp-52E6", False), ("lamp-52e", False), ("xlamp-52e6", False),
                            ("lamp-52e6x", False), ("lamp-52g6", False)]):
            body = "ROW #7031 on %s build 13031: PASS" % lamp + JN
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row" if ok else "row-incomplete lamp", lamp)

    def test_what_names_a_build(self):
        # 14 Nov (the Reviewer-lead's P3 on #7062): five digits count after build, installed or on.
        for build, ok in (("build 13031", True), ("installed **12987**", True), ("on 13000", True), ("Build `13031`", True),
                          ("build 70474a2102", True), ("build `70474a2102`", True), ("**13031**", False), ("20000 counts", False),
                          ("12345 ms", False), ("build 1303", False), ("build 130310", False), ("#13031", False),
                          ("build 70474a", False), ("run 43561597", False), ("on #13031", False)):
            body = "ROW #7031 on lamp-c0a4, %s: PASS" % build + JN
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row" if ok else "row-incomplete build", build)

    def test_a_part_name_is_no_lamp(self):
        self.assertEqual(missing("ROW #1 abc1234 on build 13031, lamp-body moved: PASS\n```\nx\n```"), ["lamp"])

    def test_lamp_and_build_past_400_characters_do_not(self):
        body = "ROW #7031: PASS " + "x" * 400 + " on lamp-c0a4 build 13031" + J
        self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row-incomplete lamp,build")

    def test_what_quotes_the_journal(self):
        for tail, ok in (("\n~~~\njournal line\n~~~", True), ("\n```\nunclosed journal line", True),
                         ("\n> lamp-c0a4 lamp-voice[9]: listening_ready", True), ("\n```\n\n   \n```", False),
                         ("\n> a quote with no lamp", False), ("\n    indented, not fenced", False)):
            body = "ROW #7031 on lamp-c0a4 build 13031: PASS" + tail
            self.assertEqual(rows([c("writer-a", body)], HEAD, W, "7031"), "row" if ok else "row-incomplete journal", tail)

    def test_a_whole_row_or_no_row_needed_beats_an_incomplete_one(self):
        short = c("writer-a", "ROW #7031 350a59035d")
        self.assertEqual(rows([short, c("writer-b", "ROW #7031 on lamp-c0a4 build 13031: PASS" + J)], HEAD, W, "7031"), "row")
        self.assertEqual(rows([short, c("writer-b", "READ-OK %s no row needed" % HEAD)], HEAD, W, "7031"), "no-row-needed")
        self.assertEqual(rows([short, c("writer-b", "READ-OK %s no row needed" % HEAD)], HEAD, W, "7031", switch_on=True),
                         "row-incomplete lamp,build,journal")


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
        cs = [c("writer-a", "READ-OK %s no row needed" % HEAD), c("writer-b", "ROW 7016" + WHOLE)]
        self.assertEqual(rows(cs, HEAD, W, "7016"), "row")


class Writers(unittest.TestCase):
    def test_a_non_writer_never_counts(self):
        for body in ("ROW 7016" + WHOLE, "lamp row for #7016" + WHOLE, "READ-OK %s no row needed" % HEAD):
            self.assertEqual(rows([c("stranger", body)], HEAD, W, "7016"), "none", body)

    def test_no_writers_fails_closed(self):
        self.assertEqual(rows([c("writer-a", "ROW 7016" + WHOLE)], HEAD, set(), "7016"), "none")

    def test_the_pr_body_never_counts(self):
        self.assertEqual(rows([{"login": "writer-a", "body": "ROW 7016" + WHOLE, "pr_body": True}], HEAD, W, "7016"), "none")

    def test_the_cli(self):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write("# fetched 1\nwriter-a\n"); path = f.name
        try:
            def cli(body, *heads):
                return subprocess.run([sys.executable, "-I", os.path.join(HERE, "rows.py"), "--writers", path, "--pr", "7016", *heads],
                                      input=json.dumps(c("writer-a", body)) + "\n", capture_output=True, text=True).stdout.strip()
            self.assertEqual(cli("ROW 7016" + WHOLE, HEAD), "row")
            self.assertEqual(cli("ROW 7016 on build 12701", HEAD), "row-incomplete lamp,journal")
            self.assertEqual(cli("READ-OK %s no row needed" % EARLIER, HEAD, EARLIER), "no-row-needed")
            self.assertEqual(cli("READ-OK %s no row needed" % EARLIER, HEAD), "none")
            self.assertEqual(cli("READ-OK %s no row needed" % HEAD, "--switch-on", HEAD), "none")
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
