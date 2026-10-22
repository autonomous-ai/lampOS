#!/usr/bin/env bash
# The measurement before any cut-in bound changes (#2602, "Cutting in", piece 1): how often the lamp yields to nobody
# while it tells a long story, and how fast it hears asides and cut-ins played into one. Run from the played kit:
#
#   LAMP_HOST=<lamp> lampOS/tools/played/cutin.sh LABEL [IDLE_STORIES] [ROUNDS]
#
# - (a) Idle: IDLE_STORIES (default 3) long stories asked for and let run about 100 s with nobody talking, then
#   stopped. Every duplex_yield in them is a false yield; the rate is per minute of blocks the reply had something to
#   yield (duplex_hearing's blocks), not wall time.
# - (b) Episodes: per round (default 1), an aside ("Hmm, interesting.", which should pause the story and let it go on)
#   and a cut-in ("Wait, what was the keeper's name?", which should stop it and be answered), each played 3, 8, 15
#   and 25 s into a fresh story, then "Lamp, stop. That's enough." For each: the actual offset (from the story's first
#   accepted write), onset to yield, yield to commit or resume, and for a commit, end of words to the first answer
#   audio. Each episode is its own story, so no fruitless yield of one carries into the next.
# - Reported with each (Third on #2602, 08:13): the build (the binary's sha256) and flags (the unit's LAMP_
#   environment, and the session's duplex_policy); per-block hearing in the bounded window from the yield's onset run
#   to its decision (0 still, 1 clear, 2 under, 3 own, 4 over, 5 unknown, - unjudged, . not in the journal), with its
#   frame sequences; stop-word results (stop_words, stop_heard); discontinuities (capture_gap, playback_gap,
#   capture_gap_closed, a yield ended by evidence or lineage loss, holes in the window's frame sequences) with their
#   service_sequence and the episode's frame sequences.
# - Endpoints, kept apart: onset is the lamp's clock just before the clip was started here (0.1 to 0.3 s early, so
#   every gap from it is that much long), end of words is that plus the clip's length. The yield, commit and resume
#   are the coordinator's decisions, not acoustic. "Accepted" is ALSA accepting the answer's first PCM
#   (speaker_first_write). "Acoustic" is a proxy: the first block after it that the lamp's microphone heard as its own
#   voice (duplex_hearing 3, about 10 ms a block, the trail's time counted back); "unmeasured" when there is none.
#   No event times the acoustic output itself.
#
# CUTIN_ENV, optional: space-separated NAME=VALUE pairs for lamp-voice (for example LAMP_CUT_IN_COUNTS_REAL=on), put
# on through a /run drop-in (zz-cutin-check.conf) and the voice restarted; on any exit the drop-in is removed and the
# voice restarted without it. Without CUTIN_ENV nothing on the lamp is changed: it measures the voice as it runs.
#
# It plays sound in the room, about 2 minutes a story: about 25 minutes with the defaults. Not between 12:00 and 14:00
# on weekdays. Settings: as the kit's (README.md); LAMP_SSH, CUTIN_SCENARIO and CUTIN_QUIET_GATE replace this kit's
# lamp_ssh, scenario.py and quiet_gate.sh; CUTIN_LEAD (3 s) is the wait from the ask's end to the story's first audio
# that the offsets assume; CUTIN_QUIET_MAX (600 s) is the longest wait for a quiet lamp before a story.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE" || exit 9
# shellcheck source=settings.sh
. ./settings.sh
LAMP_SSH=${LAMP_SSH:-$HERE/lamp_ssh}
SCENARIO=${CUTIN_SCENARIO:-$HERE/scenario.py}
QUIET=${CUTIN_QUIET_GATE:-$HERE/quiet_gate.sh}
QUIET_MAX=${CUTIN_QUIET_MAX:-600}
LEAD=${CUTIN_LEAD:-3}
OUT=$PLAYED_OUT/scenarios
GEN=$PLAYED_OUT/prompts/generated
DROPIN=/run/systemd/system/lamp-voice.service.d/zz-cutin-check.conf
LABEL=${1:-}
IDLE=${2:-3}
ROUNDS=${3:-1}
ASK="Hey Lamp, tell me a long story about a lighthouse keeper."
STOP="Lamp, stop. That's enough."

