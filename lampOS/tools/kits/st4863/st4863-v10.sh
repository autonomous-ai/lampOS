#!/usr/bin/env bash
# st4863.sh: #4863's station-back fix on lamp-52e6 (Seventh for Sixth, 2 Nov). DRAFT: no lamp time before the lead's yes.
# Test script: kit/st4863-script.md (arms, verdict sources, baseline, minutes, put-back, the lead's five rules).
# The fix (PR #4931, sixth/recover-station-back, merged; in stable 9582, lamp-voice ab6ef495ce, train 521): recover
# returns the radio to station mode when it closes, and at every start of lamp-setup-recover a set-up lamp left in
# AP mode (wpa masked, denyinterfaces wlan0, or no multi-user.target.wants/wpa_supplicant@wlan0.service link) is
# returned to station mode. The kit tests the INSTALLED lamp-setup of EXPECT_SEQ/EXPECT_COMMIT; nothing is staged.
#  - the build gate: the program recover's effective ExecStart runs (/opt/lampOS/setup/<current|sha>/lamp-setup,
#    resolved) must hash to its release folder's name (lamp-update names a part's folder by its binary's sha256),
#    and hold the fix's repair and close lines. Recorded at preflight (backup/recover.exec); re-checked before each
#    arm, inside each arm, after each boot, and by the guard's and the kit's put-back: a change (lamp-update moved
#    it) or a lamp-update switch in the last 10 min refuses, logged.
#  - the kit's only recover drop-ins are the switch (zz-seventh-st4863-on.conf: /etc for B, /run for A and C); the
#    put-back removes them. A seventh drop-in or /opt/lampOS/releases/seventh-* left by an older run of this kit
#    is refused (the put-back logs it, removes it and reports FAILED for that pass; the guard retries in 60 s).
#  - no baseline arm: C0 (installed lamp-setup without the fix) no longer exists on this build; the recorded
#    baseline is 0c89's incident (2 Nov) and #4863's diagnosis comment 6573158451 (see the test script).
#
# Usage (from ~/seventh-watch):
#   LIMIT=HHMM kit/st4863.sh phase0  # FIRST lamp step, its own yes: lock + PLAYING/QUIET, preflight, setup, phase 0,
#                                     # then the full put-back and the guard removed. Never an arm, never off station
#                                     # mode (one reboot as a station). Same as PHASE0_ONLY=1 LIMIT=HHMM kit/st4863.sh go
#   LIMIT=HHMM kit/st4863.sh go   # lock + PLAYING/QUIET on #3336, then run (EXPECT_SEQ/EXPECT_COMMIT: 9582 ab6ef495ce by default)
#   kit/st4863.sh back    # the put-back alone: waits for the lamp, restores by hash, then removes the guard (safe twice:
#                         # a lamp with nothing of the kit left is reported as already put back)
#   kit/st4863.sh dry     # every gate, every lamp command printed, no ssh; the lamp side run on the Mac on a fake
#                         # root with systemd-252-shaped fakes (PHASE0_ONLY=1 for the phase0 entry's DRY)
#   ARMS="A B C" GUARD_MIN=5 HOLD=10 are the knobs.
#
# Safety net (0c89, 2 Nov: backup and guard in /run died with the power; no lamp takes a cable, so after a Wi-Fi
# loss nothing but the lamp itself can bring it back):
#  - every file device-ap-mode touches, the Wi-Fi file's hash (no copy), the unit states and the wpa_supplicant@wlan0 wants link
#    (exists or not, and its readlink target), backed up to /var/lib/seventh-st4863
#    with sha256 before anything else, synced (the root is commit=600), never overwritten by a later run;
#  - the guard: seventh-st4863-guard.service in /etc/systemd/system, run by two /etc timers:
#      boot leg seventh-st4863-guard.timer: OnBootSec=GUARD_MIN, enabled for timers.target (not started at setup:
#        an OnBootSec already past fires at once when started), so it fires at boot + GUARD_MIN at every boot;
#      pet leg seventh-st4863-pet.timer: OnActiveSec=GUARD_MIN, no [Install], restarted by the kit before each
#        window (a reboot drops it; the boot leg covers the boot).
#    No After=/Requires=/BindsTo=/Wants= of its own: only systemd's default deps; nothing on lamp-setup units,
#    /opt or a mount (selftest reads the effective deps and fails on any). It runs /bin/sh + the image's
#    tools (selftest checks each resolves outside /opt and /var/lib). Its body (st.sh putback): FIRST, past the
#    expiry stored at setup (setup + 3 h, pushed to now + 3 h at each pet), it removes its own units and exits 0, before the lock and before any
#    restore (and $D after the units). It also stands back, touching nothing, while lamp-setup runs, a
#    .lamp-previous Wi-Fi change is pending or the done marker is gone: it never fights a person re-provisioning
#    the lamp. The Wi-Fi file is never copied or restored, only its hash logged: the kit never writes it (the cut
#    is `wpa_cli disable_network`, runtime only; device-ap-mode does not edit it), so no arm needs it restored.
#    The expiry: setup + 3 h (pushed at each pet), setup + 1 h for phase0 alone. It stands back as well (its OK
#    line) on a clean station with no arm unit and no kit drop-in (F-P2). Otherwise, under the kit's one lock
#    (/var/lib/seventh-st4863/lock, flock; also held by every kit put-back and by each arm; the guard waits up to
#    660 s, then goes on without it): kit drop-ins off, files back by hash, device-sta-mode (its own process, so its
#    set -e cannot skip anything) only when the lamp is not already a clean station, files again, unit states as
#    found, then its own station bring-up (hostapd and dnsmasq stopped and disabled, nginx stopped if it was
#    inactive, the reg domain back, wlan0 up and managed, the Wi-Fi file re-checked by hash, wpa_supplicant@wlan0
#    restarted, enable_network all, dhcpcd restarted, a LAN address waited for 90 s), skipped entirely on a clean
#    station whose Wi-Fi file never left its hash: a healthy lamp is never restarted off its LAN. Still no LAN: a
#    counted reboot (at most 2 a kit run, counted in /var/lib/seventh-st4863/reboots, logged and synced first);
#    used up: logged, no reboot. The unit has no start limit and no StartLimitAction: it retries every 5 min, and
#    a missing or broken st.sh only fails it. recover's program moved by lamp-update is logged, not a put-back
#    failure. Proved first (phase 0: a plain reboot in station mode; the boot leg's journal line and its
#    LastTriggerUSecMonotonic read raw by busctl) before any AP step; removed at the verified put-back or at expiry;
#  - the guard appends one line per run (and per counted reboot) to ~orangepi/loans.txt on the lamp;
#  - NOT covered: a lamp that hangs without rebooting fires neither leg (the boot leg needs a boot, the pet leg a
#    running systemd, and 52e6 has no hardware watchdog);
#  - seventh-st4863-lan.service (/etc, enabled, observation only): logs when wlan0 first has a LAN address each boot
#    (this image's dhcpcd logs no `wlan0: leased`);
#  - the cut writes nothing: `wpa_cli disable_network all` (runtime only; any wpa_supplicant restart reads the
#    real file again). Every arm starts with the live files equal to the backup by hash, and ends restored and
#    re-checked by hash;
#  - no power cut: `systemctl reboot -ff` stands in for B and C, only after the guard is checked armed (phase 0
#    uses a plain `systemctl reboot`).
#  - worst-case time off the client network (S5, GUARD_MIN=5). Typical put-back <= 230 s; with every systemctl
#    call blocking its full 90 s, <= 1045 s (TimeoutStartSec's budget less the lock wait).
#      A: cut, hotspot by 150 s, hold 13 s, close; if the close fails the pet leg (pet + 300 s, the cut at about pet +
#         40 s) waits for the arm's lock (released by +603 s at worst from the arm's start) and puts back:
#         typical <= ~9 min, worst ~28 min.
#      B/C: cut, hotspot by 150 s, hold 13 s, reboot (~60 s); if the fix fails, the boot leg at boot + 300 s puts
#         back (no lock held at boot): typical 30 + 150 + 13 + 60 + 300 + 230 s = ~13 min, worst ~27 min. If the
#         bring-up finds no LAN, each counted reboot adds 60 + 300 + 230 s (~10 min; worst ~24 min): typical
#         <= ~33 min with both.
#    The kit's own waits are over these: GUARD_MIN + 7 min for a new boot, GUARD_MIN + 5 min for the guard's line.
#
# Reused, not rebuilt (KITS.md):
#   lib.sh b569b4e63c44: L/LI (ssh), otherplaying (#3336), rs (lamp-voice stop/start with a restart-guard mute set
#     aside, after the unclean reboots).
#   fifth/kit3697/stage.sh 6828e3f45899: the boot-id wait across a power-off (bootid/waitback), here without the
#     replug cue (a reboot, no hand), and its DRY shape (each lamp script kept and checked by sh -n and dash -n).
#   sw2b.sh 91554330cd9a (style model): EXPECT_SEQ/EXPECT_COMMIT pin from state.json, LIMIT/left, PLAYING/QUIET, fin.
#   ~/seventh-watch/lamp-lock.sh a55b878c0665: acquire/renew/release (not `run`: run kills its command 540 s after
#     its last confirmed renewal, which a failed window can pass while the lamp is unreachable).
#   sixth/row-3020d.sh cfbf13726a3a (not in KITS.md): recover's journal lines and the 150 s hotspot wait. Not
#     taken: its cut (sed on the real Wi-Fi file) and its guard (systemd-run, gone at boot).
# Not reused: lib.sh's deadman_arm/disarm: a systemd-run timer under /run, gone at a reboot (this kit's guard
#   replaces it).
set -uo pipefail
# F2/L: this file by absolute path, symlinks resolved (a relative $0 like kit/st4863.sh no longer resolves after the
# cd below, and a symlink's own folder has no lib.sh); every later use of the kit's own path goes through ME or K
ME=$(realpath "$0") || exit 2; K=$(dirname "$ME"); MN=$(basename "$ME" .sh); cd "$K" && . ./lib.sh
# lib.sh's L/LI without its shared ControlMaster: a master left from before a Wi-Fi loss can hang a call; keepalives end it.
L="ssh -n -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=5 -o ServerAliveCountMax=3 -o ControlPath=none orangepi@lamp-52e6.local"
LI="${L/ssh -n /ssh }"
MODE=${1:-}
# phase0: the go path with PHASE0_ONLY=1 (preflight, setup, phase 0, put-back; no arm)
[ "$MODE" = phase0 ] && { PHASE0_ONLY=1; MODE=go; }
PHASE0_ONLY=${PHASE0_ONLY:-}; export PHASE0_ONLY
# F-P2: the guard's expiry: setup + 3 h (1 h for phase0 alone), pushed to now + TTL at each pet
TTL=$( [ -n "$PHASE0_ONLY" ] && echo 3600 || echo 10800)
DRY=; { [ "$MODE" = dry ] || [ "$MODE" = huptest ]; } && DRY=1   # huptest: the DRY's HUP case (a run's traps, fakes)
LIMIT=${LIMIT:-}
# stable 9582 (lamp-voice ab6ef495ce) carries PR #4931 (8dd46590d is its ancestor, train 521): installed on 52e6 tonight.
EXPECT_SEQ=${EXPECT_SEQ:-9582}; EXPECT_COMMIT=${EXPECT_COMMIT:-ab6ef495ce}
ARMS=${ARMS:-A B C}; GUARD_MIN=${GUARD_MIN:-5}; HOLD=${HOLD:-10}
XSHA=   # the installed lamp-setup's sha256 recover runs, set at preflight (= its release folder's name)
BOOT_LAN_MAX=${BOOT_LAN_MAX:-120}   # B/C: a LAN address on wlan0 by this many s after boot (Sixth; recover's own reopen+retry is 210 s+)
# The fix's journal lines (tag lamp-setup-recover; PR #4931's main.rs, confirmed by Sixth). The repair's failed tries
# log nothing; only its give-up does ("...could not start: <error>; the next boot tries again if it is still one"),
# under 4 min from recover's start, so before the guard's boot + GUARD_MIN.
CLOSE_RE=${CLOSE_RE:-recover: the hotspot closed: back to the station}
REPAIR_RE=${REPAIR_RE:-recover: the radio was left a hotspot: back to the station}
NOSTART_RE='recover: the (radio was left a hotspot|hotspot closed) and the station could not start'
WLK=/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service
D=/var/lib/seventh-st4863; GU=seventh-st4863-guard; GP=seventh-st4863-pet; GW=seventh-st4863-lan
LOCK=${ST_LOCK:-$HOME/seventh-watch/lamp-lock.sh}; SESSION=${SESSION:-st4863-$$}; SET=; GSET=
NA=$(wc -w <<< "$ARMS"); grep -qw A <<< "$ARMS" && NA=$((NA-1))
MINLEFT=$(( (5 + 9 + NA*11 + $(grep -cw A <<< "$ARMS")*6 + 4) * 60 ))
# phase0 alone: preflight + setup 5, reboot + boot leg + its put-back GUARD_MIN + 6, the put-back 6
[ -n "$PHASE0_ONLY" ] && MINLEFT=$(( (5 + GUARD_MIN + 6 + 6) * 60 ))
LOG=${LOG:-$HOME/seventh-watch/st4863-$(date +%m%d-%H%M)${PHASE0_ONLY:+-phase0}${DRY:+-dry}.txt}
DD=$K/$MN-dry${PHASE0_ONLY:+-phase0}.d; WA=0; [ "$MODE" = huptest ] && DD=$(mktemp -d)
# dl: the DRY's LIMIT, now + 2 h (left() reads a LIMIT past midnight as tomorrow's)
dl() { date -v+2H +%H%M; }
# left: seconds to LIMIT (HHMM). A LIMIT earlier than now is tomorrow's when that is within 12 h (LIMIT=0030 at
# 23:10 is 80 min away); otherwise it has passed (negative). A malformed LIMIT reads as passed.
left() { [ -z "$LIMIT" ] && { echo 99999; return; }; [[ $LIMIT =~ ^([01][0-9]|2[0-3])[0-5][0-9]$ ]] || { echo -1; return; }
  local d; set -- $(date '+%H %M %S'); d=$(( 10#${LIMIT:0:2}*3600 + 10#${LIMIT:2:2}*60 - (10#$1*3600 + 10#$2*60 + 10#$3) ))
  [ "$d" -lt 0 ] && [ $((d + 86400)) -le 43200 ] && d=$((d + 86400)); echo "$d"; }
rec() { echo "$*"; [ -n "$DRY" ] || echo "$*" >> "$LOG.verdicts"; }
# abort: a real run stops (fin puts back); the dry counts it and goes on, so every gate is seen.
abort() { if [ -n "$DRY" ]; then echo "WOULD ABORT: $*"; WA=$((WA+1)); return 1; fi; echo "ABORT $*"; exit 1; }

# ---- the lamp side: st.sh, installed to $D/st.sh (root). POSIX sh, dash-clean. ROOT= a scratch prefix (dry, selftest).
lampsh() { cat <<'EOF'
#!/bin/sh
# st4863 lamp side (Seventh, #4863). Root. Subcommands: facts build backup check selftest state putback xcheck arm
# report lanwatch disarm restore usec.
set -u
R=${ROOT:-}
D=$R/var/lib/seventh-st4863; B=$D/backup; GU=seventh-st4863-guard; GP=seventh-st4863-pet; GW=seventh-st4863-lan
FILES="/etc/dhcpcd.conf /etc/dnsmasq.d/99-lamp.conf /etc/hostapd/hostapd.conf /etc/hosts /etc/hostname /etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
WF=/etc/wpa_supplicant/wpa_supplicant-wlan0.conf
# nginx too: device-ap-mode restarts it (disabled/inactive on a station)
UNITS="wpa_supplicant@wlan0 hostapd dnsmasq dhcpcd avahi-daemon nginx"
RE=/etc/systemd/system/lamp-setup-recover.service.d; RR=/run/systemd/system/lamp-setup-recover.service.d
# the kit's only drop-ins: the switch (B: /etc, on at boot; A and C: /run, gone at boot)
KD="$RE/zz-seventh-st4863-on.conf $RR/zz-seventh-st4863-on.conf"
# the wants link: its absence also reads as a left hotspot to the fix (00cf852f7c); backed up as its readlink target
WL=/etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service
SU=$R/etc/systemd/system; STA=$R/usr/local/bin/device-sta-mode
# B1: at most MAXRB counted reboots per kit run (the counter lives in $D, made fresh at setup, gone at the put-back)
MAXRB=2
# S3: one lock for every put-back (guard and kit) and every arm. A put-back waits up to LOCKW s, over an arm's
# longest hold with every blocking step at its worst (R-P3): recover restart 90 + watching 20 polls 40 + cut 30 +
# hotspot 75 polls 160 + 3 + HOLD 10 + A's stop of recover 90 + its ExecStopPost 90 + LAN wait 90 = 603 s.
LOCKW=${LOCKW:-660}
# the guard appends one line per run to the lamp loan file (P3)
if [ -n "$R" ]; then LO=$R/loans.txt; else LO=~orangepi/loans.txt; fi
# real: take the lamp's own paths (systemctl's views, the selftest's unit half). True on the lamp (ROOT empty) and
# in the DRY's SIM=1 harness (ROOT a fake root, the tools 252-shaped fakes on PATH; T-P1).
real() { [ -z "$R" ] || [ -n "${SIM:-}" ]; }
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi | cut -c1-64; }
h() { if [ -f "$1" ]; then sha < "$1"; else echo ABSENT; fi; }
k() { echo "$1" | tr / _; }
lnk() { if [ -L "$1" ]; then readlink "$1"; elif [ -e "$1" ]; then echo NOTLINK; else echo ABSENT; fi; }
bid() { cut -c1-8 /proc/sys/kernel/random/boot_id 2>/dev/null || echo mac; }
# log: to $D/log.txt only while $D exists (never recreated after the put-back removed it), always to stdout (journal)
log() { [ -d "$D" ] && { echo "$(date +%T) b=$(bid) up=$(cut -d' ' -f1 /proc/uptime 2>/dev/null) $*" >> "$D/log.txt"; sync; }; echo "$*"; }
en() { e=$(systemctl is-enabled "$1" 2>/dev/null); echo "${e:-none}"; }
lan() { ip -4 -o addr show wlan0 2>/dev/null | awk '{print $4}' | grep -v '^192\.168\.100\.' | head -1; }
regnow() { iw reg get 2>/dev/null | awk '/^country/{sub(":","",$2); print $2; exit}'; }
state() { echo "wpa=$(en wpa_supplicant@wlan0)/$(systemctl is-active wpa_supplicant@wlan0) hostapd=$(en hostapd)/$(systemctl is-active hostapd) dnsmasq=$(en dnsmasq) deny=$(grep -c '^denyinterfaces wlan0$' "$R/etc/dhcpcd.conf" 2>/dev/null) type=$(iw dev wlan0 info 2>/dev/null | awk '/type/{print $2}') lan=$(lan) hot=$(ip -4 -o addr show wlan0 2>/dev/null | grep -c '192\.168\.100\.1/') wpa_state=$(wpa_cli -i wlan0 status 2>/dev/null | sed -n 's/^wpa_state=//p') recover=$(systemctl is-active lamp-setup-recover) wants=$([ -L "$R$WL" ] && echo 1 || echo 0)"; }
apish() { case $(state) in *wpa=masked*|*hostapd=enabled*|*deny=[1-9]*|*type=AP*|*hot=[1-9]*|*wants=0*) return 0 ;; esac
  [ "$(systemctl is-active hostapd)" = active ]; }
# clean: a station already (no AP trace, wpa active, a LAN address): device-sta-mode is never run on it
clean() { ! apish && [ -n "$(lan)" ] && [ "$(systemctl is-active wpa_supplicant@wlan0)" = active ]; }
# usec UNIT PROP: a timer's *USec* property as raw microseconds. systemd 252's `systemctl show` prints these as a
# timespan ("5min 204.511ms"); busctl gives "t <usec>". none = not readable.
usec() { o=$(busctl call org.freedesktop.systemd1 /org/freedesktop/systemd1 org.freedesktop.systemd1.Manager LoadUnit s "$1" 2>/dev/null | sed -n 's/^o "\(.*\)"$/\1/p')
  v=$(busctl get-property org.freedesktop.systemd1 "${o:-/none}" org.freedesktop.systemd1.Timer "$2" 2>/dev/null | sed -n 's/^t \([0-9][0-9]*\)$/\1/p'); echo "${v:-none}"; }
# lk WHY: the kit's lock (S3), held until this process ends. Waits up to LOCKW s.
lk() { exec 9>>"$D/lock"; flock -w "$LOCKW" 9 && return 0; log "LOCK still held after $LOCKW s ($1)"; return 1; }
# xp: the program recover's effective ExecStart runs (systemd's view; under ROOT, the unit file then its drop-ins).
xp() { if real; then systemctl show -p ExecStart --value lamp-setup-recover | sed -n 's/.*path=\([^ ;]*\).*/\1/p' | head -1; return; fi
  p=; for f in "$R/etc/systemd/system/lamp-setup-recover.service" "$R$RE"/*.conf "$R$RR"/*.conf; do
    [ -f "$f" ] || continue; q=$(sed -n 's/^ExecStart=\(\/[^ ]*\).*/\1/p' "$f" | tail -1); [ -n "$q" ] && p=$q; done; echo "$p"; }
# rel: the release folder recover's program resolves to (lamp-update names it by the binary's sha256).
rel() { basename "$(dirname "$(readlink -f "$R$1")")"; }
# build: recover's program, its sha256, its release folder's name, lamp-update's switches in the last 10 min.
build() { p=$(xp); echo "exec=$p xsha=$(h "$R$p") rel=$(rel "$p") switching=$(journalctl -u lamp-update --since -10min --no-pager 2>/dev/null | grep -c 'switching to build')"; }
# xcheck: the kit's switch drop-ins removed; any seventh drop-in or /opt/lampOS/releases/seventh-* an older run left
# is removed and refused (U=1: this pass FAILED, logged). Then recover's program against the backup's path and
# sha256: a change (lamp-update moved it) is XB=1, logged, and not a put-back failure (S4). 0 when both hold.
xcheck() { U=0; XB=0
  for f in $KD; do [ -e "$R$f" ] || continue; rm -f "$R$f"; log "xcheck: switch drop-in $f removed"; [ -e "$R$f" ] && U=1; done
  for f in "$R$RE"/zz-seventh-* "$R$RR"/zz-seventh-* "$R"/opt/lampOS/releases/seventh-*; do [ -e "$f" ] || continue
    log "xcheck: LEFTOVER from an older run: ${f#$R} ($( [ -d "$f" ] && echo folder || h "$f" | cut -c1-12)): removed, this pass refused"; rm -rf "$f"; U=1; done
  # L: the drop-in folders an arm made, removed once empty (a folder with a product drop-in stays; a folder that was
  # there empty before the kit goes too: systemd reads no difference)
  rmdir "$R$RE" "$R$RR" 2>/dev/null
  sync; real && systemctl daemon-reload
  read -r wp ws wr < "$B/recover.exec"; np=$(xp); ns=$(h "$R$np")
  real && systemctl show -p DropInPaths --value lamp-setup-recover | grep -q seventh && U=1
  [ "$np" = "$wp" ] && [ "$ns" = "$ws" ] || XB=1
  if [ $U = 0 ] && [ $XB = 0 ]; then log "XCHECK OK: no kit drop-in, recover runs $np $(echo "$ns" | cut -c1-12), the build's (release $(echo "$wr" | cut -c1-12))"
  elif [ $U = 0 ]; then log "XCHECK BUILD MOVED (logged; not a station put-back failure): recover runs $np $(echo "$ns" | cut -c1-12), want $wp $(echo "$ws" | cut -c1-12); no kit drop-in"
  else log "XCHECK FAILED: recover runs $np $(echo "$ns" | cut -c1-12), want $wp $(echo "$ws" | cut -c1-12); kit files now: $(ls -d "$R$RE"/zz-seventh-* "$R$RR"/zz-seventh-* "$R"/opt/lampOS/releases/seventh-* 2>/dev/null | tr '\n' ' ')"; fi
  [ $U = 0 ] && [ $XB = 0 ]; }

backup() {
  if [ -e "$B/manifest" ]; then echo "backup already there: kept, never overwritten"; check_backup; return; fi
  rm -rf "$B.new"; mkdir -p "$B.new"
  # T-P2: the Wi-Fi file is hashed, never copied (no root-only copy of the password) and never restored
  for f in $FILES; do x=$(h "$R$f"); [ "$x" = ABSENT ] || [ "$f" = "$WF" ] || cp -p "$R$f" "$B.new/$(k "$f")"; echo "$f $x" >> "$B.new/manifest"; done
  for u in $UNITS lamp-setup-recover; do echo "$u $(en "$u") $(systemctl is-active "$u")" >> "$B.new/units"; done
  hostname > "$B.new/hostname.now"; r=$(regnow); echo "${r:-none}" > "$B.new/reg"
  # S4/T-P2/F-P2: the guard's hard expiry, setup + TTL (1st arg: 10800 for a full run, 3600 for phase0 only)
  echo $(( $(date +%s) + ${1:-10800} )) > "$B.new/expires"
  p=$(xp); echo "$p $(h "$R$p") $(rel "$p")" > "$B.new/recover.exec"
  echo "$WL $(lnk "$R$WL")" > "$B.new/wants"
  sync; mv "$B.new" "$B"; sync
  check_backup && log "backup made: manifest $(sha < "$B/manifest" | cut -c1-12), $(wc -l < "$B/manifest" | tr -d ' ') files, wants link $(cut -d' ' -f2 "$B/wants"), reg $(cat "$B/reg"), nginx $(grep '^nginx ' "$B/units" | cut -d' ' -f2-), guard expires $(cat "$B/expires")"
}
check_backup() { bad=0
  while read -r f x; do { [ "$x" = ABSENT ] || [ "$f" = "$WF" ]; } && continue; [ "$(h "$B/$(k "$f")")" = "$x" ] || { echo "BACKUP BAD $f"; bad=$((bad+1)); }; done < "$B/manifest"
  # the build gate: recover's program hashes to its release folder's name (the build's own file)
  read -r p x n < "$B/recover.exec"; { [ -n "$p" ] && [ "$x" = "$n" ] && echo "$x" | grep -qE '^[0-9a-f]{64}$'; } || { echo "BACKUP BAD recover.exec '$p $x $n': not the build's own file"; bad=$((bad+1)); }
  wl=; wt=; [ -f "$B/wants" ] && read -r wl wt < "$B/wants"; { [ "$wl" = "$WL" ] && [ -n "$wt" ]; } || { echo "BACKUP BAD wants '$wl $wt'"; bad=$((bad+1)); }
  for f in units reg expires hostname.now; do [ -s "$B/$f" ] || { echo "BACKUP BAD $f empty"; bad=$((bad+1)); }; done
  [ $bad = 0 ] && echo "backup ok: every copy matches its hash"; [ $bad = 0 ]; }
# check: the live files against the backup's hashes; one line each; returns the number that differ. The Wi-Fi file
# is only reported (`info`): a person may re-provision the lamp, and the kit never writes it (T-P2).
check() { n=0
  while read -r f x; do c=$(h "$R$f"); if [ "$f" = "$WF" ]; then echo "info $f $( [ "$c" = "$x" ] && echo same || echo "CHANGED now $(echo "$c" | cut -c1-12)")"; elif [ "$c" = "$x" ]; then echo "same $f"; else echo "DIFF $f now $(echo "$c" | cut -c1-12) want $(echo "$x" | cut -c1-12)"; n=$((n+1)); fi; done < "$B/manifest"
  read -r wl wt < "$B/wants"; c=$(lnk "$R$wl"); if [ "$c" = "$wt" ]; then echo "same $wl -> $wt"; else echo "DIFF $wl now $c want $wt"; n=$((n+1)); fi
  return $n; }
# restore: each file whose hash differs is put back from the backup (or removed when it was absent), re-checked.
restore() { CH=0; BAD=0
  while read -r f x; do c=$(h "$R$f"); { [ "$c" = "$x" ] || [ "$f" = "$WF" ]; } && continue
    if [ "$x" = ABSENT ]; then rm -f "$R$f"; else mkdir -p "$(dirname "$R$f")"; cp -p "$B/$(k "$f")" "$R$f"; fi
    if [ "$(h "$R$f")" = "$x" ]; then CH=$((CH+1)); log "restored $f"; else BAD=$((BAD+1)); log "RESTORE FAILED $f"; fi
  done < "$B/manifest"
  # the wants link: back to the backup's target (AP mode removes it), or removed when it was absent
  read -r wl wt < "$B/wants"; c=$(lnk "$R$wl")
  if [ "$c" != "$wt" ]; then rm -f "$R$wl"; [ "$wt" = ABSENT ] || { mkdir -p "$(dirname "$R$wl")"; ln -s "$wt" "$R$wl"; }
    real && systemctl daemon-reload
    if [ "$(lnk "$R$wl")" = "$wt" ]; then CH=$((CH+1)); log "restored $wl -> $wt (was $c)"; else BAD=$((BAD+1)); log "RESTORE FAILED $wl (now $(lnk "$R$wl"), want $wt)"; fi; fi
  sync; }
# selftest: the restore proved on a scratch copy: the backup copied out, AP mode's edits made on it (the wants link
# left out, as AP mode removes it), restored, compared.
selftest() { S=$D/scratch; rm -rf "$S"; ok=1
  while read -r f x; do { [ "$x" = ABSENT ] || [ "$f" = "$WF" ]; } && continue; mkdir -p "$(dirname "$S$f")"; cp -p "$B/$(k "$f")" "$S$f"; done < "$B/manifest"
  printf 'network={\n}\n' > "$S.wf"; mkdir -p "$(dirname "$S$WF")"; mv "$S.wf" "$S$WF"   # a stand-in: check only reports it
  mkdir -p "$S/etc/dnsmasq.d" "$S/etc/hostapd"
  { echo 'denyinterfaces wlan0'; cat "$S/etc/dhcpcd.conf" 2>/dev/null; } > "$S/x" && mv "$S/x" "$S/etc/dhcpcd.conf"
  echo 'address=/#/192.168.100.1' >> "$S/etc/dnsmasq.d/99-lamp.conf"
  echo 'ssid=lamp-scratch' >> "$S/etc/hostapd/hostapd.conf"; echo '127.0.1.1 lamp-scratch' >> "$S/etc/hosts"
  ( R=$S; restore; [ "$BAD" = 0 ] ) || ok=
  ( R=$S; check >/dev/null ) || ok=
  rm -rf "$S"
  if [ -n "$ok" ]; then echo "SELFTEST restore OK"; else echo "SELFTEST restore FAILED"; fi
  real || { [ -n "$ok" ]; return; }
  # boot leg: enabled (it starts with timers.target at the next boot); pet leg: active with a next elapse (busctl,
  # raw usec: none, 0 or UINT64_MAX is no elapse).
  bt=$(en $GU.timer)/$(grep -h '^OnBootSec=' "$SU/$GU.timer"); lw=$(en $GW.service)
  t=$(systemctl is-active $GP.timer 2>/dev/null)
  nx=$(usec $GP.timer NextElapseUSecMonotonic)
  v=$(systemd-analyze verify "$SU/$GU.service" "$SU/$GU.timer" "$SU/$GP.timer" "$SU/$GW.service" 2>&1 | grep -v -e '^$' | head -3)
  # independence: the guard's and its timers' effective deps name no lamp-setup unit, nothing under /opt (where
  # lamp-setup lives) and no mount of it (systemd's own -.mount and the default targets are allowed).
  dp=$(for u in $GU.service $GU.timer $GP.timer; do systemctl show -p After -p Requires -p Requisite -p BindsTo -p PartOf -p Wants -p RequiresMountsFor --value $u; done | tr ' ' '\n' | grep -E 'lamp-setup|/opt/|^opt-.*\.mount$' | sort -u | tr '\n' ' ')
  # the image's own tools only: each resolves, and not under /opt or /var/lib.
  ms=; for x in sh systemctl journalctl busctl flock ip iw wpa_cli timeout sha256sum hostname awk sed grep cp rm sync cut tr; do
    q=$(command -v $x) || { ms="$ms $x"; continue; }; case $q in /opt/*|/var/lib/*) ms="$ms $x@$q" ;; esac; done
  [ -x "$STA" ] || ms="$ms device-sta-mode"
  ld=$(for u in $GU.service $GU.timer $GP.timer $GW.service; do systemctl show -p LoadState --value $u; done | sort -u | tr '\n' ' ')
  echo "SELFTEST guard boot=$bt pet=$t next=$nx lan=$lw load=$ld verify=${v:+noted}${v:-clean} deps=${dp:-none} tools=${ms:-ok} reboots=$(cat "$D/reboots" 2>/dev/null || echo 0)/$MAXRB expires=$(cat "$B/expires") st.sh $(sha < "$0" | cut -c1-12)"
  case $bt in enabled/OnBootSec=[0-9]*min) ;; *) ok= ;; esac
  case $nx in ''|none|0|18446744073709551615) ok= ;; esac
  # R-P3: systemd-analyze verify's notes are logged, not a gate; the gate is that systemd loaded all four units
  [ -z "$v" ] || log "selftest: systemd-analyze verify says: $(echo "$v" | tr '\n' ' ')"
  [ "$ld" = "loaded " ] || ok=
  if [ -n "$ok" ] && [ "$lw" = enabled ] && [ "$t" = active ] && [ -z "$dp" ] && [ -z "$ms" ]; then echo "SELFTEST ARMED"; else echo "SELFTEST NOT ARMED"; return 1; fi; }
setu() { c=$(en "$1"); [ "$c" = "$2" ] && return 0
  case $2 in enabled) systemctl unmask "$1"; systemctl enable "$1" ;; disabled) systemctl unmask "$1"; systemctl disable "$1" ;; masked) systemctl mask "$1" ;; esac 2>/dev/null
  log "unit $1: $c -> $(en "$1")"; }
# bringup (B1): the station brought up by the put-back itself, every time, whatever device-sta-mode did (it runs
# under set -e: a failed `iw dev wlan0 set type managed` ends it before wpa_supplicant is restarted).
# R-P2-1: on a clean station nothing is restarted: a healthy lamp (phase 0, the tidies) never leaves the LAN.
# tidyap (F-P3-4, F2-P2): nginx stopped if it was inactive at the backup, the reg domain back: neither touches the
# Wi-Fi. Run by the bring-up and by the guard's clean-station stand-back (AP mode's leftovers when the kit died after C).
tidyap() {
  grep -q '^nginx [^ ]* active$' "$B/units" || [ "$(systemctl is-active nginx)" != active ] || { systemctl stop nginx 2>/dev/null; log "bring-up: nginx stopped (inactive at the backup)"; }
  rg=$(cat "$B/reg" 2>/dev/null); case $rg in ''|none) ;; *) o=$(regnow); [ "$o" = "$rg" ] || { iw reg set "$rg" 2>/dev/null; log "bring-up: reg domain $o -> $(regnow) (the backup's $rg)"; } ;; esac; }
bringup() {
  # F-P3-4: nginx and the reg domain back even on a clean station
  tidyap
  if clean; then log "bring-up: a clean station: no wpa/dhcpcd restart, LAN $(lan)"; return 0; fi
  systemctl stop hostapd dnsmasq 2>/dev/null; systemctl disable hostapd dnsmasq 2>/dev/null
  ip addr del 192.168.100.1/24 dev wlan0 2>/dev/null
  if [ "$(iw dev wlan0 info 2>/dev/null | awk '/type/{print $2}')" != managed ]; then ip link set wlan0 down 2>/dev/null; fi
  ip link set wlan0 up 2>/dev/null
  iw dev wlan0 set type managed 2>/dev/null || { ip link set wlan0 down 2>/dev/null; iw dev wlan0 set type managed 2>/dev/null || log "bring-up: iw set type managed failed (tolerated)"; ip link set wlan0 up 2>/dev/null; }
  # every kit-touched file by hash once more, just before wpa_supplicant reads; the Wi-Fi file is never restored
  # (the kit's cut is runtime only and device-ap-mode never edits it), a difference only logged (T-P2)
  [ "$(h "$R$WF")" = "$(sed -n "s#^$WF ##p" "$B/manifest")" ] || log "bring-up: $WF differs from setup's (re-provisioned?): left as it is"
  restore
  [ "$(en wpa_supplicant@wlan0)" = enabled ] || { systemctl unmask wpa_supplicant@wlan0; systemctl enable wpa_supplicant@wlan0; log "bring-up: wpa_supplicant@wlan0 now $(en wpa_supplicant@wlan0)"; } 2>/dev/null
  systemctl restart wpa_supplicant@wlan0
  i=0; until wpa_cli -i wlan0 enable_network all >/dev/null 2>&1 || [ $i -ge 10 ]; do sleep 1; i=$((i+1)); done
  systemctl restart dhcpcd
  i=0; while [ -z "$(lan)" ] && [ $i -lt 45 ]; do sleep 2; i=$((i+1)); done
  log "bring-up: $( [ -n "$(lan)" ] && echo "LAN $(lan) after $((i*2)) s" || echo 'NO LAN after 90 s')"; }
# creboot WHY (B1): no LAN after the bring-up: a counted reboot (files and units are back, so the boot comes up a
# station and the boot leg runs again at boot + GUARD_MIN). At most MAXRB per kit run; then logged, no reboot.
creboot() { c=$(cat "$D/reboots" 2>/dev/null || echo 0)
  if [ "$c" -ge "$MAXRB" ]; then log "NO LAN after the bring-up ($1): $c of $MAXRB counted reboots used: NO REBOOT. LOOK NOW"
    echo "$(date +%H:%M) seventh: $GU: NO LAN after the bring-up, counted reboots used up ($c): LOOK NOW" >> "$LO"; sync; return 1; fi
  c=$((c+1)); echo "$c" > "$D/reboots"; log "NO LAN after the bring-up ($1): counted reboot $c of $MAXRB"
  echo "$(date +%H:%M) seventh: $GU: NO LAN after the bring-up: counted reboot $c of $MAXRB" >> "$LO"; sync
  systemctl --no-block reboot; }
# push TTL (F-P2): the expiry pushed to now + TTL at each pet (atomic write)
push() { echo $(( $(date +%s) + ${1:-10800} )) > "$B/expires.new" && mv "$B/expires.new" "$B/expires" && sync; }
# unarm: the guard's units removed now (no stop of the guard service: a run in progress ends by itself; after the
# daemon-reload systemd drops the removed units at once).
unarm() { systemctl disable --now $GU.timer $GP.timer $GW.service 2>/dev/null
  for u in $GU.service $GU.timer $GP.timer $GW.service; do rm -f "$SU/$u"; done; sync; systemctl daemon-reload
  systemctl reset-failed $GU.timer $GP.timer $GW.service 2>/dev/null; }
# expire (S4, R-P1): a guard run past the stored expiry (setup + 3 h, pushed at each pet) removes its units and exits 0 FIRST: before
# the lock and before any restore, so a leftover guard never brings back an old Wi-Fi file. The lamp clock may be
# behind after a boot without NTP (no RTC): that only delays the expiry. Returns 1 when not expired.
expire() { x=$(cat "$B/expires" 2>/dev/null); case $x in ''|*[!0-9]*) x=0 ;; esac   # F-P3-1: unreadable = expired
  [ "$(date +%s)" -gt "$x" ] || return 1
  log "GUARD EXPIRED (setup + TTL, $x, passed): units removed, nothing restored, then $D"; unarm
  n=$(ls "$SU"/seventh-st4863-* 2>/dev/null | wc -l | tr -d ' ')
  # T-P3-2: $D last, and only once no unit is left (dash already holds this script open)
  [ "$n" = 0 ] && rm -rf "$D" && sync
  echo "$(date +%H:%M) seventh: $GU expired: units removed ($n left), $D $( [ -d "$D" ] && echo kept || echo removed)" >> "$LO"; }
# putback force|ifneeded <why>: the guard's body (force, why=guard) and the kit's put-back (run by the kit as its own
# unit). Under the lock: recover stopped, xcheck, files back by hash, device-sta-mode only when not a clean station,
# files again, unit states and hostname as found, then the bring-up (always), then checks.
putback() { m=$1; why=$2
  [ "$why" = guard ] && expire && return 0
  # T-P2: the guard never fights a person: setup running, a Wi-Fi change pending (.lamp-previous) or no done marker
  # (a re-provision): it logs and exits 0, touching nothing
  if [ "$why" = guard ]; then sa=$(systemctl is-active lamp-setup 2>/dev/null)
    if case $sa in active|activating|reloading) true ;; *) false ;; esac || ls "$R"/etc/wpa_supplicant/*.lamp-previous >/dev/null 2>&1 || [ ! -e "$R/var/lib/lampos-setup/done" ]; then
      log "GUARD STANDS BACK: lamp-setup $sa, .lamp-previous $(ls "$R"/etc/wpa_supplicant/*.lamp-previous 2>/dev/null | wc -l | tr -d ' '), done marker $( [ -e "$R/var/lib/lampos-setup/done" ] && echo yes || echo no): nothing touched"; return 0; fi
  fi
  # F-P3-2: a partial backup is refused before anything is touched (recover left running), no reboot. F2-P2: before
  # the stand-back, whose check reads the manifest and the wants line.
  for f in manifest recover.exec wants units; do [ -s "$B/$f" ] || { log "PUTBACK FAILED ($why): backup incomplete ($f missing): nothing touched, no reboot"; return 1; }; done
  # F-P2: nothing of the kit at work (no arm unit, no kit drop-in) on a clean station whose files and wants link equal
  # the backup by hash (F2-P2: check; the Wi-Fi file is only reported there, never counted): nothing to restore; nginx
  # and the reg domain tidied (tidyap). Its line is the guard's OK line, so phase 0 and B/C still see the boot leg's run.
  if [ "$why" = guard ] && ! systemctl is-active seventh-st4863-arm-A seventh-st4863-arm-B seventh-st4863-arm-C 2>/dev/null | grep -qxE 'active|activating' && [ -z "$(ls -d "$R$RE"/zz-seventh-* "$R$RR"/zz-seventh-* 2>/dev/null)" ] && check >/dev/null 2>&1 && clean; then
    tidyap; log "PUTBACK OK (guard): stood back: a clean station, files equal to the backup, no arm unit, no kit drop-in: nothing restored $(state)"; return 0; fi
  lk "$why" || { [ "$why" = guard ] && log "the guard goes on without the lock"; } || { log "PUTBACK FAILED ($why): the lock is held"; return 1; }
  log "PUTBACK $m ($why): found $(state)"
  systemctl stop lamp-setup-recover 2>/dev/null; systemctl stop 'seventh-st4863-arm-*' 2>/dev/null
  xcheck; restore; s=skipped
  if clean; then s=skipped-clean
  else timeout 120 "$STA" >> "$D/log.txt" 2>&1; s=$?; log "device-sta-mode exit $s"; restore; fi
  while read -r u e a; do case $u in hostapd|dnsmasq|avahi-daemon|dhcpcd|wpa_supplicant@wlan0|nginx) setu "$u" "$e" ;; esac; done < "$B/units"
  w=$(cat "$B/hostname.now" 2>/dev/null); [ -z "$w" ] || [ "$(hostname)" = "$w" ] || { hostname "$w"; log "hostname back to $w"; }
  bringup
  grep -q '^lamp-setup-recover [^ ]* active$' "$B/units" && systemctl start lamp-setup-recover
  n=0; check > "$D/check.last" || n=$?
  r="device-sta-mode=$s files-differ=$n restore-failed=$BAD dropins=$U build=$([ $XB = 0 ] && echo same || echo MOVED) reboots=$(cat "$D/reboots" 2>/dev/null || echo 0) $(state)"
  if [ "$n" = 0 ] && [ "$U" = 0 ] && [ -n "$(lan)" ] && ! apish; then log "PUTBACK OK ($why): $r"; ok=0; else log "PUTBACK FAILED ($why): $r"; ok=1; fi
  [ "$why" = guard ] && echo "$(date +%H:%M) seventh: $GU fired $(date +%T): $( [ $ok = 0 ] && echo 'station and files back' || echo "FAILED: $r")" >> "$LO"
  [ -n "$(lan)" ] || creboot "$why"
  return $ok; }
jsince() { journalctl -u lamp-setup-recover --since "@$1" --no-pager -o cat 2>/dev/null; }
# arm X WANT: one window, run as systemd-run unit seventh-st4863-arm-X, holding the lock (a guard run waits for it,
# up to LOCKW). A: hotspot then recover stopped. B: switch on (/etc drop-in), hotspot, reboot -ff. C: as B with the
# switch in /run (gone at boot). All: the installed lamp-setup.
arm() { X=$1; WANT=$2; HOLD=${3:-10}
  res() { echo "$X $*" > "$D/arm-$X.result"; sync; log "arm $X RESULT $*"; }
  rm -f "$D/arm-$X.result"
  exec 9>>"$D/lock"; flock -w 30 9 || { res "VOID lock-held"; return 1; }
  g0=$(grep -c 'PUTBACK force (guard)' "$D/log.txt" 2>/dev/null); g0=${g0:-0}
  n=0; check > "$D/check.before-$X" || n=$?; [ "$n" = 0 ] || { res "VOID files-differ-before $n"; return 1; }
  grep -q "^info $WF same" "$D/check.before-$X" || { res "VOID wifi-file-changed-since-setup"; return 1; }
  read -r wp ws wr < "$B/recover.exec"; p=$(xp)
  [ "$p" = "$wp" ] && [ "$(h "$R$p")" = "$ws" ] && [ "$ws" = "$WANT" ] || { res "VOID build-moved: recover's program $p $(h "$R$p" | cut -c1-12), want $wp $(echo "$WANT" | cut -c1-12)"; return 1; }
  mkdir -p "$R$RE" "$R$RR"; for f in $KD; do rm -f "$R$f"; done
  if [ "$X" = B ]; then o=$R$RE; else o=$R$RR; fi
  printf '[Service]\nEnvironment=LAMP_WIFI_RECOVERY=on\n' > "$o/zz-seventh-st4863-on.conf"
  sync; systemctl daemon-reload; t0=$(date +%s); systemctl restart lamp-setup-recover
  i=0; until jsince "$t0" | grep -q 'recover: watching'; do i=$((i+1)); [ $i -ge 20 ] && { res "VOID recover-not-watching"; return 1; }; sleep 1; done
  run=$(sha < "$R/proc/$(systemctl show -p MainPID --value lamp-setup-recover)/exe")
  [ "$run" = "$WANT" ] || { res "VOID recover-runs-$(echo "$run" | cut -c1-12)"; return 1; }
  log "arm $X: recover on, $(echo "$run" | cut -c1-12); $(state)"
  tc=$(date +%s); wpa_cli -i wlan0 disable_network all >/dev/null; log "arm $X: cut at $tc (disable_network all, nothing written)"
  i=0; while [ -n "$(lan)" ]; do i=$((i+1)); [ $i -ge 30 ] && { wpa_cli -i wlan0 enable_network all >/dev/null; res "VOID cut-did-not-take"; return 1; }; sleep 1; done
  i=0; until jsince "$tc" | grep -q "opening setup's hotspot"; do i=$((i+1)); [ $i -ge 75 ] && { wpa_cli -i wlan0 enable_network all >/dev/null; res "VOID no-hotspot-150s"; return 1; }; sleep 2; done
  to=$(date +%s); sleep 3; log "arm $X: hotspot open $((to-tc)) s after the cut: $(state)"; check > "$D/check.ap-$X"; sleep "$HOLD"
  if [ "$X" = A ]; then ts=$(date +%s); systemctl stop lamp-setup-recover
    i=0; while [ -z "$(lan)" ] && [ $i -lt 45 ]; do sleep 2; i=$((i+1)); done
    res "stopped=$ts open=$((to-tc)) lan_after=$( [ -n "$(lan)" ] && echo $(( $(date +%s) - ts )) || echo none) guard_in_window=$(( $(grep -c 'PUTBACK force (guard)' "$D/log.txt") - g0 )) $(state)"; return 0; fi
  res "rebooting at $(date +%s) open=$((to-tc)) from-boot=$(bid)"; sync; systemctl reboot -ff; }
# report [X]: this boot's facts for the verdicts (monotonic seconds from boot); with X, only arm X's result (S6).
#   lan=   the first LAN address this boot, by seventh-st4863-lan.service's poll (uptime s; this image logs no lease line)
#   rec=   lamp-setup-recover's journal lines this boot;  guard= the guard's lines in the kit log this boot
#   gj=    the guard service's own journal lines this boot (`journalctl -b -u seventh-st4863-guard.service`)
#   gtimer/gpet: LastTriggerUSecMonotonic of the boot leg / pet leg, raw usec by busctl (0/none = not fired this
#          boot); gres: the service's Result; reboots: counted reboots so far
report() { X=${1:-}; b=$(bid); echo "boot=$b up=$(cut -d' ' -f1 /proc/uptime 2>/dev/null)"
  echo "lan=$(grep "b=$b .* LAN " "$D/log.txt" 2>/dev/null | head -1 | sed -n 's/.* up=\([0-9.]*\) .*/\1/p')"
  journalctl -b -u lamp-setup-recover -o short-monotonic --no-pager 2>/dev/null | grep -E 'lamp-setup: recover' | sed -E 's/^\[ *([0-9.]+)\][^:]*: /rec=\1 /' | head -20
  grep "b=$b .*PUTBACK" "$D/log.txt" 2>/dev/null | sed 's/^/guard=/'
  journalctl -b -u $GU.service -o short-monotonic --no-pager 2>/dev/null | grep -E 'PUTBACK (OK|FAILED|force)|XCHECK' | sed -E 's/^\[ *([0-9.]+)\][^:]*: /gj=\1 /'
  echo "gtimer=$(usec $GU.timer LastTriggerUSecMonotonic) gpet=$(usec $GP.timer LastTriggerUSecMonotonic) gres=$(systemctl show -p Result --value $GU.service) reboots=$(cat "$D/reboots" 2>/dev/null || echo 0)"
  echo "state=$(state)"; n=0; check > /dev/null || n=$?; echo "differ=$n"; [ -z "$X" ] || sed 's/^/result=/' "$D/arm-$X.result" 2>/dev/null
  echo "jfirst=$(journalctl -b -o short-monotonic --no-pager 2>/dev/null | head -1 | sed -n 's/^\[ *\([0-9.]*\)\].*/\1/p')"   # F-P3-5: this boot's oldest kept entry
  echo "build $(build)"; echo "seventh_dropins=$(ls "$R$RE" "$R$RR" 2>/dev/null | grep -c seventh) oldstage=$(ls -d "$R"/opt/lampOS/releases/seventh-* 2>/dev/null | wc -l | tr -d ' ')"; }
# lanwatch: seventh-st4863-lan.service's body: one log line when wlan0 first has a LAN address this boot (10 min at most).
lanwatch() { i=0; while [ $i -lt 600 ]; do a=$(lan); [ -n "$a" ] && { log "LAN $a"; return 0; }; sleep 1; i=$((i+1)); done; log "LAN none in 600 s"; }
# disarm (B2): the timers off first (no new run), then a guard run in progress is waited for (6 min at most), never
# stopped; the station checked before the units go and again after. 0 only when no unit is left and a clean station.
disarm() { systemctl disable --now $GU.timer $GP.timer 2>/dev/null
  i=0; while [ "$(systemctl is-active $GU.service 2>/dev/null)" = activating ] && [ $i -lt 72 ]; do sleep 5; i=$((i+1)); done
  if [ "$(systemctl is-active $GU.service 2>/dev/null)" = activating ]; then log "DISARM REFUSED: a guard run still in progress after 360 s: units kept"; echo "guard still running: not disarmed"; return 1; fi
  [ $i = 0 ] || log "disarm: waited $((i*5)) s for a guard run to end"
  clean || { log "DISARM REFUSED: not a clean station: $(state)"; echo "not a clean station: not disarmed: $(state)"; return 1; }
  unarm; systemctl reset-failed $GU.service 2>/dev/null
  left=$(ls "$SU"/seventh-st4863-* 2>/dev/null | wc -l | tr -d ' ')
  if clean; then st=station; else st="NOT-STATION $(state)"; fi
  log "disarm: $left unit files left, after: $st"
  echo "guard $left unit files left, boot leg $(en $GU.timer)/$(systemctl is-active $GU.timer 2>/dev/null), after: $st"
  [ "$left" = 0 ] && [ "$st" = station ]; }
facts() { echo "pin=$(python3 -I -c 'import json,sys;i=json.load(open(sys.argv[1])).get("installed") or {};print(i.get("sequence","none"),i.get("commit","none"))' "$R/var/lib/lampos-update/state.json" 2>/dev/null)"
  echo "marker=$(test -e "$R/var/lib/lampos-setup/done" && echo yes || echo no) recover_en=$(en lamp-setup-recover) recover=$(systemctl is-active lamp-setup-recover) setup=$(systemctl is-active lamp-setup) previous=$(ls "$R"/etc/wpa_supplicant/*.lamp-previous 2>/dev/null | wc -l | tr -d ' ')"
  echo "switch=$(systemctl show -p Environment --value lamp-setup-recover | grep -c 'LAMP_WIFI_RECOVERY=on') dropins=$(ls "$R$RE" "$R$RR" 2>/dev/null | grep -c conf) seventh=$(ls "$R$RE" "$R$RR" 2>/dev/null | grep -c seventh) leftovers=$(ls -d "$D" "$R"/opt/lampOS/releases/seventh-* "$R"/etc/systemd/system/seventh-st4863-* 2>/dev/null | wc -l | tr -d ' ') oldstage=$(ls -d "$R"/opt/lampOS/releases/seventh-* 2>/dev/null | wc -l | tr -d ' ')"
  # the fix's lines in the installed program: the repair and the close (PR #4931's main.rs)
  build; p=$(xp); echo "xa=$(systemctl show -p ExecStart --value lamp-setup-recover | sed -n 's/.*argv\[\]=[^ ]* \([^;]*\);.*/\1/p' | sed 's/ *$//' | tr ' ' ,) instfix=$(grep -a -c 'recover: the radio was left a hotspot: back to the station' "$R$p" 2>/dev/null) instclose=$(grep -a -c 'recover: the hotspot closed: back to the station' "$R$p" 2>/dev/null) setup_en=$(en lamp-setup)"
  echo "addr=$(cat "$R/etc/dnsmasq.d/99-lamp.conf" 2>/dev/null | grep -c '^address=/#/') ssid=$(grep -c '^[[:space:]]*ssid=' "$R$WF") sta=$(test -x "$STA" && echo yes || echo no) varfree=$(df -k "$R/var/lib" | awk 'NR==2{print $4}') flock=$(command -v flock >/dev/null && echo yes || echo no) busctl=$(command -v busctl >/dev/null && echo yes || echo no)"
  echo "units=$(systemctl is-active lamp-voice lamp-eyes lamp-controls lamp-body | tr '\n' ' ')"
  echo "mute=$(tail -1 "$R/run/lampos-bus/voice/mute/latest" 2>/dev/null | grep -o '"muted":[a-z]*')"
  echo "state=$(state)"; }
c=${1:-}; [ $# -gt 0 ] && shift
case $c in facts|build|backup|check|selftest|state|putback|xcheck|arm|report|lanwatch|disarm|restore|usec|push) "$c" "$@" ;; *) echo "usage: st.sh facts|build|backup|check|selftest|state|putback|xcheck|arm|report|lanwatch|disarm|restore|usec|push"; exit 2 ;; esac
EOF
}
# The guard's units, all in /etc/systemd/system. No ordering or requirement lines of their own (only systemd's
# default deps): nothing ties them to lamp-setup units, /opt or a mount.
guard_units() { cat <<EOF
=== /etc/systemd/system/$GU.service
[Unit]
Description=Seventh #4863 kit guard: kit drop-ins off, files back by hash, device-sta-mode if not a station, its own station bring-up (removed at the verified put-back or at its expiry, setup + 3 h pushed at each pet)
# R-P2-3: no start limit and no StartLimitAction/FailureAction: it retries every 5 min for ever; reboots come only
# from st.sh's counted creboot. A missing or broken st.sh fails the unit, never reboots the lamp.
StartLimitIntervalSec=0
[Service]
Type=oneshot
ExecStart=/bin/sh $D/st.sh putback force guard
# R-P3, every blocking step at its worst: lock 660 + stop recover 90 + its ExecStopPost 90 + stop of arm units 90
# + xcheck/restore 5 + device-sta-mode 120 + bring-up (stop hostapd/dnsmasq 90, stop nginx 90, restart wpa 90,
# enable_network 10, restart dhcpcd 90, LAN wait 90) + start recover 90 + checks 10 = 1705 s
TimeoutStartSec=1800
Restart=on-failure
RestartSec=300
=== /etc/systemd/system/$GU.timer
[Unit]
Description=Seventh #4863 kit guard, boot leg: ${GUARD_MIN} min after every boot
[Timer]
OnBootSec=${GUARD_MIN}min
AccuracySec=5s
[Install]
WantedBy=timers.target
=== /etc/systemd/system/$GP.timer
[Unit]
Description=Seventh #4863 kit guard, pet leg: ${GUARD_MIN} min after the kit's last pet (started by the kit only)
[Timer]
OnActiveSec=${GUARD_MIN}min
AccuracySec=5s
Unit=$GU.service
=== /etc/systemd/system/$GW.service
[Unit]
Description=Seventh #4863 kit: logs when wlan0 first has a LAN address this boot (observation only)
[Service]
Type=simple
ExecStart=/bin/sh $D/st.sh lanwatch
[Install]
WantedBy=multi-user.target
EOF
}

