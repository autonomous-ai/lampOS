#!/bin/bash
# lamp-sim-conformance.sh BIN_DIR: every SDK example against the virtual lamp, black
# box (sdk/js conformance/examples.js --sim): each example runs as a program of its
# own, paired with every grant as a person's program would be, and the lamp must
# show what it is for. Starts `lamp-sim run` from BIN_DIR (lamp-sim, lamp-body,
# lamp-control), ends it, and checks that no part of it is left running. About
# 2.5 minutes: the rows wait on the lamp's own clock (a debounce, a fade). Linux
# only (the real lamp-body runs there only). Exit 0 when every row held.
#   needs: python3 with the lampos SDK (pip install -e sdk/python), node with
#   sdk/js's dependencies (npm install there).
set -u
BIN=${1:?usage: lamp-sim-conformance.sh BIN_DIR}
HERE=$(cd "$(dirname "$0")/.." && pwd)
# A free port from the kernel unless LAMP_SIM_CONFORMANCE_PORT names one, so two
# runs on one machine do not collide.
PORT=${LAMP_SIM_CONFORMANCE_PORT:-$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}
# Short and private (socket paths hold 103 bytes; lamp-body wants a private
# folder outside any world-writable one).
DIR=$HOME/.lampsim-conf-$$
fail() { echo "lamp-sim-conformance: FAIL: $*" >&2; kill_lamp; exit 1; }
kill_lamp() { [ -n "${SIM:-}" ] && kill "$SIM" 2>/dev/null; sleep 2; }
mkdir -p "$DIR/home" && chmod 700 "$DIR" || exit 2
trap 'kill_lamp; rm -rf "$DIR"' EXIT

"$BIN/lamp-sim" run "$DIR/lamp" --bin-dir "$BIN" --listen 127.0.0.1:$PORT > "$DIR/run.log" 2>&1 &
SIM=$!
for _ in $(seq 1 50); do [ -S "$DIR/lamp/a.sock" ] && break; sleep 0.2; done
[ -S "$DIR/lamp/a.sock" ] || fail "the lamp did not come up ($(tail -n 2 "$DIR/run.log"))"
sleep 2

# Every grant (lamp-sim's default), as the examples ask for what they need.
export LAMPOS_HOME=$DIR/home PYTHONPATH="$HERE/sdk/python/src:${PYTHONPATH:-}"
code_json=$("$BIN/lamp-sim" code "$DIR/lamp") || fail "no pairing code"
code=$(printf '%s' "$code_json" | python3 -c 'import sys,json;print(json.load(sys.stdin)["code"])')
pin=$(printf '%s' "$code_json" | python3 -c 'import sys,json;print(json.load(sys.stdin)["pin"])')
python3 -m lampos.cli pair "$code" --pin "$pin" --host 127.0.0.1 --port $PORT > "$DIR/pair.log" 2>&1 \
  || fail "pairing: $(tail -n 2 "$DIR/pair.log")"
echo "paired: $(cat "$DIR/pair.log")"

( cd "$HERE/sdk/js" && timeout 600 node conformance/examples.js --sim "$DIR/lamp" --lamp-sim "$BIN/lamp-sim" ) > "$DIR/examples.log" 2>&1
rc=$?
cat "$DIR/examples.log"
[ $rc -eq 0 ] || fail "examples.js exited $rc"
echo "examples: $(grep -c ' ok$' "$DIR/examples.log") ok, $(grep -c skipped "$DIR/examples.log") skipped"

# Ending the lamp leaves nothing running.
kill "$SIM"; sleep 3; SIM=
left=$(ls /proc 2>/dev/null | grep -E '^[0-9]+$' | while read -r p; do cat /proc/$p/cmdline 2>/dev/null | tr '\0' ' ' | grep -q "$DIR/lamp" && echo "$p"; done)
[ -z "$left" ] || fail "parts left running after the lamp ended: $left"
echo "lamp-sim-conformance: ok"
