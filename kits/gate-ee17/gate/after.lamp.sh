# lamp, root sh (common first, then RTAG=<tag> T0=<round start epoch>). READ ONLY, read straight after a round, before
# anything restarts a unit. Raw facts only; gate/eval.py judges them on the Mac.
#   STATE, UPD                lamp-update's records and its installed binary's sha256
#   GW ...                    the gateway: ActiveState, listening on 18789, PartOf, TimeoutStopUSec, NRestarts
#   INODE host= gateway=      control.sock's inode on the host and in the gateway's own mount namespace (#8320)
#   GWJ <journal line>        the gateway unit's journal since T0: starts, stops, exits, kills, timeouts
#   GWW <epoch> LISTEN|DOWN   the round's port watcher (round.lamp.sh); the watcher is stopped here
#   UNIT <name> active= sub= result= type=   lampos.target's lamp units, the gateway and lamp-update
#   FAILED <units>            systemctl --failed
cd /
echo "STATE $(stateval) busy='$(updbusy)'"
echo "UPD $(updsha)"
echo "GW active=$(systemctl show -p ActiveState --value $GW) listening=$(listening && echo yes || echo no) partof='$(systemctl show -p PartOf --value $GW)' timeout='$(systemctl show -p TimeoutStopUSec --value $GW)' nrestarts=$(systemctl show -p NRestarts --value $GW)"
hi=$(stat -Lc %i $CTL 2>/dev/null); hostns=$(readlink /proc/1/ns/mnt); gp=
cg=$(systemctl show -p ControlGroup --value $GW)
for p in $(cat /sys/fs/cgroup$cg/cgroup.procs 2>/dev/null); do [ "$(readlink /proc/$p/ns/mnt 2>/dev/null)" != "$hostns" ] && { gp=$p; break; }; done
if [ -n "$gp" ]; then gi=$(nsenter -t $gp -m stat -Lc %i $GWCTL 2>/dev/null || stat -Lc %i /proc/$gp/root$GWCTL 2>/dev/null); ns=own
else gp=$(pid $GW); gi=$hi; ns=host; fi
echo "INODE host=${hi:-none} gateway=${gi:-none} pid=${gp:-none} ns=$ns path=$GWCTL"
journalctl -u $GW --since "@$T0" -o short-unix --no-pager 2>/dev/null |
  grep -E 'Main process exited|KILL|Killing|[Tt]imed out|timeout|Failed with result|Started |Stopping |Stopped |Scheduled restart' | sed 's/^/GWJ /'
W=$R/gwwatch.$RTAG
[ -f $W.log ] && sed 's/^/GWW /' $W.log
touch $W.stop; [ -f $W.pid ] && { kill "$(cat $W.pid)" 2>/dev/null; rm -f $W.pid; }
echo "NOW $(date +%s)"
for u in $( { systemctl list-dependencies --plain --no-legend lampos.target 2>/dev/null | sed 's/^[^a-zA-Z]*//' | awk '{print $1}' | grep -E '^lamp[a-z0-9-]*\.(service|socket|timer)$'; echo $GW.service; echo lamp-update.service; } | sort -u); do
  echo "UNIT $u active=$(systemctl show -p ActiveState --value $u) sub=$(systemctl show -p SubState --value $u) result=$(systemctl show -p Result --value $u) type=$(systemctl show -p Type --value $u)"
done
echo "FAILED $(systemctl --failed --plain --no-legend 2>/dev/null | awk '{print $1}' | tr '\n' ' ')"
