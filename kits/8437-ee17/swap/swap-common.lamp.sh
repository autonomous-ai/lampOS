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
gwrestart(){ systemctl cat $GW >/dev/null 2>&1 || { echo "GATEWAY $GW: no such unit, not restarted"; return 0; }
  systemctl is-failed -q $GW && { echo "GATEWAY $GW was failed: its start limit reset"; systemctl reset-failed $GW; }
  systemctl restart $GW; n "restarted $GW after lamp-control (#8320)"; gwlisten 90 && return 0
  # A new gateway that meets the old one's lock heartbeat still fresh (GatewayLockNamespaceError, "wait up to 90
  # seconds after its last heartbeat") exits, and systemd's quick restarts can end in its start limit (ee17, 23 Nov
  # 11:15). Once the heartbeat is older than 90 s, one more start, from a reset start limit.
  journalctl -u $GW --since -3min --no-pager -o cat 2>/dev/null | grep -q GatewayLockNamespaceError || { echo "GATEWAY restarted, not listening after 90s"; return 1; }
  echo "GATEWAY met the old lock's fresh heartbeat: one more start after 95 s"; systemctl stop $GW; sleep ${GW_LOCKWAIT:-95}
  systemctl reset-failed $GW 2>/dev/null; systemctl start $GW; n "started $GW again after the old lock's 90 s"
  gwlisten 120 && return 0; echo "GATEWAY started again, not listening after 120s"; return 1; }
gwlisten(){ i=0; while [ $i -lt $1 ]; do cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk -v p=":$GWPORT" '$2 ~ p"$" && $4 == "0A" {f=1} END {exit !f}' && { echo "GATEWAY restarted, listening after ${i}s"; return 0; }; sleep 2; i=$((i+2)); done; return 1; }
# #8180's unit lines a binary bind can't bring (deploy/product/lamp-tasks.service, lamp-control.service), laid as
# /run drop-ins by swap and removed by restore: each run's own cgroup needs lamp-tasks' Delegate (without it every run
# step and try is refused on Linux), and lamp-control mints a made skill's grants with no yes.
UD_TASKS=/run/systemd/system/lamp-tasks.service.d/zz-row-8180-delegate.conf
UD_CONTROL=/run/systemd/system/lamp-control.service.d/zz-row-8180-madecode.conf
HD=/run/systemd/system/lamp-update.service.d/zz-row-8180-hold.conf
