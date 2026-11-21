# lamp, root sh (row common first, then MODE=read|on|off|task, SINCE=<task id> CLIP=<id> for task). Leg N (#7269,
# #7394 merged dark as 4f696c2946): LAMP_NAMED_AGENT_TAKES_LINE=on for lamp-voice by a /run drop-in (gone at reboot;
# never /etc), and back off. lamp-voice reads it from its own environment (agent_tasks.rs:1497
# named_agent_takes_line_on, exactly "on") and passes it to the second route's worker alone (process.rs:1740/2851),
# which journals at its start (second_provider/worker.rs:4607):
#   "lamp-voice second: a spoken line naming an agent for the work is handed to it in code as round 1, before any own tool (LAMP_NAMED_AGENT_TAKES_LINE)"
# on/off restart lamp-voice once each (row.sh GATEs it first); read and task change nothing. Never touches volume.
# Prints NAMED ON / NAMED NOT ON / NAMED OFF / NAMED STILL, an ENGINE line, and the mute record (MUTE).
set -u
SW=LAMP_NAMED_AGENT_TAKES_LINE
WL="a spoken line naming an agent for the work is handed to it in code as round 1, before any own tool ($SW)"
TQ(){ python3 -I - "$1" <<'P'
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect("/run/lampos-tasks/request.sock"); s.sendall((sys.argv[1] + "\n").encode()); b = b""
while not b.endswith(b"\n"):
    c = s.recv(65536)
    if not c: break
    b += c
print(b.decode().strip())
P
}
# The lamp target's engine and engines, as the tasks part answers them (agent_tasks.rs lamp_engines), and the file.
eng(){ TQ '{"op":"targets"}' 2>/dev/null | python3 -I -c 'import json,sys
try: d = json.loads(sys.stdin.read() or "{}")
except Exception: d = {}
t = [x for x in d.get("targets", []) if x.get("alias") == "lamp"]
t = t[0] if t else {}
print("ENGINE default=%s engines=%s" % (t.get("engine", "?"), ",".join(str(e) for e in t.get("engines", [])) or "?"))'
  echo "ENGINEFILE $(tr -d '\n' < $EF 2>/dev/null || echo absent) engine=$(engine)"; }
mute(){ python3 -I -c 'import json,sys
try: r = json.load(open(sys.argv[1])); print("MUTE muted=%s" % r.get("muted"))
except Exception as e: print("MUTE unread (%s)" % e)' /var/lib/lampOS/voice/mute/record.json; }
# The newest task id in lamp-tasks' store (read only), or each task above SINCE: id, engine, state, prompt's words.
tasks(){ python3 -I - "${SINCE:--1}" "${CLIP:-}" <<'P'
import json, sqlite3, sys
since, clip = int(sys.argv[1]), sys.argv[2]
try:
    c = sqlite3.connect("file:/var/lib/lampos-tasks/store/tasks.sqlite3?mode=ro", uri=True)
    d = json.loads(c.execute("select snapshot from state").fetchone()[0])
except Exception as e: print("TASKS unread (%s)" % str(e)[:120]); sys.exit(0)
ts = d.get("tasks", [])
if since < 0: print("TASKMAX %d" % max([t.get("id", 0) for t in ts] or [0])); sys.exit(0)
new = [t for t in ts if t.get("id", 0) > since]
for t in new:
    l = t.get("launch") or {}; p = str(l.get("prompt") or "").lower()
    print("TASK %s id=%s engine=%s state=%s alias=%s weather=%s words=%d" % (clip or "-", t.get("id"), (l.get("target") or {}).get("engine"),
        (t.get("dispatch") or {}).get("state"), t.get("target_alias"), "yes" if "weather" in p else "no", len(p.split())))
print("TASKS %s new=%d above %d" % (clip or "-", len(new), since))
P
}
v(){ envval $(pid lamp-voice) $SW; }
case "${MODE:-read}" in
read) echo "NAMED env='$(v)' dropin=$([ -f $ND_NAMED ] && echo present || echo absent)"; eng; mute; tasks;;
on|off)
  if [ "$MODE" = on ]; then mkdir -p ${ND_NAMED%/*}
    printf '# #8180 row leg N (#7269): /run, removed by named off, by unrow and by the EXIT traps\n[Service]\nEnvironment=%s=on\n' $SW > $ND_NAMED; n "added $ND_NAMED"
  else [ -f $ND_NAMED ] && { rm -f $ND_NAMED; rmdir ${ND_NAMED%/*} 2>/dev/null; n "removed $ND_NAMED"; }; fi
  t0=$(date +%s); systemctl daemon-reload; systemctl restart lamp-voice; n "restarted lamp-voice (named $MODE)"; sleep 20
  val=$(v); w=$(journalctl -u lamp-voice --since "@$t0" -o cat --no-pager 2>/dev/null | grep -cF "$WL")
  echo "NAMED env='$val' dropin=$([ -f $ND_NAMED ] && echo present || echo absent) worker_line_since_restart=$w"; eng; mute
  if [ "$MODE" = on ]; then
    [ "$val" = on ] && [ -f $ND_NAMED ] && { echo "NAMED ON"; [ "$w" -gt 0 ] || echo "NOTE no worker start line '$WL' since the restart (second route not started yet?)"; } || echo "NAMED NOT ON ('$val')"
  else [ -z "$val" ] && [ ! -f $ND_NAMED ] && echo "NAMED OFF" || echo "NAMED STILL '$val' (drop-in $([ -f $ND_NAMED ] && echo present || echo absent))"; fi;;
task) tasks;;
esac
