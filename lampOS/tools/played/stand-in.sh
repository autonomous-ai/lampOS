#!/bin/sh
# The played kit's stand-in Harness, on the lamp, run there as root (`sudo -n sh -s -- in|out`), for ask-again.sh
# (#2702), calendar.sh (#751) and make-skill.sh (#2424): `in` puts lamp-tasks-harness-fixture in as lamp-tasks' --harness, with
# --sign-in-by-code, and the voice's LAMP_TASKS_SIGN_IN=on, through /run drop-ins; `out` takes them out and removes
# the stand-in's folder. Nothing reaches a real Harness, account or calendar. The fixture is already at
# $PLAYED_REMOTE/harness-fixture. `in` stops at the first step that fails, says which on stderr, exits 1, and leaves
# no drop-in written: nothing is played then (#2715's first run on lamp-4ace, 22 Oct 02:5x: lamp-tasks ran as
# orangepi, not lampos, and the run went on). Its settings, all optional:
#   PLAYED_PLAN       the fixture's plan.json (default: the sign-in's {"login":{"code_flow":true}}); a plan.json
#                     already in $PLAYED_REMOTE is used before it
#   PLAYED_TARGET     `auto` (the voice's LAMP_TASKS_TARGET, else office) or an alias: a targets file for the run,
#                     that one alias on the fixture's machine with Claude Code, in place of the unit's --targets
#   PLAYED_VOICE_ENV  more NAME=VALUE for the voice's drop-in, space-separated (calendar.sh: LAMP_TASKS_ACCOUNTS=on,
#                     make-skill.sh: LAMP_MAKE_SKILL=on)
#   PLAYED_CLOUD_ENV  NAME=VALUE for a drop-in of lamp-cloud's own, space-separated, and lamp-cloud restarted with
#                     the others (make-skill.sh with MAKE_FROM=app: LAMP_APP_MAKE_SKILL=on, without which lamp-cloud's
#                     system.info never names skills.make and the app shows no "Make a skill", fourteen-P11 on
#                     #2814); unset, lamp-cloud is not touched
#   PLAYED_TASKS     a lamp-tasks for this run only, in place of the unit's own (ask-again.sh's TASKS=)
# It runs on a lamp with no Harness CLI too (lamp-06c1): lamp-tasks' drop-in clears the unit's
# ConditionPathExists=/usr/local/bin/harness, which would have systemd skip it, and needs the stand-in's wrapper
# instead; the restart starts a lamp-tasks that was skipped, and `out`'s restart leaves it skipped again.
# lamp-tasks always gets a store of its own for the run ($PLAYED_REMOTE/store, in place of --dir): the real one is
# never touched.
#   PLAYED_REMOTE     the stand-in's folder (default /usr/local/lib/played-stand-in; /run may be noexec)
#   PLAYED_GUARD_S    how long the lamp's guard waits, whole seconds (default 3600)
#   PLAYED_RESTORE    a script the guard runs first, as /bin/sh, when it is still there, with lamp-voice stopped
#                     (make-skill.sh's saved copy of the voice's state, kept outside PLAYED_REMOTE, which removes
#                     itself once put back and checked)
# and, for stand-in-test.sh, PLAYED_UNITS (where the drop-ins go, default /run/systemd/system), SYSTEMCTL and
# SYSTEMD_RUN.
# The lamp's guard: before the drop-ins are written, `in` arms a systemd-run timer (zz-played-stand-in-guard) that
# puts back as `out` does, unaided, PLAYED_GUARD_S after; `out` stops it, as does a setup that fails after arming it,
# and the next run's `in` replaces it. So a Mac that sleeps or drops off mid-run, and never runs `out`, leaves the
# lamp changed for at most that long, not overnight (lamp-0c89, 22 Oct, with full-look-row.sh, #3195). A skill a
# make-skill.sh run put on is not the guard's: that run removes it itself; the voice's state it saved is, through
# PLAYED_RESTORE, put back before anything else (Third's P2 on #3889), with lamp-voice stopped first so it writes
# none of it meanwhile (a stop that fails, or a voice still active, puts nothing back and keeps the copy: Third's
# HOLD on a8962558b), and started again straight after, the put-back passed or not (El's P3s on #3889 and #3936).
REMOTE=${PLAYED_REMOTE:-/usr/local/lib/played-stand-in}
UNITS=${PLAYED_UNITS:-/run/systemd/system}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
SYSTEMD_RUN=${SYSTEMD_RUN:-systemd-run}
GUARD=zz-played-stand-in-guard
GUARD_S=${PLAYED_GUARD_S:-3600}
DROP=zz-played-stand-in.conf
TASKS_DROP=$UNITS/lamp-tasks.service.d/$DROP
VOICE_DROP=$UNITS/lamp-voice.service.d/$DROP
CLOUD_DROP=$UNITS/lamp-cloud.service.d/$DROP
fail() { echo "stand-in setup: $1" >&2; exit 1; }
unguard() { $SYSTEMCTL stop $GUARD.timer 2>/dev/null; $SYSTEMCTL reset-failed $GUARD.timer $GUARD.service 2>/dev/null; true; }

