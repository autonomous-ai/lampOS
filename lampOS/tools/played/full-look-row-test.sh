#!/usr/bin/env bash
# Tests for full-look-row.sh with no lamp and no sound: a stand-in lamp_ssh (the eyes' unit, its drop-in, its 10 s
# lines and the presence file), scenario.py (one scene with "Let me look." and the look's answer), quiet_gate.sh and a
# sleep that returns at once. Needs zsh, as the script does (ZSH= another zsh).
# Run: lampOS/tools/played/full-look-row-test.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ZSH=${ZSH:-zsh}
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
cp "$HERE/full-look-row.sh" "$HERE/settings.sh" "$T/"

cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
# The eyes run their installed line, or the drop-in's arm while $FAKE_DIR/dropin holds it.
echo "$*" >>"$FAKE_DIR/calls"
LINE="/opt/lampOS/eyes/current/lamp-eyes --sight-file /run/lampos-eyes/sight.jpg --full-look 1280x960 --full-look-file /run/lampos-eyes/look.jpg --sight-from-look on"
running() { if [[ -s $FAKE_DIR/dropin ]]; then cat "$FAKE_DIR/dropin"; else echo "$LINE"; fi; }
case "$*" in
  "ls /run/systemd/system/lamp-eyes.service.d 2>/dev/null") [[ -n ${FAKE_OTHER_DROPIN:-} ]] && echo other.conf; true ;;
  *"grep \"^ExecStart=\""*) echo "$LINE" ;;
  *"cat > /tmp/zz-look-row.conf"*)
    sed -n 's/^ExecStart=\(.\)/\1/p' >"$FAKE_DIR/dropin"
    # The lamp's own guard: armed with the drop-in, to take it off unaided.
    if [[ "$*" == *"systemd-run --unit=zz-look-row-guard --on-active="*"rm -f /run/systemd/system/lamp-eyes.service.d/zz-look-row.conf"*"restart lamp-eyes"* ]]; then
      echo armed >"$FAKE_DIR/guard"
    fi ;;
  *"systemctl is-active lamp-eyes; tr"*) echo active; running ;;
  *"tr \"\\0\""*) running ;;
  "date +%s") echo 1000 ;;
  *"journalctl -u lamp-eyes"*)
    # The Mac goes to sleep mid-arm: the script dies with no trap run. By its PID, which run() keeps: BSD pkill
    # leaves out its own ancestors, and this stand-in is the script's child (the reviewer-lead's P2 on #3195).
    [[ -n ${FAKE_DIE:-} ]] && kill -KILL "$(cat "$FAKE_DIR/pid")"
    for n in 1 2 3; do echo "lamp-eyes: 69 frames, looked at 16, a face in 0, 0 unreadable, 13$n ms a look, 2 of them unchanged and not detected, 140 ms a detected look; ms a look: sight file 8.$n, decode 22.$n, grid 1.0"; done ;;
  *"rm -f /run/systemd/system/lamp-eyes.service.d/zz-look-row.conf"*)
    echo restore >>"$FAKE_DIR/restores"
    [[ "$*" == *"systemctl stop zz-look-row-guard.timer"* ]] && echo stopped >"$FAKE_DIR/guard"
    if [[ $(wc -l <"$FAKE_DIR/restores") -le ${FAKE_RESTORE_FAILS:-0} ]]; then exit 1; fi
    rm -f "$FAKE_DIR/dropin" ;;
  *"/run/lampos-eyes/present"*)
    asks=$(cat "$FAKE_DIR/asks" 2>/dev/null || echo 0)
    if [[ -n ${FAKE_FACE_AT:-} && $asks -ge $((FAKE_FACE_AT - 1)) ]]; then echo face; else echo none; fi ;;
  *) echo "lamp_ssh stand-in: unknown call: $*" >&2; exit 1 ;;
esac
EOF

cat >"$T/scenario.py" <<'EOF'
#!/usr/bin/env python3
import os, sys
d, out = os.environ['FAKE_DIR'], os.environ['PLAYED_OUT']
n = int(open(d + '/asks').read()) + 1 if os.path.exists(d + '/asks') else 1
open(d + '/asks', 'w').write(str(n))
if os.environ.get('FAKE_SCENE_FAIL'): sys.exit(77)
open('%s/scenarios/20261022T00%04dZ-%s.txt' % (out, n, sys.argv[1]), 'w').write(
    '   1.00s PLAY  Daniel: Hey Lamp, what do you see?\n'
    '   3.10s       [end of speech decided after 600 ms of quiet]\n'
    '   4.50s       [lamp starts speaking 1.40 s after that decision]\n'
    '   4.60s LAMP  Let me look.\n'
    '   8.90s       [lamp starts speaking 5.80 s after that decision]\n'
    '   9.00s LAMP  A desk with a laptop on it.\n')
EOF

