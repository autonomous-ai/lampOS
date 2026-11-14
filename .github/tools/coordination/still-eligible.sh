#!/bin/zsh
# still-eligible.sh PR... : exit 1 naming the first PR whose markers now say HELD (or have no READ-OK); used by the train bot
# right before a train merges, so a HOLD posted while the train ran keeps its PR out. 25 Oct 21:1x: given the PR, as the
# bot gives it, so a hold lifted in holds-lifted.tsv is lifted here too (its absence threw away trains 129 and 130).
# 3 Nov 15:4x (the Reviewer-lead's read of #5068): it rechecks everything the bot's eligibility reads from comments, not
# only markers.py: a Queue HOLD (on any Queue issue, lib.sh) on the PR's head (qholds.py) and a TWO READS ask now short (reads2.py), with
# markers only from writers (lib.sh). 14 Nov: and its lamp row (rows.py). Any fetch error fails (exit 1): the train isn't merged.
# Paths from the environment (README.md): TOOLS_DIR (default: this folder), LAMP_COORD_STATE, PYTHON (default: python3).
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"
source $TOOLS_DIR/lib.sh
mkdir -p $SP || exit 1
wf=$(coord_writers) || { echo "the writers list can't be fetched"; exit 1; }
tmp=$(mktemp -d $SP/still.XXXXXX) || exit 1; trap 'rm -rf $tmp' EXIT
coord_queue $tmp/q || { echo "the Queue's comments can't be fetched"; exit 1; }
for p in "$@"; do
  head=$(coord_head $p) || { echo "#$p: its head can't be read, or is in another repository"; exit 1; }
  coord_pr_comments $p $tmp/c || { echo "#$p: its comments can't be fetched"; exit 1; }
  coord_body $p $tmp/b || { echo "#$p: its body can't be fetched"; exit 1; }
  r=$($PYTHON -I $TOOLS_DIR/markers.py --writers $wf $p < $tmp/c)
  [[ $r == READ-OK* ]] || { echo "#$p is now ${r:-unread}"; exit 1; }
  q=$($PYTHON -I $TOOLS_DIR/qholds.py --writers $wf $head $tmp/q < $tmp/c) || { echo "#$p: qholds.py failed"; exit 1; }
  [[ -z $q ]] || { echo "#$p is now held on the Queue ($q)"; exit 1; }
  # 7 Nov (#5653's two HOLDs): heads a Queue READ ASK names too (into a file, so lib.sh's patch cache holds across PRs),
  # and the PR's commits for Queue asks at an older head, as train-bot.sh. 7 Nov (#5653's P3s): fails when they can't
  # be listed, as every fetch here does.
  coord_same_patch $head $tmp/c $tmp/q > $tmp/heads
  heads=(${(f)"$(<$tmp/heads)"})
  git rev-list origin/lamp-voice..$head > $tmp/commits 2>/dev/null || { echo "#$p: its commits can't be listed"; exit 1; }
  two=$(cat $tmp/b $tmp/c | $PYTHON -I $TOOLS_DIR/reads2.py --always --writers $wf --queue $tmp/q --pr $p --pr-commits $tmp/commits $heads)
  [[ $two == none || $two == met* ]] || { echo "#$p: TWO READS ${two:-unread}"; exit 1; }
  # 14 Nov (the lead): its lamp row, as train-bot.sh asks it (rows.py): a ROW line or a reader's "no row needed" on this head.
  # 14 Nov (the readers' P3s on #7034): a switch turned on needs a real row (rows.py --switch-on).
  sw=$(coord_switch_on $head) || { echo "#$p: its diff can't be read for a switch turned on"; exit 1; }
  so=(); [[ $sw == on ]] && so=(--switch-on)
  rw=$($PYTHON -I $TOOLS_DIR/rows.py --writers $wf $so --pr $p $heads < $tmp/c)
  # 14 Nov 15:40 (the lead): "row-incomplete <parts>" (a row without the lamp, the build or the journal lines) is no row.
  [[ $rw == row || $rw == no-row-needed ]] || { echo "#$p: no lamp row (rows.py: ${rw:-failed})"; exit 1; }
done
exit 0
