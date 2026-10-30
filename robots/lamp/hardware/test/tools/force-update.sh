#!/usr/bin/env bash
# force-update.sh — put many set-up lamps on lampos-stable's newest build now,
# from the Mac at the bench, without waiting for a quiet room.
#
#   force-update.sh start IP...    on each lamp, in parallel: stop lamp-voice,
#                                  one lamp-update round (--apply on), start
#                                  lamp-voice and lamp-update again. Runs on the
#                                  lamp as its own unit, so a dropped ssh is fine.
#   force-update.sh status IP...   each lamp's hostname, installed build, and the
#                                  round's last line
#
# Why: the updater switches only at a quiet moment, judged from voice/state on
# the bus; 7071's voice doesn't publish it ("quiet cannot be judged"), so a 7071
# lamp waits forever. With lamp-voice stopped the round switches (lamp-dbd1,
# 7071 -> 8197, 28 Oct). Environment: LAMP_PASS (required), STATE (default
# ~/.factory-lamp), PAR (lamps at once, default 20).
set -uo pipefail
MODE=${1:-}; shift || true
PASS=${LAMP_PASS:?set LAMP_PASS to the lamp password}
STATE=${STATE:-$HOME/.factory-lamp}; mkdir -p "$STATE"
PAR=${PAR:-20}
[[ ($MODE == start || $MODE == status) && $# -gt 0 ]] || { sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
URL=https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/

# One root command on a lamp; its output. The command goes as base64, so no
# quoting reaches the shell.
lamp() {
  local host=$1 cmd=$2
  P="$PASS" H="$host" KH="$STATE/known_hosts_bench_$host" \
  B="$(printf '%s' "$cmd" | base64 | tr -d '\n')" expect <<'EOF' | tr -d '\r' | grep -vE 'assword:|SUDOPW|^Warning: Permanently'
set timeout 60
log_user 1
spawn -noecho ssh -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no -o ConnectTimeout=10 orangepi@$env(H) "echo $env(B) | base64 -d > /tmp/force-update.sh && sudo -S -p SUDOPW: bash /tmp/force-update.sh"
expect {
  -re "assword:" { send "$env(P)\r"; exp_continue }
  "SUDOPW:" { send "$env(P)\r"; exp_continue }
  timeout { puts "TIMEOUT" }
  eof
}
EOF
}

# Up to three rounds: an old updater first switches to its own newer build
# (LAMP_UPDATE_SELF=on; "build N needs updater 7 or newer, and this is
# updater 5"), then the next round, run by the new one, switches the build.
ROUND='L=/var/lib/lamp-test/force-update.log; : > $L
systemctl stop lamp-update lamp-voice
for round in 1 2 3; do
  before=$(jq -r .installed.sequence /var/lib/lampos-update/state.json)
  LAMP_UPDATE_SELF=on /opt/lampOS/update/current/lamp-update once --channel-url '"$URL"' --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on --quiet-wait-s 300 >> $L 2>&1
  rc=$?; echo "round $round rc $rc" >> $L
  after=$(jq -r .installed.sequence /var/lib/lampos-update/state.json)
  [ "$after" != "$before" ] && break
  grep -q "needs updater" $L || break
done
systemctl start lamp-voice lamp-update
echo "rc $rc build $(jq -r .installed.sequence /var/lib/lampos-update/state.json)" >> $L'

one() {
  local host=$1 out
  if [[ $MODE == start ]]; then
    out=$(lamp "$host" 'h=$(hostname); case $h in lamp-*) ;; *) echo "NOT A LAMP: $h"; exit 3;; esac
      systemctl is-active -q force-update && { echo "$h already updating"; exit 0; }
      mkdir -p /var/lib/lamp-test
      echo '"$(printf '%s\n' "$ROUND" | base64 | tr -d '\n')"' | base64 -d > /var/lib/lamp-test/force-round.sh
      systemd-run --unit=force-update --collect bash /var/lib/lamp-test/force-round.sh >/dev/null 2>&1 \
        && echo "$h started (was $(jq -r .installed.sequence /var/lib/lampos-update/state.json))" || echo "$h NOT started"')
  else
    out=$(lamp "$host" 'h=$(hostname); s=$(jq -r .installed.sequence /var/lib/lampos-update/state.json)
      r=$(systemctl is-active force-update 2>/dev/null); l=$(grep -vE "skills: write|kept /etc" /var/lib/lamp-test/force-update.log 2>/dev/null | tail -1 | cut -c1-120)
      echo "$h build $s round:${r:-none} $l"')
  fi
  echo "$host  ${out:-unreachable}"
}

n=0
for host in "$@"; do
  one "$host" &
  n=$((n+1)); (( n % PAR == 0 )) && wait
done
wait
