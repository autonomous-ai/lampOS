# rows.py (14 Nov (the lead): "a PR's lamp row comes before its train"): whether a PR has its lamp row, read from its
# comments as JSON lines {login, body} on stdin. argv: [--writers FILE] [--switch-on] [--pr N] HEAD [EARLIER_HEAD...].
# Prints one word:
#   row            a writer's comment whose first line opens with ROW and this PR's number (bold allowed: "**ROW #7016
#                  <head> on build N …"), or whose first 300 characters say "lamp row for #N" for this PR, as Fifth's
#                  rows open ("Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6 …"), or whose first line puts the
#                  verdict first (23 Nov): "ROW PASS: #N <head> …", "ROW #<issue> via #N …", or "ROW PASS <head>"
#                  with no number, its commit one of the heads (verdict_row_re, VERDICT_SHA);
#   no-row-needed  a writer's READ-OK whose first line is the marker, for a commit one of the heads starts with (or a
#                  prefix of one), saying "no row needed" (docs, tests, or code behind a switch that's off);
#   row-incomplete MISSING  a row of this PR (and no "no row needed" that carries it) that lacks lamp, build or journal
#                  (15:40 below), MISSING the parts it lacks, comma-separated ("lamp,journal");
#   row-stale SHA  with --switch-on, only whole rows whose ROW line names another patch's commit, SHA (17 Nov below);
#   none           neither.
# A ROW, a "lamp row for" or a "no row needed" on a quoted line ("> …") or inside a fenced code block is someone else's
# text and never counts (markers.py's quoted). Only comments from writers count (markers.py's --writers /
# LAMP_COORD_WRITERS; none given: none count, and it prints "none"). The heads are reads2.py's: the PR's head and earlier
# heads with the same patch (lib.sh's coord_same_patch), so a plain rebase keeps a READ-OK's "no row needed".
#
# 14 Nov (the readers' P3s on #7034): tighter.
# - A ROW line names its PR: "ROW" then "#N" or "N" for --pr's N. Another PR's number is no row here, and without --pr a
#   ROW line never counts (fail closed).
# - A failed row is no row: a ROW or "lamp row for #N" comment whose first 300 characters say FAIL, or VOID, and not
#   PASS (the words, in capitals) doesn't count.
# - A switch-on needs a real row: with --switch-on (the bot passes it when the PR's diff turns a LAMP_ switch on in a
#   product unit, lib.sh's coord_switch_on), "no row needed" never carries; only a row does.
#
# 14 Nov 15:40 (the lead): "a ROW line names the lamp, the build (seq or commit) and quotes the journal lines that show the
# behaviour; a ROW missing any of the three doesn't count." A row (a ROW line or a "lamp row for #N") counts only with all
# three: in its first 400 characters a lamp ("lamp-52e6", a whole word) and a build (a five-digit sequence, "12987", or
# "build <7-40 hex commit>"), and a journal: a fenced code block
# with a line in it, or a quoted line ("> …") naming a lamp-. Without one, the bot says which part it lacks.
# 14 Nov (both readers' P2 and the Reviewer-lead's P3 on #7062): a lamp is its hostname, lamp- and four hex characters
# with at least one digit (c0a4, 52e6, 06c1, 4ace, 75c9, cb94, 9adf, a0ae, 0c89), so a part's name (lamp-body,
# lamp-eyes, lamp-seen, lamp-test) is none; and a build's five digits follow "build", "installed" or "on" (bold or
# backticks between: "installed **12987**", "on 13000"), so a bare number ("20000 counts", "12345 ms") is none.
# 17 Nov (the first builder, train 1009): #7364, a switch-on, rode on a ROW at 9e6e84abcd while its head was 467781a239,
# another patch, because a ROW was matched by its PR's number alone. With --switch-on, a ROW line that names a commit
# right after its PR's number ("ROW #7364 9e6e84abcd… on lamp-9adf …") counts only if that commit is one of the heads
# (the head, or an earlier head with the same patch), as a READ-OK's does. A whole row naming another patch's commit is
# stale: its row ran on other code, and a carry is said by posting the ROW again at the head. A row short of a part says
# what it lacks first, as before. A ROW line that names no commit, and Fifth's "lamp row for #N", count as before;
# without --switch-on nothing changes.
import json, os, re, sys
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from markers import parse_marker, quoted, names_commit, writers_from_args, is_writer

