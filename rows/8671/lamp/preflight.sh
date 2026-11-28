#!/bin/sh
# Run as root on lamp-75c9, read-only: sh preflight.sh. Exit 1 when the lamp isn't ready for #8671's real reset.
# Writes only $K/preflight-*.txt and stdout. Never touches lamp-update, Wi-Fi, NetworkManager, wpa or any unit;
# never opens a database file; log lines are counted (logcount.py), never printed.
. "$(dirname "$0")/common.sh"
bad=0
host_ok; root_ok
mkdir -p $K; chmod 700 $K
echo "== preflight for #8671 on $(hostname), $(date '+%F %T %Z'), boot $(boot_id)"
for c in python3 systemd-run flock findmnt sha256sum journalctl; do command -v $c >/dev/null || { echo "REFUSE: no $c"; bad=1; }; done
l=$(journalctl -u lamp-update -o cat --no-pager 2>/dev/null | grep -o 'up to date: build [0-9]*' | tail -1)
echo "BUILD=${l##* }"
[ -n "${EXPECT_BUILD:-}" ] && [ -n "${l##* }" ] && [ "${l##* }" != "$EXPECT_BUILD" ] && { echo "REFUSE: build ${l##* }, not $EXPECT_BUILD"; bad=1; }
st=$(systemctl is-active lamp-update 2>/dev/null || true); echo "lamp-update: $st (only read)"
case "$st" in activating|deactivating|reloading) echo "REFUSE: an update round is going"; bad=1;; esac
ls /opt/lampOS/update/self.json* >/dev/null 2>&1 && { echo "REFUSE: updater self-switch pending"; bad=1; }
# -- set up, running, no reset under way: the reset needs the marker and the target, and starts setup in place
[ -e $MARKER ] && echo "setup marker: present" || { echo "REFUSE: no $MARKER: the lamp is already in setup"; bad=1; }
[ -e $RJOURNAL ] && { echo "REFUSE: a reset journal $RJOURNAL exists (a reset under way?)"; bad=1; }
echo "lampos.target: $(systemctl is-active lampos.target 2>/dev/null)"; systemctl is-active --quiet lampos.target || { echo "REFUSE: lampos.target is not active"; bad=1; }
echo "lamp-setup-reset (watch-reset): $(systemctl is-active lamp-setup-reset 2>/dev/null)"
echo "lamp-setup.service: $(systemctl is-active lamp-setup 2>/dev/null)"
systemctl cat lamp-setup.service >/dev/null 2>&1 && echo "lamp-setup.service: installed (a product lamp: the reset starts setup in place, no reboot)" \
  || echo "NOTE: no lamp-setup.service: the reset reboots instead of starting in place"
