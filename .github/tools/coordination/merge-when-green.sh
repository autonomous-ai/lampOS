#!/bin/zsh
# merge-when-green.sh PR [--behind-unrelated] [--no-crates] [--precheck PR...] (22 Oct 23:4x, the owner: "do the reviewer job yourself too ... so we can move faster"): wait for
# the PR's required checks to finish; merge it by rebase, pinned to its head, only when every check passed and the head
# already contains lamp-voice's latest commit (the owner's 22 Oct rule). Otherwise say why and stop. Never pushes.
# Paths from the environment (README.md): TOOLS_DIR (default: this folder), REPO_DIR (default: the repo holding it).
# 3 Nov 15:4x (the Reviewer-lead's P3 on #5068): the precheck is still-eligible.sh on the PRs after --precheck, run as
# a command with arguments. It used to be `eval "$PRECHECK"`, so a PRECHECK left in the caller's environment ran as code;
# that variable is ignored now.
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; R="${REPO_DIR:-$(git -C $TOOLS_DIR rev-parse --show-toplevel)}"; P=${1:?PR}; cd $R || exit 1
unset PRECHECK; typeset -a pre; pre=()
# MWG_POLL: seconds between looks (default 30; the tests set 0).
poll=${MWG_POLL:-30}
# 4 Nov (#5212 part 2): --behind-unrelated (the train bot's trains) lets a PR behind lamp-voice go on waiting and merge
# as it is when behind-unrelated.sh says lamp-voice gained only files the PR doesn't touch and no build input; the
# owner's 22 Oct "contains lamp-voice's tip" rule stands for every other case, and for every PR without the flag.
# 4 Nov (#5212 part 3): --no-crates (the no-crates lane's trains) passes --no-crates to behind-unrelated.sh, so a crate
# train's merge under this train doesn't make it rebuild when the files are disjoint.
unrelated=0; nocrates=0
while [[ $2 == --behind-unrelated || $2 == --no-crates ]]; do
  [[ $2 == --behind-unrelated ]] && unrelated=1; [[ $2 == --no-crates ]] && nocrates=1; argv[2]=()
