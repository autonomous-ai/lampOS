#!/usr/bin/env bash
# A recorder for a hand-over test: for a set time it keeps what Harness
# reported about one machine (and one agent on it, when named), each line
# with the local times it was asked for and arrived, and then lays the
# readings out as one timeline. Nothing is sent to Harness but reads
# (`watch`, `show`, `ps`).
#
#   lampOS/tools/harness-record.sh record NAME SECS [--machine ID] [--agent ID] [--out DIR]
#   lampOS/tools/harness-record.sh timeline DIR/NAME
#
# `record` writes DIR/NAME/ (DIR is --out, else $HARNESS_RECORD_DIR, else
# ./harness-records):
#   watch.jsonl   `harness watch [--machine ID] AGENT --json`, every line whole;
#                 only with --agent (watch follows one agent). When watch ends
#                 (the agent not there yet, a Harness timeout, or stopped) the
#                 end is kept as a "!" line and watch starts again 2 s after
#                 it began, or at once when it ran longer.
#   show.jsonl    `harness show [--machine ID] AGENT --json` every 2 s; only
#                 with --agent.
#   ps.jsonl      `harness ps [--machine ID] --json` every 5 s.
#   events.jsonl  `harness events ...`, only if `harness --help` names an
#                 `events` verb (Harness 0.5.27 has none).
#   timeline.txt  the readings merged and sorted by time, built when the
#                 recording ends, or again with `timeline`.
#
# Each line is "END<TAB>START<TAB>MS<TAB>BODY", times in ms since 1970:
#   show, ps   START when the call was made, END when it answered, MS how
#              long it took;
#   watch      START when this watch run began, END when the line came, MS
#              the time between;
#   BODY       one JSON object on one line (show's and ps's pretty JSON is
#              joined onto one), or a "!" line: "!rc=N TEXT" for a failed
#              call, "!watch run N started", "!rc=N watch ended", "!killed at
#              end" for a call cut off when the recording stopped, "!stopped at
#              end" for a watch run.
# A reader never runs two of its own calls at once: the next show starts 2 s
# after the last one began, or at once when it took longer; a slow call is
# never stacked. When the time is up no new call starts, and a call still
# running gets up to HARNESS_RECORD_GRACE seconds (default 30) to answer
# before it is killed; watch, which never answers, is stopped at once. On INT
# or TERM every call is killed at once. Either way the timeline is built.
#
# HARNESS_BIN names the harness binary (default `harness`): the lamp runs it
# as user lampos, with HOME=/var/lib/lampos-tasks, so no path under $HOME is
# assumed here. jq, when present, gives the timeline its summaries (state,
# tool count, error); without it the timeline shows what the line says of
# them. Works with macOS's bash 3.2 and Linux's bash 5.
set -uo pipefail

HARNESS_BIN=${HARNESS_BIN:-harness}
SHOW_EVERY_MS=${HARNESS_RECORD_SHOW_MS:-2000}
PS_EVERY_MS=${HARNESS_RECORD_PS_MS:-5000}
WATCH_AGAIN_MS=${HARNESS_RECORD_WATCH_AGAIN_MS:-2000}
GRACE_S=${HARNESS_RECORD_GRACE:-30}

usage() {
  echo "usage: harness-record.sh record NAME SECS [--machine ID] [--agent ID] [--out DIR]" >&2
  echo "       harness-record.sh timeline DIR" >&2
  exit 2
}

