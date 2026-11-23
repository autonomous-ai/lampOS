# lamp, root sh (common.lamp.sh first, then SECS=<at most how long> STOPF=<a file whose existence ends it>). READ ONLY. Row (f), started in the
# background by row.sh at the start of each make leg (A, B, D): watches for the make's TRIED run (a
# python3 running .../scripts/*.py under lamp-tasks) and, once per pid, looks into its fence from outside:
#   TRYPID <pid> <cmdline>
#   ADMIN <pid> admin_sock=visible|hidden ctl_dir=[<ls of /run/lampos-control as the fence sees it>]
#   NET <pid> links=<interfaces in its net namespace> sockfds=<n> fd5=<target or ->
# and, every tick, any new egress socket or connection to one:
#   EGRESS +<name> (a socket appeared in /run/lampos-tasks/egress)   EGRESSCONN <n> (unix connections to them)
EG=/run/lampos-tasks/egress; seen=" "; end=$(( $(date +%s) + ${SECS:-600} ))
ls -1 $EG 2>/dev/null | LC_ALL=C sort > $R/egress.ls.0; echo "EGRESS-AT-START $(tr '\n' ' ' < $R/egress.ls.0)"
TP=$(pid lamp-tasks)
while [ $(date +%s) -lt $end ]; do
  [ -n "${STOPF:-}" ] && [ -e "$STOPF" ] && { rm -f "$STOPF"; break; }
  for p in $(pgrep -f 'python3 .*/scripts/[^ ]*\.py' 2>/dev/null); do
    case "$seen" in *" $p "*) continue;; esac; seen="$seen$p "
    # under lamp-tasks? (its pid among the ancestors)
    a=$p; under=no; i=0; while [ "$a" -gt 1 ] 2>/dev/null && [ $i -lt 12 ]; do [ "$a" = "$TP" ] && { under=yes; break; }; a=$(awk '{print $4}' /proc/$a/stat 2>/dev/null || echo 1); i=$((i+1)); done
    echo "TRYPID $p under_lamp_tasks=$under $(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null | cut -c1-200)"
    if [ -e /proc/$p/root/run/lampos-control/admin.sock ] || [ -S /proc/$p/root/run/lampos-control/admin.sock ]; then av=visible; else av=hidden; fi
    echo "ADMIN $p admin_sock=$av ctl_dir=[$(ls -1A /proc/$p/root/run/lampos-control 2>&1 | tr '\n' ' ')]"
    links=$(awk -F: 'NR>2 {gsub(/ /,"",$1); printf "%s,", $1}' /proc/$p/net/dev 2>/dev/null)
    sf=$(ls -l /proc/$p/fd 2>/dev/null | grep -c 'socket:'); f5=$(readlink /proc/$p/fd/5 2>/dev/null || echo -)
    echo "NET $p links=${links:-?} sockfds=$sf fd5=$f5"
  done
  ls -1 $EG 2>/dev/null | LC_ALL=C sort > $R/egress.ls.1
  LC_ALL=C comm -13 $R/egress.ls.0 $R/egress.ls.1 | sed "s/^/EGRESS +/; s/\$/ at $(date +%T)/"; cp $R/egress.ls.1 $R/egress.ls.0
  c=$(ss -xp 2>/dev/null | grep -c "$EG/"); [ "${c:-0}" -gt 0 ] && echo "EGRESSCONN $c at $(date +%T)"
  sleep 0.2
done
echo "FENCE-WATCH END $(date +%T)"
