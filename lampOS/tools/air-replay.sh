#!/usr/bin/env bash
# Replay one synthetic air trace on a real lamp's bus, in real time, and say what Smoky air and Dusty air did
# (#1684). Run on the lamp itself, by the builder who holds it, over ssh as the lamp's own user, in a 15-minute slot:
#
#   air-replay.sh TRACE.jsonl          # the trial: preconditions, replay, put back, verdict
#   air-replay.sh --check TRACE.jsonl  # the preconditions only: printed, nothing changed
#   air-replay.sh --put-back           # after a run that was cut off (power, kill -9): put back from its set-aside
#
# TRACE.jsonl is one of lampOS/crates/skills/tests/fixtures/air-traces/ (its README says what each is), copied to the
# lamp beside this script. Only a trace that fits the slot in real time is replayed: smoke.jsonl (11.5 minutes) and
# cooking-spike.jsonl (8 minutes), ONE A SLOT: once Smoky air has spoken it says nothing for 30 minutes whatever
# comes, so a second trace in the same slot would show nothing. dust.jsonl (96 minutes) and slow-rise.jsonl (97.5
# minutes) are refused: they are test-only, since Dusty air's hour and the rise's 90 minutes cannot be shortened
# without changing the skills' numbers or the trace (`cargo test -p lamp-skills air_traces` replays all four in
# virtual time).
#
# The live sensor. lamp-air says the room's own air/state every 5 s, and a replay beside it would not show the
# trace: the two interleave, each real reading enters the 3 minutes Smoky air's rise looks back over, and any
# replayed level 100 above the room's then counts as a sudden rise, a slow climb too. So for the replay lamp-air is
# stopped (`systemctl stop`, as trials.sh pauses it for its smoky and dusty rows; "with it stopped the lamp runs on
# as before", docs/air.md) and started again at the end. On a lamp where no air/state reading comes in 12 s (no
# sensor) nothing is stopped. Every air/state message of the window is recorded with its source, and a reading from
# anything but the replay makes the verdict VOID.
#
# What it changes, and nothing else:
#   1. lamp-air.service is stopped for the replay, if it is saying readings, and started again.
#   2. Each of smoky-air and dusty-air found installed off (`enabled = false` above its skill.toml, lamp-update's
#      install::PRODUCT_SKILLS_OFF) is turned on the way a trial is meant to (docs/air.md, "Smoky air and Dusty air
#      on a trial lamp"): that one line taken out of the installed file, which the running lamp-skills reads again
#      within 2 s. Its numbers are not touched: a lamp whose [trigger] is not the one the traces were shaped for is
#      refused. One found on is left on. lamp-skills is never restarted.
#   3. air/state carries the trace for the window, so everything that reads it sees the trace: the app's air
#      readout shows its figures. Its CO2 is "not known", so Stuffy air neither starts nor ends a spell on it.
#   4. If the skill runs, the lamp says its line aloud once, lifts its head a little and breathes amber.
# Put back, after a pass, a failure, Ctrl-C or a dropped ssh: each skill.toml copied back from its
# set-aside and read back the same (SHA-256, owner, group, mode, modification time), `lamp-skills list` showing it
# as found, lamp-air active and its own reading on the bus again, the topic's declaration as found. The whole kit
# runs as root from one sudo at its start, so the put-back never has to ask for a password. A power cut or kill -9
# is the one thing it cannot catch: the set-aside is on disk (/var/tmp/air-replay.aside), a new run refuses while
# it is there, and `--put-back` finishes the job (it also ends a replay still going). A skill.toml that something
# else changed meanwhile (an update) is never written over: it is left, said, and the exit is 4.
#
# It reads no settings, keys or names: of the journal only lamp-skills' own lines about these two skills are read
# and kept. It never reboots, never restarts lamp-skills, and touches no file but the two skill.toml, its
# set-aside and its own temp folder (/tmp/air-replay.XXXXXX: the recording of air/state and voice/state, the
# replay file, the journal lines and the verdict; kept for the builder to copy off, gone at a reboot).
#
# Exit: 0 replayed, put back and judged; 1 replayed and put back, but the verdict is VOID; 2 usage; 3 refused,
# nothing changed; 4 the put-back failed or a step failed after something was changed (look at the "put back"
# lines); 5 stopped (Ctrl-C), put back.
#
# Settings (a lamp needs none; a stand-in check sets them):
#   AIR_REPLAY_SUDO    the command that runs this as root (sudo; empty: run as the user it is)
#   AIR_REPLAY_SLOT_S  the slot in seconds (900)
#   AIR_REPLAY_TMP     where its temp folder goes (/tmp)
#   AIR_REPLAY_ASIDE   its set-aside (/var/tmp/air-replay.aside)
#   LAMP_BUS_BIN       lamp-bus (on PATH, else /usr/local/bin/lamp-bus)
set -u
# A dropped ssh closes the output: a line that cannot be printed must not end the put-back.
trap '' PIPE

