#!/usr/bin/env bash
# st4863-v10-launch-dry.sh: the LAUNCH DRY of kit/st4863-v10.sh. It runs the EXACT production command line, from the
# launch dir (`cd ~/seventh-watch && LIMIT=HHMM bash kit/st4863-v10.sh phase0`, and `... ARMS=A ... go`), through the
# kit's real path: go -> lock -> PLAYING -> nohup fork of run -> preflight -> setup -> phase0 [-> arm A] -> fin -> QUIET.
# Only the outside world is stubbed, on the kit's PATH: ssh, gh, ping; and HOME (a scratch home whose seventh-watch/kit
# is a link to this kit folder and whose seventh-watch/lamp-lock.sh is a fake lock, the kit's default ST_LOCK path), so
# nothing lands in the real ~/seventh-watch. The kit has no LAUNCH_DRY branch: it cannot tell this from the lamp.
#
# The fake lamp: ssh runs each command under dash on a fake root (ROOT=, SIM=1, the kit's own fakebin() tools, as the
# DRY's SIM harness does), so st.sh really executes. The lamp's absolute paths in the kit's command text are put under
# the fake root (quoted heredoc bodies and a ROOT-aware script on stdin pass as they are) and taken out of the output
# again. Added lamp behaviour: sudo, systemd-run (--wait, --on-active, append: output), a reboot (5 s off the network,
# a new boot_id, /run's units gone, timers stopped, the journal and the monotonic clock new, the LAN watcher started,
# then the boot leg: the guard service's ExecStart run after GUARD_REAL s), recover's journal lines (watching, the
# hotspot on a cut, the close on its stop). The lamp's monotonic clock runs GUARD_MIN*60/GUARD_REAL times real time
# (the boot leg at boot + 300 s lamp time after 60 s real); the kit's own clock and sleeps are real (no knob).
# Not simulated: the pet leg firing, Restart=on-failure, killing an old boot's processes at a reboot, /proc/uptime.
#
# Usage: kit/st4863-v10-launch-dry.sh [phase0|go|neg ...]   (default: neg phase0 go). Exit 0 when every case passes.
set -uo pipefail
KD=$(dirname "$(realpath "$0")"); KIT=$KD/st4863-v10.sh
GUARD_MIN=5; GUARD_REAL=${GUARD_REAL:-60}; SCALE=$(( GUARD_MIN*60 / GUARD_REAL ))
GU=seventh-st4863-guard; GP=seventh-st4863-pet; GW=seventh-st4863-lan; D=/var/lib/seventh-st4863
WLK=/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service
CLOSE='recover: the hotspot closed: back to the station'
REPAIR='recover: the radio was left a hotspot: back to the station'
ORIG_PATH=$PATH; FAILS=0
eval "$(sed -n '/^fakebin() {/,/^  chmod +x "\$1"\/\*; }$/p' "$KIT")"   # the kit's own lamp-tool fakes
type fakebin >/dev/null || { echo "fakebin() not found in $KIT"; exit 2; }

# mk FILE: a script whose first lines pin this world's paths, then its body from stdin
mk() { { echo '#!/bin/bash'; echo "W=$W; R=$W/root; FW=$W/fw; SCALE=$SCALE; GUARD_REAL=$GUARD_REAL; GU=$GU; GW=$GW; GP=seventh-st4863-pet; PET_REAL=${PET_REAL:-300}; LAMPPATH=\"$W/lampbin:$W/fakebin:$ORIG_PATH\"; WLK=/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service"; cat; } > "$1"; chmod +x "$1"; }

