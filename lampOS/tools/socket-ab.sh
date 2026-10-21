#!/usr/bin/env bash
# The speech socket's lamp run (#2602, the first builder at 01:03 on 22 Oct;
# #2135's row, with #2629/#2660's socket model and #2658's dialogue
# endpoint): the played day lines twice on one lamp, the second route's speech
# over HTTP (LAMP_SECOND_SPEECH_SOCKET=off) and then with the speech socket
# opened ahead (LAMP_SECOND_SPEECH_SOCKET=on, LAMP_SECOND_SPEECH_SOCKET_MODEL),
# one pair, and each run's speech times. Run on the Mac beside the lamp, with
# the played kit's settings (tools/played/README.md: LAMP_HOST, LAMP_SSH_KEY,
# LAMP_USER, PLAYED_OUT) and the lamp's user able to `sudo -n`:
#
#   lampOS/tools/socket-ab.sh LABEL [--model eleven_flash_v2_5] [--arms off,on]
#
#   LABEL     names the run's files (new each time, e.g. the time)
#   --model   the socket's model for "on" (eleven_flash_v2_5; eleven_v4_turbo
#             takes the dialogue endpoint with #2658)
#   --arms    which runs, in order (off,on); one may be run again alone
#
# Each run: a drop-in under /run sets the switch (and the model for "on") on
# lamp-voice, which restarts; the second worker's own environment is checked
# for it; then `played/day.sh` plays its twelve day lines (it waits for 45 s
# of quiet first, so the restart itself is never under someone talking).
# Before the first restart the lamp must be quiet too (`played/quiet_gate.sh`).
# On any exit, Ctrl-C included, the drop-in goes and lamp-voice restarts on its
# own unit's settings.
#
# What is read, timing only, never a word heard or said:
#   from the run's session events (/var/lib/lampOS/voice/sessions/<newest>/
#   metadata/events-*.jsonl, the voice's records, as `lamp-voice turn-times`
#   reads them): each turn's `local_endpoint` (the end of speech decided) and
#   its earliest `speaker_first_write` (the answer's first sound), by `turn`
#   and `at_us` alone;
#   from lamp-voice's journal since the restart, because the per-piece speech
#   times are journal lines only, no session event (second_provider/net/tts.rs):
#     "request N <voice> piece K: C chars, sent S ms after the words were
#      heard on the speech socket opened A ms before | on a new/reused
#      connection; its first audio byte F ms after it went"
#        each piece's request-to-first-audio, F ms, socket or HTTP;
#     "the speech socket refused MODEL"                      refused
#     "the speech socket failed before its first audio"      a fallback to HTTP
#     "the speech socket didn't open"                        a fallback to HTTP
#     "the speech socket was closed: 4429"                   the socket's 429
#     "the speech socket was closed: 44NN"                   any 44xx close
#     "a service answered HTTP 429"                          HTTP 429 (any of
#                                                            the route's services)
#     "a speech socket is opened as each answer begins"      the switch, seen on
#   The voice has no pause or back-off line for speech: a 429 is the only
#   sign of one, so none is counted apart.
#
# The summary, per run (ms; median and p90 by nearest rank on sorted times):
#   pieces            all pieces that began, socket and HTTP, and first pieces
#   socket / http     request-to-first-audio: median, p90, n
#   first sound       the answer's first sound after end of speech: median, n
#   refused, 429s, 44xx closes and fallbacks to HTTP, counted
# and, for a pair, the bar (docs/lamp-runs.md): on's first-sound median no
# later than off's; on's socket pieces' median first audio below off's HTTP
# median; 0 refused.
# Written to $PLAYED_OUT/scenarios/socket-ab-LABEL.txt; each run's raw times
# beside it (socket-ab-LABEL-ARM.times), the day's own output (it has the
# words) in socket-ab-LABEL-ARM.day.log.
#
# For a test: LAMP_SSH replaces played/lamp_ssh, DAY played/day.sh and QUIET
# played/quiet_gate.sh; on the lamp UNIT_DIR its /run/systemd/system,
# SESSIONS its /var/lib/lampOS/voice/sessions and WORKER_PROC a folder with
# the worker's `environ` instead of /proc/PID; SETTLE the seconds to wait for
# the restarted voice (20) and TAIL after the day (10).
set -uo pipefail

usage() { sed -n '2,65p' "$0" | sed 's/^# \{0,1\}//'; }

