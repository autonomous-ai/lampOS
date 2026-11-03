# reads2.py (31 Oct 03:1x, first builder): whether a PR asked for two reads is met, read from its comments as JSON lines
# {login, body} on stdin. argv: PR head sha. A comment whose first line is "TWO READS" (bold allowed) asks for two; then
# two different readers (account plus signature, as markers.py signs) must each have a READ-OK line for a commit the head
# starts with. Prints "none" (not asked), "met N" or "short N". #4633 (23:37) and #4656 (02:58) went in trains on one
# READ-OK where two had been named, since the train bot read only the newest READ-OK.
import json, re, sys
head = sys.argv[1]
asked = False; readers = set()
sig_re = re.compile(r"(?:^|\n|\. )\**([A-Za-z][A-Za-z-]+(?: builder)?)\**,? \d{1,2}:\d[\dx]")
for line in sys.stdin:
    c = json.loads(line); body = c["body"].lstrip()
    # 3 Nov 14:1x: the PR body is read too (a line {"pr_body": true}): "TWO READS" in its first 400 characters asks for
    # two. #4924 asked there ("Backend: **TWO READS ASK to …**") and went in train 556 on one READ-OK.
    if c.get("pr_body"):
        # 3 Nov 14:5x: anywhere in the body, not its first 400 characters: #5049 says "TWO READS: Reviewer-lead, Tests." at
        # character 3518, and train 558 took it on one READ-OK.
        asked = asked or bool(re.search(r"\bTWO READS\b", body))
        continue
    if re.match(r"\**TWO READS\b", body):
        asked = True
    for m in re.finditer(r"READ-OK\*{0,2}\s+`?([0-9a-f]{7,40})`?", body[:400]):
        if head.startswith(m.group(1)) or m.group(1).startswith(head[:m.end(1)-m.start(1)]):
            s = sig_re.search(body[:800]) or re.match(r"\**([A-Za-z][A-Za-z-]+(?: builder)?)\**:", body)
            readers.add(c["login"] + "/" + (s.group(1).lower() if s else "unsigned"))
print("none" if not asked else ("met %d" % len(readers) if len(readers) >= 2 else "short %d" % len(readers)))
