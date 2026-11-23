# BODY putback
# lamp, root sh (common first). Idempotent put-back of put.lamp.sh: lamp-skills and lamp-tasks stopped only if one of
# our binds is there, binds unmounted (only ours), the hold and the LAMP_NAME_NEAR drop-in gone, test dirs removed,
# units started on stable's installed files. Proof: each installed file AND each running exe sha256 == the stable sha
# recorded by put before any change. Ends PUTBACK OK | PUTBACK NOT OK.
TAG=test-8237; tdir(){ echo /opt/lampOS/$1/$TAG; }; mounted(){ grep -c " $1 " /proc/self/mountinfo; }
set -u; ok=1; W=0
for p in $PUTP; do part=${p%%:*}; bin=${p##*:}; [ "$(mounted "$(inst $part $bin)")" != 0 ] && W=1; done
if [ $W = 1 ]; then systemctl stop lamp-skills lamp-tasks; n "stopped lamp-skills lamp-tasks for the put-back"
  for u in lamp-skills lamp-tasks; do [ "$(pid $u)" = 0 ] || { echo "PUTBACK NOT OK: $u still has a pid: nothing unmounted"; exit 4; }; done
  for p in $PUTP; do part=${p%%:*}; bin=${p##*:}; f=$(inst $part $bin)
    while [ "$(mounted "$f")" != 0 ]; do src=$(findmnt -n -o SOURCE "$f" | head -1)
      echo "$src" | grep -q "/opt/lampOS/$part" || { echo "NOT OURS: $f bound from $src: left"; ok=0; break; }
      umount "$f" || { echo "UMOUNT FAILED $f"; ok=0; break; }; n "umounted $f"; done; done; fi
[ -f $HD ] && { rm -f $HD; rmdir ${HD%/*} 2>/dev/null; n "removed $HD"; }
AD=/run/systemd/system/lamp-skills-manage@.service.d/zz-row-8237-anyasker.conf; [ -f $AD ] && { rm -f $AD; rmdir ${AD%/*} 2>/dev/null; n "removed $AD"; }
V=0; [ -f $ND ] && { rm -f $ND; rmdir ${ND%/*} 2>/dev/null; n "removed $ND"; V=1; }
for p in $PUTP; do part=${p%%:*}; [ -d $(tdir $part) ] && { rm -rf $(tdir $part); n "removed $(tdir $part)"; }; done
rm -rf /tmp/t8237; systemctl daemon-reload
[ $W = 1 ] && { systemctl start lamp-tasks lamp-skills; n "started lamp-tasks lamp-skills on stable"; }
[ $V = 1 ] && { systemctl restart lamp-voice; n "restarted lamp-voice without the drop-in"; }
systemctl restart lamp-update 2>/dev/null; upwait 60 lamp-tasks lamp-skills lamp-voice; sleep 10
for p in $PUTP; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}; f=$(inst $part $bin); pp=$(pid $unit)
  want=$(cat $R/$bin.stable 2>/dev/null); fs=$(sha "$f"); rs=$(sha /proc/$pp/exe)
  if [ -z "$want" ]; then echo "FAIL $bin: no stable sha recorded (put never ran?): file $fs running ${rs:-none}"; ok=0
  elif [ "$fs" = "$want" ] && [ "$rs" = "$want" ]; then echo "PASS $unit back on stable $want (file and running exe)"
  else echo "FAIL $unit: file $fs running ${rs:-none} stable $want"; ok=0; fi
  [ "$(mounted "$f")" = 0 ] || { echo "FAIL a mount left over $f"; ok=0; }; done
[ -f $HD ] && { echo "FAIL hold left: $HD"; ok=0; }; [ -f $AD ] && { echo "FAIL drop-in left: $AD"; ok=0; }; [ -f $ND ] && { echo "FAIL drop-in left: $ND"; ok=0; }
[ -n "$(envval "$(pid lamp-voice)" LAMP_NAME_NEAR | grep -x off)" ] && [ ! -f $ND ] && echo "NOTE lamp-voice still has LAMP_NAME_NEAR=off from elsewhere"
AK=/var/lib/lampos-skills-state/task-asks.json
if [ -e $R/task-asks.found ]; then cp -p $R/task-asks.found $AK; n "put $AK back as found"
  [ "$(sha $AK)" = "$(sha $R/task-asks.found)" ] && { echo "ASKS LEDGER put back as found $(sha $AK)"; rm -f $R/task-asks.found; } || { echo "FAIL asks ledger not as found"; ok=0; }
elif [ -e $R/task-asks.none ]; then rm -f $AK; n "removed the row's $AK (none before)"; echo "ASKS LEDGER none, as found"; rm -f $R/task-asks.none
else echo "ASKS LEDGER not recorded by put: left as is"; fi
sp=$(speaker); pv=$(echo "$sp" | sed -n 's/^SPEAKER [^ ]* \([0-9]*\)%$/\1/p'); echo "$sp at put-back"; [ -n "$pv" ] && [ "$pv" -gt "${SPK_MAX:-40}" ] && echo "NOTE the speaker is over ${SPK_MAX:-40}% at put-back: not set by the kit; ask the manager"
[ $ok = 1 ] && { rm -f $R/*.stable; echo "PUTBACK OK"; } || echo "PUTBACK NOT OK"
