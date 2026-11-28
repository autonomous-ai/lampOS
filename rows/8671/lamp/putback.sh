#!/bin/sh
# Run as root on lamp-75c9: sh putback.sh [deadman]. ../run.sh putback/after runs it (also run.sh's trap on ^C or an
# error), AFTER the Mac has copied the journal window and the kit's files off the lamp. Idempotent; one at a time.
# Two cases, by $K/reset-start.txt:
#   Both: leg 2b's voice drop-in removed and the stock voice back (one restart) if the PR's voice runs; leg 2b's
#     old-folder kit files removed by path ($K/oldclips.tsv).
#   NOT reset (stopped before leg 3, or the dead-man): the dead-man stopped (not when it is the one running); every
#     seeded marker removed BY ITS PATH in $K/seeded.tsv, only if it is a regular file holding the kit's own text;
#     the folders the kit made removed if empty; the tmpfs unmounted. The lamp is as before the row.
#   RESET (the row's point: it STAYS reset, nothing is put back, nothing onboarded): the recorder stopped unless
#     KEEP_RECORDER=1 (it then ends by itself after the re-pair or AFTER_WAIT_S); the tmpfs unmounted if still there;
#     any marker the reset left (the old-path probe) removed by path as above; kit-made folders removed if empty;
#     $K/STATE.txt says the state the lamp is left in (setup mode, for the lead's re-pairing).
# Then read back twice. PUTBACK-OK / PUTBACK-FAIL. Never reboots, never touches lamp-update, Wi-Fi, NetworkManager
# or wpa; never opens a database file; no pattern kills (the follower is killed by its PID after a cmdline check).
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ -d $K ] || { echo "REFUSE: no $K: nothing of this kit to put back"; exit 1; }
exec 9> $K/putback.lock
flock -w 180 9 || { echo "REFUSE: another putback holds $K/putback.lock"; exit 1; }
reset=0; [ -s $K/reset-start.txt ] && reset=1
echo "== putback ${1:-mac} $(date '+%F %T') reset=$reset"
ok=1; bad() { ok=0; echo "FAIL: $*"; }
# -- the dead-man
if [ "${1:-}" != deadman ]; then
  systemctl stop $DEADMAN.timer 2>/dev/null && echo "dead-man timer stopped"
  systemctl stop $DEADMAN.service 2>/dev/null; systemctl reset-failed $DEADMAN.service $DEADMAN.timer 2>/dev/null
else echo "run by the dead-man"; fi
# -- the recorder (after a reset)
if [ $reset = 1 ]; then
  if [ "${KEEP_RECORDER:-0}" = 1 ] && systemctl is-active --quiet $RECORDER; then echo "recorder kept running (KEEP_RECORDER=1): it ends after the re-pair or AFTER_WAIT_S"
  else
    systemctl stop $RECORDER 2>/dev/null && echo "recorder stopped"; systemctl reset-failed $RECORDER 2>/dev/null
    q=$(cat $AFTER/follower.pid 2>/dev/null); [ -n "$q" ] && [ -d /proc/$q ] && tr '\0' ' ' < /proc/$q/cmdline | grep -q '^journalctl -f' && kill "$q"
    rm -f $AFTER/follower.pid
  fi
fi
# -- leg 2b's drop-in and the stock voice (also after a reset, should one be left)
if [ -e "$OURS" ] || [ "$(readlink /proc/$(mainpid)/exe)" = "$VBIN" ]; then
  voice_to_stock && echo "voice: drop-in removed, stock voice back" || bad "the voice is not back on the stock binary"
fi
# -- leg 2b's old-folder kit files, by path (the PR's voice removed them if it ran)
if [ -s $K/oldclips.tsv ]; then
  while read -r f; do
    case "$f" in $CLIPS_OLD/$KSTORE-*) ;; *) echo "skipped (not a kit name): $f"; continue;; esac
    [ -e "$f" ] || continue
    if [ -f "$f" ] && [ ! -L "$f" ] && [ "$(head -c 200 "$f")" = "$MTEXT" ]; then rm -f "$f" && echo "removed old-folder kit file $f"
    else bad "$f is not the kit's: left"; fi
  done < $K/oldclips.tsv
  [ -e $K/olddir-made ] && [ -d $CLIPS_OLD ] && rmdir $CLIPS_OLD 2>/dev/null && echo "removed kit-made $CLIPS_OLD"
  rm -f $K/olddir-made
