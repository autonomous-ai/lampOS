#!/bin/sh
# The played kit's stand-in Harness, on the lamp, run there as root (`sudo -n sh -s -- in|out`), for ask-again.sh
# (#2702) and calendar.sh (#751): `in` puts lamp-tasks-harness-fixture in as lamp-tasks' --harness, with
# --sign-in-by-code, and the voice's LAMP_TASKS_SIGN_IN=on, through /run drop-ins; `out` takes them out and removes
# the stand-in's folder. Nothing reaches a real Harness, account or calendar. The fixture is already at
# $PLAYED_REMOTE/harness-fixture. `in` stops at the first step that fails, says which on stderr, exits 1, and leaves
# no drop-in written: nothing is played then (#2715's first run on lamp-4ace, 22 Oct 02:5x: lamp-tasks ran as
# orangepi, not lampos, and the run went on). Its settings, all optional:
#   PLAYED_PLAN       the fixture's plan.json (default: the sign-in's {"login":{"code_flow":true}}); a plan.json
#                     already in $PLAYED_REMOTE is used before it
#   PLAYED_TARGET     `auto` (the voice's LAMP_TASKS_TARGET, else office) or an alias: a targets file for the run,
#                     that one alias on the fixture's machine with Claude Code, in place of the unit's --targets
#   PLAYED_VOICE_ENV  more NAME=VALUE for the voice's drop-in, space-separated (calendar.sh: LAMP_TASKS_ACCOUNTS=on)
#   PLAYED_TASKS      a lamp-tasks for this run only, in place of the unit's own (ask-again.sh's TASKS=)
# lamp-tasks always gets a store of its own for the run ($PLAYED_REMOTE/store, in place of --dir): the real one is
# never touched.
#   PLAYED_REMOTE     the stand-in's folder (default /usr/local/lib/played-stand-in; /run may be noexec)
# and, for stand-in-test.sh, PLAYED_UNITS (where the drop-ins go, default /run/systemd/system) and SYSTEMCTL.
REMOTE=${PLAYED_REMOTE:-/usr/local/lib/played-stand-in}
UNITS=${PLAYED_UNITS:-/run/systemd/system}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
DROP=zz-played-stand-in.conf
TASKS_DROP=$UNITS/lamp-tasks.service.d/$DROP
VOICE_DROP=$UNITS/lamp-voice.service.d/$DROP
fail() { echo "stand-in setup: $1" >&2; exit 1; }

case "${1:-}" in
in)
  [ -x "$REMOTE/harness-fixture" ] || fail "no stand-in at $REMOTE/harness-fixture"
  # lamp-tasks' own ExecStart, and the lamp-tasks to run: the unit's own, or PLAYED_TASKS for this run only. It
  # must know --sign-in-by-code (#1676): lamp-4ace's lamp-tasks didn't, and crash-looped on the flag (22 Oct 03:1x).
  exec_line=$($SYSTEMCTL cat lamp-tasks | awk '/^ExecStart=./ { line = substr($0, 11) } END { print line }')
  case "$exec_line" in *"--harness "*) ;; *) fail "lamp-tasks' ExecStart has no --harness";; esac
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
      alias=$($SYSTEMCTL show -p Environment lamp-voice | tr ' ' '\n' | sed -n 's/^\(Environment=\)\{0,1\}LAMP_TASKS_TARGET=//p' | tail -1)
      [ -n "$alias" ] || alias=office
    fi
    case "$exec_line" in *"--targets "*) ;; *) fail "lamp-tasks' ExecStart has no --targets";; esac
    printf '{"version":1,"targets":{"%s":{"machine_id":"0123456789abcdef0123456789abcdef","project":"~/harnesses/played-stand-in","engine":"claude"}}}\n' \
      "$alias" > "$REMOTE/targets.json" || fail "writing the run's targets failed"
    chown "$user:$group" "$REMOTE/targets.json" && chmod 0600 "$REMOTE/targets.json" \
      || fail "chown or chmod of the run's targets failed"
    exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--targets [^ ]+#--targets $REMOTE/targets.json#")
    echo "the run's target: $alias (Claude Code on the stand-in's machine)"
  fi
  # A store of its own for the run, empty at the start and removed with the rest, so nothing a check makes outlives
  # it (lamp-4ace, 22 Oct 05:4x: the calendar run's task 381 stayed in the real store and was told in the email run).
  case "$exec_line" in *"--dir "*) ;; *) fail "lamp-tasks' ExecStart has no --dir";; esac
  store=$REMOTE/store
  rm -rf "$store" && mkdir -p "$store" || fail "mkdir $store failed"
  chown "$user:$group" "$store" && chmod 0700 "$store" || fail "chown or chmod of $store failed"
  exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--dir [^ ]+#--dir $store#")
  # Its --harness swapped for the stand-in, with --sign-in-by-code.
  exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--harness [^ ]+#--harness $REMOTE/harness#")
  case "$exec_line" in *--sign-in-by-code*) ;; *) exec_line="$exec_line --sign-in-by-code";; esac
  mkdir -p "$UNITS/lamp-tasks.service.d" "$UNITS/lamp-voice.service.d" || fail "mkdir of the drop-in folders failed"
  printf '[Service]\nExecStart=\nExecStart=%s\n' "$exec_line" > "$TASKS_DROP" || fail "writing lamp-tasks' drop-in failed"
  { printf '[Service]\nEnvironment=LAMP_TASKS_SIGN_IN=on\n'
    for setting in ${PLAYED_VOICE_ENV:-}; do printf 'Environment=%s\n' "$setting"; done
  } > "$VOICE_DROP" || {
    rm -f "$TASKS_DROP"; fail "writing the voice's drop-in failed"; }
  $SYSTEMCTL daemon-reload && $SYSTEMCTL restart lamp-tasks lamp-voice || {
    rm -f "$TASKS_DROP" "$VOICE_DROP"; $SYSTEMCTL daemon-reload; $SYSTEMCTL restart lamp-tasks lamp-voice
    fail "restarting with the stand-in failed; put back"; }
  echo "stand-in in: lamp-tasks $tasks as $user:$group, its Harness $REMOTE/harness, its store $store"
  ;;
out)
  rm -f "$TASKS_DROP" "$VOICE_DROP"
  $SYSTEMCTL daemon-reload
  $SYSTEMCTL restart lamp-tasks lamp-voice
  rm -rf "$REMOTE"
  echo "put back: lamp-tasks $($SYSTEMCTL is-active lamp-tasks), lamp-voice $($SYSTEMCTL is-active lamp-voice)"
  ;;
*) echo "usage: stand-in.sh in|out" >&2; exit 2 ;;
esac
