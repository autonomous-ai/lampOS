#!/bin/sh
# ask-again.sh's lamp side, run there as root (`sudo -n sh -s -- in|out`): `in` puts the stand-in Harness in as
# lamp-tasks' --harness, with --sign-in-by-code, and the voice's LAMP_TASKS_SIGN_IN=on, through /run drop-ins; `out`
# takes them out and removes the stand-in's folder. The fixture is already at $PLAYED_REMOTE/harness-fixture.
# `in` stops at the first step that fails, says which on stderr, exits 1, and leaves no drop-in written: nothing is
# played then (#2715's first run on lamp-4ace, 22 Oct 02:5x: lamp-tasks ran as orangepi, not lampos, and the run
# went on). For ask-again-test.sh, the folders and systemctl can be given:
#   PLAYED_REMOTE  the stand-in's folder (default /usr/local/lib/played-ask-again; /run may be noexec)
#   PLAYED_UNITS   where the drop-ins go (default /run/systemd/system)
#   SYSTEMCTL      systemctl (default systemctl)
REMOTE=${PLAYED_REMOTE:-/usr/local/lib/played-ask-again}
UNITS=${PLAYED_UNITS:-/run/systemd/system}
SYSTEMCTL=${SYSTEMCTL:-systemctl}
DROP=zz-played-ask-again.conf
TASKS_DROP=$UNITS/lamp-tasks.service.d/$DROP
VOICE_DROP=$UNITS/lamp-voice.service.d/$DROP
fail() { echo "ask-again setup: $1" >&2; exit 1; }

case "${1:-}" in
in)
  [ -x "$REMOTE/harness-fixture" ] || fail "no stand-in at $REMOTE/harness-fixture"
  # lamp-tasks' own user and group: the stand-in's HOME is theirs (lampos on most lamps, orangepi on lamp-4ace).
  owner=$($SYSTEMCTL show -p User -p Group lamp-tasks) || fail "systemctl show lamp-tasks failed"
  user=$(printf '%s\n' "$owner" | sed -n 's/^User=//p')
  group=$(printf '%s\n' "$owner" | sed -n 's/^Group=//p')
  [ -n "$user" ] || fail "lamp-tasks names no User"
  [ -n "$group" ] || group=$user
  home=$REMOTE/home
  mkdir -p "$home" || fail "mkdir $home failed"
  printf '{"login":{"code_flow":true}}\n' > "$home/plan.json" || fail "writing plan.json failed"
  printf '#!/bin/sh\nHOME=%s exec %s "$@"\n' "$home" "$REMOTE/harness-fixture" > "$REMOTE/harness" \
    || fail "writing the stand-in's wrapper failed"
  chmod 0755 "$REMOTE/harness" || fail "chmod of the wrapper failed"
  chown -R "$user:$group" "$home" || fail "chown $user:$group of $home failed"
  # lamp-tasks' own ExecStart, its --harness swapped for the stand-in, with --sign-in-by-code.
  exec_line=$($SYSTEMCTL cat lamp-tasks | awk '/^ExecStart=./ { line = substr($0, 11) } END { print line }')
  case "$exec_line" in *"--harness "*) ;; *) fail "lamp-tasks' ExecStart has no --harness";; esac
  exec_line=$(printf '%s\n' "$exec_line" | sed -E "s#--harness [^ ]+#--harness $REMOTE/harness#")
  case "$exec_line" in *--sign-in-by-code*) ;; *) exec_line="$exec_line --sign-in-by-code";; esac
  mkdir -p "$UNITS/lamp-tasks.service.d" "$UNITS/lamp-voice.service.d" || fail "mkdir of the drop-in folders failed"
  printf '[Service]\nExecStart=\nExecStart=%s\n' "$exec_line" > "$TASKS_DROP" || fail "writing lamp-tasks' drop-in failed"
  printf '[Service]\nEnvironment=LAMP_TASKS_SIGN_IN=on\n' > "$VOICE_DROP" || {
    rm -f "$TASKS_DROP"; fail "writing the voice's drop-in failed"; }
  $SYSTEMCTL daemon-reload && $SYSTEMCTL restart lamp-tasks lamp-voice || {
    rm -f "$TASKS_DROP" "$VOICE_DROP"; $SYSTEMCTL daemon-reload; $SYSTEMCTL restart lamp-tasks lamp-voice
    fail "restarting with the stand-in failed; put back"; }
  echo "stand-in in: lamp-tasks as $user:$group, its Harness $REMOTE/harness"
  ;;
out)
  rm -f "$TASKS_DROP" "$VOICE_DROP"
  $SYSTEMCTL daemon-reload
  $SYSTEMCTL restart lamp-tasks lamp-voice
  rm -rf "$REMOTE"
  echo "put back: lamp-tasks $($SYSTEMCTL is-active lamp-tasks), lamp-voice $($SYSTEMCTL is-active lamp-voice)"
  ;;
*) echo "usage: ask-again-setup.sh in|out" >&2; exit 2 ;;
esac
