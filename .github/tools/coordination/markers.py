# markers.py (23 Oct 10:1x): a PR's review markers, read from its comments as JSON lines {login, body} on stdin.
# A comment's first line `READ-OK <sha>` or `HOLD ...` (or, in its first 400 characters, a marker with a commit right after
# it); a HOLD is keyed on the account plus the signature in the comment's first 800 characters ("reviewer-two, 09:16"),
# and stands until that same reader posts a later READ-OK (the train bot's own "HOLD train N" clears on any later
# READ-OK marker for a commit). Prints "HELD", "READ-OK <sha>" (the newest), or nothing.
# 13:2x: Sixth's HOLD on #3206 ("Sixth, read at …", no time) and the reviewer-lead's later READ-OK ("Reviewer-lead: …") came from
# one account with no signature found, so one cleared the other and #3206 merged held. Now the line after the marker
# signs too ("Sixth," / "Reviewer-lead:"), and a HOLD that still has no signature stands until that account posts a
# READ-OK saying the HOLD is lifted or cleared (13:2x, #3138).
#
# 3 Nov 15:4x (the Reviewer-lead's HOLD on #5068): the repo is public, so only accounts with write access count. Every
# comment from another login is skipped: its READ-OK, its HOLD, its lifting words. The allowed logins come from a file,
# one a line (# lines ignored): `--writers FILE`, else $LAMP_COORD_WRITERS. With neither, or an unreadable file, no login
# is allowed and nothing is printed (fail closed). The train bot builds the file (lib.sh, coord_writers).
#
# The parsing below (parse_marker, signer, LIFT_RE, load_writers) is imported by reads2.py and qholds.py, so the
# three read a marker the same way.
import json, os, re, sys

# A READ-OK saying an unsigned HOLD (or a Queue HOLD) is lifted.
# 3 Nov 10:1x: "This clears my HOLD on `37b16e9d6f`" (Fifth on #4967) didn't match, and #4967 sat out trains
# 539-547 with both READ-OKs; "clears/cleared my … HOLD" counts too.
LIFT_RE = re.compile(r"(?i)\bHOLD\b[^.\n]{0,24}\b(lifted|cleared)\b|\b(?:lift|clear)(?:ed|s|ing)? my (?:[0-9:x]+ )?HOLD")


# 3 Nov 16:4x (Thirteen's HOLD on #5068): the opening `\W*` skipped "> " and a code fence, so an author's
# "> READ-OK <head>" reply, "thanks, waiting for a second read", counted as a second read. A marker on a line that starts
# with ">" (a Markdown quote), or inside a fenced code block (``` or ~~~, open at the marker's line), is never a marker.
FENCE_RE = re.compile(r" {0,3}(`{3,}|~{3,})(.*)")


def quoted(body, pos):
    """True when the character at pos sits on a quoted line ("> ...") or inside a fenced code block."""
    start = body.rfind("\n", 0, pos) + 1
    if re.match(r"[ \t]*>", body[start:]):
        return True
    fence = None  # (char, length) of the open fence
    for line in body[:start].split("\n"):
        f = FENCE_RE.match(line)
        if not f:
            continue
        mark = f.group(1)
        if fence is None:
            # An opening backtick fence's info string holds no backtick (CommonMark).
            if mark[0] == "~" or "`" not in f.group(2):
                fence = (mark[0], len(mark))
        elif mark[0] == fence[0] and len(mark) >= fence[1] and not f.group(2).strip():
            fence = None
    # The marker's own line opening a fence ("```READ-OK ...") is code too.
    return fence is not None or bool(FENCE_RE.match(body[start:]))


