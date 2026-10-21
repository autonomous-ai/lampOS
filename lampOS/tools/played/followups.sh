#!/bin/zsh
# Follow-ups that need a tool, with no name (#1465, 18 Oct: after a named "turn yourself down", "Thanks. Now turn
# yourself back up a little" was refused by the Policy and asked 72 times in 49 s). Two named asks, each followed by
# an unnamed follow-up; then from the voice's journal: tool calls run, refused, and the most asks of one request.
# The software volume is put back as it was. PLAYS SOUND, about 1.5 minutes. argv: LABEL
cd "${0:A:h}" || exit 9
. ./settings.sh
S=(./lamp_ssh)
vol() { "${S[@]}" "amixer -c allwinnerhdmi sget Speaker | sed -n 's/.*Front Left: Playback \([0-9]*\) .*/\1/p'" </dev/null; }
V0=$(vol); T0=$(date +%H:%M:%S)
until ./quiet_gate.sh 45 >/dev/null 2>&1; do sleep 10; done
./scenario.py followups-${1:-$(date +%H%M)} \
  "Samantha|Hey Lamp, make your light a little dimmer.|10" \
  "Samantha|Thanks. And a little dimmer still, please.|12" \
  "Samantha|Hey Lamp, turn yourself down a bit.|10" \
  "Samantha|Thanks. Now turn yourself back up a little, please.|14" \
  "Samantha|Hey Lamp, make your light a little brighter.|10" 2>&1 | grep -E " PLAY | LAMP " | cut -c1-110
sleep 3
"${S[@]}" "journalctl -u lamp-voice --since '$T0' --no-pager -o cat" </dev/null | /usr/bin/awk '
  / asked for [a-z_]+; run after/ {run++}
  / asked for [a-z_]+; not done:/ {refused++}
  match($0, /request [0-9]+ asked for [a-z_]+; waiting/) {k = substr($0, RSTART, RLENGTH); n[k]++; if (n[k] > most) most = n[k]}
  END {printf "follow-ups: tool calls run %d, refused %d, the most asks of one request %d\n", run, refused, most}'
V1=$(vol); [ -n "$V0" ] && [ "$V1" != "$V0" ] && { "${S[@]}" "sudo -n amixer -c allwinnerhdmi sset Speaker $V0 >/dev/null" </dev/null; echo "software volume set back to $V0 (was $V1)"; }