ME=air-replay.sh
SLOT=${AIR_REPLAY_SLOT_S:-900}
# Setting up and putting back, beside the trace's own length: the 12 s listen, the pause, the reload, lamp-air's
# first reading after its start.
OVERHEAD=150
TMP_BASE=${AIR_REPLAY_TMP:-/tmp}
ASIDE=${AIR_REPLAY_ASIDE:-/var/tmp/air-replay.aside}
BUS=${LAMP_BUS_BIN:-$(command -v lamp-bus 2>/dev/null || echo /usr/local/bin/lamp-bus)}
SKILLS_UNIT=lamp-skills.service
AIR_UNIT=lamp-air.service
SKILLS="smoky-air dusty-air"
# Smoky air says nothing for this long after a run: its 30 m cooldown, and a margin.
MASK_MIN=45

say() { printf '%s\n' "$*"; }
usage() {
  say "usage: $ME TRACE.jsonl | --check TRACE.jsonl | --put-back" >&2
  exit 2
}

MODE=run
case ${1:-} in
  --check) MODE=check; shift ;;
  --put-back) MODE=putback; shift ;;
  -*) usage ;;
esac
TRACE=${1:-}
if [ "$MODE" = putback ]; then [ $# -eq 0 ] || usage; else [ $# -eq 1 ] && [ -n "$TRACE" ] || usage; fi

# The [trigger] each trace was shaped against (lampOS/skills/<skill>/skill.toml, its comments left out). A lamp
# whose installed skill says anything else is refused: the verdict would be about other numbers.
want_trigger() { # SKILL
  case $1 in
    smoky-air)
      cat <<'EOF'
topic = "air/state"
field = "pm2_5"
above = 149.95
rise = { by = 100, within = "3m" }
for = "30s"
gap = "10s"
rearm = { below = 150, for = "10m" }
cooldown = "30m"
EOF
      ;;
    dusty-air)
      cat <<'EOF'
topic = "air/state"
field = "pm2_5"
above = 124.95
for = "60m"
gap = "2m"
rearm = "ends"
cooldown = "24h"
wait = "1h"
EOF
      ;;
  esac
}
# A skill.toml's [trigger] table, its comments and blank lines left out.
trigger_of() { # FILE
  sed -n '/^\[trigger\]/,/^\[/p' "$1" | sed -e '/^\[/d' -e 's/[[:space:]]*#.*$//' -e 's/[[:space:]]*$//' -e '/^$/d'
}
# Whether a skill.toml says `enabled = false` at its top level (before its first table), as install writes it.
found_off() { sed '/^\[/q' "$1" | grep -qx 'enabled = false'; }
# The file without that line, and nothing else changed.
without_off() { awk 'BEGIN { top = 1 } /^\[/ { top = 0 } !(top && $0 == "enabled = false")' "$1"; }
sha_of() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
# Owner, group, mode and modification time, as one word to compare.
stat_of() { stat -c '%U:%G:%a:%Y' "$1" 2>/dev/null; }

FAILED=0
ok() { say "  ok   $*"; }
no() { say "  NO   $*"; FAILED=1; }

