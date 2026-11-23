# lamp, root sh (row common first, then MODE=before|reboot|after|port|times). Leg 7170 (#7170, the gateway must serve
# before a run is handed to it), run by boot7170.sh. Only `reboot` changes anything: ONE normal `systemctl reboot`
# (no hand step; the /run drop-ins of legs E and N go with it). Never touches volume, the mute or lamp-cloud.
#   before  BOOTID, the mute (bus and record), the gateway unit
#   reboot  REBOOT ASKED: systemctl reboot in 3 s, by a transient timer, so this ssh ends cleanly first
#   after   BOOTID, lamp-cloud, then the mute once lamp-voice published it (at most 90 s): MUTEBOOT muted=<x>
#   port    waits (PORTWAIT s, default 300) for a listener on the OpenClaw gateway's loopback port 18789
#           (lamp_update::openclaw_install::PORT): PORT listening up=<uptime s> epoch=<s>, GWSTART up=<s>; or PORT NONE
#   times   (PORT_UP=<s>) this boot's gateway start, port and approvals read from the journal: STARTUP ...
set -u
GW=lamp-openclaw-gateway; PORT=18789; PHEX=4965     # 18789 = 0x4965, as /proc/net/tcp writes a local port
up(){ cut -d' ' -f1 /proc/uptime; }
gwstart(){ m=$(systemctl show -p ExecMainStartTimestampMonotonic --value $GW 2>/dev/null)
  [ -n "$m" ] && [ "$m" != 0 ] && awk -v m=$m 'BEGIN{printf "%.2f", m/1e6}' || echo none; }
listening(){ cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk -v p=":$PHEX" '$2 ~ p"$" && $4 == "0A" {f=1} END {exit !f}'; }
recmuted(){ python3 -I -c 'import json,sys
try: print(json.load(open(sys.argv[1])).get("muted"))
except Exception as e: print("unread")' $MR; }
case "${MODE:-before}" in
before) echo "BOOTID $(cat /proc/sys/kernel/random/boot_id)"; echo "UPTIME $(up)"; mute_live; echo "RECORD muted=$(recmuted)"
  echo "GATEWAY $(systemctl is-active $GW) start_up=$(gwstart)"; echo "VOICE_DROPINS $(ls /run/systemd/system/lamp-voice.service.d/ 2>/dev/null | tr '\n' ' ')";;
reboot) n "leg 7170: one normal systemctl reboot (no hand step)"
  if systemd-run --on-active=3 --unit=row-8180-reboot /bin/systemctl reboot >/dev/null 2>&1; then echo "REBOOT ASKED (systemctl reboot in 3 s)"
  else ( sleep 3; systemctl reboot ) </dev/null >/dev/null 2>&1 & echo "REBOOT ASKED (systemctl reboot in 3 s, background)"; fi;;
after) echo "BOOTID $(cat /proc/sys/kernel/random/boot_id)"; echo "UPTIME $(up)"
  echo "CLOUD $(systemctl is-active lamp-cloud)"
  s=0; while [ $s -lt 90 ]; do L=$(mute_live); echo "$L" | grep -qE 'VOICE_MUTE muted=(True|False) ' && break; sleep 3; s=$((s + 3)); done
  echo "$L"; echo "RECORD muted=$(recmuted)"
  m=$(echo "$L" | sed -n 's/^VOICE_MUTE muted=\([A-Za-z]*\) .*/\1/p')
  case "$m" in True|False) echo "MUTEBOOT muted=$m (bus, after ${s}s)";; *) echo "MUTEBOOT muted=$(recmuted) (record; the bus had none in 90 s)";; esac
  echo "VOICE $(systemctl is-active lamp-voice) GATEWAY $(systemctl is-active $GW)";;
port) s=0; w=${PORTWAIT:-300}
  while [ $s -lt $((w * 2)) ]; do listening && break; sleep 0.5; s=$((s + 1)); done
  if listening; then echo "PORT listening up=$(up) epoch=$(date +%s.%N) after_s=$((s / 2))"; echo "GWSTART up=$(gwstart)"
  else echo "PORT NONE in ${w} s ($GW $(systemctl is-active $GW), start_up=$(gwstart))"; fi
  echo "VOICE $(systemctl is-active lamp-voice) TASKS $(systemctl is-active lamp-tasks)";;
times) U=$(up); E=$(date +%s.%N); GS=$(gwstart)
  # journal epoch -> uptime on this boot
  toup(){ awk -v u=$U -v e=$E -v t=$1 'BEGIN{printf "%.2f", u - (e - t)}'; }
  JT=$(journalctl -b -u lamp-tasks -o short-unix --no-pager 2>/dev/null)
  a=$(echo "$JT" | grep -F "the OpenClaw Gateway's allow-list add for" | grep -F succeeded | head -1)
  echo "$JT" | grep -E "OpenClaw Gateway's (allow-list add|port has not answered)|hand-over is not sent|skill start: run [^ ]+('s script started|'s fence never said)|claimed by the launcher" | cut -c1-240 | sed 's/^/J /'
  if [ -n "$a" ]; then ae=$(echo "$a" | awk '{print $1}'); ms=$(echo "$a" | grep -oE 'in [0-9]+ ms' | head -1 | grep -oE '[0-9]+')
    AP=$(toup $(awk -v e=$ae -v m=${ms:-0} 'BEGIN{printf "%.3f", e - m/1000}')); else AP=none; fi
  d(){ [ "$1" = none ] || [ "$2" = none ] || [ -z "$1" ] || [ -z "$2" ] && echo "?" || awk -v a=$1 -v b=$2 'BEGIN{printf "%.1f", b - a}'; }
  P=${PORT_UP:-none}
  echo "STARTUP gateway unit start up=$GS, port listening up=$P, approvals read answering up=$AP (uptime s)"
  echo "STARTUP unit->port $(d $GS $P) s, port->approvals $(d $P $AP) s, unit->approvals $(d $GS $AP) s";;
esac
