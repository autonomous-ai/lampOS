#!/usr/bin/env bash
# Tests for stop-lines.sh with no lamp and no sound: a stand-in lamp_ssh, quiet_gate.sh, say, afplay and osascript
# answer from the FAKE_ settings; the stand-in journal holds what the voice would have said in each episode.
# Run: lampOS/tools/played/stop-lines-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/stop-lines.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

# The journal per episode: the last line played picks it (written by the stand-in afplay).
cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\nExecStart={ path=/opt/lampOS/voice/current/lamp-voice ; argv[]=/opt/lampOS/voice/current/lamp-voice serve --turn-taking duplex ; }\n' "$FAKE_ENV" ;;
  *sha256sum*) echo "0123456789abcdef0123  /opt/lampOS/voice/current/lamp-voice" ;;
  *turn_finished*)
    # The answer's wait sound retired at 3 s, its answer at 4.5 s; the turn finished at 4.6 s, 300 ms before now.
    echo '{"kind":"speaker_first_write","at_us":2000000,"owner":{"turn":7}}'
    [[ $FAKE_MODE == unfinished ]] || echo '{"kind":"turn_finished","at_us":4600000,"turn":7,"outcome":"audio_written"}'
    echo "now 4900000" ;;
  *monotonic_ns*) echo 1000000 ;;
  *journalctl*)
    last=$(tail -n 1 "$FAKE_DIR/played")
    j() { echo "2026-11-02T10:00:00+0700 lamp lamp-voice[1]: lamp-voice second: $1"; }
    j "request 7 heard 11 words, the last 200 ms after the line ended"
    case "$FAKE_MODE:$last" in
      unfinished:*poem*) ;;
      quiet:*Funk*) j "request 8 heard 0 words, the last 200 ms after the line ended" ;;
      *:*Funk*) [[ $FAKE_MODE == normal ]] && { j "request 8 heard 0 words, the last 200 ms after the line ended"; j "request 8: a cut-in with no words ended the lamp's answer: answered \"Did someone say something?\", the model not asked (LAMP_CUT_IN_WORDLESS_ASKS)"; j "request 8 first answer audio 12 ms after the words were heard: its fixed line, rendered ahead (LAMP_CUT_IN_WORDLESS_ASKS)"; } ;;
      silent:*) j "request 8 heard 1 words, the last 200 ms after the line ended"; j "request 8: a stop said over the lamp's answer: answered \"Okay, I'll stop there.\", the model not asked (LAMP_SPOKEN_STOP_ENDS)" ;;
      model:*) j "request 8 heard 1 words, the last 200 ms after the line ended"; j "request 8 chat round 1 first event in 800 ms" ;;
      *) if tail -n 2 "$FAKE_DIR/played" | grep -q poem; then
           j "request 8: a stop said just after the lamp's answer: answered \"Okay, I'll stop there.\", the model not asked (LAMP_SPOKEN_STOP_ENDS)"
         else
           j "request 8: a stop said over the lamp's answer: answered \"Okay, I'll stop there.\", the model not asked (LAMP_SPOKEN_STOP_ENDS)"
         fi
         j "request 8 first answer audio 9 ms after the words were heard: its fixed line, rendered ahead (LAMP_SPOKEN_STOP_ENDS)" ;;
    esac ;;
  *"rm -f "*) echo removed >>"$FAKE_DIR/put-back" ;;
  *"tee "*) echo on >"$FAKE_DIR/dropin" ;;
  *) exit 1 ;;
esac
EOF
# say writes its words into the file it makes, afplay records what it played.
printf '#!/usr/bin/env bash\nwhile [[ $# -gt 1 ]]; do [[ $1 == -o ]] && out=$2; shift; done; printf "%%s" "$1" >"$out"\n' >"$T/say"
printf '#!/usr/bin/env bash\n{ if [[ -s $1 ]]; then cat "$1"; else printf "%%s" "$1"; fi; echo; } >>"$FAKE_DIR/played"\n' >"$T/afplay"
printf '#!/usr/bin/env bash\necho "$*" >>"$FAKE_DIR/volume"; [[ $* == *"get volume"* ]] && echo 30; exit 0\n' >"$T/osascript"
printf '#!/usr/bin/env bash\nexit 0\n' >"$T/quiet_gate.sh"
touch "$T/Funk.aiff"
chmod +x "$T/lamp_ssh" "$T/say" "$T/afplay" "$T/osascript" "$T/quiet_gate.sh"

