#!/bin/zsh
# still-eligible.sh PR... : exit 1 naming the first PR whose markers now say HELD (or have no READ-OK); used by the train bot
# right before a train merges, so a HOLD posted while the train ran keeps its PR out. 25 Oct 21:1x: given the PR, as the
# bot gives it, so a hold lifted in holds-lifted.tsv is lifted here too (its absence threw away trains 129 and 130).
# Paths from the environment (README.md): TOOLS_DIR (default: this folder), PYTHON (default: python3).
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; PYTHON="${PYTHON:-python3}"
for p in "$@"; do
  r=$(gh api "repos/autonomous-ai/lampOS/issues/$p/comments?per_page=100" --paginate --jq '.[]|{login:.user.login,body:.body}' | $PYTHON -I $TOOLS_DIR/markers.py $p)
  [[ $r == READ-OK* ]] || { echo "#$p is now ${r:-unread}"; exit 1; }
done
exit 0
