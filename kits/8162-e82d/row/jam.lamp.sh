# lamp, root sh (common first; then LEG= MODE=). Legs L3/L4: the kept OpenClaw pane's ONE openclaw pid (panepid: the
# pane's own pid tree, never pkill or a name pattern), the gateway left as it is (its pid is checked unchanged).
#  MODE=kill  (L3) SIGKILL of that pid
#  MODE=stop  (L4) SIGSTOP of that pid; the pid is kept in $STOPPED, and unfreeze.lamp.sh / nostop.lamp.sh SIGCONT it
# Prints JAM_PID, JAM_AT <epoch>, KILL|STOP <pid>, GW_SAME yes|no; or JAM REFUSED <why> (nothing done).
cd /
set -- $(kept); A=$1
[ "$A" != none ] || { echo "JAM REFUSED: no kept session recorded"; exit 3; }
GW0=$(VP openclaw-gateway)
PP=$(panepid $A | tee /dev/stderr | sed -n 's/^PANEPID \([0-9][0-9]*\)$/\1/p')
[ -n "$PP" ] || { echo "JAM REFUSED: no single pane openclaw pid"; exit 4; }
echo "JAM_PID $PP $(comm_of $PP) $(args_of $PP)"
case ${MODE:-} in
kill) echo "JAM_AT $(date +%s.%N)"; kill -KILL $PP && echo "KILL $PP"; n "${LEG:-?}: SIGKILL pane openclaw pid $PP";;
stop) echo $PP >> $STOPPED; echo "JAM_AT $(date +%s.%N)"; kill -STOP $PP && echo "STOP $PP"; n "${LEG:-?}: SIGSTOP pane openclaw pid $PP (kept in $STOPPED)";;
*) echo "JAM REFUSED: MODE must be kill or stop"; exit 2;;
esac
sleep 2; GW1=$(VP openclaw-gateway)
[ "$GW0" = "$GW1" ] && echo "GW_SAME yes ($GW1)" || echo "GW_SAME no ($GW0 -> $GW1)"