world() { W=$(mktemp -d "${TMPDIR:-/tmp}/st4863-launch.XXXXXX"); local R=$W/root FW=$W/fw
  mkdir -p "$R" "$FW" "$W/macbin" "$W/lampbin" "$W/home/seventh-watch"; fakebin "$W/fakebin"; : > "$W/calls"
  # ---- the fake root: a set-up 52e6 as a station (the DRY's root + the SIM's systemd views)
  mkdir -p "$R/etc/hostapd" "$R/etc/wpa_supplicant" "$R/etc/systemd/system" "$R/var/lib/lampos-setup" "$R/var/lib/lampos-update" \
    "$R/run/lampos-bus/voice/mute" "$R/run/systemd/system" "$R/usr/local/bin" "$R/proc/sys/kernel/random" "$R/proc/4242" "$R/home/orangepi"
  printf 'hostname lamp\ninterface eth0\n' > "$R/etc/dhcpcd.conf"; echo 'ssid=lamp-52e6' > "$R/etc/hostapd/hostapd.conf"
  mkdir -p "$R/etc/dnsmasq.d"; printf 'interface=wlan0\ndhcp-range=192.168.100.10,192.168.100.50,12h\n' > "$R/etc/dnsmasq.d/99-lamp.conf"
  echo "${FIX:-1}" > "$W/fix"; echo "${IWFAIL_BOOT:-0}" > "$W/iwfail_boot"
  echo '127.0.1.1 lamp-52e6' > "$R/etc/hosts"; echo lamp-52e6 > "$R/etc/hostname"; printf 'network={\n\tssid="x"\n}\n' > "$R/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
  printf 'installed-9582\nrecover: the radio was left a hotspot: back to the station\n%s\n' "$CLOSE" > "$W/ls"
  XR=$(shasum -a 256 < "$W/ls" | cut -c1-64); mkdir -p "$R/opt/lampOS/setup/$XR"; mv "$W/ls" "$R/opt/lampOS/setup/$XR/lamp-setup"; ln -s "$XR" "$R/opt/lampOS/setup/current"
  : > "$R/var/lib/lampos-setup/done"; echo '{"installed":{"sequence":9582,"commit":"ab6ef495ce1234"}}' > "$R/var/lib/lampos-update/state.json"
  echo '{"muted":false}' > "$R/run/lampos-bus/voice/mute/latest"
  printf '[Service]\nExecStart=/opt/lampOS/setup/current/lamp-setup recover --config /etc/lampOS/setup/serve.json --enable\n' > "$R/etc/systemd/system/lamp-setup-recover.service"
  mkdir -p "$R${WLK%/*}"; ln -s /lib/systemd/system/wpa_supplicant@.service "$R$WLK"
  cp "$R/opt/lampOS/setup/current/lamp-setup" "$R/proc/4242/exe"; uuidgen | tr A-Z a-z > "$R/proc/sys/kernel/random/boot_id"
  sed -n '/^#!\/bin\/bash$/,/^echo "STA MODE ENABLED"$/p' "$KIT" > "$R/usr/local/bin/device-sta-mode"; chmod +x "$R/usr/local/bin/device-sta-mode"
  echo "10:00 seventh: an earlier loan line" > "$R/loans.txt"
  # ---- the fake lamp's state (fakebin's files)
  for u in wpa_supplicant@wlan0:enabled:active hostapd:disabled:inactive dnsmasq:disabled:inactive dhcpcd:enabled:active avahi-daemon:enabled:active \
    nginx:disabled:inactive lamp-setup-recover:enabled:active lamp-setup:enabled:inactive lamp-voice:enabled:active lamp-eyes:enabled:active \
    lamp-controls:enabled:active lamp-body:enabled:active; do IFS=: read -r n e a <<< "$u"; echo "$e" > "$FW/en.$n"; echo "$a" > "$FW/act.$n"; done
  echo managed > "$FW/type"; echo up > "$FW/link"; echo 192.168.1.50/24 > "$FW/lan"; echo 00 > "$FW/reg"; echo lamp-52e6 > "$FW/hostname"; echo 0 > "$FW/iw_fail"; touch "$FW/net_ok"
  printf '{ path=/opt/lampOS/setup/current/lamp-setup ; argv[]=/opt/lampOS/setup/current/lamp-setup recover --config /etc/lampOS/setup/serve.json --enable ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n' > "$FW/show.ExecStart"
  echo loaded > "$FW/show.LoadState"; echo 4242 > "$FW/show.MainPID"; : > "$FW/show.Environment"; : > "$FW/show.DropInPaths"
  perl -MTime::HiRes=time -e 'printf "%.6f\n", time - 200' > "$FW/boot_t0"   # this boot began 1000 s ago, lamp time
  # ---- lamp side: the clock, the path rewrite, the runner, and the tools added to fakebin's
  mk "$W/lampclock" <<'F'
perl -MTime::HiRes=time -e 'printf "%.6f\n", (time - $ARGV[0]) * $ARGV[1]' "$(cat "$FW/boot_t0")" "$SCALE"
F
  mk "$W/jline" <<'F'
# jline UNIT TAG MSG: one journal line, stamped with the lamp's monotonic clock (journalctl -o short-monotonic's shape)
printf '[%12s] lamp-52e6 %s[812]: %s\n' "$("$W/lampclock")" "$2" "$3" >> "$FW/journal.$1"
F
  mk "$W/rw" <<'F'
# rw: the lamp's absolute paths put under the fake root, outside quoted heredoc bodies; a ROOT-aware script passes as is
exec perl -e 'my $a = do { local $/; <STDIN> }; my ($end, @o, $aware);
  for (split /(?<=\n)/, $a) { if (defined $end) { push @o, $_; $end = undef if /^\Q$end\E\n?$/; next }
    $aware = 1 if /^R=\$\{ROOT:-\}$/; s{(^|[\s"'"'"'=:;(])(/(?:var/lib|etc|run|opt|proc|usr/local)/)}{$1$ARGV[0]$2}g;
    $end = $1 if /<<'"'"'(\w+)'"'"'/; push @o, $_ }
  print $aware ? $a : join "", @o' "$R"
F
  mk "$W/onlamp" <<F
# onlamp [-i] CMD: CMD as the lamp runs it (dash; the lamp's PATH: the added tools, fakebin, then the Mac's). -i: stdin
# is a script for it (rewritten). The fake root's prefix is taken out of the output (the lamp's own view).
export ROOT=\$R FW=\$FW SIM=1 HOME=\$R/home/orangepi PATH="\$W/lampbin:\$W/fakebin:$ORIG_PATH"
i=; [ "\$1" = -i ] && { i=1; shift; }
c=\$(printf '%s' "\$*" | "\$W/rw"); e=\$W/err.\$\$; in=/dev/null
[ -z "\$i" ] || { in=\$W/stdin.\$\$; "\$W/rw" > "\$in"; }
dash -c "\$c" < "\$in" 2> "\$e" | perl -pe 'BEGIN{\$r=shift} s{\\Q\$r\\E}{}g' "\$R"; rc=\${PIPESTATUS[0]}
perl -pe 'BEGIN{\$r=shift} s{\\Q\$r\\E}{}g' "\$R" "\$e" >&2; rm -f "\$e" "\$W/stdin.\$\$"; exit \$rc
F
  mk "$W/lampbin/sh" <<'F'
exec dash "$@"
F
  mk "$W/lampbin/sudo" <<'F'
[ "$1" = -n ] && shift; exec "$@"
F
  mk "$W/lampbin/systemd-run" <<'F'
# the forms the kit uses: --unit= --wait --on-active=N -p StandardOutput|StandardError=append:FILE -q --collect CMD...
u=run-$$; w=; on=; out=
while [ $# -gt 0 ]; do case $1 in --unit=*) u=${1#*=} ;; --wait) w=1 ;; --on-active=*) on=${1#*=} ;;
  -p) case $2 in StandardOutput=append:*) out=${2#*append:} ;; esac; shift ;; -q|--collect) ;; *) break ;; esac; shift; done
[ "$1" = /bin/sh ] && { shift; set -- dash "$@"; }
echo "systemd-run unit=$u wait=$w on=$on $*" >> "$FW/calls"; out=${out:-$FW/journal.$u}
if [ -n "$w" ]; then echo active > "$FW/act.$u"; "$@" >> "$out" 2>&1; rc=$?; echo inactive > "$FW/act.$u"; exit $rc; fi
( [ -z "$on" ] || /bin/sleep "$on"; echo active > "$FW/act.$u"; "$@" >> "$out" 2>&1; echo inactive > "$FW/act.$u" ) < /dev/null > /dev/null 2>&1 &
F
  mk "$W/lampbin/systemctl" <<'F'
case "$*" in reboot*|"--no-block reboot"*) echo "systemctl $*" >> "$FW/calls"; echo REBOOT >> "$FW/calls"
  nohup "$W/fakereboot" < /dev/null > /dev/null 2>&1 9>&- & exit 0 ;; esac
"$W/fakebin/systemctl" "$@"; rc=$?
case "$*" in "restart lamp-setup-recover"|"start lamp-setup-recover") "$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: watching" ;;
  "restart seventh-st4863-pet.timer") g=$(( $(cat "$FW/petgen" 2>/dev/null || echo 0) + 1 )); echo $g > "$FW/petgen"
    nohup "$W/petleg" $g "$(cut -c1-8 "$R/proc/sys/kernel/random/boot_id")" < /dev/null > /dev/null 2>&1 9>&- &
    echo "t $(perl -e 'printf "%d", ($ARGV[0] + 300) * 1e6' "$("$W/lampclock")")" > "$FW/bus.seventh-st4863-pet.timer.NextElapseUSecMonotonic" ;; esac
