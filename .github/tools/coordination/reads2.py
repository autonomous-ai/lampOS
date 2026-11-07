# reads2.py (31 Oct 03:1x, first builder): whether a PR asked for two reads is met, read from its comments as JSON lines
# {login, body} on stdin. argv: [--writers FILE] [--queue QUEUE_JSONL --pr N] HEAD [EARLIER_HEAD...]. A comment whose first line is "TWO READS" (bold allowed) asks
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


# 7 Nov (#5635, the PM): fail closed. The patterns above failed open: #5580 merged on one read, and #5606, #5579 and
# #5633 were stopped by hand, each with TWO READS in its body's prose or on a READ ASK's second line. (#5572 merged on
# one read too, but its second reader was asked on the Queue, which reads2.py doesn't read.) Now a body (or a writer's
# READ ASK) that names TWO READS, in capitals, anywhere asks; a false match only costs a second read. A line that opens
# with ONE READ (after an optional heading, list or bold mark, not a quote mark), followed by ":", a dash, "(" or the
# line's end, opts out of a mere mention in its own text ("ONE READ: the body names the rule only"); it never clears a
# line that asks (ASK_LINE, ASK_READERS). El's P3 on #5636: an opt-out line in a quote or a fence (``` or ~~~) is
# someone else's text, so it opts nothing out.
NAMES = re.compile(r"\bTWO\s+READS\b")
# The Reviewer-lead's P2 on #5636: a dash followed by a letter is no separator, so "- ONE READ-OK covers the docs half"
# opts nothing out.
# El's P3 on #5643: at most three spaces before it, as four spaces or a tab make an indented code block.
OPT_OUT = re.compile(r"^ {0,3}(?:#+[ \t]*|[-*+][ \t]+)?(?:\*+[ \t]*)?ONE READ\**[ \t]*"
                     r"(?::|[-\u2013\u2014](?![A-Za-z])|\(|$)", re.M)
# The Reviewer-lead's P3 on #5636: a READ ASK is a comment whose first line asks with a commit ("READ ASK <sha>", after
# a signature or not), as joining.md's markers say; a READ-OK or a note that mentions "the READ ASK" asks nothing.
READ_ASK = re.compile(r"\bREAD ASK\**[ \t]+`?[0-9a-f]{7,40}\b")


def body_asks(body):
    """Whether a PR body (or a writer's READ ASK) asks for two reads: a line that asks, or TWO READS named anywhere
    without an opt-out line."""
    if ASK_LINE.search(body) or ASK_READERS.search(body):
        return True
    return bool(NAMES.search(body)) and not OPT_OUT.search(unfenced(body))


def unfenced(body):
    """The body without its fenced blocks (``` or ~~~); an unclosed fence runs to the end. El's P3 on #5643: as in
    CommonMark, a fence closes only on a line of its own character, at least as long as the opener, with nothing but
    spaces after it, so "```" inside a "````" fence, or "```python" inside a "```" one, closes nothing."""
    out, fence = [], None
    for line in body.split("\n"):
        if fence is None:
            m = re.match(r" {0,3}(`{3,}|~{3,})", line)
            if m:
                fence = m.group(1)
            else:
                out.append(line)
        elif re.fullmatch(r" {0,3}" + re.escape(fence[0]) + "{%d,}[ \t]*\r?" % len(fence), line):
            fence = None
    return "\n".join(out)


# 4 Nov 23:4x (Thirteen's P3 on #5305); 7 Nov (#5635): the bot's log says, once per head (`reads2.py --unasked`), a
# body that names TWO READS and opts out with a ONE READ line, so an opt-out is seen before a train takes it on one read.
def body_names_unasked(body):
    """Whether a PR body names TWO READS but opts out (so body_asks is False)."""
    return bool(NAMES.search(body)) and not body_asks(body)


# 7 Nov (the PM, after Twelve's table of the PRs merged since 10579): #5647 and #5650 were asked TWO READS only on the
# Queue ("READ ASK <sha>" then "TWO READS.") and each merged on one read. A writer's Queue comment asks for a PR when its
# first line opens with READ ASK (or has it right after a signature's colon) and names the PR, by a commit its head (or a same-patch head) starts with, or by "#N", and
# the comment names TWO READS as a body does (body_asks, with its opt-out). The bot passes the Queue (--queue, lib.sh's
# coord_queue: every Queue issue's last 3 days) and the PR's number (--pr).
# 7 Nov (the second reviewer's HOLD on #5653): a sha also asks when it names any commit of the PR (--pr-commits, one sha a
# line: the bot's git rev-list origin/lamp-voice..HEAD), so an ask at a head a later fix moved past (another patch) still
# asks. Those commits are used only to match Queue asks: which READ-OKs count is still decided by the heads alone.
QUEUE_SHA = re.compile(r"\b[0-9a-f]{7,40}\b")
# The marker opens the line or follows a signature's colon ("El, 05:18, to **Faye**: **READ ASK (restate) [#5633]") or
# comma (7 Nov, #5653: "Sixth, READ ASK to the **Reviewer-lead**: [#5570]", "Friday, 17:51, READ ASK to ..."), so
# an hourly that mentions a READ ASK in its prose ("#5650 had a READ ASK naming …") asks nothing.
QUEUE_ASK = re.compile(r"(?:^|[:,])[\s*_`]*READ ASK\b")


def queue_asks(queue, heads, pr, writers, commits=()):
    """Whether a writer's Queue READ ASK names this PR (by head, one of its commits, or number) and TWO READS."""
    shas = list(heads) + list(commits or ())
    for q in queue or ():
        first = q["body"].lstrip().split("\n", 1)[0]
        if not is_writer(q, writers) or not QUEUE_ASK.search(first):
            continue
        by_head = any(h.startswith(m) for m in QUEUE_SHA.findall(first) for h in shas)
        by_number = bool(pr) and re.search(r"#%s(?!\d)" % re.escape(str(pr)), first)
        if (by_head or by_number) and body_asks(q["body"]):
            return True
    return False


def two_reads(items, heads, writers, queue=None, pr=None, commits=()):
    """items: dicts {login, body} in posting order, the PR body as {pr_body: true}. heads: the head sha, or a list of it
    and earlier heads with the same patch. queue: the Queue's comments (queue_asks), pr: the PR's number, commits: the PR's commits (Queue asks only). Returns "none",
    "met N" or "short N"."""
    heads = [heads] if isinstance(heads, str) else list(heads)
    asked = queue_asks(queue, heads, pr, writers, commits); signed = {}; unsigned = set()
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
    queue, pr, heads, commits = None, None, [], []
    i = 0
    while i < len(args):
        if args[i] == "--queue" and i + 1 < len(args):
            with open(args[i + 1]) as f:
                queue = [json.loads(line) for line in f if line.strip()]
            i += 2
        elif args[i] == "--pr-commits" and i + 1 < len(args):
            with open(args[i + 1]) as f:
                commits = [line.strip() for line in f if line.strip()]
            i += 2
        elif args[i] == "--pr" and i + 1 < len(args):
            pr = args[i + 1]; i += 2
        else:
            heads.append(args[i]); i += 1
    items = [json.loads(line) for line in sys.stdin if line.strip()]
    print(two_reads(items, heads, writers, queue, pr, commits))


if __name__ == "__main__":
    main()
