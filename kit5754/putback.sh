#!/bin/sh
# Kit 5754 put-back. Run as root:  sh /root/kit5754/putback.sh
# (The dead-man timer runs it as `putback.sh deadman` at the hard end.)
#   KEEP_MEAL=yes   keep a meal-check that was added during the kit (the app's Store Add);
#                   by default a meal-check the kit did not find at setup is removed.
# Waits for lamp-update to be quiet first (exit 2 = OWED, nothing changed); never stops,
# restarts or masks lamp-update or lamp-update-back, never takes the updater's lock.
# Last line: PUTBACK-OK, or PUTBACK-DIFF with what differs.
. /root/kit5754/common.sh
WHO=${1:-hand}
KEEP_MEAL=${KEEP_MEAL:-no}
[ "$(id -u)" = 0 ] || { echo "not root"; exit 1; }
[ -d $B ] || { echo "nothing to put back: $B is gone"; exit 0; }
mkdir -p $B/logs
log() { echo "$*" | tee -a $B/logs/putback.out; }
log "putback ($WHO) $(date -Iseconds)"
touch $B/STOP                                  # any row still running stops at its next look
i=0
while upd_busy; do
  i=$((i+1)); [ $i -gt 120 ] && { log "OWED: lamp-update still busy after 20 min, nothing put back; run again once it is quiet"; exit 2; }
  echo "waiting for lamp-update's round ($i)"; sleep 10
done

# ---- the kit's own units ----
for u in kit5754-row3 kit5754-row5 kit5754-rec kit5754-journal; do
  systemctl stop $u.service 2>/dev/null
done
systemctl stop kit5754-deadman.timer 2>/dev/null
[ "$WHO" = deadman ] || systemctl stop kit5754-deadman.service 2>/dev/null

# ---- row 3's clock drop-in ----
RESTART=no
if [ -e $DROPIN ]; then rm -f $DROPIN; RESTART=yes; log "row 3's clock drop-in removed"; fi
rmdir $DD/lamp-skills.service.d 2>/dev/null
systemctl daemon-reload
P=$(skills_pid)
[ "$P" -gt 0 ] && [ "$(tr '\0' ' ' < /proc/$P/cmdline)" != "$(cat $B/skills-argv-before.txt)" ] && RESTART=yes
if [ $RESTART = yes ]; then systemctl restart lamp-skills; sleep 6; log "lamp-skills restarted on its found argv"; fi

# ---- meal-check: as found, unless KEEP_MEAL=yes ----
MB=$(cat $B/meal-before.txt 2>/dev/null)
if [ "$MB" = absent ] && installed meal-check; then
  if [ "$KEEP_MEAL" = yes ]; then log "meal-check $(version_of meal-check) kept (KEEP_MEAL=yes): added during the kit"
  else
    if LAMP_NUDGES_PRESENT=on $SKILLS_BIN remove --dir $SK --state-dir $STATE meal-check >> $B/logs/putback.out 2>&1; then
      log "meal-check removed (it was not on the lamp at setup)"
    else log "DIFF meal-check: lamp-skills remove refused (see logs/putback.out)"; fi
  fi
fi

# ---- the off file: byte for byte as found ----
T=$(date '+%Y-%m-%d %H:%M:%S')
OB=$(cat $B/off-before.sha 2>/dev/null)
if [ "$OB" = absent ]; then
  [ -e $OFF ] && { rm -f $OFF; log "off file removed (none at setup)"; }
elif [ -n "$OB" ]; then
  NOWSHA=$(sha256sum $OFF 2>/dev/null | cut -d' ' -f1)
  if [ "$NOWSHA" != "$OB" ]; then
    cat $B/off.orig > $OFF
    set -- $(cat $B/off-before.stat); chown "$1" $OFF; chmod "$2" $OFF
    log "off file written back"
    i=0; until offkept_since "$T" | grep -q -E 'read again|kept in'; do
      i=$((i+1)); [ $i -gt 10 ] && { log "NOTE lamp-skills said no re-read of the off file in 10 s"; break; }; sleep 1; done
  fi
fi
sleep 5                                        # lamp-skills reads it again every 2 s: it must not write it back

