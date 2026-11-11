#!/bin/sh
# Kit 5194: turn LAMP_OPENS_FURTHER on for lamp-voice, between the off pass and the on pass. Root:
#   sh /run/kit5194/switch.sh on
# A /run drop-in (Environment= only, gone at a reboot) and one restart of lamp-voice, stopped alone
# first (a clean SIGTERM releases the restart guard), then started. Checks the new voice has the
# switch, said listening_ready and is not muted. On any failure it puts back (putback.sh) and exits 1.
# Last line: SWITCH-ON-OK.
. /run/kit5194/common.sh
[ "$(id -u)" = 0 ] || { echo "not root"; exit 1; }
[ "${1:-}" = on ] || { echo "usage: sh $R/switch.sh on"; exit 1; }
[ -e $S/setup.done ] || { echo "STOP: setup.sh has not run"; exit 1; }
fail() { loud "SWITCH FAILED: $*"; echo "putting back"; sh $R/putback.sh switch-failed; exit 1; }
[ -e $DROPIN ] && { echo "the drop-in is already there: $DROPIN"; }
wait_upd "the switch" || { echo "STOP: lamp-update busy for 20 min; nothing changed (the on pass waits)"; exit 2; }
M=$(mute_now)
case "$M" in *'"muted":false'*) ;; *) echo "STOP: the voice is muted before the switch: ${M:-?}; nothing changed"; exit 1;; esac
mkdir -p $DDIR
printf '# kit5194 (lampOS #5194 played before/after): removed by /run/kit5194/putback.sh\n[Service]\nEnvironment=%s=on\n' "$SW" > $DROPIN
systemctl daemon-reload
T0=$(date '+%Y-%m-%d %H:%M:%S')
echo "$T0" > $S/switch-at.txt
voice_restart || { loud "lamp-voice did not stop cleanly: its restart guard may mute it"; fail "voice stop not clean"; }
systemctl is-active -q lamp-voice || fail "lamp-voice not active after the start"
P=$(voice_pid); echo "$P" > $S/voice-pid.txt
tr '\0' '\n' < /proc/$P/environ | grep -qx "$SW=on" || fail "the new voice lacks $SW=on"
[ "$(readlink /proc/$P/exe)" = "$(cat $S/voice-exe.txt)" ] || fail "the voice runs $(readlink /proc/$P/exe), not the stock $(cat $S/voice-exe.txt)"
ready_since "$T0" || fail "voice never said listening_ready"
guard_muted "$T0" && { loud "VOICE MUTED BY THE RESTART GUARD at the switch"; fail "voice muted by the restart guard"; }
mute_now | grep -q '"muted":false' || fail "voice muted after the restart: $(mute_now)"
# The second worker says the switch once it starts (worker.rs); it may start late: a note, not a stop.
i=0; until journalctl -u lamp-voice --since "$T0" --no-pager | grep -q 'may end with one short opening (LAMP_OPENS_FURTHER)'; do
  i=$((i+1)); [ $i -gt 10 ] && break; sleep 3; done
journalctl -u lamp-voice --since "$T0" --no-pager | grep -q 'may end with one short opening (LAMP_OPENS_FURTHER)' \
  && echo "voice second worker: says $SW is on" \
  || echo "WARN: the second worker hasn't said $SW yet (read the journal before the on pass's first line)"
echo "$(cat $DROPIN | tail -1) in $DROPIN"
echo "voice: pid $P $(mute_now) $(vstate_now); expression $(asleep_now)"
echo "SWITCH-ON-OK $(date -Iseconds)"
