#!/usr/bin/env bash
# Every lamp-* binary the build made is in exactly one list of
# deploy/package-binaries.json, which the lamp-os packager reads.
# A binary in none would stop the next lamp-os build at packaging, a binary
# in two would be packaged as neither; both fail here, in the pull request
# that adds it, rather than at release.
#
#   tools/package-binaries.sh LIST_JSON BUILD_DIR
#
# BUILD_DIR is target/release (or wherever `cargo build --bins` put them).
# Exit 0 when every one is classified once, 1 otherwise (each said), 2 on
# wrong use.
set -euo pipefail
[ "$#" -eq 2 ] || { echo "usage: $0 LIST_JSON BUILD_DIR" >&2; exit 2; }
list=$1 dir=$2
[ -f "$list" ] || { echo "$list: not found" >&2; exit 2; }
[ -d "$dir" ] || { echo "$dir: not found" >&2; exit 2; }
classified=$(grep -o '"lamp-[a-z0-9-]*"' "$list" | tr -d '"' | sort)
bad=0
for twice in $(uniq -d <<<"$classified"); do
  echo "$twice is in more than one list of $list" >&2
  bad=1
done
for path in "$dir"/lamp-*; do
  [ -f "$path" ] && [ -x "$path" ] || continue
  name=$(basename "$path")
  case "$name" in *.d) continue ;; esac
  if ! grep -qx "$name" <<<"$classified"; then
    echo "$name is built but in no list of $list: add it to core, optional, provisioning or development" >&2
    bad=1
  fi
done
[ "$bad" -eq 0 ] && echo "every lamp-* binary in $dir is classified once"
exit "$bad"
