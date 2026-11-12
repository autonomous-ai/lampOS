#!/bin/sh
# Kit 5754 setup: rows 3 (nobody at the lamp) and 5 (comfort-break's words), lampOS #5754.
# Run as root from /run/kit5754, with the installed sequence you just read (RUN.md step 2):
#   sh /run/kit5754/setup.sh INSTALLED_SEQUENCE
#   LIFT_OFF=yes sh /run/kit5754/setup.sh INSTALLED_SEQUENCE   # only with the 52e6 holder's yes
#
# What it changes, all put back by putback.sh:
#   - with LIFT_OFF=yes only: the line `comfort-break` taken out of the skills' off file
#     (a person's choice), the original saved first with its sha256. lamp-skills reads that
#     file again within 2 s; nothing is restarted for it.
#   - recorders as transient units (kit5754-rec, kit5754-journal) and a dead-man timer
#     (kit5754-deadman) that runs putback.sh at HARD_END_MIN minutes (default 300).
# It restarts nothing and adds no drop-in: row3.sh adds its own /run drop-in for the clock.
set -eu
R=/run/kit5754
. $R/common.sh
HARD_END_MIN=${HARD_END_MIN:-300}
LIFT_OFF=${LIFT_OFF:-no}
CHANGED=0
stop() { echo "STOP: $*"; [ $CHANGED = 1 ] || rm -rf $B; exit 1; }
[ "$(id -u)" = 0 ] || stop "not root"
case "$HARD_END_MIN" in ''|*[!0-9]*) stop "HARD_END_MIN is minutes";; esac
[ "$HARD_END_MIN" -ge 30 ] && [ "$HARD_END_MIN" -le 420 ] || stop "HARD_END_MIN is 30..420"
not_quiet || stop "the skills' quiet hours (22:00-08:00): this kit never runs then"

# ---- the build: the operator confirms it by typing the sequence ----
NOW=$(head -c 300 /var/lib/lampos-update/state.json | tr -d ' \n' | grep -o '"installed":{"sequence":[0-9]*,"commit":"[0-9a-f]*"') || true
NOW_SEQ=$(echo "$NOW" | grep -o '"sequence":[0-9]*' | cut -d: -f2) || true
NOW_C=$(echo "$NOW" | grep -o '"commit":"[0-9a-f]*"' | cut -d'"' -f4) || true
echo "lamp installed: ${NOW_SEQ:-?} ${NOW_C:-?}"
[ -n "$NOW_SEQ" ] || stop "can't read the installed build from state.json"
[ "${1:-}" = "$NOW_SEQ" ] || stop "run as: sh $R/setup.sh $NOW_SEQ (after checking it is the build the rows are booked on)"
i=0; while upd_busy; do i=$((i+1)); [ $i -gt 120 ] && stop "lamp-update busy for 20 min"; echo "waiting for lamp-update's round ($i)"; sleep 10; done
[ ! -e $B ] || { echo "STOP: $B exists (an earlier run not put back?)"; exit 1; }
d=$DD/lamp-skills.service.d
[ -d $d ] && [ -n "$(ls -A $d)" ] && stop "someone's /run drop-ins on lamp-skills: $(ls $d)"
for u in kit5754-rec kit5754-journal kit5754-deadman kit5754-row3 kit5754-row5; do
  systemctl is-active -q $u.service && stop "$u.service runs already"
done
(cd $R && sha256sum -c --quiet SHA256SUMS) || stop "staged sums"

# ---- the skills, the voice and the switch ----
systemctl is-active -q lamp-skills || stop "lamp-skills not active"
systemctl is-active -q lamp-voice || stop "lamp-voice not active"
P=$(skills_pid); [ "$P" -gt 0 ] || stop "lamp-skills has no main process"
tr '\0' '\n' < /proc/$P/environ | grep -qx 'LAMP_NUDGES_PRESENT=on' || stop "lamp-skills runs without LAMP_NUDGES_PRESENT=on: comfort-break is not on here"
installed comfort-break || stop "comfort-break is not in $SK"
grep -q '^\[present\]' $SK/comfort-break/skill.toml || stop "comfort-break's file has no [present]"
in_off meal-check && stop "meal-check is in the off file (a person's choice): this kit lifts only comfort-break"
CB_OFF=no; in_off comfort-break && CB_OFF=yes
if [ $CB_OFF = yes ] && [ "$LIFT_OFF" != yes ]; then
  stop "comfort-break is in the off file, a person's choice. It is lifted for these rows only with the 52e6 holder's yes: rerun with LIFT_OFF=yes. Nothing was changed."
