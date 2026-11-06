#!/bin/zsh
# train-bot.sh (23 Oct 07:5x, the owner: "merging is the TOP PRIORITY ... all PRs are merged instantly"): merge trains onto
# lamp-voice, back to back, with nobody building them by hand.
#   Eligible: an open, non-draft PR onto lamp-voice (not a train) whose comments (or review bodies) hold a `READ-OK <sha>` line for its current
#   head (or a head whose diff from its merge base is the same patch, i.e. a plain rebase), with no `HOLD` line after it.
#   Each round: cherry-pick every eligible PR's own commits onto lamp-voice's head (one that conflicts is told to rebase and
#   left out), push first/train-N, open its PR, wait for ARM64 and clippy, merge by rebase, then close each carried PR whose
#   commits are all on lamp-voice. A failed run: the PRs touching the failing tests' crates are left out, told why, and the
#   rest go again at once. Nothing eligible: wait 60 s.
# DRY=1: print what one round would carry and stop (with two lanes: each PR's lane, then each lane's round).
# 3 Nov: moved into .github/tools/coordination (Python stays out of lampOS/, Thirteen on #5068); every path comes from
#   the environment (README.md):
#   TOOLS_DIR         these scripts (default: this script's folder)
#   LAMP_COORD_STATE  mutable state: log, told list, PR bodies, train-first/hold/only.txt, holds-lifted.tsv
#                     (default: ~/.lamp-coordination; never the repo)
#   REPO_DIR          the checkout it fetches and reads crates in (default: the repo holding TOOLS_DIR)
#   WORKTREE          the worktree trains are built in (default: $LAMP_COORD_STATE/wt-trainbot)
#   WORKTREE_NC       the no-crates lane's (default: $LAMP_COORD_STATE/wt-trainbot-nc)
#   NOCRATES_LANE     on (default): two lanes, crate trains and no-crates trains side by side (#5212 part 3, below);
#                     off: one lane, as before
#   PYTHON            the Python for markers.py, reads2.py and qholds.py (default: python3)
#   Markers count only from logins with write access: $LAMP_COORD_STATE/writers.txt (fetched, an hour's cache) plus
#   $LAMP_COORD_STATE/writers-extra.txt (lib.sh). A PR whose head is in another repository is never carried.
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"
SP="${LAMP_COORD_STATE:-$HOME/.lamp-coordination}"; mkdir -p $SP || exit 1
export TOOLS_DIR LAMP_COORD_STATE=$SP PYTHON="${PYTHON:-python3}"
# 24 Oct 15:10: rebuilt from the session transcript after a reboot wiped /tmp. Work files (log, told list, PR bodies,
# the train worktree) stay in $SP; the helper scripts live in $TOOLS_DIR, outside /tmp.
# 29 Oct 00:3x: the scratchpad's midnight sweep gutted $SP/wt-trainbot (its .git link and old files), so the train
# worktree lives outside any swept folder now ($WORKTREE).
R="${REPO_DIR:-$(git -C $TOOLS_DIR rev-parse --show-toplevel)}"; REPO=autonomous-ai/lampOS; W="${WORKTREE:-$SP/wt-trainbot}"
# 4 Nov (#5212 part 3): the no-crates lane's trains are built in a worktree of their own.
WNC="${WORKTREE_NC:-$SP/wt-trainbot-nc}"
# 3 Nov 15:4x (the Reviewer-lead's P3 on #5068): the train worktree gets `checkout -f`, `clean -fdqx` and, on a failure,
# `rm -rf`. Refuse a WORKTREE that is the checkout itself, its top level or the repository's main worktree.
for x in $R $(git -C $R rev-parse --show-toplevel 2>/dev/null) "$(git -C $R worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p')"; do
  [[ -n $x && ${W:A} == ${x:A} ]] && { echo "WORKTREE ($W) is the checkout $x: refusing to start" >&2; exit 1; }
  [[ -n $x && ${WNC:A} == ${x:A} ]] && { echo "WORKTREE_NC ($WNC) is the checkout $x: refusing to start" >&2; exit 1; }