HERE=$(cd "$(dirname "$0")" && pwd)
LAMP_SSH=${LAMP_SSH:-$HERE/played/lamp_ssh}
DAY=${DAY:-$HERE/played/day.sh}
QUIET=${QUIET:-$HERE/played/quiet_gate.sh}
UNIT_DIR=${UNIT_DIR:-/run/systemd/system}
SESSIONS=${SESSIONS:-/var/lib/lampOS/voice/sessions}
SETTLE=${SETTLE:-20} TAIL=${TAIL:-10}
LABEL='' MODEL=eleven_flash_v2_5 ARMS=off,on
while [[ $# -gt 0 ]]; do
  case $1 in
    --model) MODEL=${2:-}; shift 2 ;;
    --arms) ARMS=${2:-}; shift 2 ;;
    --help) usage; exit 0 ;;
    -*) echo "socket-ab: unknown option $1" >&2; usage >&2; exit 2 ;;
    *) [[ -z $LABEL ]] || { echo "socket-ab: one LABEL" >&2; exit 2; }; LABEL=$1; shift ;;
  esac
done
[[ $LABEL =~ ^[A-Za-z0-9_.-]+$ ]] || { echo "socket-ab: a LABEL of letters, digits, . _ - is required" >&2; usage >&2; exit 2; }
[[ $MODEL =~ ^[a-z0-9_]+$ ]] || { echo "socket-ab: --model is a model id (eleven_flash_v2_5, eleven_v4_turbo)" >&2; exit 2; }
[[ $ARMS =~ ^(off|on)(,(off|on))?$ ]] || { echo "socket-ab: --arms is off, on, off,on or on,off" >&2; exit 2; }
[[ -n ${LAMP_HOST:-} ]] || { echo "socket-ab: LAMP_HOST is not set (see tools/played/README.md); nothing was changed" >&2; exit 2; }
OUT=${PLAYED_OUT:-$HOME/lamp-played}/scenarios
mkdir -p "$OUT" || exit 2
SUMMARY=$OUT/socket-ab-$LABEL.txt
: >"$SUMMARY"

say_() { echo "socket-ab: $*"; }
told() { echo "$*" | tee -a "$SUMMARY"; }
# One command on the lamp, as root.
on_lamp() { "$LAMP_SSH" "sudo -n bash -c $(printf '%q' "$1")" </dev/null; }
DROPIN=$UNIT_DIR/lamp-voice.service.d/zz-socket-ab.conf

put_back() {
  # Once, and not cut short by a second ^C or the terminal closing.
  trap '' INT TERM HUP
  trap - EXIT
  say_ "putting lamp-voice back on its own unit's settings"
  on_lamp "rm -f '$DROPIN'; systemctl daemon-reload; systemctl restart lamp-voice; sleep 2; systemctl is-active lamp-voice" \
    || echo "socket-ab: could not put lamp-voice back: remove $DROPIN on the lamp, daemon-reload and restart lamp-voice" >&2
}

# A wait the traps can cut short.
pause() {
  sleep "$1" &
  wait $!
}

# The newest session's folder name on the lamp, or nothing.
newest_session() { on_lamp "ls -1t '$SESSIONS' 2>/dev/null | head -1"; }

# The second worker's environment, one variable a line.
worker_environ() {
  on_lamp "if [ -n '${WORKER_PROC:-}' ]; then cat '${WORKER_PROC:-}/environ'; else P=\$(pgrep -f '[l]amp-voice worker second' | head -1); [ -n \"\$P\" ] && cat /proc/\$P/environ; fi" | tr '\0' '\n'
}

say_ "preflight"
state=$(on_lamp 'systemctl is-active lamp-voice') || true
[[ $state == active ]] || { echo "socket-ab: lamp-voice is not active on the lamp ($state); nothing was changed" >&2; exit 1; }
"$QUIET" 45 >/dev/null 2>&1 || { echo "socket-ab: the lamp has heard or said something in the last 45 s (or could not be read); nothing was changed, try again when it is quiet" >&2; exit 1; }
environ=$(worker_environ)
told "socket-ab $LABEL: model for on $MODEL; the lamp's own: $(grep -E '^LAMP_SECOND_SPEECH(_SOCKET|_SOCKET_MODEL|_MODEL)?=' <<<"$environ" | sort | paste -sd ' ' -)"
trap put_back EXIT
trap 'exit 130' INT TERM HUP

