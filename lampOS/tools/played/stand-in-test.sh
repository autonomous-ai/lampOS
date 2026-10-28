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
#   Every case's lamp-tasks gets a store of its own: the real store's file is byte-for-byte as before, and the
#   drop-in's --dir is the run's (case 1); a unit with no --dir is refused (case 7).
#   6. calendar.sh's settings: a plan of its own, a targets file for the run with the voice's own alias (0600,
#      lamp-tasks' user's, in place of --targets), and LAMP_TASKS_ACCOUNTS=on in the voice's drop-in.
#   The lamp's guard (cases 1 and 8 to 11): `in` arms a systemd-run timer before the drop-ins are written, which puts
#   back as `out` does if the Mac never comes back; `out`, and a setup that fails after arming it, stop it; a setup
#   refused before any write arms none.
#   12. a lamp with no Harness CLI (lamp-06c1): the unit's ConditionPathExists=/usr/local/bin/harness would have
#      systemd skip lamp-tasks, so the drop-in resets the conditions before its ExecStart swap, keeps the unit's
#      others, needs the stand-in's own wrapper instead, and `out` removes it with the rest.
#   13. a second `in` with the first's drop-in still in: `systemctl cat` prints that drop-in after the unit file, and
#      the setup reads ExecStart and the conditions from the unit file's own section only, so the drop-in still
#      clears the real Harness's condition and runs the unit's own ExecStart, not the stand-in's.
#   14. a unit file whose body begins with its own `# /etc/systemd/system/lamp-tasks.service` line, copied from a
#      `systemctl cat` as lamp-4ace's is: only a drop-in's `.d/*.conf` header ends the unit's section, so it reads
#      whole and the drop-in has the unit's ExecStart and clears the harness condition (Fourteen's P2 on #3665).
#   15. PLAYED_CLOUD_ENV (make-skill.sh with MAKE_FROM=app): lamp-cloud gets a drop-in of its own with the switch and
#      is restarted with the others; the guard and `out` take it out and restart it too (fourteen-P11 on #2814).
#      Without it (case 1) lamp-cloud is never touched.
#   16. a PLAYED_CLOUD_ENV setting that isn't NAME=VALUE: setup fails, says so, writes no drop-in, arms no guard.
here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
failed=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; failed=1; fi; }

