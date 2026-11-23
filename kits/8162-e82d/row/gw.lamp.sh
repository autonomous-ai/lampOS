# lamp, root sh (common first; then LEG=). Legs L1/L2 (and L7's fallback): OpenClaw's gateway restarted (1 start), as an
# OpenClaw install or update does; then `health` asked every 1 s (lamp-update's HEALTH_CALL), 180 s at most.
# Prints GW_PID_BEFORE, GWR_AT <epoch> (just before the restart), GW_ACTIVE, GW_HEALTH <epoch> | GW_HEALTH none.
cd /
p0=$(VP openclaw-gateway); echo "GW_PID_BEFORE $p0"
echo "GWR_AT $(date +%s.%N)"
systemctl restart lamp-openclaw-gateway; n "${LEG:-?}: lamp-openclaw-gateway restarted (was pid $p0)"
echo "GW_ACTIVE $(date +%s.%N) $(systemctl is-active lamp-openclaw-gateway) pid=$(VP openclaw-gateway)"
s=$(date +%s); while [ $(( $(date +%s) - s )) -lt 180 ]; do gw_health && { echo "GW_HEALTH $(date +%s.%N)"; exit 0; }; sleep 1; done
echo "GW_HEALTH none within 180 s"
