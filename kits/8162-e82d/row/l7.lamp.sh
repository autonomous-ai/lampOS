# lamp, root sh (common first; then LEG= MODE=go|cut). Legs L7 and L8: OpenClaw's install (repair) by the lamp's own
# lamp-update on the PR build. Its changes, all put back by the install itself or by l7back.lamp.sh:
#  1. (MODE=go only) openclaw.json: the keys lamp-tasks merges at its start taken out (models.providers.lampos,
#     agents.defaults.model.primary, plugins.allow), as lampos: the first-install shape; a copy at $R/openclaw.json.before;
#  2. the wrapper /usr/local/bin/openclaw given one comment line at its end (it still runs), so openclaw_install's
#     present() is false and the round asks the one-shot; a copy at $R/openclaw.wrapper.before;
#  3. lamp-update.service restarted (1 start): its round runs after_round, which starts Harness's one-shot
#     (lamp-update-harness), which stops lamp-tasks, restarts the gateway (1), waits for health, starts lamp-tasks (1).
#  MODE=cut (L8): the moment lamp-update journals "lamp-tasks.service stopped while lamp-openclaw-gateway.service
#     restarts", `systemctl stop lamp-update-harness.service` (the install cut off between lamp-tasks' stop and start).
# Prints L7_AT, then (go) L7_ORDER PASS|FAIL with ORDER lines and L7_GW_HEALTH_S, GW_PID_AFTER; (cut) CUT_AT and GAP
# lines; or L7 STOP <why> before any change.
cd /
MAXS=${MAXS:-900}
s0=$(agent_now); echo "$s0"; echo "$s0" | grep -q 'state=ready ' || { echo "L7 STOP: the agent is not ready before the leg"; exit 3; }
[ -e $REC/openclaw-ask.json ] && { echo "L7 STOP: an OpenClaw ask is already there"; exit 3; }
cp -p $OCJ $R/openclaw.json.before && cp -p $OCW $R/openclaw.wrapper.before || { echo "L7 STOP: copies not made"; exit 3; }
if [ "${MODE:-go}" = go ]; then
runuser -u lampos -- python3 -I - $OCJ <<'P'
import json, os, sys
p = sys.argv[1]; d = json.load(open(p)); had = []
def drop(path):
    o = d
    for k in path[:-1]:
        o = o.get(k) if isinstance(o, dict) else None
        if o is None: return
    if isinstance(o, dict) and path[-1] in o: o.pop(path[-1]); had.append(".".join(path))
for path in (["models", "providers", "lampos"], ["agents", "defaults", "model", "primary"], ["plugins", "allow"]): drop(path)
t = p + ".t8162"; open(t, "w").write(json.dumps(d, indent=2) + "\n"); os.chmod(t, 0o600); os.replace(t, p)
print("STRIPPED " + (" ".join(had) or "nothing (keys were absent)"))
P
n "${LEG:-L7}: openclaw.json: lamp-tasks' merged keys taken out (first-install shape); copy $R/openclaw.json.before"
fi
printf '# t8162 (#8162 ROW %s): a line that makes the install write this wrapper again\n' "${LEG:-L7}" >> $OCW
n "${LEG:-L7}: $OCW given one comment line (present() false); copy $R/openclaw.wrapper.before"
CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p'); echo "$CUR" > $R/cursor-$LEG
echo "L7_AT $(date +%s.%N)"
systemctl restart lamp-update.service; n "${LEG:-L7}: lamp-update.service restarted to ask OpenClaw's repair"
if [ "${MODE:-go}" = cut ]; then
  s=0; while [ $s -lt $((MAXS * 2)) ]; do
    journalctl --no-pager -o cat -u lamp-update.service -u lamp-update-harness.service --after-cursor "$CUR" 2>/dev/null | grep -q "lamp-tasks.service stopped while lamp-openclaw-gateway.service restarts" && break
    sleep 0.5; s=$((s + 1)); done
  [ $s -ge $((MAXS * 2)) ] && { echo "L8 STOP: no 'lamp-tasks.service stopped while' line in $MAXS s"; exit 3; }
  systemctl stop lamp-update-harness.service; echo "CUT_AT $(date +%s.%N)"
  n "L8: lamp-update-harness.service stopped between lamp-tasks' stop and its start (the cut-off install)"
  echo "GAP lamp-tasks=$(systemctl is-active lamp-tasks) oneshot=$(systemctl is-active lamp-update-harness) record=$([ -e $REC/openclaw-tasks-stopped ] && echo yes || echo no)"
  agent_now | sed 's/^/GAP /'
  exit 0
fi
END="OpenClaw runs: |OpenClaw is not installed: |nothing installed|OpenClaw's install waits|not set up yet: it waits"
s=0; while [ $s -lt $MAXS ]; do
  J=$(journalctl --no-pager -o cat -u lamp-update.service -u lamp-update-harness.service --after-cursor "$CUR" 2>/dev/null)
  echo "$J" | grep -qE "$END" && echo "$J" | grep -qE "started again|not started again|OpenClaw is not installed|nothing installed|waits" && break
  sleep 3; s=$((s + 3)); done
sleep 5
echo "L7_INSTALL_END $(date +%s.%N) after ${s}s"
echo "GW_PID_AFTER $(VP openclaw-gateway) nrestarts=$(systemctl show -p NRestarts --value lamp-openclaw-gateway)"
python3 -I - $R/cursor-$LEG <<'P'
import re, subprocess, sys
cur = open(sys.argv[1]).read().strip()
def j(*a): return subprocess.run(["journalctl", "--no-pager", "-o", "short-unix", "--after-cursor", cur] + list(a), capture_output=True, text=True).stdout.splitlines()
def first(lines, pat):
    for l in lines:
        if re.search(pat, l): return float(l.split()[0])
upd = j("-u", "lamp-update-harness.service", "-u", "lamp-update.service"); sysd = j("_PID=1")
steps = [("tasks stop said", first(upd, r"lamp-tasks\.service stopped while lamp-openclaw-gateway\.service restarts")),
         ("lamp-tasks stopped", first(sysd, r"Stopped .*lamp-tasks")),
         ("gateway started", first(sysd, r"Started .*lamp-openclaw-gateway")),
         ("gateway health", first(upd, r"lamp-openclaw-gateway\.service answers health [0-9]+ s after its restart")),
         ("lamp-tasks started", first(sysd, r"Started .*lamp-tasks")),
         ("tasks start said", first(upd, r"lamp-tasks\.service started again"))]
for name, t in steps: print("ORDER %-18s %s" % (name, "%.3f" % t if t else "none"))
ts = [t for _, t in steps]
ok = all(ts) and all(a <= b for a, b in zip(ts, ts[1:]))
print("L7_ORDER %s%s" % ("PASS" if ok else "FAIL", "" if ok else " (a step missing or out of order)"))
m = [l for l in upd if re.search(r"answers health [0-9]+ s after its restart", l)]
if m: print("L7_GW_HEALTH_S %s" % re.search(r"answers health ([0-9]+) s", m[0]).group(1))
P
grep -q t8162 $OCW && echo "WRAPPER still has the kit's line (not rewritten)" || echo "WRAPPER rewritten by the install"
