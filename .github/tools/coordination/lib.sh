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

# The Queue's issues, oldest first: #4710 hit GitHub's 2,500-comment limit on 5 Nov and the Queue went on in #5385,
# which hit it on 10 Nov; the Queue goes on in #6059.
# The one place their numbers live: every one is read, so a HOLD on an older one still counts; the bot posts on the last.
COORD_QUEUE_ISSUES=(4710 5385 6059)
COORD_QUEUE_POST=${COORD_QUEUE_ISSUES[-1]}

# coord_queue OUT: the comments of the last 3 days on every Queue issue, where Queue HOLDs are posted, as one list of
# JSON lines (issue by issue). Any issue that can't be fetched fails it, with OUT gone: no Queue, no PR eligible.
coord_queue() {
  local i since=$(date -u -v-3d +%Y-%m-%dT%H:%M:%SZ)
  rm -f $1; : > $1.all || return 1
  for i in $COORD_QUEUE_ISSUES; do
    coord_comments $i $1.$i "$since" && cat $1.$i >> $1.all || { rm -f $1.all $1.$i; return 1; }
    rm -f $1.$i
  done
  mv $1.all $1
}

# coord_body PR OUT: the PR body as one JSON line {login:"", body, pr_body:true}, for reads2.py.
coord_body() {
  rm -f $2
  gh pr view $1 -R $REPO --json body --jq '{login:"",body:.body,pr_body:true}' > $2.new 2>/dev/null && [[ -s $2.new ]] && mv $2.new $2
}

# patch_of COMMIT: the commit's patch-id from its merge base with origin/lamp-voice. 31 Oct 04:0x (Fourth, 03:23): with
# no context lines (-U0), so lamp-voice's own edits beside a hunk don't change the id; #4562 sat out trains 425-433 as
# the same patch by range-diff.
patch_of() { local b; b=$(git merge-base origin/lamp-voice $1 2>/dev/null) || return 1; git diff -U0 $b $1 -- . | git patch-id --stable | cut -d' ' -f1; }

