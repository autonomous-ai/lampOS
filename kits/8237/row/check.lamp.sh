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
# The asks-aloud ledger, cleared by put for the row: it must be empty here (read only).
[ -s /var/lib/lampos-skills-state/task-asks.json ] && grep -q '"said":true' /var/lib/lampos-skills-state/task-asks.json && why="$why asks-not-cleared"; echo "ASKS LEDGER at check: $(cat /var/lib/lampos-skills-state/task-asks.json 2>/dev/null | head -c 80 || echo none)"
[ -z "$why" ] && echo READY || echo "NOT READY$why"
