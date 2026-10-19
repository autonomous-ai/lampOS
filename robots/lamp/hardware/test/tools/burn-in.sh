#!/usr/bin/env bash
# burn-in.sh — hardware acceptance test, run on the device before it ships.
#
# Catches the failures that look like software bugs for days: bad DRAM, a dying
# card, a thermal or power problem. Written after a unit shipped with ~50 MB of
# stuck-at-0 DRAM and spent a day masquerading as a kernel bug, a filesystem
# bug, and an agent crash in turn.
#
#   sudo ./burn-in.sh              full run  (~25-35 min, factory bench)
#   sudo ./burn-in.sh --quick      short run (~5-7 min, first boot / spot check)
#   sudo ./burn-in.sh --ram 4096 --passes 2
#   sudo ./burn-in.sh --io-mbps 2 --io-mb 128      gentler on the SD card
#
# Exit: 0 = PASS, 1 = FAIL, 2 = could not run (not root, missing tool),
#       3 = INCOMPLETE (a stage was skipped or stopped; never a pass).
# Results land in /var/lib/autonomous/burn-in.{log,json} for the fleet report.
#
# Gentle by design (v1.1): the SD write/read is rate-limited (default 4 MB/s,
# synced per 1 MB chunk, lowest I/O priority), and a thermal guard watches the
# hottest zone every second for the whole run: at 85 °C it pauses the load
# (memtester, the CPU soak), resumes at 80 °C, and at 88 °C kills the load and
# marks the run STOPPED, so the CPU never reaches 90 °C.
#
# Coverage honesty: memtester can only test memory it is allowed to allocate,
# so the kernel's own pages are never covered. This is a strong screen, not a
# proof. A unit that fails here is definitely bad; one that passes is probably
# fine.

set -uo pipefail

VERSION="1.1"
RESULT_DIR="/var/lib/autonomous"
LOG="${RESULT_DIR}/burn-in.log"
JSON="${RESULT_DIR}/burn-in.json"

# Cap how many mismatch lines reach the screen and the log. A properly broken
# DIMM emits them by the million — enough to fill a RAM-backed /var/log and
# take the device down mid-test, which would destroy the evidence we came for.
MAX_FAIL_LINES=20

QUICK=0
RAM_MB=""
PASSES=""
SOAK_S=""
CPU_WORKERS=""
IO_MB=""
IO_MBPS=4
SKIP_STORAGE=0
SKIP_THERMAL=0
# Bad memory ends the run by default; see the fail-fast block after the RAM test.
FAIL_FAST=1
# Pass limit for the soak: the hottest zone must stay under it. The SoC
# throttles well below this; we only want to catch a heatsink that was never
# seated or a fan that never spins.
TEMP_LIMIT_C=85
# Thermal guard. Pausing at the pass limit means any pause is already a
# thermal FAIL, so the guard protects the board without hiding a bad cooler.
PAUSE_C=85
RESUME_C=80
ABORT_C=88

# ── Presentation ─────────────────────────────────────────────────────────────
if [ -t 1 ]; then
  C_RST=$'\033[0m'; C_DIM=$'\033[90m'; C_RED=$'\033[31m'
  C_GRN=$'\033[32m'; C_YEL=$'\033[33m'; C_BLD=$'\033[1m'
else
  C_RST=""; C_DIM=""; C_RED=""; C_GRN=""; C_YEL=""; C_BLD=""
fi

