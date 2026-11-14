# rows.py (14 Nov (the lead): "a PR's lamp row comes before its train"): whether a PR has its lamp row, read from its
# comments as JSON lines {login, body} on stdin. argv: [--writers FILE] [--pr N] HEAD [EARLIER_HEAD...]. Prints one word:
#   row            a writer's comment whose first line opens with ROW (bold allowed: "**ROW 7016 <head> on build N …"),
#                  or whose first 300 characters say "lamp row for #N" for this PR, as Fifth's rows open ("Fifth, 09:51
#                  GMT+7: lamp row for #6907 on lamp-52e6 …");
#   no-row-needed  a writer's READ-OK whose first line is the marker, for a commit one of the heads starts with (or a
#                  prefix of one), saying "no row needed" (docs, tests, or code behind a switch that's off);
#   none           neither.
# A ROW, a "lamp row for" or a "no row needed" on a quoted line ("> …") or inside a fenced code block is someone else's
# text and never counts (markers.py's quoted). Only comments from writers count (markers.py's --writers /
# LAMP_COORD_WRITERS; none given: none count, and it prints "none"). The heads are reads2.py's: the PR's head and earlier
# heads with the same patch (lib.sh's coord_same_patch), so a plain rebase keeps a READ-OK's "no row needed".
import json, os, re, sys
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from markers import parse_marker, quoted, names_commit, writers_from_args, is_writer

ROW_LINE = re.compile(r"\**ROW\b")
NO_ROW = re.compile(r"(?i)\bno row needed\b")


def lamp_row_re(pr):
    return re.compile(r"(?i)\blamp row for #%s(?!\d)" % re.escape(str(pr)))


def unquoted(rx, body, end=None):
    """Whether rx matches body (up to end) somewhere not on a quoted line or in a fence."""
    return any(not quoted(body, m.start()) for m in rx.finditer(body, 0, len(body) if end is None else end))


def has_row(c, pr):
    body = c["body"].lstrip()
    if ROW_LINE.match(body) and not quoted(body, 0):
        return True
    return bool(pr) and unquoted(lamp_row_re(pr), body, 300)


def says_no_row(c, heads):
    body = c["body"].lstrip()
    mk = parse_marker(body)
    if not mk or mk[0] != "READ-OK" or not mk[3] or not names_commit(mk[1], heads):
        return False
    return unquoted(NO_ROW, body)


def rows(items, heads, writers, pr=None):
    """items: dicts {login, body} in posting order. heads: the head sha, or a list of it and earlier heads with the same
    patch. Returns "row", "no-row-needed" or "none"."""
    heads = [heads] if isinstance(heads, str) else [h for h in heads if h]
    found = "none"
    for c in items:
        if c.get("pr_body") or not is_writer(c, writers):
            continue
        if has_row(c, pr):
            return "row"
        if says_no_row(c, heads):
            found = "no-row-needed"
    return found


def main():
    writers, args = writers_from_args(sys.argv[1:], "rows")
    pr, heads = None, []
    i = 0
    while i < len(args):
        if args[i] == "--pr" and i + 1 < len(args):
            pr = args[i + 1]; i += 2
        else:
            heads.append(args[i]); i += 1
    items = [json.loads(line) for line in sys.stdin if line.strip()]
    print(rows(items, heads, writers, pr))


if __name__ == "__main__":
    main()
