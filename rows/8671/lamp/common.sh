# Shared by the #8671 row kit's lamp scripts (sourced, POSIX sh). #8671: a factory reset empties every engine's home
# (/var/lib/lampos-agents/<engine>), the voice's marked copies and sent turns' ids, and the text logs with their eMMC
# copy (/var/log.hdd). Kit structure from rows/8933. Lines read with git grep at the PR head cf7dacac6d
# (mac/strings.tsv; ./run.sh check greps them). Never opens a DB file; log lines are counted, never printed.
KIT=$(cd "$(dirname "$0")" 2>/dev/null && pwd); [ -f "$KIT/common.sh" ] || KIT=/root/el-8671/kit
K=/root/el-8671
AFTER=$K/after
LAMP=${EXPECT_HOST:-lamp-75c9}
HEAD=cf7dacac6dd32a1e791919300e19912470ce8c68
# The dead-man: a transient timer that runs putback.sh deadman if the Mac never comes back BEFORE the reset.
# trigger.sh stops it just before the reset: from there the lamp stays reset (the row's point).
DEADMAN=lamp8671-deadman
DEADMAN_S=${DEADMAN_S:-1800}
# The recorder: a transient unit (after.sh) started just before the reset; outside lampos.target and the reset's
# WRITERS/STARTERS, so the reset's stop leaves it running. Bounded by AFTER_WAIT_S and RuntimeMaxSec.
RECORDER=lamp8671-after
AFTER_WAIT_S=${AFTER_WAIT_S:-21600}
# The PR's lamp-setup and plan, from a tmpfs (not in reset::VOLATILE, so the in-place start leaves it).
BIN_DIR=/run/el-8671
BIN=$BIN_DIR/lamp-setup
PLAN=$BIN_DIR/reset-plan.json
PAYLOAD=$KIT/payload
STOCK_BIN=/opt/lampOS/setup/current/lamp-setup
STOCK_PLAN=/etc/lampOS/setup/reset-plan.json
WIPE_UNIT=lamp-setup-wipe
# The plan's own paths (fixtures/reset-plan-product.json at the PR head)
MARKER=/var/lib/lampos-setup/done
RJOURNAL=/var/lib/lampos-setup/reset.json
# engine_homes.rs ENGINE_HOMES = wrapper.rs:82 HOMES
HOMES=/var/lib/lampos-agents
# marked.rs DEFAULT_MARKS; history_send.rs:99 MARKS_FILE; history_forget.rs:67 FILE (KEPT by the PR: #8125's)
MARKED=/var/lib/lampOS/voice/marked
MARKS=/var/lib/lampOS/voice/history-marks.json
HFORGET=/var/lib/lampOS/voice/history-forget.json
# ring_clips.rs:55 DIR at the PR head (inside schedule-delivery, which the plan wipes); the stock build's DIR,
# dropped from the PR's plan and named nowhere at the PR head (the kit probes whether it is left behind)
CLIPS_NEW=/var/lib/lampOS/voice/schedule-delivery/ring-clips
CLIPS_OLD=/var/lib/lampOS/voice/ring-clips
# logs.rs LOG_FOLDERS, COPY_JOURNAL, TEXT_LOGS
LOGS_RAM=/var/log
LOGS_HDD=/var/log.hdd
COPY_JOURNAL=/var/log.hdd/journal
TEXT_LOGS="syslog user.log kern.log auth.log"
# What the reset keeps (tests/it/reset.rs KEPT_IN_CODE and KEPT at the PR head): existence only is read
KEPT_PATHS="/var/lib/lampOS/voice/day-tally /var/lib/lampOS/voice/language-said /var/lib/lampOS/voice/tts-cache
/var/lib/lampOS/voice/update-cue /var/lib/lampOS/voice/history-forget.json /var/lib/lampOS/voice/notices
/var/lib/lampOS/selftest /var/lib/lampos-hermes-gateway /var/lib/lampos-openclaw.old /var/lib/lamp-selftest
/var/lib/lampos-body /var/lib/lampos-agents /var/lib/lampOS/voice/schedule-delivery"
# The kit's marker: one name, a fixed text, no person data
MNAME=el8671-kit-marker
MTEXT='kit row 8671 marker file: no person data'
STRAY=$HOMES/el8671-kit-stray

# --- the lines (as rendered; mac/judge.py matches the same) ---
HOMES_LINE='lamp-setup: reset: emptied [0-9]+ entries of the engines'"'"' homes'
LOGS_LINE='lamp-setup: reset: emptied [0-9]+ log entries'
LOG_FAIL='reset: a log could not be emptied: '
STARTED_LINE='lamp-setup: reset: started in place, no reboot: '

