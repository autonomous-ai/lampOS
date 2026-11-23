# lamp, root sh (row common first). Leg H: ONE `systemctl restart lamp-tasks` while a run step's script runs (the Mac
# GATEs it), then reads the cgroup back: serve's new pid in `serve`, the controllers handed down again, the old run-*
# leaf gone (systemd's stop, or set_up's stale cleanup), and the journal's own-cgroup set-up line since the restart.
set -u
CGU=$(cgunit); ROOT=/sys/fs/cgroup$CGU; p0=$(pid lamp-tasks)
echo "BEFORE pid=$p0 kids=$(ls -1d $ROOT/*/ 2>/dev/null | xargs -n1 basename 2>/dev/null | tr '\n' ' ')"
CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
systemctl restart lamp-tasks; n "leg H: lamp-tasks restarted while a run step ran"
s=0; while [ $s -lt 40 ]; do sleep 1; s=$((s + 1))
  journalctl -u lamp-tasks --after-cursor "$CUR" --no-pager -o cat 2>/dev/null | grep -qE "each run gets its own cgroup under|runs have no own cgroup" && break; done
sleep 2; p=$(pid lamp-tasks); c=$(cgof $p)
sp=$(cat $ROOT/serve/cgroup.procs 2>/dev/null | tr '\n' ' '); rp=$(cat $ROOT/cgroup.procs 2>/dev/null | tr '\n' ' ')
echo "AFTER ${s}s pid=$p cgroup=$c leaf=${c#"$CGU"/} subtree=$(cat $ROOT/cgroup.subtree_control 2>/dev/null | tr ' ' ,) kids=$(ls -1d $ROOT/*/ 2>/dev/null | xargs -n1 basename 2>/dev/null | tr '\n' ' ')"
echo "AFTER serve_procs=[${sp}] root_procs=[${rp}] mainpid_in_serve=$(echo " $sp" | grep -q " $p " && echo yes || echo no)"
journalctl -u lamp-tasks --after-cursor "$CUR" --no-pager -o cat 2>/dev/null | grep -E "own cgroup|not removed|refused, no own|could not be set" | cut -c1-220 | sed 's/^/J /'