done
[[ ${W:A} == ${WNC:A} ]] && { echo "WORKTREE and WORKTREE_NC are the same folder ($W): refusing to start" >&2; exit 1; }
LOG=$SP/train-bot.log; cd $R || exit 1
say() { echo "$(date +%T) $*" | tee -a $LOG; }
told=$SP/train-bot.told; touch $told
# tell KEY: remember KEY in the told list. 6 Nov (the reviewer's P3 on #5542): it was append-only, a line per PR per
# head; it keeps its last 5000 lines, kept long so a `stacked` or `conflict` key is not dropped and its PR comment posted twice.
tell() { print -r -- "$1" >> $told; tail -n 5000 $told > $told.new && mv $told.new $told; }
# 3 Nov 15:4x: the fetches and the writers list are lib.sh's, shared with still-eligible.sh.
source $TOOLS_DIR/lib.sh
# once KEY MSG: say MSG the first time KEY fails, not every round; fine KEY forgets it, so a later failure is said again.
# Kept in a file: eligible() runs in a subshell, so a variable wouldn't outlive the round.
said=$SP/train-bot.said; touch $said
once() { grep -qx -- "$1" $said || { say "$2" >&2; print -r -- "$1" >> $said; }; }
# queue_post TEXT: the bot's post on the Queue (lib.sh's COORD_QUEUE_POST). 5 Nov (the Reviewer-lead's P3 on #5390):
# a refused post is logged, not dropped; when the Queue issue is full, add the next one to COORD_QUEUE_ISSUES.
queue_post() {
  gh issue comment $COORD_QUEUE_POST -R $REPO --body "$1" >/dev/null \
    || say "the Queue (#$COORD_QUEUE_POST) refused the bot's post (full? add the next Queue issue to lib.sh's COORD_QUEUE_ISSUES): $1" >&2
}
fine() { grep -qx -- "$1" $said && { grep -vx -- "$1" $said > $said.new; mv $said.new $said; }; return 0; }
eligible() {
  local p head ok sha held two own wf qf=$SP/queue.jsonl cf=$SP/pr-comments.jsonl bf=$SP/pr-body.jsonl
  # 3 Nov 15:4x (the Reviewer-lead's HOLD on #5068): the repo is public, so markers count only from accounts with write
  # access (lib.sh, coord_writers). Every fetch fails closed: without the writers list, or without the Queue (every
  # issue in lib.sh's COORD_QUEUE_ISSUES) and so its HOLDs, no PR is eligible this round; without a PR's head, comments or body, that PR is not.
  wf=$(coord_writers) || { once writers "the writers list can't be fetched and its cache is an hour old: no PR is eligible"; return; }
  fine writers
  coord_queue $qf || { once queue "the Queue's (#${(j:, #:)COORD_QUEUE_ISSUES}) comments can't be fetched: no PR is eligible"; return; }
  fine queue
  # A PR whose head is in another repository is never carried (coord_head checks it again).
  for p in $(gh pr list -R $REPO --base lamp-voice --state open --limit 200 --json number,isDraft,headRefName,isCrossRepository --jq '.[]|select(.isDraft|not)|select(.isCrossRepository|not)|select(.headRefName|startswith("first/train-")|not)|.number'); do
    head=$(coord_head $p) || { once head$p "#$p left out: its head can't be read, or is in another repository"; continue; }
    # 4 Nov: with its reviews' bodies (#5212 part 1).
    coord_pr_comments $p $cf || { once comments$p "#$p left out: its comments can't be fetched"; continue; }
    fine head$p; fine comments$p
    # 3 Nov 09:4x: a HOLD posted on the Queue (#4710, and from 5 Nov #5385: lib.sh's COORD_QUEUE_ISSUES) whose first
    # line is `HOLD <sha>` holds the PR whose head starts with that sha, until the same account posts a READ-OK on the
    # PR, after it, saying the HOLD is lifted or cleared (qholds.py, with markers.py's marker and lifting words). Train
    # 546 took #5013 on Fourteen's READ-OK while Seventh's "HOLD a13408fb8d…: lamp check pending" stood only on the Queue.
    if ! held=$($PYTHON -I $TOOLS_DIR/qholds.py --writers $wf $head $qf < $cf); then
      once qholds$p "#$p left out: qholds.py failed"; continue
    fi
    if [[ -n $held ]]; then
      grep -qx "$p qhold $head" $told || { say "#$p left out: a HOLD on the Queue ($held) names head ${head:0:9}" >&2; tell "$p qhold $head"; }
      continue
    fi
    # markers.py reads the markers: a HOLD is keyed on the account plus the reader's signed name and stands until that
    # same reader posts a later READ-OK (the bot's own "HOLD train N" clears on any later READ-OK marker for a commit, an
    # unsigned HOLD only on that account's READ-OK saying the HOLD is lifted); the PR is eligible on its newest READ-OK
    # when no HOLD stands.
    ok=$($PYTHON -I $TOOLS_DIR/markers.py --writers $wf $p < $cf)
    # 6 Nov (the PM's ask on #5542): a READ-OK from a login that isn't a writer is skipped; say it once per head.
    $PYTHON -I $TOOLS_DIR/markers.py --writers $wf --ignored < $cf | while IFS= read -r ign; do
      grep -qx -- "$p ignored $head $ign" $told || { say "#$p: $ign (head ${head:0:9})" >&2; tell "$p ignored $head $ign"; }
    done
    # 6 Nov (#5534): a held PR with a READ-OK on its head is said once per head, with the HOLDs that stand (their keys,
    # for holds-lifted.tsv), so a HOLD its reader thinks lifted shows. #5476 sat out trains 672 to 677 with both reads
    # and no line in the log.
    if [[ $ok == HELD ]]; then
      note=$($PYTHON -I $TOOLS_DIR/markers.py --writers $wf --held $p < $cf)
      if [[ -n $note && $head == ${note%% *}* ]] && ! grep -qx "$p held $head" $told; then
        say "#$p left out: held, though a READ-OK is on head ${head:0:9}; standing: ${note#* }" >&2; tell "$p held $head"
      fi
      continue
    fi
    [[ $ok == READ-OK* ]] || continue
    sha=${ok#READ-OK }; [[ -n $sha ]] || continue
    git fetch -q origin "+refs/pull/$p/head:refs/remotes/pr/$p" 2>/dev/null
    if [[ $head != $sha* ]]; then
      git cat-file -e "$sha^{commit}" 2>/dev/null || git fetch -q origin $sha 2>/dev/null
      if ! [[ "$(patch_of refs/remotes/pr/$p)" == "$(patch_of $sha)" && -n "$(patch_of $sha)" ]]; then
        # 31 Oct 04:0x: said once per head, as the red-run and two-reads notes are, so a stalled PR shows.
        grep -qx "$p patch $head" $told || { say "#$p left out: its READ-OK is for ${sha:0:9}, and head ${head:0:9} is another patch" >&2; tell "$p patch $head"; }
        continue
      fi
    fi
    # 31 Oct 03:1x: a PR whose comments ask "TWO READS" (privacy, safety, data loss) waits for READ-OKs on this head
    # from two different readers (reads2.py). #4633 and #4656 went in trains on one READ-OK where two were named.
    # 3 Nov 15:4x: only "none" or "met N" carries it; a fetch or a reads2.py error leaves it out.
    coord_body $p $bf || { once body$p "#$p left out: its body can't be fetched"; continue; }
    fine body$p
    # 4 Nov 04:1x: reads on an earlier head with the same patch count too (coord_same_patch), as for one read above.
    heads=(${(f)"$(coord_same_patch $head $cf)"})
    two=$(cat $bf $cf | $PYTHON -I $TOOLS_DIR/reads2.py --writers $wf $heads)
    if [[ $two == short* ]]; then
      grep -qx "$p short $head" $told || { say "#$p left out: TWO READS asked, $two on ${head:0:9}" >&2; tell "$p short $head"; }
      continue
    fi
    [[ $two == none || $two == met* ]] || { once reads2$p "#$p left out: reads2.py said '${two}'"; continue; }
    fine reads2$p
    # 4 Nov 23:4x (Thirteen's P3 on #5305): a body that names TWO READS without a line that asks is said once per head,
    # so an ask in words reads2.py doesn't take is seen before a train merges it on one read.
    if [[ $two == none && $($PYTHON -I $TOOLS_DIR/reads2.py --unasked < $bf) == unasked ]]; then
      grep -qx "$p unasked $head" $told || { say "#$p: its body names TWO READS but no line asks it, so one READ-OK carries ${head:0:9}" >&2; tell "$p unasked $head"; }
    fi
    # 30 Oct 15:0x (the Reviewer-lead's 14:58): a PR whose own ARM64 run on this head is red sits out. #4507's run was red
    # at 14:17 before trains 407 and 408 carried it; both failed in lamp-update, and the bot held #4524 and #4530 instead.
    # 3 Nov 16:4x (Thirteen's P3 on #5068): a run list gh can't fetch holds the PR (fail closed), not skips the check.
    if ! own=$(gh run list -R $REPO --workflow lampos-arm64.yml --commit $head -L 1 --json conclusion -q '.[0].conclusion' 2>/dev/null); then
      once run$p "#$p left out: its ARM64 runs on ${head:0:9} can't be read"; continue
    fi
    fine run$p
    if [[ $own == failure ]]; then
      grep -qx "$p red $head" $told || { say "#$p left out: its own ARM64 run on ${head:0:9} is red" >&2; tell "$p red $head"; }
      continue
    fi
    echo $p
  done
}
# 25 Oct 04:1x: a PR stacked on another open PR (#3014 and #3391 carry #3064's commit under their own) would merge that
# PR's commits with its own: every commit over lamp-voice is cherry-picked. Such a PR is left out until the one under it
# is on lamp-voice; it is told once.
unstacked() {
  local p q h line ok out
  # 3 Nov 16:4x (Thirteen's P3 on #5068): when gh can't list the open PRs, no PR is carried this round (fail closed);
  # an empty list used to pass every PR as unstacked.
  out=$(gh pr list -R $REPO --base lamp-voice --state open --limit 200 --json number,headRefOid,headRefName --jq '.[]|select(.headRefName|startswith("first/train-")|not)|"\(.number) \(.headRefOid)"') \
    || { once unstacked "the open PRs can't be listed for the stacked check: no PR is carried this round"; return; }
  fine unstacked
  local heads=(${(f)out})
  for p in "$@"; do
    ok=1
    for line in $heads; do
      q=${line%% *}; h=${line#* }; [[ $q == $p ]] && continue
      git cat-file -e "$h^{commit}" 2>/dev/null || git fetch -q origin "+refs/pull/$q/head:refs/remotes/pr/$q" 2>/dev/null
      git merge-base --is-ancestor $h origin/lamp-voice 2>/dev/null && continue
      if git merge-base --is-ancestor $h refs/remotes/pr/$p 2>/dev/null; then
        ok=0
        grep -qx "$p stacked $q $h" $told || { say "#$p left out: it carries open #$q's head ${h:0:9}" >&2; [[ ${DRY:-0} == 1 ]] || gh pr comment $p -R $REPO --body "Train bot, $(date +%H:%M): #$p carries open #$q's commits (its head \`${h:0:9}\`), so a train would merge them with it. It is left out until #$q is on lamp-voice; then rebase onto lamp-voice and your READ-OK carries over if the patch is the same." >/dev/null; [[ ${DRY:-0} == 1 ]] || tell "$p stacked $q $h"; }
        break
      fi
    done
    (( ok )) && echo $p
  done
}
# Every PR given is a rows PR and the oldest was opened under 3 h ago.
rows_only() {
  local p t c oldest=9999999999
  for p in $@; do
    t=$(gh pr view $p -R $REPO --json title,createdAt --jq '"\(.title)\t\(.createdAt)"')
    [[ ${t%%$'\t'*} == "lampOS: docs/build-rows.md as at "* ]] || return 1
    c=$(date -j -u -f %Y-%m-%dT%H:%M:%SZ ${t##*$'\t'} +%s 2>/dev/null) || return 1
    (( c < oldest )) && oldest=$c
  done
  (( $(date +%s) - oldest < 10800 ))
}
n=$(gh pr list -R $REPO --state all --limit 100 --search 'head:first/train- in:title "merge train"' --json title --jq '[.[].title|capture("train (?<n>[0-9]+)")?.n|tonumber]|max // 23')
# 27 Oct 14:0x: the search missed trains 204-212 after a restart, so train "204" went onto an old branch. Take the
# highest of the search and the remote's first/train-N branches.
b=$(git ls-remote origin 'refs/heads/first/train-*' | sed -E 's#.*/train-##' | grep -E '^[0-9]+$' | sort -n | tail -1)
[[ -n $b && $b -gt $n ]] && n=$b
# 4 Nov (#5212 part 3): docs-, tools- and other PRs that change no build input waited behind 20-minute crate trains.
# With NOCRATES_LANE=on (the default) a second lane carries them: every eligible PR goes to exactly one lane by its
# files (lib.sh, coord_lane): no file a build input (coord_input: crates, Cargo.*, workflows, anything else the ARM64
# run or the SDK check takes) -> the no-crates lane, trains first/train-nc-N; any input -> the crate lane, trains
# first/train-N as before. Each lane runs one train at a time, side by side, each in its own worktree; neither takes the
# other's PRs. A no-crates train gets no ARM64 or clippy run, so merge-when-green.sh waits only for the checks that do
# start on it. Each lane merges with --behind-unrelated (the no-crates lane with --no-crates too), so a merge in one lane
# moves lamp-voice under the other's train by disjoint files only and that train merges as it is, pinned to its head
# (--match-head-commit); a merge GitHub refuses rebuilds. NOCRATES_LANE=off: one lane, the bot as it was.
NOCRATES_LANE="${NOCRATES_LANE:-on}"; two=0; [[ $NOCRATES_LANE == off ]] || two=1
m=$(git ls-remote origin 'refs/heads/first/train-nc-*' | sed -E 's#.*/train-nc-##' | grep -E '^[0-9]+$' | sort -n | tail -1); m=${m:-0}
# A restart clears a stop: train-bot.stop (the whole bot, exit 2) and train-bot.stop-nc (the no-crates lane alone).
[[ ${DRY:-0} == 1 ]] || rm -f $SP/train-bot.stop $SP/train-bot.stop-nc
typeset -A job; job=()
# A stop (kill) takes the lanes running in the background with it, and their merge-when-green.sh.
trap 'for j in ${(v)job}; do pkill -P $j 2>/dev/null; kill $j 2>/dev/null; done; exit 143' TERM INT
# halt LANE: the crate lane (or the only lane) stops the bot for the first builder, as before (exit 2; from a lane
# running in the background, through train-bot.stop, which the main loop reads). The no-crates lane stops alone.
halt() {
  if [[ $1 == nc ]]; then touch $SP/train-bot.stop-nc; say "the no-crates lane stopped for the first builder; the crate lane goes on"; return 0; fi
  touch $SP/train-bot.stop; exit 2
}
# build_train LANE PR...: build the lane's next train from these PRs, push it and open its PR. Sets tn (the train's
# name: N, or nc-N), tp (its PR), carried, refs and h. Returns 1 when no train was opened.
build_train() {
  local lane=$1 wt br p ph own body title; shift
  if [[ $lane == nc ]]; then m=$((m+1)); tn=nc-$m; wt=$WNC; else n=$((n+1)); tn=$n; wt=$W; fi
  br=first/train-$tn
  # 24 Oct 09:5x: a fresh `git worktree add` took 16 min under a busy disk, every train. Reuse the
  # train worktree: abort anything half done, force it onto the new train branch from lamp-voice
  # (only changed files are written) and clean it; add a new one only when it is missing.
  if [[ -e $wt/.git ]]; then
    git -C $wt rebase --abort 2>/dev/null; git -C $wt cherry-pick --abort 2>/dev/null; git -C $wt merge --abort 2>/dev/null
    { git -C $wt checkout -q -f -B $br origin/lamp-voice && git -C $wt clean -fdqx; } || { say "worktree reuse failed"; git worktree remove --force $wt 2>/dev/null; rm -rf $wt; git worktree prune; return 1; }
  else
    git worktree prune; git worktree add -q -B $br $wt origin/lamp-voice || { say "worktree failed"; return 1; }
  fi
  carried=()
  if [[ $lane == nc ]]; then body="No-crates train $tn, onto lamp-voice \`$LV\`, built by the train bot (#$COORD_QUEUE_POST, the Queue). Its PRs change no build input, so no ARM64 run: it merges by rebase once the checks that start on it pass.\n"
  else body="Merge train $n, onto lamp-voice \`$LV\`, built by the train bot (#$COORD_QUEUE_POST, the Queue). One ARM64 run, merged by rebase.\n"; fi
  for p in "$@"; do
    own=(${(f)"$(git -C $wt rev-list --reverse --cherry-pick --right-only --no-merges HEAD...refs/remotes/pr/$p)"})
    [[ -n ${own[1]} ]] || continue
    if git -C $wt cherry-pick $own >/dev/null 2>&1; then
      carried+=$p; body+="\n- #$p: $(gh pr view $p -R $REPO --json title -q .title | sed 's/^lampOS: //' | cut -c1-200)"
    else
      git -C $wt cherry-pick --abort
      # 28 Oct 16:4x: told once per PR, not once per lamp-voice head (#4042 got three in an hour): post less.
      # 4 Nov 04:4x: once per PR head, so a rebased head that conflicts again is told (and logged) once more. #5043 was
      # told on 3 Nov; rebased, it conflicted again on switches.md and sat out train 584 with nothing said.
      ph=$(git rev-parse refs/remotes/pr/$p)
      grep -q "^$p conflict .* $ph\$" $told || { say "#$p left out of train $tn: it conflicts with lamp-voice $LV" >&2; gh pr comment $p -R $REPO --body "Train bot, $(date +%H:%M): #$p conflicts with lamp-voice \`$LV\` and was left out of train $tn. Please rebase onto lamp-voice; your READ-OK carries over if the patch is the same." >/dev/null; tell "$p conflict $LV $ph"; }
    fi
  done
  [[ ${#carried} -gt 0 ]] || { if [[ $lane == nc ]]; then m=$((m-1)); else n=$((n-1)); fi; return 1; }
  refs=""; for p in $carried; do refs+="#$p, "; done; refs=${refs%, }
  h=$(git -C $wt rev-parse HEAD); [[ ${#h} == 40 ]] || { say "no head"; exit 1; }
  git -C $wt push -q origin "${h}:refs/heads/$br" 2>&1 | grep -v '^remote:'
  # A push refused (the branch already there) leaves the old head under the PR: never open one on it (27 Oct 14:0x).
  [[ $(git ls-remote origin "refs/heads/$br" | cut -c1-40) == $h ]] || { say "train $tn: push didn't land, next number"; return 1; }
  print -r -- "$body\n\nPart of #$COORD_QUEUE_POST." > $SP/pr-train$tn.md
  # The crate lane's title keeps "merge train N", which the train number search reads; "no-crates train nc-N" never
  # matches it.
  if [[ $lane == nc ]]; then title="lampOS: no-crates train $tn: $refs"; else title="lampOS: merge train $n: $refs"; fi
  tp=$(gh pr create -R $REPO --base lamp-voice --head $br --title "$title" --body-file $SP/pr-train$tn.md --label builder:first 2>&1 | grep -oE '[0-9]+$')
  say "train $tn (#$tp): ${carried[*]} on $LV"
}
# run_train LANE: wait for the train built last (tn, tp, carried, refs, h) and merge it, or close it and say why. With
# two lanes it runs in the background, one per lane.
run_train() {
  local lane=$1 rc slow slowf=$SP/train-bot.slow-$1 p left added c run fails errfiles heldany hit d deps t names dd NV
  # 3 Nov 15:4x: the PRs to recheck go as arguments; merge-when-green.sh runs still-eligible.sh on them (no eval).
  # 4 Nov (#5212 part 2): --behind-unrelated, so a green train behind lamp-voice only by files it doesn't touch, and no
  # build input (behind-unrelated.sh), merges as it is instead of being rebuilt. Part 3: a no-crates train passes
  # --no-crates too, so a crate train's merge under it rebuilds it only on an overlap.
  local flags=(--behind-unrelated); [[ $lane == nc ]] && flags+=--no-crates
  $TOOLS_DIR/merge-when-green.sh $tp $flags --precheck $carried >> $LOG 2>&1; rc=$?
  if [[ $rc == 6 ]]; then
    # A carried PR was held while the train ran: it sits out, the rest go again on the same head at once.
    gh pr close $tp -R $REPO -c "Train bot: closed before merging, a carried PR was held while it ran (see the log line); the rest go again." >/dev/null
    say "train $tn (#$tp) not merged: a carried PR was held while it ran; rebuilding"; return
  fi
  if [[ $rc == 5 ]]; then
    # Checks still running after 45 min (train 41, 23 Oct 14:59): a slow runner, not a failure. Close it and build again;
    # twice in a row is CI itself, so stop for the first builder. Counted in a file per lane: a lane can run in the
    # background.
    slow=$(( $(cat $slowf 2>/dev/null || echo 0) + 1 )); echo $slow > $slowf
    gh pr close $tp -R $REPO -c "Train bot: closed, its checks hadn't finished in 45 min; the same PRs go again." >/dev/null
    if [[ $slow -ge 2 ]]; then say "train $tn (#$tp): checks unfinished twice in a row: stopping for the first builder"; queue_post "Train bot, $(date +%H:%M): two trains in a row had checks unfinished after 45 min (CI is slow or stuck); stopped for the first builder$([[ $lane == nc ]] && print ' (the no-crates lane only)')."; echo 0 > $slowf; halt $lane; return; fi
    say "train $tn (#$tp) checks unfinished in 45 min: rebuilding"; return
  fi
  echo 0 > $slowf
  # 29 Oct 16:0x (train 359): a train failed in a non-cargo check (the SDK schema copies) after a reader held one of
  # its PRs; no failing test was named, so the bot used to stop. A carried PR held since the train began sits out first.
  if [[ $rc != 0 ]] && ! $TOOLS_DIR/still-eligible.sh ${carried[*]} >/dev/null 2>&1; then
    gh pr close $tp -R $REPO -c "Train bot: closed, it failed and a carried PR was held while it ran; the rest go again." >/dev/null
    say "train $tn (#$tp) failed with a carried PR held meanwhile: rebuilding without it"; return
  fi
  if [[ $rc == 3 ]]; then
    # Green but lamp-voice moved under it (a reviewer merged meanwhile): build again on the new head at once.
    gh pr close $tp -R $REPO -c "Train bot: closed, lamp-voice moved while it ran; the same PRs go again on the new head." >/dev/null
    say "train $tn (#$tp) behind lamp-voice: rebuilding"; return
  fi
  if [[ $rc == 0 ]]; then
    [[ $lane == nc ]] && rm -f $SP/train-bot.nc-one
    git fetch -q origin lamp-voice; NV=$(git rev-parse --short=9 origin/lamp-voice)
    for p in $carried; do
      # 31 Oct 09:4x (Fourth, 09:26): fetch the PR's head again first. A commit pushed while the train ran was never
      # fetched, so the PR closed as if all had landed and the new commit was dropped (#4671's 3673a00dfc, #4703's bb15254fc7).
      git fetch -q origin "+refs/pull/$p/head:refs/remotes/pr/$p" 2>/dev/null
      left=$(git rev-list --cherry-pick --right-only --no-merges origin/lamp-voice...refs/remotes/pr/$p | wc -l | tr -d ' ')
      # 24 Oct 13:3x: train 73 rebased #3306 over a neighbour's change, so its patches' context changed and
      # --cherry-pick matched none; it stayed open and was tried again in train 74. Then each of the PR's own
      # commits is looked for by subject among the commits this train added ($LV..lamp-voice).
      if [[ $left != 0 ]]; then
        added=$(git log --no-merges --format=%s $LV..origin/lamp-voice)
        left=0; for c in $(git rev-list --no-merges origin/lamp-voice..refs/remotes/pr/$p); do grep -qxF -- "$(git log -1 --format=%s $c)" <<< "$added" || left=$((left+1)); done
      fi
      # 28 Oct 16:4x: closed with no comment; the Queue's merge line names it (post less).
      [[ $left == 0 ]] && gh pr close $p -R $REPO >/dev/null && say "  closed #$p"
    done
    queue_post "Train bot, $(date +%H:%M): **train $tn (#$tp) merged** as lamp-voice \`$NV\`: $refs."
    return
  fi
  if [[ $lane == nc ]]; then
    # A no-crates train has no ARM64 run, so there is no failing test or crate to name. Alone in its train, the PR sits
    # out (HOLD, cleared by a new READ-OK); with others, the lane carries one PR a train until a train merges, so the
    # failing one is found by itself. A merge GitHub refused (rc 4) stops the lane, as it stops the bot in the crate lane.
    gh pr close $tp -R $REPO -c "Train bot: closed, it failed (rc $rc); see the log." >/dev/null
    if [[ $rc == 2 && ${#carried} == 1 ]]; then
      say "train $tn (#$tp) failed: its checks failed with #${carried[1]} alone"
      gh pr comment ${carried[1]} -R $REPO --body "Train bot, $(date +%H:%M): no-crates train $tn's checks failed with only this PR in it. Please look, fix, and post a new READ-OK once read." >/dev/null
      gh pr comment ${carried[1]} -R $REPO --body "HOLD train $tn failed (no-crates lane, its checks)" >/dev/null; say "  held #${carried[1]}"
    elif [[ $rc == 2 ]]; then
      touch $SP/train-bot.nc-one; say "train $tn (#$tp) failed with ${#carried} PRs: the no-crates lane carries one PR a train until one merges"
    else
      say "train $tn (#$tp) failed (rc $rc)"; queue_post "Train bot, $(date +%H:%M): no-crates train $tn (#$tp) failed (rc $rc, not its checks); the no-crates lane stopped for the first builder."; halt nc
    fi
    return
  fi
  # Which crates the failing tests are in; the PRs touching them sit out with the reason, the rest go again now.
  run=$(gh api "repos/$REPO/actions/workflows/lampos-arm64.yml/runs?head_sha=$h" --jq '.workflow_runs[0].id')
  fails=$(gh run view $run -R $REPO --log-failed 2>/dev/null | grep -oE 'FAIL \[[^]]*\] *\( *[0-9]+/[0-9]+\) [a-z0-9_-]+' | awk '{print $NF}' | sort -u)
  # A compile or lint error (clippy, or a test build): the files it names, and the PRs that touch those files.
  errfiles=$(for r in $(gh api "repos/$REPO/actions/runs?head_sha=$h" --jq '.workflow_runs[]|select(.conclusion=="failure")|.id'); do gh run view $r -R $REPO --log-failed 2>/dev/null; done | grep -oE -- '--> (lampOS/)?crates/[^:]+' | sed -E 's#^--> (lampOS/)?#lampOS/#' | sort -u)
  if [[ -n $errfiles ]]; then
    say "train $n (#$tp) failed to compile or lint in: ${(f)errfiles}"
    gh pr close $tp -R $REPO -c "Train bot: closed, a compile or lint error in ${(j:, :)${(f)errfiles}}; the PRs touching those files sit out, the rest go again." >/dev/null
    for p in $carried; do
      if gh pr diff $p -R $REPO --name-only | grep -qxF -f <(print -l ${(f)errfiles}); then
        gh pr comment $p -R $REPO --body "HOLD train $n: a compile or lint error in a file this PR changes (${(j:, :)${(f)errfiles}}). Train bot, $(date +%H:%M): rebase onto lamp-voice, run clippy with --all-targets, and a reader posts a new READ-OK." >/dev/null; say "  held #$p"
      fi
    done
    return
  fi
  say "train $n (#$tp) failed (rc $rc); failing crates: ${fails:-none found}"
  gh pr close $tp -R $REPO -c "Train bot: closed, its run failed (${fails:-no failing test named}); the PRs touching those crates sit out, the rest go again." >/dev/null
  heldany=0
  for p in $carried; do
    hit=0; for c in ${(f)fails}; do d=$(grep -l "^name = \"$c\"" lampOS/crates/*/Cargo.toml 2>/dev/null | head -1 | xargs dirname 2>/dev/null); [[ -n $d ]] && gh pr diff $p -R $REPO --name-only | grep -q "^$d/" && hit=1; done
    if [[ $hit == 1 ]]; then heldany=1
      gh pr comment $p -R $REPO --body "Train bot, $(date +%H:%M): train $n's ARM64 failed in a crate this PR changes (${fails}). Please look, fix, and post a new READ-OK once read." >/dev/null
      gh pr comment $p -R $REPO --body "HOLD train $n failed in ${fails}" >/dev/null; say "  held #$p"
    fi
  done
  # 29 Oct 08:0x (Fourth, after trains 337 and 338): a failing crate no carried PR touches can still be broken by a
  # crate it depends on, dev-dependencies included (lamp-memory's tests drive lamp-voice's Days client). Then the PRs
  # touching those crates sit out, so the same set isn't sent again.
  if [[ $heldany == 0 && -n $fails ]]; then
    deps=(); for c in ${(f)fails}; do t=$(grep -l "^name = \"$c\"" lampOS/crates/*/Cargo.toml 2>/dev/null | head -1); [[ -n $t ]] && deps+=(${(f)"$(grep -oE 'path *= *"\.\./[a-z0-9_-]+"' $t | sed -E 's#.*\.\./([a-z0-9_-]+)"#lampOS/crates/\1#' | sort -u)"}); done
    for p in $carried; do
      hit=0; names=$(gh pr diff $p -R $REPO --name-only); for dd in $deps; do grep -q "^$dd/" <<< "$names" && hit=1; done
      if [[ $hit == 1 ]]; then
        gh pr comment $p -R $REPO --body "Train bot, $(date +%H:%M): train $n's ARM64 failed in ${fails}, which no carried PR changes but which depends on a crate this PR changes. Please run that crate's whole suite on your head, fix, and post a new READ-OK once read." >/dev/null
        gh pr comment $p -R $REPO --body "HOLD train $n failed in ${fails} (a dependent crate)" >/dev/null; say "  held #$p (a crate ${fails} depends on)"
      fi
    done
  fi
  [[ -z $fails ]] && { say "no failing test named: stopping for the first builder"; queue_post "Train bot, $(date +%H:%M): train $n (#$tp) failed with no failing test named; stopped for the first builder."; halt crates; }
  return 0
}
# lane_round LANE BUSY PR...: what one lane does this round with its PRs: nothing (busy, stopped, rows alone, none),
# DRY=1's line, or a train built and run (in the background with two lanes). Returns 0 when it started a train.
lane_round() {
  local lane=$1 busy=$2 name; shift 2
  local l=("$@"); [[ -n ${l[1]} ]] || l=()
  if (( two )); then [[ $lane == nc ]] && name="no-crates lane: " || name="crate lane: "; fi
  if [[ $lane == nc && -f $SP/train-bot.stop-nc ]]; then [[ ${DRY:-0} == 1 ]] && say "dry: ${name}stopped (train-bot.stop-nc)"; return 1; fi
  if (( busy )); then [[ ${DRY:-0} == 1 ]] && say "dry: ${name}a train is open or running; a real run would wait for it"; return 1; fi
  # 25 Oct 16:2x: a train of rows PRs alone (docs/build-rows.md) waits for a code PR to ride with, or until the oldest
  # is 3 h old: one alone moved lamp-voice under every ready PR (train 123), so each had to be carried again.
  # 5 Nov 02:0x (Thirteen's P3 on #5297): not in the no-crates lane. No code PR rides there, so a rows PR always waited
  # the full 3 h. And train 123's cost is gone there: crate trains merge with --behind-unrelated, so a rows merge that
  # touches none of their files leaves them as they are. The crate lane and the one-lane bot still wait.
  if [[ $lane != nc ]] && (( ${#l} )) && rows_only $l; then [[ ${DRY:-0} == 1 ]] && say "dry: ${name}rows PRs alone, waiting"; return 1; fi
  if (( ! ${#l} )); then [[ ${DRY:-0} == 1 ]] && say "dry: ${name}nothing eligible"; return 1; fi
  # After a failed no-crates train of several PRs, one PR a train (run_train).
  [[ $lane == nc && -f $SP/train-bot.nc-one ]] && l=(${l[1]})
  if [[ ${DRY:-0} == 1 ]]; then say "dry: ${name}would carry ${l[*]} onto $LV"; return 1; fi
  build_train $lane $l || return 1
  if (( two )); then run_train $lane & job[$lane]=$!; else run_train $lane; fi
  return 0
}
while true; do
  # A lane in the background stopped the bot (halt): let the other lane's train finish, then stop.
  if [[ -f $SP/train-bot.stop ]]; then (( ${#job} )) && wait; say "stopped for the first builder"; exit 2; fi
  for l in ${(k)job}; do kill -0 ${job[$l]} 2>/dev/null || unset "job[$l]"; done
  open=(${(f)"$(gh pr list -R $REPO --base lamp-voice --state open --limit 50 --json headRefName --jq '.[]|select(.headRefName|startswith("first/train-"))|.headRefName')"})
  busy_c=0; busy_n=0
  if (( two )); then
    # Each lane waits only for its own train: first/train-nc-N for the no-crates lane, any other first/train-* (a crate
    # train, or one built by hand) for the crate lane.
    for b in $open; do if [[ $b == first/train-nc-* ]]; then busy_n=1; else busy_c=1; fi; done
    [[ -n ${job[crates]} ]] && busy_c=1; [[ -n ${job[nc]} ]] && busy_n=1
    if (( busy_c && busy_n )) && [[ ${DRY:-0} != 1 ]]; then sleep 30; continue; fi
  elif (( ${#open} )); then
    # Another train still open (one built by hand, or a reviewer's merge in flight): wait, one lane.
    if [[ ${DRY:-0} == 1 ]]; then say "dry: another train is open; a real run would wait for it"; else sleep 60; continue; fi
  fi
  git fetch -q origin lamp-voice; LV=$(git rev-parse --short=9 origin/lamp-voice)
  list=(${(f)"$(eligible)"})
  [[ -n ${list[1]} ]] && list=(${(f)"$(unstacked $list)"})
  # 27 Oct 13:5x (first builder): PRs named in $LAMP_COORD_STATE/train-first.txt go first, in its order, so a ship item isn't
  # left out by a newer PR on the same files (#3466 behind #3734 in train 212). Names not eligible are skipped.
  if [[ -s $SP/train-first.txt && -n ${list[1]} ]]; then
    first=(); for p in $(<$SP/train-first.txt); do (( ${list[(Ie)$p]} )) && first+=$p; done
    list=($first ${list:|first})
  fi
  # 28 Oct 11:5x (first builder): PRs named in $LAMP_COORD_STATE/train-hold.txt sit out (read, but owing a lamp run the
  # lead's 09:36 merge condition asks of onboarding fixes), so a READ-OK alone doesn't carry them.
  if [[ -s $SP/train-hold.txt && -n ${list[1]} ]]; then
    kept=(); for p in $list; do grep -qw $p $SP/train-hold.txt || kept+=$p; done; list=($kept)
  fi
  # 28 Oct 09:5x (first builder): the lead's 12:00 onboarding deadline. While $LAMP_COORD_STATE/train-only.txt is there, a
  # train carries only the PRs it names (in train-first.txt's order), so a failure elsewhere costs no onboarding rerun.
  if [[ -f $SP/train-only.txt && -n ${list[1]} ]]; then
    only=(); for p in $list; do grep -qw $p $SP/train-only.txt && only+=$p; done; list=($only)
  fi
  if (( ! two )); then
    # One lane, as before.
    if [[ -n ${list[1]} ]] && rows_only $list; then [[ ${DRY:-0} == 1 ]] && { say "dry: rows PRs alone, waiting"; exit 0; }; sleep 60; continue; fi
    if [[ ${#list} -eq 0 || -z ${list[1]} ]]; then [[ ${DRY:-0} == 1 ]] && { say "dry: nothing eligible"; exit 0; }; sleep 60; continue; fi
    if [[ ${DRY:-0} == 1 ]]; then say "dry: would carry ${list[*]} onto $LV"; exit 0; fi
    lane_round crates 0 $list || sleep 60
    continue
  fi
  # Two lanes: each PR to exactly one, by its files on its fetched head (eligible() fetched refs/remotes/pr/N).
  coord_inputs_init origin/lamp-voice
  clist=(); nlist=()
  for p in $list; do
    [[ -n $p ]] || continue
    ln=$(coord_lane refs/remotes/pr/$p)
    if [[ $ln == nocrates ]]; then nlist+=$p; [[ ${DRY:-0} == 1 ]] && say "dry: #$p -> no-crates lane"
    else clist+=$p; [[ ${DRY:-0} == 1 ]] && say "dry: #$p -> crate lane (${ln#crates: })"; fi
  done
  lane_round crates $busy_c $clist
  lane_round nc $busy_n $nlist
  [[ ${DRY:-0} == 1 ]] && exit 0
  # Wait up to 60 s, less when a lane's train ends (a rebuild goes at once, as with one lane). While a train runs in a
  # lane, rounds are 3 min apart: each round reads every open PR (eligible), and one lane used to read none while its
  # train ran, so this bounds the extra GitHub API calls the second lane makes.
  waits=6; (( ${#job} )) && waits=18
  for i in {1..$waits}; do
    sleep 10
    for l in ${(k)job}; do kill -0 ${job[$l]} 2>/dev/null || { unset "job[$l]"; break 2; }; done
  done
done
