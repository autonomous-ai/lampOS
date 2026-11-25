#!/usr/bin/env bash
# The lamp check for #8467 (#7569): after a cut-in the heard line carries none of the lamp's own words, and the
# person's first words stay. A yield's request now begins in the line's own stretch (a pause as long as the one that ends a
# line, 600 ms), and the second route strips the lamp's own words from the front of a line begun over its voice or in its
# tail (LAMP_SECOND_OWN_ECHO, hyphens split on both sides). Both are on in the product. Run from the played kit, beside
# the lamp:
#
#   LAMP_HOST=<lamp> PLAYED_VOICE=<a macOS say voice> lampOS/tools/played/cut-in-words.sh LABEL [ROUNDS]
#
# It changes NOTHING on the lamp (no drop-in, no restart): it only reads the voice's session events. It REFUSES, with
# nothing played, unless the voice runs as the product's does: --turn-taking duplex, --end-silence-ms 600, and
# LAMP_FIRST_WORDS=kept, LAMP_REPLY_END_REACH=on, LAMP_SOFT_START=on, LAMP_SECOND_OWN_ECHO=on in its environment. It prints
# the voice binary's SHA-256 so the row says which build it ran on; the build must carry #8467 (a build made after its merge).
#
# Three legs, ROUNDS rounds each (default 2), each a scripted voice from this Mac, after the lamp has heard and said nothing
# for 45 s (the kit's quiet gate):
#   A  the request-13 cut-in: "Hey Lamp, tell me a long story about a lighthouse keeper.", then 10 s into the story,
#      over it, "Lamp, make a skill called tech news."
#   B  a real pause: the same story, then at 10 s "Lamp, stop.", then 0.9 s of quiet (more than the 600 ms that ends a line),
#      then "Lamp, make a skill called tech news."
#   C  the person starting on the lamp's last word: a short question, "Hey Lamp, how many steps does the lighthouse have?",
#      and the line begun at a wait after it that grows each round (3.0 s, 3.6 s, 4.2 s, ...). A round counts as C only
#      when the line began within 0.5 s of the lamp's last sound (the voice's speech_final_sample_retired); others are not trials.
#   D  the person correcting a time the lamp just said (#8486's P1, the first builder's 10:12): "Hey Lamp, please say exactly:
#      your alarm is set for 7:30 tomorrow.", then 6 s later (as or just after the lamp finishes) "No, for 7." A round passes
#      when the heard line still has the person's correction whole: it begins with "no" and keeps "for" and "7" or "seven"
#      (a build with the own-echo strip's old end rule sends "No," alone). Its own-words check is not used: "for 7" is the
#      lamp's own words, said by the person.
# What passes a trial (A, B, C): the heard line (the first input transcript after the line began) has no run of two or more words
# in a row the lamp said (its output transcripts before it), and begins with the line's own first words, "make a" (a
# leading "lamp" ignored). The heard lines are this kit's own scripted line, not a person's; they are printed.
#
# It plays sound in the room: about 1.5 minutes a round. Not between 12:00 and 14:00 on weekdays (the kit's rule).
# Settings: as the kit's (README.md); LAMP_SSH, CUT_IN_SCENARIO and CUT_IN_QUIET_GATE replace this kit's lamp_ssh,
# scenario.py and quiet_gate.sh (the stand-in test, cut-in-words-test.sh); CUT_IN_QUIET_MAX (600 s) is the longest wait
# for a quiet lamp before a round.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE" || exit 9
# shellcheck source=settings.sh
. ./settings.sh
LAMP_SSH=${LAMP_SSH:-$HERE/lamp_ssh}
SCENARIO=${CUT_IN_SCENARIO:-$HERE/scenario.py}
QUIET=${CUT_IN_QUIET_GATE:-$HERE/quiet_gate.sh}
QUIET_MAX=${CUT_IN_QUIET_MAX:-600}
OUT=$PLAYED_OUT/scenarios
LABEL=${1:-}
ROUNDS=${2:-2}
VOICE=${PLAYED_VOICE:-}

if [[ -z $LABEL || ! $ROUNDS =~ ^[1-9]$ ]]; then
  echo "usage: cut-in-words.sh LABEL [ROUNDS, 1 to 9]" >&2
  exit 2
fi
if [[ -z $VOICE ]]; then
  echo "cut-in-words.sh: PLAYED_VOICE is not set: name a macOS say voice (say -v '?' lists them). Nothing was played." >&2
  exit 2
