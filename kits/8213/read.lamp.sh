# lamp, root sh. READ ONLY. The end read (run twice, 60 s apart, by round.zsh). Needs SEQ2 STABLE_UPD_SHA STABLE_TASKS_SHA.
cd /
b=$(build); echo "STATE $b"
echo "UPDATER current=$(sha256sum $UPD/current/lamp-update | cut -c1-16) want=$(echo $STABLE_UPD_SHA | cut -c1-16)"
echo "TASKS running=$(exesha lamp-tasks | cut -c1-16) want=$(echo $STABLE_TASKS_SHA | cut -c1-16)"
echo "LINE $(line)"; dropins
echo "HARNESS $(hver)"; hrec
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
echo "FAILED $(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
journalctl --no-pager -o cat -u lamp-update.service -n 3 | sed 's/^/J /'
# #8156 put-back: OpenClaw is the lamp's agent, its kept session open again, nothing of the kit left.
agent_now
set -- $(kept); echo "KEPT_AGENT $1 machine $2"
journalctl -u lamp-tasks --since "-30min" --no-pager -o cat | grep -E "one session: kept session (new|live|reused)|$(printf 'tasks: agent: [a-z_]+ -> ready')" | tail -2 | cut -c1-160 | sed 's/^/J /'
echo "STOPPED_PIDS $(ps -eo pid=,stat=,user= | awk '$2 ~ /^T/ && ($3=="lampos" || $3=="lampos-tasks") {print $1}' | tr '\n' ' ')"
grep -q t8156 $OCW && echo "KIT_LEFT wrapper line" || echo "WRAPPER clean"
[ -e $REC/openclaw-ask.json ] && echo "KIT_LEFT openclaw-ask.json" || true
