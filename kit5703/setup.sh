#!/bin/sh
# Kit 5703 setup: lamp-body from lampOS #5703's CI artifact (ARM64 run 42590674657, head 5caac04a46),
# staged and put back by sha256. Run as root, from /run/kit5703, with the installed sequence:
#   sh /run/kit5703/setup.sh INSTALLED_SEQUENCE
#   HARD_END_MIN=240 sh /run/kit5703/setup.sh INSTALLED_SEQUENCE   # the dead-man's minutes (60..480)
#
# What it changes, all put back by putback.sh (and by the dead-man at the hard end):
#   - $X/lamp-body: the staged binary (root:root 0755; lampos runs it; /run may be noexec).
#   - $DROPIN: ExecStart= the found unit's own ExecStart with only the binary's path changed, and
#     TimeoutStartSec=180 (#5703's product unit). In /etc, so that a real power cycle keeps it.
#   - $DEADMAN.timer/.service: runs putback.sh at the hard end, Persistent=true, so it still runs
#     after a power cycle (enabled into timers.target; putback disables and removes it).
#   - lamp-body restarted once, onto the staged binary. Nothing else is stopped, restarted or masked:
#     not lamp-update, not lamp-update-back, not the voice.
set -eu
R=/run/kit5703
. $R/common.sh
HARD_END_MIN=${HARD_END_MIN:-240}
CHANGED=0
stop() { echo "STOP: $*"; [ $CHANGED = 1 ] || rm -rf $B $X; exit 1; }
[ "$(id -u)" = 0 ] || { echo "STOP: not root"; exit 1; }
[ ! -e $B ] || { echo "STOP: $B exists (an earlier run not put back? run putback.sh first)"; exit 1; }
[ ! -e $X ] || { echo "STOP: $X exists (an earlier run not put back? run putback.sh first)"; exit 1; }
case "$HARD_END_MIN" in ''|*[!0-9]*) stop "HARD_END_MIN is minutes";; esac
[ "$HARD_END_MIN" -ge 60 ] && [ "$HARD_END_MIN" -le 480 ] || stop "HARD_END_MIN is 60..480"
(cd $R && sha256sum -c --quiet SHA256SUMS) || stop "staged sums"
[ "$(sha256sum $R/lamp-body | cut -d' ' -f1)" = "$STAGED_SHA" ] || stop "lamp-body is not the CI artifact's ($STAGED_SHA)"
find $R -name '._*' | grep -q . && stop "._ files in $R (re-pack on the Mac with COPYFILE_DISABLE=1 tar --exclude='._*')"

# ---- the build: the operator confirms it by typing the sequence ----
NOW=$(head -c 300 /var/lib/lampos-update/state.json | tr -d ' \n' | grep -o '"installed":{"sequence":[0-9]*,"commit":"[0-9a-f]*"') || true
NOW_SEQ=$(echo "$NOW" | grep -o '"sequence":[0-9]*' | cut -d: -f2) || true
NOW_C=$(echo "$NOW" | grep -o '"commit":"[0-9a-f]*"' | cut -d'"' -f4) || true
echo "lamp installed: ${NOW_SEQ:-?} ${NOW_C:-?}; staged lamp-body: $HEAD (#5703)"
[ -n "$NOW_SEQ" ] || stop "can't read the installed build from state.json"
[ "${1:-}" = "$NOW_SEQ" ] || stop "run as: sh $R/setup.sh $NOW_SEQ (after RUN.md step 0's check of that build)"
i=0; while upd_busy; do i=$((i+1)); [ $i -gt 120 ] && stop "lamp-update busy for 20 min"; echo "waiting for lamp-update's round ($i)"; sleep 10; done

# ---- the found unit ----
systemctl is-active -q lamp-body || stop "lamp-body is not active"
[ -d /run/systemd/system/lamp-body.service.d ] && [ -n "$(ls -A /run/systemd/system/lamp-body.service.d)" ] \
  && stop "someone's /run drop-ins on lamp-body: $(ls /run/systemd/system/lamp-body.service.d)"
[ -e $DROPIN ] && stop "$DROPIN exists"
N=$(systemctl cat lamp-body 2>/dev/null | grep -c '^ExecStart=') || true
[ "$N" = 1 ] || stop "lamp-body's ExecStart is set $N times (a drop-in overrides it?): $(systemctl cat lamp-body | grep -n '^ExecStart=' | cut -c1-120)"
EXEC=$(systemctl cat lamp-body | grep '^ExecStart=')
case "$EXEC" in "ExecStart=$STOCK_PATH "*) ;; *) stop "lamp-body's ExecStart is not $STOCK_PATH ...: ${EXEC%% --*}";; esac
for t in $MEASURE $INSPECT; do [ -x $t ] || stop "no $t on this build"; done
[ -r $LAMP_ENV ] || stop "no $LAMP_ENV"
for a in home folded-arm stand; do [ -n "$(pose_arg $a)" ] || stop "lamp.env has no --$a (a seated lamp? this row needs a lamp that stands)"; done
command -v setpriv >/dev/null || stop "no setpriv"
mkdir -p /var/lib && T=$(mktemp -d /var/lib/kit5703-exec.XXXXXX) && cp /bin/true $T/t && chmod 755 $T/t
setpriv --reuid=lampos --regid=lampos --groups=dialout -- $T/t 2>/dev/null || { rm -rf $T; stop "lampos cannot exec from /var/lib (noexec?)"; }
rm -rf $T
M=$(mute_now)
case "$M" in *'"muted":true'*) stop "the voice is muted: $M";; esac
VR=$(voice_ready)
[ "${VR:-0}" -gt 0 ] || echo "NOTE: no listening_ready from the voice's current run in the journal (rotated out?); voice/state $(vstate_now)"

