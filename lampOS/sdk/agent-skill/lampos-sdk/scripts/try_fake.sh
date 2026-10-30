#!/usr/bin/env bash
# Run a lamp program against a fresh fake lamp, then stop the fake.
#
#   try_fake.sh my_program.py     # or my_program.js
#
# The fake is paired in a scratch LAMPOS_HOME, so your real pairings are
# untouched. It answers every op (`--everything`). Needs `lamp` (the lampos
# SDK) on PATH, and python3 or node for the program.
set -euo pipefail

program="${1:?usage: try_fake.sh PROGRAM.py|PROGRAM.js}"
home="$(mktemp -d)"
export LAMPOS_HOME="$home"
log="$home/fake.log"

lamp fake --everything --every 30 >"$log" 2>&1 &
fake=$!
trap 'kill "$fake" 2>/dev/null || true; rm -rf "$home"' EXIT

# Wait until the fake has paired this scratch home.
for _ in $(seq 1 50); do
  grep -q "paired as" "$log" 2>/dev/null && break
  sleep 0.2
done
grep -q "paired as" "$log" || { echo "the fake lamp didn't start:"; cat "$log"; exit 1; }

case "$program" in
  *.js) node "$program" ;;
  *) python3 "$program" ;;
esac