if [[ -z $LABEL || ! $IDLE =~ ^[0-9]$ || ! $ROUNDS =~ ^[0-9]$ ]]; then
  echo "usage: cutin.sh LABEL [IDLE_STORIES, 0 to 9] [ROUNDS, 0 to 9]" >&2
  exit 2
fi
for pair in ${CUTIN_ENV:-}; do
  [[ $pair =~ ^LAMP_[A-Z0-9_]+=[A-Za-z0-9._-]+$ ]] || { echo "cutin.sh: CUTIN_ENV: '$pair' is not LAMP_NAME=value" >&2; exit 2; }
done
if [[ $(date +%u) -le 5 && $((10#$(date +%H))) -ge 12 && $((10#$(date +%H))) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "cutin.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi

lamp() { "$LAMP_SSH" "$@" </dev/null; }

unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') \
  || { echo "cutin.sh: no answer from the lamp" >&2; exit 3; }
[[ $unit == *ActiveState=active* ]] || { echo "cutin.sh: lamp-voice is not active; nothing changed" >&2; exit 3; }
[[ $unit == *"--turn-taking duplex"* ]] \
  || { echo "cutin.sh: lamp-voice is not on --turn-taking duplex: there is nothing to yield; nothing changed" >&2; exit 3; }
bin=$(sed -n 's/.*ExecStart={ path=\([^ ;]*\).*/\1/p' <<<"$unit" | head -n 1)

mkdir -p "$OUT"
work=$(mktemp -d)
changed=no
put_back() {
  rm -rf "$work"
  [[ $changed == yes ]] || return 0
  lamp "sudo -n rm -f $DROPIN && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    && echo "cutin.sh: drop-in removed, lamp-voice restarted without it" \
    || echo "cutin.sh: putting lamp-voice back FAILED: remove $DROPIN, daemon-reload and restart lamp-voice by hand" >&2
}
trap put_back EXIT
trap 'exit 130' INT TERM HUP

if [[ -n ${CUTIN_ENV:-} ]]; then
  changed=yes
  lines=$(for pair in $CUTIN_ENV; do printf 'Environment=%s\\n' "$pair"; done)
  lamp "sudo -n mkdir -p ${DROPIN%/*} && printf '[Service]\\n$lines' | sudo -n tee $DROPIN >/dev/null && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    || { echo "cutin.sh: the drop-in could not be put on (sudo -n)" >&2; exit 4; }
  echo "cutin.sh: lamp-voice restarted with $CUTIN_ENV"
  unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') || unit=""
fi

report=$OUT/cutin-$LABEL.txt
tally=$work/tally.tsv
: >"$report"
: >"$tally"
{
  echo "cutin $LABEL  $(date -u +%Y-%m-%dT%H:%M:%SZ)  lamp $LAMP_HOST"
  echo "build: ${bin:-binary not found} sha256 $(lamp "sudo -n sha256sum ${bin:-/nonexistent} 2>/dev/null" | cut -c1-16)"
  echo "flags: $(grep -o 'LAMP_[A-Z0-9_]*=[^ ]*' <<<"$unit" | sort -u | tr '\n' ' ')"
  echo "args: $(sed -n 's/.*argv\[\]=\([^;]*\);.*/\1/p' <<<"$unit" | head -n 1)"
} | tee -a "$report"

quiet() { # until the lamp has heard and said nothing for 45 s (a voice just restarted counts as busy)
  local waited=0
  until "$QUIET" 45 >/dev/null 2>&1; do
    ((waited >= QUIET_MAX)) && { echo "cutin.sh: the lamp was not quiet for 45 s within ${QUIET_MAX} s; stopped" >&2; exit 5; }
    sleep 10
    waited=$((waited + 10))
  done
}

# shellcheck disable=SC2016 # expanded on the lamp, not here
READ='S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"'

episode() { # episode MODE OFFSET NAME LINE...: play the scene, read the events, report one line or block
  local mode=$1 offset=$2 name=$3 scene
  shift 3
  quiet
  "$SCENARIO" "$name" "$@" >/dev/null 2>&1
  # shellcheck disable=SC2012 # the kit names its own scene files: no odd characters
  scene=$(ls -t "$OUT"/*-"$name".txt 2>/dev/null | head -n 1)
  lamp "$READ" >"$work/events.jsonl"
  python3 - "$mode" "$offset" "$name" "${scene:-}" "$work/events.jsonl" "$tally" "$GEN" "${LAMP_SAY_RATE:-185}" <<'PY' | tee -a "$report"
import hashlib, json, sys, wave
mode, offset, name, scene, events, tally, gen, rate = sys.argv[1:9]
CLASSES = {'0': 'still', '1': 'clear', '2': 'under', '3': 'own', '4': 'over', '5': 'unknown', '-': 'unjudged', '.': 'absent'}
GAP_KINDS = ('capture_gap', 'capture_gap_closed', 'playback_gap')
LOST = ('evidence_discontinuity', 'capture_lineage_changed', 'hearing_expired', 'owner_changed', 'superseded_decision')
plays = []
if scene:
    for line in open(scene, errors='replace'):
        if line.startswith('#PLAY_US '):
            _, at, rest = line.rstrip('\n').split(' ', 2)
            voice, text = rest.split(': ', 1)
            plays.append((int(at), voice, text))
def dur_us(voice, text):
    key = hashlib.sha256(('%s|%s|%s' % (voice, rate, text)).encode()).hexdigest()[:16]
    try:
        with wave.open('%s/%s.wav' % (gen, key)) as w:
            return int(w.getnframes() * 1e6 / w.getframerate())
    except (OSError, EOFError, wave.Error):
        return None
def tsv(*cells):
    with open(tally, 'a') as f: f.write('\t'.join(str(c) for c in cells) + '\n')
def s(us): return '%.2f s' % (us / 1e6)
rows = []
for line in open(events, errors='replace'):
    try: r = json.loads(line)
    except ValueError: continue
    if isinstance(r, dict): rows.append(r)
rows.sort(key=lambda r: (r.get('at_us', 0), r.get('service_sequence', 0)))
def kinds(a, b, *ks): return [r for r in rows if r.get('kind') in ks and a <= r.get('at_us', 0) < b]
# Per-block hearing: frame sequence -> (class, approximate lamp time); a trail is written at its last block.
heard = {}
for r in rows:
    if r.get('kind') == 'duplex_hearing' and isinstance(r.get('heard'), str):
        n, first = len(r['heard']), r.get('first_frame_sequence', 0)
        for i, c in enumerate(r['heard']):
            heard[first + i] = (c, r.get('at_us', 0) - (n - 1 - i) * 10_000)
def blocks(a, b): return ''.join(heard.get(f, ('.',))[0] for f in range(a, b + 1))
def counts(text): return ', '.join('%s %d' % (CLASSES[c], text.count(c)) for c in '0123456-.' if text.count(c))
def lost(a, b):
    out = ['%s (service_sequence %s%s)' % (r['kind'], r.get('service_sequence'),
           ', stalled %s ms, dropped %s' % (r.get('stalled_us', 0) // 1000, r.get('dropped_frames')) if r['kind'] == 'capture_gap' else '')
           for r in kinds(a, b, *GAP_KINDS)]
    out += ['yield ended %s (service_sequence %s, frames %s..%s)' % (r.get('outcome'), r.get('service_sequence'),
            r.get('first_frame_sequence'), r.get('ending_frame_sequence'))
            for r in kinds(a, b, 'duplex_yield_end') if r.get('outcome') in LOST]
    return out
def stop_words(a, b):
    return ['%s stop=%s words=%s' % (r['kind'], r.get('stop'), r.get('words')) for r in kinds(a, b, 'stop_words', 'stop_heard')]
def window(y, decision):
    """The bounded window: from the first block of the over (4) run that ended on the yield's block, to the
    decision's block (or 110 blocks, the resume deadline, past the yield)."""
    f = y.get('frame_sequence')
    if f is None: return None
    start = f
    while heard.get(start - 1, ('.',))[0] == '4': start -= 1
    end = decision.get('frame_sequence') if decision and decision.get('frame_sequence') else f + 110
    return start, f, end
policy = [r for r in rows if r.get('kind') == 'duplex_policy' and r.get('at_us', 0) <= (plays[0][0] if plays else 0)]
if policy:
    p = {k: v for k, v in policy[-1].items() if k not in ('fallback', 'basis', 'kind', 'at_us') and v is not None}
    print('%s: policy %s' % (name, json.dumps(p, separators=(',', ':'), sort_keys=True)))
want = 2 if mode == 'idle' else 3
if len(plays) != want:
    print('%s: not played whole (%d of %d lines); not a trial' % (name, len(plays), want)); tsv(mode, offset, 'no'); sys.exit()
ask = plays[0][0]
ask_end = ask + (dur_us(plays[0][1], plays[0][2]) or 3_000_000)
stop_at = plays[-1][0]
first = next((r for r in kinds(ask, stop_at, 'speaker_first_write')), None)
if first is None:
    print('%s: the lamp never began the story; not a trial' % name); tsv(mode, offset, 'no'); sys.exit()
story = (first.get('owner') or {}).get('turn', first.get('turn'))
if mode == 'idle':
    a = ask_end + 1_000_000
    span = [f for f, (c, at) in heard.items() if a <= at < stop_at]
    minutes = len(span) / 6000
    ys = kinds(a, stop_at, 'duplex_yield')
    print('%s: idle %.2f min of reply heard (%d blocks), %d false yields, %d commits, %d resumes' % (
        name, minutes, len(span), len(ys), len(kinds(a, stop_at, 'duplex_commit')), len(kinds(a, stop_at, 'duplex_resume'))))
    for y in ys:
        d = next(iter(kinds(y['at_us'], stop_at + 1, 'duplex_commit', 'duplex_resume')), None)
        w = window(y, d)
        cls = blocks(w[0], w[2]) if w else ''
        print('  yield at %s, frame %s, %s after %s; window frames %s..%s: %s [%s]' % (
            s(y['at_us'] - first['at_us']), y.get('frame_sequence'), d['kind'][7:] if d else 'no decision',
            s(d['at_us'] - y['at_us']) if d else '-', w[0] if w else '?', w[2] if w else '?', cls, counts(cls)))
    for x in lost(a, stop_at): print('  discontinuity: %s' % x)
    for x in stop_words(a, stop_at): print('  stop words: %s' % x)
    tsv(mode, offset, 'yes', len(span), len(ys))
    sys.exit()
line_at, voice, text = plays[1]
words_us = dur_us(voice, text)
end_words = line_at + words_us if words_us else None
before = kinds(first['at_us'], line_at, 'duplex_yield')
if kinds(ask, line_at, 'turn_finished'):
    print('%s: the story ended before the line; not a trial' % name); tsv(mode, offset, 'no'); sys.exit()
actual = (line_at - first['at_us']) / 1e6
head = '%s: %s at %.1f s (asked %s s)' % (name, mode, actual, offset)
y = next(iter(kinds(line_at, line_at + (words_us or 2_000_000) + 1_500_000, 'duplex_yield')), None)
heard_words = ' '.join((r.get('text') or '').strip() for r in kinds(line_at, stop_at, 'transcript')
                       if r.get('turn') is None or r.get('role') == 'input')
if y is None:
    print('%s; no yield (%d false yields before it); words "%s"' % (head, len(before), heard_words[:60]))
    for x in lost(line_at, stop_at): print('  discontinuity: %s' % x)
    tsv(mode, offset, 'yes', '', '', 'none', '', '')
    sys.exit()
d = next(iter(kinds(y['at_us'], stop_at, 'duplex_commit', 'duplex_resume')), None)
w = window(y, d)
cls = blocks(w[0], w[2]) if w else ''
decision = d['kind'][7:] if d else 'none'
onset_yield = y['at_us'] - line_at
yield_decision = d['at_us'] - y['at_us'] if d else None
print('%s; onset->yield %s, yield->%s %s; %d false yields before it; words "%s"' % (
    head, s(onset_yield), decision, s(yield_decision) if d else '-', len(before), heard_words[:60]))
print('  hearing, frames %s..%s (yield at %s, decision at %s): %s [%s]' % (
    w[0] if w else '?', w[2] if w else '?', y.get('frame_sequence'), d.get('frame_sequence') if d else '-', cls, counts(cls)))
accepted = acoustic = ''
if decision == 'commit':
    answer = next((r for r in kinds(d['at_us'], stop_at, 'speaker_first_write')
                   if (r.get('owner') or {}).get('turn', r.get('turn')) != story), None)
    if answer is None or end_words is None:
        print('  answer: %s' % ('no answer audio before the stop line' if end_words else 'clip length unknown'))
    else:
        accepted = answer['at_us'] - end_words
        own = sorted(at for f, (c, at) in heard.items() if c == '3' and at >= answer['at_us'])
        acoustic = own[0] - end_words if own else ''
        print('  end of words->first answer audio: accepted PCM %s; acoustic (mic heard own voice) %s' % (
            s(accepted), s(acoustic) if own else 'unmeasured'))
story_end = next((r for r in kinds(line_at, stop_at + 10_000_000, 'turn_finished') if r.get('turn') == story), None)
print('  story: %s' % (story_end.get('outcome') if story_end else 'went on'))
for x in stop_words(line_at, stop_at): print('  stop words: %s' % x)
for x in lost(line_at, stop_at): print('  discontinuity: %s' % x)
tsv(mode, offset, 'yes', onset_yield, yield_decision if d else '', decision, accepted, acoustic)
PY
  sleep 5
}

for i in $(seq 1 "$IDLE"); do
  episode idle - "$LABEL-idle-$i" "Daniel|$ASK Make it a long one.|100" "Daniel|$STOP|8"
done
for r in $(seq 1 "$ROUNDS"); do
  for offset in 3 8 15 25; do
    episode aside "$offset" "$LABEL-aside-$offset-$r" "Daniel|$ASK|$((LEAD + offset))" "Samantha|Hmm, interesting.|20" "Daniel|$STOP|8"
    episode cutin "$offset" "$LABEL-cutin-$offset-$r" "Daniel|$ASK|$((LEAD + offset))" "Samantha|Wait, what was the keeper's name?|20" "Daniel|$STOP|8"
  done
done

python3 - "$tally" <<'PY' | tee -a "$report"
import sys
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1]) if l.strip()]
idle = [r for r in rows if r[0] == 'idle' and r[2] == 'yes']
blocks, ys = sum(int(r[3]) for r in idle), sum(int(r[4]) for r in idle)
print('idle: %d of %d stories; %d false yields in %.2f min of reply heard: %s per min' % (
    len(idle), sum(r[0] == 'idle' for r in rows), ys, blocks / 6000, '%.2f' % (ys / (blocks / 6000)) if blocks else '-'))
def ms(x): return '%d' % (float(x) / 1000) if x else '-'
for mode in ('aside', 'cutin'):
    for offset in ('3', '8', '15', '25'):
        mine = [r for r in rows if r[0] == mode and r[1] == offset and r[2] == 'yes']
        if not mine: continue
        print('%-5s %2s s: %s' % (mode, offset, '; '.join(
            '%s (onset->yield %s ms, ->decision %s ms, answer accepted %s / acoustic %s ms)' % (
                r[5], ms(r[3]), ms(r[4]), ms(r[6]), ms(r[7])) for r in mine)))
PY
echo "cutin.sh: the report is $report; each scene in full beside it"
