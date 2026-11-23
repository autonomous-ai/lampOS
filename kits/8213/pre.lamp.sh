# lamp, root sh. READ ONLY. Is the lamp on stable, on its product line, with nothing of anyone's staged?
# Needs STABLE_SEQ, STABLE_UPD_SHA (prepended by round.zsh from out/plan.env). Prints PRE OK or PRE STOP <why>.
cd /
why=""
b=$(build); echo "STATE $b"
[ "${b%% *}" = "$STABLE_SEQ" ] || why="$why not-on-stable-$STABLE_SEQ(${b%% *})"
u=$(sha256sum $UPD/current/lamp-update 2>/dev/null | cut -d' ' -f1); echo "UPDATER $u"
[ "$u" = "$STABLE_UPD_SHA" ] || why="$why update/current-not-stable's-lamp-update"
echo "LINE $(line)"; product_line || why="$why lamp-update-line-not-product"
D=$(dropins); [ -n "$D" ] && { echo "$D"; why="$why drop-ins-present"; }
for f in self.json journal.json apply/request.json; do [ -e $REC/$f ] && why="$why pending-$f"; done
systemctl is-active -q lamp-update.service || why="$why lamp-update-not-active"
[ -e $CH ] && why="$why $CH-exists(earlier-run-not-put-back)"
echo "HARNESS $(hver)"; hrec
echo "TASKS exe $(exesha lamp-tasks)"
echo "KEYPUB $(sha256sum /etc/lampOS/update/key.pub | cut -c1-16)"
echo "REFUSED $(python3 -I -c 'import json,sys; print(",".join(str(r.get("sequence")) for r in (json.load(open(sys.argv[1])).get("refused") or []) if isinstance(r,dict)))' $UST)"
echo "FAILED $(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
[ -z "$why" ] && echo "PRE OK" || echo "PRE STOP:$why"
