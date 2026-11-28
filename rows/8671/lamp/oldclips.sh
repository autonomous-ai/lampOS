#!/bin/sh
# Run as root on lamp-75c9 after leg1.sh and BEFORE seed.sh: sh oldclips.sh. Leg 2b (the first builder's):
#   1. kit clip files (made-up <store_id>-<id>.wav/.json names, a store id no lamp has, the kit's fixed text, no
#      person data) put in the OLD folder /var/lib/lampOS/voice/ring-clips (3), nothing in the new folder;
#      refuses if the old folder holds files that are not the kit's (the PR's voice removes them for good) unless
#      CLEAR_OK=1;
#   2. lamp-voice on the PR's CI build through the kit's /run drop-in (the stock unit's own args,
#      LAMP_OFFLINE_RINGS=off: no consumer renders), ONE restart;
#   3. from the journal since that restart: `lamp-voice: the old offline ring clips folder was removed (N files)`
#      with N, and no "was left" line; the old folder gone. The NEW folder's entry count before and after is only
#      recorded: with the switch off, #8933 (in the PR's base) clears it at the consumer's start by design;
#   4. the drop-in out, ONE restart back onto the stock voice, its exe sha and NRestarts read back.
# Names and sizes only; no clip is ever opened. Never touches lamp-update, Wi-Fi or any other unit.
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ -e $K/installed.txt ] || { echo "REFUSE: not installed"; exit 1; }
[ -s $K/seeded.tsv ] && { echo "REFUSE: already seeded: leg 2b runs before the seed (the PR's voice with the switch off clears the new folder)"; exit 1; }
[ -e "$OURS" ] && { echo "REFUSE: $OURS already there"; exit 1; }
[ -x $VBIN ] || { echo "REFUSE: no $VBIN"; exit 1; }
p=$(mainpid); [ "$(sha_of /proc/$p/exe)" = "$(sha_of $VSTOCK)" ] || { echo "REFUSE: the running voice is not the stock one"; exit 1; }
want=$(awk '$2=="lamp-voice"{print $1}' "$PAYLOAD/SHA256SUMS"); [ "$(sha_of $VBIN)" = "$want" ] || { echo "REFUSE: $VBIN is not the PR's"; exit 1; }
other=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 ! -name "$KSTORE-*" 2>/dev/null | wc -l | tr -d ' ')
echo "OLD_OTHER_BEFORE=$other"
[ "$other" -gt 0 ] && [ "${CLEAR_OK:-0}" != 1 ] && { echo "REFUSE: $other entr(ies) in $CLIPS_OLD are not the kit's: the PR's voice removes them for good (CLEAR_OK=1 to accept)"; exit 1; }
nr0=$(nrestarts); echo "NRESTARTS_BEFORE=$nr0"
# -- 1. the kit's clip files
mk() { printf '%s\n' "$MTEXT" > "$1.new" && chmod 600 "$1.new" && mv "$1.new" "$1"; }
[ -d $CLIPS_OLD ] || { mkdir -m 700 $CLIPS_OLD && touch $K/olddir-made && echo "OLD_DIR_MADE=yes"; }
: > $K/oldclips.tsv
for n in 900001.wav 900001.json 900002.wav; do mk "$CLIPS_OLD/$KSTORE-$n" && echo "$CLIPS_OLD/$KSTORE-$n" >> $K/oldclips.tsv; done
echo "KIT_OLD_FILES=$(wc -l < $K/oldclips.tsv | tr -d ' ')"
echo "NEW_ENTRIES_BEFORE=$(find $CLIPS_NEW -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' ')"
# -- 2. the PR's voice, one restart
unit_exec_of $UNIT > $K/before-exec.txt
write_dropin || { voice_to_stock; echo "LEG2B-FAILED: the drop-in"; exit 1; }
sed 's/^/DROPIN| /' "$OURS"
echo "OLD_FILES_BEFORE=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 \( -type f -o -type l \) 2>/dev/null | wc -l | tr -d ' ') OLD_KIT_PRESENT=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 -name "$KSTORE-*" -type f 2>/dev/null | wc -l | tr -d ' ')"
t=$(wall_us); echo "RESTART_WALL_US=$t"
systemctl restart $UNIT || echo "RESTART: systemctl restart returned $?"
sh "$KIT/fresh-wait.sh" || echo "RESTART: not listening_ready"
sleep 3
p=$(mainpid); echo "VOICE_PID=$p EXE=$(readlink /proc/$p/exe) SHA_OK=$([ "$(sha_of /proc/$p/exe)" = "$want" ] && echo yes || echo no) SWITCH=$(env_of $p LAMP_OFFLINE_RINGS)"
j=$(journalctl -u $UNIT --since "@$(( t / 1000000 ))" -o cat --no-pager)
echo "OLD_REMOVED_N=$(printf '%s\n' "$j" | grep -cE "$OLD_REMOVED_RE") OLD_LEFT_N=$(printf '%s\n' "$j" | grep -cF "$OLD_LEFT")"
printf '%s\n' "$j" | grep -E "$OLD_REMOVED_RE" | sed -E "s/$OLD_REMOVED_RE/OLD_REMOVED_COUNT=\\1/"
echo "NEW_CLEARED_N=$(printf '%s\n' "$j" | grep -cF 'the offline ring clips were cleared (')"
printf '%s\n' "$j" | grep -qF "$MUTE" && echo "MUTED=yes" || echo "MUTED=no"
# -- 3. the folders
echo "OLD_AFTER=$(kind_of $CLIPS_OLD)"
echo "NEW_ENTRIES_AFTER=$(find $CLIPS_NEW -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' ') (information only: #8933 clears it with the switch off)"
# -- 4. the stock voice back
if voice_to_stock; then echo "STOCK_BACK=yes"; else echo "STOCK_BACK=no"; fi
p=$(mainpid); echo "STOCK_EXE=$(readlink /proc/$p/exe) NRESTARTS_AFTER=$(nrestarts)"
journalctl -u $UNIT --since "$(systemctl show -p ActiveEnterTimestamp --value $UNIT)" -o cat --no-pager | grep -qF "$MUTE" && echo "STOCK_MUTED=yes" || echo "STOCK_MUTED=no"
echo LEG2B-DONE
