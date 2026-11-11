#!/bin/sh
# Kit 5194 put-back. Root:  sh /run/kit5194/putback.sh
# Waits for lamp-update to be quiet first (exit 2 = OWED, nothing changed); never stops, restarts,
# masks or disables lamp-update or lamp-update-back, never takes the updater's lock. Stops the kit's
# recorders, removes the drop-in, daemon-reloads, and restarts lamp-voice (stopped alone first, then
# started) only if the switch was on. Then SAME/DIFF: every file in the voice's drop-in folders
# byte-equal (sha256) to before, DropInPaths, the running binary and its sha256, argv, env, links,
# units, /run units, /etc drop-ins, state.json. Exit 3 = put back, but the VOICE IS MUTED.
# Leaves /run/kit5194/logs and state for the Mac to copy; remove /run/kit5194 after.
. /run/kit5194/common.sh
WHO=${1:-hand}
[ "$(id -u)" = 0 ] || { echo "not root"; exit 1; }
[ -d $S ] || { echo "nothing to put back: $S is gone"; exit 0; }
echo "putback ($WHO) $(date -Iseconds)"
wait_upd "put-back" || { echo "OWED: lamp-update still busy after 20 min, nothing put back; run again once it is quiet"; exit 2; }
for t in said heard vstate room mute follow checks; do systemctl stop $ON-$t 2>/dev/null; done
RESTART=no
if [ -e $DROPIN ]; then rm -f $DROPIN; RESTART=yes; echo "drop-in removed: $DROPIN"; else echo "drop-in: none to remove"; fi
grep -q "^nodir $DDIR" $S/dropins-before.txt 2>/dev/null && rmdir $DDIR 2>/dev/null
systemctl daemon-reload
P=$(voice_pid)
[ "${P:-0}" -gt 0 ] && tr '\0' '\n' < /proc/$P/environ 2>/dev/null | grep -q "^$SW=" && RESTART=yes
systemctl is-active -q lamp-voice || RESTART=yes
T0=$(date '+%Y-%m-%d %H:%M:%S')
if [ $RESTART = yes ]; then
  echo "restarting lamp-voice to stock (stopped alone first)"
  voice_restart || loud "lamp-voice did not stop cleanly: watch for the restart guard's mute below"
fi

s() { if [ "$2" = "$3" ]; then echo "SAME $1"; else echo "DIFF $1: [$2] vs [$3]"; DIFFS="$DIFFS; $1"; fi; }
DIFFS=""
P=$(voice_pid)
s "lamp-voice active" active "$(systemctl is-active lamp-voice)"
s "voice drop-in files (sha256, byte-equal)" "$(cat $S/dropins-before.txt)" "$(dropin_sums)"
s "voice DropInPaths" "$(cat $S/dropinpaths-before.txt)" "$(systemctl show -p DropInPaths --value lamp-voice)"
s "kit drop-in gone" absent "$([ -e $DROPIN ] && echo present || echo absent)"
s "voice binary path" "$(cat $S/voice-exe.txt)" "$(readlink /proc/$P/exe 2>/dev/null)"
s "voice binary sha256" "$(cat $S/voice-sha.txt)" "$(sha256sum /proc/$P/exe 2>/dev/null | cut -d' ' -f1)"
s "voice argv" "$(mask < $S/argv-before.txt)" "$(tr '\0' ' ' < /proc/$P/cmdline 2>/dev/null | mask)"
s "voice env" "$(cat $S/env-before.txt)" "$(envof $P 2>/dev/null)"
G=$(envof $P 2>/dev/null | grep -o "$SW=[a-z]*" | head -1); s "$SW gone from the voice" none "${G:-none}"
s "current links" "$(cat $S/links.txt)" "$(links_now)"
s "/run units" "$(cat $S/run-units.txt)" "$(ls /run/systemd/system/ | sort)"
s "/etc drop-ins" "$(cat $S/etc-dropins.md5)" "$(md5sum /etc/systemd/system/lamp-*.service.d/* 2>/dev/null)"
s "active lamp-* units" "$(cat $S/units.txt)" "$(systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort)"
s "failed lamp-* units" "$(cat $S/failed-before.txt)" "$(systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort)"
G=$(systemctl list-units --no-legend --plain "$ON-*" 2>/dev/null | awk '{print $1}' | tr '\n' ' '); G=${G% }; s "kit recorders stopped" none "${G:-none}"
[ -s $L/build-moved.txt ] && echo "BUILD MOVED during the run: $(head -2 $L/build-moved.txt | tr '\n' ' ')"
[ -s $L/voice-changed.txt ] && echo "NOTE voice process changed during the run (the switch's restart is one): $(head -3 $L/voice-changed.txt | tr '\n' ' ')"
echo "UPDATER self.json: $([ -e /var/lib/lampos-update/self.json ] && echo present || echo none); state.json $( [ "$(head -c 300 /var/lib/lampos-update/state.json)" = "$(cat $S/state-before.json)" ] && echo unchanged || echo CHANGED); lamp-update $(systemctl is-active lamp-update); lamp-update-back $(systemctl is-active lamp-update-back)"

# ---- the voice: listening_ready, the restart guard, voice/mute ----
if [ $RESTART = yes ]; then
  ready_since "$T0" && echo "SAME voice listening_ready (since the restart)" || { echo "DIFF voice: no listening_ready 90 s after the restart"; DIFFS="$DIFFS; listening_ready"; }
  if guard_muted "$T0"; then
    # #4332: a clean, asked-for stop ends a mute the restart guard restored. One try only;
    # a person's mute is never touched (muted_by must be restart_guard).
    loud "voice came back MUTED BY THE RESTART GUARD: one clean restart of lamp-voice to clear it (#4332)"
    T1=$(date '+%Y-%m-%d %H:%M:%S')
    voice_restart
    journalctl -u lamp-voice --since "$T1" --no-pager | grep -q 'ended the mute the restart guard restored' && echo "voice: the clean stop ended the guard's mute"
    ready_since "$T1" && echo "SAME voice listening_ready (after the retry)" || echo "DIFF voice: no listening_ready after the retry"
  fi
else
  RC=$(ready_current)
  [ "${RC:-0}" -gt 0 ] && echo "SAME voice listening_ready (its current run said it ${RC}x)" \
    || echo "NOTE voice listening_ready not in the journal of its current run (setup counted $(cat $S/ready-before.txt)); voice/state $(vstate_now)"
fi
m=$(mute_now)
s "voice/mute" '"muted":false' "$(echo "$m" | grep -o '"muted":[a-z]*')"
echo "voice/state: $(vstate_now); expression: $(asleep_now)"
echo "._ files: $(find $R /var/lib/lampOS /root/local -name '._*' 2>/dev/null | wc -l)"
echo "voice drop-ins now: $(systemctl show -p DropInPaths --value lamp-voice)"
date -Iseconds > $S/putback.done
if [ -z "$DIFFS" ]; then echo "PUTBACK-OK: drop-in gone, voice on its stock binary and env, drop-in files byte-equal"
else echo "PUTBACK-DIFF:${DIFFS#;}"; fi
echo "putback done $(date -Iseconds)"
# ---- the last step: never leave the lamp deaf without saying so ----
if echo "$m" | grep -q '"muted":false'; then
  echo "VOICE OK: not muted, listening."
else
  loud "THE VOICE IS MUTED after put-back ($m). If muted_by is restart_guard, the lamp is deaf until a head tap: tell the person NOW and post it with released."
  exit 3
fi