# ---- the only ways to the lamp. DRY: L/LI stubbed, each command printed, root scripts kept and syntax-checked.
# The fakes are systemd-252-shaped: *USec* properties from busctl ("t <usec>"), never `systemctl show` numbers.
if [ -n "$DRY" ]; then rm -rf "$DD"; mkdir -p "$DD"
  L="echo [lamp]"; LI="echo [lamp-stdin]"
  XF=feed0000000000000000000000000000000000000000000000000000000000ab   # the fakes' installed lamp-setup sha256
  XB="exec=/opt/lampOS/setup/current/lamp-setup xsha=$XF rel=$XF switching=0"
  ST="state=wpa=enabled/active hostapd=disabled/inactive dnsmasq=disabled deny=0 type=managed lan=192.168.1.50/24 hot=0 wpa_state=COMPLETED recover=active wants=1"
  GM=$((GUARD_MIN*60))
  GOK="gj=$GM.9 PUTBACK OK (guard): device-sta-mode=skipped-clean files-differ=0 restore-failed=0 dropins=0 build=same reboots=0"
  GJ="gj=$GM.3 PUTBACK force (guard): found wpa=enabled/active hostapd=disabled/inactive dnsmasq=disabled deny=0 type=managed"
  fake() { case $1 in   # pass-shaped replies, so the verdict code runs
    facts) [ -n "${FAKE_FACTS:-}" ] && { echo "$FAKE_FACTS"; return; }; printf '%s\n' "pin=$EXPECT_SEQ ${EXPECT_COMMIT}0000" "marker=yes recover_en=enabled recover=active setup=inactive previous=0" "switch=0 dropins=0 seventh=0 leftovers=0 oldstage=0" \
      "$XB" "xa=recover,--config,/etc/lampOS/setup/serve.json,--enable instfix=1 instclose=1 setup_en=enabled" \
      "addr=0 ssid=1 sta=yes varfree=900000 flock=yes busctl=yes" "units=active active active active " 'mute="muted":false' "$ST" ;;
    build) echo "$XB" ;; install) echo "st.sh $(lampsh | shasum -a 256 | cut -c1-12)"; echo "backup ok: every copy matches its hash" ;; boot) if [ -n "${FAKE_VOID:-}" ]; then echo boot-same; else echo "boot-$RANDOM"; fi ;;
    selftest) printf '%s\n' "SELFTEST restore OK" "SELFTEST guard boot=enabled/OnBootSec=${GUARD_MIN}min pet=active next=$((GM+40))000000 lan=enabled verify=clean deps=none tools=ok reboots=0/2 expires=1794000000 st.sh x" "SELFTEST ARMED" ;;
    report) printf '%s\n' boot=boot-1 lan=21.4 "gj=$GM.4 PUTBACK OK (guard): stood back: a clean station, no arm unit, no kit drop-in: nothing touched ${ST#state=}" \
      "gtimer=${GM}204511 gpet=0 gres=success reboots=0" "$ST" differ=0 "build $XB" "seventh_dropins=0 oldstage=0" ;;
    report-A) printf '%s\n' boot=boot-1 "rec=0 lamp-setup: $CLOSE_RE" "gtimer=${GM}204511 gpet=0 gres=success reboots=0" "$ST" differ=0 "result=A stopped=1 open=95 lan_after=12 guard_in_window=0 wpa=enabled/active hostapd=disabled/inactive deny=0 type=managed lan=192.168.1.50/24" "build $XB" ;;
    report-B|report-C) printf '%s\n' boot=boot-9 lan=24.5 "rec=9.1 lamp-setup: $REPAIR_RE" ${1#report-} \
      "guard=10:06:00 b=boot-9 up=$GM.3 PUTBACK force (guard): found wpa=enabled/active hostapd=disabled/inactive deny=0 type=managed" "$GOK" "gtimer=${GM}204511 gpet=0 gres=success reboots=0" "$ST" differ=0 "result=${1#report-} $( [ "${FAKE_VOID:-}" = B ] && echo 'VOID no-hotspot-150s' || echo 'rebooting at 1 open=95 from-boot=boot-8')" "build $XB" "seventh_dropins=0 oldstage=0" \
      | sed 's/^B$/rec=9.4 lamp-setup: recover: watching/; s/^C$/rec=9.4 lamp-setup: recover: off (LAMP_WIFI_RECOVERY is not on)/' ;;
    putback-start) PBN=$((${PBN:-0}+1)) ;;   # v10: FAKE_PBLOST=N: the first N starts are lost (no unit ran)
    putback-read) [ "${PBN:-1}" -le "${FAKE_PBLOST:-0}" ] && { echo inactive; return; }; printf '%s\n' inactive "PUTBACK OK ($(grep -oE 'kit-[a-zA-Z0-9-]+' <<< "$2" | head -1)): device-sta-mode=skipped-clean files-differ=0 restore-failed=0 dropins=0 build=same reboots=0 ${ST#state=}" ;;
    left) if [ -n "${FAKE_GONE:-}" ]; then echo "0 0"; else echo "2 0"; fi ;;
    mute-guard) echo yes ;; result) [ "${FAKE_VOID:-}" = B ] && echo "B VOID no-hotspot-150s" ;;
    disarm) printf '%s\n' "guard 0 unit files left, boot leg none/inactive, after: station" "REMOVED $D" ;; *) echo "" ;; esac; }
  lamp() { echo "[lamp] $2" >&2; fake "$1" "$2"; }
  root() { local f; f=$DD/$(printf %02d $(( $(ls "$DD" | wc -l) + 1 )))-$1.sh; cat > "$f"; echo "[lamp root sh] $(wc -l < "$f" | tr -d ' ') lines -> ${f##*/} (sh -n: $(sh -n "$f" 2>&1 && echo ok), dash -n: $(dash -n "$f" 2>&1 && echo ok))" >&2; fake "$1"; }
  reach() { echo "[mac] ssh -o ConnectTimeout=5 orangepi@lamp-52e6.local true; ping -c1 lamp-52e6.local" >&2; return 0; }
