# BODY stage
# lamp-skills' manage helper, asked as lamp-control's door asks it, with the network door skipped.
# lamp, root sh (common first, then OP= NAME= CLIENT= [SUM=]). stage.sh's lamp side.
# push: the package (/tmp/t8237s/package.tar, from the Mac) kept as lamp-control's door keeps a push it took
# (package.rs PUSH_DIR/push.json + package.tar, read within 60 s), then {"install_pushed": SUM} to the manage helper's
# socket: lamp-skills' install_pushed_at does the rest. Before the first push, lamp-control's task-yes.json is copied
# to the row's records ($R/task-yes.before, or $R/task-yes.none) and any push already kept there set aside.
# remove: {"remove_pushed": {client, name}}, then task-yes.json is put back as it was before the push (YES-FILE line).
P=/var/lib/lampos-control/push; Y=/var/lib/lampos-control/task-yes.json; SOCK=/run/lampos-skills-manage/manage.sock
ask(){ python3 -I - "$SOCK" "$1" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(40); s.connect(sys.argv[1]); s.sendall(sys.argv[2].encode() + b"\n")
b = b""
while not b.endswith(b"\n"):
    c = s.recv(65536)
    if not c: break
    b += c
print(b.decode(errors="replace").strip())
PY
}
okreply(){ python3 -I -c 'import json,sys; v=json.loads(sys.argv[1]); sys.exit(0 if "done" in v and "refused" not in v else 1)' "$1" 2>/dev/null; }
# The helper takes pushed requests from lamp-control's cgroup only (package.rs asker_of). For the one request, its own
# test switch LAMP_SKILLS_ANY_ASKER=on (main.rs "asked": said in the journal each time) by a /run drop-in on
# lamp-skills-manage@ (socket-activated, so the next connection reads it), removed right after the answer.
AD=/run/systemd/system/lamp-skills-manage@.service.d/zz-row-8237-anyasker.conf
any_on(){ mkdir -p ${AD%/*}; printf '[Service]\nEnvironment=LAMP_SKILLS_ANY_ASKER=on\n' > $AD; systemctl daemon-reload; n "added $AD"; }
any_off(){ [ -f $AD ] && { rm -f $AD; rmdir ${AD%/*} 2>/dev/null; systemctl daemon-reload; n "removed $AD"; }; [ ! -f $AD ]; }
helper(){ any_on; r=$(ask "$1"); any_off || r="$r (ANY_ASKER drop-in NOT removed)"; echo "$r"; }
case "$OP" in
push)
  [ -S $SOCK ] || { echo "PUSH REFUSED no manage helper socket at $SOCK"; exit 3; }
  [ "$(sha /tmp/t8237s/package.tar)" = "$SUM" ] || { echo "PUSH REFUSED /tmp/t8237s/package.tar is not $SUM"; exit 3; }
  if [ ! -e $R/task-yes.before ] && [ ! -e $R/task-yes.none ]; then
    if [ -f $Y ]; then cp -p $Y $R/task-yes.before; n "kept $Y as found ($(sha $Y))"; else : > $R/task-yes.none; n "no $Y before the push"; fi; fi
  [ -d $P ] || install -d -m 0700 $P
  for f in push.json package.tar; do [ -e $P/$f ] && { mkdir -p $R/push.before; mv $P/$f $R/push.before/$f; n "set aside $P/$f"; }; done
  install -m 0600 /tmp/t8237s/package.tar $P/package.tar; rm -rf /tmp/t8237s
  now=$(python3 -I -c 'import time; print(int(time.time() * 1000))')
  printf '{"sha256":"%s","client":"%s","client_name":"the #8237 row (local stage)","name":"%s","grants":["skills","speak"],"at_unix_ms":%s}\n' \
    "$SUM" "$CLIENT" "$NAME" "$now" > $P/push.json; chmod 0600 $P/push.json
  n "staged $NAME ($SUM) in $P for the manage helper"
  r=$(helper "{\"install_pushed\": \"$SUM\"}"); echo "HELPER $r"
  rm -f $P/push.json $P/package.tar
  for f in push.json package.tar; do [ -e $R/push.before/$f ] && { mv $R/push.before/$f $P/$f; n "put back $P/$f"; }; done
  rmdir $R/push.before 2>/dev/null
  if okreply "$r" && [ -d /var/lib/lampOS/skills-dev/$NAME ]; then n "installed $NAME by the manage helper"; echo "PUSHED $r"
  else echo "PUSH REFUSED $r"; fi;;
remove)
  r=$(helper "{\"remove_pushed\": {\"client\": \"$CLIENT\", \"name\": \"$NAME\"}}"); echo "HELPER $r"
  if [ ! -d /var/lib/lampOS/skills-dev/$NAME ]; then n "removed $NAME by the manage helper"; echo "REMOVED $NAME"
  else echo "REMOVE REFUSED $r"; fi
  if [ -f $R/task-yes.before ]; then
    if [ "$(sha $Y)" = "$(sha $R/task-yes.before)" ]; then echo "YES-FILE as found $(sha $Y)"
    else cp -p $R/task-yes.before $Y; n "put $Y back as found"; echo "YES-FILE put back $(sha $Y)"; fi
    [ "$(sha $Y)" = "$(sha $R/task-yes.before)" ] && rm -f $R/task-yes.before
  elif [ -f $R/task-yes.none ]; then
    [ -f $Y ] && { rm -f $Y; n "removed $Y (none before the push)"; }; echo "YES-FILE none, as found"; rm -f $R/task-yes.none
  else echo "YES-FILE no record kept (no push ran)"; fi;;
esac