ts()   { date '+%H:%M:%S'; }
# Every line is timestamped and tee'd, so the operator watches it live and the
# same stream survives in the log for whoever reads it later.
say()  { printf '%s[%s]%s %s\n' "$C_DIM" "$(ts)" "$C_RST" "$*" | tee -a "$LOG"; }
head2(){ printf '\n%s══ %s %s\n' "$C_BLD" "$*" "$C_RST" | tee -a "$LOG"; }
ok()   { printf '%s[%s]%s   %sPASS%s  %s\n' "$C_DIM" "$(ts)" "$C_RST" "$C_GRN" "$C_RST" "$*" | tee -a "$LOG"; }
bad()  { printf '%s[%s]%s   %sFAIL%s  %s\n' "$C_DIM" "$(ts)" "$C_RST" "$C_RED" "$C_RST" "$*" | tee -a "$LOG"; }
warn() { printf '%s[%s]%s   %sWARN%s  %s\n' "$C_DIM" "$(ts)" "$C_RST" "$C_YEL" "$C_RST" "$*" | tee -a "$LOG"; }
fmt_status() {
  case "$1" in
    PASS)    printf '%sPASS%s' "$C_GRN" "$C_RST" ;;
    FAIL)    printf '%sFAIL%s' "$C_RED" "$C_RST" ;;
    STOPPED) printf '%sSTOPPED%s' "$C_RED" "$C_RST" ;;
    *)       printf '%sSKIP%s' "$C_YEL" "$C_RST" ;;
  esac
}
die()  { printf '%sburn-in: %s%s\n' "$C_RED" "$*" "$C_RST" >&2; exit 2; }

usage() {
  sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'
  echo
  echo "Options: --quick --ram MB --passes N --soak SECONDS --cpu WORKERS"
  echo "         --io-mb MB --io-mbps MB_PER_S --skip-storage --skip-thermal --no-fail-fast"
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --quick)        QUICK=1 ;;
    --ram)          RAM_MB="${2:-}"; shift ;;
    --passes)       PASSES="${2:-}"; shift ;;
    --soak)         SOAK_S="${2:-}"; shift ;;
    --cpu)          CPU_WORKERS="${2:-}"; shift ;;
    --io-mb)        IO_MB="${2:-}"; shift ;;
    --io-mbps)      IO_MBPS="${2:-}"; shift ;;
    --skip-storage) SKIP_STORAGE=1 ;;
    --skip-thermal) SKIP_THERMAL=1 ;;
    --no-fail-fast) FAIL_FAST=0 ;;
    -h|--help)      usage ;;
    *)              die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

[ "$(id -u)" -eq 0 ] || die "must run as root (memtester needs to lock memory)"
command -v memtester >/dev/null 2>&1 || die "memtester not installed"
case "$IO_MBPS" in ''|*[!0-9]*|0) die "--io-mbps must be a whole number of MB/s, at least 1" ;; esac

mkdir -p "$RESULT_DIR" || die "cannot create $RESULT_DIR"
: > "$LOG"

# ── Thermal guard ────────────────────────────────────────────────────────────
# hottest_c prints the hottest thermal zone in whole °C. A reading over 150 is
# a sensor glitch, not heat, and is ignored; no zone at all prints 0.
hottest_c() {
  local z v best=0
  for z in /sys/class/thermal/thermal_zone*/temp; do
    v=$(awk '{printf "%d", $1/1000}' "$z" 2>/dev/null) || continue
    [ -n "$v" ] && [ "$v" -le 150 ] && [ "$v" -gt "$best" ] && best=$v
  done
  printf '%s' "$best"
}

GUARD_DIR=$(mktemp -d)
LOAD_PIDS="${GUARD_DIR}/load.pids"   # our own CPU workers, one PID per line
: > "$LOAD_PIDS"
echo 0 > "${GUARD_DIR}/peak"
echo 0 > "${GUARD_DIR}/paused_s"

# load_signal sends a signal to everything the guard may pause: memtester,
# stress, and the soak's own workers. It never touches anything else.
load_signal() {
  local sig="$1" p
  pkill "-${sig}" -x memtester 2>/dev/null
  pkill "-${sig}" -x stress 2>/dev/null
  while read -r p; do
    [ -n "$p" ] && kill "-${sig}" "$p" 2>/dev/null
  done < "$LOAD_PIDS"
}