# --- The trace: every line {"at_s":N,"air":{...}}, from 0, rising. ----------------------------------------------
AT=()
AIR=()
read_trace() {
  local line n=0 last=-1
  local shape='^\{"at_s":([0-9]+),"air":(\{.*"pm2_5":[0-9.]+.*\})\}$'
  [ -r "$TRACE" ] || { no "the trace: $TRACE cannot be read"; return; }
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    if [[ $line =~ $shape ]]; then
      if [ "${BASH_REMATCH[1]}" -le "$last" ]; then no "the trace: line $n does not come after the one before"; return; fi
      last=${BASH_REMATCH[1]}
      AT+=("$last")
      AIR+=("${BASH_REMATCH[2]}")
    else
      no "the trace: line $n is not {\"at_s\":N,\"air\":{…}} with a pm2_5"
      return
    fi
  done <"$TRACE"
  [ "${#AT[@]}" -ge 2 ] && [ "${AT[0]}" = 0 ] || { no "the trace: at least two lines, the first at 0 s"; return; }
  LENGTH=${AT[${#AT[@]} - 1]}
  NAME=$(basename "$TRACE" .jsonl)
  ok "the trace: $NAME, ${#AT[@]} readings over $LENGTH s"
  if [ $((LENGTH + OVERHEAD)) -le "$SLOT" ]; then
    ok "it fits the slot: $LENGTH s and about $OVERHEAD s around it, in $SLOT s"
  else
    no "it does not fit the slot: $LENGTH s and about $OVERHEAD s around it is more than $SLOT s. $NAME is test-only: real time cannot be shortened without changing the skills' numbers or the trace (cargo test -p lamp-skills air_traces replays it in virtual time)"
  fi
}

# --- What can be checked as anyone: is this a lamp at all? ------------------------------------------------------
check_lamp() {
  local tool
  if [ "$(uname -s)" = Linux ]; then ok "this is Linux"; else no "this is $(uname -s), not a lamp: run it on the lamp itself"; fi
  for tool in systemctl journalctl sha256sum stat base64 awk sed od; do
    command -v "$tool" >/dev/null 2>&1 || no "$tool is not here"
  done
  if command -v systemctl >/dev/null 2>&1; then
    if [ "$(systemctl is-active "$SKILLS_UNIT" 2>/dev/null)" = active ]; then
      ok "$SKILLS_UNIT is active"
    else
      no "$SKILLS_UNIT is not active: there is no lamp-skills to judge"
    fi
  fi
  if [ -x "$BUS" ]; then ok "lamp-bus is $BUS"; else no "no lamp-bus at $BUS"; fi
  if [ "$(id -u)" != 0 ] && [ -n "$SUDO" ] && ! command -v "${SUDO%% *}" >/dev/null 2>&1; then
    no "not root, and no $SUDO to become it"
  fi
}

# --- The running lamp-skills: its folder, its program, its bus. --------------------------------------------------
exec_argv() { systemctl show "$1" -p ExecStart --value 2>/dev/null | sed -n 's/.*argv\[\]=\([^;]*\) ;.*/\1/p' | head -1; }
arg_of() { exec_argv "$1" | tr ' ' '\n' | grep -A1 -xF -- "$2" | sed -n 2p; }
bus() { "$BUS" --root "$BUS_ROOT" "$@"; }
# How lamp-skills itself reads a skill's folder: on, off, or why not.
listed() { # SKILL
  local row
  row=$("$SKILLS_BIN" list --dir "$SKILLS_DIR" 2>/dev/null | awk -v name="$1" '$1 == name')
  case $row in
    '') echo "not listed" ;;
    *"not loaded:"*) echo "not loaded" ;;
    *" disabled; "*) echo off ;;
    *) echo on ;;
  esac
}
# lamp-skills' own journal lines about the two skills since EPOCH, each with its unix time first. Nothing else of
# the journal is read out.
journal_since() { # EPOCH
  journalctl -u "$SKILLS_UNIT" --since "@$1" -o short-unix --no-pager -q 2>/dev/null |
    grep -E 'lamp-skills: (smoky-air|dusty-air) (Triggered|Asked|Answered|Done):|lamp-skills: not loaded: .*(smoky-air|dusty-air)'
}
# How many times lamp-skills has loaded its skills since EPOCH (the count only: the line names every skill).
loads_since() { journalctl -u "$SKILLS_UNIT" --since "@$1" -o cat --no-pager -q 2>/dev/null | grep -c '^lamp-skills: loaded '; }
# Who publishes air/state now: "NAME PID" a line.
air_publishers() { bus info air/state 2>/dev/null | sed -n 's/^publisher \([^ ]*\) pid=\([0-9]*\) .*/\1 \2/p'; }
air_declared() { bus info air/state 2>/dev/null | sed -n 's/^declaration //p'; }
air_latest_source() { bus info air/state 2>/dev/null | sed -n 's/^latest source=\([^ ]*\) .*/\1/p'; }