# The stand-in systemctl: `show` and `cat` from files, every call kept. `cat` prints the unit file under its header
# and then each lamp-tasks drop-in in the run's units under its own, as systemd's does.
cat > "$root/systemctl" <<'EOF'
#!/bin/sh
echo "$*" >> "$STAND_IN/calls"
case "$1" in
show) case "$*" in *lamp-voice*) cat "$STAND_IN/voice_env" ;; *) cat "$STAND_IN/show" ;; esac ;;
cat) printf '# /etc/systemd/system/lamp-tasks.service\n'; cat "$STAND_IN/unit"
  for drop in "$STAND_IN"/units/lamp-tasks.service.d/*.conf; do
    [ ! -f "$drop" ] || { printf '\n# %s\n' "$drop"; cat "$drop"; }
  done ;;
is-active) echo active ;;
restart) [ -z "${FAIL_RESTART:-}" ] || exit 1 ;;
esac
EOF
chmod 0755 "$root/systemctl"
# The stand-in systemd-run: its call kept among systemctl's, and its arguments one to a line.
cat > "$root/systemd-run" <<'EOF'
#!/bin/sh
echo "systemd-run $*" >> "$STAND_IN/calls"
printf '%s\n' "$@" > "$STAND_IN/guard"
EOF
chmod 0755 "$root/systemd-run"
# Two lamp-tasks, with no --help as the real one: one has --sign-in-by-code in it, one older doesn't.
printf '#!/bin/sh\n# flags: --harness --sign-in-by-code\necho "expected serve or request" >&2; exit 1\n' > "$root/new-tasks"
printf '#!/bin/sh\n# flags: --harness\necho "expected serve or request" >&2; exit 1\n' > "$root/old-tasks"
chmod 0755 "$root/new-tasks" "$root/old-tasks"
run() {  # USER EXECSTART MODE [TASKS]: the setup against the stand-in; its output in $root/out
  rm -rf "$root/remote" "$root/units" "$root/calls" "$root/guard"
  mkdir -p "$root/remote"
  printf '#!/bin/sh\n' > "$root/remote/harness-fixture"
  chmod 0755 "$root/remote/harness-fixture"
  printf 'Group=%s\nUser=%s\n' "$(id -gn)" "$1" > "$root/show"
  { [ -z "${unit_top:-}" ] || printf '%s\n' "$unit_top"
    [ -z "${unit_head:-}" ] || printf '[Unit]\n%s\n' "$unit_head"
    printf '[Service]\nUser=%s\nExecStart=%s\n' "$1" "$2"; } > "$root/unit"
  STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
    SYSTEMD_RUN=$root/systemd-run PLAYED_TASKS=${4:-} PLAYED_GUARD_S=${guard_s:-} \
    PLAYED_TARGET=${target:-} PLAYED_VOICE_ENV=${voice_env:-} PLAYED_CLOUD_ENV=${cloud_env:-} PLAYED_PLAN=${plan:-} \
    sh "$here/stand-in.sh" "$3" > "$root/out" 2>&1
}
line="$root/new-tasks serve --enable --harness-as-person --dir $root/real-store --harness /usr/local/bin/harness --watch"
tasks_drop=$root/units/lamp-tasks.service.d/zz-played-stand-in.conf
voice_drop=$root/units/lamp-voice.service.d/zz-played-stand-in.conf
cloud_drop=$root/units/lamp-cloud.service.d/zz-played-stand-in.conf

# The real store, with a task the run must never see or change.
mkdir -p "$root/real-store"
printf 'task 381, accepted\n' > "$root/real-store/tasks.sqlite3"
real_sum=$(cksum < "$root/real-store/tasks.sqlite3")
run "$(id -un)" "$line" in; code=$?
check "1. setup as $(id -un) succeeds" '[ $code -eq 0 ]'
check "1. --dir is the run's own store, not the real one" \
  'grep -q -- "--dir $root/remote/store " "$tasks_drop" && ! grep -q -- "real-store" "$tasks_drop"'
check "1. the run's store is empty, 0700 and lamp-tasks' user's" \
  '[ -z "$(ls -A "$root/remote/store")" ] && [ "$(stat -c %a:%U "$root/remote/store" 2>/dev/null || stat -f %Lp:%Su "$root/remote/store")" = "700:$(id -un)" ]'
check "1. the stand-in's HOME is lamp-tasks' user's" '[ "$(stat -c %U "$root/remote/home" 2>/dev/null || stat -f %Su "$root/remote/home")" = "$(id -un)" ]'
check "1. --harness is the stand-in, --harness-as-person kept" \
  'grep -q -- "--harness-as-person --dir $root/remote/store --harness $root/remote/harness --watch --sign-in-by-code$" "$tasks_drop"'
check "1. the voice's switch on" 'grep -qx "Environment=LAMP_TASKS_SIGN_IN=on" "$voice_drop"'
check "1. the wrapper runs the fixture with its own HOME" 'grep -q "HOME=$root/remote/home exec $root/remote/harness-fixture" "$root/remote/harness"'
check "1. both restarted" 'grep -qx "restart lamp-tasks lamp-voice" "$root/calls"'
check "1. the guard is armed for an hour, before the first daemon-reload" \
  'grep -qx -- "--unit=zz-played-stand-in-guard" "$root/guard" && grep -qx -- "--on-active=3600s" "$root/guard" &&
   [ "$(grep -n "^systemd-run" "$root/calls" | cut -d: -f1)" -lt "$(grep -n "^daemon-reload" "$root/calls" | head -1 | cut -d: -f1)" ]'
check "1. the guard puts back as out does: both drop-ins, a reload, both restarted, the folder" \
  'grep -qx "rm -f $tasks_drop $voice_drop; systemctl daemon-reload; systemctl restart lamp-tasks lamp-voice; rm -rf $root/remote" "$root/guard"'
check "1. its command carries no \$ for systemd to expand" '[ -s "$root/guard" ] && ! tail -n 1 "$root/guard" | grep -qF "\$"'
check "1. without PLAYED_CLOUD_ENV lamp-cloud is never touched" '[ ! -e "$cloud_drop" ] && ! grep -q lamp-cloud "$root/calls" "$root/guard"'
mv "$root/calls" "$root/calls.in"  # out's own calls only: in stops any guard before it arms its own
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1
mv "$root/calls" "$root/calls.out"
check "1. out stops the guard" 'grep -qx "stop zz-played-stand-in-guard.timer" "$root/calls.out"'
check "1. out takes both drop-ins and the folder out" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && [ ! -e "$root/remote" ]'
check "1. the real store's file is byte-for-byte as before" '[ "$(cksum < "$root/real-store/tasks.sqlite3")" = "$real_sum" ]'

run no-such-user-played "$line" in; code=$?
check "2. an unknown user fails" '[ $code -ne 0 ]'
check "2. it says the chown failed" 'grep -q "chown no-such-user-played" "$root/out"'
check "2. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'
check "2. no guard armed" '! grep -q "^systemd-run" "$root/calls"'

run "$(id -un)" "$root/new-tasks serve --enable --dir $root/real-store" in; code=$?
check "3. no --harness fails" '[ $code -ne 0 ]'
check "3. it says so" 'grep -q "has no --harness" "$root/out"'
check "3. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

old_line="$root/old-tasks serve --enable --dir $root/real-store --harness /usr/local/bin/harness"
run "$(id -un)" "$old_line" in; code=$?
check "4. a lamp-tasks without --sign-in-by-code fails" '[ $code -ne 0 ]'
check "4. it says it's older than #2702" 'grep -q "older than #2702" "$root/out"'
check "4. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

run "$(id -un)" "$old_line" in "$root/new-tasks"; code=$?
check "5. with TASKS= a newer one, setup succeeds" '[ $code -eq 0 ]'
check "5. the drop-in runs it, with its own arguments" \
  'grep -qx "ExecStart=$root/new-tasks serve --enable --dir $root/remote/store --harness $root/remote/harness --sign-in-by-code" "$tasks_drop"'

printf 'Environment=LAMP_TASKS_DIR=/run/lampos-tasks LAMP_TASKS_TARGET=office-claude LAMP_TASKS_ASKED=on\n' > "$root/voice_env"
cal_line="$root/new-tasks serve --enable --dir $root/real-store --targets /var/lib/lampos-tasks/targets.json --harness /usr/local/bin/harness"
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

run "$(id -un)" "$root/new-tasks serve --enable --harness /usr/local/bin/harness" in; code=$?
check "7. no --dir fails" '[ $code -ne 0 ]'
check "7. it says so" 'grep -q "has no --dir" "$root/out"'
check "7. no drop-in written, nothing restarted" '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && ! grep -q restart "$root/calls"'

guard_s=900
run "$(id -un)" "$line" in; code=$?
unset guard_s
check "8. PLAYED_GUARD_S sets the guard's wait" '[ $code -eq 0 ] && grep -qx -- "--on-active=900s" "$root/guard"'

guard_s=soon
run "$(id -un)" "$line" in; code=$?
unset guard_s
check "9. a PLAYED_GUARD_S that isn't whole seconds fails before any write" \
  '[ $code -ne 0 ] && grep -q "PLAYED_GUARD_S" "$root/out" && [ ! -e "$tasks_drop" ] && ! grep -qs "^systemd-run" "$root/calls"'

export FAIL_RESTART=1
run "$(id -un)" "$line" in; code=$?
unset FAIL_RESTART
check "10. a restart that fails puts back and stops the guard it armed" \
  '[ $code -ne 0 ] && [ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] &&
   sed -n "/^systemd-run/,\$p" "$root/calls" | grep -qx "stop zz-played-stand-in-guard.timer"'

# The guard's command is run by /bin/sh as root: a folder it would split, or one that isn't plainly absolute, is
# refused before anything is armed or written (blue's P2 on #3231).
for bad in "$root/played stand-in" "remote" "/" "$root/../remote" "$root/it's"; do
  rm -rf "$root/units" "$root/calls" "$root/guard"
  STAND_IN=$root PLAYED_REMOTE=$bad PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl SYSTEMD_RUN=$root/systemd-run \
    sh "$here/stand-in.sh" in > "$root/out" 2>&1; code=$?
  check "11. PLAYED_REMOTE='$bad' is refused before anything is armed or written" \
    '[ $code -ne 0 ] && grep -q "PLAYED_REMOTE" "$root/out" && [ ! -e "$root/units" ] && ! grep -qs "^systemd-run" "$root/calls"'
done
rm -rf "$root/calls"
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS="$root/my units" SYSTEMCTL=$root/systemctl SYSTEMD_RUN=$root/systemd-run \
  sh "$here/stand-in.sh" in > "$root/out" 2>&1; code=$?
check "11. a PLAYED_UNITS with a space is refused too" \
  '[ $code -ne 0 ] && grep -q "PLAYED_UNITS" "$root/out" && [ ! -e "$root/my units" ] && ! grep -qs "^systemd-run" "$root/calls"'
rm -rf "$root/calls"
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units PLAYED_RESTORE="$root/saved/re store" SYSTEMCTL=$root/systemctl \
  SYSTEMD_RUN=$root/systemd-run sh "$here/stand-in.sh" in > "$root/out" 2>&1; code=$?
check "11. a PLAYED_RESTORE with a space is refused too" \
  '[ $code -ne 0 ] && grep -q "PLAYED_RESTORE" "$root/out" && ! grep -qs "^systemd-run" "$root/calls"'

unit_head="ConditionPathExists=/var/lib/lampos-setup/done
ConditionPathExists=/usr/local/bin/harness"
run "$(id -un)" "$line" in; code=$?
unset unit_head
check "12. setup on a lamp with no Harness CLI succeeds" '[ $code -eq 0 ]'
check "12. the drop-in resets ConditionPathExists in [Unit], before its ExecStart swap" \
  'r=$(grep -nx "ConditionPathExists=" "$tasks_drop" | cut -d: -f1); u=$(grep -nx "\[Unit\]" "$tasks_drop" | cut -d: -f1)
   x=$(grep -nx "ExecStart=" "$tasks_drop" | cut -d: -f1); [ -n "$r" ] && [ -n "$u" ] && [ -n "$x" ] && [ "$u" -lt "$r" ] && [ "$r" -lt "$x" ]'
check "12. it keeps the unit's other conditions and needs the stand-in's wrapper, not the real Harness" \
  'grep -qx "ConditionPathExists=/var/lib/lampos-setup/done" "$tasks_drop" &&
   grep -qx "ConditionPathExists=$root/remote/harness" "$tasks_drop" && ! grep -q "=/usr/local/bin/harness$" "$tasks_drop"'
check "12. lamp-tasks is restarted, which starts a part that was skipped" 'grep -qx "restart lamp-tasks lamp-voice" "$root/calls"'
rm -f "$root/calls"
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1
check "12. out removes the drop-in, reloads and restarts, so a lamp with no Harness skips lamp-tasks again" \
  '[ ! -e "$tasks_drop" ] && grep -n "" "$root/calls" | grep -A9 ":daemon-reload$" | grep -q ":restart lamp-tasks lamp-voice$"'

unit_head="ConditionPathExists=/var/lib/lampos-setup/done
ConditionPathExists=/usr/local/bin/harness"
run "$(id -un)" "$line" in; code=$?
unset unit_head
cp "$tasks_drop" "$root/first-drop"
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl SYSTEMD_RUN=$root/systemd-run \
  sh "$here/stand-in.sh" in > "$root/out" 2>&1; code2=$?
check "13. a second in, with the first's drop-in in place, succeeds" '[ $code -eq 0 ] && [ $code2 -eq 0 ]'
check "13. its drop-in still clears the real Harness's condition and keeps the unit's other one" \
  'grep -qx "ConditionPathExists=/var/lib/lampos-setup/done" "$tasks_drop" &&
   [ "$(grep -cx "ConditionPathExists=$root/remote/harness" "$tasks_drop")" -eq 1 ] && ! grep -q "=/usr/local/bin/harness$" "$tasks_drop"'
check "13. it runs the unit's own ExecStart, as the first in did, not the stand-in's" 'cmp -s "$root/first-drop" "$tasks_drop"'
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1

unit_top="# /etc/systemd/system/lamp-tasks.service"
unit_head="ConditionPathExists=/var/lib/lampos-setup/done
ConditionPathExists=/usr/local/bin/harness"
run "$(id -un)" "$line" in; code=$?
unset unit_top unit_head
check "14. a unit with its own copied header line, as lamp-4ace's, succeeds" '[ $code -eq 0 ]'
check "14. the drop-in has the unit's ExecStart, the stand-in in place of the real Harness" \
  'grep -qx "ExecStart=$root/new-tasks serve --enable --harness-as-person --dir $root/remote/store --harness $root/remote/harness --watch --sign-in-by-code" "$tasks_drop"'
check "14. it clears the real Harness's condition and keeps the unit's other one" \
  'grep -qx "ConditionPathExists=/var/lib/lampos-setup/done" "$tasks_drop" &&
   grep -qx "ConditionPathExists=$root/remote/harness" "$tasks_drop" && ! grep -q "=/usr/local/bin/harness$" "$tasks_drop"'
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1

cloud_env="LAMP_APP_MAKE_SKILL=on"
voice_env="LAMP_MAKE_SKILL=on LAMP_APP_MAKE_SKILL=on"
run "$(id -un)" "$line" in; code=$?
unset cloud_env voice_env
check "15. with PLAYED_CLOUD_ENV, setup succeeds" '[ $code -eq 0 ]'
check "15. lamp-cloud's drop-in has the switch" \
  '[ "$(cat "$cloud_drop" 2>/dev/null)" = "$(printf "[Service]\nEnvironment=LAMP_APP_MAKE_SKILL=on")" ]'
check "15. the voice's drop-in has both make-skill switches" \
  'grep -qx "Environment=LAMP_MAKE_SKILL=on" "$voice_drop" && grep -qx "Environment=LAMP_APP_MAKE_SKILL=on" "$voice_drop"'
check "15. lamp-cloud is restarted with the others" 'grep -qx "restart lamp-tasks lamp-voice lamp-cloud" "$root/calls"'
check "15. the guard puts lamp-cloud's drop-in back and restarts it too" \
  'grep -qx "rm -f $tasks_drop $voice_drop $cloud_drop; systemctl daemon-reload; systemctl restart lamp-tasks lamp-voice lamp-cloud; rm -rf $root/remote" "$root/guard"'
rm -f "$root/calls"
STAND_IN=$root PLAYED_REMOTE=$root/remote PLAYED_UNITS=$root/units SYSTEMCTL=$root/systemctl \
  sh "$here/stand-in.sh" out > "$root/out" 2>&1
check "15. out takes lamp-cloud's drop-in out and restarts it, with no PLAYED_CLOUD_ENV of its own" \
  '[ ! -e "$cloud_drop" ] && grep -qx "restart lamp-tasks lamp-voice lamp-cloud" "$root/calls"'

cloud_env="LAMP_APP_MAKE_SKILL"
run "$(id -un)" "$line" in; code=$?
unset cloud_env
check "16. a PLAYED_CLOUD_ENV that isn't NAME=VALUE fails" '[ $code -ne 0 ]'
check "16. it says so" 'grep -q "PLAYED_CLOUD_ENV.s LAMP_APP_MAKE_SKILL is not NAME=VALUE" "$root/out"'
check "16. no drop-in written, nothing restarted, no guard armed" \
  '[ ! -e "$tasks_drop" ] && [ ! -e "$voice_drop" ] && [ ! -e "$cloud_drop" ] && ! grep -q -e restart -e "^systemd-run" "$root/calls" 2>/dev/null'

exit $failed