exit $rc
F
  mk "$W/lampbin/wpa_cli" <<'F'
"$W/fakebin/wpa_cli" "$@"; rc=$?
# recover (watching) sees the network gone and opens setup's hotspot
case "$*" in *disable_network*) [ "$(cat "$FW/act.lamp-setup-recover")" = active ] && { "$W/apmode"; "$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: off every known network for 90 s and not joining one: opening setup's hotspot"; } ;; esac
exit $rc
F
  mk "$FW/hook.stop.lamp-setup-recover" <<F
# recover stopped with its hotspot open: the fix (FIX=1) closes it by device-sta-mode; FIX=0: left a hotspot (the bug)
[ -e "\$FW/hot" ] || exit 0; [ "\$(cat "\$W/fix")" = 1 ] || exit 0
if "\$W/station"; then "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: $CLOSE"; else "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: the hotspot closed and the station could not start: exit"; fi
F
  mk "$W/apmode" <<'F'
# device-ap-mode's lasting changes (scripts/imager/build-orangepi-v2.sh): wpa stopped, disabled (wants link gone),
# masked; denyinterfaces wlan0 first in dhcpcd.conf; the address line in 99-lamp.conf; hostapd/dnsmasq unmasked,
# enabled, started; nginx restarted; reg US; wlan0 an AP with 192.168.100.1. Suffix 52e6: hosts/hostname/ssid same.
echo inactive > "$FW/act.wpa_supplicant@wlan0"; echo masked > "$FW/en.wpa_supplicant@wlan0"; rm -f "$R$WLK"
grep -q '^denyinterfaces wlan0$' "$R/etc/dhcpcd.conf" || { { echo 'denyinterfaces wlan0'; cat "$R/etc/dhcpcd.conf"; } > "$R/etc/dhcpcd.conf.n"; cat "$R/etc/dhcpcd.conf.n" > "$R/etc/dhcpcd.conf"; rm "$R/etc/dhcpcd.conf.n"; }
grep -q '^address=/#/' "$R/etc/dnsmasq.d/99-lamp.conf" || echo 'address=/#/192.168.100.1' >> "$R/etc/dnsmasq.d/99-lamp.conf"
for u in hostapd dnsmasq; do echo enabled > "$FW/en.$u"; echo active > "$FW/act.$u"; done; echo active > "$FW/act.nginx"
echo US > "$FW/reg"; echo AP > "$FW/type"; : > "$FW/lan"; touch "$FW/hot"; echo "apmode $(date +%T)" >> "$W/calls"
F
  mk "$W/station" <<'F'
# recover's host.station(): the image's device-sta-mode (the kit's fake of it, set -e, iw may fail), then what the
# real one also does that the fake does not: `ip addr flush` (the AP address) and enable's wants link
export ROOT=$R FW=$FW SIM=1 PATH=$LAMPPATH
"$R/usr/local/bin/device-sta-mode" >> "$W/sta.out" 2>&1; rc=$?; echo "station rc=$rc $(date +%T)" >> "$W/calls"
[ $rc = 0 ] || exit $rc
rm -f "$FW/hot"; [ "$(cat "$FW/en.wpa_supplicant@wlan0")" = enabled ] && [ ! -L "$R$WLK" ] && ln -s /lib/systemd/system/wpa_supplicant@.service "$R$WLK"
exit 0
F
  mk "$W/bootrecover" <<F
# lamp-setup-recover at boot (~10 s lamp time): the fix's at_start repair (FIX=1) when the radio was left a hotspot,
# then the switch: on (B's /etc drop-in) = watching (and, no network for 90 s, its hotspot again); off = says so, exits
export ROOT=\$R FW=\$FW SIM=1 PATH=\$LAMPPATH; b=\$1
lah() { [ "\$(cat "\$FW/en.wpa_supplicant@wlan0")" = masked ] || [ ! -L "\$R\$WLK" ] || grep -q '^denyinterfaces wlan0\$' "\$R/etc/dhcpcd.conf"; }
if [ -e "\$R/var/lib/lampos-setup/done" ] && lah && [ "\$(cat "\$W/fix")" = 1 ]; then
  if "\$W/station"; then "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: $(echo "$REPAIR")"
  else "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: the radio was left a hotspot and the station could not start: exit; the next boot tries again if it is still one"; fi; fi
