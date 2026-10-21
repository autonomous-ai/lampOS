#!/bin/sh
# ask-again-setup.sh against a stand-in systemctl, with no lamp, no root and nothing played: run it after changing
# the setup. Each case prints ok or FAIL; it exits 1 if any failed.
#   1. lamp-tasks as this user (not lampos, as on lamp-4ace): the stand-in in, its HOME this user's, --harness
#      swapped and --sign-in-by-code added, the voice's switch on, both restarted; `out` takes it all out.
#   2. lamp-tasks naming a user that doesn't exist: setup fails at the chown, says so, writes no drop-in.
#   3. lamp-tasks with no --harness: setup fails, says so, writes no drop-in.
here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
failed=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; failed=1; fi; }

# The stand-in systemctl: `show` and `cat` from files, every call kept.
cat > "$root/systemctl" <<'EOF'
#!/bin/sh
echo "$*" >> "$STAND_IN/calls"
case "$1" in
show) cat "$STAND_IN/show" ;;
cat) cat "$STAND_IN/unit" ;;
is-active) echo active ;;
esac
EOF
chmod 0755 "$root/systemctl"
run() {  # USER EXECSTART MODE: the setup against the stand-in; its output in $root/out
  rm -rf "$root/remote" "$root/units" "$root/calls"
  mkdir -p "$root/remote"
  printf '#!/bin/sh\n' > "$root/remote/harness-fixture"
  chmod 0755 "$root/remote/harness-fixture"
  printf 'Group=%s\nUser=%s\n' "$(id -gn)" "$1" > "$root/show"
  printf '[Service]\nUser=%s\nExecStart=%s\n' "$1" "$2" > "$root/unit"
  STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
    sh "$here/ask-again-setup.sh" "$3" > "$root/out" 2>&1
}
line='/opt/lampOS/tasks/current/lamp-tasks serve --enable --harness-as-person --harness /usr/local/bin/harness --watch'
tasks_drop=$root/units/lamp-tasks.service.d/zz-played-ask-again.conf
voice_drop=$root/units/lamp-voice.service.d/zz-played-ask-again.conf

run "$(id -un)" "$line" in; code=$?
check "1. setup as $(id -un) succeeds" '[ $code -eq 0 ]'
check "1. the stand-in's HOME is lamp-tasks' user's" '[ "$(stat -c %U "$root/remote/home" 2>/dev/null || stat -f %Su "$root/remote/home")" = "$(id -un)" ]'
check "1. --harness is the stand-in, --harness-as-person kept" \
  'grep -q -- "--harness-as-person --harness $root/remote/harness --watch --sign-in-by-code$" "$tasks_drop"'
check "1. the voice's switch on" 'grep -qx "Environment=LAMP_TASKS_SIGN_IN=on" "$voice_drop"'
check "1. the wrapper runs the fixture with its own HOME" 'grep -q "HOME=$root/remote/home exec $root/remote/harness-fixture" "$root/remote/harness"'
check "1. both restarted" 'grep -qx "restart lamp-tasks lamp-voice" "$root/calls"'
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/ask-again-setup.sh" out > "$root/out" 2>&1
check "1. out takes both drop-ins and the folder out" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && [ ! -e "$root/remote" ]'

run no-such-user-played "$line" in; code=$?
check "2. an unknown user fails" '[ $code -ne 0 ]'
check "2. it says the chown failed" 'grep -q "chown no-such-user-played" "$root/out"'
check "2. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

run "$(id -un)" '/opt/lampOS/tasks/current/lamp-tasks serve --enable' in; code=$?
check "3. no --harness fails" '[ $code -ne 0 ]'
check "3. it says so" 'grep -q "has no --harness" "$root/out"'
check "3. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

exit $failed