# The words a row's verdict is read from (case-sensitive). 14 Nov (the second reader's P2 on #7041): FAILED, FAILS,
# VOIDED and VOIDS too; FAILOVER is no verdict.
FAILED = re.compile(r"\b(?:FAIL|VOID)(?:ED|S)?\b")
PASSED = re.compile(r"\bPASS\b")
# 20 Nov (#8066): a "no row needed" inside inline quotes ("…", “…”, '…', `…`) is mentioned, not said, and never
# counts. The Reviewer-lead's READ-OK said: I'd rather not call it "no row needed", and the bot took it as a reader's.
NO_ROW = re.compile(r"(?i)(?<![\"“‘'`])\bno row needed\b(?![\"”’'`])")
# 28 Nov (the first builder; #8892 sat out with a full ROW on lamp-dbda): the digit rule keeps a part's name ("lamp-face")
# from counting, but one lamp of the fleet, dbda, has no digit. Such a lamp is named here, so no other all-letter word counts.
ALL_LETTER_LAMPS = ("dbda",)
LAMP = re.compile(r"(?<![\w-])lamp-(?:(?=[0-9a-f]{0,3}[0-9])[0-9a-f]{4}|" + "|".join(ALL_LETTER_LAMPS) + r")(?![\w-])")
BUILD = re.compile(r"\b(?i:build|installed|on)[ \t]+[*`]*\d{5}(?!\w)|\bbuild[ \t]+`?[0-9a-f]{7,40}\b")
FENCE_OPEN = re.compile(r" {0,3}(`{3,}|~{3,})")


def lamp_row_re(pr):
    return re.compile(r"(?i)\blamp row for #%s(?!\d)" % re.escape(str(pr)))


def row_line_re(pr):
    """ROW, then this PR's number, with or without "#" (bold allowed around either)."""
    return re.compile(r"\**ROW\b\**[ \t]+\**#?%s(?![\d])" % re.escape(str(pr)))


# 23 Nov (first builder): the shapes runners write, the verdict first. On one night the bot read none of these as a
# row and three passed PRs missed gate A: "ROW PASS: #8306 <sha> (kit-8306b), lamp-52e6, …" (Fifth), "ROW PASS <sha>"
# with no number (Seventh, #8314) and "ROW #8294 via #8302 (head …) on lamp-52e6 …: PASS" (Sixth, an issue's row
# carried by this PR). A verdict-first line names this PR by its number, by "via #N", or, with no number, by a commit
# that is one of its heads. A verdict-first line's own verdict word decides (failed(), SubDarren's P2 on #8333):
# "ROW FAIL: #N …, legs 1-3 PASS" is no row, whatever PASS the counts after it say. On PR #X's thread "ROW #X via #Y"
# still counts for #X by row_line_re, the number right after ROW, as it did before.
VERDICT = r"(?:PASS|FAIL(?:ED|S)?|VOID(?:ED|S)?)"
# The verdict word of a verdict-first line, caught.
VERDICT_FIRST = re.compile(r"\**ROW\b\**[ \t]+\**(PASS|FAIL(?:ED|S)?|VOID(?:ED|S)?)\b")
# The commit a numberless verdict-first line names ("ROW PASS 0d7c0340e8…").
VERDICT_SHA = re.compile(r"\**ROW\b\**[ \t]+\**%s\**:?[ \t]+[*`]*([0-9a-f]{7,40})(?![0-9A-Za-z])" % VERDICT)


def verdict_row_re(pr):
    """ROW, a verdict word, then this PR's number ("ROW PASS: #8306"), or ROW, another number, then "via" this PR's
    ("ROW #8294 via #8302")."""
    n = re.escape(str(pr))
    return re.compile(r"\**ROW\b\**[ \t]+(?:\**%s\**:?[ \t]+\**#?%s(?!\d)|\**#?\d+\**[ \t]+via[ \t]+\**#?%s(?!\d))"
                      % (VERDICT, n, n))


def opens_row(body, pr, heads=()):
    """Whether body opens with a row line of pr: "ROW #N", a verdict-first "ROW PASS: #N" or "ROW #X via #N", or a
    numberless "ROW PASS <sha>" whose sha is one of heads."""
    if row_line_re(pr).match(body) or verdict_row_re(pr).match(body):
        return True
    m = VERDICT_SHA.match(body)
    return bool(m and heads and names_commit(m.group(1), heads))


# The commit a ROW line names right after its PR's number (bold, a backtick, "at", or "(head" as Sixth writes it,
# allowed before it: the Reviewer-lead's P2 on #8333, so a switch-on's via row at another patch reads as stale).
NAMED_SHA = re.compile(r"\**[ \t]+\(?(?:head[ \t]+|at[ \t]+)?[*`]*([0-9a-f]{7,40})(?![0-9A-Za-z])")


def named_sha(body, pr):
    """The commit a ROW line names after its PR's number, or None when it names none. A numberless verdict-first
    line ("ROW PASS <sha>") needs no read here: it counts only when its sha is one of the heads, so it's never
    stale."""
    for rx in (row_line_re(pr), verdict_row_re(pr)):
        m = rx.match(body)
        if m:
            s = NAMED_SHA.match(body, m.end())
            return s.group(1) if s else None
    return None


def row_line(body, pr=None, heads=()):
    """The line of body that makes it a row of pr, as has_row matched it: line 1 when it opens with this PR's ROW
    line (opens_row, the heads deciding a numberless one), else the first line where "lamp row for #N" is said
    unquoted. Line 1 when neither, or without pr."""
    lines = body.split("\n")
    if pr and not opens_row(body, pr, heads):
        rx, offset = lamp_row_re(pr), 0
        for line in lines:
            m = rx.search(line)
            if m and not quoted(body, offset + m.start()):
                return line
            offset += len(line) + 1
    return lines[0]


