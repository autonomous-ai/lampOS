#!/usr/bin/env bash
# score.sh OUT_DIR [--split held|tune|all]: scores a replay.sh run against the labels in lines.txt and
# prints Markdown for the issue. It prints ids and classes only, never what was heard.
# Join per play: Jev's decision is the first attention decision whose utterance ended (at_unix_ms -
# took_ms - waited_ms) in [start, end + 3 s]: the talk before a line, decided after it began, and the
# lamp's own answer heard back are not the line's. The voice's verdict is the one for that decision's
# request, else the first verdict in [start, end + 25 s] not for an earlier utterance's request, else
# what replay.sh saw. `failed` in the log is a kind (timed_out, ...), not a boolean. Taking the turn: Jev answer|look_up, voice played; correct = takes iff label is lamp.
# A missing or failed Jev decision is always wrong, on "not" lines too.
# plays.tsv: `pass id lamp_start_ms lamp_end_ms voice_seen macvol [room_read [room_at_ms]]` (or the old `id ...` 4
# columns, pass 1). Each (pass, id) is one trial. Busy-room lines are scored as "background talk", in
# whatever room the lamp read: busy is not certain (the 30/10 run never read it, the 31/10 run on 8999 did,
# crates/voice/src/room_told.rs). A trial is clean unless a quiet-room line's context.room is not quiet,
# or its decision was in_conversation (the lamp spoke within 20 s) on a line that is not a follow_up (a
# follow_up is in conversation by design).
# --split filters the class, latency, consistency and per-line tables (default all; the per-line table,
# which shows probabilities, defaults to tune so held lines' are not seen); totals always show held and
# all. Median and p90 are nearest-rank. Intervals are 95% Wilson. Hello and progress decisions in the
# window (shadow, --hello --progress) are counted by choice: the set has no arrival and hands no work over.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
OUT=${1:?usage: score.sh OUT_DIR [--split held|tune|all]}; shift
SPLIT=all LINES=tune
case "${1:-}" in --split) SPLIT=${2:-} LINES=${2:-} ;; "") ;; *) echo "usage: score.sh OUT_DIR [--split held|tune|all]" >&2; exit 2 ;; esac
case $SPLIT in held|tune|all) ;; *) echo "split must be held, tune or all" >&2; exit 2 ;; esac
[ -s "$OUT/plays.tsv" ] || { echo "no $OUT/plays.tsv" >&2; exit 1; }
or_null() { [ -f "$1" ] && echo "$1" || echo /dev/null; }

jq -nr --arg split "$SPLIT" --arg lines "$LINES" \
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
def wilson($x; $n): if $n == 0 then [null, null] else ($x / $n) as $p | 3.8416 as $z2
  | ($z2 / $n) as $k | (($p * (1 - $p) / $n + $z2 / (4 * $n * $n)) | sqrt * 1.96) as $w
  | [($p + $k / 2 - $w) / (1 + $k), ($p + $k / 2 + $w) / (1 + $k)] end;
def rate($x; $n): wilson($x; $n) as $w
  | "\($x)/\($n) (\(if $n == 0 then "-" else $x / $n | f2 end), 95% \($w[0] | f2)-\($w[1] | f2))";
def flips(f): group_by(.id) | map(select(length > 1)) | "\(map(select(map(f) | unique | length > 1)) | length)/\(length)";

($labels | rows_of | map(select(startswith("#") | not) | split("|")
  | {id: .[0], split: .[1], room: .[2], class: .[3], label: .[4]})) as $L
