#!/bin/sh
# stand-in.sh against a stand-in systemctl, with no lamp, no root and nothing played: run it after changing
# the setup. Each case prints ok or FAIL; it exits 1 if any failed.
#   1. lamp-tasks as this user (not lampos, as on lamp-4ace): the stand-in in, its HOME this user's, --harness
#      swapped and --sign-in-by-code added, the voice's switch on, both restarted; `out` takes it all out.
#   2. lamp-tasks naming a user that doesn't exist: setup fails at the chown, says so, writes no drop-in.
#   3. lamp-tasks with no --harness: setup fails, says so, writes no drop-in.
#   4. a lamp-tasks without --sign-in-by-code in it (older than #2702, as lamp-4ace's): setup fails, says so, no
#      drop-in. Neither stand-in has a --help, as lamp-tasks has none.
#   5. the same, with a newer lamp-tasks given for the run (TASKS=): it runs in the drop-in's ExecStart.
#   6. calendar.sh's settings: a plan of its own, a targets file for the run with the voice's own alias (0600,
#      lamp-tasks' user's, in place of --targets), and LAMP_TASKS_ACCOUNTS=on in the voice's drop-in.
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
show) case "$*" in *lamp-voice*) cat "$STAND_IN/voice_env" ;; *) cat "$STAND_IN/show" ;; esac ;;
cat) cat "$STAND_IN/unit" ;;
is-active) echo active ;;
esac
EOF
chmod 0755 "$root/systemctl"
# Two lamp-tasks, with no --help as the real one: one has --sign-in-by-code in it, one older doesn't.
printf '#!/bin/sh\n# flags: --harness --sign-in-by-code\necho "expected serve or request" >&2; exit 1\n' > "$root/new-tasks"
printf '#!/bin/sh\n# flags: --harness\necho "expected serve or request" >&2; exit 1\n' > "$root/old-tasks"
chmod 0755 "$root/new-tasks" "$root/old-tasks"
run() {  # USER EXECSTART MODE [TASKS]: the setup against the stand-in; its output in $root/out
  rm -rf "$root/remote" "$root/units" "$root/calls"
  mkdir -p "$root/remote"
  printf '#!/bin/sh\n' > "$root/remote/harness-fixture"
  chmod 0755 "$root/remote/harness-fixture"
  printf 'Group=%s\nUser=%s\n' "$(id -gn)" "$1" > "$root/show"
  printf '[Service]\nUser=%s\nExecStart=%s\n' "$1" "$2" > "$root/unit"
  STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl PLAYED_TASKS=${4:-} \
    PLAYED_TARGET=${target:-} PLAYED_VOICE_ENV=${voice_env:-} PLAYED_PLAN=${plan:-} \
    sh "$here/stand-in.sh" "$3" > "$root/out" 2>&1
}
line="$root/new-tasks serve --enable --harness-as-person --harness /usr/local/bin/harness --watch"
tasks_drop=$root/units/lamp-tasks.service.d/zz-played-stand-in.conf
voice_drop=$root/units/lamp-voice.service.d/zz-played-stand-in.conf

run "$(id -un)" "$line" in; code=$?
check "1. setup as $(id -un) succeeds" '[ $code -eq 0 ]'
check "1. the stand-in's HOME is lamp-tasks' user's" '[ "$(stat -c %U "$root/remote/home" 2>/dev/null || stat -f %Su "$root/remote/home")" = "$(id -un)" ]'
check "1. --harness is the stand-in, --harness-as-person kept" \
  'grep -q -- "--harness-as-person --harness $root/remote/harness --watch --sign-in-by-code$" "$tasks_drop"'
check "1. the voice's switch on" 'grep -qx "Environment=LAMP_TASKS_SIGN_IN=on" "$voice_drop"'
check "1. the wrapper runs the fixture with its own HOME" 'grep -q "HOME=$root/remote/home exec $root/remote/harness-fixture" "$root/remote/harness"'
check "1. both restarted" 'grep -qx "restart lamp-tasks lamp-voice" "$root/calls"'
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1
check "1. out takes both drop-ins and the folder out" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && [ ! -e "$root/remote" ]'

run no-such-user-played "$line" in; code=$?
check "2. an unknown user fails" '[ $code -ne 0 ]'
check "2. it says the chown failed" 'grep -q "chown no-such-user-played" "$root/out"'
check "2. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

run "$(id -un)" "$root/new-tasks serve --enable" in; code=$?
check "3. no --harness fails" '[ $code -ne 0 ]'
check "3. it says so" 'grep -q "has no --harness" "$root/out"'
check "3. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

old_line="$root/old-tasks serve --enable --harness /usr/local/bin/harness"
run "$(id -un)" "$old_line" in; code=$?
check "4. a lamp-tasks without --sign-in-by-code fails" '[ $code -ne 0 ]'
check "4. it says it's older than #2702" 'grep -q "older than #2702" "$root/out"'
check "4. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

run "$(id -un)" "$old_line" in "$root/new-tasks"; code=$?
check "5. with TASKS= a newer one, setup succeeds" '[ $code -eq 0 ]'
check "5. the drop-in runs it, with its own arguments" \
  'grep -qx "ExecStart=$root/new-tasks serve --enable --harness $root/remote/harness --sign-in-by-code" "$tasks_drop"'

printf 'Environment=LAMP_TASKS_DIR=/run/lampos-tasks LAMP_TASKS_TARGET=office-claude LAMP_TASKS_ASKED=on\n' > "$root/voice_env"
cal_line="$root/new-tasks serve --enable --targets /var/lib/lampos-tasks/targets.json --harness /usr/local/bin/harness"
target=auto voice_env=LAMP_TASKS_ACCOUNTS=on plan='{"show":{"stdout":"x"}}'
run "$(id -un)" "$cal_line" in; code=$?
unset target voice_env plan
check "6. calendar.sh's setup succeeds" '[ $code -eq 0 ]'
check "6. the fixture's plan is the one given" 'grep -qx "{\"show\":{\"stdout\":\"x\"}}" "$root/remote/home/plan.json"'
check "6. the run's target is the voice's own alias, on the fixture's machine" \
  'grep -q "\"office-claude\":{\"machine_id\":\"0123456789abcdef0123456789abcdef\"" "$root/remote/targets.json"'
check "6. the run's targets are 0600 and lamp-tasks' user's" \
  '[ "$(stat -c %a:%U "$root/remote/targets.json" 2>/dev/null || stat -f %Lp:%Su "$root/remote/targets.json")" = "600:$(id -un)" ]'
check "6. --targets is the run's" 'grep -q -- "--targets $root/remote/targets.json --harness $root/remote/harness" "$tasks_drop"'
check "6. the voice's drop-in has LAMP_TASKS_ACCOUNTS=on" 'grep -qx "Environment=LAMP_TASKS_ACCOUNTS=on" "$voice_drop"'

exit $failed