def parse_marker(body):
    """The comment's marker: (kind, sha, rest, first_line) or None. kind is READ-OK or HOLD; sha may be "";
    first_line is True when the marker opens the comment."""
    body = body.lstrip()
    m = re.match(r"\W*(READ-OK|HOLD)\b\W*(?:#\d+\W+)?([0-9a-f]{7,40})?\s*(.*)", body)
    if m and not quoted(body, m.start(1)):
        return m.group(1), m.group(2) or "", m.group(3), True
    # A marker after a signature ("reviewer-two, 10:08, to ...: HOLD 4931fff80 ..."), in the first 400 characters,
    # counts only with a commit right after it, so prose like "my HOLD is cleared" never does.
    # 25 Oct 04:0x: nor a marker named as a noun, another's ("your HOLD `889b694ea` asked for", #3371) or one replied
    # to by its comment's id ("the records for HOLD 6385086855", #3387): each held its PR under the replier's name.
    # 3 Nov 15:4x: nor a possessive's ("the Reviewer-lead's READ-OK e2b57f1f48 was on this head", the Reviewer-lead's
    # 15:14 read of #5068): it counted as the writer's own READ-OK and cleared the bot's "HOLD train N".
    m = next((x for x in re.finditer(r"\b(READ-OK|HOLD)\*{0,2}\s+(?:#\d+\s+)?`?([0-9a-f]{9,40})`?\b\s*(.*)", body[:400])
              if not x.group(2).isdigit()
              and not re.search(r"(?i)(\b(your|for|the|that|this|their|his|her|its)|['’]s)\s+\**$", body[:x.start()])
              # 27 Oct 05:3x: nor one in brackets, the HOLD a reply answers ("Friday, to Reviewer-two (HOLD
              # 3e741c220): new head ..."): three such replies held #3608 under Friday's name after both READ-OKs.
              and not re.search(r"[(\[]\**`?$", body[:x.start()])
              # 27 Oct 20:1x: nor one quoted, in backticks or quotation marks ("your 19:34 comment opens with
              # `HOLD 73aa8b23f`", the reviewer-lead explaining a hold): it held #3781 under the explainer's name.
              # 3 Nov 16:4x: \Z, not $: a closing fence ("```" then a newline) right before a marker isn't a quote.
              and not re.search(r"[`\"“']\**\Z", body[:x.start()])
              # 3 Nov 16:4x: nor one on a quoted line ("> READ-OK <sha>") or in a fenced code block (Thirteen, #5068).
              and not quoted(body, x.start(1))), None)
    # 28 Oct 13:1x: "Reviewer-lead, READ-OK #4006 56d1d90d5…" names the PR before the commit; it counts (#4006).
    if m:
        return m.group(1), m.group(2), m.group(3), False
    return None


# Words that open a line of a read and are not a reader's name ("Tests: 12 passed", "Why: ...", "Fail-first: ...").
NOT_NAMES = {"tests", "test", "gates", "gate", "checks", "check", "note", "notes", "nit", "nits", "fail-first", "why",
             "what", "how", "fixed", "fix", "fixes", "changes", "change", "summary", "result", "results", "ran", "run",
             "build", "files", "scope", "verified", "findings", "also", "and", "but", "then", "head", "commit", "diff"}


def signer(body):
    """The signed name in a comment (as written; callers lowercase it), or ""."""
    body = body.lstrip()
    # 3 Nov 10:1x: "The first builder, 18:5x" signs too (an optional "The"); #4905 sat out with my lifting READ-OK.
    # The author's signature: "<Name>, HH:MM" at the comment's or a line's start, after a sentence, or right after the
    # marker and its commit; never a name quoted in brackets ("(Fourteen, 09:3x: ...)").
    sig = re.search(r"(?:^|\n|\. |(?:READ-OK|HOLD)\*{0,2} `?[0-9a-f]{7,40}`?[:,]? )\**(?:[Tt]he )?([A-Za-z][A-Za-z-]+(?: builder)?)\**,? \d{1,2}:\d[\dx]", body[:800])
    if sig:
        return sig.group(1)
    # 29 Oct 18:1x: the name right after the marker and its commit on the same line, with no time:
    # "HOLD 3559c8ab3a…  Reviewer-lead: one P2" (#4364, #4346 sat out on it as unsigned).
    # 3 Nov 16:4x: on the same line only ([ \t]+, not \s+); the next line is nxt's, with its label check.
    same = re.search(r"(?:READ-OK|HOLD)\*{0,2} `?[0-9a-f]{7,40}`?[:,]?[ \t]+\**([A-Za-z][A-Za-z-]+(?: builder)?)\**\s*[,:]", body[:400])
    # The name that starts the line after the marker's line: "Sixth, read at …", "Reviewer-lead: my P3 …".
    # 3 Nov 16:4x (the Reviewer-lead's note on #5068, Thirteen reproduced it): "Tests: 12 passed" there became a second
    # reader. Now the name is a capitalised word or "<word> builder", and never a label a read's lines open with
    # (NOT_NAMES). A name with a time after it ("Tests, 10:02") is the signature above, label or not.
    nxt = re.search(r"\A[^\n]*\n\s*\**(?:[Tt]he )?([A-Z][A-Za-z-]+|[A-Za-z][A-Za-z-]+ builder)\**\s*[,:]", body)
    if nxt and nxt.group(1).lower() in NOT_NAMES:
        nxt = None
    # 30 Oct 22:0x: the name that opens the comment, with a colon and no time: "Sixth: #4562 at `b9521db47a`, …" then
    # "HOLD b9521db47a" a line below. It read as unsigned, and Sixth's "Sixth: … my re-read of my HOLD" READ-OK didn't
    # clear it, so #4562 sat out train 423 with both READ-OKs.
    first = re.match(r"\**([A-Za-z][A-Za-z-]+(?: builder)?)\**:", body)
    return same.group(1) if same else (nxt.group(1) if nxt else (first.group(1) if first else ""))