# The guard samples every second: temperature on this SoC climbs several
# degrees in the 10 s the soak's own log line used to wait.
guard_loop() {
  local t paused=0 peak
  while true; do
    t=$(hottest_c)
    peak=$(cat "${GUARD_DIR}/peak")
    [ "$t" -gt "$peak" ] && echo "$t" > "${GUARD_DIR}/peak"
    if [ -f "${GUARD_DIR}/aborted" ]; then
      :   # load already killed; only the peak is still tracked
    elif [ "$t" -ge "$ABORT_C" ]; then
      echo "$t" > "${GUARD_DIR}/aborted"
      load_signal CONT; load_signal TERM
      bad "thermal guard: ${t}°C ≥ ${ABORT_C}°C — load killed, run STOPPED"
    elif [ "$paused" -eq 0 ] && [ "$t" -ge "$PAUSE_C" ]; then
      paused=1; load_signal STOP
      warn "thermal guard: ${t}°C ≥ ${PAUSE_C}°C — load paused until ${RESUME_C}°C"
    elif [ "$paused" -eq 1 ] && [ "$t" -le "$RESUME_C" ]; then
      paused=0; load_signal CONT
      say "thermal guard: ${t}°C — load resumed"
    fi
    [ "$paused" -eq 1 ] && echo $(( $(cat "${GUARD_DIR}/paused_s") + 1 )) > "${GUARD_DIR}/paused_s"
    sleep 1
  done
}

HEARTBEAT=""
GUARD=""
cleanup() {
  [ -n "$HEARTBEAT" ] && kill "$HEARTBEAT" 2>/dev/null
  [ -n "$GUARD" ] && kill "$GUARD" 2>/dev/null
  # Never leave a paused worker behind.
  load_signal CONT; while read -r p; do kill "$p" 2>/dev/null; done < "$LOAD_PIDS"
  rm -f "${RESULT_DIR}/.burnin-scratch"
  rm -rf "$GUARD_DIR"
}
trap cleanup EXIT
trap 'exit 2' INT TERM

aborted() { [ -f "${GUARD_DIR}/aborted" ]; }

# ── Sizing ───────────────────────────────────────────────────────────────────
# Default to most of what is actually free. Leaving ~1 GB keeps the running
# services alive: an OOM kill mid-test reads as a hardware fault and isn't one.
AVAIL_MB=$(awk '/MemAvailable/ {printf "%d", $2/1024}' /proc/meminfo)
if [ -z "$RAM_MB" ]; then
  if [ "$QUICK" -eq 1 ]; then
    RAM_MB=512
  else
    RAM_MB=$(( AVAIL_MB - 1024 ))
    [ "$RAM_MB" -lt 512 ] && RAM_MB=512
  fi
fi
[ -z "$PASSES" ] && PASSES=1
# 10 minutes is the qualification soak (hardware-process.md, "Board heat").
[ -z "$SOAK_S" ] && { [ "$QUICK" -eq 1 ] && SOAK_S=60 || SOAK_S=600; }
[ -z "$CPU_WORKERS" ] && CPU_WORKERS=$(nproc)
[ -z "$IO_MB" ] && { [ "$QUICK" -eq 1 ] && IO_MB=64 || IO_MB=256; }

# ── Header ───────────────────────────────────────────────────────────────────
DEVICE_ID=$(python3 -c "import json;print(json.load(open('/root/config/config.json')).get('device_id',''))" 2>/dev/null || true)
DEVICE_TYPE=$(grep -ho 'DEVICE_TYPE=[^ "]*' /etc/systemd/system/os-server.service /opt/hal/.env 2>/dev/null | head -1 | cut -d= -f2)
ROOT_DEV=$(findmnt -no SOURCE / 2>/dev/null)

