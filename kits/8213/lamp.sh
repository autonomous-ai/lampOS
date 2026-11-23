#!/bin/sh
# #8156 ROW (head 8ae442dcd4), lamp side (first run on lamp-c0a4). Root sh, copied to /tmp/sd8156/ by the legs. Verbs:
#   snap                   read-only: build, engine choice, one-session.json, harness ps, Hermes sessions, units, files' sha
#   files-snap             the wrapper/launcher files copied (cp -p) to /tmp/sd8156/snap with their sha256 (SNAP.sha)
#   files-cmp              each file's sha now vs SNAP.sha: "SAME" or "DIFF"
#   files-restore          fallback only: each DIFF file put back from the snapshot (cp -p), logged
#   stage TSHA USHA        bind #8156's lamp-tasks and lamp-update over 15203's; one lamp-tasks restart; exe sha checked
#   rewrite                lamp-update restarted with --apply on (the bound binary): its round finds Hermes's entry
#                          points not this build's and has the one-shot lamp-update-harness.service rewrite them; waits
#   hold | unhold          lamp-update rounds held (/run drop-in, --apply off) / lifted (a real round runs)
#   engine hermes|openclaw the app's choice written as lamp-tasks writes it (engine-default.json), one restart
#   ready                  harness show lamp-one-hermes, and the three HERMES_* names in the kept Hermes's env (values 1)
#   hermes                 read-only: Hermes state.db sessions (JSON, as lampos) and transcript files
#   pending SECS           one-session.json's hermes_pending, with the time, every 0.5 s for SECS s (read-only)
#   ask request|voice JSON one line to a lamp-tasks socket, the answer printed
#   handover TEXT          one task handed over on the sockets, no sound
#   since TS UNIT RE       journal lines of UNIT since TS matching RE (ERE)
#   files                  the ROW's files (sea.md, fruits.md, colours.md, animals.md, story.md) and their text
#   putback IT             unbind both, engine choice back, one restart onto the installed exe, the ROW's files removed
# Every change is logged in /home/orangepi/changes.txt. Never touches volume, the speaker, cloud or Hermes's store.
set -u
C=/home/orangepi/changes.txt; K=/tmp/sd8156; O=/opt/sd-8156; SN=$K/snap
ST=/var/lib/lampos-tasks/store; EF=$ST/engine-default.json; EB=$K/engine-default.json.orig
HH=/var/lib/lampos-hermes/.hermes; HDB=$HH/state.db; KF=/var/lib/lampos-tasks/harnesses/lamp-tasks/one-session
HD=/run/systemd/system/lamp-update.service.d/zz-sd-8156-hold.conf
# What lamp-update (the one-shot) and lamp-tasks write for Hermes and Harness, snapshotted and compared.
FILES="/usr/local/bin/hermes /usr/local/lib/lampos/agent-ns /usr/local/lib/lampos/openclaw-ns /usr/local/lib/lampos/hermes-run-env.sh /usr/local/lib/lampos/ns-bin/harness /var/lib/lampos-tasks/.local/bin/harness /var/lib/lampos-tasks/harness-hold.env"
ROWFILES="-name sea.md -o -name fruits.md -o -name colours.md -o -name animals.md -o -name story.md"
n(){ echo "$(date '+%F %T') ROW kit #8156: $*" >> $C; }
exe(){ p=$(systemctl show -p MainPID --value lamp-tasks); sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1; }
H(){ runuser -u lampos -- env HOME=/var/lib/lampos-tasks /usr/local/bin/harness "$@" 2>&1 | head -c 4000; echo; }
restart(){ systemctl reset-failed lamp-tasks 2>/dev/null; systemctl restart lamp-tasks; n "restarted lamp-tasks ($1)"; }
rowfiles(){ find /var/lib/lampos-tasks/harnesses /var/lib/lampos-hermes -maxdepth 6 \( $ROWFILES \) -newer $K/lamp.sh 2>/dev/null; }
v=${1:-snap}; shift 2>/dev/null || true
case $v in
snap)
  echo "date $(date '+%F %T %Z') up since $(uptime -s)"
  python3 -I -c 'import json;d=json.load(open("/var/lib/lampos-update/state.json"));print("build", d["installed"]["sequence"], d["installed"]["commit"][:10])'
  echo "engine-default.json: $(cat $EF 2>/dev/null || echo none)"
  echo "one-session.json: $(cat $ST/one-session.json 2>/dev/null || echo none)"
  echo "harness $(/usr/local/bin/harness --version 2>/dev/null) | hermes $(python3 -I -c 'import json;print(json.load(open("'$HH'/hermes-agent/install-stamp.json"))["displayVersion"])' 2>/dev/null)"
  echo "lamp-tasks exe $(exe | cut -c1-16) installed $(sha256sum $(readlink -f /opt/lampOS/tasks/current)/lamp-tasks | cut -c1-16) | lamp-update installed $(sha256sum $(readlink -f /opt/lampOS/update/current)/lamp-update | cut -c1-16) | sd-8156 mounts: $(grep -c sd-8156 /proc/self/mountinfo)"
  echo "lamp-update $(systemctl show -p ExecStart --value lamp-update | grep -o 'apply o[nf]*') hold:$( [ -e $HD ] && echo on || echo off) one-shot:$(systemctl is-active lamp-update-harness.service)"
  for u in lamp-tasks lamp-voice lamp-update lamp-openclaw-gateway lamp-cloud; do printf '%s=%s ' $u $(systemctl is-active $u); done; echo
  for f in $FILES; do echo "file $(sha256sum $f 2>/dev/null | cut -c1-16 || echo absent) $f"; done
  echo "kept-folder wrapper line: $(grep -c 'lamp-tasks/one-session' /usr/local/bin/hermes) in /usr/local/bin/hermes; HERMES_NONINTERACTIVE in launcher: $(grep -c HERMES_NONINTERACTIVE /var/lib/lampos-tasks/.local/bin/harness), env file: $(grep -c HERMES_NONINTERACTIVE /var/lib/lampos-tasks/harness-hold.env)"
  echo "harness ps:"; H ps --json
  echo "hermes:"; sh $0 hermes ;;
