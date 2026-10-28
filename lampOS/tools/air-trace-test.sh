#!/usr/bin/env bash
# Tests for air-trace.sh with no lamp: a stand-in ssh runs the lamp's side here, and a stand-in lamp-bus records a
# made-up air/state (80, then 400 from 5.5 s in) and voice/state. The case folder it writes is checked, and
# that nothing is left of the lamp's temp folder or the recording. With LAMP_SKILLS=PATH (a built lamp-skills), the
# written case, its verdict filled in as none = true, is also run by `lamp-skills test`.
# Run: lampOS/tools/air-trace-test.sh   (about 45 s)
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/air-trace.sh
REAL_SKILLS=$(cd "$(dirname "$0")/../skills" && pwd)
T=$(mktemp -d)
trap 'pkill -f "$T/bin/lamp-bus" 2>/dev/null; rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}
mkdir -p "$T/bin" "$T/lamp-tmp" "$T/out"

# ssh: the lamp's side runs here; each command's words are kept.
cat >"$T/bin/ssh" <<'STAND_IN'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/ssh.args"
exec "$@"
STAND_IN
# lamp-bus record DIR TOPIC... --seconds S: a declaration a topic, then air/state each 0.5 s until killed.
cat >"$T/bin/lamp-bus" <<'STAND_IN'
#!/usr/bin/env python3
import base64, json, os, sys, time
args = sys.argv[1:]
assert args[0] == "record", args
dir, topics = args[1], args[2:args.index("--seconds")]
seconds = float(args[args.index("--seconds") + 1])
types = {"air/state": "lamp_msgs/AirState", "voice/state": "lamp_msgs/VoiceState"}
files = {t: open(os.path.join(dir, t.replace("/", "_") + ".jsonl"), "a") for t in topics}
def put(topic, line):
    files[topic].write(json.dumps(line, separators=(",", ":")) + "\n")
    files[topic].flush()
def msg(topic, seq, payload):
    at = time.time()
    put(topic, {"header": {"topic": topic, "kind": "latest", "enc": "json", "type": types[topic], "source": "lamp-air",
                           "boot": "0123456789abcdef", "seq": seq, "at_us": int(time.monotonic() * 1e6),
                           "at_unix_ms": int(at * 1000)},
                "payload_b64": base64.b64encode(json.dumps(payload).encode()).decode()})
for t in topics:
    put(t, {"topic": t, "declaration": {"type": types[t], "version": 1, "kind": "latest"}})
if "voice/state" in topics:
    msg("voice/state", 1, {"state": "resting"})
if os.environ.get("FAKE_SILENT"):
    time.sleep(seconds); sys.exit(0)
end = time.time() + seconds
seq = 0
while time.time() < end:
    seq += 1
    pm = 80.0 if seq < 12 else 400.0   # 400 from 5.5 s: after the start mark, before the end
    msg("air/state", seq, {"co2_ppm": 700, "feel": "fresh", "humidity": 50.0, "pm10": pm * 1.1, "pm1_0": pm * 0.8,
                           "pm2_5": pm, "pm4_0": pm * 1.05, "temperature": 38.0})
    time.sleep(0.5)
STAND_IN
chmod +x "$T/bin/ssh" "$T/bin/lamp-bus"

skills_copy() { # a fresh copy of smoky-air's tests to write into
  rm -rf "$T/skills" && mkdir -p "$T/skills" && cp -R "$REAL_SKILLS/smoky-air" "$T/skills/"
}
run() { # run STDIN ARGS...: air-trace.sh against the stand-ins
  local input=$1; shift
  rm -f "$T/ssh.args"
  printf "$input" | FAKE_DIR=$T LAMP_HOST=stand-in LAMP_SSH=$T/bin/ssh LAMP_BUS=$T/bin/lamp-bus \
    AIR_REMOTE_TMP=$T/lamp-tmp AIR_SKILLS_DIR=$T/skills AIR_PRE_S=3 AIR_POST_S=5 AIR_FIRST_S=4 AIR_LINGER_S=1 \
    PLAYED_OUT=$T/out "$SCRIPT" "$@" >"$T/stdout" 2>&1
}
lamp_tmp_empty() { [ -z "$(ls -A "$T/lamp-tmp")" ]; }
no_recorder() { sleep 0.3; ! pgrep -f "$T/bin/lamp-bus record" >/dev/null; }
only_lamp_bus_commands() { # every command on the "lamp" is the side's own sh, with one of its commands
  ! grep -vE '^sh -s -- (check|start|count|stop|fetch|clean) ' "$T/ssh.args"
}

# 1. --dry-run runs nothing and writes nothing.
skills_copy
run "" --dry-run toast-test
check "dry run exits 0" [ $? -eq 0 ]
check "dry run runs nothing on the lamp" [ ! -e "$T/ssh.args" ]
check "dry run writes no case" [ ! -e "$T/skills/smoky-air/tests/toast-test" ]
check "dry run names the recording" grep -q "record $T/lamp-tmp/air-trace.XXXXXX air/state voice/state" "$T/stdout"

