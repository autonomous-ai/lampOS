# lamp, root sh (common first; then LEG= GWPID=). Leg L7, READ ONLY, 30 s after ready: is lamp-tasks' merge (written at
# its start, after the gateway's restart) live in the gateway WITHOUT another restart?
#  - the gateway's pid is the one the install's restart left; openclaw.json holds the three keys again;
#  - the gateway's journal since lamp-tasks' start: "[reload] config change detected; evaluating reload (...plugins...)"
#    then "hot reload applied", never "config restart";
#  - `openclaw gateway call models.list` names lampos; `config.get` printed as a second sign only.
# Prints L7_CONFIG LIVE | L7_CONFIG NOT LIVE <why>.
cd /
why=""
p=$(VP openclaw-gateway); echo "GW_PID now=$p install-left=${GWPID:-?}"
[ "$p" = "${GWPID:-x}" ] || why="$why gateway-restarted-again"
python3 -I - $OCJ <<'P'
import json, sys
d = json.load(open(sys.argv[1]))
def g(*k):
    o = d
    for x in k: o = o.get(x) if isinstance(o, dict) else None
    return o
print("FILE lampos=%s primary=%s plugins.allow=%s" % ("yes" if g("models", "providers", "lampos") else "no", g("agents", "defaults", "model", "primary"), g("plugins", "allow")))
P
grep -q '"lampos"' $OCJ || why="$why file-without-lampos"
TSA=$(systemctl show -p ActiveEnterTimestamp --value lamp-tasks)
R1=$(journalctl -u lamp-openclaw-gateway --since "$TSA" --no-pager -o cat 2>/dev/null | grep -E "\[reload\]|config restart|restart-pending")
echo "$R1" | tail -8 | cut -c1-220 | sed 's/^/J /'
echo "$R1" | grep -q "config change detected" || why="$why no-reload-seen"
echo "$R1" | grep -q "hot reload applied" || why="$why no-hot-reload-applied"
echo "$R1" | grep -qE "evaluating reload \(.*plugins" || why="$why reload-not-naming-plugins"
echo "$R1" | grep -qiE "config restart|restart-pending" && why="$why gateway-asked-a-restart"
M=$(ocli gateway call models.list --json --timeout 10000 2>&1 | tr '\n' ' '); echo "MODELS_LIST $(echo "$M" | cut -c1-200)"
echo "$M" | grep -q 'lampos' || why="$why models.list-without-lampos"
G=$(ocli gateway call config.get --json --timeout 10000 2>&1 | tr '\n' ' '); echo "CONFIG_GET lampos=$(echo "$G" | grep -c '"lampos"') (a second sign only)"
[ -z "$why" ] && echo "L7_CONFIG LIVE" || echo "L7_CONFIG NOT LIVE:$why"