# coord_same_patch HEAD COMMENTS [QUEUE]: prints HEAD, then each commit a READ-OK in the COMMENTS file names whose patch
# is HEAD's (a plain rebase), one a line, for reads2.py. reads2.py still decides which READ-OKs count (writers, markers).
# 7 Nov (El's HOLD on #5653): with QUEUE (the Queue's JSON lines, coord_queue), the shas Queue READ ASKs name are
# candidates too, so a Queue "READ ASK <old head>" naming TWO READS still asks after a plain rebase and one READ-OK on
# the new head; reads2.py still decides which asks count. The Queue names many other PRs' heads, so each sha's patch
# (or that it can't be resolved, after one fetch) is kept in coord_patch_cache, keyed by origin/lamp-voice and the sha:
# call it with its output redirected to a file, not in $(...), so one bot round works each sha out once across PRs.
# 7 Nov (#5653's P3s): the Queue's shas are those reads2.py --queue-shas prints (each sha on a first line its QUEUE_ASK
# takes, signed asks like "Sixth, READ ASK to **El**: `<sha>`" too), so the candidates and queue_asks read one regex.
# A short sha resolves only when the object is local (git can't fetch an abbreviated sha): see README.md.
typeset -gA coord_patch_cache
# coord_cached_patch SHA LV: sets REPLY to "<full sha> <patch>" from the cache (filling it), or fails when SHA can't be
# resolved. Sets REPLY rather than printing, so the cache fills in the calling shell.
coord_cached_patch() {
  local k="$2:$1" full p
  if (( ! ${+coord_patch_cache[$k]} )); then
    git cat-file -e "$1^{commit}" 2>/dev/null || git fetch -q origin $1 2>/dev/null
    full=$(git rev-parse -q --verify "$1^{commit}" 2>/dev/null) && p=$(patch_of $full) && [[ -n $p ]] \
      && coord_patch_cache[$k]="$full $p" || coord_patch_cache[$k]=-
  fi
  REPLY=${coord_patch_cache[$k]}
  [[ $REPLY != - ]]
}
# 11 Nov 16:4x (first builder): a PR that changes the product's defaults trains only on the first builder's DEFAULT-ON.
# #6189 (train 794) and #6331 (train 806) turned switches on in the product on their READ-OKs alone, with no default-on
# read: the hold list (train-hold.txt) only covers PRs that exist when it's written. COORD_PRODUCT_RE names the files
# that decide what a customer's lamp does: the product units and their folder, the skills a product install ships,
# and install.rs's PRODUCT lists.
COORD_PRODUCT_RE='^lampOS/(deploy/product/|skills/|crates/update/src/install\.rs$)'
# coord_product_change HEAD: 0 when HEAD's diff from its merge base with origin/lamp-voice touches COORD_PRODUCT_RE,
# 1 when it doesn't, 2 when the diff can't be read (callers leave the PR out: fail closed).
coord_product_change() {
  local b f
  b=$(git merge-base origin/lamp-voice $1 2>/dev/null) || return 2
  f=$(git diff --name-only $b $1 2>/dev/null) || return 2
  print -r -- $f | grep -qE "$COORD_PRODUCT_RE"
}
# coord_default_on HEAD COMMENTS WRITERS: 0 when a writer's DEFAULT-ON marker in COMMENTS (markers.py --default-on)
# names a commit HEAD starts with, or a commit with HEAD's patch (a plain rebase, as a READ-OK carries).
coord_default_on() {
  local s lv hp
  local -a shas
  shas=(${(f)"$($PYTHON -I $TOOLS_DIR/markers.py --writers $3 --default-on < $2)"})
  (( ${#shas} )) || return 1
  for s in $shas; do [[ $1 == $s* ]] && return 0; done
  lv=$(git rev-parse -q --verify origin/lamp-voice 2>/dev/null) || return 1
  coord_cached_patch $1 $lv && hp=${REPLY#* } || return 1
  for s in $shas; do
    coord_cached_patch $s $lv || continue
    [[ ${REPLY#* } == $hp ]] && return 0
  done
  return 1
}
coord_same_patch() {
  local hp s lv; local -a seen
  print -r -- $1
  lv=$(git rev-parse -q --verify origin/lamp-voice 2>/dev/null) || return 0
  coord_cached_patch $1 $lv && hp=${REPLY#* } || return 0
  seen=(${REPLY%% *})
  for s in ${(u)${(f)"$( grep -oE 'READ-OK[*]{0,2}[[:space:]]+`?[0-9a-f]{7,40}' $2 2>/dev/null | grep -oE '[0-9a-f]{7,40}$'
                       [[ -n $3 ]] && $PYTHON -I $TOOLS_DIR/reads2.py --queue-shas $3 2>/dev/null )"}}; do
    [[ $1 == $s* ]] && continue
    coord_cached_patch $s $lv || continue
    (( ${seen[(Ie)${REPLY%% *}]} )) && continue
    seen+=(${REPLY%% *})
    [[ ${REPLY#* } == $hp ]] && print -r -- ${REPLY%% *}
  done
  return 0
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

# ---- Build inputs and lanes (#5212 parts 2 and 3), shared by behind-unrelated.sh, merge-when-green.sh, lane.sh and
# train-bot.sh, so "what the ARM64 run takes" is one test everywhere.
# coord_wf_paths REF WORKFLOW: the pull_request `paths` of .github/workflows/WORKFLOW at REF, one pattern a line (the
# block list form both workflows use). Prints nothing when it can't be read.
coord_wf_paths() {
  git show "$1:.github/workflows/$2" 2>/dev/null | awk '
    /^  pull_request:/ { pr = 1; next }
    pr && /^  [^ #]/ { pr = 0 }
    pr && /^    paths:/ { p = 1; next }
    pr && p && /^      - / { s = $0; sub(/^      - /, "", s); sub(/[[:space:]]+#.*$/, "", s); gsub(/^'\''|'\''$/, "", s); gsub(/^"|"$/, "", s); print s; next }
    pr && p && /^[[:space:]]*(#|$)/ { next }
    pr && p && /^    [^ ]/ { p = 0 }'
}
# coord_glob_re GLOB: a GitHub path filter as an extended regex: ** any characters (**/ also none), * and ? not /.
coord_glob_re() {
  local g=$1 r="" c i
  for (( i = 1; i <= ${#g}; i++ )); do
    c=${g[i]}
    if [[ $c == '*' && ${g[i+1]} == '*' ]]; then
      if [[ ${g[i+2]} == / ]]; then r+='(.*/)?'; (( i += 2 )); else r+='.*'; (( i++ )); fi
    elif [[ $c == '*' ]]; then r+='[^/]*'
    elif [[ $c == '?' ]]; then r+='[^/]'
    else case $c in (.|+|\(|\)|\||^|\$|\{|\}|\[|\]|\\) r+="\\$c" ;; (*) r+=$c ;; esac; fi
  done
  print -r -- "^$r\$"
}
# coord_inputs_init REF: reads lampos-arm64.yml's and lampos-sdk.yml's pull_request paths at REF into coord_arm_pats and
# coord_sdk_pats ("+regex" or "-regex" for a "!" pattern, in order). An empty list makes coord_input fail closed.
coord_inputs_init() {
  local p; typeset -ga coord_arm_pats coord_sdk_pats; coord_arm_pats=(); coord_sdk_pats=()
  for p in ${(f)"$(coord_wf_paths $1 lampos-arm64.yml)"}; do
    if [[ $p == '!'* ]]; then coord_arm_pats+="-$(coord_glob_re ${p#!})"; else coord_arm_pats+="+$(coord_glob_re $p)"; fi
  done
  for p in ${(f)"$(coord_wf_paths $1 lampos-sdk.yml)"}; do
    if [[ $p == '!'* ]]; then coord_sdk_pats+="-$(coord_glob_re ${p#!})"; else coord_sdk_pats+="+$(coord_glob_re $p)"; fi
  done
}
# coord_takes FILE ARRAY: does the workflow whose patterns are in ARRAY (coord_inputs_init) take FILE? The last
# pattern that matches decides, as GitHub's own filter does.
coord_takes() {
  local e t=0
  for e in "${(@P)2}"; do [[ $1 =~ ${e:1} ]] && { [[ $e == +* ]] && t=1 || t=0; }; done
  (( t ))
}
# coord_input FILE: prints why FILE is a build input, or nothing. An input is anything under lampOS/crates/, any Cargo.*,
# anything under .github/workflows/, and anything else the ARM64 run or the SDK check takes: lampOS/** but the docs'
# Markdown and lampOS/builders/, with the docs either workflow takes back in (lampos-arm64.yml's list; lampos-sdk.yml's
# lampOS/docs/control* and skill-format*, Thirteen's P3 on #5289), plus the ARM64 run's .github scripts and the promote
# tools. Any other file either workflow's pull_request paths take is an input too (Thirteen's P3 on #5297): a path one
# of them gains outside the cases below would make merge-when-green.sh wait for that run (coord_takes) while the lane
# test sent the PR to the no-crates lane. Needs coord_inputs_init; with either workflow's paths unread, every lampOS doc
# is an input (fail closed).
coord_input() {
  case $1 in
    lampOS/crates/*) print "crates" ;;
    Cargo.*|*/Cargo.*) print "Cargo" ;;
    .github/workflows/*) print "workflows" ;;
    lampOS/builders/*) ;;
    lampOS/docs/*.md)
      if (( ! ${#coord_arm_pats} )) || coord_takes $1 coord_arm_pats; then print "a doc the ARM64 run reads"
      elif (( ! ${#coord_sdk_pats} )) || coord_takes $1 coord_sdk_pats; then print "a doc the SDK check reads"; fi ;;
    lampOS/*) print "an ARM64 input" ;;
    .github/scripts/lampos-*|.github/tools/lampos-promote/*) print "an ARM64 input" ;;
    *)
      if coord_takes $1 coord_arm_pats; then print "an ARM64 input"
      elif coord_takes $1 coord_sdk_pats; then print "an input the SDK check takes"; fi ;;
  esac
}
# coord_lane COMMIT [LAMP_VOICE]: the train lane of a PR whose head is COMMIT (part 3). Prints "nocrates" when no file
# it changes from its merge base with LAMP_VOICE (default origin/lamp-voice) is a build input (coord_input), else
# "crates: <the first input> (<why>)". Files that can't be read, or none, go to the crate lane (fail closed: that lane
# runs every check). Read-only; the caller runs coord_inputs_init first (once for many PRs).
coord_lane() {
  local lv=${2:-origin/lamp-voice} b files f k
  b=$(git merge-base $lv $1 2>/dev/null) || { print -r -- "crates: its files can't be read"; return 0; }
  files=$(git diff --no-renames --name-only $b $1 2>/dev/null) || { print -r -- "crates: its files can't be read"; return 0; }
  [[ -n $files ]] || { print -r -- "crates: it changes no file"; return 0; }
  for f in ${(f)files}; do k=$(coord_input $f); [[ -n $k ]] && { print -r -- "crates: $f ($k)"; return 0; }; done
  print -r -- nocrates
}