files-snap)
  [ -s $SN/SNAP.sha ] && { echo "SNAP already ($SN)"; exit 0; }
  mkdir -p $SN; chmod 700 $SN; : > $SN/SNAP.sha
  for f in $FILES; do if [ -f $f ]; then cp -p $f $SN/$(echo $f | tr / _); echo "$(sha256sum $f | cut -d' ' -f1) $(stat -c '%a %U:%G' $f) $f" >> $SN/SNAP.sha; else echo "absent - $f" >> $SN/SNAP.sha; fi; done
  cat $SN/SNAP.sha ;;
files-cmp)
  while read -r sha mode own f; do [ "$sha" = absent ] && { f=$mode; [ -e "$f" ] && echo "DIFF now-present $f" || echo "SAME absent $f"; continue; }
    now="$(sha256sum $f 2>/dev/null | cut -d' ' -f1) $(stat -c '%a %U:%G' $f 2>/dev/null)"
    [ "$now" = "$sha $mode $own" ] && echo "SAME $f" || echo "DIFF $f (was ${sha%${sha#????????????????}} $mode $own, now $now)"; done < $SN/SNAP.sha ;;
files-restore)
  while read -r sha mode own f; do [ "$sha" = absent ] && continue
    now=$(sha256sum $f 2>/dev/null | cut -d' ' -f1)
    [ "$now" = "$sha" ] && continue
    cp -p $SN/$(echo $f | tr / _) $f.sd8156-next && mv -f $f.sd8156-next $f && n "hand-restored $f from the ROW's snapshot (the put-back round left it changed)" && echo "RESTORED $f"; done < $SN/SNAP.sha ;;