done
[[ $2 == --precheck ]] && pre=("${@[3,-1]}")
# DRY=1 (#5212): wait and decide as usual but change nothing: no lv-quiet wait, no merge; print what it would do. LV_AT
# (dry only) stands in for lamp-voice's tip in the behind decision, to replay a past case.
lvat=origin/lamp-voice; [[ ${DRY:-0} == 1 && -n $LV_AT ]] && lvat=$LV_AT
# merge_once: the merge, pinned to the head. 5 Nov (the PM, train 662): GitHub answered a green train's merge with a 502
# and the bot stopped. On GitHub's 5xx the PR is looked at first (a 5xx may come after the merge went through, and the
# next try's refusal too), then after MWG_RETRY_S (default 30; the tests set 0) merge_ready decides again, as lamp-voice
# may have moved or a carried PR been held meanwhile, and the merge is tried again, at most 3 tries; `server` says the
# last was a 5xx. A gh timeout or reset names no 5xx, so it is a refused merge, as before.
server=0; tries=0
merge_once() {
  local out five
  for tries in 1 2 3; do
    out=$(gh pr merge $P -R autonomous-ai/lampOS --rebase --match-head-commit $head 2>&1) && return 0
    [[ -n $out ]] && print -r -- "$out"
    five=0; grep -qE 'HTTP 5[0-9][0-9]' <<< "$out" && five=1
    (( five || server )) && [[ $(gh pr view $P -R autonomous-ai/lampOS --json state --jq .state 2>/dev/null) == MERGED ]] && { server=0; return 0; }
    server=$five; (( server )) || return 1
    echo "$(date +%T) #$P merge answered with GitHub's 5xx (try $tries of 3)"
    (( tries < 3 )) && { sleep ${MWG_RETRY_S:-30}; merge_ready; }
  done
  return 1
}
# merge_ready: on the lamp-voice the merge will meet, the PR contains its tip, or may merge as it is; else exit 3
# (rebuild). And every carried PR is still eligible; else exit 6. Before the first try and before each one after a 5xx
# (the first read's P2 on this retry: lamp-voice has no branch protection, so GitHub refuses no merge onto a newer tip).
merge_ready() {
  git fetch -q origin lamp-voice "+refs/pull/$P/head:refs/remotes/pr/$P"
  asis=0
  if ! git merge-base --is-ancestor $lvat refs/remotes/pr/$P; then
    # 4 Nov (#5212): train 597 (#5201) was green and thrown away for a factory script alone. Behind only by unrelated
    # files, it merges as it is: by rebase, pinned to its head, after the same precheck.
    if behind_ok; then asis=1
    else echo "$(date +%T) #$P green but behind lamp-voice; not merged"; exit 3; fi
  fi
  if (( ${#pre} )) && ! $TOOLS_DIR/still-eligible.sh "${pre[@]}"; then echo "$(date +%T) #$P precheck failed: not merged"; exit 6; fi
}
# behind_ok: behind-unrelated.sh's decision on this PR's head, printed; true only with --behind-unrelated and "merge as is".
behind_ok() { local d; (( unrelated )) || return 1; d=$($TOOLS_DIR/behind-unrelated.sh ${${nocrates:#0}:+--no-crates} refs/remotes/pr/$P $lvat); local rc=$?; echo "$(date +%T) #$P behind lamp-voice: $d"; return $rc; }
# A PR whose every file the lampOS workflows skip (the docs' Markdown no workflow takes back in, and lampOS/builders/**)
# gets no checks at all, so waiting for them timed out trains 41 and 44 (23 Oct): it merges on the same current-base
# and precheck rules, with nothing to wait for.
# 26 Oct 12:5x: a PR with no file the ARM64 workflow takes gets no build-and-test or clippy check, only the others its
# files start (#3536, scripts/imager only: trains 157 and 158 waited 45 min twice for an ARM64 run that never comes).
# Such a PR merges once every check that did start has passed.
# 4 Nov (#5212 part 3): both tests read the workflows' own pull_request paths (lib.sh, coord_inputs_init and
# coord_takes) from the PR's head, not a list kept here: the seven docs written here had fallen behind the workflow's
# twelve, so a setup.md-only PR merged without waiting for the ARM64 run it starts. Paths that can't be read: the ARM64
# run is waited for (fail closed).
source $TOOLS_DIR/lib.sh
files=$(gh pr diff $P -R autonomous-ai/lampOS --name-only)
git fetch -q origin "+refs/pull/$P/head:refs/remotes/pr/$P" 2>/dev/null
wref=refs/remotes/pr/$P; git rev-parse -q --verify "$wref^{commit}" >/dev/null || wref=origin/lamp-voice
coord_inputs_init $wref
nochecks=1; armless=1
for f in ${(f)files}; do
  if ! (( ${#coord_arm_pats} )) || coord_takes $f coord_arm_pats; then nochecks=0; armless=0; continue; fi
  case $f in
    lampOS/builders/*) ;;
    lampOS/docs/*.md) [[ -z $(coord_input $f) ]] || nochecks=0 ;;
    *) nochecks=0 ;;
  esac
done
[[ -n $files ]] || { nochecks=0; armless=0; }
(( nochecks )) && armless=0
# 3 Nov 16:4x (Thirteen's P2 on #5068): the checks are counted with jq, not matched as a string. join(" ") turned a null
# conclusion into "", so "SUCCESS SUCCESS " with a third check pending passed `*SUCCESS*SUCCESS*`. Now:
# checks MODE prints "<checks> <passed> <failed> <running> <name: status conclusion, ...>" over the PR's checks. A check
# run's status and conclusion, or a commit status's state, read as one. Passed is COMPLETED with SUCCESS. Failed is
# COMPLETED with anything else: FAILURE, CANCELLED, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, STALE, ERROR or a null
# conclusion. Running is any other status (QUEUED, IN_PROGRESS, PENDING, WAITING ...). MODE named: only the ARM64
# "build and test" and "clippy" checks, a skipped one failed too. MODE every: all checks but those that never ran by
# design (SKIPPED, NEUTRAL: the promote, publish and cache jobs a PR run skips). A gh error prints nothing: not green.
checks() {
  gh pr view $P -R autonomous-ai/lampOS --json statusCheckRollup 2>/dev/null | jq -r --arg mode $1 '
    [.statusCheckRollup[]
     | {n: (.name // .context // "?"),
        s: (if .__typename == "StatusContext" then (if .state == "PENDING" or .state == "EXPECTED" then "PENDING" else "COMPLETED" end) else (.status // "") end),
        c: (if .__typename == "StatusContext" then .state else .conclusion end)}
     | select(if $mode == "named" then (.n | test("build and test|clippy")) else ((.c == "SKIPPED" or .c == "NEUTRAL") | not) end)]
    | "\(length) \(map(select(.s == "COMPLETED" and .c == "SUCCESS")) | length) \(map(select(.s == "COMPLETED" and .c != "SUCCESS")) | length) \(map(select(.s != "COMPLETED")) | length) \(map("\(.n): \(.s) \(.c)") | join(", "))"' 2>/dev/null
}
for i in {1..90}; do
  head=$(gh pr view $P -R autonomous-ai/lampOS --json headRefOid -q .headRefOid)
  # 27 Oct 15:5x (the reviewer-lead: trains kept running on a tip that had moved): every other look, a lamp-voice that
  # moved past this PR's base ends the wait at once, rather than after a full green run that can't merge.
  if (( i % 2 == 0 )); then
    git fetch -q origin lamp-voice "+refs/pull/$P/head:refs/remotes/pr/$P"
    if ! git merge-base --is-ancestor origin/lamp-voice refs/remotes/pr/$P; then
      # 4 Nov (#5212): behind only by unrelated files, the wait goes on (said once per lamp-voice head).
      lvh=$(git rev-parse origin/lamp-voice)
      if [[ $lvh == $ok_lv ]]; then :
      elif behind_ok; then ok_lv=$lvh
      else echo "$(date +%T) #$P behind lamp-voice while its checks ran; not merged"; exit 3; fi
    fi
  fi
  green=0
  if (( armless )); then
    # No ARM64 for this PR: every check that ran, finished and passed (at least one), the same in two looks 30 s apart,
    # from 3 min on.
    every=$(checks every); total=0 passed=0 failed=0 running=0
    [[ -n $every ]] && read -r total passed failed running rest <<< "$every"
    if (( failed )); then echo "$(date +%T) #$P checks failed: ${every#* * * * }"; exit 2; fi
    if (( i >= 6 && total >= 1 && passed == total && running == 0 )) && [[ $every == "$last_every" ]]; then
      echo "$(date +%T) #$P has no ARM64 run (no file it takes): its $passed checks passed"
      green=1
    else
      last_every=$every; sleep $poll; continue
    fi
  else
    # The ARM64 build and test and clippy: at least two, every one COMPLETED with SUCCESS.
    named=$(checks named); total=0 passed=0 failed=0 running=0
    [[ -n $named ]] && read -r total passed failed running rest <<< "$named"
    if (( failed )); then echo "$(date +%T) #$P checks failed: ${named#* * * * }"; exit 2; fi
    (( total >= 2 && passed == total )) && green=1
  fi
  if (( nochecks || green )); then
    [[ $nochecks == 1 ]] && echo "$(date +%T) #$P is docs the workflows skip: no checks to wait for"
    # 28 Oct 20:4x (first builder): never move lamp-voice under a build still to publish (#3906's 403); the
    # ancestry check below runs after the wait, on the lamp-voice the merge will meet.
    [[ ${DRY:-0} == 1 ]] || $TOOLS_DIR/lv-quiet.sh
    merge_ready
    if [[ ${DRY:-0} == 1 ]]; then echo "$(date +%T) dry: #$P would merge by rebase at ${head:0:9}$( (( asis )) && echo ', as it is, behind lamp-voice')"; exit 0; fi
    merge_once && echo "$(date +%T) #$P merged at ${head:0:9}$( (( asis )) && echo ', as it is, behind lamp-voice')" && exit 0
    # 5 Nov (the PM, train 662): a merge GitHub answered with 5xx three times is GitHub's, not the train's: exit 7, the same
    # PRs go again, never the failed-run path that stops the bot at once.
    (( server )) && { echo "$(date +%T) #$P merge answered with GitHub's 5xx $tries times; rebuilding"; exit 7; }
    # A refused merge as is goes back to the old path (exit 3, rebuild), never the failed-run path.
    (( asis )) && { echo "$(date +%T) #$P merge as is refused; rebuilding as behind lamp-voice"; exit 3; }
    # 4 Nov (#5212 part 3): with two lanes, the other lane can move lamp-voice between the check above and the merge.
    # A merge refused because lamp-voice moved is a rebuild (exit 3), not a failure that stops the bot.
    git fetch -q origin lamp-voice
    if ! git merge-base --is-ancestor origin/lamp-voice refs/remotes/pr/$P; then echo "$(date +%T) #$P merge refused; lamp-voice moved meanwhile: rebuilding"; exit 3; fi
    echo "$(date +%T) #$P merge refused"; exit 4
  fi
  sleep $poll
done
echo "$(date +%T) #$P checks not done in 45 min"; exit 5