# The times of one run, one a line: `piece socket|http INDEX MS`, `end TURN
# AT_US`, `sound TURN AT_US`, and the journal's counts as `count NAME`.
READ_EVENTS='awk '\''/"kind":"(local_endpoint|speaker_first_write)"/ {
  k = ($0 ~ /"kind":"local_endpoint"/) ? "end" : "sound"
  if (!match($0, /"turn":[0-9]+/)) next; t = substr($0, RSTART + 7, RLENGTH - 7)
  if (!match($0, /"at_us":[0-9]+/)) next; a = substr($0, RSTART + 8, RLENGTH - 8)
  print k, t, a }'\'''
READ_JOURNAL='awk '\''
  / piece [0-9]+: [0-9]+ chars, sent [0-9]+ ms after the words were heard on / && /its first audio byte [0-9]+ ms after it went/ {
    k = ($0 ~ / on the speech socket opened [0-9]+ ms before;/) ? "socket" : "http"
    match($0, / piece [0-9]+:/); i = substr($0, RSTART + 7, RLENGTH - 8)
    match($0, /its first audio byte [0-9]+ ms/); f = substr($0, RSTART + 21, RLENGTH - 24)
    print "piece", k, i, f; next }
  /the speech socket refused / { print "count refused"; next }
  /the speech socket failed before its first audio/ { print "count fallback_failed"; next }
  /the speech socket didn.t open/ { print "count fallback_not_open"; next }
  /the speech socket was closed: 44[0-9][0-9]/ { print "count close_44xx"; if ($0 ~ /closed: 4429/) print "count socket_429"; next }
  /a service answered HTTP 429/ { print "count http_429"; next }
  /a speech socket is opened as each answer begins/ { print "count switch_on"; next }'\'''

run() {
  local arm=$1 before session since times=$OUT/socket-ab-$LABEL-$1.times env_lines
  if [[ $arm == on ]]; then
    env_lines="Environment=LAMP_SECOND_SPEECH_SOCKET=on\nEnvironment=LAMP_SECOND_SPEECH_SOCKET_MODEL=$MODEL\n"
  else
    env_lines="Environment=LAMP_SECOND_SPEECH_SOCKET=off\n"
  fi
  before=$(newest_session)
  since=$(on_lamp 'date +%s') || return 1
  on_lamp "mkdir -p '$(dirname "$DROPIN")' && printf '[Service]\n$env_lines' >'$DROPIN' && systemctl daemon-reload && systemctl restart lamp-voice" \
    || { echo "socket-ab: the drop-in could not be set for $arm" >&2; return 1; }
  # The restarted voice's own session, and its worker with the switch.
  session=''
  for _ in $(seq "$SETTLE"); do
    session=$(newest_session)
    [[ -n $session && $session != "$before" ]] && break
    pause 1
  done
  [[ -n $session && $session != "$before" ]] || { echo "socket-ab: $arm: no new voice session after the restart; nothing measured" >&2; return 1; }
  environ=$(worker_environ)
  if [[ $arm == on ]]; then
    grep -qx LAMP_SECOND_SPEECH_SOCKET=on <<<"$environ" && grep -qx "LAMP_SECOND_SPEECH_SOCKET_MODEL=$MODEL" <<<"$environ" \
      || { echo "socket-ab: on: the second worker runs without LAMP_SECOND_SPEECH_SOCKET=on and model $MODEL (not the second route?); nothing measured" >&2; return 1; }
  else
    ! grep -qx LAMP_SECOND_SPEECH_SOCKET=on <<<"$environ" \
      || { echo "socket-ab: off: the second worker still has LAMP_SECOND_SPEECH_SOCKET=on; nothing measured" >&2; return 1; }
    [[ -n $environ ]] || { echo "socket-ab: off: no second worker found (not the second route?); nothing measured" >&2; return 1; }
  fi
  say_ "$arm: session $session; playing the day lines (day.sh waits for quiet first)"
  "$DAY" "socket-ab-$LABEL-$arm" >"$OUT/socket-ab-$LABEL-$arm.day.log" 2>&1 \
    || say_ "note: $arm: day.sh ended with an error; see socket-ab-$LABEL-$arm.day.log"
  pause "$TAIL"
  on_lamp "cat '$SESSIONS/$session'/metadata/events-*.jsonl 2>/dev/null | $READ_EVENTS
journalctl -u lamp-voice --since @$since --no-pager -o cat | $READ_JOURNAL" >"$times" \
    || { echo "socket-ab: $arm: the times could not be read" >&2; return 1; }
  summarise "$arm" "$times"
}

