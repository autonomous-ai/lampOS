# lamp, root sh (common first; then LEG=). READ ONLY: the kept session as lamp-tasks records it, its pane's pid, the
# gateway's pid. Prints KEPT_AGENT, PANEPID, GW_PID.
cd /
set -- $(kept); echo "KEPT_AGENT $1"; echo "KEPT_MACHINE $2"
[ "$1" != none ] && panepid $1
echo "GW_PID $(VP openclaw-gateway) $(systemctl is-active lamp-openclaw-gateway)"
echo "TASKS_PID $(VP tasks) $(systemctl is-active lamp-tasks)"
