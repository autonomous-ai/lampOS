# lamp, root sh (row common first, then SECS=<at most> STOPF=<a file whose existence ends it>). READ ONLY. Leg G (#8180):
# started by row.sh in the background around B's PRICEASK run. Reads lamp-tasks' delegated cgroup from outside:
#   CGROOT <dir> subtree=<controllers>               the unit's cgroup and what it hands down
#   SERVELEAF pid=<MainPID> path=<its cgroup> leaf=<name>   serve's own leaf (serve or main), at start and at the end
#   RUNLEAF +run-<id> at <t> memory.max=.. memory.high=.. memory.swap.max=.. cpu.max=.. pids.max=.. procs=<n> [comms]
#   RUNLEAF -run-<id> gone at <t>
#   CGEND left=<run-* still there>
CGU=$(cgunit); ROOT=${CGFS:-/sys/fs/cgroup}$CGU; end=$(( $(date +%s) + ${SECS:-300} ))
sl(){ p=$(pid lamp-tasks); c=$(cgof $p); l=${c#"$CGU"/}; [ "$c" = "$CGU" ] && l=unit-root; echo "SERVELEAF pid=$p path=$c leaf=$l"; }
echo "CGROOT $ROOT subtree=$(cat $ROOT/cgroup.subtree_control 2>/dev/null | tr ' ' ,) kids=$(ls -1d $ROOT/*/ 2>/dev/null | xargs -n1 basename 2>/dev/null | tr '\n' ' ')"
sl
rd(){ if [ -e "$1" ]; then tr ' ' _ < "$1" 2>/dev/null; else echo absent; fi; }
seen=" "; gone=" "; pend=" "
while [ $(date +%s) -lt $end ]; do
  [ -n "${STOPF:-}" ] && [ -e "$STOPF" ] && { rm -f "$STOPF"; break; }
  for d in $ROOT/run-*; do [ -d "$d" ] || continue; nm=${d##*/}
    case "$seen" in *" $nm "*) continue;; esac
    # The run's first child joins only after every limit is written (run_cgroup.rs join_at_exec): read the limits once
    # a process is in it, so a leaf caught between mkdir and its writes is not read as uncapped.
    pr=$(cat $d/cgroup.procs 2>/dev/null | tr '\n' ' ')
    if [ -z "$pr" ]; then case "$pend" in *" $nm "*) ;; *) pend="$pend$nm "; echo "RUNLEAF ~$nm made at $(date +%T.%N | cut -c1-12), no process yet";; esac; continue; fi
    seen="$seen$nm "
    m=$(rd $d/memory.max); [ -e $d/memory.swap.max ] && sw=$(rd $d/memory.swap.max) || sw=absent
    cm=""; for q in $pr; do cm="$cm$(cat /proc/$q/comm 2>/dev/null),"; done
    echo "RUNLEAF +$nm at $(date +%T.%N | cut -c1-12) memory.max=$m memory.high=$(rd $d/memory.high) memory.swap.max=$sw cpu.max=$(rd $d/cpu.max) pids.max=$(rd $d/pids.max) procs=$(echo $pr | wc -w) [${cm%,}]"
  done
  # each live leaf's pids.events (its "max" count: forks refused at pids.max), kept for its gone line
  for nm in $seen; do [ -d $ROOT/$nm ] && cat $ROOT/$nm/pids.events > $R/cgpe.$nm 2>/dev/null; done
  for nm in $seen $pend; do [ -d $ROOT/$nm ] && continue
    case "$gone" in *" $nm "*) ;; *) gone="$gone$nm "; echo "RUNLEAF -$nm gone at $(date +%T.%N | cut -c1-12) pids.events.max=$(sed -n 's/^max //p' $R/cgpe.$nm 2>/dev/null)";; esac; done
  sleep 0.1
done
sleep 2; for nm in $seen $pend; do case "$gone" in *" $nm "*) ;; *) [ -d $ROOT/$nm ] || echo "RUNLEAF -$nm gone at $(date +%T.%N | cut -c1-12) pids.events.max=$(sed -n 's/^max //p' $R/cgpe.$nm 2>/dev/null)";; esac; done
rm -f $R/cgpe.*
sl
echo "CGEND left=$(ls -1d $ROOT/run-* 2>/dev/null | xargs -n1 basename 2>/dev/null | tr '\n' ' ') kids=$(ls -1d $ROOT/*/ 2>/dev/null | xargs -n1 basename 2>/dev/null | tr '\n' ' ') subtree=$(cat $ROOT/cgroup.subtree_control 2>/dev/null | tr " " ,)"
