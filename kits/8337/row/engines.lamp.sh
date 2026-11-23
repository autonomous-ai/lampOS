# lamp, root sh (common first; then N=0|1). READ ONLY. The agents list the app is shown: update/status's `engines`
# (docs/cloud.md "The lamp's agents"), as lamp-update keeps it on the bus (<root>/update/status/latest, a header line
# then the JSON payload), and lamp-update's last word on its Harness look since the round's mark (engines.rs: "the
# lamp's Harness said its engines" or "... did not say its engines, the last kept: <why>").
#   ENGINES<N> <the engines object, compact> | ENGINES<N> unreadable:<why>
#   HLOOK<N> said | not: <why> | none   (the look's line since round 1's mark; N=0: since lamp-update's start)
#   HAUTH ...                           (Harness's sign-in, a fact only)
cd /
python3 -I - ${N:-0} <<'P'
import json, sys
n = sys.argv[1]
try:
    h, _, p = open("/run/lampos-bus/update/status/latest", "rb").read().partition(b"\n"); d = json.loads(p)
    e = d.get("engines")
    print("ENGINES%s %s" % (n, json.dumps(e, sort_keys=True, separators=(",", ":")) if isinstance(e, dict) else "unreadable:no-engines"))
except Exception as x:
    print("ENGINES%s unreadable:%s" % (n, type(x).__name__))
P
if [ "${N:-0}" = 0 ]; then S=$(systemctl show -p ActiveEnterTimestamp --value lamp-update.service); A="--since=@$(date -d "$S" +%s 2>/dev/null || echo 0)"
else CUR=$(cat $R/cursor-round1 2>/dev/null); A=${CUR:+--after-cursor=$CUR}; fi
l=$(journalctl --no-pager -o cat -u lamp-update.service $A 2>/dev/null | grep -E "the lamp's Harness (said|did not say) its engines" | tail -1)
case "$l" in *"did not say"*) echo "HLOOK${N:-0} not: ${l#*the last kept: }" | cut -c1-200;; *"said its engines"*) echo "HLOOK${N:-0} said";; *) echo "HLOOK${N:-0} none";; esac
hauth
