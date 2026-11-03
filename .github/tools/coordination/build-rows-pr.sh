#!/bin/zsh
# build-rows-pr.sh (20 Oct 17:5x): lampOS/docs/build-rows.md regenerated from the review page's tables, and, when it
# changed beyond its time line, a docs-only PR onto lamp-voice from a new branch first/build-rows-MMDD-HHMM (the lead on #1711).
# 3 Nov: paths from the environment (README.md):
#   TOOLS_DIR         these scripts (default: this script's folder)
#   LAMP_COORD_STATE  work files: labels.json, gen/ (default: ~/.lamp-coordination)
#   REPO_DIR          the checkout gh runs in (default: the repo holding TOOLS_DIR)
#   ROWS_WORKTREE     a clean worktree the rows branch is made in (default: $LAMP_COORD_STATE/wt-rows, added if missing)
#   ROWS_SOURCE       the review page's build.py, private and not in the repo (default: $LAMP_COORD_STATE/review-page/build.py)
#   PRIVATE_NAMES     names that must never reach the public file, one extended regex a line, not in the repo
#                     (default: $LAMP_COORD_STATE/private-names.txt; missing or empty: stop)
#   PYTHON            default: python3
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; PYTHON="${PYTHON:-python3}"
SP="${LAMP_COORD_STATE:-$HOME/.lamp-coordination}"; mkdir -p $SP/gen || exit 1
R="${REPO_DIR:-$(git -C $TOOLS_DIR rev-parse --show-toplevel)}"; W="${ROWS_WORKTREE:-$SP/wt-rows}"
ROWS_SOURCE="${ROWS_SOURCE:-$SP/review-page/build.py}"; NAMES="${PRIVATE_NAMES:-$SP/private-names.txt}"
[[ -s $ROWS_SOURCE ]] || { echo "no review page at $ROWS_SOURCE (set ROWS_SOURCE): stop"; exit 1; }
names=$(grep -vE '^[[:space:]]*(#|$)' $NAMES 2>/dev/null | paste -sd'|' -)
[[ -n $names ]] || { echo "no private names in $NAMES (set PRIVATE_NAMES): stop"; exit 1; }
[[ -e $W/.git ]] || git -C $R worktree add -q --detach $W origin/lamp-voice || exit 1
# 24 Oct 16:0x: rebuilt from the session transcript after a reboot wiped /tmp; helpers in $TOOLS_DIR, work files in $SP.
git -C $W status --short | grep -q . && { echo "$W has changes: stop"; exit 1; }
git -C $W fetch -q origin +refs/heads/lamp-voice:refs/remotes/origin/lamp-voice || exit 1
# 29 Oct 00:3x: the scratchpad's midnight sweep removed labels.json and gutted the rows worktree; refresh the labels each run.
(cd $R && gh issue list -R autonomous-ai/lampOS --state all --limit 6000 --json number,labels -q 'map({key:(.number|tostring), value:[.labels[].name]})|from_entries') > $SP/labels.new && [[ -s $SP/labels.new ]] && mv $SP/labels.new $SP/labels.json
# 3 Nov 15:4x: the private names go in too, so a builder label that matches one is left out of the Owners column.
$PYTHON -I $TOOLS_DIR/gen-build-rows.py $ROWS_SOURCE $SP/gen/build-rows.md $SP/labels.json $NAMES || exit 1
git -C $W show origin/lamp-voice:lampOS/docs/build-rows.md > $SP/gen/build-rows.base.md 2>/dev/null || : > $SP/gen/build-rows.base.md
if diff -q <(grep -v '^Written by' $SP/gen/build-rows.md) <(grep -v '^Written by' $SP/gen/build-rows.base.md) >/dev/null; then echo "unchanged"; exit 0; fi
grep -niE "\\| You, |your report|$names" $SP/gen/build-rows.md && { echo "a name or an owner-addressed note: stop"; exit 1; }
B=first/build-rows-$(date +%m%d-%H%M); figures=$(grep -m1 '^\*\*Build' $SP/gen/build-rows.md | tr -d '*')
git -C $W checkout -q -B $B origin/lamp-voice && cp $SP/gen/build-rows.md $W/lampOS/docs/build-rows.md && git -C $W add lampOS/docs/build-rows.md && git -C $W commit -q -m "lampOS: docs/build-rows.md as at $(date +%H:%M): ${figures%.}" && git -C $W push -q origin $B 2>&1 | grep -v '^remote:'
# 25 Oct 10:2x: #3438's commit carried three other files (onboarding as it was before #3435), from a stale tree while
# train 114 landed. The commit must hold this one file against lamp-voice, or nothing is pushed.
changed=$(git -C $W diff --name-only origin/lamp-voice HEAD)
[[ "$changed" == "lampOS/docs/build-rows.md" ]] || { echo "the rows commit holds more than docs/build-rows.md: $changed; stop"; git -C $W checkout -q --detach origin/lamp-voice; git -C $W branch -q -D $B; exit 1; }
print -r -- "The build rows and quality rows as at $(date +%H:%M) (+07), regenerated from the first builder's review page, as #2285 set up for the lead (#1711). Docs only, the one file. ${figures}" > $SP/gen/build-rows-pr.md
(cd $R && gh pr create -B lamp-voice -H $B -t "lampOS: docs/build-rows.md as at $(date +%H:%M): ${figures%.}" -F $SP/gen/build-rows-pr.md -l builder:first </dev/null | tail -1)
git -C $W checkout -q --detach
