# lamp, root sh (common first; then SEQ1= HV_PR= STABLE_UPD_SHA= PR_UPDATE_SHA= PR_TASKS_SHA= FAILED0=). READ ONLY, one
# look (run.sh's verify step looks again every 30 s): after round 1, is the lamp on the PR build, its lamp-update the
# PR's (a self-switch away from stable's), Harness moved to the PR's pin, nothing newly failed, and tasks/agent ready
# with engine openclaw? Prints VERIFY PASS | VERIFY FAIL <why>.
cd /
why=""
b=$(build); echo "STATE $b busy='$(updbusy)'"; [ "$b" = "$SEQ1" ] || why="$why state-not-$SEQ1"
[ -z "$(updbusy)" ] || why="$why round-under-way"
u=$(sha256sum $UPD/current/lamp-update | cut -d' ' -f1); r=$(exesha lamp-update)
echo "UPDATER current=$u running=$r"
[ "$u" = "$r" ] || why="$why updater-not-running-current"
[ "$r" != "$STABLE_UPD_SHA" ] || why="$why updater-still-stable's"
[ -z "${PR_UPDATE_SHA:-}" ] || [ "$r" = "$PR_UPDATE_SHA" ] || why="$why updater-not-the-given-PR-sha"
t=$(exesha lamp-tasks); echo "TASKS running=$t"
[ -z "${PR_TASKS_SHA:-}" ] || [ "$t" = "$PR_TASKS_SHA" ] || why="$why lamp-tasks-not-the-given-PR-sha"
v=$(hver); echo "HARNESS --version: $v"; hrec
echo "$v" | grep -qF "$HV_PR" || why="$why harness-not-$HV_PR"
python3 -I -c 'import json,sys; r=json.load(open(sys.argv[1])); sys.exit(0 if r.get("install")=="installed" and r.get("version")==sys.argv[2] else 1)' $REC/harness-install.json "$HV_PR" 2>/dev/null || why="$why harness-record-not-installed-$HV_PR"
for f in $(readlink -f $HBIN) $HHOME/.harness/cli/cli.js $HHOME/.harness/cli/notify.mjs; do [ -f "$f" ] && echo "HSHA $(sha256sum $f | cut -d' ' -f1) $(stat -c %s $f) $f"; done
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
for x in lamp-voice lamp-tasks lamp-openclaw-gateway lamp-cloud lamp-update; do systemctl is-active -q $x || why="$why $x-not-active"; done
F=$(failed); echo "FAILED $F"
for x in $F; do case " ${FAILED0:-} " in *" $x "*) ;; *) why="$why newly-failed-$x";; esac; done
a=$(agent_now); echo "$a"
echo "$a" | grep -q 'state=ready ' || why="$why agent-not-ready"
echo "$a" | grep -q 'engine=openclaw ' || why="$why agent-engine-not-openclaw"
[ -z "$why" ] && echo "VERIFY PASS" || echo "VERIFY FAIL:$why"
