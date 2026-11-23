# lamp, root sh (common first). Before each try: the state of $SNAP put back from $R/snap with its units and
# lamp-voice stopped, then started (1 start each), so a try starts a fresh conversation on the lamp as it was.
# Prints FRESH OK | FRESH FAIL.
set -u; [ -d $R/snap ] || { echo "FRESH FAIL: no $R/snap (save first)"; exit 3; }
systemctl stop lamp-voice $SNAPUNITS
for d in $SNAP; do [ -d $R/snap$d ] || continue; rm -rf $d; cp -a $R/snap$d $d; done
systemctl start $SNAPUNITS lamp-voice; n "fresh: $SNAP put back from $R/snap; $SNAPUNITS lamp-voice started"
s=0; while [ $s -lt 40 ]; do sleep 2; s=$((s+2)); [ "$(systemctl is-active lamp-voice)" = active ] && break; done
sleep 6; p=$(pid lamp-voice)
[ "$(systemctl is-active lamp-voice)" = active ] && [ "$(envval $p LAMP_ACTION_CLAIM_WORDED)" = on ] && echo "FRESH OK pid $p after ${s}s" || echo "FRESH FAIL (lamp-voice $(systemctl is-active lamp-voice), WORDED '$(envval $p LAMP_ACTION_CLAIM_WORDED)')"
