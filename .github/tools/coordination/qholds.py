# qholds.py [--writers FILE] HEAD QUEUE_JSONL (3 Nov 15:4x, out of train-bot.sh for the Reviewer-lead's read of #5068):
# whether a HOLD posted on the Queue holds this PR. QUEUE_JSONL holds the comments of every Queue issue (lib.sh's
# COORD_QUEUE_ISSUES: #4710, then #5385 from 5 Nov, then #6059 from 10 Nov) as JSON lines {login, body, created_at}; stdin holds the PR's
# comments the same way. Prints the created_at of the first Queue HOLD that stands, or nothing.
# 3 Nov 09:4x: a Queue comment whose first line is `HOLD <sha>` holds the PR whose head starts with that sha, until the
# same account posts a READ-OK on the PR, after it, saying the HOLD is lifted or cleared. Train 546 took #5013 on
# Fourteen's READ-OK while Seventh's "HOLD a13408fb8d…: lamp check pending" stood only on the Queue.
# 15:4x: only writers' comments count (markers.py's --writers / LAMP_COORD_WRITERS), the marker and the lifting words
# are markers.py's own (so `**READ-OK**` lifts and a quoted READ-OK doesn't), and so are its lifting words (lifted_commits).
import json, os, sys
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from markers import parse_marker, writers_from_args, is_writer, lifted_commits, names_commit


def standing(head, queue, comments, writers):
    """The created_at of the first Queue HOLD on this head that no lifting READ-OK answers, or ""."""
    for q in queue:
        if not is_writer(q, writers):
            continue
        mk = parse_marker(q["body"])
        if not mk or mk[0] != "HOLD" or not mk[3] or not mk[1] or not head.startswith(mk[1]):
            continue
        lifted = False
        for c in comments:
            if c["created_at"] <= q["created_at"] or (c.get("login") or "").lower() != q["login"].lower():
                continue
            ck = parse_marker(c["body"])
            # 6 Nov (#5534): lifting words with a full commit count; naming a commit, it must be this HOLD's.
            lift = lifted_commits(c["body"].lstrip()[:400]) if ck and ck[0] == "READ-OK" else None
            if lift is not None and (not lift or names_commit(mk[1], lift)):
                lifted = True; break
        if not lifted:
            return q["created_at"]
    return ""


def main():
    writers, args = writers_from_args(sys.argv[1:], "qholds")
    head, qfile = args[0], args[1]
    with open(qfile) as f:
        queue = [json.loads(line) for line in f if line.strip()]
    comments = [json.loads(line) for line in sys.stdin if line.strip()]
    print(standing(head, queue, comments, writers))


if __name__ == "__main__":
    main()
