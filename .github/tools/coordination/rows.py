# rows.py (14 Nov (the lead): "a PR's lamp row comes before its train"): whether a PR has its lamp row, read from its
# comments as JSON lines {login, body} on stdin. argv: [--writers FILE] [--switch-on] [--pr N] HEAD [EARLIER_HEAD...].
# Prints one word:
#   row            a writer's comment whose first line opens with ROW and this PR's number (bold allowed: "**ROW #7016
#                  <head> on build N …"), or whose first 300 characters say "lamp row for #N" for this PR, as Fifth's
#                  rows open ("Fifth, 09:51 GMT+7: lamp row for #6907 on lamp-52e6 …");
#   no-row-needed  a writer's READ-OK whose first line is the marker, for a commit one of the heads starts with (or a
#                  prefix of one), saying "no row needed" (docs, tests, or code behind a switch that's off);
#   row-incomplete MISSING  a row of this PR (and no "no row needed" that carries it) that lacks lamp, build or journal
#                  (15:40 below), MISSING the parts it lacks, comma-separated ("lamp,journal");
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
# three: in its first 400 characters a lamp ("lamp-52e6": lamp- and 4 lowercase letters or digits, a whole word) and a
# build (a five-digit sequence as a whole word, "12987", or "build <7-40 hex commit>"), and a journal: a fenced code block
# with a line in it, or a quoted line ("> …") naming a lamp-. Without one, the bot says which part it lacks.
import json, os, re, sys
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from markers import parse_marker, quoted, names_commit, writers_from_args, is_writer

# The words a row's verdict is read from (case-sensitive). 14 Nov (the second reader's P2 on #7041): FAILED, FAILS,
# VOIDED and VOIDS too; FAILOVER is no verdict.
FAILED = re.compile(r"\b(?:FAIL|VOID)(?:ED|S)?\b")
PASSED = re.compile(r"\bPASS\b")
NO_ROW = re.compile(r"(?i)\bno row needed\b")
LAMP = re.compile(r"(?<![\w-])lamp-[a-z0-9]{4}(?![\w-])")
BUILD = re.compile(r"(?<![#\w])\d{5}(?!\w)|\bbuild[ \t]+`?[0-9a-f]{7,40}\b")
FENCE_OPEN = re.compile(r" {0,3}(`{3,}|~{3,})")


def lamp_row_re(pr):
    return re.compile(r"(?i)\blamp row for #%s(?!\d)" % re.escape(str(pr)))


def row_line_re(pr):
    """ROW, then this PR's number, with or without "#" (bold allowed around either)."""
    return re.compile(r"\**ROW\b\**[ \t]+\**#?%s(?![\d])" % re.escape(str(pr)))


def failed(body):
    """A row whose first 300 characters say FAIL or VOID and not PASS."""
    head = body[:300]
    return bool(FAILED.search(head)) and not PASSED.search(head)


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


def has_row(c, pr):
    """None when the comment is no row of this PR (or a failed one), else the parts it lacks ([] for a whole row)."""
    body = c["body"].lstrip()
    if not pr:
        return None
    if not ((row_line_re(pr).match(body) and not quoted(body, 0)) or unquoted(lamp_row_re(pr), body, 300)):
        return None
    return None if failed(body) else missing(body)


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
    heads = [heads] if isinstance(heads, str) else [h for h in heads if h]
    found, short = "none", None
    for c in items:
        if c.get("pr_body") or not is_writer(c, writers):
            continue
        lacks = has_row(c, pr)
        if lacks == []:
            return "row"
        if lacks:
            short = lacks
        if not switch_on and says_no_row(c, heads):
            found = "no-row-needed"
    return "row-incomplete " + ",".join(short) if found == "none" and short else found


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