fi
# -- the markers, by path
if [ -s $K/seeded.tsv ]; then
  while IFS="$(printf '\t')" read -r f kind made; do
    case "$f" in */$MNAME|$STRAY) ;; *) echo "skipped (not a kit name): $f"; continue;; esac
    [ -e "$f" ] || continue
    if [ -f "$f" ] && [ ! -L "$f" ] && [ "$(head -c 200 "$f")" = "$MTEXT" ]; then rm -f "$f" && echo "removed $kind $f"
    else bad "$f is not the kit's marker: left"; fi
  done < $K/seeded.tsv
  # the folders the kit made, deepest first, only if empty
  cut -f3 $K/seeded.tsv | tr ',' '\n' | grep '^/' | awk '{print length, $0}' | sort -rn | cut -d' ' -f2- | while read -r d; do
    [ -d "$d" ] && { rmdir "$d" 2>/dev/null && echo "removed kit folder $d" || echo "kept kit folder $d (not empty: the product wrote there)"; }
  done
fi
# -- the tmpfs
sleep 1
if grep -q " $BIN_DIR " /proc/mounts; then
  if [ $reset = 1 ] && systemctl is-active --quiet $WIPE_UNIT; then bad "the reset is still running: $BIN_DIR left mounted"
  else umount $BIN_DIR && echo "unmounted $BIN_DIR" || bad "$BIN_DIR cannot be unmounted"; fi
fi
rmdir $BIN_DIR 2>/dev/null
# -- the state left
if [ $reset = 1 ]; then
  { echo "# lamp-75c9 after #8671's row reset, $(date '+%F %T %Z'): left reset, NOT onboarded (the re-pairing is the lead's)"
    echo "setup marker: $( [ -e $MARKER ] && echo present || echo 'absent (setup mode)')"
    echo "reset journal: $( [ -e $RJOURNAL ] && echo 'present (a reset unfinished: resume-reset runs it at the next start)' || echo absent)"
    for u in lamp-setup lampos.target lamp-voice lamp-setup-reset nginx lamp-update $RECORDER; do echo "unit $u: $(systemctl is-active $u 2>/dev/null)"; done
    echo "wlan0: $(ip -br addr show wlan0 2>/dev/null | awk '{print $2, $3}')"
    echo "wpa_supplicant-wlan0.conf: $(kind_of /etc/wpa_supplicant/wpa_supplicant-wlan0.conf | cut -d' ' -f1)"
    echo "voice credential: $(kind_of /etc/lampOS/voice/credential | cut -d' ' -f1); backend.json: $(kind_of /etc/lampOS/setup/backend.json | cut -d' ' -f1)"
    echo "the stock lamp-setup and plan: unchanged ($(sha_of $STOCK_BIN | cut -c1-12), $(sha_of $STOCK_PLAN | cut -c1-12)); the PR's binary was only ever in /run"
    echo "kit files kept on the lamp: $K (no person data; the Mac has a copy)"; } > $K/STATE.txt
  cat $K/STATE.txt
fi
# -- read-back, twice
for r in 1 2; do
  left=$(while IFS="$(printf '\t')" read -r f kind made; do [ -e "$f" ] && echo "$f"; done < $K/seeded.tsv 2>/dev/null)
  left="$left$(cat $K/oldclips.tsv 2>/dev/null | while read -r f; do [ -e "$f" ] && echo " $f"; done)"
  [ -z "$left" ] && echo "read-back $r: no kit marker left" || bad "read-back $r: kit markers left: $left"
  [ -e "$OURS" ] && bad "read-back $r: $OURS still there" || echo "read-back $r: no voice drop-in"
  grep -q " $BIN_DIR " /proc/mounts && bad "read-back $r: $BIN_DIR still mounted" || echo "read-back $r: no kit tmpfs"
  if [ "${1:-}" != deadman ]; then
    systemctl list-units --all --no-legend "$DEADMAN*" 2>/dev/null | grep -q . && bad "read-back $r: a $DEADMAN unit is still there" || echo "read-back $r: dead-man gone"; fi
  if systemctl is-active --quiet $RECORDER; then
    [ "${KEEP_RECORDER:-0}" = 1 ] && echo "read-back $r: recorder running (kept)" || bad "read-back $r: the recorder still runs"
  else echo "read-back $r: no recorder"; fi
  [ $r = 1 ] && sleep 3
done
n=$(find $K -name '._*' 2>/dev/null | wc -l); echo "._ files: $n"; [ "$n" = 0 ] || bad "._ files"
if [ $ok = 1 ]; then
  [ -e $K/installed.txt ] && mv $K/installed.txt "$K/installed-done-$(date +%H%M).txt"
  # After a reset seeded.tsv stays: the recorder's later snapshots read it (and the lamp is never seeded again).
  [ $reset = 0 ] && [ -s $K/seeded.tsv ] && mv $K/seeded.tsv "$K/seeded-done-$(date +%H%M).tsv"
  echo PUTBACK-OK
else echo PUTBACK-FAIL; exit 1; fi
