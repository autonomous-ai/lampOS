# lamp, root sh (common first; then MODE= N= [SEQ=]). One real round of the lamp's own lamp-update (#8162's lead
# reviewer: lamp-update changes, so the ROW comes from a real round, never a drop-in of binaries).
#  MODE=mark  the journal's cursor kept for round N ($R/cursor-roundN); nothing changed
#  MODE=go    channel mode only: lamp-update's own product line given --channel-url file://$CH/chN/ and --key-file
#             $CH/test.pub by a /run drop-in, the test key bound for lamp-update-apply, `systemctl restart
#             lamp-update.service` (serve rounds at its start). 1 start. The drop-ins go in putback.lamp.sh.
#  MODE=read  READ ONLY: SEQ installed=<n> busy='<what>', then round N's journal end lines since the mark (J lines)
cd /
case ${MODE:-read} in
mark) CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p'); echo "$CUR" > $R/cursor-round$N
      echo "MARK round $N at $(date +%s.%N) build=$(build) max=$(seqmax)";;
go)   [ -d $CH/ch$N ] || { echo "ROUND STOP: ch$N not staged"; exit 3; }
      mkdir -p "$(dirname $DROP)" "$(dirname $DROPA)"
      printf '# t8337 (#8337 ROW): round %s from file://%s/ch%s/, the runner'"'"'s test key. /run only; removed by putback.lamp.sh.\n[Service]\nExecStart=\nExecStart=/opt/lampOS/update/current/lamp-update serve --channel-url file://%s/ch%s/ --channel lamp-voice --key-file %s/test.pub --apply on\n' $N $CH $N $CH $N $CH > $DROP
      printf '# t8337 (#8337 ROW): apply re-checks the envelope with key.pub; the test key bound there. /run only.\n[Service]\nBindReadOnlyPaths=%s/test.pub:/etc/lampOS/update/key.pub\n' $CH > $DROPA
      systemctl daemon-reload
      systemctl show -p DropInPaths --value lamp-update-apply.service | tr ' ' '\n' | grep -qxF "$DROPA" && systemctl show -p BindReadOnlyPaths --value lamp-update-apply.service | grep -q test.pub \
        || { rm -f $DROPS; systemctl daemon-reload; echo "ROUND STOP: lamp-update-apply did not load the test-key bind"; exit 3; }
      systemctl restart lamp-update.service
      n "round $N: lamp-update.service pointed at file://$CH/ch$N/ (drop-ins $DROPS), restarted"
      echo "GO round $N at $(date +%s.%N)";;
read) echo "SEQ installed=$(build) busy='$(updbusy)'"
      CUR=$(cat $R/cursor-round$N 2>/dev/null)
      journalctl --no-pager -o short-unix -u lamp-update.service -u lamp-update-harness.service ${CUR:+--after-cursor "$CUR"} 2>/dev/null \
        | grep -E "switching to build|switched to build|up to date: build|refused, nothing switched|put back|not switched: |failed: |switches to its build|switches from its build|confirmed: |not confirmed: |is older than build|Harness .* (moved|pinned)" | cut -c1-260 | sed 's/^/J /'
      echo "UPDATER running=$(exesha lamp-update)";;
esac