def failed(body, pr=None, heads=()):
    """A row whose first 300 characters say FAIL or VOID, and whose own row line doesn't say PASS.

    22 Nov (#8180, train 1162): the PASS that clears a FAIL is read from the row's own line only, the line has_row
    matched (row_line), never from the rest of the first 300 characters. "ROW #8180 … build 15510: FAIL" followed by
    a check's "`VERIFY PASS`" in the next paragraph was counted as a row and cut into a train. A row line that says
    "PASS, the earlier FAIL was the kit" or "FAILED then PASS on a rerun" still counts. The Reviewer-lead's P2 on
    #8293: the line is the one has_row matched, so an earlier line naming a kit like "row-8053r DRY PASS" can't
    clear a mid-post "lamp row for #N: FAIL".

    23 Nov (SubDarren's P2 on #8333): when this PR's row line puts the verdict first, that word decides alone:
    "ROW FAIL: #8306 …, legs 1-3 PASS, leg 4 timed out" is failed, and "ROW PASS: #8306 …, leg 4 FAIL then PASS on its
    rerun" is not."""
    head = body[:300]
    if not FAILED.search(head):
        return False
    verdict = VERDICT_FIRST.match(head) if pr and opens_row(head, pr, heads) else None
    if verdict:
        return not verdict.group(1).startswith("PASS")
    return not PASSED.search(row_line(head, pr, heads))


def unquoted(rx, body, end=None):
    """Whether rx matches body (up to end) somewhere not on a quoted line or in a fence."""
    return any(not quoted(body, m.start()) for m in rx.finditer(body, 0, len(body) if end is None else end))


def journal(body):
    """A fenced code block (``` or ~~~; an unclosed one runs to the end) with a non-empty line, or a quoted line naming
    a lamp-."""
    fence = None
    for line in body.split("\n"):
        if fence is None:
            m = FENCE_OPEN.match(line)
            if m:
                fence = m.group(1)
            elif re.match(r"[ \t]*>", line) and "lamp-" in line:
                return True
        elif re.fullmatch(r" {0,3}" + re.escape(fence[0]) + "{%d,}[ \t]*\r?" % len(fence), line):
            fence = None
        elif line.strip():
            return True
    return False


def missing(body):
    """The parts a row lacks (15:40): a list of "lamp", "build", "journal"."""
    head = body[:400]
    return [part for part, ok in (("lamp", LAMP.search(head)), ("build", BUILD.search(head)), ("journal", journal(body)))
            if not ok]


def has_row(c, pr, heads=()):
    """None when the comment is no row of this PR (or a failed one), else the parts it lacks ([] for a whole row).
    heads: the PR's heads, which a numberless "ROW PASS <sha>" must name."""
    body = c["body"].lstrip()
    if not pr:
        return None
    if not ((opens_row(body, pr, heads) and not quoted(body, 0)) or unquoted(lamp_row_re(pr), body, 300)):
        return None
    return None if failed(body, pr, heads) else missing(body)


def says_no_row(c, heads):
    body = c["body"].lstrip()
    mk = parse_marker(body)
    if not mk or mk[0] != "READ-OK" or not mk[3] or not names_commit(mk[1], heads):
        return False
    return unquoted(NO_ROW, body)


def rows(items, heads, writers, pr=None, switch_on=False):
    """items: dicts {login, body} in posting order. heads: the head sha, or a list of it and earlier heads with the same
    patch. switch_on: the PR turns a switch on, so "no row needed" doesn't carry it. Returns "row", "no-row-needed",
    "row-incomplete <parts>" (the newest incomplete row's) or "none"."""
    # 18 Nov: a "code:<sha>" head (lib.sh's code_patch_of) counts for reads only, never for a row or "no row needed".
    heads = [heads] if isinstance(heads, str) else [h for h in heads if h and not h.startswith("code:")]
    found, short, stale = "none", None, None
    for c in items:
        if c.get("pr_body") or not is_writer(c, writers):
            continue
        lacks = has_row(c, pr, heads)
        # A whole row of a switch-on at another patch is stale; one short of a part says what it lacks first.
        if lacks == [] and switch_on:
            sha = named_sha(c["body"].lstrip(), pr)
            if sha and not names_commit(sha, heads):
                stale = sha
                continue
        if lacks == []:
            return "row"
        if lacks:
            short = lacks
        if not switch_on and says_no_row(c, heads):
            found = "no-row-needed"
    if found == "none" and short:
        return "row-incomplete " + ",".join(short)
    return "row-stale " + stale[:10] if found == "none" and stale else found


def main():
    writers, args = writers_from_args(sys.argv[1:], "rows")
    pr, heads, switch_on = None, [], False
    i = 0
    while i < len(args):
        if args[i] == "--switch-on":
            switch_on = True; i += 1
        elif args[i] == "--pr" and i + 1 < len(args):
            pr = args[i + 1]; i += 2
        else:
            heads.append(args[i]); i += 1
    items = [json.loads(line) for line in sys.stdin if line.strip()]
    print(rows(items, heads, writers, pr, switch_on))


if __name__ == "__main__":
    main()
