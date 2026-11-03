#!/bin/zsh
# lv-quiet.sh (28 Oct 20:4x, first builder): wait, at most 45 min, until no lamp-voice push build of lampos-arm64.yml is
# running, so a train never moves lamp-voice under a build that is still to publish: its sign-and-publish is refused
# (#3906, HTTP 403) once lamp-voice has moved past the tested commit, until the release token of #3934 exists.
# REPO_DIR from the environment (README.md), default: the repo holding this folder.
R=autonomous-ai/lampOS; cd "${REPO_DIR:-$(git -C ${0:A:h} rev-parse --show-toplevel)}" || exit 0
for i in {1..90}; do
  n=$(gh run list -R $R --workflow lampos-arm64.yml --branch lamp-voice --event push -L 5 --json status -q '[.[]|select(.status!="completed")]|length' 2>/dev/null)
  [[ $n == 0 ]] && exit 0
  [[ $i == 1 ]] && echo "$(date +%T) a lamp-voice build is still to publish: the merge waits for it"
  sleep 30
done
echo "$(date +%T) waited 45 min for a lamp-voice build; merging anyway"; exit 0