fi
if [[ $(date +%u) -le 5 && $((10#$(date +%H))) -ge 12 && $((10#$(date +%H))) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "cut-in-words.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi

lamp() { "$LAMP_SSH" "$@" </dev/null; }

# The voice as the product runs it: nothing is changed here, so nothing to put back.
unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') \
  || { echo "cut-in-words.sh: no answer from the lamp" >&2; exit 3; }
[[ $unit == *ActiveState=active* ]] || { echo "cut-in-words.sh: lamp-voice is not active; nothing played" >&2; exit 3; }
for need in "--turn-taking duplex" "--end-silence-ms 600" LAMP_FIRST_WORDS=kept LAMP_REPLY_END_REACH=on LAMP_SOFT_START=on LAMP_SECOND_OWN_ECHO=on; do
  [[ $unit == *"$need"* ]] || { echo "cut-in-words.sh: lamp-voice does not run as the product does ('$need' is missing); nothing played" >&2; exit 3; }
done
bin=$(sed -n 's/.*ExecStart={ path=\([^ ;]*\).*/\1/p' <<<"$unit" | head -n 1)
sha=$(lamp "sudo -n sha256sum $bin" | cut -c1-64)
mkdir -p "$OUT"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT TERM HUP
report=$OUT/cut-in-words-$LABEL.txt
: >"$report"
echo "cut-in-words.sh: lamp-voice ${bin:-?} sha256 ${sha:-unreadable}; nothing on the lamp is changed" | tee -a "$report"

quiet() { # until the lamp has heard and said nothing for 45 s
  local waited=0
  until "$QUIET" 45 >/dev/null 2>&1; do
    ((waited >= QUIET_MAX)) && { echo "cut-in-words.sh: the lamp was not quiet for 45 s within ${QUIET_MAX} s; stopped" >&2; exit 5; }
    sleep 10
    waited=$((waited + 10))
  done
}

# shellcheck disable=SC2016 # expanded on the lamp, not here
READ='S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"'
tally=$work/tally.tsv
: >"$tally"
LINE="Lamp, make a skill called tech news."
STORY="Hey Lamp, tell me a long story about a lighthouse keeper."
SHORT="Hey Lamp, how many steps does the lighthouse have?"

for leg in A B C D; do
  for i in $(seq 1 "$ROUNDS"); do
    quiet
    name=$LABEL-words-$leg-$i
    case $leg in
      A) steps=("$VOICE|$STORY|10" "$VOICE|$LINE|20") ;;
      B) steps=("$VOICE|$STORY|10" "$VOICE|Lamp, stop.|0.9" "$VOICE|$LINE|20") ;;
      D) steps=("$VOICE|Hey Lamp, please say exactly: your alarm is set for 7:30 tomorrow.|6" "$VOICE|No, for 7.|20") ;;
      C) wait=$(python3 -c "print(round(3.0 + 0.6 * ($i - 1), 1))")
         steps=("$VOICE|$SHORT|$wait" "$VOICE|$LINE|20") ;;
    esac
    "$SCENARIO" "$name" "${steps[@]}" >/dev/null 2>&1
    # shellcheck disable=SC2012 # the kit names its own scene files: no odd characters
    scene=$(ls -t "$OUT"/*-"$name".txt 2>/dev/null | head -n 1)
    lamp "$READ" >"$work/events.jsonl"
    python3 - "$leg" "$i" "${scene:-}" "$work/events.jsonl" "$tally" <<'PY' | tee -a "$report"
import json, re, sys
leg, i, scene, events, tally = sys.argv[1:6]
plays = []
if scene:
    for line in open(scene, errors='replace'):
        if line.startswith('#PLAY_US '):
            plays.append(int(line.split()[1]))
def say(text, ok='', verdict=''):
    print('%s round %s: %s' % (leg, i, text))
    with open(tally, 'a') as f: f.write('\t'.join([leg, ok, verdict]) + '\n')
need = {'A': 2, 'B': 3, 'C': 2, 'D': 2}[leg]
if len(plays) != need:
    say('not played whole (%d of %d lines); not a trial' % (len(plays), need)); sys.exit()
line_at = plays[-1]
rows = []
for l in open(events, errors='replace'):
    try: rows.append(json.loads(l))
    except ValueError: pass
rows.sort(key=lambda r: (r.get('at_us', 0), r.get('service_sequence', 0)))
def words(t): return re.findall(r"[a-z0-9']+", (t or '').lower())
heard = next((r for r in rows if r.get('kind') == 'transcript' and r.get('role') == 'input'
              and line_at <= r.get('at_us', 0) < line_at + 15_000_000), None)
if heard is None:
    say('the line was not heard (no input transcript within 15 s); FAIL', 'yes', 'FAIL'); sys.exit()
text = (heard.get('text') or '').strip()
lamp_said = []
for r in rows:
    if r.get('kind') == 'transcript' and r.get('role') != 'input' and r.get('at_us', 0) < line_at + 15_000_000:
        lamp_said += words(r.get('text'))
h = words(text)
if leg == 'D':
    kept = h[:1] == ['no'] and 'for' in h and ('7' in h or 'seven' in h)
    say('heard "%s": the correction %s%s' % (text[:90], 'whole' if kept else 'CUT', '  PASS' if kept else '  FAIL'),
        'yes', 'PASS' if kept else 'FAIL'); sys.exit()
own = next((' '.join(h[k:k + 2]) for k in range(len(h) - 1)
            if any(lamp_said[j:j + 2] == h[k:k + 2] for j in range(len(lamp_said) - 1))), None)
first = h[1:] if h[:1] == ['lamp'] else h
kept = first[:2] == ['make', 'a']
if leg == 'C':
    end = next((r.get('at_us') for r in rows if r.get('kind') == 'speech_final_sample_retired'
                and plays[0] <= r.get('at_us', 0) <= line_at + 2_000_000), None)
    if end is None:
        say('the lamp\'s answer has no last sound before the line; not a trial'); sys.exit()
    gap = (line_at - end) / 1e6
    if abs(gap) > 0.5:
        say('the line began %+.1f s from the lamp\'s last sound, not within 0.5 s; not a trial' % gap); sys.exit()
how = ('own words "%s"' % own) if own else 'none of the lamp\'s words'
ok = (own is None) and kept
say('heard "%s": %s, %s%s' % (text[:90], how, 'first words kept' if kept else 'first words NOT kept',
                              '  PASS' if ok else '  FAIL'), 'yes', 'PASS' if ok else 'FAIL')
PY
    sleep 5
  done
done

python3 - "$tally" <<'PY' | tee -a "$report"
import sys
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1])]
for leg in 'ABCD':
    mine = [r for r in rows if r[0] == leg]
    ok = [r for r in mine if r[1] == 'yes']
    print('%s: trials %d of %d; pass %d, fail %d' % (leg, len(ok), len(mine),
          sum(r[2] == 'PASS' for r in ok), sum(r[2] == 'FAIL' for r in ok)))
PY
echo "cut-in-words.sh: the report is $report; each scene in full beside it"