# 2. A whole run: two Enters, the case written.
skills_copy
run "\n\n" toast-test "toaster 1 m away"
status=$?
C=$T/skills/smoky-air/tests/toast-test
check "a run exits 0 ($(tail -1 "$T/stdout"))" [ "$status" -eq 0 ]
check "air_state.jsonl written" [ -s "$C/air_state.jsonl" ]
check "voice_state.jsonl written" [ -s "$C/voice_state.jsonl" ]
check "the declaration comes first" [ "$(head -c 40 "$C/air_state.jsonl")" = '{"topic":"air/state","declaration":{"typ' ]
check "every line is a whole JSON line" python3 -I -c '
import json, sys
for p in sys.argv[1:]:
    for text in open(p):
        json.loads(text)' "$C/air_state.jsonl" "$C/voice_state.jsonl"
check "about 6 s of readings or more" [ "$(grep -c '"header"' "$C/air_state.jsonl")" -ge 10 ]
check "nothing else in the case folder" [ "$(ls "$C" | tr '\n' ' ')" = "air_state.jsonl expect.toml voice_state.jsonl " ]
check "clock set" grep -qE '^clock = "[0-2][0-9]:[0-5][0-9]"$' "$C/expect.toml"
check "the marks are in" grep -qE '^# Marks, in seconds from the first message: the event started at [0-9]+ s and ended at [0-9]+ s;' "$C/expect.toml"
check "the note is in" grep -q "toaster 1 m away" "$C/expect.toml"
check "PM2.5 before and after" grep -q '^# PM2.5 (ug/m3): 80 lowest and 80 highest before the start; 400 highest from the start on; 400 at the end.$' "$C/expect.toml"
check "the verdict is left open" bash -c "! grep -qE '^(none|\[\[expect\]\])' '$C/expect.toml'"
check "the verdict's say is offered" grep -q '^# args = { text = "The air reads like smoke.' "$C/expect.toml"
check "the lamp's temp folder is removed" lamp_tmp_empty
check "the recording is stopped" no_recorder
check "only the side's commands ran on the lamp" only_lamp_bus_commands
check "a log written" [ -n "$(ls "$T/out/air-trace/" 2>/dev/null)" ]
if [ -n "${LAMP_SKILLS:-}" ]; then
  check "lamp-skills fails it while the verdict is open" bash -c "! '$LAMP_SKILLS' test '$T/skills/smoky-air' >/dev/null 2>&1"
  printf 'none = true\n' >>"$C/expect.toml"
  "$LAMP_SKILLS" test "$T/skills/smoky-air" >"$T/skills.out" 2>&1
  check "lamp-skills reads the case ($(tail -1 "$T/skills.out"))" grep -q "cases pass" "$T/skills.out"
fi

# 3. A folder with a recording is kept.
run "\n\n" toast-test
check "a recorded case is kept (exit 3)" [ $? -eq 3 ]
check "and nothing ran on the lamp" [ ! -e "$T/ssh.args" ]

# 4. A placeholder is replaced, its event lines carried over.
skills_copy
run "\n\n" kettle-steam
check "a placeholder is replaced" [ -s "$T/skills/smoky-air/tests/kettle-steam/air_state.jsonl" ]
check "its event line carried" grep -q '^# The event: A kettle boiled' "$T/skills/smoky-air/tests/kettle-steam/expect.toml"
check "and no longer a placeholder" bash -c "! grep -q PLACEHOLDER '$T/skills/smoky-air/tests/kettle-steam/expect.toml'"

# 5. No sensor: nothing written, the lamp left clean.
skills_copy
FAKE_SILENT=1 run "\n\n" match-test
check "no air/state reading exits 4" [ $? -eq 4 ]
check "and writes no case" [ ! -e "$T/skills/smoky-air/tests/match-test" ]
check "and removes the lamp's temp folder" lamp_tmp_empty
check "and stops the recording" no_recorder

# 6. No Enter (the input ends): nothing written, the lamp left clean.
run "" match-test
check "no Enter exits 5" [ $? -eq 5 ]
check "and writes no case" [ ! -e "$T/skills/smoky-air/tests/match-test" ]
check "and removes the lamp's temp folder" lamp_tmp_empty
check "and stops the recording" no_recorder

# 7. Bad words.
run "" "Bad Name"
check "an EVENT that is not a folder name exits 2" [ $? -eq 2 ]
run "" --skill stuffy-air toast-test
check "another skill exits 2" [ $? -eq 2 ]

sleep 1.5   # the lamp side's linger
check "nothing left in the lamp's temp" lamp_tmp_empty
if [ "$fails" -eq 0 ]; then echo "all passed"; else echo "$fails failed"; exit 1; fi
