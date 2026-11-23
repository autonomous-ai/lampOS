# lamp, root sh (common first; then LEG=). Leg L5: Harness's daemon started again the way lamp-tasks does it,
# `harness start --restart` as lampos with lamp-tasks' HOME (harness.rs Cli::restart_daemon). 1 start (harnessd, a
# user unit). Prints HDR_AT <epoch> and HD_BACK.
cd /
U=$(id -u lampos)
HDS(){ runuser -u lampos -- env XDG_RUNTIME_DIR=/run/user/$U DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$U/bus systemctl --user "$@" harnessd.service 2>&1; }
echo "HD_BEFORE pid=$(HDS show -p MainPID --value | tail -1) $(HDS is-active | tail -1)"
echo "HDR_AT $(date +%s.%N)"
hcli start --restart 2>&1 | tail -2 | cut -c1-160
n "${LEG:-L5}: Harness's daemon started again (harness start --restart as lampos)"
i=0; until [ "$(HDS is-active | tail -1)" = active ] || [ $i -ge 60 ]; do sleep 1; i=$((i+1)); done
echo "HD_BACK $(date +%s.%N) $(HDS is-active | tail -1) pid=$(HDS show -p MainPID --value | tail -1)"
