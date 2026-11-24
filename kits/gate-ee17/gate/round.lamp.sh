# lamp, root sh (common first, then RTAG=<tag> URL=<channel url> [T0=<epoch>]). One round by the INSTALLED updater
# (/opt/lampOS/update/current/lamp-update), exactly the product's: lamp-update.service's own Environment= lines and its
# --key-file (the signed production manifest, no test key), `once --channel-url URL --channel lamp-voice --apply on`.
# Before it, a watcher of the gateway's port (18789) is started: each LISTEN/DOWN change with its epoch into
# ~/tgate/gwwatch.<tag>.log (after.lamp.sh reads it and stops it; it stops itself after 1 h).
# Prints T0, UPD before, CMD, "--- OUTPUT", the round's whole output (stdout and stderr), "--- END", EXIT <code>,
# UPD after, STATE.
cd /
u=$(updbusy); [ -z "$u" ] || { echo "ROUND STOP: a round is under way ($u): wait"; echo "EXIT busy"; exit 3; }
T0=${T0:-$(date +%s)}; echo "T0 $T0"
cat > $R/gwwatch.sh <<'W'
# gateway port watcher: $1 log, $2 stop file
s=; i=0
while [ $i -lt 3600 ] && [ ! -e "$2" ]; do
  if cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk '$2 ~ ":4965$" && $4 == "0A" {f=1} END {exit !f}'; then x=LISTEN; else x=DOWN; fi
  [ "$x" = "$s" ] || { echo "$(date +%s) $x" >> "$1"; s=$x; }
  sleep 1; i=$((i+1))
done
W
W=$R/gwwatch.$RTAG
if [ -f $W.pid ] && kill -0 "$(cat $W.pid)" 2>/dev/null; then echo "WATCH already running (pid $(cat $W.pid))"
else rm -f $W.stop $W.log; setsid sh $R/gwwatch.sh $W.log $W.stop </dev/null >/dev/null 2>&1 & echo $! > $W.pid; echo "WATCH started pid $(cat $W.pid)"; fi
KEY=$(systemctl show -p ExecStart --value lamp-update | grep -oE -- '--key-file [^ ;]+' | head -1 | awk '{print $2}'); KEY=${KEY:-/etc/lampOS/update/key.pub}
ENVS=$(systemctl show -p Environment --value lamp-update)
echo "UPD before $(updsha) ($(readlink -f $UPDBIN))"
echo "ENV $ENVS"
echo "CMD $UPDBIN once --channel-url $URL --channel lamp-voice --key-file $KEY --apply on"
n "round $RTAG: installed updater $(updsha | cut -c1-12) once --channel-url $URL --apply on"
echo "--- OUTPUT"
# shellcheck disable=SC2086 # the unit's Environment= words (KEY=value, no spaces in the product's unit)
env $ENVS "$UPDBIN" once --channel-url "$URL" --channel lamp-voice --key-file "$KEY" --apply on 2>&1; rc=$?
echo "--- END"
echo "EXIT $rc"
echo "UPD after $(updsha) ($(readlink -f $UPDBIN))"
echo "STATE $(stateval)"
n "round $RTAG ended exit $rc, $(stateval)"
