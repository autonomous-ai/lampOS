#!/usr/bin/env bash
# Tests for mic2-voice-table.sh with no lamp and no model: ssh runs the
# lamp's commands here, with a stand-in lamp-bus that records a fixture, and
# a stand-in same_voice_pairs that prints what it was asked.
# Run: lampOS/tools/mic2-voice-table-test.sh
set -uo pipefail

TABLE=$(cd "$(dirname "$0")" && pwd)/mic2-voice-table.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}

mkdir -p "$T/bin" "$T/lamp/bin" "$T/tmp"
cat >"$T/bin/ssh" <<EOF
#!/usr/bin/env bash
echo "ssh \$*" >>"$T/ssh-args"
while [[ \$1 == -o || \$1 == -O ]]; do shift 2; done
shift
PATH=$T/lamp/bin:\$PATH bash -c "\$1"
EOF
cat >"$T/bin/scp" <<'EOF'
#!/usr/bin/env bash
args=("$@")
n=${#args[@]}
from=${args[n-2]}
cp "${from#*:}" "${args[n-1]}"
EOF
# The lamp's GNU date, for a Mac's BSD one here: milliseconds by perl.
cat >"$T/lamp/bin/date" <<'EOF'
#!/usr/bin/env bash
perl -MTime::HiRes=time -e 'printf "%d\n", time() * 1000'
EOF
# The recorder: two whole lines, then one cut short, then it waits.
cat >"$T/lamp/bin/lamp-bus" <<EOF
#!/usr/bin/env bash
[[ \$1 == record && \$3 == mic2/audio ]] || exit 9
echo "\$*" >"$T/lamp-bus-args"
printf '{"topic":"mic2/audio"}\n{"header":{"at_unix_ms":1},"payload_b64":""}\n{"header":{"at' >"\$2/mic2_audio.jsonl"
exec sleep 1007
EOF
cat >"$T/bin/same_voice_pairs" <<EOF
#!/usr/bin/env bash
echo "pairs \$*" >>"$T/pairs"
wc -l <"\$3" >>"$T/pairs"
EOF
chmod +x "$T/bin/"* "$T/lamp/bin/"*
echo model >"$T/model.onnx"
echo '{"rows":[]}' >"$T/round.json"
# Another run's recording on the lamp, its recorder still going: no run of
# this one may touch it (reviewer-two's P2 on #3154).
mkdir -p "$T/stage/mic2-voice-table.other"
echo keep >"$T/stage/mic2-voice-table.other/mic2_audio.jsonl"
sleep 1011 &
other=$!
echo "$other" >"$T/stage/mic2-voice-table.other/pid"
# The lamp's own removal sleeps the hard end (60 s) plus 600: stop only the
# timers this test started, the ones naming its own folder, and their sleeps
# (reviewer-two's P2 on #3154), never another run's.
stop_timers() {
  local timer
  for timer in $(pgrep -f -- "rm -rf \"$T/stage/mic2-voice-table"); do
    pkill -P "$timer" 2>/dev/null
    kill "$timer" 2>/dev/null
  done
}
trap 'stop_timers; kill "$other" 2>/dev/null; rm -rf "$T"' EXIT
left() { ls -d "$T/stage/"mic2-voice-table.* 2>/dev/null | grep -vc '\.other$'; }
untouched() { [[ -s "$T/stage/mic2-voice-table.other/mic2_audio.jsonl" ]] && kill -0 "$other" 2>/dev/null; }

run() {
  TMPDIR=$T/tmp PATH=$T/bin:$PATH SAME_VOICE_PAIRS=$T/bin/same_voice_pairs \
    bash "$TABLE" --ssh lamp --model "$T/model.onnx" --plays "$T/round.json" \
    --stage "$T/stage" --seconds 60 -- "$@"
}

out=$(run bash -c "echo played >>'$T/played'") ; code=$?
check "a run ends 0" "[[ $code -eq 0 ]]"
check "the round was played once" "[[ \$(cat '$T/played') == played ]]"
check "the recorder took mic2/audio with the hard end" "grep -qE 'record $T/stage/mic2-voice-table\.[A-Za-z0-9]+ mic2/audio --seconds 60 --max-mb 512' '$T/lamp-bus-args'"
check "scored at 300 and 600 ms" "[[ \$(grep -c '^pairs' '$T/pairs') -eq 2 ]] && grep -q 'end-silence-ms 300' '$T/pairs' && grep -q 'end-silence-ms 600' '$T/pairs'"
check "with the bus recording, the round and an offset" "grep -qE -- '--bus .*mic2_audio.jsonl --plays $T/round.json --offset-ms -?[0-9]' '$T/pairs'"
check "the cut-short last line is left out" "[[ \$(grep -v '^pairs' '$T/pairs' | head -1 | tr -d ' ') == 2 ]]"
check "it says so" "grep -q 'cut short' <<<\"\$out\""
check "the model's sha256 is printed" "grep -q '^model sha256 [0-9a-f]\{64\}' <<<\"\$out\""
check "this run's folder is gone from the lamp" "[[ \$(left) -eq 0 ]]"
check "another run's folder and recorder are untouched" "untouched"
check "the recorder is stopped" "! pgrep -f '^sleep 1007$' >/dev/null"
check "no copy is left here" "[[ -z \$(ls -A '$T/tmp') ]]"
check "every ssh call shares one connection" "! grep -v -- 'ControlPath=' '$T/ssh-args'"
check "the lamp's own removal is set, after the hard end" "grep -q 'sleep 660; rm -rf' '$T/ssh-args'"
# Another run's timer, the same command in another folder: never stopped.
sh -c 'sleep 660; rm -rf "/nonexistent/mic2-voice-table.other-run"' &
foreign=$!
stop_timers
check "stopping this test's timers stops them" "[[ -z \$(pgrep -f -- \"rm -rf \\\"$T/stage/mic2-voice-table\") ]]"
check "and leaves another run's timer going" "kill -0 $foreign 2>/dev/null"
pkill -P "$foreign" 2>/dev/null; kill "$foreign" 2>/dev/null
check "which is closed at the end" "grep -q -- '-O exit' '$T/ssh-args'"
check "and opened before the clock is read" "[[ \$(grep -v -- '-O exit' '$T/ssh-args' | head -1) == *' true' ]]"

# A failed round still stops the recorder and removes everything.
rm -f "$T/pairs"
run false >/dev/null; code=$?
check "a failed round is still scored" "[[ $code -eq 0 && \$(grep -c '^pairs' '$T/pairs') -eq 2 ]]"
check "and its folder is gone" "[[ \$(left) -eq 0 ]] && untouched"

# Nothing recorded: it stops before playing.
cat >"$T/lamp/bin/lamp-bus" <<'EOF'
#!/usr/bin/env bash
echo "no permission on mic2/audio" >&2
exit 1
EOF
rm -f "$T/played"
run bash -c "echo played >>'$T/played'" >/dev/null 2>"$T/err"; code=$?
check "nothing recorded ends 1" "[[ $code -eq 1 ]]"
check "before playing" "[[ ! -e '$T/played' ]]"
check "with the recorder's words" "grep -q 'no permission' '$T/err'"
check "and its folder is gone" "[[ \$(left) -eq 0 ]] && untouched"

# The lamp's clock unreadable: it stops before it has a folder, and removes
# nothing there.
cat >"$T/lamp/bin/date" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "$T/lamp/bin/date"
: >"$T/ssh-args"
run true >/dev/null 2>&1; code=$?
check "an unreadable clock ends 1" "[[ $code -eq 1 ]]"
check "with no folder made" "! grep -q mktemp '$T/ssh-args'"
check "and another run's folder and recorder untouched" "untouched && [[ \$(left) -eq 0 ]]"
check "no kill or removal was sent" "! grep -qE 'kill|rm -rf' '$T/ssh-args'"

check "usage without a command" "! bash '$TABLE' --ssh lamp --model '$T/model.onnx' --plays x 2>/dev/null"

echo "failures: $failures"
[[ $failures -eq 0 ]]
