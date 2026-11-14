#!/bin/sh
# lampOS: an agent on the lamp, by the one run path (#6510, #6685). Written by lamp-update.
# lampOS/docs/install.md, "One agent namespace for every engine". It runs the agent below, as its caller (lampos,
# Harness's user), in a user, mount and PID namespace (/usr/local/lib/lampos/agent-ns) with its own home: no lamp
# service's files or sockets, no camera, mic or serial device, no lamp process. A skill's run, started by lamp-tasks
# in its own folder, gets its one-run token and a network namespace of its own. Only the agent's data differs from
# one agent to another; the run path after it is the same for every agent.
# The agent's data:
E=hermes
H=/var/lib/lampos-hermes
X=/var/lib/lampos-hermes/.local/bin/hermes
agent_env() {
  T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || fail "no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_HERMES_ON_LAMP=on"
  LAMP_HERMES_TOKEN=$T HERMES_HOME=/var/lib/lampos-hermes/.hermes
  export LAMP_HERMES_TOKEN HERMES_HOME
  unset T HERMES_YOLO_MODE
  case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_MAKE ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) LAMP_NS_MAKE=$LAMP_NS_HOME HERMES_HOME="$LAMP_NS_HOME/.hermes"; export LAMP_NS_MAKE HERMES_HOME ;; *) unset LAMP_NS_MAKE ;; esac
  :
}
agent_run_env() {
  # Nobody answers a skill's run: Hermes asks no approval in it, its hardline list still holds (#6684).
  HERMES_YOLO_MODE=1
  export HERMES_YOLO_MODE
  :
}
agent_task() {
  M=$(cat -- "$1") && [ -n "$M" ] || fail "no task in $1"
  agent_start chat -q "$M"
}
# The run path, the same for every agent:
fail() { echo "$E: $1" >&2; exit 1; }
unset LAMP_CONTROL_TOKEN LAMP_NS_NET
# A skill's run (#6510): started by lamp-tasks through Harness in /var/lib/lampos-tasks/harnesses/lamp-tasks/skill-<run-id>,
# it gets its one-run token from /run/lampos-tasks/runs/<run-id>, read once and deleted, in its environment only.
P=$(pwd -P 2>/dev/null) || P=
R=
case $P in /var/lib/lampos-tasks/harnesses/lamp-tasks/skill-?*) R=${P#/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-} ;; esac
case $R in *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) R= ;; esac
[ ${#R} -le 64 ] || R=
if [ -n "$R" ]; then
  F=/run/lampos-tasks/runs/$R
  C=
  if [ -f "$F" ] && [ ! -L "$F" ]; then C=$(cat -- "$F") || C=; fi
  rm -f -- "$F" || { echo "${0##*/}: the token file of run $R is not deleted" >&2; exit 1; }
  [ -n "$C" ] || { echo "${0##*/}: no token for run $R" >&2; exit 1; }
  LAMP_CONTROL_TOKEN=$C LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock PYTHONPATH=/usr/local/lib/lampos/sdk-python LAMP_NS_NET=run
  unset C
  export LAMP_CONTROL_TOKEN LAMP_CONTROL_SOCKET PYTHONPATH LAMP_NS_NET
fi
agent_env
[ -z "$R" ] || agent_run_env
N=
[ "${LAMP_NS_NET-}" != run ] || N=--net
if [ -n "$X" ]; then
  r=$(readlink -f "$X") && [ -f "$r" ] && [ -x "$r" ] || fail "no program at $X"
  X=$r
else
  I=$(readlink -f "$0") || fail "this wrapper is not found"
  o=$IFS
  set -f
  IFS=:
  for d in ${PATH-}; do
    IFS=$o
    case $d in /*) ;; *) continue ;; esac
    [ -f "$d/$E" ] && [ -x "$d/$E" ] || continue
    r=$(readlink -f "$d/$E") || continue
    [ "$r" != "$I" ] || continue
    head -c 512 "$r" | grep -qF "Written by lamp-update." && continue
    X=$r
    break
  done
  IFS=$o
  set +f
  [ -n "$X" ] || fail "no $E on PATH besides lamp-update's wrappers"
fi
logger -t "$E" "Harness hooks aren't available inside the lamp's agent namespace yet (openharness#4194)" 2>/dev/null || :
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
LAMP_NS_ENGINE=$E LAMP_NS_ENGINE_BIN=$X LAMP_NS_HOME=$H
export LAMP_NS_UID LAMP_NS_GID LAMP_NS_ENGINE LAMP_NS_ENGINE_BIN LAMP_NS_HOME
agent_start() {
  exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/agent-ns "$@"
}
# A skill's run lamp-tasks starts with no Harness (#6685): `--lamp-task` alone, in the run's own folder. The agent's data
# (agent_task) turns the folder's task file into the agent's own arguments and starts it with agent_start.
if [ "${1-}" = --lamp-task ]; then
  [ -n "$R" ] && [ $# -eq 1 ] || fail "--lamp-task is only for a skill's run, started by lamp-tasks in its own folder"
  case $(command -V agent_task 2>/dev/null) in *function*) ;; *) fail "it takes no skill's task without Harness yet" ;; esac
  Q=$P/task
  [ -f "$Q" ] && [ ! -L "$Q" ] || fail "no task at $Q"
  agent_task "$Q"
  fail "its data started no task"
fi
agent_start "$@"
