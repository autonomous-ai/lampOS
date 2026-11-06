# reads2.py (31 Oct 03:1x, first builder): whether a PR asked for two reads is met, read from its comments as JSON lines
# {login, body} on stdin. argv: [--writers FILE] HEAD [EARLIER_HEAD...]. A comment whose first line is "TWO READS" (bold allowed) asks
# for two, as does a body or a READ ASK that names TWO READS anywhere (7 Nov, #5635, below); then two different readers must each have a READ-OK marker for a commit the head starts with. Prints "none"
# (not asked), "met N" or "short N". #4633 (23:37) and #4656 (02:58) went in trains on one READ-OK where two had been
# named, since the train bot read only the newest READ-OK.
#
# 3 Nov 15:4x (the Reviewer-lead's HOLD on #5068): the markers and signatures are markers.py's own (parse_marker,
# signer), so a READ-OK that markers.py wouldn't count (quoted, bracketed, named as a noun: "thanks for the READ-OK
# `e2b57f1f48`") is no read here either. Only comments from writers count (markers.py's --writers / LAMP_COORD_WRITERS;
# none given: none count), for the ask and the reads alike; the PR body's ask always counts.
#
# One read per person, and a person is a login plus the name it signs (lowercase), not a login alone: several builders
# share one GitHub account (the Reviewer-lead and Sixth post from one login, Fourteen and others from another), so
# counting distinct logins would make a legitimate two-reader PR impossible. An unsigned READ-OK never adds a second
# person to a login: it merges into that login's signed reader(s), and a login whose READ-OKs on the head are all
# unsigned is one reader. (A restate whose signature the parser misses filed as a second reader before.)
#
# 4 Nov 04:1x: a READ-OK on an earlier head with the same patch (a plain rebase; the caller passes those heads, lib.sh's
# coord_same_patch) counts as one on the head, as it does for one read. #5043's two reads were on e05764f95b; rebased
# to 91ccaf30bb, the same patch, it sat out "short 0" from 00:42.
import json, os, re, sys
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from markers import parse_marker, signer, writers_from_args, is_writer


# 4 Nov 22:1x (the PM, on #5212): a PR body asks for two reads only on a line that asks it, not wherever the words
# appear. A body quoting the rule held #5240 ("a separate default-on PR (TWO READS)") and #5202 ("privacy pull requests
# need TWO READS") for a second read neither needed, and #5286's test list ("- TWO READS across a comment and a review")
# the same. Asked: a line opening with TWO READS (after an optional heading mark, list mark, quote mark or bold),
# followed by ":", a dash, "(" or the line's end ("TWO READS: Fourth and the first builder", "## TWO READS"); or
# "TWO READS:" with readers after it anywhere in a line, as #5049's "Privacy. TWO READS: Reviewer-lead, Tests.".
ASK_LINE = re.compile(r"^[ \t]*(?:#+[ \t]*|[-*+][ \t]+|>[ \t]*)?\**[ \t]*TWO READS\**[ \t]*(?::|-|\u2013|\u2014|\(|$)", re.M)
ASK_READERS = re.compile(r"\bTWO READS\**[ \t]*:\**[ \t]*\S")


# 7 Nov (#5635, the PM): fail closed. The patterns above failed open: #5572 and #5580 merged on one read, and #5606,
# #5579 and #5633 were stopped by hand, each with TWO READS in its body's prose or on a READ ASK's second line. Now a
# body (or a writer's READ ASK) that names TWO READS anywhere asks; a false match only costs a second read. A line that
# opens with ONE READ (after the same marks as ASK_LINE), followed by ":", a dash, "(" or the line's end, opts out of a
# mere mention ("ONE READ: the body names the rule only"); it never clears a line that asks (ASK_LINE, ASK_READERS).
NAMES = re.compile(r"\bTWO\s+READS\b")
# The Reviewer-lead's P2 on #5636: a dash followed by a letter is no separator, so "- ONE READ-OK covers the docs half"
# opts nothing out.
OPT_OUT = re.compile(r"^[ \t]*(?:#+[ \t]*|[-*+][ \t]+|>[ \t]*)?\**[ \t]*ONE READ\**[ \t]*"
                     r"(?::|[-\u2013\u2014](?![A-Za-z])|\(|$)", re.M)
# The Reviewer-lead's P3 on #5636: a READ ASK is a comment whose first line asks with a commit ("READ ASK <sha>", after
# a signature or not), as joining.md's markers say; a READ-OK or a note that mentions "the READ ASK" asks nothing.
READ_ASK = re.compile(r"\bREAD ASK\**[ \t]+`?[0-9a-f]{7,40}\b")


def body_asks(body):
    """Whether a PR body (or a writer's READ ASK) asks for two reads: a line that asks, or TWO READS named anywhere
    without an opt-out line."""
    if ASK_LINE.search(body) or ASK_READERS.search(body):
        return True
    return bool(NAMES.search(body)) and not OPT_OUT.search(body)


# 4 Nov 23:4x (Thirteen's P3 on #5305); 7 Nov (#5635): the bot's log says, once per head (`reads2.py --unasked`), a
# body that names TWO READS and opts out with a ONE READ line, so an opt-out is seen before a train takes it on one read.
def body_names_unasked(body):
    """Whether a PR body names TWO READS but opts out (so body_asks is False)."""
    return bool(NAMES.search(body)) and not body_asks(body)


def two_reads(items, heads, writers):
    """items: dicts {login, body} in posting order, the PR body as {pr_body: true}. heads: the head sha, or a list of it
    and earlier heads with the same patch. Returns "none", "met N" or "short N"."""
    heads = [heads] if isinstance(heads, str) else list(heads)
    asked = False; signed = {}; unsigned = set()
    for c in items:
        body = c["body"].lstrip()
        # 3 Nov 14:1x: the PR body is read too (a line {"pr_body": true}). 14:5x: anywhere in the body: #5049 says
        # "TWO READS: Reviewer-lead, Tests." at character 3518, and train 558 took it on one READ-OK.
        if c.get("pr_body"):
            asked = asked or body_asks(c["body"])
            continue
        if not is_writer(c, writers):
            continue
        # 7 Nov (#5635): or a READ ASK that names TWO READS anywhere, as a body does.
        if re.match(r"\**TWO READS\b", body) or (READ_ASK.search(body.split("\n", 1)[0]) and body_asks(body)):
            asked = True
        mk = parse_marker(body)
        if not mk or mk[0] != "READ-OK" or not mk[1] or not any(h.startswith(mk[1]) for h in heads):
            continue
        login = c["login"].lower(); name = signer(body).lower()
        if name: signed.setdefault(login, set()).add(name)
        else: unsigned.add(login)
    n = sum(len(s) for s in signed.values()) + len(unsigned - set(signed))
    return "none" if not asked else ("met %d" % n if n >= 2 else "short %d" % n)


def main():
    if sys.argv[1:] == ["--unasked"]:
        # stdin: the PR body line (lib.sh's coord_body). Prints "unasked" or nothing.
        items = [json.loads(line) for line in sys.stdin if line.strip()]
        if any(c.get("pr_body") and body_names_unasked(c["body"]) for c in items):
            print("unasked")
        return
    writers, args = writers_from_args(sys.argv[1:], "reads2")
    items = [json.loads(line) for line in sys.stdin if line.strip()]
    print(two_reads(items, args, writers))


if __name__ == "__main__":
    main()