def load_writers(path):
    """The allowed logins (lowercase) from a file, one a line; an empty set when there is no file (fail closed)."""
    if not path:
        return set()
    try:
        with open(path) as f:
            return {l.strip().lower() for l in f if l.strip() and not l.lstrip().startswith("#")}
    except OSError:
        return set()


def writers_from_args(argv, tool="markers"):
    """(writers, the other arguments): `--writers FILE` in argv, else $LAMP_COORD_WRITERS."""
    rest, path = [], os.environ.get("LAMP_COORD_WRITERS", "")
    i = 0
    while i < len(argv):
        if argv[i] == "--writers" and i + 1 < len(argv):
            path = argv[i + 1]; i += 2; continue
        rest.append(argv[i]); i += 1
    writers = load_writers(path)
    if not writers:
        print(tool + ": no writers (--writers FILE or LAMP_COORD_WRITERS): every comment is skipped", file=sys.stderr)
    return writers, rest


def is_writer(c, writers):
    return (c.get("login") or "").lower() in writers


def read_markers(comments, writers, pr=None, lifted_path=None):
    """comments: dicts {login, body} in posting order. Returns "HELD", "READ-OK <sha>" or ""."""
    holds = {}; ok = ""
    for n, c in enumerate(comments):
        # The index n counts every comment, a skipped one too, so holds-lifted.tsv's "unsigned#N" keys stay put.
        if not is_writer(c, writers):
            continue
        body = c["body"].lstrip()
        mk = parse_marker(body)
        if not mk:
            continue
        kind, sha, rest, _ = mk
        name = signer(body)
        who = c["login"] + "/" + name.lower()
        if kind == "HOLD":
            if rest.startswith("train "): holds["bot"] = sha
            # Unsigned: nobody's later READ-OK can be told to be its reader's, so it stands (fail closed).
            else: holds[who if name else c["login"] + "/unsigned#" + str(n)] = sha
        else:
            ok = sha; holds.pop(who, None)
            # 3 Nov 15:4x: the bot's own "HOLD train N" clears only on a READ-OK marker for a commit; a quoted,
            # bracketed or possessive READ-OK never parses as a marker above.
            if sha: holds.pop("bot", None)
            # An account's unsigned HOLD clears only on that account's READ-OK saying so ("my P2 is fixed; HOLD lifted", #3138).
            if LIFT_RE.search(body[:400]):
                for k in [k for k in holds if k.startswith(c["login"] + "/unsigned#")]: holds.pop(k)
    # 29 Oct 18:0x: "HOLD b9bb97e67a, Third, 16:45" (a comma after the commit) read as unsigned, and "this lifts my 16:45 HOLD"
    # didn't clear it, so #4353 sat out trains 363 and 364 with three READ-OKs. Both read now.
    # 25 Oct 20:0x: a HOLD its own reader closed in writing but never cleared with a READ-OK (Second's on #3437, closed in
    # words at 11:39, then six silent hours) is lifted only by a line in holds-lifted.tsv: PR, key, commit prefix and the
    # reader's closing comment, posted on the PR when added. Given the PR number; without it, nothing is lifted.
    # 3 Nov: the file is state, not code (its keys are account logins): $HOLDS_LIFTED, else holds-lifted.tsv in
    # $LAMP_COORD_STATE (default ~/.lamp-coordination). The repo ships holds-lifted.tsv.example with the format.
    if pr is not None:
        lifted = lifted_path or os.environ.get("HOLDS_LIFTED") or os.path.join(
            os.environ.get("LAMP_COORD_STATE") or os.path.expanduser("~/.lamp-coordination"), "holds-lifted.tsv")
        try:
            with open(lifted) as rows:
                for row in rows:
                    f = row.rstrip("\n").split("\t")
                    if len(f) >= 4 and f[0] == str(pr) and holds.get(f[1], "").startswith(f[2]): holds.pop(f[1])
        except FileNotFoundError: pass
    return "HELD" if holds else ("READ-OK " + ok if ok else "")


def main():
    # markers.py [--writers FILE] [PR]
    writers, args = writers_from_args(sys.argv[1:])
    comments = [json.loads(line) for line in sys.stdin if line.strip()]
    print(read_markers(comments, writers, pr=args[0] if args else None))


if __name__ == "__main__":
    main()
