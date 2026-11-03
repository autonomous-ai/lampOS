# markers.py (23 Oct 10:1x): a PR's review markers, read from its comments as JSON lines {login, body} on stdin.
# A comment's first line `READ-OK <sha>` or `HOLD ...` (or, in its first 400 characters, a marker with a commit right after
# it); a HOLD is keyed on the account plus the signature in the comment's first 800 characters ("reviewer-two, 09:16"),
# and stands until that same reader posts a later READ-OK (the train bot's own "HOLD train N" clears on any later
# READ-OK). Prints "HELD", "READ-OK <sha>" (the newest), or nothing.
# 13:2x: Sixth's HOLD on #3206 ("Sixth, read at …", no time) and the reviewer-lead's later READ-OK ("Reviewer-lead: …") came from
# one account with no signature found, so one cleared the other and #3206 merged held. Now the line after the marker
# signs too ("Sixth," / "Reviewer-lead:"), and a HOLD that still has no signature stands until that account posts a
# READ-OK saying the HOLD is lifted or cleared (13:2x, #3138).
import json, re, sys
holds = {}; ok = ""
for n, line in enumerate(sys.stdin):
    c = json.loads(line); body = c["body"].lstrip()
    m = re.match(r"\W*(READ-OK|HOLD)\b\W*(?:#\d+\W+)?([0-9a-f]{7,40})?\s*(.*)", body)
    if not m:
        # A marker after a signature ("reviewer-two, 10:08, to ...: HOLD 4931fff80 ..."), in the first 400 characters,
        # counts only with a commit right after it, so prose like "my HOLD is cleared" never does.
        # 25 Oct 04:0x: nor a marker named as a noun, another's ("your HOLD `889b694ea` asked for", #3371) or one replied
        # to by its comment's id ("the records for HOLD 6385086855", #3387): each held its PR under the replier's name.
        m = next((x for x in re.finditer(r"\b(READ-OK|HOLD)\*{0,2}\s+(?:#\d+\s+)?`?([0-9a-f]{9,40})`?\b\s*(.*)", body[:400])
                  if not x.group(2).isdigit()
                  and not re.search(r"(?i)\b(your|for|the|that|this|their)\s+\**$", body[:x.start()])
                  # 27 Oct 05:3x: nor one in brackets, the HOLD a reply answers ("Friday, to Reviewer-two (HOLD
                  # 3e741c220): new head ..."): three such replies held #3608 under Friday's name after both READ-OKs.
                  and not re.search(r"[(\[]\**`?$", body[:x.start()])
                  # 27 Oct 20:1x: nor one quoted, in backticks or quotation marks ("your 19:34 comment opens with
                  # `HOLD 73aa8b23f`", the reviewer-lead explaining a hold): it held #3781 under the explainer's name.
                  and not re.search(r"[`\"\u201c']\**$", body[:x.start()])), None)
    # 28 Oct 13:1x: "Reviewer-lead, READ-OK #4006 56d1d90d5…" names the PR before the commit; it counts (#4006).
    if not m: continue
    kind, sha, rest = m.group(1), m.group(2) or "", m.group(3)
    # 3 Nov 10:1x: "The first builder, 18:5x" signs too (an optional "The"); #4905 sat out with my lifting READ-OK.
    # The author's signature: "<Name>, HH:MM" at the comment's or a line's start, after a sentence, or right after the
    # marker and its commit; never a name quoted in brackets ("(Fourteen, 09:3x: ...)").
    sig = re.search(r"(?:^|\n|\. |(?:READ-OK|HOLD)\*{0,2} `?[0-9a-f]{7,40}`?[:,]? )\**(?:[Tt]he )?([A-Za-z][A-Za-z-]+(?: builder)?)\**,? \d{1,2}:\d[\dx]", body[:800])
    if not sig:
        # 29 Oct 18:1x: the name right after the marker and its commit on the same line, with no time:
        # "HOLD 3559c8ab3a…  Reviewer-lead: one P2" (#4364, #4346 sat out on it as unsigned).
        same = re.search(r"(?:READ-OK|HOLD)\*{0,2} `?[0-9a-f]{7,40}`?[:,]?\s+\**([A-Za-z][A-Za-z-]+(?: builder)?)\**\s*[,:]", body[:400])
        # The name that starts the line after the marker's line: "Sixth, read at …", "Reviewer-lead: my P3 …".
        nxt = re.search(r"\A[^\n]*\n\s*\**([A-Za-z][A-Za-z-]+(?: builder)?)\**\s*[,:]", body)
        # 30 Oct 22:0x: the name that opens the comment, with a colon and no time: "Sixth: #4562 at `b9521db47a`, …" then
        # "HOLD b9521db47a" a line below. It read as unsigned, and Sixth's "Sixth: … my re-read of my HOLD" READ-OK didn't
        # clear it, so #4562 sat out train 423 with both READ-OKs.
        first = re.match(r"\**([A-Za-z][A-Za-z-]+(?: builder)?)\**:", body)
        name = same.group(1) if same else (nxt.group(1) if nxt else (first.group(1) if first else ""))
    else:
        name = sig.group(1)
    who = c["login"] + "/" + name.lower()
    if kind == "HOLD":
        if rest.startswith("train "): holds["bot"] = sha
        # Unsigned: nobody's later READ-OK can be told to be its reader's, so it stands (fail closed).
        else: holds[who if name else c["login"] + "/unsigned#" + str(n)] = sha
    else:
        ok = sha; holds.pop(who, None); holds.pop("bot", None)
        # An account's unsigned HOLD clears only on that account's READ-OK saying so ("my P2 is fixed; HOLD lifted", #3138).
        # 3 Nov 10:1x: "This clears my HOLD on `37b16e9d6f`" (Fifth on #4967) didn't match, and #4967 sat out trains
        # 539-547 with both READ-OKs; "clears/cleared my … HOLD" counts too.
        if re.search(r"(?i)\bHOLD\b[^.\n]{0,24}\b(lifted|cleared)\b|\b(?:lift|clear)(?:ed|s|ing)? my (?:[0-9:x]+ )?HOLD", body[:400]):
            for k in [k for k in holds if k.startswith(c["login"] + "/unsigned#")]: holds.pop(k)