case "${1:-}" in
in)
  case "$GUARD_S" in '' | *[!0-9]*) fail "PLAYED_GUARD_S=$GUARD_S is not whole seconds";; esac
  # The guard's command is split and run by /bin/sh as root, `rm -rf` included: its folders must be plain absolute
  # paths, letters, digits and . _ - / only, not / and with no .. (blue's P2 on #3231: a space would split
  # PLAYED_REMOTE into another folder to remove).
  for place in "PLAYED_REMOTE=$REMOTE" "PLAYED_UNITS=$UNITS" ${PLAYED_RESTORE:+"PLAYED_RESTORE=$PLAYED_RESTORE"}; do
    path=${place#*=}
    case "$path" in
      /*/* | /?*) ;;
      *) fail "${place%%=*}=$path is not an absolute path below /";;
    esac
    case "$path" in
      *[!A-Za-z0-9._/-]* | */../* | */.. ) fail "${place%%=*}=$path holds more than letters, digits and . _ - /, or a ..";;
    esac
  done
  for setting in ${PLAYED_CLOUD_ENV:-}; do
    case "$setting" in
      [A-Z_]*=*) case "${setting%%=*}" in *[!A-Z0-9_]*) fail "PLAYED_CLOUD_ENV's $setting is not NAME=VALUE";; esac ;;
      *) fail "PLAYED_CLOUD_ENV's $setting is not NAME=VALUE";;
    esac
  done
  [ -x "$REMOTE/harness-fixture" ] || fail "no stand-in at $REMOTE/harness-fixture"
  # lamp-tasks' own ExecStart, and the lamp-tasks to run: the unit's own, or PLAYED_TASKS for this run only. It
  # must know --sign-in-by-code (#1676): lamp-4ace's lamp-tasks didn't, and crash-looped on the flag (22 Oct 03:1x).
  # Only the unit file's own section of `systemctl cat`, its first: up to the first drop-in's `# /….d/….conf`
  # header. A second `in` finds the first's drop-in printed after it, whose ExecStart is the stand-in's and whose
  # conditions name the wrapper, so lamp-tasks would keep the real Harness's condition and be skipped again (El's
  # P3 on #3652). Any other `# /` line is the unit's own: lamp-4ace's unit file begins with a `# /etc/…` line
  # copied from a `systemctl cat` (Fourteen's P2 on #3665). With no drop-in header, the whole output is read.
  unit=$($SYSTEMCTL cat lamp-tasks) || fail "systemctl cat lamp-tasks failed"
  # Each step's own status is read: a pipeline's is its last command's, and a printf of a variable has none to hide.
  unit=$(printf '%s\n' "$unit" | awk 'NR > 1 && /^# \/.*\.d\/[^\/]*\.conf$/ { exit } { print }') || fail "reading lamp-tasks' unit failed"
  exec_line=$(printf '%s\n' "$unit" | awk '/^ExecStart=./ { line = substr($0, 11) } END { print line }') || fail "reading lamp-tasks' ExecStart failed"
  case "$exec_line" in *"--harness "*) ;; *) fail "lamp-tasks' ExecStart has no --harness";; esac
  # The unit's ConditionPathExists, but the one on its real Harness (#811): on a lamp with no Harness CLI (lamp-06c1)
  # systemd would skip lamp-tasks, a condition not met, and nothing would be handed over. The drop-in resets the list
  # and puts these back, with the stand-in's own wrapper in place of the real Harness.
  real_harness=${exec_line#*--harness }
  real_harness=${real_harness%% *}
  conditions=$(printf '%s\n' "$unit" | sed -n 's/^ConditionPathExists=\(..*\)$/\1/p') || fail "reading lamp-tasks' conditions failed"
  # grep -v: 1 is "none left", more is an error.
  conditions=$(printf '%s\n' "$conditions" | grep -vxF -- "$real_harness")
  [ $? -le 1 ] || fail "reading lamp-tasks' conditions failed"
  tasks=${PLAYED_TASKS:-${exec_line%% *}}
  [ -x "$tasks" ] || fail "no lamp-tasks at $tasks"
  # lamp-tasks has no --help (anything but serve or request is "expected serve or request"), so the flag is
  # looked for in the binary itself (the first builder's 03:41).
  grep -aq -- '--sign-in-by-code' "$tasks" \
    || fail "lamp-tasks is older than #2702 ($tasks has no --sign-in-by-code): give TASKS= a lamp-tasks from the fixture's build"
  exec_line="$tasks ${exec_line#* }"
  # lamp-tasks' own user and group: the stand-in's HOME is theirs (lampos on most lamps, orangepi on lamp-4ace).
  owner=$($SYSTEMCTL show -p User -p Group lamp-tasks) || fail "systemctl show lamp-tasks failed"
  user=$(printf '%s\n' "$owner" | sed -n 's/^User=//p')
  group=$(printf '%s\n' "$owner" | sed -n 's/^Group=//p')
  [ -n "$user" ] || fail "lamp-tasks names no User"
  [ -n "$group" ] || group=$user
  home=$REMOTE/home
  mkdir -p "$home" || fail "mkdir $home failed"
  # The plan: one uploaded beside the fixture (calendar.sh's), else PLAYED_PLAN, else the sign-in's.
  plan=${PLAYED_PLAN:-}
  [ -f "$REMOTE/plan.json" ] && plan=$(cat "$REMOTE/plan.json")
  [ -n "$plan" ] || plan='{"login":{"code_flow":true}}'
  printf '%s\n' "$plan" > "$home/plan.json" || fail "writing plan.json failed"
  printf '#!/bin/sh\nHOME=%s exec %s "$@"\n' "$home" "$REMOTE/harness-fixture" > "$REMOTE/harness" \
    || fail "writing the stand-in's wrapper failed"
  chmod 0755 "$REMOTE/harness" || fail "chmod of the wrapper failed"
  chown -R "$user:$group" "$home" || fail "chown $user:$group of $home failed"
  # A targets file for the run: one alias, the voice's own, on the fixture's machine.
  if [ -n "${PLAYED_TARGET:-}" ]; then
    alias=$PLAYED_TARGET
    if [ "$alias" = auto ]; then
      voice_env=$($SYSTEMCTL show -p Environment lamp-voice) || fail "systemctl show lamp-voice failed"
      alias=$(printf '%s\n' "$voice_env" | tr ' ' '\n' | sed -n 's/^\(Environment=\)\{0,1\}LAMP_TASKS_TARGET=//p' | tail -1) \
        || fail "reading the voice's target failed"
      [ -n "$alias" ] || alias=office
    fi
    case "$exec_line" in *"--targets "*) ;; *) fail "lamp-tasks' ExecStart has no --targets";; esac
    printf '{"version":1,"targets":{"%s":{"machine_id":"0123456789abcdef0123456789abcdef","project":"~/harnesses/played-stand-in","engine":"claude"}}}\n' \
      "$alias" > "$REMOTE/targets.json" || fail "writing the run's targets failed"
    chown "$user:$group" "$REMOTE/targets.json" && chmod 0600 "$REMOTE/targets.json" \
      || fail "chown or chmod of the run's targets failed"
    exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--targets [^ ]+#--targets $REMOTE/targets.json#") || fail "swapping --targets failed"
    echo "the run's target: $alias (Claude Code on the stand-in's machine)"
  fi
  # A store of its own for the run, empty at the start and removed with the rest, so nothing a check makes outlives
  # it (lamp-4ace, 22 Oct 05:4x: the calendar run's task 381 stayed in the real store and was told in the email run).
  case "$exec_line" in *"--dir "*) ;; *) fail "lamp-tasks' ExecStart has no --dir";; esac
  store=$REMOTE/store
  rm -rf "$store" && mkdir -p "$store" || fail "mkdir $store failed"
  chown "$user:$group" "$store" && chmod 0700 "$store" || fail "chown or chmod of $store failed"
  exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--dir [^ ]+#--dir $store#") || fail "swapping --dir failed"
  # Its --harness swapped for the stand-in, with --sign-in-by-code.
  exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--harness [^ ]+#--harness $REMOTE/harness#") || fail "swapping --harness failed"
  case "$exec_line" in *--sign-in-by-code*) ;; *) exec_line="$exec_line --sign-in-by-code";; esac
  # The guard first (above), so nothing is written that it would not take out. Its command has no $, which systemd
  # would expand in the unit's command line. Its put-back runs only once lamp-voice's stop succeeded and `is-active`
  # printed exactly inactive or failed; any other answer, or none, keeps the copy (Third's HOLD on b2d2dfb0b). Its
  # last step, the copy gone, is its status: a copy still there (a put-back that failed or never ran) fails the
  # guard, after its cleanup and restart.
  unguard
  restore=""
  [ -z "${PLAYED_RESTORE:-}" ] || restore="[ ! -e $PLAYED_RESTORE ] || { systemctl stop lamp-voice && systemctl is-active lamp-voice | grep -qx -e inactive -e failed && /bin/sh $PLAYED_RESTORE; systemctl start lamp-voice; }; "
  # lamp-cloud is in the run, its drop-in put back and itself restarted with the others, only with PLAYED_CLOUD_ENV.
  drops="$TASKS_DROP $VOICE_DROP"
  restarted="lamp-tasks lamp-voice"
  [ -z "${PLAYED_CLOUD_ENV:-}" ] || { drops="$drops $CLOUD_DROP"; restarted="$restarted lamp-cloud"; }
  $SYSTEMD_RUN --unit=$GUARD --on-active="${GUARD_S}s" /bin/sh -c \
    "${restore}rm -f $drops; systemctl daemon-reload; systemctl restart $restarted; rm -rf $REMOTE${PLAYED_RESTORE:+; [ ! -e $PLAYED_RESTORE ]}" \
    >/dev/null || fail "arming the lamp's guard (systemd-run) failed"
  mkdir -p "$UNITS/lamp-tasks.service.d" "$UNITS/lamp-voice.service.d" || { unguard; fail "mkdir of the drop-in folders failed"; }
  { printf '[Unit]\nConditionPathExists=\n' &&
    { [ -z "$conditions" ] || printf '%s\n' "$conditions" | sed 's/^/ConditionPathExists=/'; } &&
    printf 'ConditionPathExists=%s\n[Service]\nExecStart=\nExecStart=%s\n' "$REMOTE/harness" "$exec_line"
  } > "$TASKS_DROP" || { rm -f "$TASKS_DROP"; unguard; fail "writing lamp-tasks' drop-in failed"; }
  { printf '[Service]\nEnvironment=LAMP_TASKS_SIGN_IN=on\n'
    for setting in ${PLAYED_VOICE_ENV:-}; do printf 'Environment=%s\n' "$setting"; done
  } > "$VOICE_DROP" || {
    rm -f "$TASKS_DROP"; unguard; fail "writing the voice's drop-in failed"; }
  if [ -n "${PLAYED_CLOUD_ENV:-}" ]; then
    { mkdir -p "$UNITS/lamp-cloud.service.d" &&
      { printf '[Service]\n'; for setting in $PLAYED_CLOUD_ENV; do printf 'Environment=%s\n' "$setting"; done; } > "$CLOUD_DROP"
    } || { rm -f $drops; unguard; fail "writing lamp-cloud's drop-in failed"; }
  fi
  # $restarted is unit names split on purpose.
  $SYSTEMCTL daemon-reload && $SYSTEMCTL restart $restarted || {
    rm -f $drops; $SYSTEMCTL daemon-reload; $SYSTEMCTL restart $restarted; unguard
    fail "restarting with the stand-in failed; put back"; }
  echo "stand-in in: lamp-tasks $tasks as $user:$group, its Harness $REMOTE/harness, its store $store"
  ;;
out)
  # The guard is stopped last: a session lost before then leaves it to restart both and remove the folder (El's P3
  # on #3936), where stopping it first left nothing to.
  # lamp-cloud only if `in` gave it a drop-in (PLAYED_CLOUD_ENV), whatever this call's settings are.
  restarted="lamp-tasks lamp-voice"
  [ ! -e "$CLOUD_DROP" ] || restarted="$restarted lamp-cloud"
  rm -f "$TASKS_DROP" "$VOICE_DROP" "$CLOUD_DROP"
  $SYSTEMCTL daemon-reload
  $SYSTEMCTL restart $restarted
  rm -rf "$REMOTE"
  unguard
  echo "put back: lamp-tasks $($SYSTEMCTL is-active lamp-tasks), lamp-voice $($SYSTEMCTL is-active lamp-voice)"
  ;;
*) echo "usage: stand-in.sh in|out" >&2; exit 2 ;;
esac
