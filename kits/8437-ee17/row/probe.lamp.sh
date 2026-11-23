# lamp, root sh (common.lamp.sh first, then "OP=push|remove SUM=<sha256> NAME=<fixture>"). Legs C, H, P: a row
# fixture by the product's push install (common.lamp.sh PUSHD, mhelper), the #8237 kit's stage.lamp.sh ported
# (thirteen/kit-8237 5b5e8e4291). push.sh's lamp side.
# push: the package (/tmp/t8180-probe/package.tar, packed on the Mac by the SDK's lampos.push.pack) kept as
# lamp-control's door keeps a push it took (PUSHD/push.json + package.tar, read within 60 s), then
# {"install_pushed": SUM} to the manage helper's socket: lamp-skills' install_pushed_at does the rest, into skills-dev.
# Before the first push, lamp-control's task-yes.json is copied to the row's records ($R/task-yes.before, or
# $R/task-yes.none) and any push already kept there set aside. The name is noted in $R/pushed.names for unrow.
# Prints PUSHED <the helper's answer> and PROBE INSTALLED, or PUSH REFUSED <why> and PROBE NOT INSTALLED.
# remove: {"remove_pushed": {client, name}} (unrow does the same for every name in pushed.names). Prints REMOVED <name>
# or REMOVE REFUSED <why>.
set -u
OP=${OP:-push}; NAME=${NAME:-lemon-probe}; SUM=${SUM:-}
refused(){ echo "PUSH REFUSED $*"; echo "PROBE NOT INSTALLED"; exit 3; }
case "$OP" in
push)
  if [ -d $DEV/$NAME ]; then grep -qx "$NAME" $PUSHED 2>/dev/null || echo "$NAME" >> $PUSHED
    echo "PROBE already pushed ($DEV/$NAME)"; echo "PROBE INSTALLED"; exit 0; fi
  [ -e $SK/$NAME ] && refused "$SK/$NAME is there (an older hand install): a pushed skill never takes its place; ./run.sh unrow first"
  [ -S $MSOCK ] || refused "no manage helper socket at $MSOCK"
  [ "$(sha /tmp/t8180-probe/package.tar)" = "$SUM" ] || refused "/tmp/t8180-probe/package.tar is not $SUM"
  if [ ! -e $R/task-yes.before ] && [ ! -e $R/task-yes.none ]; then
    if [ -f $YESF ]; then cp -p $YESF $R/task-yes.before; n "kept $YESF as found ($(sha $YESF))"; else : > $R/task-yes.none; n "no $YESF before the push"; fi; fi
  [ -d $PUSHD ] || install -d -m 0700 $PUSHD
  for f in push.json package.tar; do [ -e $PUSHD/$f ] && { mkdir -p $R/push.before; mv $PUSHD/$f $R/push.before/$f; n "set aside $PUSHD/$f"; }; done
  install -m 0600 /tmp/t8180-probe/package.tar $PUSHD/package.tar; rm -rf /tmp/t8180-probe
  now=$(python3 -I -c 'import time; print(int(time.time() * 1000))')
  printf '{"sha256":"%s","client":"%s","client_name":"the #8180 row (local push)","name":"%s","grants":["skills","speak"],"at_unix_ms":%s}\n' \
    "$SUM" "$PCLIENT" "$NAME" "$now" > $PUSHD/push.json; chmod 0600 $PUSHD/push.json
  n "staged $NAME ($SUM) in $PUSHD for the manage helper"
  r=$(mhelper "{\"install_pushed\": \"$SUM\"}"); echo "HELPER $r"
  rm -f $PUSHD/push.json $PUSHD/package.tar
  for f in push.json package.tar; do [ -e $R/push.before/$f ] && { mv $R/push.before/$f $PUSHD/$f; n "put back $PUSHD/$f"; }; done
  rmdir $R/push.before 2>/dev/null
  if okreply "$r" && [ -d $DEV/$NAME ]; then
    grep -qx "$NAME" $PUSHED 2>/dev/null || echo "$NAME" >> $PUSHED
    n "installed $NAME by the manage helper (pushed)"; echo "PUSHED $r"; sleep 3; echo "PROBE INSTALLED"
  else echo "PUSH REFUSED $r"; echo "PROBE NOT INSTALLED"; fi;;
remove)
  r=$(mhelper "{\"remove_pushed\": {\"client\": \"$PCLIENT\", \"name\": \"$NAME\"}}"); echo "HELPER $r"
  if [ ! -d $DEV/$NAME ]; then n "removed $NAME by the manage helper"; echo "REMOVED $NAME"
  else echo "REMOVE REFUSED $r"; fi;;
*) echo "PUSH REFUSED OP=$OP: push or remove"; echo "PROBE NOT INSTALLED"; exit 2;;
esac