else
  lamp() { $L "$2"; }
  root() { shift; $LI "sudo -n sh -s -- $*"; }
  # A fresh connection (no ControlMaster): the shared one is dead once the Wi-Fi went.
  reach() { ping -c1 -t3 lamp-52e6.local >/dev/null 2>&1 && ssh -n -o BatchMode=yes -o ConnectTimeout=5 -o ControlPath=none orangepi@lamp-52e6.local true 2>/dev/null; }
fi
st() { local t=$1; shift; lamp "$t" "sudo -n sh $D/st.sh $*"; }
bootid() { lamp boot 'cat /proc/sys/kernel/random/boot_id | cut -c1-8'; }
get() { sed -n "s/^.*$1=\([^ ]*\).*/\1/p" <<< "$2" | head -1; }
# waitreach <secs> [old-boot]: the lamp answers again (on a new boot when old-boot is given). Prints the seconds.
waitreach() { local t0 b; t0=$(date +%s)
  while :; do if reach; then [ -z "${2:-}" ] && break; b=$(bootid 2>/dev/null); [ -n "$b" ] && [ "$b" != "$2" ] && break; fi
    [ -n "$DRY" ] && break; [ $(( $(date +%s) - t0 )) -ge "$1" ] && { echo none; return 1; }; sleep 10; done
  echo $(( $(date +%s) - t0 )); }