LIVE=no
AIR_ACTIVE=no
check_as_root() {
  local skill file state want got ran
  SKILLS_DIR=$(arg_of "$SKILLS_UNIT" --dir)
  SKILLS_BIN=$(exec_argv "$SKILLS_UNIT" | cut -d' ' -f1)
  BUS_ROOT=$(arg_of "$SKILLS_UNIT" --bus-root)
  BUS_ROOT=${BUS_ROOT:-/run/lampos-bus}
  if [ -n "$SKILLS_DIR" ] && [ -d "$SKILLS_DIR" ] && [ -x "$SKILLS_BIN" ]; then
    ok "lamp-skills runs $SKILLS_BIN on $SKILLS_DIR, bus $BUS_ROOT"
  else
    no "$SKILLS_UNIT's command line names no --dir that is a folder, or no program"
    return
  fi
  if [ -e "$ASIDE" ]; then
    no "$ASIDE is there: an earlier run was cut off. Run '$ME --put-back' first"
  else
    ok "no earlier run's set-aside is left"
  fi
  for skill in $SKILLS; do
    file=$SKILLS_DIR/$skill/skill.toml
    if [ ! -f "$file" ]; then no "$skill: no $file (a build before #1836?)"; continue; fi
    want=$(want_trigger "$skill")
    got=$(trigger_of "$file")
    if [ "$want" != "$got" ]; then
      no "$skill: its [trigger] on this lamp is not the one the traces were shaped for:"
      say "$got" | sed 's/^/         /'
      continue
    fi
    if found_off "$file"; then state=off; else state=on; fi
    if [ "$(listed "$skill")" != "$state" ]; then
      no "$skill: its file reads as $state but lamp-skills list says $(listed "$skill"): not a state this kit knows"
      continue
    fi
    ok "$skill: installed $state, with the numbers the traces were shaped for$([ "$state" = on ] && echo " (found on: left on)")"
  done
  if journalctl -u "$SKILLS_UNIT" -n 1 -o cat --no-pager -q 2>/dev/null | grep -q .; then
    ran=$(journal_since "$(($(date +%s) - MASK_MIN * 60))" | grep -c -E 'smoky-air (Triggered|Done):')
    if [ "$ran" -gt 0 ]; then
      no "Smoky air ran on this lamp in the last $MASK_MIN minutes: once a spell and its 30 m cooldown would keep it silent whatever the trace. Come back later"
    else
      ok "the journal of lamp-skills can be read, and Smoky air has not run in the last $MASK_MIN minutes"
    fi
  else
    no "the journal of $SKILLS_UNIT cannot be read"
  fi
  if bus topics >/dev/null 2>&1; then ok "the bus at $BUS_ROOT answers"; else no "lamp-bus cannot read the bus at $BUS_ROOT"; return; fi
  [ "$(systemctl is-active "$AIR_UNIT" 2>/dev/null)" = active ] && AIR_ACTIVE=yes
  say "       listening 12 s for the room's own air/state…"
  if bus hz air/state --seconds 12 >/dev/null 2>&1; then LIVE=yes; fi
  if [ "$LIVE" = yes ] && [ "$AIR_ACTIVE" = yes ]; then
    got=$(air_publishers | awk '$1 != "air"' | tr '\n' ' ')
    if [ -n "$got" ]; then
      no "air/state has a publisher that is not lamp-air's ($got): this kit pauses only $AIR_UNIT"
    else
      ok "the sensor is live: $AIR_UNIT says air/state, and is paused for the replay"
    fi
  elif [ "$LIVE" = yes ]; then
    no "air/state readings come, but $AIR_UNIT is not active: something else says them, and this kit pauses only $AIR_UNIT"
  else
    ok "no air/state reading in 12 s (no sensor answers here): nothing to pause, $AIR_UNIT is left as it is"
  fi
}

