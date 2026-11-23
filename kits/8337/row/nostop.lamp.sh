# lamp, root sh (common first). Put-back guard: the kit's own stopped pids ($STOPPED), then any process left in state T
# (stopped) owned by lampos or OpenClaw's user, each resumed by its own number (never by pattern); the check again.
# Prints NOSTOP OK (resumed: ...) | NOSTOP FAIL <pids>.
cd /
L=""; for p in $(cat $STOPPED 2>/dev/null) $(stopped_lampos); do
  [ -d /proc/$p ] && [ "$(awk '{print $3}' /proc/$p/stat 2>/dev/null)" = T ] || continue
  echo "STOPPED pid $p $(comm_of $p)"; kill -CONT $p && n "put-back: SIGCONT stopped pid $p"; L="$L $p"; done
: > $STOPPED; sleep 1
R2=$(stopped_lampos)
[ -z "$R2" ] && echo "NOSTOP OK (resumed:${L:- none})" || echo "NOSTOP FAIL $R2"