# waitguard <fake> [X]: after a reboot, the boot leg's run in this boot: the guard service's own journal line
# `PUTBACK OK|FAILED (guard)` (report's gj=). Prints the report. Every reboot waits for it, so no pet comes before
# boot + GUARD_MIN (the boot leg would then fire inside the next window).
waitguard() { local t0 r; t0=$(date +%s)
  until r=$(st "$1" report ${2:-} 2>/dev/null) && grep -qE '^gj=[0-9.]+ PUTBACK (OK|FAILED) \(guard\)' <<< "$r"; do renew
    [ -n "$DRY" ] && break
    [ $(( $(date +%s) - t0 )) -ge $(( GUARD_MIN*60 + 300 )) ] && { echo "$r"; return 1; }; sleep 20; done
  echo "$r"; }
# pet: the pet leg restarted (OnActiveSec from now) and the guard checked armed (selftest), before every window.
pet() { local r; r=$(root selftest <<EOF
systemctl restart $GP.timer && sh $D/st.sh push $TTL && sh $D/st.sh selftest
EOF
); echo "$r"; grep -q '^SELFTEST ARMED' <<< "$r" || abort "the guard is not armed: $r"; }
# renew: lamp-lock's own status (0 ours, 1 lost to another or released, 3 the lamp not asked). The loops ignore it (the
# lamp may be off its network); before each window and after each wait, a failure aborts (fin: the put-back only).
# DRY: FAKE_RENEW_OK=N succeeds N times, then fails as a lost lock does.
renew() { [ -n "$DRY" ] && { echo "[lock] $LOCK lamp-52e6 renew seventh $SESSION" >&2; [ -z "${FAKE_RENEW_OK:-}" ] && return 0; [ "$FAKE_RENEW_OK" -gt 0 ] || return 1; FAKE_RENEW_OK=$((FAKE_RENEW_OK-1)); return 0; }; "$LOCK" lamp-52e6 renew seventh "$SESSION" >/dev/null 2>&1; }
# buildok: before each arm, recover's program is still the build preflight recorded (path, sha256 = its release
# folder's name) and lamp-update switched no build in the last 10 min. Otherwise the kit refuses (logged).
buildok() { local b; b=$(st build build)
  [ "$(get exec "$b")" = "$XEXEC" ] && [ "$(get xsha "$b")" = "$XSHA" ] && [ "$(get rel "$b")" = "$XSHA" ] || abort "recover's lamp-setup is no longer the build preflight saw ($XEXEC ${XSHA:0:12}): $b (lamp-update moved it?)"
  [ "$(get switching "$b")" = 0 ] || abort "lamp-update switched builds in the last 10 min: $b"; }
# kputback TAG (S2): the kit's put-back runs ON the lamp as its own unit (systemd-run --wait --collect, output
# appended to $D/putback.out, every step in $D/log.txt), so the bring-up's Wi-Fi restart, which drops this ssh, does
# not end it. Its result is read over a fresh ssh from the lamp log, by its tag. A counted reboot inside it: the new
# boot waited for and the put-back run again (3 tries). A start lost (the ssh failed before the unit began: inactive,
# no line for this call) is started again, 20 s apart, at most 3 times. Prints the result line; 0 on PUTBACK OK.
kputback() { local k id t b r s c n
  for k in 1 2 3; do id=$(date +%s)$k; t=kit-$1-$id; b=$(bootid 2>/dev/null); n=0
    c="sudo -n systemd-run --wait --collect -q --unit=seventh-st4863-putback-$id -p StandardOutput=append:$D/putback.out -p StandardError=append:$D/putback.out /bin/sh $D/st.sh putback ifneeded $t"
    lamp putback-start "$c" >/dev/null 2>&1
    s=$(date +%s)
    while :; do
      if waitreach 60 >/dev/null; then
        r=$(lamp putback-read "sudo -n sh -c 'systemctl is-active seventh-st4863-putback-$id; grep -F \"($t)\" $D/log.txt | grep -E \"PUTBACK (OK|FAILED)|counted reboot|NO REBOOT\" | tail -1'")
        case $r in active*|activating*) ;;
          *"counted reboot"*) break ;;
          *"PUTBACK OK ($t)"*) sed 1d <<< "$r"; return 0 ;;
          *"PUTBACK FAILED ($t)"*|*"NO REBOOT"*) sed 1d <<< "$r"; return 1 ;;
          inactive|failed) [ $n -ge 3 ] && { echo "PUTBACK unknown ($t): its unit never ran ($n restarts)"; return 1; }
            echo "  $t: the unit is $r with no line for this call: started again" >&2
            [ -n "$DRY" ] || sleep 20; n=$((n+1)); lamp putback-start "$c" >/dev/null 2>&1; s=$(date +%s); continue ;; esac
      fi
      [ -n "$DRY" ] && { echo "PUTBACK unknown ($t): $r"; return 1; }
      [ $(( $(date +%s) - s )) -ge 900 ] && { echo "PUTBACK unknown ($t): no result in 15 min"; return 1; }; sleep 10; done
    echo "  $t: a counted reboot; waiting for the new boot" >&2
    waitreach $(( GUARD_MIN*60 + 420 )) "$b" >/dev/null || { echo "PUTBACK unknown ($t): no new boot after its counted reboot"; return 1; }
  done; echo "PUTBACK FAILED (kit-$1): 3 tries"; return 1; }
# p0check REPORT: phase 0's checks (S1). The guard's first journal line this boot at boot + GUARD_MIN (+30 s); the
# boot leg's LastTriggerUSecMonotonic, raw usec from busctl, at boot + GUARD_MIN (+30 s); 'PUTBACK OK (guard)'; the
# pet leg not fired (0, or none: not loaded this boot); Result success; a clean station, files equal to the backup.
p0check() { local r=$1 g1 gok gt gp m=$((GUARD_MIN*60))
  g1=$(grep -m1 -E '^gj=[0-9.]+ ' <<< "$r" | sed -E 's/^gj=([0-9.]+).*/\1/')
  gok=$(grep -m1 -E '^gj=[0-9.]+ PUTBACK OK \(guard\)' <<< "$r" | sed -E 's/^gj=([0-9.]+).*/\1/')
  gt=$(get gtimer "$r"); gp=$(get gpet "$r")
  [ -n "$gok" ] || { echo "the guard's journal this boot has no 'PUTBACK OK (guard)'"; return 1; }
  awk -v t="$g1" -v m=$m 'BEGIN{exit !(t+0 >= m && t+0 <= m+30)}' || { echo "the guard's first journal line at boot + ${g1:-none} s, not boot + $GUARD_MIN min (+30 s)"; return 1; }
  { grep -qE '^[0-9]+$' <<< "$gt" && awk -v t="$gt" -v m=$m 'BEGIN{exit !(t/1e6 >= m && t/1e6 <= m+30)}'; } || { echo "the boot leg's LastTriggerUSecMonotonic '$gt' (raw usec wanted) is not boot + $GUARD_MIN min (+30 s)"; return 1; }
  case $gp in 0|none) ;; *) echo "the pet leg fired this boot ($gp): the boot leg is not proved"; return 1 ;; esac
  [ "$(get gres "$r")" = success ] || { echo "$GU.service Result is $(get gres "$r")"; return 1; }
  { grep -q '^differ=0' <<< "$r" && grep -q 'deny=0 type=managed lan=[0-9]' <<< "$r" && grep -q '^seventh_dropins=0 oldstage=0' <<< "$r"; } || { echo "after the guard the lamp is not a clean station"; return 1; }
  echo "boot leg fired at boot + $(awk -v t="$gt" 'BEGIN{printf "%.1f", t/1e6}') s, first guard line at boot + $g1 s, 'PUTBACK OK (guard)' at boot + $gok s"; }

# ---- preflight: read-only. Mac gates, then the lamp's facts.
F=
preflight() { local s
  echo "== st4863 $(date +%T): LIMIT=${LIMIT:-none} ARMS=$( [ -n "$PHASE0_ONLY" ] && echo "none (phase0 only)" || echo "$ARMS") GUARD_MIN=$GUARD_MIN EXPECT=$EXPECT_SEQ $EXPECT_COMMIT, needs $((MINLEFT/60)) min"
  [ -n "$LIMIT" ] || abort "LIMIT unset"
  [ "$(left)" -ge "$MINLEFT" ] || abort "$(left) s to LIMIT, the kit needs $MINLEFT s"
  otherplaying && abort "another lamp is PLAYING (#3336)"
  [ "$GUARD_MIN" -ge 5 ] && [ "$GUARD_MIN" -le 7 ] || abort "GUARD_MIN $GUARD_MIN: 5..7 (over the 4 min windows, under the lock's 9 min)"
  F=$(lampsh | root facts facts); echo "$F"
  set -- $(get pin "$F") $(sed -n 's/^pin=[^ ]* //p' <<< "$F")
  [ "${1:-}" = "$EXPECT_SEQ" ] && case ${2:-} in "$EXPECT_COMMIT"*) true ;; *) false ;; esac || abort "the installed build is not $EXPECT_SEQ ($EXPECT_COMMIT)"
  [ "$(get marker "$F")" = yes ] || abort "no done marker: not a set-up lamp"
  [ "$(get recover_en "$F")" = enabled ] || abort "lamp-setup-recover is not enabled: the boot repair would never run"
  [ "$(get setup "$F")" = inactive ] || abort "lamp-setup (setup itself) is running"
  [ "$(get previous "$F")" = 0 ] || abort "a .lamp-previous Wi-Fi file: a Wi-Fi change is pending"
  [ "$(get seventh "$F")" = 0 ] && [ "$(get leftovers "$F")" = 0 ] || abort "a seventh drop-in, $D, /opt/lampOS/releases/seventh-* (an older run's staging) or a guard unit is left: kit/st4863.sh back"
  [ "$(get switch "$F")" = 0 ] || abort "LAMP_WIFI_RECOVERY is already on in lamp-setup-recover: C needs it off at boot (ask Sixth before moving a product drop-in)"
  [ "$(get dropins "$F")" = 0 ] || echo "note: lamp-setup-recover has $(get dropins "$F") drop-in(s) of its own: left alone"
  [ "$(get xa "$F")" = recover,--config,/etc/lampOS/setup/serve.json,--enable ] || abort "recover's ExecStart arguments are not 'recover --config /etc/lampOS/setup/serve.json --enable': $(get xa "$F")"
  # the build gate: the lamp-setup recover runs is the build's own file (its sha256 = its release folder's name)
  XEXEC=$(get exec "$F"); XSHA=$(get xsha "$F")
  grep -qE '^[0-9a-f]{64}$' <<< "$XSHA" || abort "recover's program $XEXEC is not readable"
  [ "$XSHA" = "$(get rel "$F")" ] || abort "recover's program $XEXEC hashes to ${XSHA:0:12}, its release folder is $(get rel "$F"): not the build's own file"
  [ "$(get instfix "$F")" -ge 1 ] && [ "$(get instclose "$F")" -ge 1 ] || abort "the installed lamp-setup lacks the fix's repair or close line: not a build with PR #4931"
  echo "build ok: recover runs $XEXEC ${XSHA:0:12} (its release folder), with the repair and close lines"
  [ "$(get addr "$F")" = 0 ] && [ "$(get ssid "$F")" -ge 1 ] && [ "$(get sta "$F")" = yes ] || abort "not a station lamp with a saved network and device-sta-mode"
  [ "$(get switching "$F")" = 0 ] || abort "lamp-update switched builds in the last 10 min"
  [ "$(get varfree "$F")" -gt 51200 ] || abort "under 50 MB free in /var/lib"
  [ "$(get flock "$F")" = yes ] && [ "$(get busctl "$F")" = yes ] || abort "no flock or busctl on the lamp (the guard's lock, phase 0's raw timer read)"
  case $F in *wpa=enabled/active*hostapd=disabled*deny=0*type=managed*) ;; *) abort "the radio is not a clean station" ;; esac
  grep -q '^state=.* wants=1' <<< "$F" || abort "no $WLK link: the fix reads that as a left hotspot, so the lamp is not a clean station"
  get lan "$F" | grep -q . || abort "no LAN address on wlan0"
  grep '^units=' <<< "$F" | grep -qE 'inactive|failed|activating' && abort "a lamp unit is not active"
  grep -q '"muted":false' <<< "$F" || abort "voice/mute is not false"
}

# ---- setup: st.sh, the backup (files, unit states, recover's program and sha256), the guard's units; then the
# lamp-side DRY (restore on scratch, guard armed). The boot leg and lan watcher are enabled, not started: an
# OnBootSec already past fires at once when started; this boot is covered by the pet leg.
setup() { local r; SET=1
  r=$(root install <<EOF
set -e
install -d -m 0700 $D
cat > $D/st.sh.new <<'STEOF'
$(lampsh)
STEOF
sh -n $D/st.sh.new; mv $D/st.sh.new $D/st.sh; chmod 0700 $D/st.sh; sync
echo "st.sh \$(sha256sum < $D/st.sh | cut -c1-12)"
sh $D/st.sh backup $TTL
EOF
); echo "$r"; grep -q "st.sh $(lampsh | shasum -a 256 | cut -c1-12)" <<< "$r" || abort "st.sh on the lamp is not this kit's"
  grep -q 'backup ok' <<< "$r" || abort "the backup is not proved"
  GSET=1; r=$(root selftest <<EOF
$(guard_units | awk -v q="'" '/^=== /{if(f)print "UEOF"; f=$2; print "cat > " f " <<" q "UEOF" q; next} {print} END{print "UEOF"}')
sync; systemctl daemon-reload && systemctl enable $GU.timer $GW.service && systemctl restart $GP.timer && sync
sh $D/st.sh selftest
EOF
); echo "$r"; grep -q '^SELFTEST ARMED' <<< "$r" || abort "lamp-side DRY failed: $r"
  echo "SETUP OK: backup, guard armed (boot leg enabled, pet leg $GUARD_MIN min), restore proved on a scratch copy"; }

# ---- phase 0: the boot leg proved without leaving station mode: a plain reboot, then in this boot's journal of
# seventh-st4863-guard.service the line `PUTBACK OK (guard): ...` (gj=), the boot leg's LastTriggerUSecMonotonic at
# boot + GUARD_MIN (+30 s), the pet leg not fired (gpet=0), the service's Result success. Any one missing: abort.
phase0() { local b w r v
  pet; b=$(bootid); renew || abort "the lamp lock was lost (lease expired while 52e6 was off the network): no further arm (lamp-lock exit $?)"
  lamp reboot "sudo -n sh -c 'sync; systemd-run -q --on-active=3 systemctl reboot'"   # R-P3: a plain reboot, as a station
  [ -n "$DRY" ] || sleep 20
  w=$(waitreach 300 "$b") || abort "phase 0: the lamp is not back in 5 min after a station-mode reboot (no AP step ran: the image's own boot failed)"
  echo "phase 0: back on a new boot after $w s"; renew || abort "the lamp lock was lost (lease expired while 52e6 was off the network): no further arm (lamp-lock exit $?)"
  r=$(waitguard report) || abort "phase 0: no 'PUTBACK OK|FAILED (guard)' in this boot's journal of $GU.service by boot + $((GUARD_MIN+5)) min: the boot leg did not fire. $r"
  echo "$r"
  v=$(p0check "$r") || abort "phase 0: $v"
  rec "ST4863 phase0 PASS: $v, pet leg not fired, LAN at boot + $(get lan "$r") s (a normal boot), files equal to the backup"; }