# --- Putting back. Everything it needs is in $ASIDE/state, so --put-back can do it after a cut-off. -------------
REPLAY_PID=
RECORD_PID=
PUT_BACK_DONE=no
put_back() {
  [ "$PUT_BACK_DONE" = yes ] && return 0
  PUT_BACK_DONE=yes
  [ -f "$ASIDE/state" ] || return 0
  # Not stopped half way: a second Ctrl-C waits for it.
  trap '' INT TERM HUP
  local failed=0 skill file found sha st trial now line i source
  [ -n "$REPLAY_PID" ] && kill "$REPLAY_PID" 2>/dev/null
  [ -n "$RECORD_PID" ] && kill "$RECORD_PID" 2>/dev/null
  wait 2>/dev/null
  # shellcheck disable=SC1090,SC1091 # its own file, written below
  . "$ASIDE/state"
  # After a cut-off the replay and the recorder of that run may still be going: they are the only programs with
  # the run's temp folder in their command line.
  [ -z "${OUT:-}" ] || pkill -f -- "$OUT/" 2>/dev/null
  while IFS='|' read -r skill file found sha st trial; do
    [ -n "$skill" ] || continue
    now=$(sha_of "$file")
    if [ "$now" = "$trial" ] && [ "$now" != "$sha" ]; then
      # The trial's own file, as this kit wrote it: the one as found goes back over it.
      cp -p "$ASIDE/$skill.found" "$file" || failed=1
    elif [ "$now" != "$sha" ]; then
      # Neither as found nor as this kit left it: something else wrote it meanwhile (an update?). Not written over.
      say "PUT BACK NOT DONE: $file was changed by something else during the trial (sha256 $now): left as it is, look at it"
      failed=1
      continue
    fi
    if [ "$(sha_of "$file")" = "$sha" ] && [ "$(stat_of "$file")" = "$st" ] && [ "$(listed "$skill")" = "$found" ]; then
      say "put back: $skill is $found again: $file read back the same (sha256 ${sha:0:12}, $st), lamp-skills list says $found"
    else
      say "PUT BACK FAILED: $file is not as found (sha256 $(sha_of "$file") against $sha, $(stat_of "$file") against $st, list says $(listed "$skill"))"
      failed=1
    fi
  done <"$ASIDE/skills"
  if [ "${PAUSED:-no}" = yes ]; then
    systemctl start "$AIR_UNIT" || failed=1
    i=0
    source=
    while [ $i -lt 20 ]; do
      source=$(air_latest_source)
      [ "$source" = air ] && [ "$(systemctl is-active "$AIR_UNIT" 2>/dev/null)" = active ] && break
      sleep 3
      i=$((i + 1))
    done
    if [ "$source" = air ]; then
      say "put back: $AIR_UNIT is active and the newest air/state is its own again"
    else
      say "PUT BACK FAILED: $AIR_UNIT was started ($(systemctl is-active "$AIR_UNIT" 2>/dev/null)) but no reading of its own came in 60 s (the newest is from '${source:-nobody}'): look at journalctl -u $AIR_UNIT"
      failed=1
    fi
  else
    say "put back: $AIR_UNIT was not touched"
  fi
  line=$(air_declared)
  if [ "$line" = "${DECLARED:-}" ]; then
    say "put back: air/state is declared as found (${line:-not declared})"
  else
    say "PUT BACK FAILED: air/state is declared '$line', found '${DECLARED:-}'"
    failed=1
  fi
  if [ $failed = 0 ]; then
    rm -rf "$ASIDE"
    say "put back: done, the set-aside is removed"
  else
    say "PUT BACK FAILED: the set-aside is kept in $ASIDE (each skill's file as found is <skill>.found there). By hand: copy it back with cp -p, and systemctl start $AIR_UNIT"
    PUT_BACK_FAILED=yes
  fi
}

# --- Start. ----------------------------------------------------------------------------------------------------
if [ -n "${AIR_REPLAY_SUDO+set}" ]; then SUDO=$AIR_REPLAY_SUDO; elif [ "$(id -u)" = 0 ]; then SUDO=; else SUDO=sudo; fi

if [ -n "${AIR_REPLAY_AS_ROOT:-}" ]; then
  # Said already, before the one sudo: checked again here without saying them twice.
  [ "$MODE" = putback ] || read_trace >/dev/null
  check_lamp >/dev/null
else
  say "$ME: preconditions"
  [ "$MODE" = putback ] || read_trace
  check_lamp
fi
if [ $FAILED != 0 ]; then
  say "$ME: refused. Nothing was changed."
  exit 3
