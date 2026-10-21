#!/usr/bin/env bash
# Tests for socket-ab.sh with no lamp: lamp_ssh runs the lamp's commands here,
# with stand-ins for sudo, systemctl, journalctl and the played day, and a
# small fixture of session events and journal lines (timing only).
# Run: lampOS/tools/socket-ab-test.sh
set -uo pipefail

AB=$(cd "$(dirname "$0")" && pwd)/socket-ab.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}

mkdir -p "$T/bin" "$T/lamp/bin" "$T/fixture"
cat >"$T/bin/lamp_ssh" <<EOF
#!/usr/bin/env bash
PATH=$T/lamp/bin:\$PATH bash -c "\$1"
EOF
cat >"$T/bin/day" <<EOF
#!/usr/bin/env bash
echo "day \$1" >>"$T/log"
echo "the day's words, never in the summary"
[[ -n \${FAKE_DAY_WAIT:-} ]] && sleep "\$FAKE_DAY_WAIT"
exit 0
EOF
cat >"$T/bin/quiet" <<EOF
#!/usr/bin/env bash
echo "quiet \$*" >>"$T/log"
[[ -z \${FAKE_BUSY:-} ]]
EOF
# The lamp's.
cat >"$T/lamp/bin/sudo" <<'EOF'
#!/usr/bin/env bash
while [[ $1 == -* ]]; do shift; done
"$@"
EOF
# A restart starts a new session (the old ones moved aside, so the newest is
# plain) with the arm's events, and a worker whose environment is the
# drop-in's, unless FAKE_NO_SWITCH leaves it out.
cat >"$T/lamp/bin/systemctl" <<EOF
#!/usr/bin/env bash
echo "systemctl \$*" >>"$T/log"
D=$T/units/lamp-voice.service.d/zz-socket-ab.conf
case \$1 in
  is-active) echo active ;;
  restart)
    arm=none
    grep -q '^Environment=LAMP_SECOND_SPEECH_SOCKET=on$' "\$D" 2>/dev/null && arm=on
    grep -q '^Environment=LAMP_SECOND_SPEECH_SOCKET=off$' "\$D" 2>/dev/null && arm=off
    echo "\$arm" >"$T/arm"
    cat "\$D" >>"$T/dropins" 2>/dev/null || echo "(none)" >>"$T/dropins"
    mkdir -p "$T/old"; mv "$T/sessions"/* "$T/old/" 2>/dev/null
    n=\$(ls "$T/old" | wc -l | tr -d ' ')
    mkdir -p "$T/sessions/s\$n/metadata"
    [[ -f $T/fixture/events-\$arm.jsonl ]] && cp "$T/fixture/events-\$arm.jsonl" "$T/sessions/s\$n/metadata/events-000001.jsonl"
    if [[ -n \${FAKE_NO_SWITCH:-} ]]; then printf 'LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo\0' >"$T/worker/environ"
    else sed -n 's/^Environment=//p' "\$D" 2>/dev/null | tr '\n' '\0' >"$T/worker/environ"
         printf 'LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo\0' >>"$T/worker/environ"; fi ;;
esac
exit 0
EOF
cat >"$T/lamp/bin/journalctl" <<EOF
#!/usr/bin/env bash
cat "$T/fixture/journal-\$(cat "$T/arm").txt" 2>/dev/null
EOF
chmod +x "$T"/bin/* "$T"/lamp/bin/*

# The fixture. Off: three turns, the first sound 1600, 1700 and 1500 ms after
# the end of speech; a lamp's own line (turn 4, no end of speech) and a
# transcript left out. On: 1500, 1400 and 1550.
events() { # events END_MS... as turns 1..n, each end at n*10 s
  local n=0 ms
  for ms in "$@"; do
    n=$((n + 1))
    echo "{\"kind\":\"local_endpoint\",\"at_us\":$((n * 10000000)),\"turn\":$n}"
    echo "{\"kind\":\"transcript\",\"at_us\":$((n * 10000000 + 5)),\"turn\":$n,\"text\":\"secret words\"}"
    echo "{\"kind\":\"speaker_first_write\",\"at_us\":$((n * 10000000 + ms * 1000)),\"turn\":$n}"
    echo "{\"kind\":\"speaker_first_write\",\"at_us\":$((n * 10000000 + ms * 1000 + 900000)),\"turn\":$n}"
  done
  echo "{\"kind\":\"speaker_first_write\",\"at_us\":99000000,\"turn\":$((n + 1))}"
}
events 1600 1700 1500 >"$T/fixture/events-off.jsonl"
events 1500 1400 1550 >"$T/fixture/events-on.jsonl"
piece() { # piece REQUEST INDEX socket|new|reused FIRST_MS
  local on
  case $3 in
    socket) on="the speech socket opened 300 ms before" ;;
    *) on="a $3 connection" ;;
  esac
  echo "lamp-voice second: request $1 ElevenLabs piece $2: 40 chars, sent 900 ms after the words were heard on $on; its first audio byte $4 ms after it went"
}
{
  echo "lamp-voice second: request 1 heard: secret words"
  piece 1 1 new 480; piece 1 2 reused 500; piece 2 1 reused 300; piece 3 1 new 700
  echo "lamp-voice second: request 3 ElevenLabs: 1 of 1 pieces began, 0 on a reused connection, 40 chars; the first piece's first audio byte 700 ms after it went, the slowest 700 ms"
} >"$T/fixture/journal-off.txt"
{
  echo "lamp-voice second: a speech socket is opened as each answer begins, for its first piece (LAMP_SECOND_SPEECH_SOCKET)"
  echo "lamp-voice second: request 1: the speech socket is open, 210 ms after it was asked"
  piece 1 1 socket 116; piece 1 2 reused 450; piece 2 1 socket 130; piece 3 1 socket 120; piece 3 2 reused 520
  echo "lamp-voice second: the speech socket was closed: 4429 \"rate limited\""
  echo "lamp-voice second: request 4: the speech socket failed before its first audio (service answered HTTP 429); the piece goes over HTTP"
  piece 4 1 new 610
} >"$T/fixture/journal-on.txt"

run() {
  rm -rf "$T/sessions" "$T/old" "$T/units" "$T/out-dir" "$T/worker"
  mkdir -p "$T/sessions/s-before/metadata" "$T/units" "$T/worker"
  : >"$T/log"; : >"$T/dropins"; echo none >"$T/arm"
  printf 'LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo\0' >"$T/worker/environ"
  LAMP_HOST=lamp LAMP_SSH=$T/bin/lamp_ssh DAY=$T/bin/day QUIET=$T/bin/quiet PLAYED_OUT=$T/out-dir \
    UNIT_DIR=$T/units SESSIONS=$T/sessions WORKER_PROC=$T/worker SETTLE=3 TAIL=0 \
    bash "$AB" "$@" >"$T/out" 2>&1
}
S=$T/out-dir/scenarios/socket-ab-t1.txt

run t1; status=$?
check "both runs go, off then on, each with the day lines, and it ends" '[[ $status == 0 ]] && grep -q "done; the summary" "$T/out" && [[ $(grep "^day " "$T/log" | paste -sd, -) == "day socket-ab-t1-off,day socket-ab-t1-on" ]]'
check "quiet is asked before anything is changed" '[[ $(grep -m1 -n "^quiet 45$" "$T/log" | cut -d: -f1) -lt $(grep -m1 -n "daemon-reload" "$T/log" | cut -d: -f1) ]]'
check "off sets the switch off, on sets it on with Flash, each by a drop-in, then put back" '[[ $(grep -c "^Environment=LAMP_SECOND_SPEECH_SOCKET=off$" "$T/dropins") == 1 ]] && [[ $(grep -c "^Environment=LAMP_SECOND_SPEECH_SOCKET=on$" "$T/dropins") == 1 ]] && grep -qx "Environment=LAMP_SECOND_SPEECH_SOCKET_MODEL=eleven_flash_v2_5" "$T/dropins" && [[ $(tail -1 "$T/dropins") == "(none)" ]] && [[ ! -e $T/units/lamp-voice.service.d/zz-socket-ab.conf ]]'
check "off: pieces, all over HTTP, their median and p90" 'grep -qx "off pieces=4 (socket 0, http 4, first pieces 3)" "$S" && grep -qx "off http first audio=median 480, p90 700, n 4" "$S" && grep -qx "off socket first audio=none" "$S"'
check "on: socket and HTTP pieces apart" 'grep -qx "on pieces=6 (socket 3, http 3, first pieces 4)" "$S" && grep -qx "on socket first audio=median 120, p90 130, n 3" "$S" && grep -qx "on http first audio=median 520, p90 610, n 3" "$S"'
check "the answer's first sound after end of speech, the earliest write, own lines left out" 'grep -qx "off first sound after end of speech=median 1600, p90 1700, n 3" "$S" && grep -qx "on first sound after end of speech=median 1500, p90 1550, n 3" "$S"'
check "refused, 429s, 44xx closes and fallbacks counted" 'grep -q "^on refused=0 socket_429=1 http_429=0 closes_44xx=1 fallbacks_to_http=1 (failed 1, not opened 0) switch_on_line=1$" "$S" && grep -q "^off refused=0 socket_429=0 .* switch_on_line=0$" "$S"'
check "the bar" 'grep -qx "bar first sound: on 1500 <= off 1600 ms: yes" "$S" && grep -qx "bar piece first audio: on'"'"'s socket median 120 < off'"'"'s http median 480 ms: yes" "$S" && grep -qx "bar refused: 0: yes" "$S"'
check "timing only: no words in the summary or the times" '! grep -qi "secret\|words, never" "$S" "$T"/out-dir/scenarios/*.times "$T/out"'
check "the day's own output kept apart" 'grep -q "never in the summary" "$T/out-dir/scenarios/socket-ab-t1-on.day.log"'

# A refused model fails the bar.
echo 'lamp-voice second: request 2: the speech socket refused eleven_v4_turbo (service answered HTTP 400): no socket is opened ahead for it again until the voice starts again' >>"$T/fixture/journal-on.txt"
run t1 --model eleven_v4_turbo; status=$?
check "--model reaches the drop-in, and a refusal fails the bar" '[[ $status == 0 ]] && grep -qx "Environment=LAMP_SECOND_SPEECH_SOCKET_MODEL=eleven_v4_turbo" "$T/dropins" && grep -qx "bar refused: 1: no" "$S"'

run t2 --arms on; status=$?
check "--arms on runs only on, no bar, and puts back" '[[ $status == 0 ]] && [[ $(grep -c "^day " "$T/log") == 1 ]] && ! grep -q "^bar" "$T/out-dir/scenarios/socket-ab-t2.txt" && [[ ! -e $T/units/lamp-voice.service.d/zz-socket-ab.conf ]]'
run t3 --arms both; status=$?
check "--arms takes only off and on" '[[ $status == 2 ]] && ! grep -q "^systemctl" "$T/log"'
run 'bad label'; status=$?
check "a LABEL with a space is refused" '[[ $status == 2 ]] && [[ ! -s $T/log ]]'

FAKE_BUSY=1 run t4; status=$?
check "a lamp not quiet: nothing changed, nothing played" '[[ $status == 1 ]] && ! grep -q "systemctl restart" "$T/log" && ! grep -q "^day" "$T/log"'

FAKE_NO_SWITCH=1 run t5 --arms on; status=$?
check "a worker without the switch: nothing measured, put back" '[[ $status == 1 ]] && grep -q "runs without LAMP_SECOND_SPEECH_SOCKET=on" "$T/out" && ! grep -q "^day" "$T/log" && [[ ! -e $T/units/lamp-voice.service.d/zz-socket-ab.conf ]] && [[ $(tail -1 "$T/dropins") == "(none)" ]]'

# Ctrl-C during the day: put back once, no second run.
rm -rf "$T/sessions" "$T/old" "$T/units" "$T/worker" && mkdir -p "$T/sessions/s-before/metadata" "$T/units" "$T/worker"
: >"$T/log"; : >"$T/dropins"
set -m
FAKE_DAY_WAIT=30 LAMP_HOST=lamp LAMP_SSH=$T/bin/lamp_ssh DAY=$T/bin/day QUIET=$T/bin/quiet PLAYED_OUT=$T/out-dir \
  UNIT_DIR=$T/units SESSIONS=$T/sessions WORKER_PROC=$T/worker SETTLE=3 TAIL=0 \
  bash "$AB" t6 >"$T/out" 2>&1 &
ab=$!
set +m
for _ in $(seq 100); do grep -q "^day" "$T/log" && break; sleep 0.1; done
kill -INT -- -"$ab"; wait "$ab"; status=$?
check "Ctrl-C: the drop-in goes, lamp-voice restarts on its own, no second run" '[[ $status == 130 ]] && [[ ! -e $T/units/lamp-voice.service.d/zz-socket-ab.conf ]] && [[ $(tail -1 "$T/dropins") == "(none)" ]] && [[ $(grep -c "^day" "$T/log") == 1 ]] && [[ $(grep -c "putting lamp-voice back" "$T/out") == 1 ]]'

echo "$failures failed"
[[ $failures == 0 ]]
