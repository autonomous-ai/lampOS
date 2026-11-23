# lamp, root sh. One real round of the lamp's own lamp-update.service, pointed at local channel N (1 = the PR build,
# 2 = back to stable's bytes). Needs N and SEQ prepended. The only change: a /run drop-in giving lamp-update's own
# product line another --channel-url and --key-file (in lamp-update.service.d, not lamp-.service.d, which
# head_touch::no_mode_overrides would refuse: Migrate, #7920 16:09), the test key bound for lamp-update-apply, and
# `systemctl restart lamp-update.service` (serve rounds at its start, in the product sandbox). The rest is the round's.
cd /
MAXS=${MAXS:-1500}
[ -d $CH/ch$N ] || { echo "ROUND STOP: not staged"; exit 3; }
CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
echo "$CUR" > $R/cursor-round$N
mkdir -p "$(dirname $DROP)" "$(dirname $DROPA)"
cat > $DROP <<EOF
# t8156 (#8156 round ROW): round $N from file://$CH/ch$N/, test key. /run only; removed by putback.lamp.sh.
[Service]
ExecStart=
ExecStart=/opt/lampOS/update/current/lamp-update serve --channel-url file://$CH/ch$N/ --channel lamp-voice --key-file $CH/test.pub --apply on
EOF
cat > $DROPA <<EOF
# t8156 (#8156 round ROW): apply re-checks the envelope with key.pub; bind the test key there. /run only.
[Service]
BindReadOnlyPaths=$CH/test.pub:/etc/lampOS/update/key.pub
EOF
systemctl daemon-reload
systemctl show -p DropInPaths --value lamp-update-apply.service | tr ' ' '\n' | grep -qxF "$DROPA" && systemctl show -p BindReadOnlyPaths --value lamp-update-apply.service | grep -q test.pub \
  || { rm -f $DROPS; systemctl daemon-reload; echo "ROUND STOP: lamp-update-apply did not load the test-key bind"; exit 3; }
T0=$(date '+%F %T')
systemctl restart lamp-update.service
n "round $N: lamp-update.service pointed at file://$CH/ch$N/ (drop-ins $DROPS), restarted"
say "round $N to build $SEQ started $T0"
END="switched to build $SEQ|refused, nothing switched|build $SEQ put back|failed: |not switched: |up to date: build $SEQ|is older than build|nothing changed: "
s=0; ended=""
while [ $s -lt $MAXS ]; do
  J=$(journalctl --no-pager -o cat -u lamp-update.service --after-cursor "$CUR" 2>/dev/null)
  [ -z "$ended" ] && echo "$J" | grep -qE "$END" && ended=$s
  if [ -n "$ended" ]; then
    # A switch of lamp-update's own ends when the new updater confirms itself and says its own round's end.
    if echo "$J" | grep -qE "lamp-update switches to its build"; then
      echo "$J" | grep -qE "not confirmed: " && break
      echo "$J" | grep -q "confirmed: lamp-update runs its new build" && echo "$J" | grep -qE "up to date: build $SEQ" && break
    else break; fi
    [ $((s - ended)) -ge 660 ] && break
  fi
  sleep 5; s=$((s + 5))
done
[ -n "$ended" ] || say "no end line within ${MAXS}s"
say "round $N waited ${s}s"
for u in lamp-update lamp-update-apply lamp-update-writer lamp-update-back lamp-update-harness; do
  journalctl --no-pager -o short-iso -u $u.service --after-cursor "$CUR" 2>/dev/null | grep -v '^-- ' | sed "s/^/J $u: /"
done
echo "STATE $(build)"
echo "UPDATER current=$(sha256sum $UPD/current/lamp-update | cut -c1-12) running=$(exesha lamp-update | cut -c1-12)"
echo "FAILED $(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
say "ROUND $N DONE"