systemctl list-units --all --no-legend "$WIPE_UNIT*" 2>/dev/null | grep -q . && { echo "REFUSE: a $WIPE_UNIT unit exists"; bad=1; }
[ -x $STOCK_BIN ] && echo "STOCK_BIN_SHA=$(sha_of $STOCK_BIN) ($(readlink -f $STOCK_BIN))" || { echo "REFUSE: no $STOCK_BIN"; bad=1; }
[ -f $STOCK_PLAN ] && echo "STOCK_PLAN_SHA=$(sha_of $STOCK_PLAN)" || { echo "REFUSE: no $STOCK_PLAN"; bad=1; }
# -- other kits
d=$(find /run/systemd/system -mindepth 2 -maxdepth 2 -type f -name '*.conf' 2>/dev/null); echo "/run drop-ins: ${d:-none}"; [ -n "$d" ] && { echo "REFUSE: /run drop-ins present (another kit's?)"; bad=1; }
m=$(findmnt -rn -o TARGET | grep -E '^/opt/lampOS|^/run/(el|friday|jarvis|privacy)-' || true); [ -n "$m" ] && { echo "REFUSE: kit mounts present: $m"; bad=1; }
o=$(ls /root/*/installed.txt 2>/dev/null || true); [ -n "$o" ] && { echo "REFUSE: a kit install is recorded: $o"; bad=1; }
systemctl list-units --all --no-legend "$DEADMAN*" "$RECORDER*" 2>/dev/null | grep -q . && { echo "REFUSE: a $DEADMAN or $RECORDER unit exists"; bad=1; }
[ -s $K/reset-start.txt ] && { echo "REFUSE: $K/reset-start.txt: this lamp was already reset by this kit (./run.sh after)"; bad=1; }
# -- the network the Mac is on: over wlan0, the reset takes the lamp off it (the plan wipes wpa_supplicant-wlan0.conf,
#    reset::FORGET runs wpa_cli disconnect and remove_network all); only read
sip=$(echo "${SSH_CONNECTION:-}" | awk '{print $3}')
via=$([ -n "$sip" ] && ip -o -4 addr show 2>/dev/null | awk -v a="$sip" '{split($4,x,"/"); if (x[1]==a) print $2}' | head -1)
echo "SSH_VIA=${via:-?} (server address ${sip:-?})"
ip -br link 2>/dev/null | awk '{print "LINK " $1 " " $2}'
[ "$via" = wlan0 ] && echo "NOTE: SSH comes over wlan0: after the reset the lamp is off this network and serves its setup hotspot; the after legs are kept on the lamp by the recorder and read when it is reachable again (Ethernet, the hotspot, or after the lead's re-pairing)"
# -- the logs: ramlog, rsyslog, the eMMC copy (lamp-voice lines counted, never printed)
echo "rsyslog: $(systemctl is-active rsyslog 2>/dev/null)"
echo "orangepi-ramlog: $(systemctl is-active orangepi-ramlog 2>/dev/null) ENABLED=$(sed -n 's/^ENABLED=//p' /etc/default/orangepi-ramlog 2>/dev/null | tr -d '"')"
echo "LOGS_HDD=$(kind_of $LOGS_HDD) COPY_JOURNAL=$(kind_of $COPY_JOURNAL)"
logcount 0 | sed 's/^/BEFORE_ALL /'
logcount 0 | grep '^TEXT dir=/var/log.hdd \|^JOURNAL ' | grep -qE ' voice=[1-9]' || echo "NOTE: no lamp-voice line in /var/log.hdd now: leg 4c has nothing to forget (VOID unless a sync adds some before the reset)"
log_inodes | sed 's/^/LOGFILE /'
# -- the engines' homes (engine names only), the voice's files the PR adds, the KEPT paths
echo "HOMES=$(kind_of $HOMES)"; for h in $(homes); do echo "HOME $h $(kind_of $h)"; done
top_entries | while read -r e; do [ -d "$e" ] && [ ! -L "$e" ] || echo "HOMES_OTHER $e $(kind_of "$e")"; done
for p in $MARKED $MARKS $HFORGET $CLIPS_NEW $CLIPS_OLD; do echo "PATH $p $(kind_of $p)"; done
o=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' '); echo "OLD_CLIPS_ENTRIES=$o"
[ "$o" -gt 0 ] && echo "NOTE: $o entr(ies) in $CLIPS_OLD (the stock voice's clips): leg 2b's PR voice removes them for good; oldclips.sh refuses without CLEAR_OK=1"
t=$(systemctl show -p ActiveEnterTimestamp --value $UNIT)
[ "$(unit_exec_of $UNIT | awk '{print $1}')" = "$VSTOCK" ] || { echo "REFUSE: lamp-voice's ExecStart is not $VSTOCK"; bad=1; }
journalctl -u $UNIT --since "$t" -o cat --no-pager | grep -qF "$READY" && echo "voice listening_ready (since $t)" || { echo "REFUSE: voice not listening_ready"; bad=1; }
journalctl -u $UNIT --since "$t" -o cat --no-pager | grep -qF "$MUTE" && { echo "REFUSE: voice muted by the restart guard"; bad=1; }
for p in $KEPT_PATHS; do echo "KEPT $p $(kind_of $p)"; done
f=$(find $HOMES $MARKED $CLIPS_NEW $CLIPS_OLD -maxdepth 2 -name "$MNAME" 2>/dev/null; [ -e $STRAY ] && echo $STRAY)
[ -n "$f" ] && { echo "REFUSE: kit markers already on the lamp: $f (./run.sh putback)"; bad=1; }
# -- the history switches (#8125 is not in this PR's lamp-setup: no account delete at this reset)
p=$(systemctl show -p MainPID --value lamp-voice)
for v in LAMP_HISTORY_SEND LAMP_HISTORY_FORGET LAMP_OFFLINE_RINGS LAMP_MARK_WEIRD; do
  echo "VOICE_ENV $v=$(tr '\0' '\n' < /proc/$p/environ 2>/dev/null | sed -n "s/^$v=//p")"; done
echo "free on /run: $(df -h /run | awk 'NR==2{print $4}')"
n=$(find $K -name '._*' 2>/dev/null | wc -l); echo "._ files: $n"; [ "$n" = 0 ] || { echo "REFUSE: ._ files in the kit"; bad=1; }
[ $bad = 0 ] && echo PREFLIGHT-OK || echo PREFLIGHT-REFUSED
exit $bad