stage)
  WT=${1:?tasks sha}; WU=${2:?update sha}
  DT=$(readlink -f /opt/lampOS/tasks/current)/lamp-tasks; DU=$(readlink -f /opt/lampOS/update/current)/lamp-update
  [ -e $O ] && { echo "STAGE STOP: $O exists (putback first)"; exit 3; }
  [ "$(sha256sum $K/lamp-tasks | cut -d' ' -f1)" = "$WT" ] || { echo "STAGE STOP: copied lamp-tasks sha mismatch"; exit 3; }
  [ "$(sha256sum $K/lamp-update | cut -d' ' -f1)" = "$WU" ] || { echo "STAGE STOP: copied lamp-update sha mismatch"; exit 3; }
  for d in "$DT" "$DU"; do grep -q " $d " /proc/self/mountinfo && { echo "STAGE STOP: $d is already a mount"; exit 3; }; done
  [ "$(systemctl is-active lamp-update-harness.service)" = active ] && { echo "STAGE STOP: lamp-update's one-shot is running now"; exit 3; }
  echo "INSTALLED tasks $(sha256sum $DT | cut -d' ' -f1) update $(sha256sum $DU | cut -d' ' -f1)"
  [ -f $EB ] || cp -p $EF $EB
  mkdir -p $O; install -m 755 -o root -g root $K/lamp-tasks $O/lamp-tasks; install -m 755 -o root -g root $K/lamp-update $O/lamp-update
  mount --bind $O/lamp-tasks "$DT"; n "bind $O/lamp-tasks ($(echo $WT | cut -c1-16)) over $DT"
  mount --bind $O/lamp-update "$DU"; n "bind $O/lamp-update ($(echo $WU | cut -c1-16)) over $DU (lamp-update itself not restarted here)"
  restart "onto #8156's binary"; sleep ${SETTLE:-8}
  [ "$(exe)" = "$WT" ] && echo "EXE MATCH lamp-tasks $(systemctl is-active lamp-tasks)" || echo "EXE DIFFERS $(exe)"
  [ "$(sha256sum $DU | cut -d' ' -f1)" = "$WU" ] && echo "UPDATE BOUND" || echo "UPDATE NOT BOUND" ;;
rewrite)
  [ -e $HD ] && { echo "REWRITE STOP: the hold is on; the rewrite needs a round that acts"; exit 3; }
  t=$(date '+%F %T'); systemctl reset-failed lamp-update lamp-update-harness 2>/dev/null; systemctl restart lamp-update; n "restarted lamp-update on #8156's binary (--apply on): its round rewrites Hermes's entry points"
  p=$(systemctl show -p MainPID --value lamp-update); echo "lamp-update exe $(sha256sum /proc/$p/exe 2>/dev/null | cut -c1-16)"
  i=0; started=""; while [ $i -lt ${REWRITE_S:-480} ]; do
    s=$(systemctl show -p ActiveState --value lamp-update-harness.service)
    [ "$s" = active ] || [ "$s" = activating ] && started=1
    [ -n "$started" ] && [ "$s" = inactive -o "$s" = failed ] && break
    [ -z "$started" ] && [ $i -ge 180 ] && break
    sleep 3; i=$((i+3)); done
  echo "ONE-SHOT started=${started:-no} state=$(systemctl show -p ActiveState,Result --value lamp-update-harness.service | tr '\n' ' ') after ${i}s"
  journalctl -u lamp-update --since "$t" --no-pager -o cat | grep -E 'Hermes|Harness|up to date|switched' | cut -c1-200 | tail -8
  journalctl -u lamp-update-harness --since "$t" --no-pager -o cat | cut -c1-200 | tail -12
  echo "kept-folder wrapper line now: $(grep -c 'lamp-tasks/one-session' /usr/local/bin/hermes)" ;;
hold)
  [ -e $HD ] && { echo "HOLD already"; exit 0; }
  L=$(systemctl show -p ExecStart --value lamp-update | sed -n 's/.*argv\[\]=\([^;]*\) ;.*/\1/p' | sed 's/ *$//')
  case "$L" in *"--apply on"*) ;; *) echo "HOLD STOP: unexpected lamp-update ExecStart: $L"; exit 3;; esac
  mkdir -p $(dirname $HD)
  printf '# ROW kit #8156: rounds held (no build switch under the binds); /run, gone on reboot\n[Service]\nExecStart=\nExecStart=%s\n' "$(printf '%s' "$L" | sed 's/--apply on/--apply off/')" > $HD
  systemctl daemon-reload; systemctl restart lamp-update; n "lamp-update held (--apply off, $HD)"
  sleep 3; echo "HOLD $(systemctl show -p ExecStart --value lamp-update | grep -o 'apply o[nf]*') $(systemctl is-active lamp-update)" ;;
