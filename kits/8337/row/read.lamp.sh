# lamp, root sh (common first; then SEQ2= STABLE_UPD_SHA= HV= FAILED0=). READ ONLY. The end read, run twice 60 s apart
# by run.sh: build SEQ2, stable's updater, its product line, no drop-ins, Harness at stable's pin, OpenClaw the agent
# with its kept session open, no stopped pids, the wrapper clean, nothing newly failed. Lines that move with time are
# prefixed T (the Mac's comparison leaves them out). Prints READ OK | READ FAIL <why>.
cd /
why=""
b=$(build); echo "STATE $b busy='$(updbusy)'"; [ "$b" = "$SEQ2" ] || why="$why state-not-$SEQ2"
echo "INSTALLED $(python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence"), i.get("commit"))' $UST 2>/dev/null)"   # sequence and commit, compared across the two reads
u=$(sha256sum $UPD/current/lamp-update | cut -d' ' -f1); r=$(exesha lamp-update); echo "UPDATER current=$u running=$r"
[ "$u" = "$STABLE_UPD_SHA" ] && [ "$r" = "$STABLE_UPD_SHA" ] || why="$why updater-not-stable's"
echo "LINE $(line)"; product_line || why="$why line-not-product"
D=$(dropins); [ -n "$D" ] && { echo "$D"; why="$why drop-ins-left"; }
[ -e $CH ] && why="$why $CH-left"
v=$(hver); echo "HARNESS $v"; hrec; echo "$v" | grep -qF "$HV" || why="$why harness-not-$HV"
for f in $HHOME/.harness/cli/cli.js $HHOME/.harness/cli/notify.mjs; do [ -f "$f" ] && echo "HSHA $(sha256sum $f | cut -d' ' -f1) $(stat -c %s $f) $f"; done
e=$(engine); echo "ENGINE ${e:-none}"; [ "$e" = openclaw ] || why="$why engine-not-openclaw"
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
for x in lamp-voice lamp-tasks lamp-openclaw-gateway lamp-cloud lamp-update; do systemctl is-active -q $x || why="$why $x-not-active"; done
F=$(failed); echo "FAILED $F"; for x in $F; do case " ${FAILED0:-} " in *" $x "*) ;; *) why="$why newly-failed-$x";; esac; done
set -- $(kept); echo "KEPT_AGENT $1"
K=$(journalctl -u lamp-tasks --since "$(systemctl show -p ActiveEnterTimestamp --value lamp-tasks)" --no-pager -o cat 2>/dev/null | grep -E "one session: kept session (new|live|reused) " | tail -1 | cut -c1-140)
echo "T J ${K:-no kept session line since lamp-tasks started}"; [ -n "$K" ] || why="$why kept-session-not-open"
S=$(stopped_lampos); echo "STOPPED_PIDS ${S:-none}"; [ -z "$S" ] || why="$why stopped-pids"
grep -q t8337 $OCW 2>/dev/null && { echo "WRAPPER the kit's line left"; why="$why wrapper"; } || echo "WRAPPER clean"
exeshas
echo "T $(agent_now) (stable's lamp-tasks keeps no tasks/agent: printed, not judged)"
echo "T $(mute_live | head -1)"
[ -z "$why" ] && echo "READ OK" || echo "READ FAIL:$why"