head2 "AUTONOMOUS BURN-IN v${VERSION}"
say "host        $(hostname)"
say "device      ${DEVICE_TYPE:-unknown}  id=${DEVICE_ID:-unknown}"
say "kernel      $(uname -r)  $(uname -m)"
say "started     $(date '+%Y-%m-%d %H:%M:%S %Z')"
say "plan        RAM ${RAM_MB} MB x${PASSES} pass · SD ${IO_MB} MB at ≤${IO_MBPS} MB/s · soak ${SOAK_S}s x${CPU_WORKERS} · root ${ROOT_DEV}"
say "guard       pause ${PAUSE_C}°C · resume ${RESUME_C}°C · stop ${ABORT_C}°C · pass under ${TEMP_LIMIT_C}°C · now $(hottest_c)°C"
[ "$QUICK" -eq 1 ] && warn "quick mode — a screen, not an acceptance test"

# Swap on the card turns a big memtester allocation into heavy SD writes as
# the services' pages are pushed out. zram is fine; anything else is on disk.
DISK_SWAP=$(awk 'NR>1 && $1 !~ /zram/ {print $1}' /proc/swaps | tr '\n' ' ')
[ -n "$DISK_SWAP" ] && warn "swap on disk (${DISK_SWAP}) — memtester may push pages to the SD card; consider --ram smaller or swapoff"

[ "$(hottest_c)" -eq 0 ] && warn "no thermal zone readable — the guard cannot protect this run"

guard_loop &
GUARD=$!

FAIL_RAM=0; FAIL_STORAGE=0; FAIL_THERMAL=0
RAM_ERRORS=0; MAX_TEMP=0
STATUS_RAM="PASS"; STATUS_STORAGE="PASS"; STATUS_THERMAL="PASS"
STORAGE_MBPS_W=0; STORAGE_MBPS_R=0

# write_result snapshots the verdict to disk. Called as soon as memory is known
# and again at the end, so an unstable board still leaves a usable record.
write_result() {
  local verdict all="${STATUS_RAM} ${STATUS_STORAGE} ${STATUS_THERMAL}"
  case "$all" in
    *FAIL*)          verdict=FAIL ;;
    *STOPPED*|*SKIP*) verdict=INCOMPLETE ;;
    *)               verdict=PASS ;;
  esac
  cat > "$JSON" <<EOF
{
  "version": "${VERSION}",
  "verdict": "${verdict}",
  "quick": $([ "$QUICK" -eq 1 ] && echo true || echo false),
  "finished_at": "$(date -Is)",
  "device_id": "${DEVICE_ID}",
  "device_type": "${DEVICE_TYPE}",
  "kernel": "$(uname -r)",
  "memory": { "status": "${STATUS_RAM}", "tested_mb": ${RAM_MB}, "passes": ${PASSES}, "mismatches": ${RAM_ERRORS} },
  "storage": { "status": "${STATUS_STORAGE}", "tested_mb": ${IO_MB}, "limit_mbps": ${IO_MBPS}, "write_mbps": ${STORAGE_MBPS_W}, "read_mbps": ${STORAGE_MBPS_R} },
  "thermal": { "status": "${STATUS_THERMAL}", "peak_c": ${MAX_TEMP}, "limit_c": ${TEMP_LIMIT_C}, "paused_s": $(cat "${GUARD_DIR}/paused_s" 2>/dev/null || echo 0), "stopped_at_c": $(cat "${GUARD_DIR}/aborted" 2>/dev/null || echo null) }
}
EOF
  printf '%s' "$verdict"
  sync 2>/dev/null || true
}

# ── 1. Memory ────────────────────────────────────────────────────────────────
# The headline test. Everything else here is quick by comparison.
head2 "1/3  MEMORY"
say "memtester ${RAM_MB}M ${PASSES} — each line below appears as that subtest finishes"
say "this is the slow one; expect roughly 8 min per GB per pass (longer if the guard pauses it)"

FAILCOUNT_FILE="${GUARD_DIR}/failcount"
echo 0 > "$FAILCOUNT_FILE"

# Heartbeat: memtester emits a whole subtest per line and some take minutes, so
# without this the operator cannot tell a slow test from a hung one.
( while true; do sleep 60; printf '%s[%s]%s   %s…still running · %s°C%s\n' \
    "$C_DIM" "$(ts)" "$C_RST" "$C_DIM" "$(hottest_c)" "$C_RST"; done ) &