unhold)
  t=$(date '+%F %T'); rm -f $HD; systemctl daemon-reload; systemctl reset-failed lamp-update lamp-update-harness 2>/dev/null; systemctl restart lamp-update; n "lamp-update hold lifted (a round runs on the installed lamp-update)"
  i=0; started=""; while [ $i -lt ${REWRITE_S:-480} ]; do
    s=$(systemctl show -p ActiveState --value lamp-update-harness.service)
    [ "$s" = active ] || [ "$s" = activating ] && started=1
    r=$(journalctl -u lamp-update --since "$t" --no-pager -o cat | grep -cE 'up to date: build [0-9]+|switched to build')
    [ "$r" -gt 0 ] && { [ -n "$started" ] && [ "$s" = inactive -o "$s" = failed ] && break; [ -z "$started" ] && [ $i -ge 120 ] && break; }
    sleep 3; i=$((i+3)); done
  echo "UNHOLD $(systemctl show -p ExecStart --value lamp-update | grep -o 'apply o[nf]*') round: $(journalctl -u lamp-update --since "$t" --no-pager -o cat | grep -oE 'up to date: build [0-9]+|switched to build [0-9]+' | tail -1) one-shot started=${started:-no} $(systemctl show -p ActiveState,Result --value lamp-update-harness.service | tr '\n' ' ')"
  journalctl -u lamp-update-harness --since "$t" --no-pager -o cat | cut -c1-200 | tail -8 ;;
engine)
  E=${1:?hermes|openclaw}; case $E in hermes|openclaw) ;; *) echo "engine: hermes or openclaw"; exit 2;; esac
  [ -f $EB ] || cp -p $EF $EB
  tmp=$EF.next-$$; ( umask 077 && printf '{"version":1,"engine":"%s"}' $E > $tmp ) && chown lampos:lampos $tmp && chmod 600 $tmp && sync && mv -f $tmp $EF
  n "engine-default.json = $E (as the app writes it)"; t=$(date '+%F %T'); restart "engine $E"
  i=0; while [ $i -lt ${WAIT_S:-120} ]; do journalctl -u lamp-tasks --since "$t" --no-pager -o cat | grep -qE "kept session [a-z]+ [0-9a-f]+ \($E" && break; sleep 2; i=$((i+2)); done
  echo "SINCE $t (${i}s)"; journalctl -u lamp-tasks --since "$t" --no-pager -o short-precise | grep -E "default engine|one session|own engines" | cut -c1-240 | tail -14
  echo "one-session.json: $(cat $ST/one-session.json 2>/dev/null)" ;;
ready)
  echo "harness show:"; H show lamp-one-hermes --json | head -c 1500; echo
  found=""; for p in $(pgrep -u lampos -f 'hermes' 2>/dev/null); do
    cwd=$(readlink /proc/$p/cwd 2>/dev/null); case "$cwd" in *lamp-tasks/one-session*) ;; *) continue;; esac
    found=1; echo "pid $p cwd $cwd: $(tr '\0' '\n' < /proc/$p/environ 2>/dev/null | grep -E '^(HERMES_NONINTERACTIVE|HERMES_YOLO_MODE|HERMES_ACCEPT_HOOKS)=' | sort | tr '\n' ' ')"; done
  [ -n "$found" ] || echo "no Hermes process in $KF found" ;;
hermes)
  # As lampos (Hermes's owner), read-only: a root reader could leave a root-owned state.db-shm behind.
  runuser -u lampos -- python3 -I - "$HDB" <<'PY'
import json, os, sqlite3, sys
p = sys.argv[1]
if not os.path.exists(p):
    print(json.dumps({"db": "absent"})); sys.exit(0)
c = sqlite3.connect(f"file:{p}?mode=ro", uri=True)
rows = c.execute("SELECT s.id, s.parent_session_id, s.source, s.end_reason, (SELECT count(*) FROM messages m WHERE m.session_id = s.id) FROM sessions s ORDER BY s.started_at").fetchall()
print(json.dumps({"db": "present", "schema": c.execute("SELECT max(version) FROM schema_version").fetchone()[0],
                  "sessions": [dict(zip(("id", "parent", "source", "end", "messages"), r)) for r in rows]}))
PY
  echo "transcripts: $(ls $HH/sessions 2>/dev/null | tr '\n' ' ')" ;;
pending)
  end=$(( $(date +%s) + ${1:-60} )); last="x"
  while [ $(date +%s) -lt $end ]; do
    now=$(python3 -I -c 'import json,sys
try: d=json.load(open(sys.argv[1]))
except Exception: d={}
print(json.dumps({"agent": (d or {}).get("agent"), "hermes_pending": (d or {}).get("hermes_pending", [])}))' $ST/one-session.json)
    [ "$now" != "$last" ] && { echo "$(date '+%T.%3N') $now"; last=$now; }
    sleep 0.5; done ;;
ask)
  python3 -I - "$1" "$2" <<'PY'