if grep -qs 'LAMP_WIFI_RECOVERY=on' "\$R"/etc/systemd/system/lamp-setup-recover.service.d/*.conf; then
  echo active > "\$FW/act.lamp-setup-recover"; "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: watching wlan0"
  /bin/sleep \$(( 90 / SCALE ))
  [ "\$(cut -c1-8 "\$R/proc/sys/kernel/random/boot_id")" = "\$b" ] && [ "\$(cat "\$FW/act.lamp-setup-recover")" = active ] && [ -z "\$(cat "\$FW/lan")" ] && [ ! -e "\$FW/hot" ] && {
    "\$W/apmode"; "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: off every known network for 90 s and not joining one: opening setup's hotspot"; }
else echo inactive > "\$FW/act.lamp-setup-recover"; "\$W/jline" lamp-setup-recover lamp-setup "lamp-setup: recover: off (LAMP_WIFI_RECOVERY is not on)"; fi
F
  mk "$W/petleg" <<'F'
# the pet leg: OnActiveSec from its last restart; fires the guard service PET_REAL s later unless restarted, stopped
# or a reboot came between
g=$1 b=$2; /bin/sleep "$PET_REAL"
[ "$(cat "$FW/petgen")" = "$g" ] && [ "$(cut -c1-8 "$R/proc/sys/kernel/random/boot_id")" = "$b" ] && [ "$(cat "$FW/act.$GP.timer" 2>/dev/null)" = active ] || exit 0
echo "t $(perl -e 'printf "%d", $ARGV[0] * 1e6' "$("$W/lampclock")")" > "$FW/bus.$GP.timer.LastTriggerUSecMonotonic"
[ -e "$R/etc/systemd/system/$GU.service" ] || exit 0
x=$(sed -n 's/^ExecStart=//p' "$R/etc/systemd/system/$GU.service" | sed 's|^/bin/sh |sh |'); echo "pet leg $b $(date +%T): $x" >> "$W/calls"
echo activating > "$FW/act.$GU.service"
"$W/onlamp" "$x" 2>&1 | while IFS= read -r l; do "$W/jline" "$GU.service" sh "$l"; done
echo inactive > "$FW/act.$GU.service"; echo "pet leg done $(date +%T) lan=$(cat "$FW/lan")" >> "$W/calls"
F
  mk "$W/fakereboot" <<'F'
touch "$FW/down"; /bin/sleep 5
uuidgen | tr A-Z a-z > "$R/proc/sys/kernel/random/boot_id"; b=$(cut -c1-8 "$R/proc/sys/kernel/random/boot_id")
ls "$R/etc/systemd/system" | grep seventh-st4863 > "$W/units-at-boot-$b"
rm -rf "$R/run/systemd/system"; mkdir -p "$R/run/systemd/system"; rm -f "$FW"/journal.* "$FW"/bus.* "$FW/hot"
for f in "$FW"/act.*.timer "$FW"/act.seventh-st4863-*; do [ -e "$f" ] && echo inactive > "$f"; done
# the boot: units by their enable state; wlan0 managed unless hostapd starts; a LAN address only for a station
if [ "$(cat "$FW/en.wpa_supplicant@wlan0")" = enabled ] && [ -L "$R$WLK" ]; then echo active > "$FW/act.wpa_supplicant@wlan0"; else echo inactive > "$FW/act.wpa_supplicant@wlan0"; fi
for u in hostapd dnsmasq nginx; do [ "$(cat "$FW/en.$u")" = enabled ] && echo active > "$FW/act.$u" || echo inactive > "$FW/act.$u"; done
echo 00 > "$FW/reg"; echo up > "$FW/link"; [ "$(cat "$FW/en.hostapd")" = enabled ] && echo AP > "$FW/type" || echo managed > "$FW/type"
: > "$FW/lan"; [ "$(cat "$FW/act.wpa_supplicant@wlan0")" = active ] && [ "$(cat "$FW/type")" = managed ] && ! grep -q '^denyinterfaces wlan0$' "$R/etc/dhcpcd.conf" && [ -f "$FW/net_ok" ] && echo 192.168.1.50/24 > "$FW/lan"
echo inactive > "$FW/act.lamp-setup-recover"; echo "$(cat "$W/iwfail_boot")" > "$FW/iw_fail"
echo "boot-state $b lan=$(cat "$FW/lan") type=$(cat "$FW/type") wpa=$(cat "$FW/en.wpa_supplicant@wlan0")" >> "$W/calls"
( /bin/sleep 2; "$W/bootrecover" "$b"; echo "after-recover $b lan=$(cat "$FW/lan") type=$(cat "$FW/type")" >> "$W/calls" ) &
perl -MTime::HiRes=time -e 'printf "%.6f\n", time' > "$FW/boot_t0"; rm -f "$FW/down"; echo "boot $b" >> "$W/calls"
# a unit's ExecStart as the lamp runs it: /bin/sh is dash there
x() { sed -n 's/^ExecStart=//p' "$R/etc/systemd/system/$1" | sed 's|^/bin/sh |sh |'; }
[ "$(cat "$FW/en.$GW.service" 2>/dev/null)" = enabled ] && { "$W/onlamp" "$(x $GW.service)" > /dev/null 2>&1 & }
[ "$(cat "$FW/en.$GU.timer" 2>/dev/null)" = enabled ] || exit 0
echo active > "$FW/act.$GU.timer"; /bin/sleep "$GUARD_REAL"
[ "$(cut -c1-8 "$R/proc/sys/kernel/random/boot_id")" = "$b" ] && [ "$(cat "$FW/en.$GU.timer" 2>/dev/null)" = enabled ] || exit 0
# the boot leg: its trigger time (busctl's raw usec), then the guard service's own ExecStart, its lines in its journal
echo "t $(perl -e 'printf "%d", $ARGV[0] * 1e6' "$("$W/lampclock")")" > "$FW/bus.$GU.timer.LastTriggerUSecMonotonic"
x=$(x $GU.service); echo "boot leg $b: $x" >> "$W/calls"
echo activating > "$FW/act.$GU.service"
"$W/onlamp" "$x" 2>&1 | while IFS= read -r l; do "$W/jline" "$GU.service" sh "$l"; done; rc=${PIPESTATUS[0]}
[ "$rc" = 0 ] && echo success > "$FW/show.$GU.service.Result" || echo exit-code > "$FW/show.$GU.service.Result"
echo inactive > "$FW/act.$GU.service"; cp "$FW/journal.$GU.service" "$W/bootleg-$b"
F
  # ---- Mac side: ssh, ping, gh, and the lock at the kit's default path
  mk "$W/macbin/ssh" <<'F'
n=; while [ $# -gt 0 ]; do case $1 in -n) n=1; shift ;; -o|-i|-p|-l|-S) shift 2 ;; -*) shift ;; *) break ;; esac; done
shift; echo "ssh $*" | cut -c1-160 >> "$W/calls"
[ -e "$W/sshdown" ] && { echo "ssh: dropped (injected)" >&2; sleep 1; exit 255; }
# v10: the put-back's start lost once (the ssh fails before the unit begins)
case "$*" in *"putback ifneeded"*) [ -e "$W/pblose" ] && { rm -f "$W/pblose"; echo "PBSTART LOST $(date +%T)" >> "$W/calls"; echo "ssh: dropped (injected)" >&2; exit 255; } ;; esac
[ -e "$FW/down" ] && { echo "ssh: connect to host lamp-52e6.local port 22: Host is down" >&2; exit 255; }
if [ -n "$n" ]; then exec "$W/onlamp" "$*"; else exec "$W/onlamp" -i "$*"; fi
F
  mk "$W/macbin/ping" <<'F'
echo "ping $*" >> "$W/calls"; [ -e "$FW/down" ] && exit 2; exit 0
F
  mk "$W/macbin/gh" <<'F'
echo "gh $*" | tr '\n' ' ' >> "$W/calls"; echo >> "$W/calls"; exit 0
F
  mk "$W/home/seventh-watch/lamp-lock.sh" <<'F'
echo "lock $*" >> "$W/calls"
# v10: lock.stolen: another session took the lock (lamp-lock: renew 1, release refuses and prints the lock)
[ -e "$W/lock.stolen" ] && case $2 in renew) exit 1 ;; release) printf 'refusing: lamp-52e6 is not held by seventh (session %s):\n  holder: fifth\n  session: st-77\n' "$4" >&2; exit 1 ;; esac
case $2 in status) [ ! -e "$W/lock.held" ] ;; acquire) echo "$4" > "$W/lock.held" ;; release) [ "$(cat "$W/lock.held" 2>/dev/null)" = "$4" ] && rm -f "$W/lock.held"; exit 0 ;; *) exit 0 ;; esac
F
}
# snap: the fake root's every path, mode, link target and content hash (not /proc: the kernel's; not loans.txt: compared apart)
snap() { (cd "$W/root" && find . -path ./proc -prune -o -path ./loans.txt -prune -o -print | sort | while IFS= read -r f; do
  if [ -L "$f" ]; then echo "L $f -> $(readlink "$f")"; elif [ -d "$f" ]; then echo "D $f $(stat -f %Lp "$f")"
  else echo "F $f $(stat -f %Lp "$f") $(shasum -a 256 < "$f" | cut -c1-16)"; fi; done); }
ok() { if eval "$2"; then echo "  PASS  $1"; else echo "  FAIL  $1"; FAILS=$((FAILS+1)); fi; }

# launch CASE KITDIR CMDLINE: the world, the exact command line from the launch dir, then the run's files
launch() { world; ln -s "$2" "$W/home/seventh-watch/kit"; snap > "$W/root.before"; cp "$W/root/loans.txt" "$W/loans.before"
  echo "== $1 $(date +%T): HOME=<scratch home> PATH=<ssh gh ping fakes>:\$PATH zsh -c '$3'"
  # the fakes, checked before the run: ssh, gh, ping and the lock resolve to this world's, never the real ones
  local fk; fk=$(env HOME="$W/home" PATH="$W/macbin:$ORIG_PATH" /bin/zsh -fc 'for t in ssh gh ping; do whence -p $t; done; echo ~/seventh-watch/lamp-lock.sh' | tr '\n' ' ')
  case "$fk" in "$W/macbin/ssh $W/macbin/gh $W/macbin/ping $W/home/seventh-watch/lamp-lock.sh ") echo "  fakes ok: $fk" ;; *) echo "  FAKES NOT IN PLACE: $fk: not run"; FAILS=$((FAILS+1)); return 1 ;; esac
  ( env HOME="$W/home" PATH="$W/macbin:$ORIG_PATH" /bin/zsh -fc "$3" > "$W/go.out" 2>&1; echo $? > "$W/go.rc" ) & local p=$! t=0
  [ -z "${INJ:-}" ] || { LP=$p "$INJ" > "$W/inject.out" 2>&1 & }
  while kill -0 $p 2>/dev/null; do sleep 5; t=$((t+5)); [ $t -ge ${LTO:-1200} ] && { echo "  (${LTO:-1200} s: killed)"; pkill -P $p; kill $p; }; done
  LOGF=$(ls "$W/home/seventh-watch"/st4863-*.txt 2>/dev/null | head -1)
  echo "  go exit $(cat "$W/go.rc") after $t s; LOG ${LOGF##*/}; the kit's own output (go's tail of LOG), its decision lines:"
  grep -E '^== |ABORT|build ok|SETUP OK|SELFTEST (ARMED|NOT)|phase 0:|ST4863|PUTBACK|PUT BACK|guard [0-9]|lamp-voice not|REMOVED|arm A' "$W/go.out" | cut -c1-190 | sed 's/^/    | /'; }

