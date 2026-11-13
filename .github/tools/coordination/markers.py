# markers.py (23 Oct 10:1x): a PR's review markers, read from its comments as JSON lines {login, body} on stdin.
# 4 Nov: with its reviews' bodies merged in by time (lib.sh's coord_pr_comments, merge_reviews below; #5212 part 1).
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
# The parsing below (parse_marker, signer, lifted_commits, load_writers) is imported by reads2.py and qholds.py, so the
# three read a marker the same way.
import json, os, re, sys

# A READ-OK saying an unsigned HOLD (or a Queue HOLD) is lifted.
# 3 Nov 10:1x: "This clears my HOLD on `37b16e9d6f`" (Fifth on #4967) didn't match, and #4967 sat out trains
# 539-547 with both READ-OKs; "clears/cleared my … HOLD" counts too.
# 6 Nov (#5534): "My HOLD on 1d3d9711f8ce871e21483fd840f57b3a5570d174 is lifted" (the Reviewer-lead on #5521, also #5476
# and #5486) didn't match: a full commit is 40 characters and the window was 24. A commit (7 to 40 hex, not all
# digits) is read as one short word here, so the window fits it, and the commits the lifting words name are returned.
COMMIT_RE = re.compile(r"`?\b(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b`?")
LIFT_RE = re.compile(r"(?i)\bHOLD\b[^.\n]{0,24}\b(lifted|cleared)\b|\b(?:lift|clear)(?:ed|s|ing)? my (?:[0-9:x]+ )?HOLD(?: (?:on|at) \S+)?")


def lifted_commits(text):
    """The lifting words in `text`: None when it says no HOLD is lifted, else the commits they name (an empty list:
    no commit named, as before #5534)."""
    commits = []
    def short(m):
        commits.append(m.group(0).strip("`"))
        return "\x00%d\x00" % (len(commits) - 1)
    plain = COMMIT_RE.sub(short, text)
    found = None
    for m in LIFT_RE.finditer(plain):
        if negated(plain, m):
            continue
        found = found or []
        found += [commits[int(i)] for i in re.findall(r"\x00(\d+)\x00", m.group(0))]
    return found


# 6 Nov (the reviewer's P3 on #5542): "my HOLD on <A> is not lifted", "isn't lifted", "is not yet cleared", "won't lift my
# HOLD", "not lifting my HOLD" matched LIFT_RE and lifted A. Lifting words with "not", "n't" or "never" inside them, or
# in the same clause right before them, lift nothing.
NEG_RE = re.compile(r"(?i)\b(?:not|never)\b|n['’]t\b")


def negated(plain, m):
    clause = re.split(r"[.;:!?\n]", plain[max(0, m.start() - 16):m.start()])[-1]
    return bool(NEG_RE.search(clause + m.group(0)))


def names_commit(sha, commits):
    """Whether a HOLD on `sha` is one of `commits` (either may be a prefix of the other). A HOLD with no commit is none."""
    return bool(sha) and any(sha.startswith(x) or x.startswith(sha) for x in commits)


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
    # 14 Nov 00:5x (Heron's and SubDarren's HOLDs on #6979): here only, a name may carry digits after its first letter
    # ("sim-3, 22:5x"), so the sim builders sign; a priority or check label with a time ("P1, 22:50") never does. The
    # no-time paths below keep letters only, so "P3: …", "ARM64: green" and "H2: x" opening a line sign no one.
    sig = re.search(r"(?:^|\n|\. |(?:READ-OK|HOLD)\*{0,2} `?[0-9a-f]{7,40}`?[:,]? )\**(?:[Tt]he )?([A-Za-z][A-Za-z0-9-]+(?: builder)?)\**,? \d{1,2}:\d[\dx]", body[:800])
    if sig and not re.fullmatch(r"(?i)p\d+s?|h\d+|arm\d+|x86(-\d+)?|ipv\d", sig.group(1)):
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
    holds, ok = standing_holds(comments, writers, pr, lifted_path)
    return "HELD" if holds else ("READ-OK " + ok if ok else "")