import json, socket, sys
path = {"request": "/run/lampos-tasks/request.sock", "voice": "/run/lampos-tasks/voice.sock"}[sys.argv[1]]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.settimeout(15); s.connect(path)
s.sendall(sys.argv[2].encode() + b"\n"); buf = b""
while b"\n" not in buf:
    part = s.recv(65536)
    if not part: break
    buf += part
print(buf.split(b"\n", 1)[0].decode())
PY
  ;;
handover)
  # TEXT path (no sound): status, prepare on request.sock, confirm on voice.sock, as tools/played/work-records-row.py
  python3 -I - "$1" <<'PY'
import json, socket, sys, time
def ask(path, obj):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.settimeout(15); s.connect(path)
    s.sendall(json.dumps(obj).encode() + b"\n"); buf = b""
    while b"\n" not in buf:
        part = s.recv(65536)
        if not part: break
        buf += part
    return json.loads(buf.split(b"\n", 1)[0])
RQ, VS = "/run/lampos-tasks/request.sock", "/run/lampos-tasks/voice.sock"
def guard(st, tag): return {"store_id": st["store_id"], "revision": st["revision"], "request_id": f"row-8156-{tag}-{int(time.time()*1000)}"}
st = ask(RQ, {"op": "status"})
for _ in range(4):
    pr = ask(RQ, {"op": "prepare", "guard": guard(st, "prepare"), "text": sys.argv[1], "target": "lamp"})
    if pr.get("error") != "conflict": break
    time.sleep(1); st = ask(RQ, {"op": "status"})
if not pr.get("ok"): sys.exit(f"HANDOVER prepare refused: {pr}")
for _ in range(4):
    st = ask(RQ, {"op": "status"})
    cf = ask(VS, {"op": "confirm", "guard": guard(st, "confirm"), "id": pr["id"], "prepared_digest": pr["prepared_digest"], "session_id": st["summary"]["session_id"]})
    if cf.get("ok"): print("HANDOVER", pr["id"], time.strftime("%T")); sys.exit(0)
    if cf.get("error") != "conflict": break
    time.sleep(1)
sys.exit(f"HANDOVER confirm refused: {cf}")
PY
  ;;
since) journalctl -u "$2" --since "$1" --no-pager -o short-precise | grep -E "$3" | cut -c1-260 ;;
hermeslog) # Hermes's own logs since TS, lines naming a block (read-only)
  find $HH/logs -type f -newermt "$1" 2>/dev/null | while read -r f; do grep -HiE "${2:-hardline|blocked|BLOCKED|denied|refus}" "$f" | cut -c1-260 | tail -8; done ;;
files) for f in $(rowfiles); do echo "== $f"; cat "$f"; echo; done ;;
putback)
  IT=${1:?installed sha}
  findmnt -rn -o TARGET,SOURCE | awk '$2 ~ /sd-8156/ {print $1}' | while read -r t; do umount -l "$t" && n "umount -l $t" && echo "UNBOUND $t"; done
  if [ -f $EB ] && ! cmp -s $EB $EF; then cp -p $EB $EF; n "engine-default.json put back: $(cat $EF)"; fi
  [ "$(exe)" != "$IT" ] && { restart "put-back onto the installed binary"; sleep ${SETTLE:-8}; }
  for f in $(rowfiles); do rm -f "$f" && n "removed the ROW's $f"; done
  rm -rf $O; left=""
  [ "$(exe)" = "$IT" ] && echo "EXE RESTORED lamp-tasks $(systemctl is-active lamp-tasks)" || left="$left exe:$(exe | cut -c1-16)"
  grep -q sd-8156 /proc/self/mountinfo && left="$left mount"
  [ -e $O ] && left="$left $O"
  echo "engine-default.json: $(cat $EF)"
  [ -n "$left" ] && echo "LEFT:$left" || echo "lamp: nothing of #8156 left (binds, binary, files; hold: $( [ -e $HD ] && echo on || echo off))" ;;
*) echo "verb?"; exit 2 ;;
esac
