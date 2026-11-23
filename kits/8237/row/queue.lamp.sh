# BODY queue
# From Twelve's #8180 kit (kit-8180-ee17), the manager's 23 Nov 17:17 ask.
# lamp, root sh (row common first). Before the legs: is lamp-tasks' one-session queue free for the row's makes?
# A kept session lamp-tasks can't close (its "the kept session <id> not closed: working; tried again next" loop, as
# #8237's run 6 on lamp-ee17 15667 met, 23 Nov 17:07-17:16: 116 tries on the stable unit's just-resumed session) queues every
# make "behind the queue" until it times out: the legs would be VOID for the lamp's state, not for #8237.
# Such a session is closed with Harness's own `close --now`, as lamp-tasks' user, by the program token lamp-tasks
# keeps; the step is written in changes.txt and the journal. Prints QUEUE CLEAR | QUEUE CLOSED <id> | QUEUE STUCK <why>.
cd /
LOOPN=${QUEUE_LOOPN:-3}; WIN=${QUEUE_WIN:-20}; SETTLE=${QUEUE_SETTLE:-15}
loop(){ journalctl -u lamp-tasks --since "-${1}s" --no-pager -o cat 2>/dev/null | grep -E 'one session: the kept session [0-9a-f]+ not closed: working'; }
L=$(loop $WIN); c=$(printf '%s' "$L" | grep -c . )
[ "$c" -lt "$LOOPN" ] && { echo "QUEUE CLEAR (close loop lines in ${WIN}s: $c)"; exit 0; }
id=$(printf '%s\n' "$L" | tail -1 | sed -E 's/.*kept session ([0-9a-f]+) not closed.*/\1/')
ST=/var/lib/lampos-tasks/store; T=$ST/kept-session.token
name=$(python3 -I - "$ST/one-session.json" "$id" <<'P'
import json, sys
try: d = json.load(open(sys.argv[1]))
except Exception: sys.exit(0)
found = []
def walk(v):
    if isinstance(v, dict):
        for x in v.values(): walk(x)
    elif isinstance(v, list):
        for x in v: walk(x)
    elif isinstance(v, str) and v.startswith("lamp-one-"): found.append(v)
walk(d); print(found[0] if len(set(found)) == 1 else "")
P
)
# one-session.json may hold no name (ee17 15667, 23 Nov: agent/engine/session only): Harness's own list names it by id.
[ -n "$name" ] || name=$(cd /var/lib/lampos-tasks && sudo -u lampos env HOME=/var/lib/lampos-tasks PATH=/usr/local/bin:/usr/bin:/bin timeout 30 harness ps --json 2>/dev/null |
  python3 -I -c 'import json,sys
i=sys.argv[1]
try: h=json.load(sys.stdin).get("harnesses",[])
except Exception: h=[]
m=[x.get("name") for x in h if str(x.get("id","")).startswith(i) and str(x.get("name","")).startswith("lamp-one-")]
print(m[0] if len(m)==1 else "")' "$id")
echo "QUEUE loop: $c lines in ${WIN}s for kept session $id (name ${name:-unknown})"
[ -n "$name" ] || { echo "QUEUE STUCK no-single-session-name-in-one-session.json"; exit 3; }
[ -f $T ] || { echo "QUEUE STUCK no-program-token-file"; exit 3; }
CMD="harness close $name --program-token-file $T --now --json"
n "the kept session $id ($name) was in lamp-tasks' close loop before the legs: $CMD"
logger -t row-8237 "${LOCK_WHO:-row}: $CMD (kept session $id in a close loop before the legs)"
out=$(cd /var/lib/lampos-tasks && sudo -u lampos env HOME=/var/lib/lampos-tasks PATH=/usr/local/bin:/usr/bin:/bin timeout 60 $CMD 2>&1); rc=$?
echo "QUEUE close rc=$rc: $(printf '%s' "$out" | tr '\n' ' ' | cut -c1-300)"; n "close rc=$rc"
sleep $SETTLE
a=$(loop 12 | grep -c .)
[ "$rc" = 0 ] && [ "$a" = 0 ] && { echo "QUEUE CLOSED $id ($name): no close loop line in the 12 s after"; exit 0; }
echo "QUEUE STUCK close rc=$rc, loop lines after: $a"; exit 3