cat >"$T/quiet_gate.sh" <<'EOF'
#!/usr/bin/env bash
echo quiet >>"$FAKE_DIR/quiets"
[[ -z ${FAKE_NOT_QUIET:-} ]]
EOF
mkdir -p "$T/bin"
printf '#!/bin/sh\nexit 0\n' >"$T/bin/sleep"
chmod +x "$T/lamp_ssh" "$T/scenario.py" "$T/quiet_gate.sh" "$T/bin/sleep"

fails=0
run() { # run NAME ENV...: a fresh stand-in lamp; the script's output in $T/NAME.out, its exit in $T/NAME.exit
  local name=$1; shift
  rm -rf "$T/fake" "$T/out"; mkdir -p "$T/fake" "$T/out/scenarios" "$T/out/prompts/generated"
  env PATH="$T/bin:$PATH" FAKE_DIR="$T/fake" PLAYED_OUT="$T/out" LAMP_HOST=stand-in "$@" \
    "$ZSH" "$T/full-look-row.sh" "t-$name" >"$T/$name.out" 2>&1 &
  local pid=$!
  echo "$pid" >"$T/fake/pid"
  wait "$pid" 2>/dev/null
  echo $? >"$T/$name.exit"
}
check() { # check NAME WHAT CONDITION...
  local name=$1 what=$2; shift 2
  if "$@"; then echo "ok   $name: $what"; else echo "FAIL $name: $what"; sed 's/^/     | /' "$T/$name.out"; fails=$((fails + 1)); fi
}
exit_is() { [[ $(cat "$T/$1.exit") == "$2" ]]; }
asks() { cat "$T/fake/asks" 2>/dev/null || echo 0; }
no_dropin() { [[ ! -s $T/fake/dropin ]]; }
guard_is() { [[ $(cat "$T/fake/guard" 2>/dev/null) == "$1" ]]; }

run healthy
check healthy "exits 0" exit_is healthy 0
check healthy "ten asks" test "$(asks)" = 10
check healthy "the pair's medians" grep -q "sight-from-look off: 3 lines, ms a look 132.0, sight file 8.2, decode 22.2" "$T/healthy.out"
check healthy "both arms put on" grep -q "sight-from-look on: 3 lines" "$T/healthy.out"
check healthy "each ask's two sounds" grep -q "ask 10: first sound 1.40 s, second 5.80 s; said: Let me look. | A desk with a laptop on it." "$T/healthy.out"
check healthy "the drop-in is off after" no_dropin
check healthy "quiet before each eyes change and each ask" test "$(wc -l <"$T/fake/quiets")" -ge 13
check healthy "the lamp's guard was armed with each drop-in" test "$(grep -c 'systemd-run --unit=zz-look-row-guard' "$T/fake/calls")" = 2
check healthy "the put-back stopped the guard" guard_is stopped

run loud FAKE_NOT_QUIET=1 QUIET_MAX=20
check loud "no guard was armed" test ! -e "$T/fake/guard"
check loud "stops (exit 5)" exit_is loud 5
check loud "nothing played" test "$(asks)" = 0
check loud "the eyes never changed" bash -c "! grep -q 'cat > /tmp/zz-look-row.conf' '$T/fake/calls'"
check loud "the eyes never restarted" bash -c "! grep -q 'restart lamp-eyes' '$T/fake/calls'"

run face FAKE_FACE_AT=3
check face "stops at a face (exit 8)" exit_is face 8
check face "two asks played, none with the face" test "$(asks)" = 2
check face "the drop-in is off" no_dropin

run restore-once FAKE_RESTORE_FAILS=1
check restore-once "the second restore takes, and the run goes on (exit 0)" exit_is restore-once 0
check restore-once "ten asks" test "$(asks)" = 10
check restore-once "the drop-in is off" no_dropin

run restore-never FAKE_RESTORE_FAILS=99
check restore-never "stops (exit 6)" exit_is restore-never 6
check restore-never "nothing played" test "$(asks)" = 0
check restore-never "says what to do by hand" grep -q "take it off and restart lamp-eyes by hand" "$T/restore-never.out"

run die FAKE_DIE=1
check die "the script was killed mid-arm (exit 137)" exit_is die 137
check die "no put-back ran" test ! -e "$T/fake/restores"
check die "the drop-in is still on" test -s "$T/fake/dropin"
check die "but the lamp's guard is armed to take it off" guard_is armed

run scene-fails FAKE_SCENE_FAIL=1
check scene-fails "stops (exit 7)" exit_is scene-fails 7
check scene-fails "no results printed" bash -c "! grep -q '^ask 1:' '$T/scene-fails.out'"

run other-dropin FAKE_OTHER_DROPIN=1
check other-dropin "refuses (exit 3)" exit_is other-dropin 3
check other-dropin "touches nothing" bash -c "! grep -qE 'cat > /tmp/zz-look-row.conf|rm -f' '$T/fake/calls'"

echo "full-look-row-test: $fails failed"
[[ $fails -eq 0 ]]
