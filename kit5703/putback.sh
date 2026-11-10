#!/bin/sh
# Kit 5703 put-back. Run as root:  sh /root/kit5703/putback.sh
# (The dead-man runs it as `putback.sh deadman` at the hard end, also after a power cycle.)
# Waits for lamp-update to be quiet first (exit 2 = OWED, nothing changed); never stops, restarts,
# masks or disables lamp-update or lamp-update-back, never takes the updater's lock. Restarts only
# lamp-body (a standing lamp folds as it stops, then starts on its stock binary from folded).
# Last line: PUTBACK-OK, or PUTBACK-DIFF with what differs.
. /root/kit5703/common.sh
WHO=${1:-hand}
[ "$(id -u)" = 0 ] || { echo "not root"; exit 1; }
[ -d $B ] || { echo "nothing to put back: $B is gone"; exit 0; }
mkdir -p $B/logs
log() { echo "$*" | tee -a $B/logs/putback.out; }
log "putback ($WHO) $(date -Iseconds)"
i=0
while upd_busy; do
  i=$((i+1)); [ $i -gt 120 ] && { log "OWED: lamp-update still busy after 20 min, nothing put back; run again once it is quiet"; exit 2; }
  echo "waiting for lamp-update's round ($i)"; sleep 10
done

# ---- the drop-in, the dead-man, the staged binary ----
RESTART=no
if [ -e $DROPIN ]; then rm -f $DROPIN; RESTART=yes; log "drop-in removed: $DROPIN"; fi
rmdir $ED/lamp-body.service.d 2>/dev/null
systemctl disable $ON-deadman.timer >/dev/null 2>&1
systemctl stop $ON-deadman.timer 2>/dev/null
rm -f $DEADMAN.timer
[ "$WHO" = deadman ] || rm -f $DEADMAN.service    # the dead-man's own run: its file goes at the end
systemctl daemon-reload
P=$(body_pid)
[ "${P:-0}" -gt 0 ] && [ "$(readlink /proc/$P/exe)" != "$(cat $B/stock-exe.txt 2>/dev/null)" ] && RESTART=yes
systemctl is-active -q lamp-body || RESTART=yes     # a row left midway (stopped, the lamp let go)
if [ $RESTART = yes ]; then
  T0=$(date '+%Y-%m-%d %H:%M:%S')
  log "restarting lamp-body on its stock binary (a standing lamp folds first; keep hands clear)"
  systemctl reset-failed lamp-body 2>/dev/null || true
  systemctl restart lamp-body
  i=0; until systemctl is-active -q lamp-body; do i=$((i+1)); [ $i -gt 200 ] && break; sleep 1; done
  log "the staged run's exit report:"; exit_report_since "$T0" | head -12 | tee -a $B/logs/putback.out
fi
rm -f $STAGED; rmdir $X 2>/dev/null

# ---- checks ----
DIFFS=""
s() { if [ "$2" = "$3" ]; then log "SAME $1"; else log "DIFF $1: [$2] vs [$3]"; DIFFS="$DIFFS; $1"; fi; }
sleep 3
P=$(body_pid)
s "lamp-body active" active "$(systemctl is-active lamp-body)"
s "lamp-body binary sha256 (stock, recorded at setup)" "$(cat $B/stock-sha.txt)" "$(body_exe_sha)"
s "lamp-body binary path" "$(cat $B/stock-exe.txt)" "$(readlink /proc/$P/exe 2>/dev/null)"
s "lamp-body argv" "$(cat $B/argv-before.txt)" "$(tr '\0' ' ' < /proc/$P/cmdline 2>/dev/null)"
s "lamp-body ExecStart" "$(cat $B/execstart.txt)" "$(systemctl cat lamp-body | grep '^ExecStart=')"
s "lamp-body TimeoutStartUSec" "$(cat $B/timeout-before.txt)" "$(systemctl show -p TimeoutStartUSec --value lamp-body)"
s "kit drop-in gone" absent "$([ -e $DROPIN ] && echo present || echo absent)"
s "staged binary gone" absent "$([ -e $STAGED ] && echo present || echo absent)"
s "dead-man timer gone" absent "$([ -e $DEADMAN.timer ] || [ -e $ED/timers.target.wants/$ON-deadman.timer ] && echo present || echo absent)"
s "/etc drop-ins" "$(cat $B/etc-dropins.md5)" "$(md5sum $ED/lamp-*.service.d/* 2>/dev/null)"
s "current links" "$(cat $B/links.txt)" "$(links_now)"
units() { systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' \
  | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort; }
s "active lamp-* units" "$(cat $B/units.txt)" "$(units)"
s "failed lamp-* units" "$(cat $B/failed-before.txt)" "$(systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort)"
# The lamp is standing-capable: started, its start not refused.
BL=$(journalctl -u lamp-body --since "${T0:-$(cat $B/setup.done 2>/dev/null | cut -c1-19 | tr T ' ')}" --no-pager -o cat 2>/dev/null | grep -E 'will not stand|waiting to be set straight' | tail -1)
if [ -n "$BL" ]; then log "DIFF lamp-body start: $BL"; DIFFS="$DIFFS; lamp-body start"; else log "SAME lamp-body started with no 'will not stand' and no wait"; fi
# The voice: listening and not muted (this kit never restarts it; a power cut row does).
M=$(mute_now)
case "$M" in
  *'"muted":false'*) log "SAME voice/mute $M";;
  '') log "NOTE voice/mute unread (no lamp-bus on this lamp)";;
  *) loud "THE VOICE IS MUTED ($M)"; DIFFS="$DIFFS; voice muted"
     log "If muted_by is restart_guard (after a power cut): a clean 'systemctl stop lamp-voice; systemctl start lamp-voice' ends it (#4332), only with the holder's yes; a person's mute is theirs.";;
esac
R=$(voice_ready)
if [ "${R:-0}" -gt 0 ]; then log "SAME voice listening_ready (its current run said it ${R}x)"
elif [ "$(cat $B/ready-before.txt 2>/dev/null)" = 0 ] && [ -n "$(vstate_now)" ]; then
  log "NOTE voice listening_ready: out of the journal before the kit too (0 at setup); the voice publishes $(vstate_now)"
else log "DIFF voice listening_ready: none from the voice's current run"; DIFFS="$DIFFS; voice listening_ready"; fi
log "voice/state: $(vstate_now)"
log "UPDATER self.json: $([ -e /var/lib/lampos-update/self.json ] && echo present || echo none); state.json $( [ "$(head -c 300 /var/lib/lampos-update/state.json)" = "$(cat $B/state-before.json)" ] && echo unchanged || echo CHANGED); lamp-update $(systemctl is-active lamp-update)"
log "._ files: $(find /run/kit5703 $B /root/local -name '._*' 2>/dev/null | wc -l)"
[ -s $B/logs/results.txt ] && { log "rows played:"; tee -a $B/logs/putback.out < $B/logs/results.txt; }
date -Iseconds > $B/putback.done
if [ -z "$DIFFS" ]; then
  log "PUTBACK-OK: drop-in, dead-man and staged binary gone; lamp-body active on its stock binary (sha256 $(cut -c1-12 $B/stock-sha.txt)), ExecStart and TimeoutStartUSec as found; voice listening and not muted"
else
  log "PUTBACK-DIFF:${DIFFS#;}"
fi
log "Copy $B/logs to the Mac, then: rm -rf $B /run/kit5703"
[ "$WHO" = deadman ] && { rm -f $DEADMAN.service; systemctl daemon-reload; }
exit 0
