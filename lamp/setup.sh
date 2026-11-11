#!/bin/sh
# Kit 5194 setup (lampOS #5194, LAMP_OPENS_FURTHER played before/after). Run as root:
#   sh /run/kit5194/setup.sh INSTALLED_SEQUENCE
# It changes NO unit: it checks, records the before-state and starts the kit's recorders (transient
# units kit5194-*: bus taps of voice/said, voice/heard, voice/state, room/state, voice/mute, and a
# lamp-voice journal follower). The switch goes on later, by switch.sh, between the two passes.
# Refuses, having changed nothing but /run/kit5194/state and logs (removed again), when:
#   - the sums are wrong, or ._ files are in /run/kit5194;
#   - the sequence typed is not the installed one; lamp-update is busy for 20 min;
#   - the installed voice has no LAMP_OPENS_FURTHER (built before #5235, 29cc36da91, train 678);
#   - lamp-voice is not active, has someone's /run drop-ins, or already runs with LAMP_OPENS_FURTHER;
#   - the voice is muted, or the lamp is asleep at start (expression/state asleep:true);
#   - there is no lamp-bus (the replies are read from voice/said); /run/kit5194/state exists.
set -eu
R0=/run/kit5194
. $R0/common.sh
CHANGED=0
stop() { echo "STOP: $*"; [ $CHANGED = 1 ] || rm -rf $S $L; exit 1; }
[ "$(id -u)" = 0 ] || { echo "STOP: not root"; exit 1; }
[ ! -e $S ] || { echo "STOP: $S exists (an earlier run not put back? run putback.sh first)"; exit 1; }
(cd $R && sha256sum -c --quiet SHA256SUMS) || stop "staged sums"
find $R -name '._*' | grep -q . && stop "._ files in $R (re-pack on the Mac with COPYFILE_DISABLE=1 tar --exclude='._*')"

# ---- the build: the operator confirms it by typing the sequence ----
NOW=$(head -c 300 /var/lib/lampos-update/state.json | tr -d ' \n' | grep -o '"installed":{"sequence":[0-9]*,"commit":"[0-9a-f]*"') || true
NOW_SEQ=$(echo "$NOW" | grep -o '"sequence":[0-9]*' | cut -d: -f2) || true
NOW_C=$(echo "$NOW" | grep -o '"commit":"[0-9a-f]*"' | cut -d'"' -f4) || true
echo "lamp installed: ${NOW_SEQ:-?} ${NOW_C:-?}"
[ -n "$NOW_SEQ" ] || stop "can't read the installed build from state.json"
[ "${1:-}" = "$NOW_SEQ" ] || stop "run as: sh $R/setup.sh $NOW_SEQ (after RUN.md step 0's git check that $NOW_C carries $MERGE)"
wait_upd setup || stop "lamp-update busy for 20 min"

