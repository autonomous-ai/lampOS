#!/bin/bash
# Mac. Leg 7170 (#7170: after a boot, OpenClaw's gateway answers on 127.0.0.1:18789 long before it serves its RPCs; a
# run handed over in that window was taken but never ran). Run by ../run.sh's `boot` step, after leg D (its made `task`
# skill is the run asked here: a task step goes through the gateway and its launcher, so its script start is journaled).
#   1. ONE normal `systemctl reboot` (no hand step; GATEd first), ssh back with a new boot id within BOOTWAIT (300) s.
#   2. The mute after the reboot is read and reported. It is lifted again ONLY by the same yes path as micon (run.sh's
#      MIC_YES=yes and this ROW's micon MUTE LIFTED OK), by the same micon.lamp.sh; else the leg is VOID.
#   3. Poll (lamp side) until the gateway's port 18789 is listening (PORTWAIT, 300 s); within 30 s of that, MAILASK (or WEEKASK, leg A's task)
#      asks D's task skill to run, by the voice (the kit's run-asking way: a clip).
#   4. The run's script start: "[lamp-tasks] skill start: run R's script started" (skill_start.rs) within RUNWAIT7170
#      (420 s: lamp-tasks waits up to 240 s for the gateway to serve, gateway::SERVING_WITHIN).
#   5. The gateway's start, from this boot: unit start -> port listening -> approvals read answering (lamp-tasks'
#      own `openclaw approvals get --gateway` succeeded: journaled by the allow-list add that follows it at once,
#      gateway.rs Serving::ready_until). logs/boot-times.txt.
# CHECK (7170-1..4) and LEG 7170 PASS|FAIL|VOID. Exit 0 on PASS.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh
LG=${ROWLOGS:-$D/logs}; mkdir -p $LG; J=$LG/leg-7170.journal.txt; LEG=7170; JP=
POLL=${POLL:-1}; BOOTWAIT=${BOOTWAIT:-300}; PORTWAIT=${PORTWAIT:-300}; ASKWITHIN=${ASKWITHIN:-30}; RUNWAIT7170=${RUNWAIT7170:-420}
trap '[ -n "$JP" ] && kill $JP 2>/dev/null; RECSTOP' EXIT; trap 'exit 130' INT TERM HUP
c1=FAIL c2=FAIL c3=FAIL c4=FAIL
out(){ echo "CHECK (7170-1) ssh back after one reboot: $c1"; echo "CHECK (7170-2) the gateway's port listening within $PORTWAIT s: $c2"
  echo "CHECK (7170-3) the run asked within $ASKWITHIN s of the port: $c3"; echo "CHECK (7170-4) the run's script started: $c4"
  echo "LEG 7170 $1 ($2)"; [ "$1" = PASS ]; exit $?; }
lines(){ wc -l < $J | tr -d ' '; }
waitfor(){ local s=$(date +%s) r=$(date +%s) n lim=$3
  [ -n "${WAITMAX:-}" ] && [ "$lim" -gt "$WAITMAX" ] && lim=$WAITMAX     # WAITMAX: mock runs only
  while [ $(( $(date +%s) - s )) -lt $lim ]; do
    n=$(tail -n +$(( $2 + 1 )) $J | grep -nE -- "$1" | head -1 | cut -d: -f1); [ -n "$n" ] && { echo $(( n + $2 )); return 0; }
    [ $(( $(date +%s) - r )) -ge 120 ] && { RENEW || return 2; r=$(date +%s); }
    sleep $POLL; done; return 1; }
