#!/usr/bin/env bash
# Tests for desk-kit.sh's put-on and put-back with no lamp and no sound: a stand-in lamp_ssh (lamp-voice's /run
# drop-in folder, its lock, the lamp's guard timer), a stub osascript and a sleep that returns at once. Each run stops
# at the first arm's preflight, before anything would be played: the stand-in reports the arm's switches unset, so
# the kit exits 6 and puts the lamp back; or, with FAKE_DIE, the stand-in kills the kit there, as a Mac that sleeps
# mid-arm would, and nothing on the Mac puts it back.
# Run: lampOS/tools/played/desk-kit-test.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
cp "$HERE/desk-kit.sh" "$HERE/settings.sh" "$T/"

cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
# The drop-in is $FAKE_DIR/dropin, the guard's state $FAKE_DIR/guard (armed or stopped), and every call is logged.
echo "$*" >>"$FAKE_DIR/calls"
D=/run/systemd/system/lamp-voice.service.d
case "$*" in
  "ls $D/ 2>/dev/null") [[ -n ${FAKE_OTHER_DROPIN:-} ]] && echo zz-desk-kit.conf; true ;;
  "date +%s") echo 1000 ;;
  *"t=\$(mktemp) && cat > \$t"*)
    # The put-on: the staged text's first line is the run's token; the guard is armed before the write.
    cat >"$FAKE_DIR/staged"
    if [[ "$*" == *"systemd-run --unit=zz-desk-kit-guard --on-active="* ]]; then
      echo armed >"$FAKE_DIR/guard"
      printf '%s\n' "$*" >"$FAKE_DIR/guard-call"
    fi
    cp "$FAKE_DIR/staged" "$FAKE_DIR/dropin" ;;
  *"readlink /proc/"*)
    # The first arm's preflight: the Mac dies here (FAKE_DIE), or the switches read unset and the kit stops (exit 6).
    [[ -n ${FAKE_DIE:-} ]] && kill -KILL "$(cat "$FAKE_DIR/pid")"
    echo "/opt/lampOS/voice/current/lamp-voice" ;;
  *"rm -f $D/zz-desk-kit.conf"*"is-active lamp-voice"*)
    # The put-back: the guard stopped first, then the drop-in off while it is still this run's.
    [[ "$*" == *"systemctl stop zz-desk-kit-guard.timer"*"rm -f $D/zz-desk-kit.conf"* ]] && echo stopped >"$FAKE_DIR/guard"
    rm -f "$FAKE_DIR/dropin"; echo active ;;
  *) echo "lamp_ssh stand-in: unknown call: $*" >&2; exit 1 ;;
esac
EOF
mkdir -p "$T/bin"
printf '#!/bin/sh\nexit 0\n' >"$T/bin/sleep"
printf '#!/bin/sh\nexit 0\n' >"$T/bin/osascript"
chmod +x "$T/lamp_ssh" "$T/bin/sleep" "$T/bin/osascript"

fails=0
run() { # run NAME ENV...: a fresh stand-in lamp; the kit's output in $T/NAME.out, its exit in $T/NAME.exit
  local name=$1; shift
  rm -rf "$T/fake" "$T/out"; mkdir -p "$T/fake" "$T/out"
  env PATH="$T/bin:$PATH" FAKE_DIR="$T/fake" PLAYED_OUT="$T/out" LAMP_HOST=stand-in LAMP_SSH="$T/lamp_ssh" \
    LOANS= DESK_KIT_TAR=none LAMP_PLAY_AT_LUNCH=yes ARMS="a:LAMP_X=on" TRIALS=2 "$@" \
    bash "$T/desk-kit.sh" "t-$name" >"$T/$name.out" 2>&1 &
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
guard_is() { [[ $(cat "$T/fake/guard" 2>/dev/null) == "$1" ]]; }
token() { head -1 "$T/fake/staged"; }

run stops
check stops "stops at the preflight (exit 6)" exit_is stops 6
check stops "the guard was armed with the drop-in" grep -q 'systemd-run --unit=zz-desk-kit-guard' "$T/fake/guard-call"
check stops "the guard waits one arm and 10 minutes: (2 x 8 + 3) x 60 + 600 s" grep -q -- '--on-active=1740s' "$T/fake/guard-call"
check stops "the guard removes the drop-in only while it is this run's: its token, under the kit's lock" \
  bash -c 'grep -qF "flock /run/lock/desk-kit.lock" "$1" && grep -qF -- "grep -qxF \\\"$2\\\" && rm -f /run/systemd/system/lamp-voice.service.d/zz-desk-kit.conf" "$1"' _ "$T/fake/guard-call" "$(token)"
check stops "the guard's restart follows only that removal" grep -qF 'zz-desk-kit.conf" && systemctl daemon-reload && systemctl restart lamp-voice' "$T/fake/guard-call"
check stops "the guard's command carries no \$ for systemd to expand" bash -c "test -s '$T/fake/guard-call' && ! sed -n \"s/.*systemd-run\\(.*restart lamp-voice'\\).*/\\1/p\" '$T/fake/guard-call' | grep -qF '\$'"
check stops "the put-back stopped the guard" guard_is stopped
check stops "the drop-in is off" test ! -e "$T/fake/dropin"

run die FAKE_DIE=1
check die "the kit was killed mid-arm (exit 137)" exit_is die 137
check die "no put-back ran" bash -c "! grep -q 'is-active lamp-voice' '$T/fake/calls'"
check die "the drop-in is still on" test -s "$T/fake/dropin"
check die "but the lamp's guard is armed to take it off" guard_is armed

run other FAKE_OTHER_DROPIN=1
check other "refuses (exit 3)" exit_is other 3
check other "no guard was armed, nothing written" bash -c "! grep -q 'systemd-run\|mktemp' '$T/fake/calls'"

run one-trial TRIALS=1
check one-trial "one scene an arm: (1 x 8 + 3) x 60 + 600 s" grep -q -- '--on-active=1260s' "$T/fake/guard-call"

echo "desk-kit-test: $fails failed"
[[ $fails -eq 0 ]]
