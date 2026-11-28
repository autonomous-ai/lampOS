#!/bin/sh
# Run as root on lamp-75c9 after seed.sh: sh trigger.sh. Leg 3, THE REAL RESET, started the product's own way:
# exactly the command watch-reset hands the button's reset to (main.rs start_reset: the plan's `detach`, then
# current_exe, `reset-now --plan PLAN --enable`), and the factory's step 4 runs (deploy/factory/steps.sh:648),
# with current_exe = the PR's lamp-setup in /run/el-8671 and PLAN = the PR's plan there:
#   systemd-run --no-block --collect --unit=lamp-setup-wipe --property=Type=oneshot \
#     /run/el-8671/lamp-setup reset-now --plan /run/el-8671/reset-plan.json --enable
# Before it, in this order: every seeded marker read present; the dead-man STOPPED (past this point the lamp stays
# reset; nothing is put back); the reset's start time and boot id kept; the recorder (after.sh) started as its
# own transient unit and ready (its journal follower catches the reset's lines before the reset's vacuum).
# reset-now says "Factory reset starting." through the lamp's own say service (the product's line).
# The lamp then stops lampos.target, wipes, and starts setup again in place (no reboot) or reboots if that fails.
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ "${CONFIRM_RESET:-}" = "$LAMP" ] || { echo "REFUSE: CONFIRM_RESET is not $LAMP: no reset"; exit 1; }
[ -e $K/installed.txt ] && [ -s $K/seeded.tsv ] || { echo "REFUSE: not installed and seeded"; exit 1; }
[ -s $K/reset-start.txt ] && { echo "REFUSE: already triggered ($K/reset-start.txt)"; exit 1; }
[ -e "$OURS" ] && { echo "REFUSE: leg 2b's drop-in $OURS is still there"; exit 1; }
[ "$(sha_of /proc/$(mainpid)/exe)" = "$(sha_of $VSTOCK)" ] || { echo "REFUSE: the running voice is not the stock one"; exit 1; }
[ -e $MARKER ] || { echo "REFUSE: no setup marker: the lamp is in setup already"; exit 1; }
[ -e $RJOURNAL ] && { echo "REFUSE: a reset journal exists"; exit 1; }
systemctl is-active --quiet lampos.target || { echo "REFUSE: lampos.target is not active"; exit 1; }
systemctl list-units --all --no-legend "$WIPE_UNIT*" "$RECORDER*" 2>/dev/null | grep -q . && { echo "REFUSE: a $WIPE_UNIT or $RECORDER unit exists"; exit 1; }
[ "$(sha_of $BIN)" = "$(awk '$2=="lamp-setup"{print $1}' "$PAYLOAD/SHA256SUMS")" ] || { echo "REFUSE: $BIN is not the PR's"; exit 1; }
[ "$(sha_of $PLAN)" = "$(awk '$2=="reset-plan.json"{print $1}' "$PAYLOAD/SHA256SUMS")" ] || { echo "REFUSE: $PLAN is not the PR's"; exit 1; }
"$BIN" check --plan $PLAN >/dev/null 2>&1 || { echo "REFUSE: the PR's lamp-setup refuses the PR's plan"; exit 1; }
# -- every marker present now (the stock voice's clip sweep could have taken the old-path probe)
n=0; p=0; while IFS="$(printf '\t')" read -r f kind made; do n=$((n+1))
  if [ -f "$f" ] && [ ! -L "$f" ]; then p=$((p+1)); echo "PRESENT $kind $f"; else echo "MISSING $kind $f"; fi
done < $K/seeded.tsv
echo "PRESENT_N=$p SEEDED_N=$n"
awk -F'\t' '$2!="clips_old"' $K/seeded.tsv | grep -q . || { echo "REFUSE: nothing the PR claims was seeded"; exit 1; }
awk -F'\t' '$2!="clips_old"{print $1}' $K/seeded.tsv | while read -r f; do [ -f "$f" ] || echo x; done | grep -q x \
  && { echo "REFUSE: a marker of the PR's paths is missing before the reset"; exit 1; }
logcount 0 > $K/pretrigger-counts.txt; sed 's/^/PRETRIGGER /' $K/pretrigger-counts.txt
# -- the dead-man stopped: from here the lamp stays reset
systemctl stop $DEADMAN.timer 2>/dev/null; systemctl stop $DEADMAN.service 2>/dev/null
systemctl reset-failed $DEADMAN.service $DEADMAN.timer 2>/dev/null
systemctl list-units --all --no-legend "$DEADMAN*" 2>/dev/null | grep -q . && { echo "REFUSE: the dead-man would not stop: no reset"; exit 1; }
echo "dead-man stopped"
# -- the start, the recorder
mkdir -p $AFTER; chmod 700 $AFTER; rm -f $AFTER/ready
t=$(wall_us); { echo "RESET_WALL_US=$t"; echo "BOOT_ID=$(boot_id)"; } > $K/reset-start.txt
systemd-run --quiet --unit=$RECORDER --collect --property=RuntimeMaxSec=$((AFTER_WAIT_S + 900)) \
  --setenv=AFTER_WAIT_S=$AFTER_WAIT_S /bin/sh "$KIT/after.sh" || { mv $K/reset-start.txt $K/reset-start-void.txt; echo "REFUSE: the recorder did not start: no reset (put back)"; exit 1; }
i=0; until [ -e $AFTER/ready ]; do i=$((i+1)); [ $i -gt 150 ] && break; sleep 0.1; done
if [ ! -e $AFTER/ready ]; then
  systemctl stop $RECORDER 2>/dev/null; mv $K/reset-start.txt $K/reset-start-void.txt
  echo "REFUSE: the recorder is not ready in 15 s: no reset (put back)"; exit 1; fi
# -- the plan's own detach, read from the PR's plan, then current_exe and the args
set -- $(python3 -I -c 'import json,sys; print(" ".join(json.load(open(sys.argv[1]))["detach"]))' $PLAN)
[ "$*" = "/usr/bin/systemd-run --no-block --collect --unit=$WIPE_UNIT --property=Type=oneshot" ] \
  || { systemctl stop $RECORDER 2>/dev/null; mv $K/reset-start.txt $K/reset-start-void.txt; echo "REFUSE: the plan's detach is '$*'"; exit 1; }
echo "RESET_CMD=$* $BIN reset-now --plan $PLAN --enable"
"$@" "$BIN" reset-now --plan $PLAN --enable; r=$?
echo "SYSTEMD_RUN_EXIT=$r" | tee -a $K/reset-start.txt
[ $r = 0 ] || { systemctl stop $RECORDER 2>/dev/null; mv $K/reset-start.txt $K/reset-start-void.txt
  echo "TRIGGER-FAILED: systemd-run exit $r, no reset started (put back)"; exit 1; }
echo "TRIGGERED RESET_WALL_US=$t"