HEARTBEAT=$!

# memtester animates progress in place: a \|/- spinner for some subtests and a
# "setting N/testing N" counter for others, both drawn with backspaces. Dropping
# the backspaces alone would unroll every frame onto one multi-megabyte line, so
# the spinner glyphs and the counter frames are collapsed away too, leaving one
# tidy line per subtest. FAILURE lines are untouched — they are the payload.
stdbuf -oL memtester "${RAM_MB}M" "$PASSES" 2>&1 \
  | stdbuf -oL tr -d '\010' \
  | stdbuf -oL sed -E 's@[\|/\\-]{3,}@@g; s/( *(sett|test)ing +[0-9]+)+/ /g; s/ {2,}/ /g; s/ +$//' \
  | stdbuf -oL awk -v max="$MAX_FAIL_LINES" -v cf="$FAILCOUNT_FILE" '
      /FAILURE/ {
        n++
        if (n <= max) { print "      " $0; fflush() }
        else if (n == max + 1) { print "      … further mismatches suppressed (still counting)"; fflush() }
        next
      }
      /^[[:space:]]*$/ { next }
      { print; fflush() }
      END { print n+0 > cf }
    ' | tee -a "$LOG"
MEMTESTER_RC=${PIPESTATUS[0]}
kill "$HEARTBEAT" 2>/dev/null; HEARTBEAT=""
RAM_ERRORS=$(cat "$FAILCOUNT_FILE" 2>/dev/null || echo 0)
[ -z "$RAM_ERRORS" ] && RAM_ERRORS=0

if [ "$RAM_ERRORS" -gt 0 ]; then
  FAIL_RAM=1; STATUS_RAM="FAIL"
  bad "memory: ${RAM_ERRORS} mismatches (memtester exit ${MEMTESTER_RC})"
  bad "this board has bad DRAM — it cannot be fixed in software. Reject the unit."
elif aborted; then
  # The guard killed memtester: no mismatch seen, but the test did not finish.
  STATUS_RAM="STOPPED"
  warn "memory: stopped by the thermal guard before it finished — not a pass"
elif [ "$MEMTESTER_RC" -ne 0 ]; then
  FAIL_RAM=1; STATUS_RAM="FAIL"
  bad "memory: memtester exit ${MEMTESTER_RC} with no mismatch line — see the log"
else
  ok "memory: ${RAM_MB} MB x${PASSES} clean"
fi

# Persist the verdict the moment it is known. A board that just failed a memory
# test can hang or panic at any point after, and the result has to survive that.
write_result >/dev/null

# Stop here on bad memory. The unit is already rejected, so later stages add no
# information — and they routinely take the machine down mid-test, which costs
# the log we came for. --no-fail-fast when you deliberately want the full sweep.
if [ "$FAIL_RAM" -eq 1 ] && [ "$FAIL_FAST" -eq 1 ]; then
  warn "skipping storage and thermal — verdict already decided by memory"
  SKIP_STORAGE=1
  SKIP_THERMAL=1
fi
# A board the guard had to stop is too hot to keep loading.
if aborted; then
  SKIP_THERMAL=1
fi

# ── 2. Storage ───────────────────────────────────────────────────────────────
head2 "2/3  STORAGE"
if [ "$SKIP_STORAGE" -eq 1 ]; then
  warn "not run"; STATUS_STORAGE="SKIP"
