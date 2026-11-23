# lamp, root sh. READ ONLY. After round 1: waits (HWAIT s) for the new updater's Harness move to the pin, then checks.
# Needs SEQ1 PR_UPD_SHA PR_TASKS_SHA HARNESS_WANT prepended. Prints VERIFY PASS or VERIFY FAIL <why>.
# The move is held while Harness is in use (a task up to 6 h, a sign-in or hand-over always: harness_install.rs
# held_back/DEFER_CAP), and runs only after a round that is neither a self-switch nor unreachable (main.rs ~1305),
# so it comes on the new updater's own round, not the switching one.
cd /
HWAIT=${HWAIT:-1500}
CUR=$(cat $R/cursor-round1)
s=0
while [ $s -lt $HWAIT ]; do
  J=$(journalctl --no-pager -o cat -u lamp-update.service -u lamp-update-harness.service --after-cursor "$CUR" 2>/dev/null)
  echo "$J" | grep -qE "Harness moved from [0-9.]+ to the pinned $HARNESS_WANT|Harness $HARNESS_WANT is the pinned version|Harness $HARNESS_WANT not installed|Harness [0-9.]+ put back" && break
  [ "$(hver | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)" = "$HARNESS_WANT" ] && break
  sleep 10; s=$((s + 10))
done
say "Harness wait ${s}s"
journalctl --no-pager -o short-iso -u lamp-update.service -u lamp-update-harness.service --after-cursor "$CUR" | grep -E "Harness|harness" | sed 's/^/J /' | tail -25
why=""
b=$(build); echo "STATE $b"; [ "${b%% *}" = "$SEQ1" ] || why="$why state-not-$SEQ1"
u=$(sha256sum $UPD/current/lamp-update | cut -d' ' -f1); r=$(exesha lamp-update)
echo "UPDATER current=$u running=$r want=$PR_UPD_SHA"
[ "$u" = "$PR_UPD_SHA" ] && [ "$r" = "$PR_UPD_SHA" ] || why="$why updater-not-PR-build"
t=$(exesha lamp-tasks); tc=$(sha256sum /opt/lampOS/tasks/current/lamp-tasks | cut -d' ' -f1)
echo "TASKS running=$t current=$tc want=$PR_TASKS_SHA"
[ "$t" = "$PR_TASKS_SHA" ] && [ "$tc" = "$PR_TASKS_SHA" ] || why="$why lamp-tasks-not-PR-build"
v=$(hver); echo "HARNESS --version: $v"; hrec
echo "$v" | grep -qF "$HARNESS_WANT" || why="$why harness-not-$HARNESS_WANT"
python3 -I -c 'import json,sys; r=json.load(open(sys.argv[1])); sys.exit(0 if r.get("install")=="installed" and r.get("version")==sys.argv[2] else 1)' $REC/harness-install.json "$HARNESS_WANT" 2>/dev/null || why="$why harness-record-not-installed-$HARNESS_WANT"
# Harness's files as installed (shas for the ROW; the pinned manifest's own sha is checked by lamp-update before install).
echo "HARNESS bin -> $(readlink -f $HBIN)"
for f in $(readlink -f $HBIN) $HHOME/.harness/cli/cli.js; do [ -f "$f" ] && echo "HSHA $(sha256sum $f | cut -c1-16) $f"; done
[ -e $HHOME/.harness/lampos-upgrade-from ] && echo "HKEPT lampos-upgrade-from still there (move not finished?)"
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
F=$(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' '); echo "FAILED $F"
[ -z "$why" ] && echo "VERIFY PASS" || echo "VERIFY FAIL:$why"
