# lamp, root sh (row common first). READ ONLY. Before the legs: is the lamp running #8180's build, with #8180's cgroup
# set up, and ready as a person waits after plugging it in? (The round kit's verify + ready, without the round: the
# runner installed the build by their own /run drop-in, so no sequence, channel or unit-file check is made here.)
# With PR_<PART>_SHA given (the artifact's sha256), the running exe of each of the five units must be exactly it.
# Prints VERIFY PASS | VERIFY FAIL <why>, then READY | NOT_READY <why>.
cd /
why=""
for p in tasks control skills voice update; do
  case $p in update) w=${PR_UPDATE_SHA:-};; tasks) w=${PR_TASKS_SHA:-};; control) w=${PR_CONTROL_SHA:-};; skills) w=${PR_SKILLS_SHA:-};; voice) w=${PR_VOICE_SHA:-};; esac
  q=$(pid lamp-$p); r=$( [ "${q:-0}" != 0 ] && sha256sum /proc/$q/exe 2>/dev/null | cut -d' ' -f1 || echo none)
  echo "BIN lamp-$p running=$r exe=$(readlink /proc/${q:-0}/exe 2>/dev/null) dropins=$(systemctl show -p DropInPaths --value lamp-$p 2>/dev/null | tr ' ' ',')"
  [ -n "$w" ] && { [ "$r" = "$w" ] && echo "BIN lamp-$p OK = the given PR sha" || why="$why lamp-$p-not-the-given-PR-sha"; }
done
# #8180: Delegate on, and lamp-tasks' processes in a leaf under its own cgroup (none left in the unit's root).
DL=$(systemctl show lamp-tasks -p Delegate --value); DC=$(systemctl show lamp-tasks -p DelegateControllers --value)
echo "DELEGATE Delegate=$DL DelegateControllers=$DC"
case "$DL" in yes) ;; *) echo "$DC" | grep -q memory && echo "$DC" | grep -q cpu && echo "$DC" | grep -q pids || why="$why no-Delegate";; esac
CGU=$(cgunit); MP=$(pid lamp-tasks); MC=$(cgof $MP); leaf=${MC#"$CGU"/}
echo "CGROUP unit=$CGU mainpid=$MP its=$MC leaf=$leaf"
if [ "$MC" = "$CGU" ] || [ "$leaf" = "$MC" ]; then why="$why mainpid-not-in-a-leaf"
else case "$leaf" in */*) why="$why mainpid-leaf-nested($leaf)";; serve|main) echo "CGROUP OK serve in its own leaf '$leaf'";; *) why="$why leaf-name-$leaf";; esac; fi
RP=$(cat /sys/fs/cgroup$CGU/cgroup.procs 2>/dev/null | tr '\n' ' '); echo "CGROUP root procs: ${RP:-none}; subtree_control: $(cat /sys/fs/cgroup$CGU/cgroup.subtree_control 2>/dev/null)"
[ -z "$RP" ] || why="$why procs-in-unit-root"
since=$(systemctl show -p ActiveEnterTimestamp --value lamp-tasks)
JA=$(journalctl -u lamp-tasks --since "$since" --no-pager -o cat 2>/dev/null)
JS=$(echo "$JA" | grep -F "each run gets its own cgroup under" | tail -1); JM=$(echo "$JA" | grep -F "runs have no own cgroup" | tail -1)
[ -n "$JS" ] && echo "J $JS"; [ -n "$JM" ] && echo "J $JM"
[ -n "$JS" ] || why="$why no-own-cgroup-start-line"
[ -n "$JM" ] && why="$why runs-have-no-own-cgroup"
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
for x in lamp-voice lamp-tasks lamp-skills lamp-cloud lamp-control; do systemctl is-active -q $x || why="$why $x-not-active"; done
[ -z "$why" ] && echo "VERIFY PASS" || echo "VERIFY FAIL:$why"
# Ready (the round kit's ready.lamp.sh): units up, OpenClaw among the lamp's engines, targets not starting.
why=""
for u in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud; do [ "$(systemctl is-active $u)" = active ] || why="$why $u-not-active"; done
oe=$(echo "$JA" | grep 'own engines, as Harness reports them:' | tail -1)
[ -n "$oe" ] && ! echo "$oe" | grep -q '"openclaw"' && why="$why openclaw-not-in-lamp-engines"
T=$(python3 -I - <<'P'
import socket
s=socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect("/run/lampos-tasks/request.sock"); s.sendall(b'{"op":"targets"}\n'); b=b""
while not b.endswith(b"\n"):
    c=s.recv(65536)
    if not c: break
    b+=c
print(b.decode().strip())
P
)
echo "$T" | grep -q '"lamp": *"starting"' && why="$why targets-starting"
echo "$T" | grep -q 'openclaw' || why="$why targets-without-openclaw"
[ -z "$why" ] && echo "READY" || echo "NOT_READY:$why"
