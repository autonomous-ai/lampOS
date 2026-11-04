#!/bin/zsh
# merge-when-green.sh PR [--behind-unrelated] [--precheck PR...] (22 Oct 23:4x, the owner: "do the reviewer job yourself too ... so we can move faster"): wait for
# the PR's required checks to finish; merge it by rebase, pinned to its head, only when every check passed and the head
# already contains lamp-voice's latest commit (the owner's 22 Oct rule). Otherwise say why and stop. Never pushes.
# Paths from the environment (README.md): TOOLS_DIR (default: this folder), REPO_DIR (default: the repo holding it).
# 3 Nov 15:4x (the Reviewer-lead's P3 on #5068): the precheck is still-eligible.sh on the PRs after --precheck, run as
# a command with arguments. It used to be `eval "$PRECHECK"`, so a PRECHECK left in the caller's environment ran as code;
# that variable is ignored now.
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; R="${REPO_DIR:-$(git -C $TOOLS_DIR rev-parse --show-toplevel)}"; P=${1:?PR}; cd $R || exit 1
unset PRECHECK; typeset -a pre; pre=()
# 4 Nov (#5212 part 2): --behind-unrelated (the train bot's trains) lets a PR behind lamp-voice go on waiting and merge
# as it is when behind-unrelated.sh says lamp-voice gained only files the PR doesn't touch and no build input; the
# owner's 22 Oct "contains lamp-voice's tip" rule stands for every other case, and for every PR without the flag.
unrelated=0; [[ $2 == --behind-unrelated ]] && { unrelated=1; argv[2]=(); }
[[ $2 == --precheck ]] && pre=("${@[3,-1]}")
# DRY=1 (#5212): wait and decide as usual but change nothing: no lv-quiet wait, no merge; print what it would do. LV_AT
# (dry only) stands in for lamp-voice's tip in the behind decision, to replay a past case.
lvat=origin/lamp-voice; [[ ${DRY:-0} == 1 && -n $LV_AT ]] && lvat=$LV_AT
# behind_ok: behind-unrelated.sh's decision on this PR's head, printed; true only with --behind-unrelated and "merge as is".
behind_ok() { local d; (( unrelated )) || return 1; d=$($TOOLS_DIR/behind-unrelated.sh refs/remotes/pr/$P $lvat); local rc=$?; echo "$(date +%T) #$P behind lamp-voice: $d"; return $rc; }
# A PR whose every file the lampOS workflows skip (lampOS/docs/**/*.md and lampOS/builders/**, but not the seven docs
# they take back in) gets no checks at all, so waiting for them timed out trains 41 and 44 (23 Oct): it merges on the
# same current-base and precheck rules, with nothing to wait for.
files=$(gh pr diff $P -R autonomous-ai/lampOS --name-only)
nochecks=1
# 26 Oct 12:5x: a PR with no file the ARM64 workflow takes (lampos-arm64.yml's pull_request paths: lampOS/** but the
# docs' Markdown and builders/, plus its own .github files) gets no build-and-test or clippy check, only the others its
# files start (#3536, scripts/imager only: trains 157 and 158 waited 45 min twice for an ARM64 run that never comes).
# Such a PR merges once every check that did start has passed.
armless=1
for f in ${(f)files}; do
  case $f in
    lampOS/docs/attention.md|lampOS/docs/brain-build.md|lampOS/docs/endpoint-detection.md|lampOS/docs/endpoint-pilot.md|lampOS/docs/install.md|lampOS/docs/memory.md|lampOS/docs/switches.md) nochecks=0; armless=0 ;;
    lampOS/docs/*.md|lampOS/docs/*/*.md|lampOS/docs/*/*/*.md|lampOS/builders/*) ;;
    lampOS/*|.github/workflows/lampos-arm64.yml|.github/scripts/lampos-pr-crates.py|.github/scripts/lampos-source-times.py|.github/scripts/lampos-publish*.py|.github/tools/lampos-promote/*) nochecks=0; armless=0 ;;
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
      last_every=$every; sleep 30; continue
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
    git fetch -q origin lamp-voice "+refs/pull/$P/head:refs/remotes/pr/$P"
    asis=0
    if ! git merge-base --is-ancestor $lvat refs/remotes/pr/$P; then
      # 4 Nov (#5212): train 597 (#5201) was green and thrown away for a factory script alone. Behind only by unrelated
      # files, it merges as it is: by rebase, pinned to its head, after the same precheck.
      if behind_ok; then asis=1
      else echo "$(date +%T) #$P green but behind lamp-voice; not merged"; exit 3; fi
    fi
    if (( ${#pre} )) && ! $TOOLS_DIR/still-eligible.sh "${pre[@]}"; then echo "$(date +%T) #$P precheck failed: not merged"; exit 6; fi
    if [[ ${DRY:-0} == 1 ]]; then echo "$(date +%T) dry: #$P would merge by rebase at ${head:0:9}$( (( asis )) && echo ', as it is, behind lamp-voice')"; exit 0; fi
    gh pr merge $P -R autonomous-ai/lampOS --rebase --match-head-commit $head && echo "$(date +%T) #$P merged at ${head:0:9}$( (( asis )) && echo ', as it is, behind lamp-voice')" && exit 0
    # A refused merge as is goes back to the old path (exit 3, rebuild), never the failed-run path.
    (( asis )) && { echo "$(date +%T) #$P merge as is refused; rebuilding as behind lamp-voice"; exit 3; }
    echo "$(date +%T) #$P merge refused"; exit 4
  fi
  sleep 30
done
echo "$(date +%T) #$P checks not done in 45 min"; exit 5