fail=0
run() { # run NAME EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings, one round
  local name=$1 want=$2 got out
  shift 2
  rm -rf "$T/d" "$T/out"; mkdir -p "$T/d"; : >"$T/d/played"
  out=$(FAKE_DIR=$T/d LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh STOP_LINES_QUIET_GATE=$T/quiet_gate.sh STOP_LINES_SAY=$T/say \
    STOP_LINES_PLAY=$T/afplay STOP_LINES_VOLUME=$T/osascript STOP_LINES_SOUND=$T/Funk.aiff PLAYED_OUT=$T/out \
    LAMP_PLAY_AT_LUNCH=yes PATH="$T:$PATH" "$SCRIPT" "$name" 1 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  grep -qF 'set volume output volume 45' "$T/d/volume" || { echo "FAIL $name: the volume was not set to 45"; fail=1; }
  [[ $(tail -n 1 "$T/d/volume") == "-e set volume output volume 30" ]] || { echo "FAIL $name: the volume was not put back"; fail=1; }
}

export FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_SPOKEN_STOP_ENDS=on" FAKE_MODE=normal
sleep() { echo "$*" >>"$FAKE_DIR/slept"; }
export -f sleep

run normal 0 \
  "over: request 8: a stop said over the lamp's answer; played  PASS" \
  "after: request 8: a stop said just after the lamp's answer; played  PASS" \
  "wordless: request 8: a cut-in with no words ended the lamp's answer; played  PASS" \
  'over-name: 1 of 1 pass'
[[ ! -e $T/d/put-back ]] || { echo "FAIL normal: it put back a drop-in it never put on"; fail=1; }
# "Stop." 1 s after the turn finished: seen 300 ms after it, so 700 ms more.
grep -qF 'the poem ended 300 ms before it was seen' "$T/out/scenarios/stop-lines-normal.txt" \
  || { echo "FAIL normal: the poem's end not reported"; fail=1; }
grep -qx '0.700' "$T/d/slept" || { echo "FAIL normal: no 0.700 s wait before the after episode's stop"; fail=1; }

FAKE_MODE=silent run silent 1 "over: request 8: a stop said over the lamp's answer; NOT PLAYED (no first answer audio)  FAIL"
FAKE_MODE=model run model 1 'over: no fixed line: request 8 heard 1 words, asked of the model  FAIL' \
  'wordless: no fixed line, and no request after the ask (a yield that resumed?)  PASS'
# The short answer's end is its turn_finished, not a retirement: none in time, and "Stop." is not said after it.
FAKE_MODE=unfinished STOP_LINES_END_WAIT=1 run unfinished 1 \
  'after: no fixed line, and no request after the ask (a yield that resumed?)  FAIL'
grep -qF "the poem's end was never seen; not played" "$T/out/scenarios/stop-lines-unfinished.txt" \
  || { echo "FAIL unfinished: \"Stop.\" was said with no turn_finished seen"; fail=1; }
# A wordless cut-in that ended the story and left the lamp quiet: the 30 Oct case.
FAKE_MODE=quiet run quiet 1 'wordless: no fixed line: request 8 heard 0 words, left quiet  FAIL'

# The drop-in, put on and taken off.
STOP_LINES_ENV="LAMP_CUT_IN_WORDLESS_ASKS=on" run dropin 0 'drop-in removed, lamp-voice restarted without it'
[[ $(grep -c removed "$T/d/put-back") == 1 ]] || { echo "FAIL dropin: not put back exactly once"; fail=1; }

# No LAMP_HOST: nothing at all.
out=$(env -u LAMP_HOST "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"LAMP_HOST is not set"* ]] || { echo "FAIL no-host: $out"; fail=1; }

[[ $fail == 0 ]] && echo "stop-lines-test: all passed"
exit $fail