# ---- the voice as found ----
systemctl is-active -q lamp-voice || stop "lamp-voice is not active"
[ -d $DDIR ] && [ -n "$(ls -A $DDIR)" ] && stop "someone's /run drop-ins on lamp-voice: $(ls $DDIR)"
P=$(voice_pid)
[ "${P:-0}" -gt 0 ] || stop "lamp-voice has no main process"
EXE=$(readlink /proc/$P/exe)
DIR=$(dirname "$EXE")
# The switch's name and the worker's own line, both from #5235, in one of the voice's binaries.
HAS=$(grep -l -a -F "$SW" "$DIR"/* 2>/dev/null | head -1) || true
[ -n "$HAS" ] || stop "the installed voice ($EXE) has no $SW: it is older than #5235 ($MERGE, train 678)"
grep -q -a -F 'its answer may end with one short opening (LAMP_OPENS_FURTHER)' "$HAS" \
  || stop "$HAS names $SW but has no 'may end with one short opening' line: not #5235's build?"
echo "voice: $EXE sha256 $(sha256sum "$EXE" | cut -c1-16); $SW found in $HAS"
envof $P | grep -q "$SW=" && stop "lamp-voice already runs with $(envof $P | grep -o "$SW=[a-z]*"): the off pass would not be stock"
[ -n "$BUSBIN" ] || stop "no lamp-bus on this lamp (the kit reads the replies from voice/said)"
M=$(mute_now)
case "$M" in *'"muted":false'*) ;; *) stop "the voice is muted or unread: ${M:-?}";; esac
A=$(asleep_now)
case "$A" in *true*) stop "asleep at start ($A): play in the day, with the lamp awake";; esac
VR=$(ready_current)

# ---- the before-state, compared SAME/DIFF by putback.sh ----
mkdir -p $S $L
echo "$BUSBIN" > $S/busbin
head -c 300 /var/lib/lampos-update/state.json > $S/state-before.json
echo "$NOW_SEQ $NOW_C" > $S/build.txt
echo "$EXE" > $S/voice-exe.txt
sha256sum "$EXE" | cut -d' ' -f1 > $S/voice-sha.txt
tr '\0' ' ' < /proc/$P/cmdline > $S/argv-before.txt
envof $P > $S/env-before.txt
systemctl show -p DropInPaths --value lamp-voice > $S/dropinpaths-before.txt
dropin_sums > $S/dropins-before.txt
links_now > $S/links.txt
ls /run/systemd/system/ | sort > $S/run-units.txt
md5sum /etc/systemd/system/lamp-*.service.d/* 2>/dev/null > $S/etc-dropins.md5 || true
systemctl list-units --type=service --state=active --no-legend --plain 'lamp-*' | awk '{print $1}' \
  | grep -v -E '@|^lamp-(update|update-back|update-apply|records|selftest)\.service$' | sort > $S/units.txt
systemctl list-units --state=failed --no-legend --plain 'lamp-*' | awk '{print $1}' | sort > $S/failed-before.txt
echo "$M" > $S/mute-before.txt
echo "${VR:-0}" > $S/ready-before.txt
echo "$P" > $S/voice-pid.txt
echo "voice env before: $(grep -o -E 'LAMP_[A-Z_]+=[^ ]*' $S/env-before.txt | wc -l) LAMP_ settings, $SW unset"
echo "voice drop-ins before: $(cat $S/dropinpaths-before.txt)"

# ---- the recorders: transient units, so they outlive this ssh; stopped by putback.sh ----
CHANGED=1
trap 'echo "setup failed: putting back"; sh $R/putback.sh setup-failed' EXIT
tap() { systemd-run --unit=$ON-$1 --collect sh -c "$BUS tap --name $3 $2 2>&1 | awk -f $R/chg.awk >> $L/$4"; }
tap said voice/said k94_said said.txt
tap heard voice/heard k94_heard heard.txt
tap vstate voice/state k94_vstate vstate.txt
tap room room/state k94_room room.txt
tap mute voice/mute k94_mute mute.txt
systemd-run --unit=$ON-follow --collect sh -c "journalctl -f -n0 -o short-iso-precise -u lamp-voice --no-pager | grep --line-buffered -v -E 'ElevenLabs|tokens|first piece|kept ready|opened ahead|frames, looked' >> $L/journal.txt"
# every 60 s: mute, voice state, build links, self.json, and whether the voice's process changed
systemd-run --unit=$ON-checks --collect sh -c "while true; do d=\$(date -Iseconds); m=\$($BUS latest voice/mute 2>&1 | grep -o '\"muted\":[a-z]*'); v=\$($BUS latest voice/state 2>&1 | grep -o '\"state\":\"[a-z_]*\"'); l=\$(for x in /opt/lampOS/*/current; do echo \"\$x \$(readlink \$x)\"; done); [ \"\$l\" = \"\$(cat $S/links.txt)\" ] || echo \"\$d BUILD MOVED\" >> $L/build-moved.txt; [ -e /var/lib/lampos-update/self.json ] && echo \"\$d self.json present\" >> $L/build-moved.txt; vp=\$(systemctl show -p MainPID --value lamp-voice); [ \"\$vp\" = \"\$(cat $S/voice-pid.txt)\" ] || echo \"\$d VOICE PROCESS CHANGED \$(cat $S/voice-pid.txt) -> \$vp\" >> $L/voice-changed.txt; echo \"\$d \$m \$v\" >> $L/checks.txt; sleep 60; done"
sleep 4
for u in said heard vstate room mute follow checks; do systemctl is-active -q $ON-$u || stop "recorder $ON-$u not running"; done
[ -s $L/vstate.txt ] || { sleep 4; [ -s $L/vstate.txt ] || stop "voice/state tap wrote nothing (is the voice on the bus? LAMP_BUS=on)"; }
trap - EXIT
date -Iseconds > $S/setup.done
echo "bus tool: $BUSBIN"
echo "voice: $(mute_now) $(vstate_now); expression $(asleep_now); listening_ready in its current run: ${VR:-0}x"
echo "lamp clock: $(date +%s.%N)"
echo "setup done $(date -Iseconds). The switch is OFF (stock). Next: the Mac's driver.zsh."
