#!/bin/sh
# Run as root on lamp-75c9 after leg1.sh and oldclips.sh (leg 2b), the stock voice back: sh seed.sh.
# Leg 2: harmless kit marker files (a fixed text, no person
# data), one name ($MNAME), in each place the reset must empty, listed in $K/seeded.tsv (path, kind, folders the
# kit made) for the judge and the put-back:
#   home       each engine's home folder in /var/lib/lampos-agents (a link never followed)
#   stray      /var/lib/lampos-agents/el8671-kit-stray: "anything else there is removed"
#   marked     /var/lib/lampOS/voice/marked/ (made 0700 if missing)
#   clips_new  the PR's ring clips folder, /var/lib/lampOS/voice/schedule-delivery/ring-clips (made 0700 if missing)
# The OLD clips folder (/var/lib/lampOS/voice/ring-clips) is leg 2b's (oldclips.sh): the PR's voice removes it.
# NOT seeded, only read (kind and size): history-marks.json and history-forget.json. The voice reads both
# (history_forget.rs:493 takes either file's presence as "history was sent"), so a kit file there could start an
# account delete; history-forget.json is also KEPT by the PR (#8125's fallback).
# Then the "before": the KEPT paths, the homes' owner:mode, the log files' inodes, the lamp-voice line counts.
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ -e $K/installed.txt ] || { echo "REFUSE: not installed"; exit 1; }
[ -s $K/seeded.tsv ] && { echo "REFUSE: $K/seeded.tsv exists: putback.sh first"; exit 1; }
# After leg 2b (oldclips.sh), with the stock voice back: its PR voice, switch off, clears the new clips folder (#8933)
[ -e "$OURS" ] && { echo "REFUSE: leg 2b's drop-in is still there: the new clips folder would be cleared"; exit 1; }
[ "$(sha_of /proc/$(mainpid)/exe)" = "$(sha_of $VSTOCK)" ] || { echo "REFUSE: the running voice is not the stock one"; exit 1; }
: > $K/seeded.tsv.new; bad=0
put() { # put FILE KIND MADE_DIRS
  if printf '%s\n' "$MTEXT" > "$1.new" && chmod 600 "$1.new" && mv "$1.new" "$1"; then
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> $K/seeded.tsv.new; echo "SEEDED $2 $1"
  else rm -f "$1.new"; echo "SEED_FAIL $2 $1"; bad=1; fi
}
mkd() { # mkd DIR: made 0700 if missing; prints the folders it made
  made=''; for d in $(dirname "$(dirname "$1")") $(dirname "$1") "$1"; do
    [ -d "$d" ] && continue
    [ -e "$d" ] && { echo "SEED_FAIL $d is not a folder" >&2; return 1; }
    mkdir -m 700 "$d" && made="$made${made:+,}$d" || return 1
  done; echo "${made:--}"
}
for h in $(homes); do put "$h/$MNAME" home -; done
[ -d $HOMES ] && { echo "HOMES_DIR=present"; put $STRAY stray -; } || echo "HOMES_DIR=absent"
m=$(mkd $MARKED) && put "$MARKED/$MNAME" marked "$m" || { echo "SEED_FAIL marked"; bad=1; }
m=$(mkd $CLIPS_NEW) && put "$CLIPS_NEW/$MNAME" clips_new "$m" || { echo "SEED_FAIL clips_new"; bad=1; }
mv $K/seeded.tsv.new $K/seeded.tsv
# -- the before
{ echo "SEED_WALL_US=$(wall_us)"
  for p in $KEPT_PATHS $MARKS $HFORGET $MARKED $CLIPS_NEW $CLIPS_OLD; do echo "KIND $p $(kind_of $p)"; done
  for h in $(homes); do echo "HOME $h $(kind_of $h)"; done
  echo "CLIPS_OLD_OTHER=$(find $CLIPS_OLD -mindepth 1 -maxdepth 1 ! -name "$MNAME" 2>/dev/null | wc -l | tr -d ' ')"
  log_inodes | sed 's/^/LOGFILE /'
  logcount 0 --live; } > $K/before.txt
cat $K/before.txt
echo "SEEDED_N=$(wc -l < $K/seeded.tsv | tr -d ' ')"
[ $bad = 0 ] && echo SEED-OK || { echo SEED-FAIL; exit 1; }