# ---- checks ----
DIFFS=""
s() { if [ "$2" = "$3" ]; then log "SAME $1"; else log "DIFF $1: [$2] vs [$3]"; DIFFS="$DIFFS; $1"; fi; }
P=$(skills_pid)
s "lamp-skills binary" "$(cat $B/skills-exe-before.txt)" "$(readlink /proc/$P/exe)"
s "lamp-skills argv" "$(cat $B/skills-argv-before.txt)" "$(tr '\0' ' ' < /proc/$P/cmdline)"
s "lamp-skills env" "$(cat $B/skills-env-before.txt)" "$(envof $P)"
s "lamp-skills ExecStart" "$(cat $B/skills-execstart.txt)" "$(systemctl cat lamp-skills | grep '^ExecStart=.' | tail -1)"
s "clock drop-in gone" absent "$([ -e $DROPIN ] && echo present || echo absent)"
s "/run units" "$(cat $B/run-units.txt)" "$(ls /run/systemd/system/ | sort)"
s "/etc drop-ins" "$(cat $B/etc-dropins.md5)" "$(md5sum /etc/systemd/system/lamp-*.service.d/* 2>/dev/null)"
s "current links" "$(cat $B/links.txt)" "$(for l in /opt/lampOS/*/current; do echo "$l $(readlink $l)"; done)"
units() { systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' \
  | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort; }
s "active lamp-* units" "$(cat $B/units.txt)" "$(units)"
s "failed lamp-* units" "$(cat $B/failed-before.txt)" "$(systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort)"
if [ "$OB" = absent ]; then s "off file" absent "$([ -e $OFF ] && echo present || echo absent)"
else
  s "off file sha256" "$OB" "$(sha256sum $OFF 2>/dev/null | cut -d' ' -f1)"
  s "off file owner/mode" "$(cat $B/off-before.stat)" "$(stat -c '%u:%g %a' $OFF 2>/dev/null)"
fi
K=$(offkept_since "$T"); [ -n "$K" ] || K=$(journalctl -u lamp-skills --no-pager -o cat -n 400 2>/dev/null | grep -E 'turned off (kept in|read again)' | tail -1)
log "lamp-skills keeps off: $K"
if [ "$MB" = absent ] && [ "$KEEP_MEAL" = yes ] && installed meal-check; then
  log "NOTE skills folder: meal-check kept by KEEP_MEAL=yes"
  s "skills folder (meal-check aside)" "$(cat $B/skills-dir-before.txt)" "$(ls -1 $SK | grep -vx meal-check | sort)"
else
  s "skills folder" "$(cat $B/skills-dir-before.txt)" "$(ls -1 $SK | sort)"
fi
s "kit units gone" "" "$(systemctl list-units --all --no-legend --plain 'kit5754-*' | awk '$1 !~ /deadman.service/ {print $1}' | tr '\n' ' ')"
s "voice/mute" '"muted":false' "$(mute_now)"
R=$(voice_ready)
VS=$(vstate_now)
if [ "${R:-0}" -gt 0 ]; then log "SAME voice listening_ready (its current run said it ${R}x)"
elif [ "$(cat $B/ready-before.txt 2>/dev/null)" = 0 ] && [ -n "$VS" ]; then
  log "NOTE voice listening_ready: out of the journal before the kit too (0 at setup); the voice publishes $VS and was never restarted"
else log "DIFF voice listening_ready: none from the voice's current run"; DIFFS="$DIFFS; voice listening_ready"; fi
log "voice/state: $(vstate_now)"
log "UPDATER self.json: $([ -e /var/lib/lampos-update/self.json ] && echo present || echo none); state.json $( [ "$(head -c 300 /var/lib/lampos-update/state.json)" = "$(cat $B/state-before.json)" ] && echo unchanged || echo CHANGED)"
log "._ files: $(find /var/lib/lampOS /run/kit5754 $B -name '._*' 2>/dev/null | wc -l)"
date -Iseconds > $B/putback.done
if [ -z "$DIFFS" ]; then
  log "PUTBACK-OK: no drop-in, lamp-skills on its found binary/argv/env, off file sha256 $(echo "$OB" | cut -c1-12) as found, meal-check $( [ "$MB" = absent ] && { installed meal-check && echo "kept (KEEP_MEAL)" || echo "removed (not there at setup)"; } || echo "as found"), kit units and dead-man gone, voice listening_ready and not muted"
else
  log "PUTBACK-DIFF:${DIFFS#;}"
fi
log "Copy $B to the Mac (logs and rec), then: rm -rf $B /run/kit5754"