else
  FS_STATE=$(dumpe2fs -h "$ROOT_DEV" 2>/dev/null | awk -F': *' '/Filesystem state/{print $2}')
  LIFETIME=$(dumpe2fs -h "$ROOT_DEV" 2>/dev/null | awk -F': *' '/Lifetime writes/{print $2}')
  say "filesystem  state=${FS_STATE:-unknown}  lifetime writes=${LIFETIME:-unknown}"

  case "$FS_STATE" in
    clean) ok "filesystem clean" ;;
    "")    warn "could not read filesystem state" ;;
    *)     STATUS_STORAGE="FAIL"; FAIL_STORAGE=1; bad "filesystem state '${FS_STATE}' — run fsck before shipping" ;;
  esac

  # Write and read back in 1 MB chunks, at most IO_MBPS chunks a second, at the
  # lowest I/O priority. Each chunk is synced on its own (dsync), so the card
  # never takes a burst of hundreds of dirty megabytes at once. The read uses
  # O_DIRECT so it comes from the card, not the page cache.
  SCRATCH="${RESULT_DIR}/.burnin-scratch"
  FREE_MB=$(df -Pm "$RESULT_DIR" | awk 'NR==2 {print $4}')
  GAP=$(awk -v r="$IO_MBPS" 'BEGIN {printf "%.3f", 1/r}')
  IOPRIO=""
  command -v ionice >/dev/null 2>&1 && IOPRIO="ionice -c3"
  if [ "${FREE_MB:-0}" -lt $(( IO_MB * 2 )) ]; then
    STATUS_STORAGE="SKIP"
    warn "only ${FREE_MB} MB free — need $(( IO_MB * 2 )) MB for a ${IO_MB} MB check; not run"
  else
    say "write/verify ${IO_MB} MB to ${ROOT_DEV} at ≤${IO_MBPS} MB/s (synced per MB, cache bypassed on read)"
    T0=$(date +%s)
    W_OK=1
    for i in $(seq 0 $(( IO_MB - 1 ))); do
      $IOPRIO dd if=/dev/urandom of="$SCRATCH" bs=1M count=1 seek="$i" \
        conv=notrunc oflag=dsync status=none 2>/dev/null || { W_OK=0; break; }
      sleep "$GAP"
    done
    T1=$(date +%s)
    if [ "$W_OK" -eq 1 ]; then
      STORAGE_MBPS_W=$(( IO_MB / ( T1 - T0 > 0 ? T1 - T0 : 1 ) ))
      W_SUM=$(md5sum "$SCRATCH" | cut -d' ' -f1)
      sync; echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true
      R_SUM=$(for i in $(seq 0 $(( IO_MB - 1 ))); do
                $IOPRIO dd if="$SCRATCH" bs=1M count=1 skip="$i" iflag=direct status=none 2>/dev/null
                sleep "$GAP"
              done | md5sum | cut -d' ' -f1)
      T2=$(date +%s)
      STORAGE_MBPS_R=$(( IO_MB / ( T2 - T1 > 0 ? T2 - T1 : 1 ) ))
      rm -f "$SCRATCH"; sync
      say "took $(( T1 - T0 ))s write · $(( T2 - T1 ))s read"
      if [ "$W_SUM" = "$R_SUM" ]; then
        ok "storage read-back matches (${W_SUM:0:12}…)"
      else
        STATUS_STORAGE="FAIL"; FAIL_STORAGE=1
        bad "storage read-back MISMATCH — card is corrupting data"
      fi
    else
      rm -f "$SCRATCH"
      STATUS_STORAGE="FAIL"; FAIL_STORAGE=1; bad "could not write scratch file — disk full or read-only?"
    fi
  fi

  DISK_PCT=$(df --output=pcent / | tail -1 | tr -dc '0-9')
  [ "${DISK_PCT:-0}" -gt 90 ] && { warn "root filesystem ${DISK_PCT}% full"; } || say "root usage ${DISK_PCT}%"
fi

# ── 3. Thermal + load ────────────────────────────────────────────────────────
head2 "3/3  THERMAL SOAK"
if [ "$SKIP_THERMAL" -eq 1 ]; then
  warn "not run"; STATUS_THERMAL="SKIP"
elif [ "$(hottest_c)" -eq 0 ]; then
  warn "no thermal zone exposed — cannot measure"; STATUS_THERMAL="SKIP"
