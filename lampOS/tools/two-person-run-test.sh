#!/usr/bin/env bash
# Tests for two-person-run.sh with no lamp: "the lamp" is a folder here,
# reached with `sh -s`, a stand-in for systemctl, and a `say` (bash and
# jq) that plays the voice: it keeps each person's code as whose the
# stand-in names, answers with it into the thread, and with LEAKY=1 answers
# with the last code kept by anyone, as a voice that mixed two people's
# things would.
# Run: lampOS/tools/two-person-run-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/two-person-run.sh
T=$(mktemp -d)
trap 'pkill -f "$T/run/stand-in.sh" 2>/dev/null; rm -rf "$T"' EXIT
fails=0
check() { # check WHAT CONDITION...
  local what=$1; shift
  if "$@"; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi
}

mkdir -p "$T/bin"
cat >"$T/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE/systemctl.log"
EOF
# systemd-run: the stand-in in the background, here.
cat >"$T/bin/systemd-run" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE/systemctl.log"
shift 2
nohup "$@" </dev/null >/dev/null 2>&1 &
EOF
cat >"$T/bin/say" <<'EOF'
#!/usr/bin/env bash
text=$(tr '[:upper:]' '[:lower:]' <<<"${*: -1}")
who=$(cat "$RUN_DIR/who")
whose=
[[ -n $who ]] && whose=p-$(printf '%s' "$who" | tr '[:upper:]' '[:lower:]' | shasum -a 256 | cut -c1-16)
codes=$FAKE/codes.json
[[ -f $codes ]] || echo '{}' >"$codes"
key=${whose:-room}
case $text in
  *remember*)
    if [[ $text == *"four one seven"* ]]; then code="four one seven"; else code="two five eight zero"; fi
    jq --arg k "$key" --arg c "$code" '.[$k] = $c | .last = $c' "$codes" >"$codes.new"
    answer="Got it, I'll keep that." ;;
  *forget*)
    jq --arg k "$key" 'del(.[$k])' "$codes" >"$codes.new"
    answer="Done, it's gone." ;;
  *"just ask"*)
    cp "$codes" "$codes.new"
    answer="You asked me about your test code." ;;
  *)
    cp "$codes" "$codes.new"
    if [[ -n ${LEAKY:-} ]]; then code=$(jq -r '.last // empty' "$codes"); else code=$(jq -r --arg k "$key" '.[$k] // empty' "$codes"); fi
    if [[ -n $code ]]; then answer="Your test code is $code."; else answer="I don't know your test code."; fi ;;
esac
mv "$codes.new" "$codes"
[[ -f $THREAD ]] || echo '[]' >"$THREAD"
# As the voice's thread, at most eight are kept here, so a full thread
# keeps its length as answers come (Fifth's run on #2165).
jq --arg p "$text" --arg l "$answer" --arg w "$whose" \
  '(. + [{at_unix_ms: ((map(.at_unix_ms) | max // 0) + 1), person: $p, lamp: $l}
         + (if $w == "" then {} else {whose: $w} end)])[-8:]' \
  "$THREAD" >"$THREAD.new" && mv "$THREAD.new" "$THREAD"
EOF
chmod +x "$T/bin/systemctl" "$T/bin/systemd-run" "$T/bin/say"

run() { # run [ENV...]: the whole run against the fake lamp, its output in $T/out
  rm -rf "$T/run" "$T/units" "$T/codes.json" "$T/systemctl.log"
  # A thread already full of eight earlier exchanges.
  jq -n '[range(8) | {at_unix_ms: ., person: "earlier", lamp: "earlier"}]' >"$T/thread.json"
  # As on lamp-52e6: the switch in the worker's environment, the sockets
  # its arguments.
  mkdir -p "$T/proc"
  printf 'LAMP_SECOND_DEEPSEEK_BASE_URL=x\0LAMP_SECOND_SCOPED_CONTEXT=on\0' >"$T/proc/environ"
  printf 'lamp-voice\0worker\0second\0--memory-socket\0/m.sock\0--who-socket\0%s\0' "$T/run/who.sock" >"$T/proc/cmdline"
  env PATH="$T/bin:$PATH" FAKE="$T" LAMP_SH="sh -s" RUN_DIR="$T/run" THREAD="$T/thread.json" \
    UNIT_DIR="$T/units" WORKER_PROC="$T/proc" SLEEP=sleep WAIT_S=3 PAUSE_S=0 SAY="$T/bin/say" "$@" \
    bash "$SCRIPT" >"$T/out" 2>&1
}