def standing_holds(comments, writers, pr=None, lifted_path=None):
    """The HOLDs still standing ({key: commit}) and the newest READ-OK's commit ("" if none)."""
    holds = {}; ok = ""; n = -1
    for c in comments:
        # The index n counts every comment, a skipped one too, so holds-lifted.tsv's "unsigned#N" keys stay put.
        # 4 Nov (#5212 part 1): reviews merged in (merge_reviews) don't count, so a review never shifts a comment's N.
        if not c.get("review"): n += 1
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
            # An unsigned review HOLD is keyed on its submit time: "<login>/unsigned-review#<submitted_at>".
            elif name: holds[who] = sha
            elif c.get("review"): holds[c["login"] + "/unsigned-review#" + (c.get("created_at") or "")] = sha
            else: holds[c["login"] + "/unsigned#" + str(n)] = sha
        else:
            ok = sha; holds.pop(who, None)
            # 3 Nov 15:4x: the bot's own "HOLD train N" clears only on a READ-OK marker for a commit; a quoted,
            # bracketed or possessive READ-OK never parses as a marker above.
            if sha: holds.pop("bot", None)
            # An account's unsigned HOLD clears only on that account's READ-OK saying so ("my P2 is fixed; HOLD lifted", #3138).
            # 6 Nov (#5534): lifting words that name a commit clear only that account's HOLDs on it.
            lift = lifted_commits(body[:400])
            if lift is not None:
                unsigned = (c["login"] + "/unsigned#", c["login"] + "/unsigned-review#")
                for k in [k for k in holds if k.startswith(unsigned) and (not lift or names_commit(holds[k], lift))]:
                    holds.pop(k)
                # 6 Nov (the reviewer's P3 on #5542): an unsigned READ-OK never lifts a signed HOLD, even the only one on
                # the commit: on a shared account it may be another reader's ("my HOLD on <A> is lifted" from reader Y
                # cleared reader X's). A signed HOLD is lifted only by its own signer's READ-OK (holds.pop(who) above).
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
    return holds, ok


def held_note(comments, writers, pr=None, lifted_path=None):
    """6 Nov (#5534): for a held PR, "<READ-OK commit> <key>=<commit> ..." (the newest READ-OK, then each standing
    HOLD), so the train bot can say why a PR with a READ-OK sits out; "" when nothing is held or there is no READ-OK."""
    holds, ok = standing_holds(comments, writers, pr, lifted_path)
    if not holds or not ok:
        return ""
    return " ".join([ok] + ["%s=%s" % (k, v[:10] or "-") for k, v in sorted(holds.items())])


def ignored_reads(comments, writers):
    """6 Nov (the PM's ask on #5542): the READ-OK markers skipped because their login is not a writer, which was silent,
    as lines "READ-OK <sha7> by <login> ignored: not a writer" (oldest first, one per login and commit)."""
    out = []
    for c in comments:
        if is_writer(c, writers):
            continue
        mk = parse_marker(c.get("body") or "")
        if mk and mk[0] == "READ-OK" and mk[1]:
            line = "READ-OK %s by %s ignored: not a writer" % (mk[1][:7], c.get("login") or "?")
            if line not in out: out.append(line)
    return out


def merge_reviews(comments, reviews):
    """4 Nov (#5212 part 1): a PR's comments plus its reviews' bodies, as one list in time order. reviews: dicts
    {login, body, created_at (submitted_at), state}. A review with an empty body, not submitted (PENDING) or
    DISMISSED is dropped; its state is never a read, only its body's marker, read as a comment's. The sort is stable, so items with
    the same time keep comments before reviews."""
    items = list(comments)
    for r in reviews:
        if not (r.get("body") or "").strip() or not r.get("created_at") or r.get("state") in ("PENDING", "DISMISSED"):
            continue
        items.append({"login": r.get("login") or "", "body": r["body"], "created_at": r["created_at"], "review": True})
    return sorted(items, key=lambda c: c.get("created_at") or "")


def main():
    # markers.py --merge COMMENTS_JSONL REVIEWS_JSONL: prints merge_reviews' list as JSON lines (lib.sh).
    if sys.argv[1:2] == ["--merge"]:
        lists = []
        for path in sys.argv[2:4]:
            with open(path) as f:
                lists.append([json.loads(line) for line in f if line.strip()])
        for c in merge_reviews(*lists):
            print(json.dumps(c))
        return
    # markers.py [--writers FILE] [--held|--ignored] [PR]; --held prints held_note's line instead (#5534), --ignored
    # ignored_reads' lines, one a line.
    held = "--held" in sys.argv[1:]
    ignored = "--ignored" in sys.argv[1:]
    writers, args = writers_from_args([a for a in sys.argv[1:] if a not in ("--held", "--ignored")])
    comments = [json.loads(line) for line in sys.stdin if line.strip()]
    pr = args[0] if args else None
    if ignored:
        for line in ignored_reads(comments, writers): print(line)
        return
    print(held_note(comments, writers, pr=pr) if held else read_markers(comments, writers, pr=pr))


if __name__ == "__main__":
    main()
