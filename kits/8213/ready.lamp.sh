# lamp, root sh. READ ONLY. Is the lamp ready, as a person waits after plugging it in? Prints READY or NOT_READY <why>.
cd /
why=""
for u in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud; do [ "$(systemctl is-active $u)" = active ] || why="$why $u-not-active"; done
since=$(systemctl show -p ActiveEnterTimestamp --value lamp-tasks)
J=$(journalctl -u lamp-tasks --since "$since" --no-pager -o cat 2>/dev/null)
echo "$J" | grep -q "older than 0.6.11" && why="$why harness-version-too-old"
# journald may drop or rotate lamp-tasks' lines on a busy lamp: judge by the last engines line seen, if any
oe=$(echo "$J" | grep 'own engines, as Harness reports them:' | tail -1)
[ -n "$oe" ] && ! echo "$oe" | grep -q '"openclaw"' && why="$why openclaw-not-in-lamp-engines"
last=$(echo "$J" | grep -E "one session: (kept session live|kept session new|not opened|dropped)" | tail -1)
case "$last" in *"not opened"*|*dropped*) why="$why kept-session:$(echo $last | cut -c1-80 | tr ' ' _)";; esac
T=$(python3 -I - <<'P'
import json,socket
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
up=$(( $(date +%s) - $(date -d "$since" +%s) ))
[ -z "$why" ] && echo "READY lamp-tasks up ${up}s; ${last:-no kept line yet}" || echo "NOT_READY up ${up}s:$why"