fi
# One sudo, here, for everything after: the put-back then never has to ask.
if [ "$(id -u)" != 0 ] && [ -n "$SUDO" ]; then
  say "$ME: the rest runs as root ($SUDO), once"
  if [ "$MODE" = putback ]; then set -- --put-back; elif [ "$MODE" = check ]; then set -- --check "$TRACE"; else set -- "$TRACE"; fi
  # shellcheck disable=SC2086 # SUDO is a command and its words
  exec $SUDO env AIR_REPLAY_SLOT_S="$SLOT" AIR_REPLAY_TMP="$TMP_BASE" AIR_REPLAY_ASIDE="$ASIDE" LAMP_BUS_BIN="$BUS" \
    AIR_REPLAY_SUDO= AIR_REPLAY_AS_ROOT=1 bash "$0" "$@"
fi

if [ "$MODE" = putback ]; then
  SKILLS_DIR=$(arg_of "$SKILLS_UNIT" --dir)
  SKILLS_BIN=$(exec_argv "$SKILLS_UNIT" | cut -d' ' -f1)
  BUS_ROOT=$(arg_of "$SKILLS_UNIT" --bus-root)
  BUS_ROOT=${BUS_ROOT:-/run/lampos-bus}
  if [ ! -e "$ASIDE" ]; then say "$ME: no set-aside in $ASIDE: nothing to put back"; exit 0; fi
  if [ ! -f "$ASIDE/state" ]; then
    # The state is written before the first change: without it the run was cut off with nothing changed.
    rm -rf "$ASIDE"
    say "$ME: the set-aside in $ASIDE was from a run cut off before it changed anything: removed"
    exit 0
  fi
  put_back
  [ "${PUT_BACK_FAILED:-no}" = yes ] && exit 4
  exit 0
fi

check_as_root
if [ $FAILED != 0 ]; then
  say "$ME: refused. Nothing was changed."
  exit 3
fi
if [ "$MODE" = check ]; then
  say "$ME: every precondition holds. Nothing was changed."
  exit 0
fi

say "$ME: $NAME on this lamp, about $(((LENGTH + OVERHEAD + 59) / 60)) minutes. If Smoky air runs, the lamp says its line aloud once."
OUT=$(mktemp -d "$TMP_BASE/air-replay.XXXXXX") || { say "$ME: no temp folder in $TMP_BASE. Nothing was changed."; exit 3; }
DECLARED=$(air_declared)

# The set-aside first, whole, before anything is changed: each skill's file as found, and what to undo.
umask 077
mkdir "$ASIDE" || { say "$ME: $ASIDE cannot be made. Nothing was changed."; exit 3; }
: >"$ASIDE/skills"
for skill in $SKILLS; do
  file=$SKILLS_DIR/$skill/skill.toml
  if found_off "$file"; then found=off; else found=on; fi
  cp -p "$file" "$ASIDE/$skill.found" || { rm -rf "$ASIDE"; say "$ME: $file cannot be set aside. Nothing was changed."; exit 3; }
  without_off "$file" >"$ASIDE/$skill.trial"
  printf '%s|%s|%s|%s|%s|%s\n' "$skill" "$file" "$found" "$(sha_of "$file")" "$(stat_of "$file")" "$(sha_of "$ASIDE/$skill.trial")" >>"$ASIDE/skills"
done
write_state() { printf 'PAUSED=%s\nDECLARED=%q\nOUT=%q\n' "$1" "$DECLARED" "$OUT" >"$ASIDE/state.new" && mv "$ASIDE/state.new" "$ASIDE/state"; }
write_state no
umask 022

trap 'put_back' EXIT
trap 'say "$ME: stopped"; exit 5' INT TERM HUP
fail() { say "$ME: FAILED: $*"; exit 4; }

