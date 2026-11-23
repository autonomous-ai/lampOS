# lamp, root sh (row common, swap-common). Idempotent put-back of swap.lamp.sh: the four units stopped only if a bind of ours is
# there, the bind unmounted, the hold gone, the test dirs removed, the units started on the installed build, shas
# checked. The row's drop-in and snapshot are unrow's (../run.sh unrow first). RESTORED OK | RESTORE NOT OK.
set -u; ok=1; W=0
for p in $PARTS; do part=${p%%:*}; bin=${p##*:}; [ "$(mounted "$(inst $part $bin)")" != 0 ] && W=1; done
if [ $W = 1 ]; then
  systemctl stop $UNITS; n "stopped $UNITS for restore"
  for u in $UNITS; do [ "$(pid $u)" = 0 ] || { echo "RESTORE STOP: $u still has a pid: nothing unmounted"; exit 4; }; done
  for p in $PARTS; do part=${p%%:*}; bin=${p##*:}; f=$(inst $part $bin)
    while [ "$(mounted "$f")" != 0 ]; do src=$(findmnt -n -o SOURCE "$f" | head -1)
      echo "$src" | grep -q "$TAG" || echo "$src" | grep -q "/opt/lampOS/$part" || { echo "NOT OURS: $f bound from $src: left"; ok=0; break; }
      umount "$f" || { echo "UMOUNT FAILED $f"; ok=0; break; }; n "umounted $f"; done
  done
fi
[ -f $HD ] && { rm -f $HD; rmdir ${HD%/*} 2>/dev/null; n "removed $HD"; }
for f in $UD_TASKS $UD_CONTROL; do [ -f $f ] && { rm -f $f; rmdir ${f%/*} 2>/dev/null; n "removed $f"; W=1; }; done
for p in $PARTS; do part=${p%%:*}; [ -d $(tdir $part) ] && { rm -rf $(tdir $part); n "removed $(tdir $part)"; }; done
rm -rf /tmp/t8180; systemctl daemon-reload
[ $W = 1 ] && { systemctl restart $UNITS; n "restarted $UNITS on the installed build"; sleep 25; gwrestart || ok=0; }
systemctl restart lamp-update 2>/dev/null; sleep 20
for p in $PARTS; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}; f=$(inst $part $bin); pp=$(pid $unit)
  want=$(cat $R/$bin.installed 2>/dev/null); fs=$(sha256sum "$f" | cut -d' ' -f1); rs=$(sha256sum /proc/$pp/exe 2>/dev/null | cut -d' ' -f1)
  if [ -z "$want" ]; then echo "NOTE $bin: no installed sha recorded: running $rs"
  elif [ "$fs" = "$want" ] && [ "$rs" = "$want" ]; then echo "PASS $unit back on installed $want"
  else echo "FAIL $unit: file $fs running ${rs:-none} installed $want"; ok=0; fi
  [ "$(mounted "$f")" = 0 ] || { echo "FAIL mount left over $f"; ok=0; }
done
[ -f $HD ] && { echo "FAIL hold left: $HD"; ok=0; }
for f in $UD_TASKS $UD_CONTROL; do [ -f $f ] && { echo "FAIL drop-in left: $f"; ok=0; }; done
[ $ok = 1 ] && { rm -f $R/*.installed; echo "RESTORED OK"; } || echo "RESTORE NOT OK"
