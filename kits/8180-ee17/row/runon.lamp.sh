# lamp, root sh (common.lamp.sh first, then MODE=on|off). Leg E: LAMP_MAKE_MAY_RUN_ON=on for lamp-voice by a
# /run drop-in (gone at reboot), and back off. One lamp-voice restart each way (row.sh GATEs it first).
set -u
if [ "$MODE" = on ]; then
  mkdir -p ${ND_RUNON%/*}; printf '# #8180 row leg E: /run, removed by runon off and by unrow\n[Service]\nEnvironment=LAMP_MAKE_MAY_RUN_ON=on\n' > $ND_RUNON; n "added $ND_RUNON"
else [ -f $ND_RUNON ] && { rm -f $ND_RUNON; rmdir ${ND_RUNON%/*} 2>/dev/null; n "removed $ND_RUNON"; }; fi
systemctl daemon-reload; systemctl restart lamp-voice; n "restarted lamp-voice (runon $MODE)"; sleep 20
v=$(pid lamp-voice); val=$(envval $v LAMP_MAKE_MAY_RUN_ON); s=$(sha256sum /proc/$v/exe | cut -d' ' -f1)
[ -n "${PR_VOICE_SHA:-}" ] && [ "$s" = "$PR_VOICE_SHA" ] && echo "PASS lamp-voice still on the PR build" || echo "NOTE lamp-voice exe $s (PR ${PR_VOICE_SHA:-not given})"
if [ "$MODE" = on ]; then [ "$val" = on ] && echo "RUNON ON" || echo "RUNON NOT ON ('$val')"
else [ -z "$val" ] && echo "RUNON OFF" || echo "RUNON STILL '$val'"; fi
