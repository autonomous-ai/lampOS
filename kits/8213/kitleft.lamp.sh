# lamp, root sh (common first). Piece B's trap only, when legs8213 was cut before its own put-back: its agent-first
# lamp-voice drop-in (x.sh `first on`) removed, one lamp-voice restart, as x.sh `first off` does. Prints KITLEFT CLEAN.
cd /
FD=/run/systemd/system/lamp-voice.service.d/zz-sd-8213-first.conf
if [ -e $FD ]; then rm -f $FD; systemctl daemon-reload; systemctl restart lamp-voice; n "split kit: legs8213's $FD removed; lamp-voice restarted"; sleep 15; echo "FIRST REMOVED"; fi
[ ! -e $FD ] && echo "KITLEFT CLEAN" || echo "KITLEFT FAIL $FD"
