#!/usr/bin/env bash
# tools/package-binaries.sh against a made-up build folder.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
check="$here/package-binaries.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
cat >"$work/list.json" <<'JSON'
{"core": ["lamp-voice"], "optional": ["lamp-peek"], "provisioning": [], "development": ["lamp-station"]}
JSON
for name in lamp-voice lamp-peek lamp-station; do
  printf '#!/bin/sh\n' >"$work/bin/$name" && chmod +x "$work/bin/$name"
done
touch "$work/bin/lamp-voice.d"
bash "$check" "$work/list.json" "$work/bin" >/dev/null || { echo "FAIL: all classified, still refused"; exit 1; }
printf '#!/bin/sh\n' >"$work/bin/lamp-new" && chmod +x "$work/bin/lamp-new"
if out=$(bash "$check" "$work/list.json" "$work/bin" 2>&1); then echo "FAIL: an unclassified binary passed"; exit 1; fi
grep -q "lamp-new is built but in no list" <<<"$out" || { echo "FAIL: said $out"; exit 1; }
rm "$work/bin/lamp-new"
sed -i.bak 's/"lamp-station"\]/"lamp-station", "lamp-peek"]/' "$work/list.json"
if out=$(bash "$check" "$work/list.json" "$work/bin" 2>&1); then echo "FAIL: a binary in two lists passed"; exit 1; fi
grep -q "lamp-peek is in more than one list" <<<"$out" || { echo "FAIL: said $out"; exit 1; }
set +e; bash "$check" "$work/list.json" >/dev/null 2>&1; code=$?; set -e
[ "$code" -eq 2 ] || { echo "FAIL: wrong use gave $code"; exit 1; }
echo "package-binaries: ok"
