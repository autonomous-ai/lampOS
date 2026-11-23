# lamp, root sh (common first). The row's put-back: the drop-in removed, $SNAP put back from $R/snap (the lamp's
# state as before the row), $SNAPUNITS and lamp-voice started (1 start each). The swap is restore's. Prints UNROW OK
# | UNROW NOT OK.
set -u; ok=1
systemctl stop lamp-voice $SNAPUNITS
[ -f $DROP ] && { rm -f $DROP; n "removed $DROP"; }; systemctl daemon-reload
if [ -d $R/snap ]; then for d in $SNAP; do [ -d $R/snap$d ] || continue; rm -rf $d; cp -a $R/snap$d $d; done; n "unrow: $SNAP put back from $R/snap"
else echo "NOTE no $R/snap: nothing put back"; fi
systemctl start $SNAPUNITS lamp-voice; n "unrow: $SNAPUNITS lamp-voice started"; sleep 10
p=$(pid lamp-voice); [ -f $DROP ] && { echo "FAIL drop-in left"; ok=0; }
[ "$(envval $p LAMP_ACTION_CLAIM_WORDED)" = on ] && { echo "FAIL WORDED still on in lamp-voice"; ok=0; }
for u in lamp-voice $SNAPUNITS; do [ "$(systemctl is-active $u)" = active ] || { echo "FAIL $u $(systemctl is-active $u)"; ok=0; }; done
[ $ok = 1 ] && { rm -rf $R/snap; echo "UNROW OK"; } || echo "UNROW NOT OK"
