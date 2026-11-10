#!/bin/bash
# lamp-sim-check.sh BIN_DIR: the SDKs against the virtual lamp, with no special
# mode. Starts `lamp-sim run` from BIN_DIR (lamp-sim, lamp-body, lamp-control),
# pairs the Python SDK's CLI and the JS SDK with it, runs the JS SDK's tour
# (every kind of call, unchanged) and a Python SDK round trip, then ends the
# lamp and checks that no part of it is left running. Linux only (the real
# lamp-body runs there only). Exit 0 when all of it held.
#   needs: python3 with the lampos SDK on PYTHONPATH (sdk/python/src), node.
set -u
BIN=${1:?usage: lamp-sim-check.sh BIN_DIR}
HERE=$(cd "$(dirname "$0")/.." && pwd)
# A free port from the kernel unless LAMP_SIM_CHECK_PORT names one, so two runs
# on one machine do not collide (python3 is needed anyway).
PORT=${LAMP_SIM_CHECK_PORT:-$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')}
# Short and private (socket paths hold 103 bytes; lamp-body wants a private
# folder outside any world-writable one).
DIR=$HOME/.lampsim-check-$$
HOMEDIR=$DIR/home
fail() { echo "lamp-sim-check: FAIL: $*" >&2; kill_lamp; exit 1; }
kill_lamp() { [ -n "${SIM:-}" ] && kill "$SIM" 2>/dev/null; sleep 2; }
mkdir -p "$HOMEDIR" && chmod 700 "$DIR" || exit 2
trap 'kill_lamp; rm -rf "$DIR"' EXIT

"$BIN/lamp-sim" run "$DIR/lamp" --bin-dir "$BIN" --listen 127.0.0.1:$PORT > "$DIR/run.log" 2>&1 &
SIM=$!
for _ in $(seq 1 50); do [ -S "$DIR/lamp/a.sock" ] && break; sleep 0.2; done
[ -S "$DIR/lamp/a.sock" ] || fail "the lamp did not come up ($(tail -n 2 "$DIR/run.log"))"
sleep 2

export LAMPOS_HOME=$HOMEDIR PYTHONPATH="$HERE/sdk/python/src:${PYTHONPATH:-}"
code_json=$("$BIN/lamp-sim" code "$DIR/lamp" --grants body,light,speak,look,hear,air) || fail "no pairing code"
code=$(printf '%s' "$code_json" | python3 -c 'import sys,json;print(json.load(sys.stdin)["code"])')
pin=$(printf '%s' "$code_json" | python3 -c 'import sys,json;print(json.load(sys.stdin)["pin"])')
python3 -m lampos.cli pair "$code" --pin "$pin" --host 127.0.0.1 --port $PORT > "$DIR/pair.log" 2>&1 \
  || fail "pairing: $(tail -n 2 "$DIR/pair.log")"
echo "paired: $(cat "$DIR/pair.log")"

# The JS SDK's tour, unchanged: every step runs; a step may be refused, a crash is a failure.
( cd "$HERE/sdk/js" && timeout 120 node examples/tour.js ) > "$DIR/tour.log" 2>&1 || fail "tour.js: $(tail -n 3 "$DIR/tour.log")"
grep -q '^body lease:' "$DIR/tour.log" || fail "tour.js did not get to the end"
echo "tour.js: $(grep -c ': ' "$DIR/tour.log") lines, $(grep -c refused "$DIR/tour.log") refused"

# The Python SDK, a round trip through the real door.
python3 - "$PORT" <<'PY' || fail "python round trip"
import asyncio, sys
from lampos import Lamp
async def main():
    async with await Lamp.connect() as lamp:
        body = (await lamp.get("body"))["body"]
        assert body["known"], body
        await lamp.light("green", 3, 1500)
        await lamp.gesture("nod")
        # The ring says what it drew a moment later.
        for _ in range(30):
            light = (await lamp.get("light"))["light"]
            if light["on"] and light["colour"] == "green":
                break
            await asyncio.sleep(0.1)
        assert light["on"] and light["colour"] == "green", light
asyncio.run(main())
PY
echo "python round trip: ok"

# Ending the lamp leaves nothing running.
kill "$SIM"; sleep 3; SIM=
left=$(ls /proc 2>/dev/null | grep -E '^[0-9]+$' | while read -r p; do cat /proc/$p/cmdline 2>/dev/null | tr '\0' ' ' | grep -q "$DIR/lamp" && echo "$p"; done)
[ -z "$left" ] || fail "parts left running after the lamp ended: $left"
echo "lamp-sim-check: ok"