run
status=$?
sed 's/^/  | /' "$T/out"
check "a voice that keeps each person's own passes every line, in a full thread" \
  bash -c "[[ $status == 0 ]] && grep -q '^12 passed, 0 failed' '$T/out'"
check "B's first line is kept as B's and answered with B's code" \
  bash -c "grep -q '^step 5 B: kept as B (expected B), codes in the answer: 2580: PASS' '$T/out'"
check "with nobody named, the line is the room's and holds no code" \
  bash -c "grep -q '^step 7 A: kept as room (expected room), codes in the answer: -: PASS' '$T/out'"
check "nothing anyone said is printed" bash -c "! grep -qiE 'remember|your test code is|four one seven' '$T/out'"
check "put back: the drop-in and the stand-in's folder gone, lamp-voice restarted" \
  bash -c "[[ ! -e '$T/units/lamp-voice.service.d/zz-two-person.conf' && ! -d '$T/run' ]] &&
    [[ \$(grep -c '^restart lamp-voice' '$T/systemctl.log') == 2 ]]"

run LEAKY=1
status=$?
check "a voice that gives one person's code to the other fails, its lines named" \
  bash -c "[[ $status != 0 ]] && grep -q '^step 5 A: kept as A (expected A), codes in the answer: 2580: FAIL' '$T/out' &&
    grep -q '^step 6 A: kept as A (expected A), codes in the answer: 2580: FAIL' '$T/out'"

# Another drop-in's eyes socket in the worker: nothing measured.
rm -rf "$T/run" "$T/units" "$T/systemctl.log"
mkdir -p "$T/proc.eyes"
printf 'LAMP_SECOND_SCOPED_CONTEXT=on\0' >"$T/proc.eyes/environ"
printf 'lamp-voice\0worker\0second\0--memory-socket\0/m.sock\0--who-socket\0/run/lampos-eyes/who.sock\0' >"$T/proc.eyes/cmdline"
env PATH="$T/bin:$PATH" FAKE="$T" LAMP_SH="sh -s" RUN_DIR="$T/run" THREAD="$T/thread.json" \
  UNIT_DIR="$T/units" WORKER_PROC="$T/proc.eyes" SLEEP=true SAY=false \
  bash "$SCRIPT" >"$T/out" 2>&1
status=$?
check "a worker reading the real eyes' socket measures nothing, and all is put back" \
  bash -c "[[ $status == 4 && ! -d '$T/run' ]] && grep -q 'is not the stand-in' '$T/out'"

# The switch not in the worker: nothing is said, and it is put back.
rm -rf "$T/run" "$T/units" "$T/systemctl.log"
mkdir -p "$T/proc.off"
printf 'PATH=/usr/bin\0' >"$T/proc.off/environ"
: >"$T/proc.off/cmdline"
env PATH="$T/bin:$PATH" FAKE="$T" LAMP_SH="sh -s" RUN_DIR="$T/run" THREAD="$T/thread.json" \
  UNIT_DIR="$T/units" WORKER_PROC="$T/proc.off" SLEEP=true SAY=false \
  bash "$SCRIPT" >"$T/out" 2>&1
status=$?
check "without the switch in the worker, nothing is said and all is put back" \
  bash -c "[[ $status == 4 && ! -d '$T/run' && ! -e '$T/units/lamp-voice.service.d/zz-two-person.conf' ]] &&
    grep -q 'without LAMP_SECOND_SCOPED_CONTEXT=on' '$T/out'"

echo "$fails failed"
[[ $fails == 0 ]]
