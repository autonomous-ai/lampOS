#!/usr/bin/env bash
# Where the wait from the end of speech to the first sound went, stage by
# stage, for each turn of one session (the first builder, 22 Oct: two
# lamp-4ace sessions whose day lines' median went from 1.59 s to 2.06 s, and
# which stage grew). Times only, from the voice's own session records; never
# a word heard or said. Run it where the records are, on the lamp or on a
# copy (one with its text fields blanked does as well):
#
#   lampOS/tools/day-stages.sh [--turns 3,5,7] [--last N] SESSION [SESSION_B]
#
# SESSION is a session's folder (/var/lib/lampOS/voice/sessions/<id>), its
# `metadata/` folder, or one `events-*.jsonl` file. A folder's files are read
# in name order. With two, each gets its table, then one line per stage: the
# second's median less the first's.
#
#   --turns 3,5,7  keep only these turns (the day lines)
#   --last N       keep only the last N turns (after --turns)
#
# Of each record only `kind` and the timing fields below are taken (jq builds
# a new object of those fields; nothing else reaches the table):
#   local_endpoint      the line's end, the decision that the person had
#                       finished (coordinator.rs, `AdmissionStep::End`):
#                       `at_us`, `last_block_host_read_us` (the last block
#                       read), `includes_silence_wait_ms` (the pause it
#                       waited, endpoint-detection.md: "the actual pause"),
#                       `early_end.before_this_ms` (the line was sent to the
#                       route that long before the decision). One with
#                       `not_speech` or `onboarding_near_call` ends its turn
#                       unanswered and is no line.
#   answer_stages       the second route's own count (`from: line_end`,
#                       second_provider.rs `Times`, from the End as the route
#                       got it): `decided_at_us` (the decision it goes with),
#                       `words_ms`, `cue_ms`, `chat_first_ms`.
#   answer_first_audio  `after_decision_ms`: the answer's first audio, either
#                       route, after the decision (coordinator.rs
#                       `AnswerAudio`, never a wait cue).
#   speaker_first_write `at_us`, the earliest of the turn: the first sound
#                       handed to the speaker (a wait cue counts).
# Every time is the lamp's monotonic clock; turns count afresh each run, so
# a session is one run.
#
# The columns, in ms:
#   wait   end of speech to line end: the last block read less the pause,
#          to the decision (`at_us`)
#   words  speech to text's words, after the line end
#   cue    a wait cue began, after the line end
#   chat   the chat's first event, after the line end
#   audio  the answer's first audio, after the line end
#   sound  the first sound written, after the end of speech
# words, cue and chat are the route's count less `early_end.before_this_ms`,
# so every "after the line end" is after the decision; a row whose line went
# early is marked `e`. A stage a turn never reached is `-`. A turn with no
# line end (the lamp's own line) is no row. Under the rows: each column's
# median and p90 (nearest rank), with how many turns had it.
set -euo pipefail

usage() {
  echo "usage: day-stages.sh [--turns 3,5,7] [--last N] SESSION [SESSION_B]" >&2
  exit 2
}

turns="" last=""
files=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --turns) [[ $# -ge 2 && $2 =~ ^[0-9]+(,[0-9]+)*$ ]] || usage; turns=$2; shift 2 ;;
    --last) [[ $# -ge 2 && $2 =~ ^[1-9][0-9]*$ ]] || usage; last=$2; shift 2 ;;
    -h | --help) usage ;;
    -*) usage ;;
    *) files+=("$1"); shift ;;
  esac