else
  say "loading ${CPU_WORKERS} workers for ${SOAK_S}s from $(hottest_c)°C, logging every 10s"
  if command -v stress >/dev/null 2>&1; then
    stress --cpu "$CPU_WORKERS" --timeout "${SOAK_S}s" >/dev/null 2>&1 &
    LOADPID=$!
  else
    # No stress: plain shell busy loops, one per worker, killed at the end.
    say "stress not installed — using shell busy loops"
    for _ in $(seq 1 "$CPU_WORKERS"); do
      ( while :; do :; done ) &
      echo $! >> "$LOAD_PIDS"
    done
    LOADPID=""
  fi
  ELAPSED=0
  while [ "$ELAPSED" -lt "$SOAK_S" ] && ! aborted; do
    sleep 10; ELAPSED=$((ELAPSED + 10))
    say "  ${ELAPSED}s  $(hottest_c)°C  (peak $(cat "${GUARD_DIR}/peak")°C)"
  done
  if [ -n "$LOADPID" ]; then
    wait "$LOADPID" 2>/dev/null
  else
    while read -r p; do kill "$p" 2>/dev/null; done < "$LOAD_PIDS"
    : > "$LOAD_PIDS"
  fi
fi

# The guard watched the whole run, memtester included, so its peak is the
# board's peak, not just the soak's.
MAX_TEMP=$(cat "${GUARD_DIR}/peak")
PAUSED_S=$(cat "${GUARD_DIR}/paused_s")
if aborted; then
  # Past the stop point is past the pass limit: a thermal FAIL, not just a stop.
  STATUS_THERMAL="FAIL"; FAIL_THERMAL=1
  bad "thermal: stopped at $(cat "${GUARD_DIR}/aborted")°C — check heatsink seating and both fans"
elif [ "$STATUS_THERMAL" = "SKIP" ]; then
  :
elif [ "$MAX_TEMP" -ge "$TEMP_LIMIT_C" ]; then
  STATUS_THERMAL="FAIL"; FAIL_THERMAL=1
  bad "peak ${MAX_TEMP}°C ≥ ${TEMP_LIMIT_C}°C limit (load paused ${PAUSED_S}s) — check heatsink seating and both fans"
else
  ok "peak ${MAX_TEMP}°C under load"
fi

# ── Verdict ──────────────────────────────────────────────────────────────────
head2 "RESULT"
printf '  %-10s %s\n' "memory"  "$(fmt_status "$STATUS_RAM")$([ "$STATUS_RAM" = "FAIL" ] && echo "  ${RAM_ERRORS} mismatches")" | tee -a "$LOG"
printf '  %-10s %s\n' "storage" "$(fmt_status "$STATUS_STORAGE")" | tee -a "$LOG"
printf '  %-10s %s\n' "thermal" "$(fmt_status "$STATUS_THERMAL")  peak ${MAX_TEMP}°C" | tee -a "$LOG"

VERDICT=$(write_result)

echo | tee -a "$LOG"
case "$VERDICT" in
  PASS)
    printf '%s  ██  BURN-IN PASS  ██%s\n' "$C_GRN$C_BLD" "$C_RST" | tee -a "$LOG"
    [ "$QUICK" -eq 1 ] && say "quick mode: a full run is still required before shipping"
    say "log ${LOG} · result ${JSON}"
    exit 0 ;;
  INCOMPLETE)
    printf '%s  ██  BURN-IN INCOMPLETE  ██%s\n' "$C_YEL$C_BLD" "$C_RST" | tee -a "$LOG"
    say "a stage was skipped or stopped — this is not a pass; run it again"
    say "log ${LOG} · result ${JSON}"
    exit 3 ;;
  *)
    printf '%s  ██  BURN-IN FAIL  ██%s\n' "$C_RED$C_BLD" "$C_RST" | tee -a "$LOG"
    [ "$FAIL_RAM" -eq 1 ] && say "DO NOT SHIP. Bad DRAM surfaces later as kernel panics, filesystem"
    [ "$FAIL_RAM" -eq 1 ] && say "corruption and random process crashes that look like software bugs."
    say "log ${LOG} · result ${JSON}"
    exit 1 ;;
esac