# ---- the before-state, compared SAME/DIFF by putback.sh ----
mkdir -p $B/logs; chmod 700 $B
for f in common.sh row.sh putback.sh SHA256SUMS RUN.md; do install -m 644 $R/$f $B/$f; done
P=$(body_pid)
STOCK_EXE=$(readlink /proc/$P/exe)
echo "$EXEC" > $B/execstart.txt
echo "$STOCK_EXE" > $B/stock-exe.txt
sha256sum /proc/$P/exe | cut -d' ' -f1 > $B/stock-sha.txt
[ "$(cat $B/stock-sha.txt)" = "$STAGED_SHA" ] && stop "lamp-body already runs the staged binary's sha256"
tr '\0' ' ' < /proc/$P/cmdline > $B/argv-before.txt
head -c 300 /var/lib/lampos-update/state.json > $B/state-before.json
echo "${NOW_SEQ} ${NOW_C}" > $B/build.txt
links_now > $B/links.txt
systemctl show -p TimeoutStartUSec --value lamp-body > $B/timeout-before.txt
ls /run/systemd/system/ | sort > $B/run-units.txt
md5sum $ED/lamp-*.service.d/* 2>/dev/null > $B/etc-dropins.md5 || true
systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort > $B/failed-before.txt
units() { systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' \
  | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort; }
units > $B/units.txt
echo "${M:-?}" > $B/mute-before.txt
echo "${VR:-0}" > $B/ready-before.txt
echo "$(pose_arg home) $(pose_arg folded-arm) $(pose_arg stand)" > $B/poses.txt
echo "found ExecStart: $(echo "$EXEC" | cut -c1-140)..."
echo "stock lamp-body: $STOCK_EXE sha256 $(cat $B/stock-sha.txt)"
echo "TimeoutStartUSec before: $(cat $B/timeout-before.txt)"
echo "lamp.env: --home $(pose_arg home) --folded-arm $(pose_arg folded-arm) --stand $(pose_arg stand)"

# ---- changes: put back by putback.sh, also by the dead-man and on any failure below ----
CHANGED=1
trap 'echo "setup failed: putting back"; sh $B/putback.sh setup-failed' EXIT
mkdir -p $X; chmod 755 $X
install -o root -g root -m 755 $R/lamp-body $STAGED
[ "$(sha256sum $STAGED | cut -d' ' -f1)" = "$STAGED_SHA" ] || stop "staged copy's sha256"
mkdir -p $ED/lamp-body.service.d
{ echo "# kit5703 (lampOS #5703 played row): put back by /root/kit5703/putback.sh"
  echo "[Service]"
  echo "ExecStart="
  echo "ExecStart=$STAGED ${EXEC#ExecStart="$STOCK_PATH" }"
  echo "TimeoutStartSec=180"; } > $DROPIN
END_AT=$(date -d "+${HARD_END_MIN} min" '+%Y-%m-%d %H:%M:%S')
echo "$END_AT" > $B/hard-end.txt
printf '[Unit]\nDescription=kit5703 dead-man: put lamp-body back at the hard end\n[Service]\nType=oneshot\nExecStart=/bin/sh %s/putback.sh deadman\n' $B > $DEADMAN.service
printf '[Unit]\nDescription=kit5703 dead-man timer\n[Timer]\nOnCalendar=%s\nPersistent=true\nAccuracySec=1s\n[Install]\nWantedBy=timers.target\n' "$END_AT" > $DEADMAN.timer
systemctl daemon-reload
systemctl enable --now $ON-deadman.timer >/dev/null 2>&1 || stop "dead-man timer not armed"
systemctl is-active -q $ON-deadman.timer || stop "dead-man timer not active"
[ "$(systemctl show -p TimeoutStartUSec --value lamp-body)" = 3min ] || stop "TimeoutStartSec=180 not in effect: $(systemctl show -p TimeoutStartUSec --value lamp-body)"

# The one restart: lamp-body onto the staged binary. A standing lamp folds as it stops, then it
# starts from folded as every day.
T0=$(date '+%Y-%m-%d %H:%M:%S')
systemctl reset-failed lamp-body 2>/dev/null || true
systemctl restart lamp-body
i=0; until systemctl is-active -q lamp-body; do i=$((i+1)); [ $i -gt 200 ] && stop "lamp-body not active 200 s after the restart"; sleep 1; done
P=$(body_pid)
[ "$(sha256sum /proc/$P/exe | cut -d' ' -f1)" = "$STAGED_SHA" ] || stop "lamp-body does not run the staged binary"
[ "$(readlink /proc/$P/exe)" = "$STAGED" ] || stop "lamp-body runs $(readlink /proc/$P/exe)"
body_lines "$T0" | tail -5
stop_lines "$T0" | grep -q . && stop "lamp-body's start on the staged binary had a stop line"
trap - EXIT
date -Iseconds > $B/setup.done
echo "lamp-body: $STAGED sha256 $STAGED_SHA, active, TimeoutStartUSec $(systemctl show -p TimeoutStartUSec --value lamp-body)"
echo "voice: $(mute_now) $(vstate_now)"
echo "dead-man: putback.sh runs at $END_AT (Persistent: also after a power cycle) unless put back before"
echo "lamp clock: $(date -Iseconds)"
echo "setup done. Rows: sh $B/row.sh 1a | 1b | 2   (RUN.md)"
