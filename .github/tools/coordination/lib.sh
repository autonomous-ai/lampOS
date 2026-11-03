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

# coord_queue OUT: the Queue's (#4710) comments of the last 3 days, where Queue HOLDs are posted.
coord_queue() { coord_comments 4710 $1 "$(date -u -v-3d +%Y-%m-%dT%H:%M:%SZ)"; }

# coord_body PR OUT: the PR body as one JSON line {login:"", body, pr_body:true}, for reads2.py.
coord_body() {
  rm -f $2
  gh pr view $1 -R $REPO --json body --jq '{login:"",body:.body,pr_body:true}' > $2.new 2>/dev/null && [[ -s $2.new ]] && mv $2.new $2
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
