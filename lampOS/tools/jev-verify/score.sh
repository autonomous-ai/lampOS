#!/usr/bin/env bash
# score.sh OUT_DIR [--split held|tune|all]: scores a replay.sh run against the labels in lines.txt and
# prints Markdown for the issue. It prints ids and classes only, never what was heard.
# Join per play: Jev's decision is the first attention decision in [start, end + 6 s]; the voice's verdict
# is the one for that decision's request, else the first verdict in [start, end + 25 s], else what
# replay.sh saw. Taking the turn: Jev answer|look_up, voice played; correct = takes iff label is lamp.
# --split filters the class, latency and per-line tables (default all); totals always show held and all.
# Median and p90 are nearest-rank.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
OUT=${1:?usage: score.sh OUT_DIR [--split held|tune|all]}; shift
SPLIT=all
case "${1:-}" in --split) SPLIT=${2:-} ;; "") ;; *) echo "usage: score.sh OUT_DIR [--split held|tune|all]" >&2; exit 2 ;; esac
case $SPLIT in held|tune|all) ;; *) echo "split must be held, tune or all" >&2; exit 2 ;; esac
[ -s "$OUT/plays.tsv" ] || { echo "no $OUT/plays.tsv" >&2; exit 1; }
or_null() { [ -f "$1" ] && echo "$1" || echo /dev/null; }

jq -nr --arg split "$SPLIT" \
  --rawfile labels "$HERE/lines.txt" --rawfile plays "$OUT/plays.tsv" \
  --rawfile dec "$(or_null "$OUT/decisions.jsonl")" --rawfile voice "$(or_null "$OUT/voice.txt")" '
def rows_of: split("\n") | map(select(length > 0));
def dash: if . == null then "-" else tostring end;
def f2: if . == null then "-" else (. * 100 | round) as $i
  | "\($i / 100 | floor).\($i % 100 | tostring | if length < 2 then "0" + . else . end)" end;
def pct($q): sort | if length == 0 then null else .[[(($q * length) | ceil) - 1, 0] | max] end;
def frac(f): "\(map(select(f)) | length)/\(length)";
def cnt(f): map(select(f)) | length;
def row: "| " + (map(dash) | join(" | ")) + " |";

($labels | rows_of | map(select(startswith("#") | not) | split("|")
  | {id: .[0], split: .[1], room: .[2], class: .[3], label: .[4]})) as $L
| ($L | to_entries | map({key: .value.id, value: (.value + {ord: .key})}) | from_entries) as $lab
| ($dec | rows_of | map(fromjson? // empty | select(.about == "attention")) | sort_by(.at_unix_ms)) as $D
| ($voice | rows_of | map(capture("^(?<t>[0-9.]+) .*request (?<r>[0-9]+): its answer is (?<v>played|held back)")? // empty
  | {t: ((.t | tonumber) * 1000 | floor), r: (.r | tonumber), v: (if .v == "played" then "played" else "held" end)})) as $V
| ($plays | rows_of | .[1:] | map(split("\t") | {id: .[0], a: (.[1] | tonumber), b: (.[2] | tonumber), seen: .[3]})) as $P
| [$P[] | . as $p | $lab[$p.id] as $l | select($l != null)
  | (first($D[] | select(.at_unix_ms >= $p.a and .at_unix_ms <= $p.b + 6000)) // null) as $d
  | ($d.request // null) as $req
  | (if $d == null then "missing" elif $d.failed == true then "failed" else ($d.judgement.choice // "failed") end) as $c
  | ((if $req != null then first($V[] | select(.r == $req) | .v) else empty end)
     // first($V[] | select(.t >= $p.a and .t <= $p.b + 25000) | .v) // $p.seen) as $v
  | $l + {choice: $c, req: $req, v: $v,
      prob: (if $c == "failed" then null else $d.judgement.probability end),
      lat: (if $d == null then null else $d.at_unix_ms - $p.b end), took: $d.took_ms,
      jt: ($c == "answer" or $c == "look_up"), vt: ($v == "played")}
  | . + {jc: (.jt == (.label == "lamp")), vc: (.vt == (.label == "lamp"))}] as $all
| ([$P[] | select($lab[.id] == null)] | length) as $unknown
| ($all | map(select($split == "all" or .split == $split))) as $R
| def totals($name): map(select($name == "all" or .split == $name)) as $s | ($s | map(select(.class == "desk_talk"))) as $t
    | [$name, ($s | length), ($s | frac(.jc)), ($s | frac(.vc)), ($s | cnt(.choice == "failed")),
       ($t | cnt(.choice == "answer")), ($t | cnt(.jt)), ($t | cnt(.vt)), ($t | length)] | row;
"## Jev vs voice (split: \($split))", "",
"| class | room | n | label | answer | look_up | stay_quiet | failed | missing | Jev correct | voice played | held | none | voice correct |",
"|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
($R | group_by([.class, .room]) | sort_by(map(.ord) | min)[]
  | [.[0].class, .[0].room, length, (map(.label) | unique | join("/")),
     cnt(.choice == "answer"), cnt(.choice == "look_up"), cnt(.choice == "stay_quiet"), cnt(.choice == "failed"),
     cnt(.choice == "missing"), frac(.jc), cnt(.v == "played"), cnt(.v == "held"), cnt(.v == "none"), frac(.vc)] | row),
"", "### Totals", "",
"| split | n | Jev correct | voice correct | Jev failed | desk talk: Jev answer | desk talk: Jev answer+look_up | desk talk: voice played | desk talk n |",
"|---|---|---|---|---|---|---|---|---|",
($all | totals("held"), totals("all"), (if $split == "tune" then totals("tune") else empty end)),
"", "### Latency (ms)", "",
"| measure | median | p90 | max | n |", "|---|---|---|---|---|",
($R | map(.lat | select(. != null)) | ["decision after line end", pct(0.5), pct(0.9), max, length] | row),
($R | map(.took | select(. != null)) | ["took_ms", pct(0.5), pct(0.9), max, length] | row),
"", "### Per line", "",
"| id | split | room | class | label | Jev | prob | voice | request |", "|---|---|---|---|---|---|---|---|---|",
($R[] | [.id, .split, .room, .class, .label, .choice, (.prob | f2), .v, .req] | row),
(if $unknown > 0 then "", "\($unknown) played ids are not in lines.txt and were not scored." else empty end)
'
