# lamp, root sh: put-back guard. Any process in state T (stopped) owned by the Harness or OpenClaw users is resumed by
# its own pid (never by pattern), then the check runs again; prints NOSTOP OK or NOSTOP FAIL <pids>.
cd /
L=""; for p in $(ps -eo pid=,stat=,user= | awk '$2 ~ /^T/ && ($3=="lampos" || $3=="lampos-tasks" || $3 ~ /openclaw/) {print $1}'); do
  echo "STOPPED pid $p $(cat /proc/$p/comm 2>/dev/null)"; kill -CONT $p && n "put-back: SIGCONT stopped pid $p"; L="$L $p"; done
sleep 1
R2=$(ps -eo pid=,stat=,user= | awk '$2 ~ /^T/ && ($3=="lampos" || $3=="lampos-tasks" || $3 ~ /openclaw/) {print $1}' | tr '\n' ' ')
[ -z "$R2" ] && echo "NOSTOP OK (resumed:${L:- none})" || echo "NOSTOP FAIL $R2"