# The local time in ms since 1970: bash 5's EPOCHREALTIME, GNU date, perl,
# or whole seconds as a last resort.
if [[ -n ${EPOCHREALTIME:-} ]]; then
  now_ms() { local t=${EPOCHREALTIME/,/.}; local s=${t%.*} f=${t#*.}000; echo "$s${f:0:3}"; }
elif [[ $(date +%3N 2>/dev/null) =~ ^[0-9]{3}$ ]]; then
  now_ms() { date +%s%3N; }
elif command -v perl >/dev/null 2>&1; then
  now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }
else
  now_ms() { echo "$(date +%s)000"; }
fi

# One line of JSON from what a call printed: jq -c when it parses, else the
# lines joined (pretty JSON has no raw newline inside a string).
one_line() { # one_line FILE
  local line
  if command -v jq >/dev/null 2>&1 && line=$(jq -c . <"$1" 2>/dev/null) && [[ -n $line && $line != *$'\n'* ]]; then
    printf '%s\n' "$line"
  else
    tr '\n\r' '  ' <"$1" | sed 's/  */ /g; s/ $//'
    echo
  fi
}

# --- reading -----------------------------------------------------------------
# A reader is a background subshell. USR1 asks it to stop starting calls (and
# ends its sleep, and a watch run); TERM kills its call at once.

CHILD=
STAMPER=
NAP=
STOPPING=0
CALL_START=
OUT=

put() { # put START BODY: one line, END now
  local end
  end=$(now_ms)
  printf '%s\t%s\t%s\t%s\n' "$end" "$1" "$((end - $1))" "$2" >>"$OUT"
}

reader_term() {
  [[ -n $CHILD ]] && kill "$CHILD" 2>/dev/null
  [[ -n $STAMPER ]] && kill "$STAMPER" 2>/dev/null
  [[ -n $NAP ]] && kill "$NAP" 2>/dev/null
  [[ -n $CALL_START ]] && put "$CALL_START" "!killed at end"
  exit 0
}

# Waits for PID even when a signal's trap cuts the wait short; sets RC.
wait_for() { # wait_for PID
  while :; do
    wait "$1" 2>/dev/null
    RC=$?
    kill -0 "$1" 2>/dev/null || return 0
  done
}

nap_ms() { # nap_ms MS: a sleep that USR1 or TERM ends at once
  local ms=$1
  ((ms > 0 && STOPPING == 0)) || return 0
  sleep "$((ms / 1000)).$(printf '%03d' $((ms % 1000)))" &
  NAP=$!
  wait_for "$NAP"
  NAP=
}

# Calls `harness ARGS...` every EVERY ms into OUT, one call at a time.
poll() { # poll OUT EVERY ARGS...
  OUT=$1
  local every=$2 tmp start
  shift 2
  tmp="$WORK/${OUT##*/}.call"
  trap reader_term TERM
  trap 'STOPPING=1; [[ -n $NAP ]] && kill "$NAP" 2>/dev/null' USR1
  while ((STOPPING == 0)); do
    start=$(now_ms)
    CALL_START=$start
    "$HARNESS_BIN" "$@" >"$tmp" 2>&1 </dev/null &
    CHILD=$!
    wait_for "$CHILD"
    CHILD=
    CALL_START=
    if ((RC == 0)); then
      put "$start" "$(one_line "$tmp")"
    else
      put "$start" "!rc=$RC $(one_line "$tmp")"
    fi
    nap_ms $((start + every - $(now_ms)))
  done
  exit 0
}

# Follows `harness ARGS...` (a stream) into OUT, each line stamped as it comes;
# when it ends, the end is kept and it starts again AGAIN ms after it began.
follow() { # follow OUT AGAIN ARGS...
  OUT=$1
  local again=$2 fifo run=0 start
  shift 2
  fifo="$WORK/${OUT##*/}.fifo"
  trap reader_term TERM
  trap 'STOPPING=1; [[ -n $NAP ]] && kill "$NAP" 2>/dev/null; [[ -n $CHILD ]] && kill "$CHILD" 2>/dev/null' USR1
  mkfifo "$fifo" || exit 1
  while ((STOPPING == 0)); do
    run=$((run + 1))
    start=$(now_ms)
    put "$start" "!watch run $run started"
    "$HARNESS_BIN" "$@" >"$fifo" 2>&1 </dev/null &
    CHILD=$!
    (
      trap 'exit 0' TERM
      while IFS= read -r line || [[ -n $line ]]; do put "$start" "$line"; done <"$fifo"
    ) &
    STAMPER=$!
    wait_for "$CHILD"
    CHILD=
    # Should a child of the binary hold the pipe open, the stamper is not
    # waited on for more than a second.
    local i=0
    while kill -0 "$STAMPER" 2>/dev/null && ((i < 10)); do sleep 0.1; i=$((i + 1)); done
    kill "$STAMPER" 2>/dev/null
    i=0
    while kill -0 "$STAMPER" 2>/dev/null && ((i < 5)); do sleep 0.1; i=$((i + 1)); done
    kill -9 "$STAMPER" 2>/dev/null
    wait "$STAMPER" 2>/dev/null
    STAMPER=
    if ((STOPPING)); then
      put "$start" "!stopped at end"
    else
      put "$start" "!rc=$RC watch ended"
      nap_ms $((start + again - $(now_ms)))
    fi
  done
  exit 0
}

# --- the timeline ------------------------------------------------------------

timeline() { # timeline DIR
  local dir=$1 src f
  [[ -d $dir ]] || { echo "harness-record: no folder $dir" >&2; return 1; }
  {
    for src in watch events show ps; do
      f="$dir/$src.jsonl"
      [[ -f $f ]] && awk -v s="$src" 'BEGIN { FS = OFS = "\t" }
        NF >= 4 { print $1, s, $3, substr($0, length($1) + length($2) + length($3) + 4) }' "$f"
    done
  } | sort -s -t "$(printf '\t')" -k1,1n >"$dir/.merged"

  if command -v jq >/dev/null 2>&1; then
    jq -R -r '
      def clock: (. / 1000 | floor | strflocaltime("%H:%M:%S")) + "." + ((. % 1000 + 1000) | tostring | .[1:]);
      def tools: if .tools != null then " tools=\(.tools)" else "" end;
      split("\t") as $f
      | ($f[0] | tonumber) as $ms
      | $f[1] as $src
      | $f[2] as $took
      | ($f[3:] | join("\t")) as $body
      | (if ($body | startswith("!")) then
           (([$body | capture("^!rc=(?<rc>[0-9]+) (?<rest>\\{.*)$")?] | .[0]) as $c
            | (if $c == null then null else (try ($c.rest | fromjson | .error) catch null) end) as $e
            | if $e != null then "!rc=\($c.rc) error=\($e)" else $body end)
         else (try ($body | fromjson) catch null) as $j
         | if $j == null then ($body | .[0:120])
           elif ($j | type) != "object" then ($body | .[0:120])
           elif $j.error != null then "error=\($j.error)"
           elif $src == "ps" then
             ($j.harnesses // []) as $h
             | "\($h | length) harnesses: "
               + ([$h[].state // "?"] | group_by(.) | map("\(.[0])=\(length)") | join(" "))
           elif $src == "watch" or $src == "events" then
             "\($j.event // "?") state=\($j.state // "?")" + ($j | tools)
           else "state=\($j.state // "?")" + ($j | tools)
           end
         end) as $sum
      | "\($ms | clock)  \($ms)  \($src)\t\($sum)"
        + (if $src == "show" or $src == "ps" then "  (took \($took) ms)" else "" end)
    ' <"$dir/.merged" >"$dir/timeline.txt" 2>/dev/null || timeline_plain "$dir"
  else
    timeline_plain "$dir"
  fi
  rm -f "$dir/.merged"
}

# Without jq: the time, the source and what the line says of event, state,
# tools and error.
timeline_plain() { # timeline_plain DIR
  awk 'BEGIN { FS = "\t" }
    function grab(text, re,   m) {
      if (match(text, re)) { m = substr(text, RSTART, RLENGTH); gsub(/[" ]/, "", m); sub(/:/, "=", m); return m }
      return ""
    }
    {
      body = substr($0, length($1) + length($2) + length($3) + 4)
      if (substr(body, 1, 1) == "!") {
        sum = body
        e = grab(body, "\"error\": *\"[A-Za-z_]*\"")
        if (e != "" && substr(body, 1, 4) == "!rc=") sum = substr(body, 1, index(body, " ")) e
      } else {
        sum = grab(body, "\"error\": *\"[A-Za-z_]*\"")
        if (sum == "")
          sum = grab(body, "\"event\": *\"[a-z_]*\"") " " grab(body, "\"state\": *\"[a-z_]*\"") " " grab(body, "\"tools\": *[0-9]+")
        gsub(/^ +| +$/, "", sum)
        if (sum == "") sum = substr(body, 1, 120)
      }
      took = ($2 == "show" || $2 == "ps") ? "  (took " $3 " ms)" : ""
      print $1 "  " $2 "\t" sum took
    }' "$dir/.merged" >"$dir/timeline.txt"
}

# --- recording ---------------------------------------------------------------

READERS=
MAIN_NAP=
kill_readers() {
  local pid
  for pid in $READERS; do kill "$pid" 2>/dev/null; done
  for pid in $READERS; do wait "$pid" 2>/dev/null; done
  READERS=
  [[ -n $MAIN_NAP ]] && kill "$MAIN_NAP" 2>/dev/null
  MAIN_NAP=
}

# Asks the readers to stop and gives a call still running GRACE_S seconds.
stop_readers_gently() {
  local pid alive deadline
  for pid in $READERS; do kill -USR1 "$pid" 2>/dev/null; done
  deadline=$(($(now_ms) + GRACE_S * 1000))
  while :; do
    alive=0
    for pid in $READERS; do kill -0 "$pid" 2>/dev/null && alive=1; done
    ((alive && $(now_ms) < deadline)) || break
    sleep 0.2
  done
  kill_readers
}

record() {
  local name=${1:-} secs=${2:-} machine='' agent='' out=${HARNESS_RECORD_DIR:-./harness-records}
  [[ -n $name && $name != */* && $name != .* ]] || usage
  [[ $secs =~ ^[0-9]+$ && $GRACE_S =~ ^[0-9]+$ ]] || usage
  shift 2
  while [[ $# -gt 0 ]]; do
    case $1 in
      --machine) machine=${2:-}; [[ -n $machine ]] || usage; shift 2 ;;
      --agent) agent=${2:-}; [[ -n $agent ]] || usage; shift 2 ;;
      --out) out=${2:-}; [[ -n $out ]] || usage; shift 2 ;;
      *) usage ;;
    esac
  done
  command -v "$HARNESS_BIN" >/dev/null 2>&1 || { echo "harness-record: no $HARNESS_BIN" >&2; exit 1; }

  DIR="$out/$name"
  mkdir -p "$DIR" || exit 1
  WORK=$(mktemp -d "${TMPDIR:-/tmp}/harness-record.XXXXXX") || exit 1
  local on=()
  [[ -n $machine ]] && on=(--machine "$machine")

  local finished=0
  finish() { # finish CODE
    trap '' INT TERM
    kill_readers
    if ((finished == 0)); then
      finished=1
      timeline "$DIR"
      rm -rf "$WORK"
      echo "harness-record: $DIR"
    fi
    exit "$1"
  }
  trap 'finish 130' INT
  trap 'finish 143' TERM
  trap 'kill_readers; rm -rf "$WORK"' EXIT
  local end_ms left
  end_ms=$(($(now_ms) + secs * 1000))

  if [[ -n $agent ]]; then
    follow "$DIR/watch.jsonl" "$WATCH_AGAIN_MS" watch ${on[@]+"${on[@]}"} "$agent" --json &
    READERS="$READERS $!"
    poll "$DIR/show.jsonl" "$SHOW_EVERY_MS" show ${on[@]+"${on[@]}"} "$agent" --json &
    READERS="$READERS $!"
  fi
  poll "$DIR/ps.jsonl" "$PS_EVERY_MS" ps ${on[@]+"${on[@]}"} --json &
  READERS="$READERS $!"
  # An events verb, if a later Harness has one: followed like watch.
  if "$HARNESS_BIN" --help 2>/dev/null | grep -Eq '^[[:space:]]*harness events( |$)'; then
    follow "$DIR/events.jsonl" "$WATCH_AGAIN_MS" events ${on[@]+"${on[@]}"} ${agent:+"$agent"} --json &
    READERS="$READERS $!"
  fi

  # The time is counted from before the readers started.
  left=$((end_ms - $(now_ms)))
  ((left < 0)) && left=0
  sleep "$((left / 1000)).$(printf '%03d' $((left % 1000)))" &
  MAIN_NAP=$!
  wait "$MAIN_NAP"
  MAIN_NAP=
  stop_readers_gently
  finish 0
}

case ${1:-} in
  record) shift; record "$@" ;;
  timeline) [[ $# -eq 2 ]] || usage; timeline "$2" && echo "$2/timeline.txt" ;;
  *) usage ;;
esac
