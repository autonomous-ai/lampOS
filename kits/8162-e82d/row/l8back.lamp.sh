# lamp, root sh (common first; then LEG=). Leg L8, after lamp-tasks is back: the gap's end read from the journal, then
# lamp-update restarted once more (1 start) so its round finishes the repair the wrapper line still asks for; then
# health and the agent. If the gateway is left down, it is restarted once (1 start).
# Prints ONESHOT_END <epoch>, TASKS_BACK <epoch>, CUTBACK_LINES <n>, RECORD gone|left, then L8BACK OK | L8BACK FAIL <why>.
cd /
why=""
CUR=$(cat $R/cursor-$LEG 2>/dev/null)
python3 -I - "$CUR" <<'P'
import re, subprocess, sys
cur = sys.argv[1]
def j(*a): return subprocess.run(["journalctl", "--no-pager", "-o", "short-unix"] + (["--after-cursor", cur] if cur else []) + list(a), capture_output=True, text=True).stdout.splitlines()
def first(lines, pat):
    for l in lines:
        if re.search(pat, l): return float(l.split()[0])
sysd = j("_PID=1"); upd = j("-u", "lamp-update.service")
print("ONESHOT_END %s" % (first(sysd, r"(Stopped|Deactivated|Failed).*lamp-update-harness") or "none"))
print("TASKS_BACK %s" % (first(sysd, r"Started .*lamp-tasks") or "none"))
print("CUTBACK_LINES %d" % sum(1 for l in upd if "started again: OpenClaw's install ended while it was stopped for the gateway's restart (#8138)" in l))
P
[ -e $REC/openclaw-tasks-stopped ] && { echo "RECORD left"; why="$why record-left"; } || echo "RECORD gone"
systemctl is-active -q lamp-tasks || why="$why lamp-tasks-not-active"
CUR2=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
systemctl restart lamp-update.service; n "L8: lamp-update restarted once more to finish OpenClaw's repair"
s=0; while [ $s -lt 600 ]; do
  journalctl --no-pager -o cat -u lamp-update.service -u lamp-update-harness.service --after-cursor "$CUR2" 2>/dev/null | grep -qE "lamp-tasks.service started again|OpenClaw runs: |not started again" && break
  sleep 5; s=$((s + 5)); done
echo "REPAIR waited ${s}s"
i=0; until gw_health || [ $i -ge 12 ]; do sleep 5; i=$((i + 1)); done
if gw_health; then echo "GW health ok"; else echo "GW health none: the gateway restarted once"; systemctl restart lamp-openclaw-gateway; n "L8: gateway restarted once (left down)"; sleep 20; gw_health || why="$why gateway-down"; fi
[ -z "$why" ] && echo "L8BACK OK" || echo "L8BACK FAIL:$why"