# 29 Oct 18:0x: "HOLD b9bb97e67a, Third, 16:45" (a comma after the commit) read as unsigned, and "this lifts my 16:45 HOLD"
# didn't clear it, so #4353 sat out trains 363 and 364 with three READ-OKs. Both read now.
# 25 Oct 20:0x: a HOLD its own reader closed in writing but never cleared with a READ-OK (Second's on #3437, closed in
# words at 11:39, then six silent hours) is lifted only by a line in holds-lifted.tsv: PR, key, commit prefix and the
# reader's closing comment, posted on the PR when added. Given the PR number as argv[1]; without it, nothing is lifted.
# 3 Nov: the file is state, not code (its keys are account logins): $HOLDS_LIFTED, else holds-lifted.tsv in
# $LAMP_COORD_STATE (default ~/.lamp-coordination). The repo ships holds-lifted.tsv.example with the format.
if len(sys.argv) > 1:
    import os
    lifted = os.environ.get("HOLDS_LIFTED") or os.path.join(
        os.environ.get("LAMP_COORD_STATE") or os.path.expanduser("~/.lamp-coordination"), "holds-lifted.tsv")
    try:
        for row in open(lifted):
            f = row.rstrip("\n").split("\t")
            if len(f) >= 4 and f[0] == sys.argv[1] and holds.get(f[1], "").startswith(f[2]): holds.pop(f[1])
    except FileNotFoundError: pass
print("HELD" if holds else ("READ-OK " + ok if ok else ""))
