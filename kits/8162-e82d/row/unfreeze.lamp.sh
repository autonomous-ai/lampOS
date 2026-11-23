# lamp, root sh (common first). Leg L4's frozen pid(s), from $STOPPED only, SIGCONTed by number when still there and
# stopped. Prints UNFREEZE CONT <pid> | UNFREEZE <pid> gone or running, then UNFREEZE OK.
cd /
[ -s $STOPPED ] || { echo "UNFREEZE nothing"; echo "UNFREEZE OK"; exit 0; }
for p in $(sort -u $STOPPED); do
  if [ -d /proc/$p ] && [ "$(awk '{print $3}' /proc/$p/stat)" = T ]; then kill -CONT $p; n "SIGCONT pid $p"; echo "UNFREEZE CONT $p at $(date +%s.%N)"
  else echo "UNFREEZE $p already gone or running"; fi; done
: > $STOPPED; echo "UNFREEZE OK"
