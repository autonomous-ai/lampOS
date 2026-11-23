# lamp, root sh. After round 2 (ch2 = stable's bytes at SEQ2): takes the run's drop-ins and channels away and starts
# lamp-update on its product line (the real stable feed). Writes no lampOS record by hand (an earlier kit's put-back
# rewrote records; this one does not). Idempotent. Needs SEQ2 STABLE_SEQ STABLE_UPD_SHA.
cd /
CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
rm -f $DROPS; systemctl daemon-reload
# The service only reads its channel at start: restart it on the product line before the channel folder goes.
systemctl restart lamp-update.service
rm -rf $CH $IN
n "put back: drop-ins $DROPS and $CH removed; lamp-update restarted on its product line"
s=0; UP=""
while [ $s -lt 300 ]; do
  UP=$(journalctl --no-pager -o cat -u lamp-update.service --after-cursor "$CUR" | grep -E 'up to date: build|refused, nothing switched|failed: |unreachable|cannot be reached' | tail -1)
  [ -n "$UP" ] && break; sleep 5; s=$((s + 5))
done
echo "J lamp-update: ${UP:-(no round end in 300 s)}"
why=""
# With the lamp at SEQ2 (> stable), the real feed's one allowed answer is "build STABLE_SEQ is older than build SEQ2".
echo "$UP" | grep -qE "build $STABLE_SEQ is older than build $SEQ2|up to date: build $STABLE_SEQ" || why="$why real-feed-answer"
b=$(build); echo "STATE $b"; [ "${b%% *}" = "$SEQ2" ] || why="$why state-not-$SEQ2"
u=$(sha256sum $UPD/current/lamp-update | cut -d' ' -f1); echo "UPDATER current=$u running=$(exesha lamp-update)"
[ "$u" = "$STABLE_UPD_SHA" ] || why="$why updater-not-stable's"
echo "LINE $(line)"; product_line || why="$why line-not-product"
D=$(dropins); [ -n "$D" ] && { echo "$D"; why="$why drop-ins-left"; }
[ -e $CH ] && why="$why $CH-left"
echo "KEYPUB $(sha256sum /etc/lampOS/update/key.pub | cut -c1-16) (compare with logs/pre.txt's)"
[ -z "$why" ] && echo "PUTBACK OK" || echo "PUTBACK NOT CLEAN:$why"