# ---- one arm: build check, pet + renew + otherplaying, the lamp-side window, then wait, read, judge, tidy.
armrun() { local X=$1 want=$XSHA b w r v=
  k=0; while otherplaying; do [ $k -ge 3 ] && abort "another lamp still PLAYING (the lamp says its hotspot line)"; [ -n "$DRY" ] && break; sleep 60; k=$((k+1)); done
  buildok
  pet; renew || abort "the lamp lock was lost (lease expired while 52e6 was off the network): no further arm (lamp-lock exit $?)"; b=$(bootid)
  echo "== arm $X $(date +%T): the lamp leaves the LAN in ~20 s; the pet leg fires at $(date -v+${GUARD_MIN}M +%H:%M) unless petted"
  lamp start "sudo -n systemd-run -q --collect --unit=seventh-st4863-arm-$X /bin/sh $D/st.sh arm $X $want $HOLD"
  [ -n "$DRY" ] || sleep 40
  if [ "$X" = A ]; then local t0; t0=$(date +%s)
    # bounded: the window (~5 min) + the pet leg's put-back; then the kit stops and fin puts back
    until w=$(waitreach $(( GUARD_MIN*60 + 300 ))) && r=$(st report-A "report A") && grep -q "^result=A " <<< "$r"; do
      [ "$w" = none ] && abort "arm A: the lamp is not back in $((GUARD_MIN+5)) min: run kit/st4863.sh back"; [ -n "$DRY" ] && break
      [ $(( $(date +%s) - t0 )) -ge $(( GUARD_MIN*60 + 600 )) ] && abort "arm A: no result=A in $((GUARD_MIN+10)) min: run kit/st4863.sh back"; sleep 10; done
  else local t0 nb v0=; t0=$(date +%s); RFF=1
    # T-P3-3: a B/C window that ends VOID does not reboot: its arm-X.result is read on the same boot, no 12 min wait
    while :; do
      if reach; then nb=$(bootid 2>/dev/null); [ -n "$nb" ] && [ "$nb" != "$b" ] && break
        v0=$(lamp result "sudo -n cat $D/arm-$X.result 2>/dev/null"); case $v0 in "$X VOID"*) break ;; esac; fi
      [ -n "$DRY" ] && break
      [ $(( $(date +%s) - t0 )) -ge $(( GUARD_MIN*60 + 420 )) ] && abort "arm $X: no new boot in $((GUARD_MIN+7)) min: run kit/st4863.sh back"; sleep 10; done
    case $v0 in "$X VOID"*) echo "arm $X: VOID without a reboot: $v0"; r=$(st report-$X "report $X") ;;
      *) # F-P3-5: recover's lines read at once after the boot (the journal keeps ~28 min at SystemMaxUse=20M)
         r0=$(st report-$X "report $X")
         r=$(waitguard report-$X "$X") || abort "arm $X: the boot leg did not run this boot: run kit/st4863.sh back. $r"
         grep -q '^rec=' <<< "$r" || r=$(printf '%s\n%s' "$r" "$(grep '^rec=' <<< "$r0")") ;; esac; fi
  renew || abort "the lamp lock was lost (lease expired while 52e6 was off the network): no further arm (lamp-lock exit $?)"; echo "$r" | sed 's/^/  /'
  verdict "$X" "$r"
  r=$(kputback after-$X); echo "  $r"; grep -q 'PUTBACK OK' <<< "$r" || abort "the tidy after arm $X failed: no further AP step"; }

# verdict X report: PASS/FAIL/VOID from the report's lines (see the test script).
verdict() { local X=$1 r=$2 lan reb
  lan=$(get lan "$r")
  # the build gate after the window: recover's program is still the one preflight recorded
  if [ "$(get xsha "$(grep '^build ' <<< "$r")")" != "$XSHA" ]; then rec "ST4863 $X VOID recover's lamp-setup changed in the window: $(grep '^build ' <<< "$r")"; return; fi
  case $X in
  A) local res; res=$(grep '^result=A ' <<< "$r")   # report A prints only arm-A.result (S6)
     case $res in *VOID*) v="VOID ${res#*VOID }" ;; *lan_after=none*) v="FAIL not back on the LAN within 90 s of recover's stop" ;;
       *) if grep -qE "$NOSTART_RE" <<< "$r"; then v="FAIL the close could not start the station"
          elif ! grep -qF "$CLOSE_RE" <<< "$r"; then v="FAIL back on the LAN but no close line ('$CLOSE_RE')"
          elif ! grep -q '^differ=0' <<< "$r"; then v="FAIL files differ from the backup after the close"
          elif ! grep -q 'guard_in_window=0 ' <<< "$res"; then v="VOID the guard fired in the window"
          else v="PASS $(grep -oE 'open=[0-9]+ lan_after=[0-9]+' <<< "$res")"; fi ;; esac ;;
  B|C) reb=$(grep "^result=$X " <<< "$r")   # only this arm's result (report X reads arm-X.result alone: S6)
     case $reb in *VOID*) v="VOID ${reb#*VOID }" ;;
     *) if grep -qE "$NOSTART_RE" <<< "$r"; then v="FAIL the boot repair could not start the station"
        # F-P3-5: the kernel's first line is at boot + 0 s and rotation drops whole files (minutes): > 30 s is a rotated start
        elif ! grep -q '^rec=' <<< "$r" && awk -v j="$(get jfirst "$r")" 'BEGIN{exit !(j+0 > 30)}'; then v="VOID no recover line kept: this boot's journal starts at boot + $(get jfirst "$r") s (rotated)"
        elif ! grep -qF "$REPAIR_RE" <<< "$r"; then v="FAIL no boot repair line ('$REPAIR_RE'): the installed lamp-setup did not repair the left hotspot"
        elif [ -z "$lan" ] || ! awk -v l="$lan" -v m="$BOOT_LAN_MAX" 'BEGIN{exit !(l+0 <= m+0)}'; then v="FAIL no LAN address on wlan0 by boot + $BOOT_LAN_MAX s (${lan:-none})"
        elif grep -q "opening setup's hotspot" <<< "$r"; then v="FAIL the hotspot opened again this boot"
        elif grep -qE '^guard=.*PUTBACK force \(guard\): found .*(wpa=masked|hostapd=enabled|deny=1|wants=0)' <<< "$r"; then v="FAIL the guard found AP mode"
        elif [ "$X" = C ] && ! grep -q 'off (LAMP_WIFI_RECOVERY is not on)' <<< "$r"; then v="VOID the switch was not off at boot"
        elif ! grep -q '^differ=0' <<< "$r"; then v="FAIL files differ from the backup"
        else v="PASS LAN at boot + $lan s, $(grep -m1 -F "$REPAIR_RE" <<< "$r" | grep -oE '^rec=[0-9.]+')"; fi ;; esac ;;
  esac
  rec "ST4863 $X $v"; }

# ---- the put-back: wait for the lamp; the pet leg off; the put-back as a lamp unit (kputback); checks; then the
# guard removed as soon as the station, the files by hash and no kit drop-in hold (S4: the build, units and mute are
# reported apart and never keep it); $D removed only after disarm confirms no unit is left.
fin() { trap '' INT TERM HUP PIPE; exec >&3 2>&1; local why= note= r n
  echo "== back $(date +%T)"; [ -n "${RW:-}" ] && kill "$RW" 2>/dev/null; [ -n "${WD:-}" ] && kill "$WD" 2>/dev/null
  [ "$MODE" = run ] && [ -z "$SET" ] && { echo "nothing was written on the lamp (stopped before setup)"; return 0; }
  # R-P3: setup stopped before the guard's units were written: only $D exists (the backup reads, never writes, the
  # live files), so it is removed, once no guard unit is confirmed
  if [ -n "$SET" ] && [ -z "$GSET" ]; then waitreach 300 >/dev/null || { echo "PUTBACK NOT OK: the lamp is not reachable after a setup abort ($D may be left). LOOK NOW"; exit 4; }
    r=$(lamp unsetup "sudo -n sh -c 'if ls /etc/systemd/system/seventh-st4863-* >/dev/null 2>&1; then echo UNITS; else rm -rf $D && sync && echo REMOVED; fi'")
    if [ "$r" = REMOVED ]; then echo "PUTBACK OK: setup stopped before the guard's units: $D removed, nothing else was written"
      [ -n "$DRY" ] || { [ -n "${LOCKED:-}" ] && "$LOCK" lamp-52e6 release seventh "$SESSION" >/dev/null 2>&1; }; echo "PUT BACK $(date +%T). log: $LOG"; return 0; fi
    echo "setup abort: guard units found ($r): the full put-back follows"; fi
  waitreach $(( GUARD_MIN*60 + 420 )) >/dev/null || { echo "PUTBACK NOT OK: the lamp is not reachable; the guard stays armed and fires at boot + $GUARD_MIN min (and counts its reboots). LOOK NOW"; exit 4; }
  # `back` run twice: nothing of the kit left on the lamp is the done state, not an alarm
  n=$(lamp left "sudo -n sh -c 'echo \$(ls -d $D /etc/systemd/system/seventh-st4863-* 2>/dev/null | wc -l) \$(ls /etc/systemd/system/lamp-setup-recover.service.d /run/systemd/system/lamp-setup-recover.service.d 2>/dev/null | grep -c seventh)'")
  if [ "$n" = "0 0" ]; then echo "PUTBACK OK: nothing of the kit is on the lamp ($D, guard units, recover drop-ins: none): already put back"
    [ -n "$DRY" ] || { [ -n "${LOCKED:-}" ] && "$LOCK" lamp-52e6 release seventh "$SESSION" >/dev/null 2>&1; }; echo "PUT BACK $(date +%T). log: $LOG"; return 0; fi
  [ -n "$DRY" ] || $L "sudo -n cat $D/log.txt 2>/dev/null" > "$LOG.lamplog"
  # R-P2-2: the pet leg restarted (not stopped) so a leg is left if this put-back loses the LAN; the flock keeps it
  # and the put-back apart, a guard run in progress is never stopped, and disarm removes the leg at the end
  lamp pet-restart "sudo -n test -e /etc/systemd/system/$GP.timer && sudo -n systemctl restart $GP.timer; true" >/dev/null
  r=$(kputback back); echo "$r"; grep -q 'PUTBACK OK' <<< "$r" || why="$why station put-back;"
  grep -q 'build=MOVED' <<< "$r" && note="$note recover's program moved (lamp-update): logged;"
  # T-P3-1: lamp-voice is restarted (lib.sh's rs sets a restart-guard mute aside) only after a full run and only
  # when such a mute exists (a reboot -ff can leave one); phase 0 alone never touches lamp-voice
  if [ -z "$PHASE0_ONLY" ] && lamp mute-guard "sudo -n test -f /var/lib/lampOS/voice/mute/record.json.restart-guard && echo yes" | grep -q yes; then
    if [ -n "$DRY" ]; then echo "[lamp] $(rs lamp-voice)" >&2; else $L "$(rs lamp-voice)"; fi
  else echo "lamp-voice not restarted (${PHASE0_ONLY:+phase 0 only; }no restart-guard mute)"; fi
  [ -n "$DRY" ] || sleep 15
  F=$(lamp facts "sudo -n sh $D/st.sh facts"); echo "$F"
  [ "$(get seventh "$F")" = 0 ] || why="$why kit drop-ins left;"
  [ "$(get oldstage "$F")" = 0 ] || why="$why /opt/lampOS/releases/seventh-* left;"
  case $F in *wpa=enabled/active*hostapd=disabled*deny=0*type=managed*wants=1*) ;; *) why="$why not a clean station (or no wants link);" ;; esac
  grep '^units=' <<< "$F" | grep -qE 'inactive|failed|activating' && note="$note a lamp unit not active;"
  grep -q '"muted":false' <<< "$F" || note="$note voice/mute not false;"
  [ "$(get xsha "$F")" = "${XSHA:-$(get xsha "$F")}" ] || note="$note recover's program is not preflight's;"
  if [ -z "$why" ]; then r=$(lamp disarm "sudo -n sh $D/st.sh disarm && sudo -n rm -rf $D && sync && echo REMOVED $D"); echo "$r"
    grep -q "^REMOVED $D" <<< "$r" || why="$why guard not removed (disarm refused: a guard run still going, or not a station; $D kept);"; fi
  if [ -z "$why" ]; then echo "PUTBACK OK: files equal to the backup by hash, a clean station, no kit drop-in; guard removed, then $D"
    [ -z "$note" ] || echo "REPORTED APART (the guard is not kept for these):$note LOOK"
  else echo "PUTBACK NOT OK:$why the guard stays armed (pet leg restarted). LOOK NOW"
    [ -n "$DRY" ] || $L "sudo -n test -e /etc/systemd/system/$GP.timer && sudo -n systemctl restart $GP.timer" >/dev/null 2>&1; fi
  [ -n "$DRY" ] || { [ -n "${LOCKED:-}" ] && "$LOCK" lamp-52e6 release seventh "$SESSION" >/dev/null 2>&1; }
  echo "PUT BACK $(date +%T). log: $LOG"; [ -z "$why" ] || exit 4; }

if [ "$MODE" = back ]; then exec 3>&1; fin; exit 0; fi

if [ "$MODE" = go ]; then rc=0   # F-P3-3: rc set before anything, so QUIET always has it
  [ -n "$LIMIT" ] || abort "LIMIT unset: LIMIT=HHMM"
  until "$LOCK" lamp-52e6 status >/dev/null 2>&1; do [ "$(left)" -gt "$MINLEFT" ] || abort "still locked, too little time to LIMIT"; sleep 10; done
  k=0; while otherplaying; do [ $k -ge 3 ] && abort "another lamp still PLAYING after 3 min (#3336)"; sleep 60; k=$((k+1)); done
  # F2-P3: Ctrl-C/TERM before the run starts: the lock released, and QUIET posted once PLAYING was; replaced below.
  # Set just before the acquire, so a signal while it waits or as it returns is covered too (lamp-lock.sh releases
  # only a lock whose holder and session are ours: SESSION is this go's own, so a release not held is a no-op).
  PLAYED=; trap '"$LOCK" lamp-52e6 release seventh "$SESSION" >/dev/null 2>&1; [ -z "$PLAYED" ] || gh issue comment 3336 -R autonomous-ai/lampOS --body "QUIET lamp-52e6 $(date +%H:%M) (Seventh): #4863 kit stopped before its run started (INT/TERM): nothing written on the lamp. FREE." >/dev/null; exit 130' INT TERM
  "$LOCK" lamp-52e6 acquire seventh "$SESSION" "#4863 station-back kit" >/dev/null || abort "lamp lock"   # F-P3-3: PLAYING only once the lock is ours
  # the lock is a 600 s lease on the lamp (lamp-lock.sh), renewed every 60 s: it cannot be renewed while the lamp is off
  # its LAN, so a B/C window plus boot (typically ~13 min, worst ~27) can outlive it; the arms' and put-backs' own
  # flock on the lamp still keeps the kit's steps apart, and the PLAYING post on #3336 stands until QUIET
  if [ -n "$PHASE0_ONLY" ]; then PB="phase 0 only: 52e6 reboots once as a station (no AP step, it stays on its Wi-Fi), a guard in /etc is proved at boot + $GUARD_MIN min, then removed"
  else PB="52e6 drops off Wi-Fi for up to ~5 min at a time and reboots 3 times; it says its hotspot line; a guard in /etc brings it back by itself"; fi
  PLAYED=1   # before the post: a signal during it may follow a post that went out
  gh issue comment 3336 -R autonomous-ai/lampOS --body "PLAYING lamp-52e6 $(date +%H:%M)–${LIMIT:0:2}:${LIMIT:2:2} (Seventh for Sixth, #4863: recover's station-back fix, PR #4931, as installed in stable $EXPECT_SEQ; $PB)" >/dev/null
  # R-P1: a closed terminal must not end the run or its put-back: the run is nohup'd with its output only in $LOG
  # (shown here by tail), go ignores HUP, and Ctrl-C/TERM on go is passed to the run as TERM (it then puts back).
  trap '' HUP; export LOG
  LOCKED=1 SESSION=$SESSION LIMIT=$LIMIT nohup bash "$ME" ${GO_RUN:-run} </dev/null >/dev/null 2>&1 & RP=$!
  trap 'kill -TERM $RP 2>/dev/null' INT TERM
  touch "$LOG"; tail -n +1 -F "$LOG" 2>/dev/null & TP=$!
  # F-P3-3: the run waited for at least once (it may have ended already); a trapped signal re-waits
  while :; do wait $RP; rc=$?; kill -0 $RP 2>/dev/null || break; done; kill $TP 2>/dev/null
  # a lock taken by another session (our lease expired while 52e6 was off the network): the release refuses and
  # names the holder, and QUIET says so instead of FREE
  rl=$("$LOCK" lamp-52e6 release seventh "$SESSION" 2>&1); lh=$(awk '/^ *holder: /{h=$2} /^ *session: /{s=$2} END{if(h!="")print h" ("s")"}' <<< "$rl")
  gh issue comment 3336 -R autonomous-ai/lampOS --body "QUIET lamp-52e6 $(date +%H:%M) (Seventh): #4863 kit${PHASE0_ONLY:+ (phase 0 only)} done (exit $rc); $(tr '\n' ';' 2>/dev/null < "$LOG.verdicts"). $( [ -n "$lh" ] && echo "lock lost to $lh" || echo FREE)." >/dev/null
  exit $rc
fi
[ "$MODE" = run ] || [ -n "$DRY" ] || { echo "usage: LIMIT=HHMM $0 phase0 | LIMIT=HHMM $0 go | $0 back | $0 dry   (EXPECT_SEQ/EXPECT_COMMIT default to 9582 ab6ef495ce)"; exit 2; }

# ---- run (inside the lock) / dry
# runtraps: a run's output to $LOG (and the terminal), fin on EXIT; TERM, INT and HUP (a closed terminal: R-P1) in
# the main shell exit 143, so fin puts back. The tee and fin ignore HUP and PIPE, so a gone terminal cannot end them.
runtraps() { exec > >(trap '' INT TERM HUP PIPE; exec tee -a "$LOG") 2>&1 3>&1
  MAINPID=$$; trap fin EXIT
  trap '[ "$(exec sh -c "echo \$PPID")" = "$MAINPID" ] && { echo "TERM/INT/HUP $(date +%T): putting back" >&3; exit 143; }' TERM INT HUP
  # the loops drop the inherited traps, so fin's kill ends them (with the trap they ignored TERM and renewed for ever)
  ( trap - TERM INT HUP; while sleep 60; do renew; done ) & RW=$!
  ( trap - TERM INT HUP; while sleep 5; do [ "$(left)" -le 0 ] && { echo "HARD END $LIMIT: TERM (aborted)"; kill -TERM $$; exit; }; done ) & WD=$!; }
if [ "$MODE" = huptest ]; then LIMIT=$(date -v+1H +%H%M); runtraps; SET=1; GSET=1
  echo "huptest $(date +%T): a run in progress (pid $$), waiting for HUP"; sleep 30 & wait $!; exit 0
fi
# fakebin DIR: the lamp's tools as systemd-252-shaped fakes for the DRY's fake root (state in $FW; every call logged
# to $FW/calls). busctl answers "t <usec>", `systemctl show` a timespan, as 252 does. dhcpcd gives wlan0 a LAN
# address only when wpa_supplicant@wlan0 is active, wlan0 is up and managed, dhcpcd.conf has no denyinterfaces and
# the network is there ($FW/net_ok). iw's set type fails while $FW/iw_fail > 0 (Device or resource busy).
fakebin() { mkdir -p "$1"
  cat > "$1/systemctl" <<'F'
#!/bin/sh
echo "systemctl $*" >> "$FW/calls"; c=$1; shift
case $c in
--no-block|reboot) echo REBOOT >> "$FW/calls" ;;
is-enabled) cat "$FW/en.$1" 2>/dev/null || echo disabled ;;
is-active) for u; do if [ -s "$FW/seq.$u" ]; then head -1 "$FW/seq.$u"; sed -i '' 1d "$FW/seq.$u"; else cat "$FW/act.$u" 2>/dev/null || echo inactive; fi; done ;;
enable|disable|mask|unmask) now=; [ "$1" = --now ] && { now=1; shift; }
  for u; do e=$(cat "$FW/en.$u" 2>/dev/null || echo disabled)
    case $c in enable) [ "$e" = masked ] && exit 1; e=enabled; [ -z "$now" ] || echo active > "$FW/act.$u" ;;
      disable) [ "$e" = masked ] || e=disabled; [ -z "$now" ] || echo inactive > "$FW/act.$u" ;; mask) e=masked ;; unmask) [ "$e" = masked ] && e=disabled ;; esac
    echo "$e" > "$FW/en.$u"; done ;;
stop) for u; do echo inactive > "$FW/act.$u"; [ ! -x "$FW/hook.stop.$u" ] || "$FW/hook.stop.$u"; done ;;
start|restart) for u; do [ "$(cat "$FW/en.$u" 2>/dev/null)" = masked ] && exit 1; echo active > "$FW/act.$u"
    if [ "$u" = dhcpcd ] && [ "$(cat "$FW/act.wpa_supplicant@wlan0")" = active ] && [ "$(cat "$FW/type")" = managed ] && [ "$(cat "$FW/link")" = up ] && ! grep -q '^denyinterfaces wlan0' "$ROOT/etc/dhcpcd.conf" && [ -f "$FW/net_ok" ]; then echo 192.168.1.50/24 > "$FW/lan"; fi; done ;;
show) p=; u=; while [ $# -gt 0 ]; do case $1 in -p) p=$2; shift 2 ;; --value) shift ;; *) u=$1; shift ;; esac; done; cat "$FW/show.$u.$p" 2>/dev/null || cat "$FW/show.$p" 2>/dev/null ;;
esac; exit 0
F
  cat > "$1/ip" <<'F'
#!/bin/sh
echo "ip $*" >> "$FW/calls"
case "$*" in
"-4 -o addr show wlan0") l=$(cat "$FW/lan" 2>/dev/null); [ -z "$l" ] || echo "3: wlan0    inet $l brd 192.168.1.255 scope global wlan0"; [ ! -f "$FW/hot" ] || echo "3: wlan0    inet 192.168.100.1/24 scope global wlan0" ;;
"addr del 192.168.100.1/24 dev wlan0") rm -f "$FW/hot" ;;
"link set wlan0 up") echo up > "$FW/link" ;;
"link set wlan0 down") echo down > "$FW/link"; : > "$FW/lan" ;;
esac; exit 0
F
  cat > "$1/iw" <<'F'