# common asserts after a run that must pass
asserts() { local LOGF=$1 nreb=$2
  ok "the run started (no 127): LOG has the run's banner, QUIET says exit 0" "grep -q '^== st4863' '$LOGF' && grep -q 'QUIET lamp-52e6 .*(exit 0)' '$W/calls'"
  ok "preflight passed (build ok, no ABORT)" "grep -q '^build ok: recover runs /opt/lampOS/setup/current/lamp-setup' '$LOGF' && ! grep -q 'ABORT' '$LOGF'"
  ok "setup installed st.sh and the 4 units in the fake root (seen at the reboot): $(cat "$W"/units-at-boot-* 2>/dev/null | tr '\n' ' ')" "grep -q '^SETUP OK' '$LOGF' && [ \"\$(cat '$W'/units-at-boot-* | sort -u | wc -l | tr -d ' ')\" = 4 ]"
  ok "the lamp rebooted $nreb time(s): $(grep -c '^REBOOT' "$W/fw/calls")" "[ \"\$(grep -c '^REBOOT' '$W/fw/calls')\" = $nreb ]"
  ok "the boot leg ran the guard's ExecStart and logged PUTBACK OK (guard): $(grep -h -m1 'PUTBACK OK (guard)' "$W"/bootleg-* 2>/dev/null | cut -c1-90)" "grep -q 'PUTBACK OK (guard)' '$W'/bootleg-*"
  ok "phase0 PASS: $(grep -m1 'ST4863 phase0' "$LOGF" | cut -c1-150)" "grep -q '^ST4863 phase0 PASS' '$LOGF'"
  ok "fin put back and disarmed: $(grep -m1 '^PUTBACK OK: files equal' "$LOGF" | cut -c1-110)" "grep -q '^REMOVED /var/lib/seventh-st4863' '$LOGF' && grep -q '^PUTBACK OK: files equal' '$LOGF' && grep -q '^PUT BACK' '$LOGF'"
  snap > "$W/root.after"; diff "$W/root.before" "$W/root.after" > "$W/root.diff"
  ok "the fake root ended byte-identical (paths, modes, links, contents)$( [ -s "$W/root.diff" ] && echo ": $(tr '\n' ' ' < "$W/root.diff" | cut -c1-200)")" "[ ! -s '$W/root.diff' ]"
  ok "loans.txt: the earlier lines kept, $(( $(wc -l < "$W/root/loans.txt") - 1 )) line(s) added" "[ \"\$(head -1 '$W/root/loans.txt')\" = \"\$(cat '$W/loans.before')\" ]"
  ok "the lock: acquired, released, not held: $(grep -oE '^lock lamp-52e6 (acquire|release)' "$W/calls" | cut -d' ' -f3 | tr '\n' ' ')" "grep -q '^lock lamp-52e6 acquire' '$W/calls' && grep -q '^lock lamp-52e6 release' '$W/calls' && [ ! -e '$W/lock.held' ]"
  local lp lq; lp=$(grep -n 'gh issue comment 3336 .*PLAYING lamp-52e6' "$W/calls" | cut -d: -f1); lq=$(grep -n 'gh issue comment 3336 .*QUIET lamp-52e6' "$W/calls" | cut -d: -f1)
  ok "PLAYING (call ${lp:-none}) then QUIET (call ${lq:-none}), one each: $(grep -o 'QUIET lamp-52e6.*' "$W/calls" | cut -c1-160)" "[ -n '$lp' ] && [ -n '$lq' ] && [ '$lp' -lt '$lq' ] && [ \$(grep -c 'gh issue comment' '$W/calls') = 2 ]"
}

