#!/bin/zsh
# Rewrites lamp/SHA256SUMS after any edit to the lamp's files (setup.sh checks it), syntax-checks
# them, and builds $K/lamp5194.tgz (what goes to /run/kit5194) with no AppleDouble files.
set -eu
K=${0:A:h:h}; cd $K/lamp
find . -name "._*" -delete
F=(common.sh setup.sh switch.sh q.sh putback.sh chg.awk)
shasum -a 256 $F > SHA256SUMS && cat SHA256SUMS
for f in common.sh setup.sh switch.sh q.sh putback.sh; do sh -n $f; done; echo "syntax ok"
COPYFILE_DISABLE=1 tar --exclude='._*' -czf $K/lamp5194.tgz $F SHA256SUMS
tar -tzf $K/lamp5194.tgz | grep -q '\._' && { echo "AppleDouble file in the tgz"; exit 1; }
shasum -a 256 $K/lamp5194.tgz
