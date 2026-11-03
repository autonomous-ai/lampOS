#!/bin/zsh
# merge-when-green.sh PR [--precheck PR...] (22 Oct 23:4x, the owner: "do the reviewer job yourself too ... so we can move faster"): wait for
# the PR's required checks to finish; merge it by rebase, pinned to its head, only when every check passed and the head
# already contains lamp-voice's latest commit (the owner's 22 Oct rule). Otherwise say why and stop. Never pushes.
# Paths from the environment (README.md): TOOLS_DIR (default: this folder), REPO_DIR (default: the repo holding it).
# 3 Nov 15:4x (the Reviewer-lead's P3 on #5068): the precheck is still-eligible.sh on the PRs after --precheck, run as
# a command with arguments. It used to be `eval "$PRECHECK"`, so a PRECHECK left in the caller's environment ran as code;
# that variable is ignored now.
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; R="${REPO_DIR:-$(git -C $TOOLS_DIR rev-parse --show-toplevel)}"; P=${1:?PR}; cd $R || exit 1
unset PRECHECK; typeset -a pre; pre=()
[[ $2 == --precheck ]] && pre=("${@[3,-1]}")
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
for i in {1..90}; do
  head=$(gh pr view $P -R autonomous-ai/lampOS --json headRefOid -q .headRefOid)
  # 27 Oct 15:5x (the reviewer-lead: trains kept running on a tip that had moved): every other look, a lamp-voice that
  # moved past this PR's base ends the wait at once, rather than after a full green run that can't merge.
  if (( i % 2 == 0 )); then
    git fetch -q origin lamp-voice "+refs/pull/$P/head:refs/remotes/pr/$P"
    if ! git merge-base --is-ancestor origin/lamp-voice refs/remotes/pr/$P; then echo "$(date +%T) #$P behind lamp-voice while its checks ran; not merged"; exit 3; fi
  fi
  states=$(gh pr view $P -R autonomous-ai/lampOS --json statusCheckRollup --jq '[.statusCheckRollup[]|select(.name|test("build and test|clippy"))|.conclusion]|join(" ")')
  if [[ $states == *FAILURE* || $states == *CANCELLED* ]]; then echo "$(date +%T) #$P checks failed: $states"; exit 2; fi
  # No ARM64 for this PR: every check that started, finished and passed, the same two looks 30 s apart, from 3 min on.
  if (( armless )); then
    every=$(gh pr view $P -R autonomous-ai/lampOS --json statusCheckRollup --jq '[.statusCheckRollup[]|"\(.status) \(.conclusion)"]|join(",")')
    if [[ $every == *FAILURE* || $every == *CANCELLED* || $every == *TIMED_OUT* ]]; then echo "$(date +%T) #$P checks failed: $every"; exit 2; fi
    if (( i >= 6 )) && [[ -n $every && $every != *IN_PROGRESS* && $every != *QUEUED* && $every != *PENDING* && $every == "$last_every" ]]; then
      echo "$(date +%T) #$P has no ARM64 run (no file it takes): its $(echo $every | tr ',' '\n' | wc -l | tr -d ' ') checks passed"
      states="SUCCESS SUCCESS"
    else
      last_every=$every; sleep 30; continue
    fi
  fi
  if [[ $nochecks == 1 ]] || [[ -n $states && $states != *"  "* && $(echo $states | wc -w) -ge 2 && $states != *null* && $states == *SUCCESS*SUCCESS* ]]; then
    [[ $nochecks == 1 ]] && echo "$(date +%T) #$P is docs the workflows skip: no checks to wait for"
    # 28 Oct 20:4x (first builder): never move lamp-voice under a build still to publish (#3906's 403); the
    # ancestry check below runs after the wait, on the lamp-voice the merge will meet.
    $TOOLS_DIR/lv-quiet.sh
    git fetch -q origin lamp-voice "+refs/pull/$P/head:refs/remotes/pr/$P"
    if ! git merge-base --is-ancestor origin/lamp-voice refs/remotes/pr/$P; then echo "$(date +%T) #$P green but behind lamp-voice; not merged"; exit 3; fi
    if (( ${#pre} )) && ! $TOOLS_DIR/still-eligible.sh "${pre[@]}"; then echo "$(date +%T) #$P precheck failed: not merged"; exit 6; fi
    gh pr merge $P -R autonomous-ai/lampOS --rebase --match-head-commit $head && echo "$(date +%T) #$P merged at ${head:0:9}" && exit 0
    echo "$(date +%T) #$P merge refused"; exit 4
  fi
  sleep 30
done
echo "$(date +%T) #$P checks not done in 45 min"; exit 5
