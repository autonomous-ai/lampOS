# lamp, root sh (common first). #6449's product-unit change, laid as the row's own /run drop-in on lamp-voice:
# LAMP_ACTION_CLAIM_WORDED=on, and LAMP_NAME_NEAR=off (the Mac's voice, the first builder's 07:59). 1 lamp-voice
# start. MODE=off takes it away (1 start). Prints SWITCH ON OK | SWITCH OFF OK | SWITCH FAIL.
set -u
if [ "${MODE:-on}" = off ]; then [ -f $DROP ] || { echo "SWITCH OFF OK (no drop-in)"; exit 0; }
  rm -f $DROP; systemctl daemon-reload; systemctl restart lamp-voice; n "removed $DROP; lamp-voice restarted"; sleep 8
  p=$(pid lamp-voice); [ "$(envval $p LAMP_ACTION_CLAIM_WORDED)" != on ] && echo "SWITCH OFF OK" || echo "SWITCH FAIL: still on"; exit 0; fi
mkdir -p ${DROP%/*}
printf '[Service]\n# #6449 row (the product unit change), gone at reboot\nEnvironment=LAMP_ACTION_CLAIM_WORDED=on\nEnvironment=LAMP_NAME_NEAR=off\n' > $DROP
systemctl daemon-reload; systemctl restart lamp-voice; n "added $DROP (WORDED on, NAME_NEAR off); lamp-voice restarted"; sleep 8
p=$(pid lamp-voice); w=$(envval $p LAMP_ACTION_CLAIM_WORDED); c=$(envval $p LAMP_ACTION_CLAIM_CHECKED); nn=$(envval $p LAMP_NAME_NEAR)
echo "ENV LAMP_ACTION_CLAIM_WORDED=$w LAMP_ACTION_CLAIM_CHECKED=$c LAMP_NAME_NEAR=$nn (pid $p)"
[ "$w" = on ] && [ "$c" = on ] && [ "$nn" = off ] && echo "SWITCH ON OK" || echo "SWITCH FAIL"
