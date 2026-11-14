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
# which hit it on 10 Nov, and #6059, which hit it on 13 Nov; the Queue goes on in #6811 (#6809, opened beside it the same minute, is read too).
# The one place their numbers live: every one is read, so a HOLD on an older one still counts; the bot posts on the last.
COORD_QUEUE_ISSUES=(4710 5385 6059 6809 6811)
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

# coord_switch_on HEAD: prints "on" when the PR's own diff (from its merge base with origin/lamp-voice) adds a line
# `Environment=LAMP_…=on` to a product unit (lampOS/deploy/product/, a .service or .conf file), else "off"; fails when
# the diff can't be read (fail closed). 14 Nov (the readers' P3s on #7034): such a PR needs a real lamp row (rows.py
# --switch-on), not a reader's "no row needed". A git pathspec's * crosses folders, so units in subfolders count too.
# 14 Nov (the Reviewer-lead's P3s on #7041): the settings files (lampOS/deploy/product/settings/*.env, lines
# `LAMP_X=value` with no `Environment=`) count too, and so does any value but off, 0, false or empty, as switches take
# words ("LAMP_NAMES=checked", "LAMP_ATTENTION=busy-near"). A value may be quoted, the assignment too
# (`Environment="LAMP_X=on"`); a comment line (`#…`) never counts.
coord_switch_on() {
  local d
  d=$(git diff --no-color --no-ext-diff -U0 origin/lamp-voice...$1 -- 'lampOS/deploy/product/*.service' 'lampOS/deploy/product/*.conf' \
    'lampOS/deploy/product/settings/*.env' 2>/dev/null) || return 1
  awk -v q="'" '
    BEGIN { lead = "^[+][ \t]*(Environment=)?[\"" q "]?LAMP_[A-Z0-9_]+="; trim = "^[ \t\"" q "]+|[ \t\"" q "]+$"; on = 0 }
    $0 ~ lead { v = $0; sub(lead, "", v); gsub(trim, "", v); v = tolower(v)
                if (v != "off" && v != "0" && v != "false" && v != "") on = 1 }
    END { print (on ? "on" : "off") }' <<< "$d"
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

# coord_red_checks_jq: a jq filter over `gh pr view --json statusCheckRollup` printing "ok", or "red <name>, ..." for
# every check COMPLETED with anything but SUCCESS (FAILURE, CANCELLED, TIMED_OUT, STARTUP_FAILURE, ACTION_REQUIRED,
# ERROR, a null conclusion). SKIPPED and NEUTRAL never ran by design; a check still running is not red. A commit
# status reads as a check (its state). 12 Nov (#6556 B): the train bot's eligible() sits out a PR whose head has any.
coord_red_checks_jq='[(.statusCheckRollup // [])[]
  | {n: (.name // .context // "?"),
     s: (if .__typename == "StatusContext" then (if .state == "PENDING" or .state == "EXPECTED" then "PENDING" else "COMPLETED" end) else (.status // "") end),
     c: (if .__typename == "StatusContext" then .state else .conclusion end)}
  | select(.s == "COMPLETED" and .c != "SUCCESS" and .c != "SKIPPED" and .c != "NEUTRAL")]
  | if length == 0 then "ok" else "red " + (map(.n) | join(", ")) end'
# coord_arm64_green_jq: over the same JSON, "ok" when the PR's own ARM64 run is there and all green: its (build) and
# (release) jobs and every shard (k/N), one N, at least one shard, each COMPLETED with SUCCESS; else "waiting <what>".
# 12 Nov (#6556, train 841): #6572's head had no ARM64 pull_request run at the cut, and its release job went red after.
coord_arm64_green_jq='[(.statusCheckRollup // [])[] | select((.name // "") | startswith("lampOS — build and test on Linux ARM64 ("))
  | {j: (.name | capture("\\((?<j>[^()]*)\\)$").j), ok: (.status == "COMPLETED" and .conclusion == "SUCCESS")}] as $a
  | [$a[] | .j | select(test("^[0-9]+/[0-9]+$"))] as $sh
  | ([$sh[] | split("/")[1]] | unique) as $n
  | [ (if any($a[]; .j == "build") then empty else "no (build)" end),
      (if any($a[]; .j == "release") then empty else "no (release)" end),
      (if ($n | length) == 1 and ($sh | unique | length) == ($n[0] | tonumber) then empty else "shards " + ($sh | join(",") | if . == "" then "none" else . end) end),
      ($a[] | select(.ok | not) | "(\(.j)) not green") ]
  | if length == 0 then "ok" else "waiting " + join(", ") end'
# coord_arm_takes COMMIT [LAMP_VOICE]: true when the ARM64 run takes a file COMMIT changes from its merge base with
# LAMP_VOICE (default origin/lamp-voice), by lampos-arm64.yml's pull_request paths at COMMIT, as merge-when-green.sh
# reads them. Fails closed (true) when the files or the paths can't be read. Leaves the caller's coord_*_pats alone.
coord_arm_takes() {
  local b files f; local -a coord_arm_pats coord_sdk_pats
  b=$(git merge-base ${2:-origin/lamp-voice} $1 2>/dev/null) && files=$(git diff --no-renames --name-only $b $1 2>/dev/null) && [[ -n $files ]] || return 0
  coord_inputs_init $1; (( ${#coord_arm_pats} )) || return 0
  for f in ${(f)files}; do coord_takes $f coord_arm_pats && return 0; done
  return 1
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
# coord_wf_paths REF WORKFLOW [KEY]: the pull_request `paths` (or KEY, e.g. `paths-ignore`) of
# .github/workflows/WORKFLOW at REF, one pattern a line. Prints nothing when it can't be read. 12 Nov (#6556 B, the
# Reviewer-lead's P3 on #6586): an inline list (`paths-ignore: ['lampOS/**', ...]`, as ci.yml writes it) reads as the
# block list form does.
coord_wf_paths() {
  git show "$1:.github/workflows/$2" 2>/dev/null | awk -v key="${3:-paths}" '
    function emit(s) { gsub(/^[[:space:]]+|[[:space:]]+$/, "", s); gsub(/^'\''|'\''$/, "", s); gsub(/^"|"$/, "", s); if (s != "") print s }
    /^  pull_request:/ { pr = 1; next }
    pr && /^  [^ #]/ { pr = 0 }
    pr && $0 ~ ("^    " key ":[[:space:]]*\\[") {
      s = $0; sub(/^[^[]*\[/, "", s); sub(/\].*$/, "", s); n = split(s, a, ","); for (k = 1; k <= n; k++) emit(a[k]); next }
    pr && $0 ~ ("^    " key ":[[:space:]]*(#.*)?$") { p = 1; next }
    pr && p && /^      - / { s = $0; sub(/^      - /, "", s); sub(/[[:space:]]+#.*$/, "", s); emit(s); next }
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
# coord_wf_pats_init REF WORKFLOW: any one workflow's pull_request paths at REF into coord_wf_pats, as
# coord_inputs_init reads the two. 12 Nov (#6556 B): the train bot reads the paths of a train's red workflow (SDK, doc
# links, virtual lamp) to find the carried PRs that workflow takes. Empty when they can't be read.
# 12 Nov (the Reviewer-lead's P3 on #6586): a workflow with only `paths-ignore` (ci.yml's V1 checks) takes a file none
# of those patterns match: "+" everything, then a "-" for each, so a PR is taken when some changed file is not ignored.
# A workflow with a pull_request trigger and neither list takes every PR: that names nobody here (empty), so the bot
# stops for the first builder rather than sit out a whole train.
coord_wf_pats_init() {
  local p ign; typeset -ga coord_wf_pats; coord_wf_pats=()
  for p in ${(f)"$(coord_wf_paths $1 $2)"}; do
    if [[ $p == '!'* ]]; then coord_wf_pats+="-$(coord_glob_re ${p#!})"; else coord_wf_pats+="+$(coord_glob_re $p)"; fi
  done
  (( ${#coord_wf_pats} )) && return 0
  ign=(${(f)"$(coord_wf_paths $1 $2 paths-ignore)"}); (( ${#ign} )) || return 0
  coord_wf_pats=('+^.*$'); for p in $ign; do coord_wf_pats+="-$(coord_glob_re $p)"; done
}
# coord_red_jobs SHA LOG: every failed (or timed-out) job of every workflow run on SHA, one "<workflow path> <job name>"
# a line, with each such job's own log appended to LOG. 12 Nov (#6556 B, train 833): a job's log is served as soon as
# that job ends, while `gh run view --log-failed` serves nothing until the whole run has, and since #6513 a run's other
# shards run on after one fails.
# Returns 3 when any red job's log came back empty (just ended, not served yet), else 0.
coord_red_jobs() {
  local r rid rpath j one empty=0
  for r in ${(f)"$(gh api "repos/$REPO/actions/runs?head_sha=$1&per_page=50" --jq '.workflow_runs[]|"\(.id) \(.path)"' 2>/dev/null)"}; do
    rid=${r%% *}; rpath=${r#* }; [[ $rid == <-> ]] || continue
    for j in ${(f)"$(gh api "repos/$REPO/actions/runs/$rid/jobs?per_page=100" --paginate --jq '.jobs[]|select(.conclusion=="failure" or .conclusion=="timed_out")|"\(.id) \(.name)"' 2>/dev/null)"}; do
      [[ ${j%% *} == <-> ]] || continue
      print -r -- "$rpath ${j#* }"
      one=$(gh api "repos/$REPO/actions/jobs/${j%% *}/logs" 2>/dev/null)
      if [[ -n $one ]]; then print -r -- $one >> $2; else empty=1; fi
    done
  done
  (( empty )) && return 3
  return 0
}
# coord_red_jobs_wait SHA LOG: coord_red_jobs, looked at again while any red job's log came back empty, at most
# COORD_RED_TRIES looks (default 3) COORD_RED_WAIT_S apart (default 20; the tests set 0); LOG holds the last look's.
# 12 Nov (the Reviewer-lead's P3 on #6586): one empty log among several used to be read as no failing test named.
coord_red_jobs_wait() {
  local i out rc
  for (( i = 1; i <= ${COORD_RED_TRIES:-3}; i++ )); do
    : > $2; out=$(coord_red_jobs $1 $2); rc=$?
    (( rc == 3 && i < ${COORD_RED_TRIES:-3} )) || break
    sleep ${COORD_RED_WAIT_S:-20}
  done
  [[ -n $out ]] && print -r -- $out
  return 0
}
# coord_prs_taken REF WORKFLOW PR...: the PRs (of those given) with a file WORKFLOW's pull_request paths at REF take,
# one a line. 12 Nov (#6556 B): a train red only in that workflow sits them out. Nothing when the paths can't be read.
coord_prs_taken() {
  local ref=$1 wf=$2 p f; shift 2
  coord_wf_pats_init $ref $wf; (( ${#coord_wf_pats} )) || return 0
  for p in "$@"; do
    for f in ${(f)"$(gh pr diff $p -R $REPO --name-only 2>/dev/null)"}; do coord_takes $f coord_wf_pats && { print -r -- $p; break; }; done
  done
  return 0
}
# coord_sit_out SITOUT CARRIED TOLD WHY PR...: sit each PR out on the head the train carried (CARRIED: "<PR> <head>"
# lines, written as the train was built), one "<PR> <head>" line in SITOUT, and comment on it once per head, with WHY,
# keyed in TOLD. 12 Nov (the Reviewer-lead's P3s on #6586): the head fetched after the failure could be a push made
# while the train ran, which then sat out unseen; and a red virtual lamp (lampos-sim.yml takes lampOS/crates/**) sits
# out every crate PR, so each one is told why and that a push brings it back. A PR missing from CARRIED is skipped.
coord_sit_out() {
  local so=$1 cf=$2 tf=$3 why=$4 p ch; shift 4
  for p in "$@"; do
    ch=$(awk -v p=$p '$1 == p { print $2; exit }' $cf 2>/dev/null); [[ -n $ch ]] || continue
    grep -qx "$p $ch" $so 2>/dev/null || print -r -- "$p $ch" >> $so
    print -r -- $p
    grep -qx "$p sitout-told $ch" $tf 2>/dev/null && continue
    gh pr comment $p -R $REPO --body "Train bot, $(date +%H:%M): $why This PR sits out at ${ch:0:9} with no HOLD, and the next push to it brings it back. If the failure isn't this PR's, a push of the same patch (a rebase onto lamp-voice) is enough." >/dev/null \
      && print -r -- "$p sitout-told $ch" >> $tf
  done
}
# coord_sitout_trim SITOUT OPEN: keep only SITOUT's lines whose PR is in OPEN ("<PR> <head> ..." lines, the round's open
# PRs) at that same head, so a closed PR or a pushed one leaves the file.
coord_sitout_trim() {
  [[ -s $1 ]] || return 0
  awk 'NR == FNR { h[$1] = $2; next } ($1 in h) && h[$1] == $2' $2 $1 > $1.new && mv $1.new $1
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