# ---- injections (run beside the kit, on this world)
waitfor() { local i=0; until grep -q "$1" "$W/fw/calls" "$W/calls" 2>/dev/null; do sleep 1; i=$((i+1)); [ $i -ge 1500 ] && return 1; done; return 0; }
# killmac: every Mac-side process of this world dies at once (SIGKILL): the processes started with this world's HOME;
# the lamp's (onlamp exports the lamp's HOME) live on
killmac() { local ps; ps=$(ps -axww -o pid=,ppid=,command= | python3 -I -c '
import sys
w, root = sys.argv[1], int(sys.argv[2]); kids = {}; cmd = {}
for l in sys.stdin:
    f = l.split(None, 2)
    if len(f) < 3: continue
    p, pp = int(f[0]), int(f[1]); kids.setdefault(pp, []).append(p); cmd[p] = f[2]
out = []
def walk(p):
    for c in kids.get(p, []):
        out.append(c)
        # an ssh in flight dies with the Mac; what it runs on the lamp lives on (not walked)
        if w + "/macbin/ssh" in cmd[c] or w + "/onlamp" in cmd[c]: continue
        walk(c)
walk(root); print(" ".join(map(str, out)))' "$W" "$LP")
  echo "killmac $(date +%T): $(echo $ps | wc -w) processes"; kill -9 $ps 2>/dev/null; echo "MACDEAD $(date +%T)" >> "$W/calls"; }
# v10: the lock taken by another session once B's window starts (52e6 off the network); a put-back start lost
inj_steal() { waitfor 'unit=seventh-st4863-arm-B' && touch "$W/lock.stolen" && echo "LOCK STOLEN $(date +%T)" >> "$W/calls"; }
inj_pblose() { touch "$W/pblose"; }
inj_killA() { waitfor 'unit=seventh-st4863-arm-A' && sleep 3 && killmac; }
inj_killB() { waitfor 'unit=seventh-st4863-arm-B' && sleep 1 && killmac; }
inj_ssh() { waitfor 'unit=seventh-st4863-arm-A' && touch "$W/sshdown" && echo "ssh down $(date +%T)" >> "$W/calls" && sleep "${SSHDOWN:-600}"; rm -f "$W/sshdown"; echo "ssh up $(date +%T)" >> "$W/calls"; }
# a build switch in B's window: after B's reboot, lamp-update re-points setup's current to a new release folder
inj_build() { waitfor 'unit=seventh-st4863-arm-B' && waitfor '^boot-state' && sleep 1
  local R=$W/root n; printf 'installed-9583\n' > "$W/nb"; n=$(shasum -a 256 < "$W/nb" | cut -c1-64); mkdir -p "$R/opt/lampOS/setup/$n"; mv "$W/nb" "$R/opt/lampOS/setup/$n/lamp-setup"
  ln -sfn "$n" "$R/opt/lampOS/setup/current"; echo "lamp-update[9]: switching to build 9583" >> "$W/fw/journal.lamp-update"; echo "BUILD SWITCHED $(date +%T)" >> "$W/calls"; }
# lampok: the fake lamp's own view now: a clean station with a LAN address
lampok() { "$W/onlamp" "sh /var/lib/seventh-st4863/st.sh state 2>/dev/null || true" 2>/dev/null | tail -1; }
# after a dead Mac: wait for the lamp to be a station by itself, then run `back` and check the root
afterdeath() { local i=0 s; while :; do s=$(env ROOT=$W/root FW=$W/fw PATH="$W/fakebin:$ORIG_PATH" sh -c '. /dev/null; l=$(cat $FW/lan); t=$(cat $FW/type); w=$(cat $FW/en.wpa_supplicant@wlan0); h=$(cat $FW/en.hostapd); echo "lan=$l type=$t wpa=$w hostapd=$h"')
    case $s in "lan=192.168.1.50/24 type=managed wpa=enabled hostapd=disabled") break ;; esac; sleep 5; i=$((i+5)); [ $i -ge ${1:-900} ] && break; done
  echo "  lamp by itself $i s after the Mac died: $s"; ok "the lamp came back on its LAN by itself, no Mac: $s" "[ '$s' = 'lan=192.168.1.50/24 type=managed wpa=enabled hostapd=disabled' ]"
  grep -E 'MACDEAD|pet leg|boot leg|boot-state|after-recover|station rc|apmode|REBOOT' "$W/calls" "$W/fw/calls" 2>/dev/null | sed 's/^[^:]*://' | grep -E 'MACDEAD|pet leg|boot leg|boot-state|after-recover|station rc|apmode' | cut -c1-140 | sed 's/^/    ~ /'
  sleep 20; echo "  now: kit/st4863-v10.sh back"
  env HOME="$W/home" PATH="$W/macbin:$ORIG_PATH" /bin/zsh -fc 'cd ~/seventh-watch && bash kit/st4863-v10.sh back' > "$W/back.out" 2>&1; echo "  back exit $?"; grep -E 'PUTBACK|REMOVED|PUT BACK|LOOK' "$W/back.out" | cut -c1-170 | sed 's/^/    | /'
  snap > "$W/root.after"; diff "$W/root.before" "$W/root.after" > "$W/root.diff"
  ok "after back: the fake root byte-identical$( [ -s "$W/root.diff" ] && echo ": $(tr '\n' ' ' < "$W/root.diff" | cut -c1-300)")" "[ ! -s '$W/root.diff' ]"; }
