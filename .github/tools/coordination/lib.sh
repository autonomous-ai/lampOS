# lib.sh (3 Nov 15:4x, for the Reviewer-lead's read of #5068): what train-bot.sh and still-eligible.sh share, sourced
# by both (zsh). Every fetch writes a file and returns non-zero on an error, so a caller can fail closed: a PR whose
# comments or body can't be read is not eligible, and nothing is eligible while the Queue or the writers can't be read.
# Needs TOOLS_DIR; sets defaults for SP ($LAMP_COORD_STATE), REPO and PYTHON.
SP="${SP:-${LAMP_COORD_STATE:-$HOME/.lamp-coordination}}"; REPO="${REPO:-autonomous-ai/lampOS}"; PYTHON="${PYTHON:-python3}"

# coord_writers: prints the path of the allowed-logins file ($SP/writers-allowed.txt) for markers.py, reads2.py and
# qholds.py. It holds the repo's collaborators with push access, cached in $SP/writers.txt for an hour (its first line
# "# fetched <epoch>"), plus the operator's own list, $SP/writers-extra.txt (one login a line). Returns 1 when the list
# can't be fetched and the cache is an hour old or more: then no marker counts (fail closed).
coord_writers() {
  local c=$SP/writers.txt now=$(date +%s) t list
  t=$(sed -n '1s/^# fetched //p' $c 2>/dev/null)
  if [[ $t != <-> ]] || (( now - t >= 3600 )); then
    list=$(gh api "repos/$REPO/collaborators?per_page=100" --paginate --jq '.[]|select(.permissions.push)|.login' 2>/dev/null) || return 1
    [[ -n $list ]] || return 1
    print -r -- "# fetched $now"$'\n'"$list" > $c.new && mv $c.new $c || return 1
  fi
  { cat $c; if [[ -r $SP/writers-extra.txt ]]; then cat $SP/writers-extra.txt; fi; } > $SP/writers-allowed.txt.new \
    && mv $SP/writers-allowed.txt.new $SP/writers-allowed.txt || return 1
  print -r -- $SP/writers-allowed.txt
}

# coord_comments ISSUE OUT [SINCE]: the issue's (or PR's) comments as JSON lines {login, body, created_at}, in order.
coord_comments() {
  rm -f $2
  gh api "repos/$REPO/issues/$1/comments?per_page=100${3:+&since=$3}" --paginate \
    --jq '.[]|{login:.user.login,body:.body,created_at:.created_at}' > $2.new 2>/dev/null && mv $2.new $2
}

# coord_pr_comments PR OUT: the PR's comments and its reviews' bodies as one list of JSON lines {login, body,
# created_at}, in time order (markers.py --merge). 4 Nov (#5212 part 1): El posts its reads as reviews, and #5080, #5180,
# #5096 and #5159 each counted one read short; a review body now reads exactly as a comment does (same writers, same
# marker rules, a later HOLD still beats an earlier READ-OK whichever kind each is). A review's state (APPROVED, ...)
# is never a read; an empty body, an unsubmitted (pending) or a dismissed review counts for nothing. Either fetch failing fails.
coord_pr_comments() {
  rm -f $2
  coord_comments $1 $2.c || return 1
  gh api "repos/$REPO/pulls/$1/reviews?per_page=100" --paginate \
    --jq '.[]|{login:.user.login,body:.body,created_at:.submitted_at,state:.state}' > $2.r 2>/dev/null || { rm -f $2.c $2.r; return 1; }
  $PYTHON -I $TOOLS_DIR/markers.py --merge $2.c $2.r > $2.new && mv $2.new $2; local e=$?; rm -f $2.c $2.r; return $e
}

# coord_queue OUT: the Queue's (#4710) comments of the last 3 days, where Queue HOLDs are posted.
coord_queue() { coord_comments 4710 $1 "$(date -u -v-3d +%Y-%m-%dT%H:%M:%SZ)"; }

# coord_body PR OUT: the PR body as one JSON line {login:"", body, pr_body:true}, for reads2.py.
coord_body() {
  rm -f $2
  gh pr view $1 -R $REPO --json body --jq '{login:"",body:.body,pr_body:true}' > $2.new 2>/dev/null && [[ -s $2.new ]] && mv $2.new $2
}

# patch_of COMMIT: the commit's patch-id from its merge base with origin/lamp-voice. 31 Oct 04:0x (Fourth, 03:23): with
# no context lines (-U0), so lamp-voice's own edits beside a hunk don't change the id; #4562 sat out trains 425-433 as
# the same patch by range-diff.
patch_of() { local b; b=$(git merge-base origin/lamp-voice $1 2>/dev/null) || return 1; git diff -U0 $b $1 -- . | git patch-id --stable | cut -d' ' -f1; }

# coord_same_patch HEAD COMMENTS: prints HEAD, then each commit a READ-OK in the COMMENTS file names whose patch is
# HEAD's (a plain rebase), one a line, for reads2.py. reads2.py still decides which READ-OKs count (writers, markers).
coord_same_patch() {
  local hp s full
  print -r -- $1
  hp=$(patch_of $1) && [[ -n $hp ]] || return 0
  for s in ${(u)${(f)"$(grep -oE 'READ-OK[*]{0,2}[[:space:]]+`?[0-9a-f]{7,40}' $2 2>/dev/null | grep -oE '[0-9a-f]{7,40}$')"}}; do
    [[ $1 == $s* ]] && continue
    git cat-file -e "$s^{commit}" 2>/dev/null || git fetch -q origin $s 2>/dev/null
    full=$(git rev-parse -q --verify "$s^{commit}" 2>/dev/null) || continue
    [[ "$(patch_of $full)" == $hp ]] && print -r -- $full
  done
}

# coord_head PR: prints the PR's head sha, or fails when it can't be read or its head is in another repository
# (a fork's PR is never carried, whatever its markers say).
coord_head() {
  local v
  v=$(gh pr view $1 -R $REPO --json headRefOid,isCrossRepository,headRepositoryOwner,headRepository \
    --jq '"\(.isCrossRepository) \(.headRepositoryOwner.login)/\(.headRepository.name) \(.headRefOid)"' 2>/dev/null) || return 1
  [[ $v == "false $REPO "* ]] || return 1
  print -r -- ${v##* }
}
