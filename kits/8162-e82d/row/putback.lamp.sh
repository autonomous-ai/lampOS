# lamp, root sh (common first; then MODE=clean|read SEQ2= STABLE_SEQ= STABLE_UPD_SHA=). After round 2 (stable's bytes
# at SEQ2): lamp-update back on its product line, nothing of the kit's left. Writes no lampOS record by hand.
#  MODE=clean (channel mode) the kit's drop-ins and $CH removed, daemon-reload, lamp-update restarted on its product
#             line (1 start); its round on the real stable feed must answer "build STABLE_SEQ is older than build SEQ2"
#  MODE=read  (cmd mode) nothing changed: the runner's ROUND2_CMD must have left lamp-update on its product line
# Prints PUTBACK OK | PUTBACK NOT CLEAN <why>.
cd /
why=""
if [ "${MODE:-read}" = clean ]; then
  CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
  rm -f $DROPS; systemctl daemon-reload
  systemctl restart lamp-update.service
  rm -rf $CH $IN
  n "put back: drop-ins $DROPS and $CH removed; lamp-update restarted on its product line"
  s=0; UP=""
  while [ $s -lt 300 ]; do
    UP=$(journalctl --no-pager -o cat -u lamp-update.service --after-cursor "$CUR" 2>/dev/null | grep -E 'up to date: build|refused, nothing switched|failed: |nothing changed: ' | tail -1)
    [ -n "$UP" ] && break; sleep 5; s=$((s + 5)); done
  echo "J ${UP:-(no round end in 300 s)}"
  # On SEQ2 (above stable), the real feed's one allowed answer: stable's build is older than the lamp's.
  echo "$UP" | grep -qE "build $STABLE_SEQ is older than build $SEQ2|up to date: build $SEQ2" || why="$why real-feed-answer"
fi
b=$(build); echo "STATE $b busy='$(updbusy)'"; [ "$b" = "$SEQ2" ] || why="$why state-not-$SEQ2"
u=$(sha256sum $UPD/current/lamp-update | cut -d' ' -f1); echo "UPDATER current=$u running=$(exesha lamp-update)"
[ "$u" = "$STABLE_UPD_SHA" ] || why="$why updater-not-stable's"
echo "LINE $(line)"; product_line || why="$why line-not-product"
D=$(dropins); [ -n "$D" ] && { echo "$D"; why="$why drop-ins-left"; }
[ -e $CH ] && why="$why $CH-left"
[ -z "$why" ] && echo "PUTBACK OK" || echo "PUTBACK NOT CLEAN:$why"