#!/bin/sh
echo "iw $*" >> "$FW/calls"
case "$*" in
"dev wlan0 info") printf 'Interface wlan0\n\ttype %s\n' "$(cat "$FW/type")" ;;
"dev wlan0 set type managed") n=$(cat "$FW/iw_fail" 2>/dev/null || echo 0); if [ "$n" -gt 0 ]; then echo $((n-1)) > "$FW/iw_fail"; echo "command failed: Device or resource busy (-16)" >&2; exit 240; fi; echo managed > "$FW/type" ;;
"reg get") printf 'global\ncountry %s: DFS-FCC\n' "$(cat "$FW/reg")" ;;
"reg set "*) echo "$3" > "$FW/reg" ;;
esac; exit 0
F
  cat > "$1/wpa_cli" <<'F'
#!/bin/sh
echo "wpa_cli $*" >> "$FW/calls"
case "$*" in *status*) [ -s "$FW/lan" ] && echo wpa_state=COMPLETED || echo wpa_state=DISCONNECTED ;; *disable_network*) : > "$FW/lan" ;; *) [ "$(cat "$FW/act.wpa_supplicant@wlan0")" = active ] || exit 1 ;; esac
F
  cat > "$1/busctl" <<'F'
#!/bin/sh
echo "busctl $*" >> "$FW/calls"
case $1 in call) echo "o \"/org/freedesktop/systemd1/unit/$7\"" ;; get-property) u=${3##*/}; cat "$FW/bus.$u.$5" 2>/dev/null || echo "t 0" ;; esac
F
  cat > "$1/journalctl" <<'F'
#!/bin/sh
while [ $# -gt 0 ]; do [ "$1" = -u ] && { cat "$FW/journal.$2" 2>/dev/null; exit 0; }; shift; done
F
  printf '#!/bin/sh\n[ $# = 0 ] && cat "$FW/hostname" || echo "$1" > "$FW/hostname"\n' > "$1/hostname"
  printf '#!/bin/sh\nshift; exec "$@"\n' > "$1/timeout"; printf '#!/bin/sh\nexit 0\n' > "$1/sleep"
  printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' > "$1/sha256sum"
  # verify notes, as 252 prints for units it cannot fully check off the lamp: logged by selftest, not a gate
  printf '#!/bin/sh\necho "seventh-st4863-pet.timer: note: fake 252 verify line"\n' > "$1/systemd-analyze"
  # flock(1) on an inherited fd, as util-linux does it (flock(2) on the open file)
  cat > "$1/flock" <<'F'
#!/usr/bin/env python3
import fcntl, sys, time
a = sys.argv[1:]; w = None
if a[0] == '-w': w = float(a[1]); a = a[2:]
elif a[0] == '-n': w = 0.0; a = a[1:]
fd = int(a[0]); t = time.time()
while True:
    try: fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB); sys.exit(0)
    except BlockingIOError:
        if w is not None and time.time() - t >= w: sys.exit(1)
        time.sleep(0.1)
F
  chmod +x "$1"/*; }
if [ -n "$DRY" ]; then exec 3>&1
  echo "== DRY $(date +%T): no ssh; fakes are pass-shaped and systemd-252-shaped. Lamp side run on the Mac under ROOT= first (dash, fake tools):"
  T=$(mktemp -d); lampsh > "$T/st.sh"; mkdir -p "$T/r/etc/hostapd" "$T/r/etc/wpa_supplicant"
  FW=$T/fw; mkdir -p "$FW"; fakebin "$T/bin"; export FW
  RUN() { env PATH="$T/bin:$PATH" ROOT="$T/r" dash "$T/st.sh" "$@"; }
  # the fake world as a station (52e6's facts: nginx disabled/inactive, reg 00, dhcpcd on wlan0)
  station() { : > "$FW/calls"; rm -f "$FW"/seq.* "$FW/hot"; echo 0 > "$FW/iw_fail"; touch "$FW/net_ok"
    for u in wpa_supplicant@wlan0:enabled:active hostapd:disabled:inactive dnsmasq:disabled:inactive dhcpcd:enabled:active avahi-daemon:enabled:active nginx:disabled:inactive lamp-setup-recover:enabled:active $GU.service:static:inactive; do
      IFS=: read -r n e a <<< "$u"; echo "$e" > "$FW/en.$n"; echo "$a" > "$FW/act.$n"; done
    echo managed > "$FW/type"; echo up > "$FW/link"; echo 192.168.1.50/24 > "$FW/lan"; echo 00 > "$FW/reg"; echo lamp-52e6 > "$FW/hostname"; }
  # device-ap-mode on the fake world (scripts/imager/build-orangepi-v2.sh): wpa masked and its wants link gone,
  # denyinterfaces wlan0, hostapd/dnsmasq enabled and started, nginx restarted, ssid/hosts/hostname, reg US, the
  # AP address, the wlan0 lease gone
  apmode() { echo masked > "$FW/en.wpa_supplicant@wlan0"; echo inactive > "$FW/act.wpa_supplicant@wlan0"; rm -f "$T/r$WLK"
    { echo 'denyinterfaces wlan0'; cat "$T/r/etc/dhcpcd.conf"; } > "$T/x" && mv "$T/x" "$T/r/etc/dhcpcd.conf"
    mkdir -p "$T/r/etc/dnsmasq.d"; echo 'address=/#/192.168.100.1' >> "$T/r/etc/dnsmasq.d/99-lamp.conf"
    echo 'ssid=lamp-ab12' > "$T/r/etc/hostapd/hostapd.conf"; echo '127.0.1.1 lamp-ab12' > "$T/r/etc/hosts"; echo lamp-ab12 > "$T/r/etc/hostname"; echo lamp-ab12 > "$FW/hostname"
    for u in hostapd dnsmasq; do echo enabled > "$FW/en.$u"; echo active > "$FW/act.$u"; done; echo active > "$FW/act.nginx"
    echo US > "$FW/reg"; echo AP > "$FW/type"; : > "$FW/lan"; touch "$FW/hot"; }
  units() { guard_units | awk -v r="$T/r" '/^=== /{f=r $2; printf "" > f; next} {print >> f}'; }
  nunits() { ls "$T/r/etc/systemd/system"/seventh-st4863-* 2>/dev/null | wc -l | tr -d ' '; }
  printf 'hostname lamp\ninterface eth0\n' > "$T/r/etc/dhcpcd.conf"; echo 'ssid=lamp-52e6' > "$T/r/etc/hostapd/hostapd.conf"
  echo '127.0.1.1 lamp-52e6' > "$T/r/etc/hosts"; echo lamp-52e6 > "$T/r/etc/hostname"; printf 'network={\n\tssid="x"\n}\n' > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
  # recover's unit and installed program, as on the image (deploy/product/lamp-setup-recover.service's ExecStart):
  # the binary in its release folder named by its sha256, `current` a link to that folder
  printf 'installed-9582\nrecover: the radio was left a hotspot: back to the station\nrecover: the hotspot closed: back to the station\n' > "$T/ls"
  XR=$(shasum -a 256 < "$T/ls" | cut -c1-64); mkdir -p "$T/r/opt/lampOS/setup/$XR" "$T/r/etc/systemd/system"
  mv "$T/ls" "$T/r/opt/lampOS/setup/$XR/lamp-setup"; ln -s "$XR" "$T/r/opt/lampOS/setup/current"
  # a set-up lamp: the done marker, the installed build's state.json, voice unmuted
  mkdir -p "$T/r/var/lib/lampos-setup" "$T/r/var/lib/lampos-update" "$T/r/run/lampos-bus/voice/mute"; : > "$T/r/var/lib/lampos-setup/done"
  echo '{"installed":{"sequence":9582,"commit":"ab6ef495ce1234"}}' > "$T/r/var/lib/lampos-update/state.json"; echo '{"muted":false}' > "$T/r/run/lampos-bus/voice/mute/latest"
  printf '[Service]\nExecStart=/opt/lampOS/setup/current/lamp-setup recover --config /etc/lampOS/setup/serve.json --enable\n' > "$T/r/etc/systemd/system/lamp-setup-recover.service"
  mkdir -p "$T/r${WLK%/*}"; ln -s /lib/systemd/system/wpa_supplicant@.service "$T/r$WLK"   # the wants link, as on 52e6
  # the image's device-sta-mode, its steps in order under set -e (it stops at a failed `iw dev wlan0 set type managed`)
  mkdir -p "$T/r/usr/local/bin"; cat > "$T/r/usr/local/bin/device-sta-mode" <<'F'
#!/bin/bash
set -e
echo "Switching to STA mode..."
systemctl stop hostapd dnsmasq 2>/dev/null || true
systemctl disable hostapd dnsmasq 2>/dev/null || true
ip link set wlan0 down 2>/dev/null || true; sleep 1
iw dev wlan0 set type managed
ip link set wlan0 up; sleep 1
sed -i '' '/^denyinterfaces wlan0$/d' "$ROOT/etc/dhcpcd.conf"; sed -i '' '/^address=\/#\//d' "$ROOT/etc/dnsmasq.d/99-lamp.conf" 2>/dev/null || true
systemctl unmask wpa_supplicant@wlan0 2>/dev/null || true
systemctl enable wpa_supplicant@wlan0
systemctl restart wpa_supplicant@wlan0
systemctl enable dhcpcd
systemctl restart dhcpcd
echo "STA MODE ENABLED"
F
  chmod +x "$T/r/usr/local/bin/device-sta-mode"; station; units
  echo "  sh -n: $(sh -n "$T/st.sh" && echo ok), dash -n: $(dash -n "$T/st.sh" && echo ok)"
  RUN backup | sed 's/^/  /'
  echo "  backup/wants: $(cat "$T/r$D/backup/wants"); backup/reg: $(cat "$T/r$D/backup/reg"); backup/units nginx: $(grep '^nginx ' "$T/r$D/backup/units")"
  RUN selftest | sed 's/^/  /'
  { echo 'denyinterfaces wlan0'; cat "$T/r/etc/dhcpcd.conf"; } > "$T/x" && mv "$T/x" "$T/r/etc/dhcpcd.conf"; mkdir -p "$T/r/etc/dnsmasq.d"; echo 'address=/#/192.168.100.1' > "$T/r/etc/dnsmasq.d/99-lamp.conf"
  rm -f "$T/r$WLK"   # AP mode's removal of the wants link
  RUN check | grep DIFF | sed 's/^/  live AP edit: /'
  RUN restore | grep -- '->' | sed 's/^/  /'; RUN check >/dev/null && echo "  restore by hash on the fake root: all files equal to the backup (99-lamp.conf removed: it was absent), wants link -> $(readlink "$T/r$WLK")" || { echo "  RESTORE CHECK FAILED"; WA=$((WA+1)); }
  ln -sf /dev/null "$T/r$WLK"; RUN check | grep DIFF | sed 's/^/  wrong target: /'
  RUN restore >/dev/null; RUN check >/dev/null && echo "  wrong target restored: wants link -> $(readlink "$T/r$WLK")" || { echo "  WANTS RESTORE FAILED"; WA=$((WA+1)); }
  RUN backup | sed 's/^/  second backup: /'
  echo "  backup/recover.exec: $(cut -c1-60 < "$T/r$D/backup/recover.exec")... (path, sha256, release folder: equal)"
  # B's switch drop-in as the arm leaves it for the reboot, plus an older run's staging left behind; the guard's
  # xcheck removes all and refuses that pass; its retry is OK; a third is a no-op
  RRD=/run/systemd/system/lamp-setup-recover.service.d; SRE=$T/r/etc/systemd/system/lamp-setup-recover.service.d; mkdir -p "$SRE" "$T/r/opt/lampOS/releases/seventh-st4863"
  printf '[Service]\nEnvironment=LAMP_WIFI_RECOVERY=on\n' > "$SRE/zz-seventh-st4863-on.conf"
  printf '[Service]\nExecStart=\nExecStart=/opt/lampOS/releases/seventh-st4863/lamp-setup recover\n' > "$SRE/zz-seventh-st4863-bin.conf"; echo old > "$T/r/opt/lampOS/releases/seventh-st4863/lamp-setup"
  echo "  as B leaves it + an older run's leftovers: $(cd "$T/r" && ls -d etc/systemd/system/lamp-setup-recover.service.d/* opt/lampOS/releases/seventh-* | tr '\n' ' ')"
  if RUN xcheck | sed 's/^/  guard xcheck: /'; [ "${PIPESTATUS[0]}" != 0 ]; then echo "  leftovers: refused and removed, as they must be"; else echo "  LEFTOVER CHECK FAILED"; WA=$((WA+1)); fi
  RUN xcheck | sed 's/^/  guard xcheck (retry): /'; [ "${PIPESTATUS[0]}" = 0 ] || { echo "  XCHECK RETRY FAILED"; WA=$((WA+1)); }
  echo "  after: $(cd "$T/r" && ls -d etc/systemd/system/lamp-setup-recover.service.d/* opt/lampOS/releases/seventh-* 2>/dev/null | wc -l | tr -d ' ') kit files left"
  RUN xcheck | sed 's/^/  guard xcheck again: /'
  pb() { RUN putback "$@" > "$T/pb.out" 2>&1; echo $? > "$T/pb.rc"; }
  # kd: the kit's /run switch drop-in, as arm A or C leaves it (the guard then does not stand back: F-P2)
  kd() { mkdir -p "$T/r$RRD"; printf '[Service]\nEnvironment=LAMP_WIFI_RECOVERY=on\n' > "$T/r$RRD/zz-seventh-st4863-on.conf"; }
  pbl() { grep -E "$1" "$T/r$D/log.txt" | tail -${2:-1} | sed -E 's/^[0-9:]+ b=[^ ]+ up=[^ ]* //' | cut -c1-170 | sed 's/^/    /'; }
  echo "== DRY cases on the fake root (the review's findings):"
  # B1: device-ap-mode's state, device-sta-mode fails at `iw set type managed` (set -e): the bring-up still runs
  station; apmode; echo 1 > "$FW/iw_fail"; pb force guard
  echo "  B1 bring-up after a failed device-sta-mode (AP state, iw busy once): exit $(cat "$T/pb.rc")"; pbl 'device-sta-mode exit|bring-up|unit wpa|PUTBACK (OK|FAILED) \(guard\)' 6
  if grep -q 'PUTBACK OK (guard)' "$T/pb.out" && grep -q 'device-sta-mode exit 240' "$T/pb.out" && grep -q 'systemctl restart wpa_supplicant@wlan0' "$FW/calls"; then echo "  B1: wpa_supplicant restarted by the bring-up after device-sta-mode stopped: station back, PASS"; else echo "  B1 BRING-UP CASE FAILED"; WA=$((WA+1)); fi
  echo "  reg $(cat "$FW/reg"), nginx $(cat "$FW/act.nginx"), hostapd $(cat "$FW/en.hostapd")/$(cat "$FW/act.hostapd"), wpa $(cat "$FW/en.wpa_supplicant@wlan0")/$(cat "$FW/act.wpa_supplicant@wlan0"), type $(cat "$FW/type"), lan $(cat "$FW/lan"), units $(nunits) (not expired: kept)"
  # lead's condition (1): on a lamp that is not a clean station, device-sta-mode runs FIRST, then the bring-up
  ia=$(grep -n 'iw dev wlan0 set type managed' "$FW/calls" | head -1 | cut -d: -f1); wr=$(grep -n 'systemctl restart wpa_supplicant@wlan0' "$FW/calls" | head -1 | cut -d: -f1)
  echo "  B1 order: device-sta-mode's iw (call $ia) before the bring-up's wpa restart (call $wr)"; [ -n "$ia" ] && [ -n "$wr" ] && [ "$ia" -lt "$wr" ] || { echo "  B1 ORDER CASE FAILED"; WA=$((WA+1)); }
  # R-P2-1 / lead's condition (2): a healthy lamp, for the kit's tidy and for phase 0's guard run: no device-sta-mode
  # and no wpa/dhcpcd restart at all (phase 0 never blinks the Wi-Fi)
  for w in "ifneeded kit-healthy" "force guard"; do station; : > "$T/r$D/log.txt"; pb $w
    nr=$(grep -cE 'systemctl (re)?start (wpa_supplicant@wlan0|dhcpcd)|wpa_cli -i wlan0 enable_network|ip link set wlan0 down' "$FW/calls")
    echo "  healthy lamp, putback $w: exit $(cat "$T/pb.rc"), $(grep -oE 'device-sta-mode=[a-z-]*|stood back' "$T/pb.out" | head -1), device-sta-mode runs: $(grep -c 'Switching to STA' "$T/r$D/log.txt"), wpa/dhcpcd restarts and link downs: $nr, LAN kept: $(cat "$FW/lan")"
    [ "$(cat "$T/pb.rc")" = 0 ] && grep -qE 'device-sta-mode=skipped-clean|PUTBACK OK \(guard\): stood back' "$T/pb.out" && ! grep -q 'Switching to STA' "$T/r$D/log.txt" && [ "$nr" = 0 ] || { echo "  HEALTHY CASE RESTARTED THE WI-FI"; WA=$((WA+1)); }; done
  # F-P3-4: a clean station with nginx active (inactive at the backup) and reg US: both back, no Wi-Fi restart
  station; echo active > "$FW/act.nginx"; echo US > "$FW/reg"; : > "$T/r$D/log.txt"; pb ifneeded kit-ngx
  nr=$(grep -cE 'systemctl (re)?start (wpa_supplicant@wlan0|dhcpcd)|wpa_cli -i wlan0 enable_network|ip link set wlan0 down' "$FW/calls")
  echo "  F-P3-4 clean station, nginx active, reg US: exit $(cat "$T/pb.rc"), nginx $(cat "$FW/act.nginx"), reg $(cat "$FW/reg"), wpa/dhcpcd restarts and link downs: $nr"; pbl 'bring-up' 3
  [ "$(cat "$T/pb.rc")" = 0 ] && [ "$(cat "$FW/act.nginx")" = inactive ] && [ "$(cat "$FW/reg")" = 00 ] && [ "$nr" = 0 ] || { echo "  F-P3-4 CASE FAILED"; WA=$((WA+1)); }
  # B1: no LAN after the bring-up (the network gone): counted reboots 1 and 2, then none (the unit retries, no reboot)
  station; apmode; rm -f "$FW/net_ok"; rm -f "$T/r$D/reboots"; : > "$T/r$D/log.txt"
  for i in 1 2 3; do : > "$FW/calls"; apmode; pb force guard; echo "  no LAN, guard run $i: exit $(cat "$T/pb.rc"), reboot issued: $(grep -c REBOOT "$FW/calls"), counter $(cat "$T/r$D/reboots" 2>/dev/null || echo 0)"; done
  pbl 'counted reboot|NO REBOOT' 3
  if [ "$(cat "$T/r$D/reboots")" = 2 ] && [ "$(grep -c REBOOT "$FW/calls")" = 0 ] && grep -q 'NO REBOOT' "$T/r$D/log.txt"; then echo "  counted reboot: 2 issued, then none (count exhausted; the unit retries every 5 min without rebooting): PASS"; else echo "  COUNTED REBOOT CASE FAILED"; WA=$((WA+1)); fi
  echo "  loans.txt (guard lines): $(wc -l < "$T/r/loans.txt" | tr -d ' '); last: $(tail -1 "$T/r/loans.txt" | cut -c1-110)"
  # R-P2-3: the unit has no start limit and no reboot action; a broken or missing st.sh fails it, no reboot
  gu=$(guard_units | awk '/^=== .*guard.service/{f=1;next} /^=== /{f=0} f')
  { head -c 4000 "$T/st.sh"; printf '\n fi ) # cut mid-write\n'; } > "$T/broken.sh"; : > "$FW/calls"
  env PATH="$T/bin:$PATH" ROOT="$T/r" dash "$T/broken.sh" putback force guard >/dev/null 2>&1; b1=$?; env PATH="$T/bin:$PATH" dash "$T/missing.sh" putback force guard >/dev/null 2>&1; b2=$?
  echo "  broken st.sh: exit $b1, missing st.sh: exit $b2, reboots issued: $(grep -c REBOOT "$FW/calls"); unit: $(grep -E '^(StartLimit|FailureAction|RestartSec|TimeoutStartSec)' <<< "$gu" | tr '\n' ' ')"
  [ "$b1" != 0 ] && [ "$b2" != 0 ] && [ "$(grep -c REBOOT "$FW/calls")" = 0 ] && ! grep -qE '^(StartLimitAction|FailureAction|SuccessAction)' <<< "$gu" && grep -q '^StartLimitIntervalSec=0' <<< "$gu" || { echo "  BROKEN ST.SH CASE FAILED"; WA=$((WA+1)); }
  rm -f "$T/r$D/reboots"; station
  # S3: one lock. A holder (an arm) keeps it 4 s: the kit's put-back (LOCKW=1) refuses, the guard's goes on, a
  # put-back with LOCKW=10 waits for it
  python3 -c 'import fcntl,sys,time; f=open(sys.argv[1],"a"); fcntl.flock(f,fcntl.LOCK_EX); time.sleep(4)' "$T/r$D/lock" & HP=$!; sleep 0.5
  LOCKW=1 RUN putback ifneeded kit-lock > "$T/pb.out" 2>&1; r1=$?
  kd; t1=$(date +%s); LOCKW=1 RUN putback force guard > "$T/pb2.out" 2>&1; r2=$?
  LOCKW=10 RUN putback ifneeded kit-wait > "$T/pb3.out" 2>&1; r3=$?; t3=$(date +%s); wait $HP 2>/dev/null
  echo "  flock: kit put-back while held: exit $r1 ($(grep -o 'PUTBACK FAILED (kit-lock): the lock is held' "$T/pb.out")); guard while held: exit $r2 ($(grep -o 'the guard goes on without the lock' "$T/pb2.out")); kit put-back LOCKW=10: exit $r3 after $((t3-t1)) s ($(grep -o 'PUTBACK OK (kit-wait)' "$T/pb3.out"))"
  [ "$r1" = 1 ] && [ "$r2" = 0 ] && [ "$r3" = 0 ] && grep -q 'the guard goes on without the lock' "$T/pb2.out" && grep -q 'PUTBACK OK (kit-wait)' "$T/pb3.out" || { echo "  FLOCK CASE FAILED"; WA=$((WA+1)); }
  # S1: phase 0's check on the fake root's own report: busctl raw usec, `systemctl show` a 252 timespan
  station; GM=$((GUARD_MIN*60)); echo "t ${GM}204511" > "$FW/bus.$GU.timer.LastTriggerUSecMonotonic"; echo "5min 204.511ms" > "$FW/show.LastTriggerUSecMonotonic"; echo success > "$FW/show.Result"
  printf '[  %s.312000] lamp-52e6 sh[812]: PUTBACK force (guard): found %s\n[  %s.401000] lamp-52e6 sh[812]: PUTBACK OK (guard): device-sta-mode=skipped-clean files-differ=0\n' "$GM" "x" "$((GM+19))" > "$FW/journal.$GU.service"
  r=$(RUN report); echo "  report on the fake root: $(grep -E '^gtimer=' <<< "$r")"
  if v=$(p0check "$r"); then echo "  S1 phase 0 check (busctl): PASS: $v"; else echo "  S1 PHASE 0 CHECK FAILED: $v"; WA=$((WA+1)); fi
  o="gtimer=$(cat "$FW/show.LastTriggerUSecMonotonic")"; ot=$(get gtimer "$o")
  awk -v t="${ot:-0}" -v m=$GM 'BEGIN{exit !(t/1e6 >= m && t/1e6 <= m+30)}' && { echo "  S1: the old read passed a timespan: WRONG"; WA=$((WA+1)); } || echo "  S1: the old read of systemd 252's \`systemctl show\` ('$o' -> '$ot') would have aborted phase 0 on a good lamp: caught"
  echo "t 0" > "$FW/bus.$GU.timer.LastTriggerUSecMonotonic"; v=$(p0check "$(RUN report)") && { echo "  S1 NEGATIVE PASSED"; WA=$((WA+1)); } || echo "  S1 negative (the boot leg did not fire): refused: $v"
  # F-P2: the guard on a clean station, no arm unit, no kit drop-in: stands back, 0 writes. Its own line, as the
  # journal keeps it, is what waitguard and p0check accept: phase 0 still proves the boot leg fired and the guard
  # ran and logged OK.
  station; : > "$FW/calls"; h0=$(cat "$T/r"/etc/dhcpcd.conf "$T/r"/etc/hosts "$T/r"/etc/hostname | shasum | cut -c1-12); pb force guard
  h1=$(cat "$T/r"/etc/dhcpcd.conf "$T/r"/etc/hosts "$T/r"/etc/hostname | shasum | cut -c1-12)
  nw=$(grep -cE 'systemctl (stop|start|restart|enable|disable|mask|unmask|daemon-reload|reset-failed|--no-block)|^ip (addr del|link set)|^iw (dev wlan0 set|reg set)|^wpa_cli .*(enable|disable)_network' "$FW/calls")
  echo "  F-P2 stand-back (clean, no arm unit, no kit drop-in): exit $(cat "$T/pb.rc"), writes: $nw, files $( [ "$h0" = "$h1" ] && echo untouched || echo CHANGED): $(grep -o 'PUTBACK OK (guard): stood back[^:]*' "$T/pb.out")"
  [ "$(cat "$T/pb.rc")" = 0 ] && [ "$nw" = 0 ] && [ "$h0" = "$h1" ] && grep -q 'PUTBACK OK (guard): stood back' "$T/pb.out" || { echo "  F-P2 STAND-BACK CASE FAILED"; WA=$((WA+1)); }
  echo "t ${GM}204511" > "$FW/bus.$GU.timer.LastTriggerUSecMonotonic"
  printf '[  %s.402000] lamp-52e6 sh[812]: %s\n' "$GM" "$(grep -m1 'PUTBACK OK (guard)' "$T/pb.out")" > "$FW/journal.$GU.service"
  r=$(RUN report); echo "  F-P2 phase 0 on the stand-back: $(grep -E '^gj=' <<< "$r" | cut -c1-80) | $(grep -E '^gtimer=' <<< "$r")"
  # waitguard's own test (st4863.sh waitguard), then p0check
  grep -qE '^gj=[0-9.]+ PUTBACK (OK|FAILED) \(guard\)' <<< "$r" && grep -q '^gj=.* stood back' <<< "$r" && echo "  F-P2 waitguard: the boot leg's run seen" || { echo "  F-P2 WAITGUARD MISSES THE STAND-BACK"; WA=$((WA+1)); }
  if v=$(p0check "$r"); then echo "  F-P2 phase 0 check on the stand-back: PASS: $v"; else echo "  F-P2 PHASE 0 CHECK FAILED: $v"; WA=$((WA+1)); fi
  station; echo active > "$FW/act.seventh-st4863-arm-A"; pb force guard; rm -f "$FW"/act.seventh-st4863-arm-*   # an arm unit active: no stand-back
  echo "  F-P2 with arm A's unit active: exit $(cat "$T/pb.rc"), $(grep -oE 'stood back|PUTBACK OK \(guard\): device-sta-mode=[a-z-]*' "$T/pb.out")"; grep -q 'stood back' "$T/pb.out" && { echo "  F-P2 STOOD BACK DURING AN ARM"; WA=$((WA+1)); }
  # F2-P2: a clean station whose files differ from the backup (AP mode's hosts edit left when the kit died after C):
  # the stand-back is refused (check) and the guard goes on to restore by hash
  station; h0=$(shasum < "$T/r/etc/hosts" | cut -c1-12); echo '127.0.1.1 lamp-ap-left' >> "$T/r/etc/hosts"; : > "$FW/calls"; pb force guard; h1=$(shasum < "$T/r/etc/hosts" | cut -c1-12)
  echo "  F2-P2 clean station, hosts edited: exit $(cat "$T/pb.rc"), $(grep -oE 'stood back|restored /etc/hosts|PUTBACK (OK|FAILED) \(guard\): device-sta-mode=[a-z-]*' "$T/pb.out" | tr '\n' ' ')hosts $h0 -> $h1"
  [ "$(cat "$T/pb.rc")" = 0 ] && ! grep -q 'stood back' "$T/pb.out" && grep -q 'restored /etc/hosts' "$T/pb.out" && grep -q 'PUTBACK OK (guard): device-sta-mode=skipped-clean' "$T/pb.out" && [ "$h0" = "$h1" ] || { echo "  F2-P2 FILE-DIFFERS CASE FAILED"; WA=$((WA+1)); }
  # F2-P2: tidyap on the stand-back: a clean station, files equal, nginx active (inactive at the backup), reg US:
  # stood back, both tidied, no wpa/dhcpcd restart
  station; echo active > "$FW/act.nginx"; echo US > "$FW/reg"; : > "$FW/calls"; : > "$T/r$D/log.txt"; pb force guard
  nr=$(grep -cE 'systemctl (re)?start (wpa_supplicant@wlan0|dhcpcd)|wpa_cli -i wlan0 enable_network|ip link set wlan0 down' "$FW/calls")
  echo "  F2-P2 stand-back, nginx active, reg US: exit $(cat "$T/pb.rc"), $(grep -o 'PUTBACK OK (guard): stood back' "$T/pb.out"), nginx $(cat "$FW/act.nginx"), reg $(cat "$FW/reg"), wpa/dhcpcd restarts and link downs: $nr"; pbl 'bring-up|stood back' 3
  [ "$(cat "$T/pb.rc")" = 0 ] && grep -q 'PUTBACK OK (guard): stood back' "$T/pb.out" && [ "$(cat "$FW/act.nginx")" = inactive ] && [ "$(cat "$FW/reg")" = 00 ] && [ "$nr" = 0 ] || { echo "  F2-P2 TIDYAP CASE FAILED"; WA=$((WA+1)); }
  # its line still the guard's OK line for waitguard's regex and p0check
  printf '[  %s.402000] lamp-52e6 sh[812]: %s\n' "$GM" "$(grep -m1 'PUTBACK OK (guard)' "$T/pb.out")" > "$FW/journal.$GU.service"; echo "t ${GM}204511" > "$FW/bus.$GU.timer.LastTriggerUSecMonotonic"
  r=$(RUN report); grep -qE '^gj=[0-9.]+ PUTBACK (OK|FAILED) \(guard\)' <<< "$r" && v=$(p0check "$r") && echo "  F2-P2 waitguard/p0check on the tidied stand-back: PASS: $v" || { echo "  F2-P2 TIDIED STAND-BACK NOT SEEN: $v"; WA=$((WA+1)); }
  rm -f "$FW"/bus.* "$FW"/journal.* "$FW"/show.*
  # B2: disarm while the guard runs: waited for, never stopped; then units gone and the station re-checked
  station; printf 'activating\nactivating\nactivating\n' > "$FW/seq.$GU.service"
  out=$(RUN disarm | tr '\n' ' '); rc=$?; echo "  disarm with a guard run in progress: exit $rc: $out"
  [ $rc = 0 ] && [ "$(nunits)" = 0 ] && ! grep -q "stop $GU.service" "$FW/calls" && grep -q 'waited 15 s' "$T/r$D/log.txt" || { echo "  DISARM WAIT CASE FAILED"; WA=$((WA+1)); }
  units; station; for i in {1..80}; do echo activating; done > "$FW/seq.$GU.service"
  out=$(RUN disarm | tr '\n' ' '); rc=$?; echo "  disarm with a guard run that does not end in 6 min: exit $rc: $out; units kept: $(nunits)"
  [ $rc = 1 ] && [ "$(nunits)" = 4 ] || { echo "  DISARM REFUSE CASE FAILED"; WA=$((WA+1)); }
  station; apmode; out=$(RUN disarm | tr '\n' ' '); rc=$?; echo "  disarm on a lamp not a clean station: exit $rc, units kept: $(nunits)"; [ $rc = 1 ] && [ "$(nunits)" = 4 ] || { echo "  DISARM STATION CASE FAILED"; WA=$((WA+1)); }
  station; RUN restore >/dev/null   # the world back to the station
  # (run here, before any fin of this DRY: fin ignores HUP/TERM and a child would inherit that)
  # R-P1: HUP (a closed terminal) during a run: the run's own traps (runtraps), fin still puts back, to $LOG
  echo "  HUP during a run: kit/st4863.sh huptest (a run's own traps, fakes), then kill -HUP"; HL=$(mktemp -t st4863hup)
  LOG=$HL bash "$ME" huptest >/dev/null 2>&1 & HP=$!; sleep 2; kill -HUP $HP; wait $HP; hr=$?
  for i in 1 2 3 4 5 6 7 8 9 10; do grep -q '^PUT BACK' "$HL" && break; sleep 1; done   # the tee writes last
  grep -E 'huptest|putting back|PUTBACK (OK|NOT OK)|PUT BACK' "$HL" | sed -E 's/^/  /; s/ log: .*//'; echo "  run exit $hr; its loops left: $(pgrep -f "$MN.sh huptest" | wc -l | tr -d ' ')"
  grep -q 'TERM/INT/HUP .*putting back' "$HL" && grep -q '^PUTBACK OK: files equal' "$HL" || { echo "  HUP CASE FAILED"; WA=$((WA+1)); }; rm -f "$HL"
  # R-P1: past the expiry the guard unarms FIRST: the Wi-Fi changed since setup (a new network) is not restored, no
  # lock is taken, no reboot, exit 0
  station; echo 1000 > "$T/r$D/backup/expires"; printf 'network={\n\tssid="new-home"\n}\n' > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
  w0=$(shasum -a 256 < "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf" | cut -c1-12); : > "$FW/calls"; : > "$T/r$D/log.txt"; pb force guard
  w1=$(shasum -a 256 < "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf" | cut -c1-12)
  echo "  expiry passed, Wi-Fi file changed: exit $(cat "$T/pb.rc"), $(grep 'GUARD EXPIRED' "$T/pb.out"), Wi-Fi file $w0 -> $w1, restores: $(grep -c 'restored /' "$T/pb.out"), reboots: $(grep -c REBOOT "$FW/calls"), units left: $(nunits), $D $( [ -d "$T/r$D" ] && echo LEFT || echo removed) (T-P3-2); loans: $(tail -1 "$T/r/loans.txt" | cut -c1-80)"
  [ "$(cat "$T/pb.rc")" = 0 ] && [ "$w0" = "$w1" ] && ! grep -qE 'restored /|PUTBACK' "$T/pb.out" && [ "$(nunits)" = 0 ] && [ ! -d "$T/r$D" ] && [ "$(grep -c REBOOT "$FW/calls")" = 0 ] || { echo "  EXPIRY CASE FAILED"; WA=$((WA+1)); }
  printf 'network={\n\tssid="x"\n}\n' > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"; units; station; RUN backup >/dev/null   # a fresh kit run's setup
  # F-P3-1: an empty or non-numeric expiry counts as expired (units removed, nothing restored)
  for x in '' 'garbage'; do station; printf '%s' "$x" > "$T/r$D/backup/expires"; : > "$FW/calls"; pb force guard
    echo "  F-P3-1 expiry '$x': exit $(cat "$T/pb.rc"), $(grep -c 'GUARD EXPIRED' "$T/pb.out") expired line, restores: $(grep -c 'restored /' "$T/pb.out"), units left $(nunits), $D $( [ -d "$T/r$D" ] && echo LEFT || echo removed)"
    [ "$(cat "$T/pb.rc")" = 0 ] && grep -q 'GUARD EXPIRED' "$T/pb.out" && ! grep -q 'restored /' "$T/pb.out" && [ "$(nunits)" = 0 ] || { echo "  F-P3-1 CASE FAILED"; WA=$((WA+1)); }
    units; station; RUN backup >/dev/null; done
  # F-P3-2: a partial backup (each of manifest, recover.exec, wants, units missing) is refused before anything is
  # touched: AP state left as found, recover not stopped, no reboot
  for f in manifest recover.exec wants units; do station; apmode; mv "$T/r$D/backup/$f" "$T/pbk"; : > "$FW/calls"
    h0=$(cat "$T/r"/etc/dhcpcd.conf "$T/r"/etc/hosts | shasum | cut -c1-12); pb force guard; h1=$(cat "$T/r"/etc/dhcpcd.conf "$T/r"/etc/hosts | shasum | cut -c1-12)
    nw=$(grep -cE 'systemctl (stop|start|restart|enable|disable|mask|unmask|daemon-reload|--no-block)|^ip (addr del|link set)|^iw (dev wlan0 set|reg set)|REBOOT' "$FW/calls")
    echo "  F-P3-2 backup without $f: exit $(cat "$T/pb.rc"), writes: $nw (recover stop: $(grep -c 'stop lamp-setup-recover' "$FW/calls"), reboot: $(grep -c REBOOT "$FW/calls")), files $( [ "$h0" = "$h1" ] && echo untouched || echo CHANGED): $(grep -o 'backup incomplete[^:]*' "$T/pb.out")"
    [ "$(cat "$T/pb.rc")" = 1 ] && [ "$nw" = 0 ] && [ "$h0" = "$h1" ] && grep -q "backup incomplete ($f missing)" "$T/pb.out" || { echo "  F-P3-2 CASE ($f) FAILED"; WA=$((WA+1)); }
    mv "$T/pbk" "$T/r$D/backup/$f"; done; station; RUN restore >/dev/null

  # T-P2: within the expiry, a person re-provisioned the lamp (a new Wi-Fi file, on its LAN): the guard leaves the
  # Wi-Fi file alone, no restart, PUTBACK OK
  station; kd; printf 'network={\n\tssid="new-home"\n}\n' > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"; w0=$(shasum -a 256 < "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf" | cut -c1-12); pb force guard
  w1=$(shasum -a 256 < "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf" | cut -c1-12); nr=$(grep -cE 'systemctl (re)?start (wpa_supplicant@wlan0|dhcpcd)' "$FW/calls")
  echo "  T-P2 re-provisioned lamp, guard run: exit $(cat "$T/pb.rc"), Wi-Fi file $w0 -> $w1, wpa/dhcpcd restarts $nr, $(grep -oE 'PUTBACK (OK|FAILED) \(guard\)' "$T/pb.out"), check: $(grep "^info" "$T/r$D/check.last" | cut -c1-80)"
  [ "$(cat "$T/pb.rc")" = 0 ] && [ "$w0" = "$w1" ] && [ "$nr" = 0 ] || { echo "  T-P2 RE-PROVISION CASE FAILED"; WA=$((WA+1)); }
  printf 'network={\n\tssid="x"\n}\n' > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf"
  # T-P2: the guard stands back (setup running, a Wi-Fi change pending, no done marker): nothing touched, exit 0
  for c in setup previous nomarker; do station; apmode; case $c in setup) echo active > "$FW/act.lamp-setup" ;; previous) : > "$T/r/etc/wpa_supplicant/wpa_supplicant-wlan0.conf.lamp-previous" ;; nomarker) rm -f "$T/r/var/lib/lampos-setup/done" ;; esac
    h0=$(cat "$T/r/etc/dhcpcd.conf" | shasum | cut -c1-12); pb force guard; h1=$(cat "$T/r/etc/dhcpcd.conf" | shasum | cut -c1-12)
    echo "  T-P2 guard with $c: exit $(cat "$T/pb.rc"), $(grep -o 'GUARD STANDS BACK[^:]*' "$T/pb.out"), systemctl writes: $(grep -cE 'systemctl (stop|start|restart|enable|disable|unmask)' "$FW/calls"), dhcpcd.conf $( [ "$h0" = "$h1" ] && echo untouched || echo CHANGED)"
    [ "$(cat "$T/pb.rc")" = 0 ] && grep -q 'GUARD STANDS BACK' "$T/pb.out" && [ "$h0" = "$h1" ] && ! grep -qE 'systemctl (stop|start|restart|enable|disable|unmask)' "$FW/calls" || { echo "  T-P2 STAND-BACK CASE ($c) FAILED"; WA=$((WA+1)); }
    echo inactive > "$FW/act.lamp-setup"; rm -f "$T/r/etc/wpa_supplicant/"*.lamp-previous; : > "$T/r/var/lib/lampos-setup/done"; RUN restore >/dev/null; done
  # T-P1: the lamp's own (ROOT-empty) halves, run for real under dash: SIM=1 takes every `real` branch (systemctl's
  # views, the selftest's unit half) on the fake root, with the 252-shaped fakes. Traced (dash -x) for coverage.
  SIMRUN() { env PATH="$T/bin:$PATH" ROOT="$T/r" SIM=1 dash -x "$T/st.sh" "$@" 2>>"$T/trace"; }
  : > "$T/trace"; units; station; RUN restore >/dev/null
  printf '{ path=/opt/lampOS/setup/current/lamp-setup ; argv[]=/opt/lampOS/setup/current/lamp-setup recover --config /etc/lampOS/setup/serve.json --enable ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n' > "$FW/show.ExecStart"
  echo loaded > "$FW/show.LoadState"; echo 4242 > "$FW/show.MainPID"; echo success > "$FW/show.Result"; : > "$FW/show.Environment"; : > "$FW/show.DropInPaths"
  for u in lamp-voice lamp-eyes lamp-controls lamp-body; do echo active > "$FW/act.$u"; done; echo inactive > "$FW/act.lamp-setup"
  for u in $GU.timer $GW.service; do echo enabled > "$FW/en.$u"; done; echo static > "$FW/en.$GP.timer"
  echo "t $(( (GUARD_MIN*60+40) * 1000000 ))" > "$FW/bus.$GP.timer.NextElapseUSecMonotonic"
  mkdir -p "$T/r/proc/4242"; cp "$T/r/opt/lampOS/setup/current/lamp-setup" "$T/r/proc/4242/exe"
  # facts as preflight reads them: before setup, so no $D and no guard unit yet
  mv "$T/r$D" "$T/Dsave"; rm -f "$T/r/etc/systemd/system"/seventh-st4863-*; sf=$(SIMRUN facts); mv "$T/Dsave" "$T/r$D"; units; echo active > "$FW/act.$GP.timer"
  echo "  SIM facts:"; sed 's/^/    /' <<< "$sf" | cut -c1-150
  out=$(LIMIT=$(dl) FAKE_FACTS="$sf" preflight 2>&1); echo "$out" | grep -E 'WOULD ABORT|build ok' | sed 's/^/  SIM preflight on the real facts: /'
  grep -q 'WOULD ABORT' <<< "$out" && { echo "  T-P1 PREFLIGHT ON REAL FACTS FAILED"; WA=$((WA+1)); }
  r=$(SIMRUN selftest); echo "$r" | sed 's/^/  SIM selftest: /'; grep -q '^SELFTEST ARMED' <<< "$r" || { echo "  T-P1 SELFTEST (REAL HALF) FAILED"; WA=$((WA+1)); }
  grep -q 'verify says' "$T/r$D/log.txt" && echo "  SIM selftest: the verify note was logged, not a gate" || { echo "  T-P1 VERIFY NOTE NOT LOGGED"; WA=$((WA+1)); }
  echo $(( $(date +%s) + 60 )) > "$T/r$D/backup/expires"   # F-P2: an expiry a minute away, then the pet (the kit's pet line)
  r=$(env PATH="$T/bin:$PATH" ROOT="$T/r" SIM=1 sh -c "systemctl restart $GP.timer && dash -x $T/st.sh push $TTL && dash -x $T/st.sh selftest" 2>>"$T/trace"); echo "  SIM pet: $(tail -1 <<< "$r")"; grep -q '^SELFTEST ARMED' <<< "$r" || { echo "  T-P1 PET FAILED"; WA=$((WA+1)); }
  e2=$(( $(cat "$T/r$D/backup/expires") - $(date +%s) )); echo "  F-P2 pet pushed the expiry: now + $e2 s (TTL $TTL)"; [ "$e2" -gt $((TTL-100)) ] && [ "$e2" -le "$TTL" ] || { echo "  F-P2 PUSH FAILED"; WA=$((WA+1)); }
  r=$(SIMRUN build); echo "  SIM build: $(cut -c1-110 <<< "$r")"; grep -q "xsha=$XR rel=$XR" <<< "$r" || { echo "  T-P1 BUILD FAILED"; WA=$((WA+1)); }
  SIMRUN lanwatch >/dev/null; SIMRUN usec $GP.timer NextElapseUSecMonotonic >/dev/null; SIMRUN state >/dev/null; SIMRUN check >/dev/null
  # arm A for real: the cut takes (disable_network), the hotspot line, recover stopped closes it (a hook restores the LAN)
  printf '%s\n' "lamp-setup[812]: lamp-setup: recover: watching" "lamp-setup[812]: lamp-setup: recover: no network: opening setup's hotspot" > "$FW/journal.lamp-setup-recover"
  printf '#!/bin/sh\necho 192.168.1.50/24 > "$FW/lan"\n' > "$FW/hook.stop.lamp-setup-recover"; chmod +x "$FW/hook.stop.lamp-setup-recover"
  SIMRUN arm A "$XR" 1 >/dev/null; echo "  SIM arm A: $(cat "$T/r$D/arm-A.result" | cut -c1-120)"; grep -q '^A stopped=.* lan_after=[0-9]' "$T/r$D/arm-A.result" || { echo "  T-P1 ARM A FAILED"; WA=$((WA+1)); }
  rm -f "$FW/hook.stop.lamp-setup-recover"; r=$(SIMRUN report A); echo "  SIM report A: $(grep -E '^(gtimer|result)=' <<< "$r" | tr '\n' ' ' | cut -c1-150)"
  grep -q '^result=A ' <<< "$r" || { echo "  T-P1 REPORT FAILED"; WA=$((WA+1)); }
  SIMRUN putback ifneeded kit-sim > "$T/pb.out"; echo "  SIM put-back: $(grep -oE 'PUTBACK (OK|FAILED) \(kit-sim\)' "$T/pb.out"), $(grep -c 'systemctl daemon-reload' "$FW/calls") daemon-reloads so far"
  grep -q 'PUTBACK OK (kit-sim)' "$T/pb.out" || { echo "  T-P1 PUTBACK FAILED"; WA=$((WA+1)); }
  : > "$FW/calls"; SIMRUN arm B "$XR" 1 >/dev/null; echo "  SIM arm B: $(cut -c1-60 < "$T/r$D/arm-B.result"), reboot -ff issued: $(grep -c REBOOT "$FW/calls"), switch drop-in in /etc: $(ls "$T/r/etc/systemd/system/lamp-setup-recover.service.d/" 2>/dev/null | tr '\n' ' ')"
  grep -q '^B rebooting' "$T/r$D/arm-B.result" && [ "$(grep -c REBOOT "$FW/calls")" = 1 ] || { echo "  T-P1 ARM B FAILED"; WA=$((WA+1)); }
  echo 192.168.1.50/24 > "$FW/lan"; SIMRUN putback force guard > "$T/pb.out"; echo "  SIM guard after B: $(grep -oE 'xcheck: switch drop-in [^ ]+ removed|PUTBACK (OK|FAILED) \(guard\)' "$T/pb.out" | tr '\n' ' ')"
  SIMRUN xcheck >/dev/null; SIMRUN restore >/dev/null
  # T-P2(c): phase0's short expiry (setup + 1 h) on a fresh backup; the full run's is setup + 3 h (F-P2)
  mv "$T/r$D/backup" "$T/bk"; SIMRUN backup 3600 >/dev/null; e1=$(( $(cat "$T/r$D/backup/expires") - $(date +%s) )); rm -rf "$T/r$D/backup"; mv "$T/bk" "$T/r$D/backup"
  echo "  T-P2 phase0 backup: expires in $e1 s (setup + 1 h); full run: 10800"; [ "$e1" -gt 3500 ] && [ "$e1" -le 3600 ] || { echo "  T-P2 PHASE0 TTL FAILED"; WA=$((WA+1)); }
  : > "$FW/calls"; out=$(SIMRUN disarm); echo "  SIM disarm: $(tail -1 <<< "$out")"; [ "$(nunits)" = 0 ] || { echo "  T-P1 DISARM FAILED"; WA=$((WA+1)); }
  # the no-LAN path (creboot) and the expiry (unarm, then $D) for real; then a fresh setup for the cases after
  units; station; apmode; rm -f "$FW/net_ok"; rm -f "$T/r$D/reboots"; : > "$FW/calls"; SIMRUN putback force guard > "$T/pb.out"; echo "  SIM no LAN: $(grep -oE 'counted reboot [0-9] of [0-9]' "$T/pb.out"), reboot issued: $(grep -c REBOOT "$FW/calls")"
  grep -q 'counted reboot' "$T/pb.out" || { echo "  T-P1 CREBOOT FAILED"; WA=$((WA+1)); }
  station; RUN restore >/dev/null; echo 1000 > "$T/r$D/backup/expires"; SIMRUN putback force guard > "$T/pb.out"; echo "  SIM expiry: units left $(nunits), $D $( [ -d "$T/r$D" ] && echo LEFT || echo removed)"
  [ "$(nunits)" = 0 ] && [ ! -d "$T/r$D" ] || { echo "  T-P1 EXPIRY FAILED"; WA=$((WA+1)); }
  units; station; RUN backup >/dev/null
  fs=$(grep -oE '^[a-z_]+\(\)' "$T/st.sh" | tr -d '()' | sort -u); ran=; miss=
  for fn in $fs; do if grep -qE "^\++ $fn( |$)" "$T/trace"; then ran="$ran $fn"; else miss="$miss $fn"; fi; done
  echo "  T-P1 functions run for real (SIM=1, dash -x):$ran"; echo "  T-P1 not run:${miss:- none}"
  [ -z "$miss" ] || { echo "  T-P1 COVERAGE GAP"; WA=$((WA+1)); }
  # S4: recover's program moved by lamp-update: logged, not a station put-back failure (xcheck itself refuses)
  units; station; NR=$(echo installed-9600 | shasum -a 256 | cut -c1-64); mkdir -p "$T/r/opt/lampOS/setup/$NR"; echo installed-9600 > "$T/r/opt/lampOS/setup/$NR/lamp-setup"; ln -sfn "$NR" "$T/r/opt/lampOS/setup/current"
  echo "  build moved: $(RUN build | cut -c1-110)"
  if RUN xcheck | sed 's/^/  negative (lamp-update moved the build): /'; [ "${PIPESTATUS[0]}" != 0 ]; then echo "  negative: xcheck refused, as it must"; else echo "  NEGATIVE CHECK FAILED"; WA=$((WA+1)); fi
  pb ifneeded kit-moved; echo "  put-back with the build moved: exit $(cat "$T/pb.rc"): $(grep -oE 'PUTBACK (OK|FAILED) \(kit-moved\)|build=[A-Z]+' "$T/pb.out" | tr '\n' ' ')"
  [ "$(cat "$T/pb.rc")" = 0 ] && grep -q 'build=MOVED' "$T/pb.out" || { echo "  BUILD-MOVED CASE FAILED"; WA=$((WA+1)); }
  # F-P3-3: go with a run that ends at once (GO_RUN=nosuch: usage, exit 2): the lock first, then PLAYING, and QUIET
  # still posted with the run's exit. gh and the lock are fakes (logged to a file: nothing posted, no lock taken).
  mkdir -p "$T/gobin"; printf '#!/bin/sh\necho "gh $*" >> "%s"\n' "$T/go.calls" > "$T/gobin/gh"; printf '#!/bin/sh\necho "lock $*" >> "%s"\n' "$T/go.calls" > "$T/golock"; chmod +x "$T/gobin/gh" "$T/golock"
  env PATH="$T/gobin:$PATH" ST_LOCK="$T/golock" GO_RUN=nosuch LOG="$T/go.log" LIMIT=$(dl) bash "$ME" go >/dev/null 2>&1; gr=$?
  la=$(grep -n '^lock lamp-52e6 acquire' "$T/go.calls" | cut -d: -f1); lp=$(grep -n '^gh issue comment 3336 .*PLAYING' "$T/go.calls" | cut -d: -f1); lq=$(grep -n '^gh issue comment 3336 .*QUIET' "$T/go.calls" | cut -d: -f1)
  echo "  F-P3-3 go, the run exits at once: go exit $gr; lock acquired (call ${la:-none}) before PLAYING (call ${lp:-none}); QUIET (call ${lq:-none}): $(grep -o 'QUIET .*(exit [0-9]*)' "$T/go.calls" | cut -c1-90)"
  [ "$gr" = 2 ] && [ -n "$la" ] && [ -n "$lp" ] && [ "$la" -lt "$lp" ] && grep -q 'QUIET .*(exit 2)' "$T/go.calls" || { echo "  F-P3-3 GO CASE FAILED"; WA=$((WA+1)); }
  # F2-P3: TERM to go as the acquire returns (no PLAYING yet): lock released, no post; TERM during PLAYING: released + QUIET
  for at in lock play; do : > "$T/go.calls"
    if [ $at = lock ]; then printf '#!/bin/sh\necho "lock $*" >> "%s"\n[ "$2" = acquire ] && kill -TERM $PPID\nexit 0\n' "$T/go.calls" > "$T/golock"
    else printf '#!/bin/sh\necho "lock $*" >> "%s"\n' "$T/go.calls" > "$T/golock"; printf '#!/bin/sh\necho "gh $*" >> "%s"\ncase "$*" in *PLAYING*) kill -TERM $PPID ;; esac\n' "$T/go.calls" > "$T/gobin/gh"; fi
    env PATH="$T/gobin:$PATH" ST_LOCK="$T/golock" GO_RUN=nosuch LOG="$T/go.log" LIMIT=$(dl) bash "$ME" go >/dev/null 2>&1; gr=$?
    echo "  F2-P3 TERM at $at: go exit $gr; calls: $(grep -oE '^lock lamp-52e6 [a-z]+|PLAYING|QUIET|nosuch' "$T/go.calls" | tr '\n' ' ')"
    q=$(grep -c 'QUIET' "$T/go.calls"); [ "$gr" = 130 ] && grep -q '^lock lamp-52e6 release' "$T/go.calls" && { { [ $at = lock ] && [ "$q" = 0 ] && ! grep -q PLAYING "$T/go.calls"; } || { [ $at = play ] && [ "$q" = 1 ]; }; } || { echo "  F2-P3 TERM CASE ($at) FAILED"; WA=$((WA+1)); }; done
  # v10 P2-1: another session took the lock (our lease expired off the network): the release refuses with the holder,
  # and QUIET says "lock lost to <holder>", not FREE
  : > "$T/go.calls"; printf '#!/bin/sh\necho "gh $*" >> "%s"\n' "$T/go.calls" > "$T/gobin/gh"
  printf '#!/bin/sh\necho "lock $*" >> "%s"\n[ "$2" = release ] && { printf "refusing: lamp-52e6 is not held by seventh (session x):\\n  holder: fifth\\n  session: st-99\\n" >&2; exit 1; }\nexit 0\n' "$T/go.calls" > "$T/golock"
  env PATH="$T/gobin:$PATH" ST_LOCK="$T/golock" GO_RUN=nosuch LOG="$T/go.log" LIMIT=$(dl) bash "$ME" go >/dev/null 2>&1; gr=$?
  echo "  v10 lock lost to another: go exit $gr; QUIET: $(grep -o 'QUIET .*' "$T/go.calls" | sed 's/^.*(exit/(exit/' | cut -c1-90)"
  grep -q 'QUIET .*lock lost to fifth (st-99)\.' "$T/go.calls" && ! grep -q 'QUIET .*FREE' "$T/go.calls" || { echo "  v10 LOCK-LOST QUIET CASE FAILED"; WA=$((WA+1)); }
  # L: left() past midnight, the clock stubbed (date as a shell function in a subshell): at 23:10 LIMIT 0030 is
  # 4800 s, 2300 has passed, 1200 has passed (tomorrow's is over 12 h away); at 00:10 0030 is 1200 s; a bad LIMIT -1
  lt=$(for c in "23 10 00 0030" "23 10 00 2300" "23 10 00 1200" "00 10 00 0030" "23 10 00 9999"; do set -- $c
    ( NOWF="$1 $2 $3"; date() { echo "$NOWF"; }; LIMIT=$4; left ); done | tr '\n' ' ')
  echo "  L left() at 23:10 for 0030 2300 1200, at 00:10 for 0030, LIMIT 9999: $lt"; [ "$lt" = "4800 -600 -40200 1200 -1 " ] || { echo "  L LEFT CASE FAILED"; WA=$((WA+1)); }
  rm -rf "$T"
  echo "== guard units:"; guard_units | sed 's/^/  /'
  LIMIT=${LIMIT:-$(dl)}
