# Lamp-side defines for swap.lamp.sh and restore.lamp.sh, cat'ed after row/common.lamp.sh. The lamp stays on its
# installed stable build; #8180's four runtime parts (lamp-tasks, lamp-control, lamp-skills, lamp-voice) are each
# swapped by a runtime read-only bind of /opt/lampOS/<part>/test-8180/<bin> over the installed file (gone on reboot),
# so every caller of /opt/lampOS/<part>/current/<bin> gets it. lamp-update is not swapped: #8180 changes its tests only.
TAG=test-8180
# part:unit:binary
PARTS="tasks:lamp-tasks:lamp-tasks control:lamp-control:lamp-control skills:lamp-skills:lamp-skills voice:lamp-voice:lamp-voice"
UNITS="lamp-voice lamp-skills lamp-control lamp-tasks"
inst(){ readlink -f /opt/lampOS/$1/current/$2; }
tdir(){ echo /opt/lampOS/$1/$TAG; }
mounted(){ grep -c " $1 " /proc/self/mountinfo; }
# #8320 (stable 15543 has no #8323): OpenClaw's gateway binds lamp-control's socket into its namespace once, at its
# start, and a skill run's fence inherits that namespace. A lamp-control restarted under it (the swap's, restore's)
# leaves the gateway on the old socket. So after the units start, the gateway is restarted too, and waited for.
GW=lamp-openclaw-gateway; GWPORT=4965                 # 18789 = 0x4965, as /proc/net/tcp writes a local port
gwrestart(){ systemctl is-active -q $GW || { echo "GATEWAY $GW not active: not restarted"; return 0; }
  systemctl restart $GW; n "restarted $GW after lamp-control (#8320)"; i=0
  while [ $i -lt 90 ]; do cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk -v p=":$GWPORT" '$2 ~ p"$" && $4 == "0A" {f=1} END {exit !f}' && { echo "GATEWAY restarted, listening after ${i}s"; return 0; }; sleep 2; i=$((i+2)); done
  echo "GATEWAY restarted, not listening after 90s"; return 1; }
HD=/run/systemd/system/lamp-update.service.d/zz-row-8180-hold.conf