done
[[ ${#files[@]} -ge 1 && ${#files[@]} -le 2 ]] || usage
command -v jq >/dev/null || { echo "day-stages.sh: needs jq" >&2; exit 2; }

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

# The events files of SESSION, one a line, in name order.
event_files() {
  local at=$1
  if [[ -f $at ]]; then
    echo "$at"
  elif [[ -d $at/metadata ]]; then
    find "$at/metadata" -maxdepth 1 -type f -name 'events-*.jsonl' | sort
  elif [[ -d $at ]]; then
    find "$at" -maxdepth 1 -type f -name 'events-*.jsonl' | sort
  fi
}

# One TSV row per turn: turn, early mark, wait, words, cue, chat, audio, sound.
rows() {
  local list=() one
  while IFS= read -r one; do list+=("$one"); done < <(event_files "$1")
  [[ ${#list[@]} -gt 0 ]] || { echo "day-stages.sh: no events-*.jsonl in $1" >&2; exit 1; }
  # grep only narrows the lines read; jq keeps the named kinds and fields.
  { grep -hE '"kind": ?"(local_endpoint|answer_stages|answer_first_audio|speaker_first_write)"' \
    "${list[@]}" || true; } |
    jq -cR 'fromjson? | objects
      | select(.kind == "local_endpoint" or .kind == "answer_stages"
               or .kind == "answer_first_audio" or .kind == "speaker_first_write")
      | select((.turn | type) == "number")
      | {kind, turn, at_us, last_block_host_read_us, includes_silence_wait_ms,
         early_ms: .early_end.before_this_ms?,
         no_line: (.not_speech != null or .onboarding_near_call != null),
         decided_at_us, words_ms, cue_ms, chat_first_ms, after_decision_ms}' |
    jq -rs --arg turns "$turns" --arg last "$last" '
      def ms(us): if us == null then null else ((us / 1000) + 0.5 | floor) end;
      def less(a; b): if a == null or b == null then null else a - b end;
      def cell: if . == null then "-" else tostring end;
      group_by(.turn) | map(
        . as $all
        | ($all | map(select(.kind == "answer_stages")) | last) as $st
        | ($all | map(select(.kind == "local_endpoint" and (.no_line | not)))) as $ends
        # The decision the stages go with, else the last, as AnswerAudio keeps.
        | (($ends | map(select(.at_us == $st.decided_at_us)) | last) // ($ends | last)) as $endpoint
        | select($endpoint != null and ($endpoint.at_us | type) == "number")
        | ($endpoint.at_us) as $line
        | (if ($endpoint.last_block_host_read_us | type) == "number"
             and ($endpoint.includes_silence_wait_ms | type) == "number"
           then $endpoint.last_block_host_read_us - $endpoint.includes_silence_wait_ms * 1000
           else null end) as $speech
        | ($endpoint.early_ms // 0) as $early
        | ($all | map(select(.kind == "speaker_first_write"
                             and (.at_us | type) == "number"
                             and .at_us >= ($speech // $line)) | .at_us) | min) as $sound
        | ($all | map(select(.kind == "answer_first_audio")
                      | .after_decision_ms | numbers) | first) as $audio
        | [ $all[0].turn,
            (if $early > 0 then "e" else "" end),
            ms(less($line; $speech)),
            less($st.words_ms; $early),
            less($st.cue_ms; $early),
            less($st.chat_first_ms; $early),
            $audio,
            ms(less($sound; $speech)) ])
      | sort_by(.[0])
      | (if $turns == "" then . else
           ($turns | split(",") | map(tonumber)) as $keep
           | map(select(.[0] as $t | $keep | index($t))) end)
      | (if $last == "" then . else .[(- ($last | tonumber)):] end)
      # A slice of an empty list is null.
      | (. // [])
      | .[] | [.[0], .[1]] + (.[2:] | map(cell)) | @tsv'
}

COLUMNS_=(wait words cue chat audio sound)

# The table of one session's rows ($1: the rows file, $2: its name).
table() {
  echo "### $2"
  echo
  echo "| turn | wait | words | cue | chat | audio | sound |"
  echo "|---|---|---|---|---|---|---|"
  awk -F'\t' '{ printf "| %s%s | %s | %s | %s | %s | %s | %s |\n", $1, ($2 == "" ? "" : " " $2), $3, $4, $5, $6, $7, $8 }' "$1"
  local median="| median |" p90="| p90 |" n="| turns |" c
  for c in 3 4 5 6 7 8; do
    read -r m p k < <(stat_of "$1" "$c")
    median+=" $m |"; p90+=" $p |"; n+=" $k |"
  done
  echo "$median"
  echo "$p90"
  echo "$n"
  echo
}

# The median, p90 (nearest rank) and count of column $2 of rows $1.
stat_of() {
  awk -F'\t' -v c="$2" '$c != "-" { print $c }' "$1" | sort -n |
    awk '{ v[++n] = $1 }
      END {
        if (!n) { print "- - 0"; exit }
        median = (n % 2) ? v[(n + 1) / 2] : (v[n / 2] + v[n / 2 + 1]) / 2
        r = int(0.9 * n); if (r < 0.9 * n) r++
        printf "%g %g %d\n", median, v[r], n
      }'
}

i=0
for f in "${files[@]}"; do
  i=$((i + 1))
  rows "$f" >"$T/rows$i"
  table "$T/rows$i" "$f"
done

if [[ ${#files[@]} -eq 2 ]]; then
  echo "### median, second less first (ms)"
  echo
  line="|"
  head="|"
  for c in 3 4 5 6 7 8; do
    read -r a _ _ < <(stat_of "$T/rows1" "$c")
    read -r b _ _ < <(stat_of "$T/rows2" "$c")
    head+=" ${COLUMNS_[c - 3]} |"
    if [[ $a == - || $b == - ]]; then line+=" - |"; else line+=" $(awk -v a="$a" -v b="$b" 'BEGIN { printf "%+g", b - a }') |"; fi
  done
  echo "$head"
  echo "|---|---|---|---|---|---|"
  echo "$line"
fi