host_ok() { [ "$(hostname)" = "$LAMP" ] || { echo "REFUSE: this is $(hostname), not $LAMP"; exit 1; }; }
root_ok() { [ "$(id -u)" = 0 ] || { echo "REFUSE: run as root"; exit 1; }; }
wall_us() { date +%s%6N; }
boot_id() { cat /proc/sys/kernel/random/boot_id; }
sha_of() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
# A path's kind and, for a folder, how many entries (names never printed); for a file its size; owner:mode.
kind_of() {
  if [ -L "$1" ]; then echo link
  elif [ -d "$1" ]; then echo "dir n=$(find "$1" -mindepth 1 -maxdepth 1 2>/dev/null | wc -l | tr -d ' ') $(stat -c '%U:%G:%a' "$1")"
  elif [ -f "$1" ]; then echo "file bytes=$(stat -c %s "$1") $(stat -c '%U:%G:%a' "$1")"
  elif [ -e "$1" ]; then echo other
  else echo absent; fi
}
# The engines' homes: each real folder in HOMES (a link is never followed)
homes() { find "$HOMES" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort; }
top_entries() { find "$HOMES" -mindepth 1 -maxdepth 1 2>/dev/null | sort; }
# Log files' names and inodes only (the text logs and their rotated copies, both folders)
log_inodes() {
  for d in $LOGS_RAM $LOGS_HDD; do for l in $TEXT_LOGS; do
    for f in "$d/$l" "$d/$l".*; do [ -e "$f" ] && [ ! -L "$f" ] && echo "$f $(stat -c %i "$f")"; done
  done; done
}
# Counts lamp-voice lines older than reset time T (epoch s) in the text logs and the journal copies; counts only.
logcount() { python3 -I "$KIT/logcount.py" "$@"; }

# --- leg 2b (the first builder's): the PR's lamp-voice removes the OLD clips folder at its served start ---
# ring_clips.rs OLD_DIR / remove_old_dir; coordinator.rs run_source calls it at a served (continuous) start.
UNIT=lamp-voice
VBIN=$BIN_DIR/lamp-voice
VSTOCK=/opt/lampOS/voice/current/lamp-voice
RDIR=/run/systemd/system/lamp-voice.service.d
OURS=$RDIR/el-8671.conf
OLD_REMOVED_RE='^lamp-voice: the old offline ring clips folder was removed \(([0-9]+) files\)$'
OLD_LEFT='lamp-voice: the old offline ring clips folder was left: '
READY='"status":"listening_ready"'
MUTE='muted by the restart guard after a fault, not by choice'
# Made-up clip names in the <store_id>-<id>.wav/.json pattern (ring_clips.rs :124), a store id no lamp has
KSTORE=e18671e18671e18671e18671e18671e1
mainpid() { systemctl show -p MainPID --value $UNIT; }
nrestarts() { systemctl show -p NRestarts --value $UNIT; }
env_of() { tr '\0' '\n' < /proc/$1/environ 2>/dev/null | sed -n "s/^$2=//p"; }
unit_exec_of() { systemctl cat "$1" 2>/dev/null | sed -n 's/^ExecStart=//p' | awk 'NF{l=$0} END{print l}'; }
# A folder's file names and sizes (never contents), sorted
names_sizes() { find "$1" -mindepth 1 -maxdepth 1 -printf '%f %s %y\n' 2>/dev/null | sort; }
# The drop-in: lamp-voice on the PR's CI build with the stock unit's own args and LAMP_OFFLINE_RINGS=off, so no
# Schedule consumer renders or sweeps the new clips folder while the leg reads it (no clip is cleared at this
# head with the switch off: #8933 is not in it).
write_dropin() {
  a=$(cut -d' ' -f2- $K/before-exec.txt)
  mkdir -p $RDIR
  { echo "# #8671 row (kit rows/8671), leg 2b: lamp-voice runs the PR's CI build with the stock unit's own args."
    echo "# In /run: a reboot drops it. Removed by the kit's oldclips.sh, putback.sh or the dead-man ($DEADMAN)."
    echo "[Service]"; echo "Environment=LAMP_OFFLINE_RINGS=off"
    echo "ExecStart="; printf 'ExecStart=%s %s\n' "$VBIN" "$a"; } > "$OURS.new" && chmod 644 "$OURS.new" && mv "$OURS.new" "$OURS"
  systemctl daemon-reload
  [ "$(unit_exec_of $UNIT | awk '{print $1}')" = "$VBIN" ] || { echo "DROPIN: ExecStart is not $VBIN"; return 1; }
}
# The drop-in out and the stock voice back (one restart) when the running voice is the PR's: 0 ok, 1 not
voice_to_stock() {
  [ -e "$OURS" ] && rm -f "$OURS" "$OURS.new"; rmdir $RDIR 2>/dev/null; systemctl daemon-reload
  if [ "$(readlink /proc/$(mainpid)/exe)" = "$VBIN" ]; then systemctl restart $UNIT || return 1; sh "$KIT/fresh-wait.sh" || return 1; fi
  [ "$(sha_of /proc/$(mainpid)/exe)" = "$(sha_of $VSTOCK)" ]
}