| ($L | to_entries | map({key: .value.id, value: (.value + {ord: .key})}) | from_entries) as $lab
| ($dec | rows_of | map(fromjson? // empty)) as $DA
| ($DA | map(select(.about == "attention")
  | . + {ended: (.at_unix_ms - (.took_ms // 0) - (.waited_ms // 0))}) | sort_by(.at_unix_ms)) as $D
| ($voice | rows_of | map(capture("^(?<t>[0-9.]+) .*request (?<r>[0-9]+): its answer is (?<v>played|held back)")? // empty
  | {t: ((.t | tonumber) * 1000 | floor), r: (.r | tonumber), v: (if .v == "played" then "played" else "held" end)})) as $V
| ($plays | rows_of) as $pr | ($pr[0] | split("\t") | to_entries | map({key: .value, value: .key}) | from_entries) as $h
| ($pr[1:] | map(split("\t") | {pass: (if $h.pass == null then "1" else .[$h.pass] end), id: .[$h.id],
    a: (.[$h.lamp_start_ms] | tonumber), b: (.[$h.lamp_end_ms] | tonumber), seen: .[$h.voice_seen],
    rr: (if $h.room_read == null then null else .[$h.room_read] end)})) as $P
| ($P | map(select($lab[.id].room == "busy" and .rr == "busy")) | length) as $busyread
| [$P[] | . as $p | $lab[$p.id] as $l | select($l != null)
  | [$D[] | select(.ended >= $p.a and .ended <= $p.b + 3000)] as $in
  | ($in[0] // null) as $d
  | ($d.request // null) as $req
  | [$D[] | select(.ended < $p.a and .request != null) | .request] as $before
  | (if $d == null then "missing" elif $d.failed != null then "failed" else ($d.judgement.choice // "failed") end) as $c
  | ((if $req != null then first($V[] | select(.r == $req) | .v) else empty end)
     // first($V[] | select(.t >= $p.a and .t <= $p.b + 25000) | select(.r as $r | $before | index($r) | not) | .v)
     // $p.seen) as $v
  | ($d.context.room // null) as $room
  | $l + {pass: $p.pass, choice: $c, ndec: ($in | length), conv: $d.in_conversation, room_seen: $room,
      clean: (($l.class == "follow_up" or $d.in_conversation != true) and ($room == null or $room == $l.room or $l.room == "busy")), req: $req, v: $v,
      prob: (if $c == "failed" then null else $d.judgement.probability end),
      lat: (if $d == null then null else $d.at_unix_ms - $p.b end), took: $d.took_ms,
      jt: ($c == "answer" or $c == "look_up"), vt: ($v == "played")}
  | . + {jc: ((.choice != "missing" and .choice != "failed") and (.jt == (.label == "lamp"))), vc: (.vt == (.label == "lamp"))}] as $all
| ([$P[] | select($lab[.id] == null)] | length) as $unknown
| ($all | map(select($split == "all" or .split == $split))) as $R
| def totals($name; $which): map(select(($name == "all" or .split == $name) and ($which == "all" or .clean)))
    as $s | ($s | map(select(.class == "desk_talk"))) as $t
    | [$name, $which, ($s | length), ($s | frac(.jc)), ($s | frac(.vc)), ($s | cnt(.choice == "failed")),
       ($t | cnt(.choice == "answer")), ($t | cnt(.jt)), ($t | cnt(.vt)), ($t | length)] | row;
"## Jev vs voice (split: \($split))", "",
"| class | room | n | label | answer | look_up | stay_quiet | failed | missing | Jev correct | voice played | held | none | voice correct |",
"|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
($R | group_by([.class, .room]) | sort_by(map(.ord) | min)[]
  | [.[0].class, (if .[0].room == "busy" then "background talk" else .[0].room end), length, (map(.label) | unique | join("/")),
     cnt(.choice == "answer"), cnt(.choice == "look_up"), cnt(.choice == "stay_quiet"), cnt(.choice == "failed"),
     cnt(.choice == "missing"), frac(.jc), cnt(.v == "played"), cnt(.v == "held"), cnt(.v == "none"), frac(.vc)] | row),
"", "### Totals", "",
"| split | trials | n | Jev correct | voice correct | Jev failed | desk talk: Jev answer | desk talk: Jev answer+look_up | desk talk: voice played | desk talk n |",
"|---|---|---|---|---|---|---|---|---|---|",
($all | (["held", "all"] + (if $split == "tune" then ["tune"] else [] end))[] as $n | totals($n; "all"), totals($n; "clean")),
"", "In conversation (lamp spoke within 20 s): \($all | cnt(.conv == true)) trials. Room seen not the label room: \($all | cnt(.room_seen != null and .room_seen != .room)) trials. Clean: \($all | cnt(.clean))/\($all | length). Background talk: the room as the lamp read it; read busy before \($busyread) of them. Trials with 2+ attention decisions (the line heard in parts; the first is scored): \($all | cnt(.ndec > 1)).",
"", "Consistency across passes (ids played 2+ times, split \($split)): Jev take flipped on \($R | flips(.jt)) ids, voice on \($R | flips(.vt)) ids.",
"", "### Latency (ms)", "",
"| measure | median | p90 | max | n |", "|---|---|---|---|---|",
($R | map(.lat | select(. != null)) | ["decision after line end", pct(0.5), pct(0.9), max, length] | row),
($R | map(.took | select(. != null)) | ["took_ms", pct(0.5), pct(0.9), max, length] | row),
"", "### Per line (split: \($lines))", "",
"| pass | id | split | room | class | label | Jev | prob | voice | request | in_conv | room_seen |", "|---|---|---|---|---|---|---|---|---|---|---|---|",
($all[] | select($lines == "all" or .split == $lines) | [.pass, .id, .split, .room, .class, .label, .choice, (.prob | f2), .v, .req, .conv, .room_seen] | row),
(if $unknown > 0 then "", "\($unknown) played ids are not in lines.txt and were not scored." else empty end),
"", "### Decision inputs (held, clean trials)", "",
($all | map(select(.split == "held" and .clean)) | length as $n | cnt(.jc) as $j | cnt(.vc) as $v
  | (wilson($j; $n)[0]) as $lo
  | "- Jev correct: \(rate($j; $n))", "- voice correct: \(rate($v; $n))", "- margin (Jev - voice): \($j - $v) trials",
    "- desk talk false answers new for Jev (Jev took, voice held back): \(cnt(.class == "desk_talk" and .jt and (.vt | not)))",
    "", "Jev beats the voice: \(if $n > 0 and $j > $v and $lo > ($v / $n) then "yes" else "not shown" end)"),
"", "### Hello and progress (shadow)", "",
(["hello", "progress"][] as $q | $DA | map(select(.about == $q)) as $x
  | "- \($q): \($x | length) decisions\(if ($x | length) == 0 then " (none asked: thin, nothing to score)" else ": " + ($x | group_by(if .failed != null then "failed" else .judgement.choice // "failed" end)
      | map("\(.[0] | if .failed != null then "failed" else .judgement.choice // "failed" end) \(length)") | join(", ")) end)")
'
