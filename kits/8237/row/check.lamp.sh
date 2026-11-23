# BODY check
# lamp, root sh (common first). READ ONLY. Ready for the row: engine openclaw, no update round, gateway active, mute off,
# lamp-skills and lamp-tasks running #8237's bytes (put first). Ends READY | NOT READY <why>.
TAG=test-8237; why=
[ "$(engine)" = openclaw ] || why="$why engine=$(engine)"
u=$(updbusy); [ -z "$u" ] || why="$why update:$u"
[ "$(systemctl is-active lamp-openclaw-gateway)" = active ] || why="$why gateway=$(systemctl is-active lamp-openclaw-gateway)"
for p in $PUTP; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}; s=$(sha /proc/$(pid $unit)/exe); t=$(sha /opt/lampOS/$part/$TAG/$bin)
  [ -n "$t" ] && [ "$s" = "$t" ] && echo "RUNS $unit #8237 $s" || why="$why $unit-not-on-8237"; done
L=$(mute_live); echo "$L"; echo "$L" | grep -q 'VOICE_MUTE muted=False' || why="$why mute:$(echo "$L" | head -1)"
echo "$L" | grep -q 'physical_muted=True' && why="$why mic-slide-closed"
# The asks aloud said in 24 h, all skills (skills task.rs ASKS_ALL = 3, read only): at 3, leg P's ask is refused.
na=$(tr -d '\n' < /var/lib/lampos-skills-state/task-asks.json 2>/dev/null | awk -v now=$(date +%s) 'BEGIN{RS="}"} /"said":true/ { if (match($0, /"at":[0-9]+/)) { if (now - substr($0, RSTART + 5, RLENGTH - 5) / 1000 < 86400) n++ } } END{print n + 0}')
echo "ASKS said in 24 h: $na of 3"; [ "${na:-0}" -lt 3 ] || why="$why asks-cap:$na"
[ -z "$why" ] && echo READY || echo "NOT READY$why"