else runtraps
fi
preflight
setup
phase0
if [ -n "$PHASE0_ONLY" ]; then echo "== phase 0 only: no arm; the put-back follows $(date +%T)"
else for X in $ARMS; do armrun "$X"; done; echo "== all arms played $(date +%T)"
  # T-P3-3: a B window that ends VOID without a reboot is read on the same boot (no wait for a new boot)
  [ -z "$DRY" ] || { echo "== arm B ending VOID, no reboot (dry)"; FAKE_VOID=B armrun B
    # F-P3-5: B's report with no rec= line: VOID when this boot's journal starts late (rotated), FAIL when it starts at boot
    r=$(fake report-B | grep -v '^rec='); o1=$(verdict B "$r"$'\n'"jfirst=1712.3"); o2=$(verdict B "$r"$'\n'"jfirst=0.000000")
    echo "== no rec= line (dry): jfirst=1712.3: $o1"; echo "== no rec= line (dry): jfirst=0.000000: $o2"
    grep -q 'VOID no recover line kept' <<< "$o1" && grep -q 'FAIL no boot repair line' <<< "$o2" || { echo "F-P3-5 JFIRST CASE FAILED"; WA=$((WA+1)); }
    # v10 P2-1: the lamp lock lost (renew fails) before B's window (no lamp start), or after B's waitguard (no tidy, no
    # further arm); abort as a real run's (exit, fin on EXIT): fin then puts back only (no arm start, no reboot)
    for c in "0 before" "1 after"; do set -- $c
      o=$( ( abort() { echo "ABORT $*"; exit 1; }; trap fin EXIT; FAKE_RENEW_OK=$1 armrun B ) 3>&1 2>&1 ); a=$(sed -n '/^ABORT/,$p' <<< "$o")
      echo "== v10 lock lost $2 B's window (dry): $(grep -m1 '^ABORT' <<< "$o" | cut -c1-110); B started $(grep -c 'unit=seventh-st4863-arm-B' <<< "$o"), then: $(grep -oE 'kit-(back|after-B)|systemctl reboot|unit=seventh-st4863-arm-[ABC]|PUTBACK OK: files equal|PUT BACK' <<< "$a" | sort -u | tr '\n' ' ')"
      grep -q '^ABORT the lamp lock was lost' <<< "$o" && grep -q 'kit-back-' <<< "$a" && grep -q '^PUTBACK OK: files equal' <<< "$a" && ! grep -qE 'kit-after-B|systemctl reboot|reboot -ff|unit=seventh-st4863-arm-' <<< "$a" \
        && [ "$(grep -c 'unit=seventh-st4863-arm-B' <<< "$o")" = "$( [ $2 = before ] && echo 0 || echo 1)" ] || { echo "v10 LOCK-LOST $2 CASE FAILED"; WA=$((WA+1)); }; done
    # v10 P2-2: kputback's first start lost (ssh failed: inactive, no line for this call): started again, then OK;
    # never started: 3 restarts, then unknown (the caller aborts)
    PBN=0; o=$(FAKE_PBLOST=1 kputback lost 2>&1); r=$?; echo "== v10 kputback, its first start lost (dry): exit $r, $(grep -c 'started again' <<< "$o") restart(s): $(tail -1 <<< "$o" | cut -c1-80)"
    [ $r = 0 ] && [ "$(grep -c 'started again' <<< "$o")" = 1 ] && grep -q 'PUTBACK OK (kit-lost-' <<< "$o" || { echo "v10 KPUTBACK RETRY CASE FAILED"; WA=$((WA+1)); }
    PBN=0; o=$(FAKE_PBLOST=9 kputback lost 2>&1); r=$?; echo "== v10 kputback, no start ever runs (dry): exit $r, $(grep -c 'started again' <<< "$o") restart(s): $(tail -1 <<< "$o" | cut -c1-80)"
    [ $r = 1 ] && [ "$(grep -c 'started again' <<< "$o")" = 3 ] && grep -q 'its unit never ran (3 restarts)' <<< "$o" || { echo "v10 KPUTBACK GIVE-UP CASE FAILED"; WA=$((WA+1)); }; }; fi
if [ -n "$DRY" ]; then echo "== put-back (dry)"; fin
  echo "== back run a second time (dry): the lamp has nothing of the kit left"; r=$(FAKE_GONE=1 fin 3>&1 2>&1); echo "$r"
  grep -q 'already put back' <<< "$r" && ! grep -q 'NOT OK' <<< "$r" || { echo "BACK TWICE FALSE ALARM"; WA=$((WA+1)); }
  echo "== DRY verdict: $WA would-abort gate(s) (with pass-shaped, systemd-252-shaped lamp fakes)"; ls "$DD" | sed 's/^/  kept: /'; fi

