#!/bin/sh
# In the rig: one real round, `sh round.sh <name>`, as lamp-update.service would run it:
#   env <the unit's Environment= lines> /opt/lampOS/update/current/lamp-update once \
#     --channel-url file:///var/tmp/el-8400/chanB/ --channel lamp-voice \
#     --key-file /var/tmp/el-8400/test.pub --apply on
# (the runbook's step 7 with #8369's --key-file). Whatever updater the lamp runs runs it: on
# build A that is A's, which switches and self-switches, then `exec`s B's in its place
# (main.rs's SelfSwitched arm), whose confirm and first round run in the same process.
# Kept in out/<name>/: journal.txt (the round's lamp-update: lines, stderr and stdout in
# order, then EXIT n), state.json and log.jsonl (new lines) before/after, self.json if any,
# the systemctl calls (calls.txt), the units and links (files.txt).
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
N=${1:?usage: sh round.sh <name>}
D=$OUT/$N
[ -e "$D" ] && die "$D is there already: a round's name is used once"
mkdir -p "$D"
[ -e "$REC/self.json" ] && cp "$REC/self.json" "$D/self.before.json"
cp "$REC/state.json" "$D/state.before.json"
logn=$(wc -l < "$REC/log.jsonl" 2>/dev/null || echo 0)
calln=$(wc -l < "$K/calls.log")
prog=$(readlink -f /opt/lampOS/update/current)/lamp-update
echo "$prog" > "$D/updater.before"
mark "round $N by $prog"
ENVS=$(unit_env)
echo "$ENVS" > "$D/env.txt"
# shellcheck disable=SC2086
( env $ENVS timeout 1800 /opt/lampOS/update/current/lamp-update once \
    --channel-url "file://$K/chanB/" --channel lamp-voice --key-file "$K/test.pub" --apply on 2>&1
  echo "EXIT $?" ) > "$D/journal.txt"
mark "round $N ended"
cp "$REC/state.json" "$D/state.json"
[ -e "$REC/self.json" ] && cp "$REC/self.json" "$D/self.json"
tail -n +"$((logn + 1))" "$REC/log.jsonl" > "$D/log.jsonl" 2>/dev/null
tail -n +"$((calln + 1))" "$K/calls.log" > "$D/calls.txt"
readlink -f /opt/lampOS/update/current > "$D/updater.after"
{
  echo "## links"
  for l in /opt/lampOS/*/current; do echo "$l -> $(readlink "$l")"; done
  echo "## units (lamp-npu, lamp-apps*, lamp-app*, lamp-eyes, lamp-update*)"
  for f in "$UNITS"/lamp-npu.service "$UNITS"/lamp-apps* "$UNITS"/lamp-app@.service "$UNITS"/lamp-app-net@.service \
           "$UNITS"/lamp-eyes.service "$UNITS"/lamp-update*.service; do
    [ -e "$f" ] && echo "$(sha256sum "$f" | cut -c1-64) $f"
  done
  echo "## wants links"
  find "$UNITS" -path '*.wants/*' \( -name 'lamp-npu*' -o -name 'lamp-app*' \) -printf '%p -> %l\n' | sort
  echo "## stand-in states"
  for s in /var/lib/el-8400-stub/active/*; do echo "$(basename "$s"): $(cat "$s")"; done
} > "$D/files.txt"
tail -n 25 "$D/journal.txt"
say "round $N kept in $D"