show(){ sed -n "${1}p" $J | cut -c1-240; }
echo "== leg 7170 $(date +%T) on $LAMP"
RENEW || exit 1; VOLOK || exit 2
# The run asked: leg D's mail task, or, where D was VOID (no linked mail), leg A's made skill when it is a task
# step (it needs no service). Its own clip asks it. Neither: VOID.
TS=; CLIP=
for pick in D:MAILASK A:WEEKASK; do
  TS=$(grep '^MADE ' $LG/made-${pick%%:*}.txt 2>/dev/null | grep ' kind=task ' | head -1 | awk '{print $2}')
  [ -n "$TS" ] && { CLIP=${pick#*:}; FROM=${pick%%:*}; break; }
done
[ -n "$TS" ] || out VOID "no task skill made by leg D or leg A (logs/made-D.txt, logs/made-A.txt): run the legs first"
[ -f $A/$CLIP-el.wav ] || out VOID "no clip $A/$CLIP-el.wav (../run.sh render)"
echo "  the run asked: $TS (leg $FROM's task skill), by $CLIP"
GATE lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway || out VOID "start budget before the reboot (not run)"
LAMP "MODE=before" $D/boot.lamp.sh > $LG/boot-before.txt 2>&1; sed 's/^/  before /' $LG/boot-before.txt | cut -c1-200
B0=$(sed -n 's/^BOOTID //p' $LG/boot-before.txt)
[ -n "$B0" ] || out VOID "the lamp's boot id not read before the reboot (logs/boot-before.txt)"
LAMP "MODE=reboot" $D/boot.lamp.sh > $LG/boot-reboot.txt 2>&1; cat $LG/boot-reboot.txt | sed 's/^/  /'
grep -q '^REBOOT ASKED' $LG/boot-reboot.txt || out VOID "reboot not asked (logs/boot-reboot.txt)"
T0=$(date +%s); back=
while [ $(( $(date +%s) - T0 )) -lt $BOOTWAIT ]; do sleep 5
  b=$($SSH -o ConnectTimeout=5 -o ConnectionAttempts=1 -n $H 'cat /proc/sys/kernel/random/boot_id' 2>/dev/null)
  [ -n "$b" ] && [ "$b" != "$B0" ] && { back=$b; break; }
  RENEW || out VOID "lock lost while the lamp rebooted"; done
[ -n "$back" ] || out FAIL "ssh not back with a new boot in $BOOTWAIT s: STOP, look at the lamp (nothing else runs)"
c1=PASS; echo "  ssh back on a new boot after $(( $(date +%s) - T0 )) s"
RENEW || exit 1
LAMP "MODE=after" $D/boot.lamp.sh > $LG/boot-after.txt 2>&1; sed 's/^/  after /' $LG/boot-after.txt | cut -c1-200
grep -q '^CLOUD active' $LG/boot-after.txt || echo "  NOTE lamp-cloud is not active after the boot ($(sed -n 's/^CLOUD //p' $LG/boot-after.txt)); the kit never stops it"
if grep -q '^MUTEBOOT muted=True' $LG/boot-after.txt; then
  echo "  MUTE after the reboot: muted"
  if [ "${MIC_YES:-}" = yes ] && grep -q '^MUTE LIFTED OK' $D/logs/micon.txt 2>/dev/null; then
    GATE lamp-voice || out VOID "muted after the reboot; no start budget to lift it again"
    LAMP "" $D/micon.lamp.sh > $LG/micon-boot.txt 2>&1; grep -E '^(MUTE|RECORD_KEPT)' $LG/micon-boot.txt | sed 's/^/  /'
    grep -qE '^MUTE (LIFTED OK|NOT MUTED)' $LG/micon-boot.txt || out VOID "muted after the reboot and not lifted again (logs/micon-boot.txt)"
    grep -q '^MUTE LIFTED OK' $LG/micon-boot.txt && echo "MUTE LIFTED OK (after the reboot, the same yes)" >> $D/logs/micon.txt
  else out VOID "muted after the reboot; not lifted: only on the same yes as micon (MIC_YES=yes and this ROW's MUTE LIFTED OK)"; fi
else echo "  MUTE after the reboot: $(sed -n 's/^MUTEBOOT //p' $LG/boot-after.txt)"; fi
: > $J; JP=$(JF $J); SK_=$(SKEW | awk '{print $1}'); echo "  skew lamp-Mac ${SK_} s"; RECSTART 7170 $LG/leg-7170.mac.wav
LAMP "MODE=port PORTWAIT=$PORTWAIT" $D/boot.lamp.sh > $LG/boot-port.txt 2>&1; cat $LG/boot-port.txt | sed 's/^/  /'
grep -q '^PORT listening' $LG/boot-port.txt || out FAIL "the gateway's port 18789 not listening within $PORTWAIT s of ssh: $(grep '^PORT' $LG/boot-port.txt | cut -c1-120)"
c2=PASS; PUP=$(sed -n 's/^PORT listening up=\([0-9.]*\) .*/\1/p' $LG/boot-port.txt); PEP=$(sed -n 's/^PORT listening .*epoch=\([0-9.]*\).*/\1/p' $LG/boot-port.txt)
n0=$(lines); PLAY $CLIP
dt=$(python3 -c "import sys;print('%.1f'%(float(sys.argv[1])+float(sys.argv[2])-float(sys.argv[3])))" $PEND ${SK_:-0} $PEP)
echo "  $CLIP ended $dt s after the port listened (limit $ASKWITHIN)"
python3 -c "import sys;sys.exit(0 if float(sys.argv[1])<=float(sys.argv[2]) else 1)" $dt $ASKWITHIN && c3=PASS
OK="skill start: run [^ ]+'s script started"; BAD="hand-over is not sent|fence never said it started|Harness didn't start the run|openclaw_gateway_not_ready"
e=$(waitfor "$OK|$BAD" $n0 $RUNWAIT7170); [ -n "$e" ] && echo "  $(show $e)"
[ -n "$e" ] && sed -n "${e}p" $J | grep -qE "$OK" && c4=PASS
t=$(waitfor "skill task [^ ]+: ended" $n0 ${ENDWAIT:-120}); [ -n "$t" ] && echo "  $(show $t)"
sleep ${TAIL:-8}; kill $JP 2>/dev/null; JP=; RECSTOP
LAMP "MODE=times PORT_UP=${PUP:-none}" $D/boot.lamp.sh > $LG/boot-times.txt 2>&1; grep -E '^(STARTUP|J )' $LG/boot-times.txt | cut -c1-240 | sed 's/^/  /'
if [ $c3 != PASS ]; then out VOID "the run was asked $dt s after the port, past $ASKWITHIN s (not the window #7170 is about)"
elif [ $c4 = PASS ]; then out PASS "$TS's script started after a boot, asked $dt s after the port; $(grep -m1 '^STARTUP unit' $LG/boot-times.txt)"
else out FAIL "$TS's script did not start: $( [ -n "$e" ] && show $e | cut -c1-160 || echo "no script-start or not-sent line in $RUNWAIT7170 s")"; fi