# 1. The recording, from before anything changes: air/state and voice/state, as lamp-bus record writes them.
mkdir "$OUT/rec"
"$BUS" --root "$BUS_ROOT" record "$OUT/rec" air/state voice/state --seconds $((LENGTH + 2 * OVERHEAD)) >"$OUT/record.log" 2>&1 &
RECORD_PID=$!
i=0
while [ ! -e "$OUT/rec/air_state.jsonl" ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
[ -e "$OUT/rec/air_state.jsonl" ] || fail "lamp-bus record did not start: $(head -3 "$OUT/record.log")"
sleep 1
# The topic declared for the replay exactly as this lamp declares it (the recording's own first line), so the
# replay changes no declaration; on a lamp where nobody has declared it, the replay declares nothing either.
DECLARATION=$(grep -m1 '^{"topic":"air/state","declaration":' "$OUT/rec/air_state.jsonl")
[ -n "$DECLARATION" ] || say "air/state is not declared on this lamp: replayed undeclared"

# 2. The live sensor paused, and the topic heard silent.
if [ "$LIVE" = yes ]; then
  write_state yes
  systemctl stop "$AIR_UNIT" || fail "$AIR_UNIT could not be stopped"
  say "paused: $AIR_UNIT is stopped for the replay"
  sleep 4
  if bus hz air/state --seconds 7 >/dev/null 2>&1; then
    fail "air/state readings still come with $AIR_UNIT stopped: something else says them, and the trace would be crossed"
  fi
  say "air/state is silent"
fi

# 3. The two skills on, as a trial puts them on, and read back.
EDITED=$(date +%s)
CHANGED=0
while IFS='|' read -r skill file found _ _ _; do
  [ "$found" = off ] || { say "$skill: found on, left on"; continue; }
  cp "$ASIDE/$skill.trial" "$file" || fail "$file could not be written"
  CHANGED=$((CHANGED + 1))
  if ! found_off "$file" && [ "$(listed "$skill")" = on ]; then
    say "$skill: on for the trial ('enabled = false' taken out of $file; lamp-skills list says on)"
  else
    fail "$skill is not on after the change (lamp-skills list says $(listed "$skill"))"
  fi
done <"$ASIDE/skills"
if [ $CHANGED -gt 0 ]; then
  i=0
  while [ "$(loads_since "$EDITED")" -lt 1 ] && [ $i -lt 15 ]; do sleep 1; i=$((i + 1)); done
  [ "$(loads_since "$EDITED")" -ge 1 ] || fail "the running lamp-skills did not load the change in 15 s"
  say "the running lamp-skills has loaded them"
fi
# A rise begins again at a load: the replay starts after the last of them (lamp-skills looks every 2 s, and may
# take the two files in two looks), so the trace's first 3 minutes are the rise's own.
sleep 5

# 4. The replay, in real time, by lamp-bus replay.
BOOT=$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
{
  [ -z "$DECLARATION" ] || printf '%s\n' "$DECLARATION"
  i=0
  while [ $i -lt ${#AT[@]} ]; do
    printf '{"header":{"topic":"air/state","kind":"latest","enc":"json","type":"lamp_msgs/AirState","source":"replay","boot":"%s","seq":%d,"at_us":%d,"at_unix_ms":0},"payload_b64":"%s"}\n' \
      "$BOOT" $((i + 1)) $((AT[i] * 1000000)) "$(printf '%s' "${AIR[i]}" | base64 | tr -d '\n')"
    i=$((i + 1))
  done
} >"$OUT/replay.jsonl"
START=$(date +%s)
"$BUS" --root "$BUS_ROOT" replay "$OUT/replay.jsonl" --speed 1 >"$OUT/replay.log" 2>&1 &
REPLAY_PID=$!
say "replaying $NAME: ${#AT[@]} readings over $LENGTH s, from $(date +%H:%M:%S)"
SEEN=0
while kill -0 "$REPLAY_PID" 2>/dev/null; do
  sleep 5
  journal_since "$START" >"$OUT/journal.txt" 2>/dev/null
  n=$(wc -l <"$OUT/journal.txt")
  if [ "$n" -gt "$SEEN" ]; then
    tail -n $((n - SEEN)) "$OUT/journal.txt" | sed "s/^\([0-9]*\)[^ ]* [^ ]* [^ ]* /\1 /" |
      while read -r at rest; do say "  $((at - START)) s: $rest"; done
    SEEN=$n
  fi
done
wait "$REPLAY_PID"
REPLAY_STATUS=$?
REPLAY_PID=
END=$(date +%s)
# A load in the middle of the replay would have begun the rise again: counted before the put-back makes one.
LOADS=$(loads_since "$START")
# A say waits for a pause, for up to 2 minutes; the trace's own tail is longer than that, and 5 s more for the journal.
sleep 5
journal_since "$START" >"$OUT/journal.txt" 2>/dev/null
kill "$RECORD_PID" 2>/dev/null
wait "$RECORD_PID" 2>/dev/null
RECORD_PID=

# 5. Everything back, and read back.
put_back

# 6. The verdict, from the journal and the recording.
{
  say "VERDICT $NAME (replayed $(date -d "@$START" +%Y-%m-%dT%H:%M:%S 2>/dev/null || echo "@$START") to +$((END - START)) s)"
  say "  the replay: $(tail -1 "$OUT/replay.log")$([ $REPLAY_STATUS = 0 ] || echo " (lamp-bus replay exit $REPLAY_STATUS)")"
  COUNTS=$(sed -n 's/.*"source":"\([^"]*\)".*"at_unix_ms":\([0-9]*\).*/\1 \2/p' "$OUT/rec/air_state.jsonl" |
    awk -v from="$START" -v to="$END" '$2 >= from * 1000 && $2 <= (to + 1) * 1000 { if ($1 == "replay") ours++; else other++ } END { printf "%d %d", ours, other }')
  FROM_REPLAY=${COUNTS% *}
  FROM_OTHER=${COUNTS#* }
  say "  recorded in the window: $FROM_REPLAY air/state readings from the replay of ${#AT[@]} sent, $FROM_OTHER from anything else"
  say "  lamp-skills loaded its skills $LOADS time(s) during the replay"
  VALID=yes
  [ "$REPLAY_STATUS" = 0 ] && [ "$FROM_REPLAY" = "${#AT[@]}" ] && [ "$FROM_OTHER" = 0 ] && [ "$LOADS" = 0 ] || VALID=no
  for skill in $SKILLS; do
    case $NAME:$skill in
      smoke:smoky-air) expect=230 ;;
      cooking-spike:smoky-air) expect=255 ;;
      smoke:dusty-air | cooking-spike:dusty-air) expect=none ;;
      *) expect=unknown ;;
    esac
    at=$(grep "lamp-skills: $skill Triggered:" "$OUT/journal.txt" | sed 's/^\([0-9]*\).*/\1/' | head -1)
    runs=$(grep -c "lamp-skills: $skill Triggered:" "$OUT/journal.txt")
    if [ "$runs" = 0 ]; then
      got="nothing"
      [ "$expect" = none ] && same="as the tests" || same="NOT as the tests ($expect s)"
    else
      got="ran $runs time(s), first $((at - START)) s into the trace"
      if [ "$expect" = none ]; then
        same="NOT as the tests (nothing)"
      # Within 10 s of the tests' second: a live hold is counted on each reading's arrival, so it can end one
      # reading (5 s) later than in virtual time, and the journal's clock is to the second.
      elif [ "$expect" != unknown ] && [ "$runs" = 1 ] && [ $((at - START - expect)) -ge -10 ] && [ $((at - START - expect)) -le 10 ]; then
        same="as the tests ($expect s)"
      else
        same="NOT as the tests ($expect s, once)"
      fi
    fi
    [ "$expect" = unknown ] && same="the tests say nothing of this trace"
    say "  $skill: $got: $same"
    grep "lamp-skills: $skill " "$OUT/journal.txt" | sed 's/^[^ ]* [^ ]* [^ ]* /      /'
  done
  [ "$NAME" = cooking-spike ] && say "  A run of Smoky air on this trace is the one line its design allows for a pan (SKILL.md: frying may set it off, once)."
  if [ "$VALID" = yes ]; then
    say "  VALID: every reading of the trace, and only the replay, was on air/state in the window, and no load."
  else
    say "  VOID: the trace was crossed or cut (see the three lines above); what the skills did is not a verdict on it."
  fi
} >"$OUT/verdict.txt"
cat "$OUT/verdict.txt"
# The builder's to copy off without sudo: nothing in it but the two topics and the two skills' own lines.
[ -z "${SUDO_USER:-}" ] || chown -R "$SUDO_USER" "$OUT" 2>/dev/null
say "$ME: the recording, the journal lines and the verdict are in $OUT (copy them off; /tmp goes at a reboot)"
say "$ME: Smoky air's own cooldown runs on in lamp-skills for 30 minutes after a run; with the skill off again it holds nothing back. Another replay on this lamp: after $MASK_MIN minutes."
[ "${PUT_BACK_FAILED:-no}" = yes ] && exit 4
[ "$VALID" = yes ] || exit 1
exit 0
