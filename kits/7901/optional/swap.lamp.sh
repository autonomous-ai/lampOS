# OPTIONAL PATH ONLY. lamp, root sh (row common, swap-common, then SHAS='lamp-voice=<sha> ...'). Stops the four units,
# binds each test binary read-only over the installed file, holds lamp-update's apply, starts them (1 start each),
# restarts OpenClaw's gateway (its own mount namespace). Every change -> changes.txt. Ends SWAPPED OK | SWAP NOT OK.
set -e
u=$(updbusy); [ -z "$u" ] || { echo "lamp-update round under way ($u): wait"; exit 3; }
[ "$(engine)" = openclaw ] || { echo "engine is '$(engine)', not openclaw: stop"; exit 3; }
for p in $PARTS; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}
  f=$(inst $part $bin); [ "$(mounted "$f")" = 0 ] || { echo "already a mount over $f"; exit 3; }
  w=$(echo " $SHAS " | sed -n "s/.* $bin=\([0-9a-f]*\) .*/\1/p")
  [ "$(sha256sum /tmp/t7901/$bin | cut -d' ' -f1)" = "$w" ] || { echo "/tmp/t7901/$bin is not $w"; exit 3; }
  sha256sum "$f" | cut -d' ' -f1 > $R/$bin.installed; echo "INSTALLED $bin $(cat $R/$bin.installed) ($f)"
done
X=$(systemctl show -p ExecStart --value lamp-update | sed -n 's/.*argv\[\]=\([^;]*\);.*/\1/p' | sed 's/ *$//')
if echo "$X" | grep -q -- '--apply on'; then mkdir -p ${HD%/*}
  printf '[Service]\nExecStart=\nExecStart=%s\n' "$(echo "$X" | sed 's/--apply on/--apply off/')" > $HD; n "added $HD (lamp-update --apply off)"
fi
for p in $PARTS; do part=${p%%:*}; bin=${p##*:}; install -d $(tdir $part); install -m 0755 -o root -g root /tmp/t7901/$bin $(tdir $part)/$bin; n "placed $(tdir $part)/$bin"; done
rm -rf /tmp/t7901; systemctl daemon-reload
[ -f $HD ] && { systemctl restart lamp-update; n "lamp-update restarted apply off"; }
systemctl stop $UNITS; n "stopped $UNITS for the binds"
for u in $UNITS; do [ "$(pid $u)" = 0 ] || { echo "SWAP STOP: $u still has a pid: nothing bound"; exit 4; }; done
for p in $PARTS; do part=${p%%:*}; bin=${p##*:}; mount --bind -o ro $(tdir $part)/$bin "$(inst $part $bin)"; n "bind $(tdir $part)/$bin over $(inst $part $bin)"; done
systemctl start $UNITS; n "started $UNITS on the #7901 build"; sleep 25
set +e; ok=1
for p in $PARTS; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}; pp=$(pid $unit)
  s=$(sha256sum /proc/$pp/exe 2>/dev/null | cut -d' ' -f1); t=$(sha256sum $(tdir $part)/$bin | cut -d' ' -f1)
  [ -n "$s" ] && [ "$s" = "$t" ] && echo "PASS $unit runs $bin $s" || { echo "FAIL $unit not on the test build (pid $pp, $s)"; ok=0; }
done
systemctl restart lamp-openclaw-gateway; n "restarted lamp-openclaw-gateway"; sleep 20
[ $ok = 1 ] && echo "SWAPPED OK" || echo "SWAP NOT OK"