fi
M=$(mute_now); [ "$M" = '"muted":false' ] || stop "voice is muted or silent: $M"
VR=$(voice_ready)
[ "${VR:-0}" -gt 0 ] || echo "NOTE: no listening_ready from the voice's current run in the journal (rotated out?); voice/state $(vstate_now)"
mkdir -p $B/logs $B/rec; chmod 700 $B
install -m 755 $R/lamp-bus $B/lamp-bus          # /run is noexec
install -m 755 $R/kit5754-skip $B/kit5754-skip
for f in common.sh row3.sh row5.sh putback.sh; do install -m 644 $R/$f $B/$f; done
. $B/common.sh

# ---- the before-state, compared SAME/DIFF by putback.sh ----
head -c 300 /var/lib/lampos-update/state.json > $B/state-before.json
for l in /opt/lampOS/*/current; do echo "$l $(readlink $l)"; done > $B/links.txt
units() { systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' \
  | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort; }
units > $B/units.txt
ls /run/systemd/system/ | sort > $B/run-units.txt
md5sum /etc/systemd/system/lamp-*.service.d/* > $B/etc-dropins.md5 2>/dev/null || true
readlink /proc/$P/exe > $B/skills-exe-before.txt
tr '\0' ' ' < /proc/$P/cmdline > $B/skills-argv-before.txt
envof $P > $B/skills-env-before.txt
systemctl cat lamp-skills | grep '^ExecStart=.' | tail -1 > $B/skills-execstart.txt
ls -1 $SK | sort > $B/skills-dir-before.txt
installed meal-check && echo "present $(version_of meal-check)" > $B/meal-before.txt || echo absent > $B/meal-before.txt
if [ -e $OFF ]; then
  cp -p $OFF $B/off.orig
  sha256sum $OFF | cut -d' ' -f1 > $B/off-before.sha
  stat -c '%u:%g %a' $OFF > $B/off-before.stat
else
  echo absent > $B/off-before.sha
fi
systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort > $B/failed-before.txt
echo "$(mute_now)" > $B/mute-before.txt
echo "${VR:-0}" > $B/ready-before.txt
echo "skills folder: $(wc -l < $B/skills-dir-before.txt) entries; meal-check: $(cat $B/meal-before.txt)"
echo "off file: $(cat $B/off-before.sha | cut -c1-12); lines: $( [ -e $OFF ] && tr '\n' ' ' < $OFF)"
echo "comfort-break in off: $CB_OFF; LIFT_OFF=$LIFT_OFF"
echo "lamp-skills: $(cat $B/skills-exe-before.txt), LAMP_NUDGES_PRESENT=on"
echo "faces now (faces near max_h): $(faces_now); voice $(vstate_now) $(mute_now)"

# ---- changes: put back by putback.sh, also by the dead-man and on any failure below ----
CHANGED=1
trap 'echo "setup failed: putting back"; sh $B/putback.sh' EXIT
systemd-run --unit=kit5754-deadman --collect --on-active=${HARD_END_MIN}m sh $B/putback.sh deadman >/dev/null 2>&1 \
  || stop "dead-man timer not armed"
echo "$(date -d "+${HARD_END_MIN} min" '+%H:%M')" > $B/hard-end.txt
systemd-run --unit=kit5754-rec --collect sh -c "exec $BUS record $B/rec skills/events voice/said voice/state voice/mute humans/faces/tracked --max-mb 64" >/dev/null
# lamp-skills whole (its events and every `not run` line); from the voice only its readiness and mute.
systemd-run --unit=kit5754-journal --collect sh -c "journalctl -f -n0 -o short-unix -u lamp-skills -u lamp-voice -u lamp-update | grep --line-buffered -E 'lamp-skills|listening_ready|restart guard|muted|lamp-update' >> $B/logs/journal.txt" >/dev/null
sleep 3
systemctl is-active -q kit5754-rec || stop "the recorder did not start"
systemctl is-active -q kit5754-journal || stop "the journal follower did not start"
systemctl is-active -q kit5754-deadman.timer || stop "the dead-man timer is not active"

if [ $CB_OFF = yes ]; then
  T0=$(date '+%Y-%m-%d %H:%M:%S')
  grep -vx comfort-break $B/off.orig > $B/off.lifted || true
  cat $B/off.lifted > $OFF        # same file, owner and mode; lamp-skills reads it again within 2 s
  echo lifted > $B/off-lifted.txt
  i=0; until offkept_since "$T0" | grep -q 'read again'; do
    i=$((i+1)); [ $i -gt 10 ] && stop "lamp-skills did not read the off file again in 10 s"; sleep 1; done
  echo "off file: comfort-break lifted ($(sha256sum $OFF | cut -c1-12)); lamp-skills read it again"
fi
in_off comfort-break && stop "comfort-break is still in the off file"
trap - EXIT
date -Iseconds > $B/setup.done
echo "dead-man: putback.sh runs at $(cat $B/hard-end.txt) unless put back before"
echo "lamp clock: $(date -Iseconds)"
echo "setup done"