armasserts() { local LOGF=$1; shift
  for X in "$@"; do ok "arm $X: verdict read: $(grep -m1 "^ST4863 $X " "$LOGF" | cut -c1-140)" "grep -q '^ST4863 $X ' '$LOGF'"
    ok "arm $X: its tidy: $(grep -m1 "PUTBACK OK (kit-after-$X" "$LOGF" | cut -c1-70)" "grep -q 'PUTBACK OK (kit-after-$X' '$LOGF'"; done
  grep -E 'boot-state|after-recover|station rc|apmode|boot leg|pet leg' "$W/calls" | cut -c1-140 | sed 's/^/    ~ /'; }

CASES=${*:-neg phase0 go}
echo "== LAUNCH DRY of kit/st4863-v10.sh $(shasum -a 256 < "$KIT" | cut -c1-12) $(date '+%d %b %T') (lib.sh $(shasum -a 256 < "$KD/lib.sh" | cut -c1-12)); GUARD_REAL=$GUARD_REAL s (lamp clock x$SCALE)"
LIM=$(date -v+2H +%H%M)
for c in $CASES; do case $c in
  world) world; echo "$W"; continue ;;   # a world only, for a look
  neg) # L0: the same launch against v9 with its ME line put back as the 22:05 kit had it: the run must die 127
    N=$(mktemp -d); cp "$KD/lib.sh" "$N/"; sed 's#^ME=$(realpath "$0") || exit 2; K=$(dirname "$ME")#K=$(cd "$(dirname "$0")" \&\& pwd); ME=$0#' "$KIT" > "$N/st4863-v10.sh"
    grep -q '^K=$(cd "$(dirname "$0")" && pwd); ME=$0;' "$N/st4863-v10.sh" || { echo "neg: the ME line was not found"; FAILS=$((FAILS+1)); continue; }
    launch "L0 negative (ME=\$0, a relative path after the cd)" "$N" "cd ~/seventh-watch && LIMIT=$LIM bash kit/st4863-v10.sh phase0"
    ok "L0 the harness catches the 22:05 failure: QUIET says exit 127, the run never started: $(grep -o 'QUIET lamp-52e6.*' "$W/calls" | cut -c1-100)" "grep -q 'QUIET lamp-52e6 .*(exit 127)' '$W/calls' && ! grep -q '^== st4863' '$LOGF' && [ ! -s '$W/fw/calls' ]"
    rm -rf "$N" ;;
  phase0) launch "L1 phase0" "$KD" "cd ~/seventh-watch && LIMIT=$LIM bash kit/st4863-v10.sh phase0"; asserts "$LOGF" 1
    ok "L1 no arm ran, lamp-voice untouched" "! grep -q 'unit=seventh-st4863-arm-' '$W/fw/calls' && grep -q 'lamp-voice not restarted (phase 0 only' '$LOGF'" ;;
  go) launch "L2 go, ARMS=A" "$KD" "cd ~/seventh-watch && ARMS=A LIMIT=$LIM bash kit/st4863-v10.sh go"; asserts "$LOGF" 1
    ok "L2 arm A ran as its unit and PASSed: $(grep -m1 '^ST4863 A ' "$LOGF" | cut -c1-120)" "grep -q '^ST4863 A PASS' '$LOGF' && grep -q 'unit=seventh-st4863-arm-A' '$W/fw/calls'"
    ok "L2 the tidy after arm A: $(grep -m1 'PUTBACK OK (kit-after-A' "$LOGF" | cut -c1-90)" "grep -q 'PUTBACK OK (kit-after-A' '$LOGF' && grep -q '^== all arms played' '$LOGF'" ;;
  goabc) launch "R1 go ARMS=A B C FIX=1" "$KD" "cd ~/seventh-watch && ARMS='A B C' LIMIT=$LIM bash kit/st4863-v10.sh go"; asserts "$LOGF" 3; armasserts "$LOGF" A B C ;;
  nofix) export FIX=0; launch "R2 go ARMS=B C FIX=0 (no repair, no close)" "$KD" "cd ~/seventh-watch && ARMS='B C' LIMIT=$LIM bash kit/st4863-v10.sh go"; unset FIX; asserts "$LOGF" 3; armasserts "$LOGF" B C ;;
  stafail) export FIX=0 IWFAIL_BOOT=2; launch "R3 go ARMS=C FIX=0, iw fails twice at each boot (device-sta-mode dies after link down; bring-up's first iw too)" "$KD" "cd ~/seventh-watch && ARMS=C LIMIT=$LIM bash kit/st4863-v10.sh go"; unset FIX IWFAIL_BOOT; asserts "$LOGF" 2; armasserts "$LOGF" C ;;
  stafail3) export IWFAIL_BOOT=3; launch "R3b go ARMS=B FIX=1, iw fails 3x at each boot (the fix's repair, the guard's device-sta-mode, the bring-up's 1st iw)" "$KD" "cd ~/seventh-watch && ARMS=B LIMIT=$LIM bash kit/st4863-v10.sh go"; unset IWFAIL_BOOT; asserts "$LOGF" 2; armasserts "$LOGF" B ;;
  killA) export FIX=0; INJ=inj_killA LTO=600 launch "R4 go ARMS=A FIX=0, the Mac dies 3 s into A's window" "$KD" "cd ~/seventh-watch && ARMS=A LIMIT=$LIM bash kit/st4863-v10.sh go"; unset FIX; afterdeath 900 ;;
  killB) export FIX=0; INJ=inj_killB LTO=600 launch "R5 go ARMS=B FIX=0, the Mac dies as B's window starts" "$KD" "cd ~/seventh-watch && ARMS=B LIMIT=$LIM bash kit/st4863-v10.sh go"; unset FIX; afterdeath 900 ;;
  sshdrop) INJ=inj_ssh LTO=2400 launch "R6 go ARMS=A, ssh drops for ${SSHDOWN:-600} s as A's window starts" "$KD" "cd ~/seventh-watch && ARMS=A LIMIT=$LIM bash kit/st4863-v10.sh go"
    grep -E 'ssh (down|up)' "$W/calls" | sed 's/^/    ~ /'; ok "R6 the run ended and put back: $(grep -E '^PUTBACK (OK|NOT OK)' "$LOGF" | tail -1 | cut -c1-120)" "grep -q '^PUTBACK OK: files equal' '$LOGF'"
    snap > "$W/root.after"; diff "$W/root.before" "$W/root.after" > "$W/root.diff"; ok "R6 fake root identical$( [ -s "$W/root.diff" ] && echo ": $(tr '\n' ' ' < "$W/root.diff" | cut -c1-200)")" "[ ! -s '$W/root.diff' ]" ;;
  buildsw) INJ=inj_build launch "R7 go ARMS=B C, lamp-update switches build after B's reboot" "$KD" "cd ~/seventh-watch && ARMS='B C' LIMIT=$LIM bash kit/st4863-v10.sh go"
    grep -E 'BUILD SWITCHED' "$W/calls" | sed 's/^/    ~ /'; grep -E 'XCHECK|ST4863 B|ABORT|REPORTED APART|^PUTBACK' "$LOGF" | cut -c1-170 | sed 's/^/    | /'
    ok "R7 back on its LAN, put back, guard removed: $(grep -E '^PUTBACK (OK|NOT OK)' "$LOGF" | tail -1 | cut -c1-100)" "grep -q '^PUTBACK OK: files equal' '$LOGF' && grep -q '^REMOVED /var/lib/seventh-st4863' '$LOGF'"
    snap > "$W/root.after"; diff "$W/root.before" "$W/root.after" | grep -v 'opt/lampOS/setup' | grep '^[<>]' > "$W/root.diff"; ok "R7 fake root identical outside lamp-update's own switch$( [ -s "$W/root.diff" ] && echo ": $(tr '\n' ' ' < "$W/root.diff" | cut -c1-200)")" "[ ! -s '$W/root.diff' ]" ;;
  lockloss) INJ=inj_steal launch "V1 go ARMS='B C', another session takes the lock as B's window starts" "$KD" "cd ~/seventh-watch && ARMS='B C' LIMIT=$LIM bash kit/st4863-v10.sh go"
    grep -E 'LOCK STOLEN' "$W/calls" | sed 's/^/    ~ /'
    ok "V1 abort after B's wait: $(grep -m1 'ABORT' "$LOGF" | cut -c1-120)" "grep -q '^ABORT the lamp lock was lost' '$LOGF'"
    ok "V1 B ran, no arm C, no tidy after B (fin's put-back instead)" "grep -q 'unit=seventh-st4863-arm-B' '$W/fw/calls' && ! grep -q 'unit=seventh-st4863-arm-C' '$W/fw/calls' && ! grep -q 'kit-after-B' '$LOGF'"
    ok "V1 one reboot only (phase 0's + B's = 2), fin put back: $(grep -E '^PUTBACK (OK|NOT OK)' "$LOGF" | tail -1 | cut -c1-90)" "[ \"\$(grep -c '^REBOOT' '$W/fw/calls')\" = 2 ] && grep -q '^PUTBACK OK: files equal' '$LOGF'"
    ok "V1 QUIET says the lock was lost, not FREE: $(grep -o 'QUIET lamp-52e6.*' "$W/calls" | sed 's/^.*(exit/(exit/' | cut -c1-120)" "grep -q 'QUIET lamp-52e6 .*lock lost to fifth (st-77)\.' '$W/calls' && ! grep -q 'QUIET .*FREE' '$W/calls'"
    snap > "$W/root.after"; diff "$W/root.before" "$W/root.after" > "$W/root.diff"; ok "V1 fake root identical$( [ -s "$W/root.diff" ] && echo ": $(tr '\n' ' ' < "$W/root.diff" | cut -c1-200)")" "[ ! -s '$W/root.diff' ]" ;;
  pblost) INJ=inj_pblose launch "V2 go ARMS=A, the first put-back start's ssh fails" "$KD" "cd ~/seventh-watch && ARMS=A LIMIT=$LIM bash kit/st4863-v10.sh go"; asserts "$LOGF" 1
    grep -E 'PBSTART LOST' "$W/calls" | sed 's/^/    ~ /'
    ok "V2 the lost start was started again, then OK: $(grep -m1 'started again' "$LOGF" | cut -c1-110)" "grep -q 'PBSTART LOST' '$W/calls' && grep -q 'with no line for this call: started again' '$LOGF' && grep -q 'PUTBACK OK (kit-after-A' '$LOGF'" ;;
  esac; echo "  world: $W"; done
echo "== LAUNCH DRY verdict: $FAILS failed check(s)"; [ "$FAILS" = 0 ]