# median and p90 of the numbers on stdin, nearest rank, as "median M, p90 P, n N".
spread() {
  sort -n | awk '{v[++n] = $1} END {
    if (!n) { print "none"; exit }
    m = int((n + 1) / 2); p = int(0.9 * n); if (p < 0.9 * n) p++
    printf "median %d, p90 %d, n %d\n", v[m], v[p], n }'
}
median_of() { sort -n | awk '{v[++n] = $1} END { if (n) print v[int((n + 1) / 2)] }'; }
count_of() { grep -c "^count $1$" "$2"; }

# Each turn's first sound after its end of speech, ms, one a line.
first_sounds() {
  awk '$1 == "end" { if (!($2 in e) || $3 < e[$2]) e[$2] = $3 }
       $1 == "sound" { s[++n] = $2 " " $3 }
       END { for (i = 1; i <= n; i++) { split(s[i], x, " "); t = x[1]; a = x[2]
               if ((t in e) && a >= e[t] && (!(t in f) || a < f[t])) f[t] = a }
             for (t in f) printf "%d\n", (f[t] - e[t]) / 1000 }' "$1"
}

summarise() {
  local arm=$1 times=$2
  local socket http all
  socket=$(awk '$1 == "piece" && $2 == "socket" {print $4}' "$times" | wc -l | tr -d ' ')
  http=$(awk '$1 == "piece" && $2 == "http" {print $4}' "$times" | wc -l | tr -d ' ')
  all=$((socket + http))
  told "$arm pieces=$all (socket $socket, http $http, first pieces $(awk '$1 == "piece" && $3 == 1' "$times" | wc -l | tr -d ' '))"
  told "$arm socket first audio=$(awk '$1 == "piece" && $2 == "socket" {print $4}' "$times" | spread)"
  told "$arm http first audio=$(awk '$1 == "piece" && $2 == "http" {print $4}' "$times" | spread)"
  told "$arm first piece first audio=$(awk '$1 == "piece" && $3 == 1 {print $4}' "$times" | spread)"
  told "$arm first sound after end of speech=$(first_sounds "$times" | spread)"
  told "$arm refused=$(count_of refused "$times") socket_429=$(count_of socket_429 "$times") http_429=$(count_of http_429 "$times") closes_44xx=$(count_of close_44xx "$times") fallbacks_to_http=$(($(count_of fallback_failed "$times") + $(count_of fallback_not_open "$times"))) (failed $(count_of fallback_failed "$times"), not opened $(count_of fallback_not_open "$times")) switch_on_line=$(count_of switch_on "$times")"
}

# The bar, when both runs were made.
bar() {
  local off=$OUT/socket-ab-$LABEL-off.times on=$OUT/socket-ab-$LABEL-on.times
  local s_off s_on p_off p_on refused verdict
  s_off=$(first_sounds "$off" | median_of) s_on=$(first_sounds "$on" | median_of)
  p_off=$(awk '$1 == "piece" && $2 == "http" {print $4}' "$off" | median_of)
  p_on=$(awk '$1 == "piece" && $2 == "socket" {print $4}' "$on" | median_of)
  refused=$(count_of refused "$on")
  verdict() { if [[ -n $2 && -n $3 ]] && (($2 $1 $3)); then echo yes; else echo no; fi; }
  told "bar first sound: on ${s_on:-none} <= off ${s_off:-none} ms: $(verdict '<=' "$s_on" "$s_off")"
  told "bar piece first audio: on's socket median ${p_on:-none} < off's http median ${p_off:-none} ms: $(verdict '<' "$p_on" "$p_off")"
  told "bar refused: $refused: $([[ $refused == 0 ]] && echo yes || echo no)"
}

for arm in ${ARMS//,/ }; do
  case $arm in
    off) say_ "off: the second route's speech over HTTP" ;;
    on) say_ "on: the speech socket opened ahead, model $MODEL" ;;
  esac
  run "$arm" || exit 1
done
[[ $ARMS == off,on || $ARMS == on,off ]] && bar
say_ "done; the summary is $SUMMARY"
