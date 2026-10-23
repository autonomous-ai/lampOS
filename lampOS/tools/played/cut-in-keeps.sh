#!/usr/bin/env bash
# The lamp check for LAMP_CUT_IN_KEEPS_LISTENING (#2739, #2602 "Cutting in"): a person who keeps trying stops a story
# fallen back after two fruitless yields. Each round: "Hey Lamp, tell me a long story about a lighthouse keeper.", two
# asides over the story that should pause it and let it go on ("Hmm, interesting." and "What was that?", the aside of
# lamp-4ace on 20 Oct that #2739's replay copies), then a third line about 5 s after the second aside's start, inside
# the 20 s before a fruitless yield is forgiven. Run from the played kit:
#
#   LAMP_HOST=<lamp> lampOS/tools/played/cut-in-keeps.sh LABEL [ROUNDS]
#
# - The switch: a /run drop-in on lamp-voice (zz-cut-in-keeps-check.conf), first with LAMP_CUT_IN_KEEPS_LISTENING=off
#   (as today: the voice reads only `on`), then with it on, the voice restarted each time, ROUNDS rounds each (default
#   3), so both are played in the same hour. On any exit the drop-in is removed and the voice restarted without it.
# - What is counted, from the voice's session events (the kit's READ, as scenario.py): the asides' duplex_yield and
#   duplex_resume (a round with fewer than two of each, or an aside that commits, is not a trial); after the third
#   line, whether it was heard (a duplex_yield, a duplex_commit, an admitted input or its words), whether the story
#   stopped (the story's turn_finished as user_interrupted), and the gap from the third line's start to that stop. The
#   start is the lamp's clock just before the clip was started here, 0.1 to 0.3 s early, so the gaps are that much long.
#   duplex_policy's keeps_listening says whether the switch was in force.
# - What passes: on, the third line heard and the story stopped within about 1 s; off, as today (#2739's replay: not
#   heard, the story goes on).
#
# It plays sound in the room, about 2 minutes a round and 1 minute a restart. Not between 12:00 and 14:00 on weekdays.
# Settings: as the kit's (README.md); LAMP_SSH, CUT_IN_SCENARIO and CUT_IN_QUIET_GATE replace this kit's lamp_ssh,
# scenario.py and quiet_gate.sh (the stand-in test, cut-in-keeps-test.sh); CUT_IN_QUIET_MAX (600 s) is the longest
# wait for a quiet lamp before a round.
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
DROPIN=/run/systemd/system/lamp-voice.service.d/zz-cut-in-keeps-check.conf
LABEL=${1:-}
ROUNDS=${2:-3}

if [[ -z $LABEL || ! $ROUNDS =~ ^[1-9][0-9]?$ ]]; then
  echo "usage: cut-in-keeps.sh LABEL [ROUNDS, 1 to 99]" >&2
  exit 2
fi
if [[ $(date +%u) -le 5 && $((10#$(date +%H))) -ge 12 && $((10#$(date +%H))) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "cut-in-keeps.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi

lamp() { "$LAMP_SSH" "$@" </dev/null; }

# The voice as it runs: active, duplex, the switch not already set, and a binary that knows it.
unit=$(lamp 'systemctl show -p ActiveState -p Environment -p ExecStart lamp-voice') \
  || { echo "cut-in-keeps.sh: no answer from the lamp" >&2; exit 3; }
[[ $unit == *ActiveState=active* ]] || { echo "cut-in-keeps.sh: lamp-voice is not active; nothing changed" >&2; exit 3; }
[[ $unit == *"--turn-taking duplex"* ]] \
  || { echo "cut-in-keeps.sh: lamp-voice is not on --turn-taking duplex, which the switch needs; nothing changed" >&2; exit 3; }
[[ $unit == *LAMP_CUT_IN_KEEPS_LISTENING=* ]] \
  && { echo "cut-in-keeps.sh: LAMP_CUT_IN_KEEPS_LISTENING is already set on lamp-voice; nothing changed" >&2; exit 3; }
bin=$(sed -n 's/.*ExecStart={ path=\([^ ;]*\).*/\1/p' <<<"$unit" | head -n 1)
if [[ -z $bin ]] || ! lamp "sudo -n grep -q -a LAMP_CUT_IN_KEEPS_LISTENING $bin"; then
  echo "cut-in-keeps.sh: lamp-voice's binary (${bin:-not found}) is older than #2739; nothing changed" >&2
  exit 3
fi

mkdir -p "$OUT"
work=$(mktemp -d)
changed=no
put_back() {
  rm -rf "$work"
  [[ $changed == yes ]] || return 0
  lamp "sudo -n rm -f $DROPIN && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    && echo "cut-in-keeps.sh: drop-in removed, lamp-voice restarted without it" \
    || echo "cut-in-keeps.sh: putting lamp-voice back FAILED: remove $DROPIN, daemon-reload and restart lamp-voice by hand" >&2
}
trap put_back EXIT
trap 'exit 130' INT TERM HUP

switch() { # switch on|off: the drop-in with that value, the voice restarted with it
  changed=yes
  lamp "sudo -n mkdir -p ${DROPIN%/*} && printf '[Service]\nEnvironment=LAMP_CUT_IN_KEEPS_LISTENING=$1\n' | sudo -n tee $DROPIN >/dev/null && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    || { echo "cut-in-keeps.sh: the drop-in could not be put on (sudo -n)" >&2; exit 4; }
  echo "cut-in-keeps.sh: lamp-voice restarted with LAMP_CUT_IN_KEEPS_LISTENING=$1"
}

quiet() { # until the lamp has heard and said nothing for 45 s (a voice just restarted counts as busy)
  local waited=0
  until "$QUIET" 45 >/dev/null 2>&1; do
    ((waited >= QUIET_MAX)) && { echo "cut-in-keeps.sh: the lamp was not quiet for 45 s within ${QUIET_MAX} s; stopped" >&2; exit 5; }
    sleep 10
    waited=$((waited + 10))
  done
}

# shellcheck disable=SC2016 # expanded on the lamp, not here
READ='S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"'
tally=$work/tally.tsv
report=$OUT/cut-in-keeps-$LABEL.txt
: >"$report"

for arm in off on; do
  switch $arm
  for i in $(seq 1 "$ROUNDS"); do
    quiet
    name=$LABEL-cut-in-$arm-$i
    # The ask; the story plays about 6 s; two asides 2 s apart; the third line about 5 s after the second began.
    "$SCENARIO" "$name" \
      "Daniel|Hey Lamp, tell me a long story about a lighthouse keeper.|9" \
      "Daniel|Hmm, interesting.|1.2" \
      "Daniel|What was that?|4" \
      "Daniel|Lamp, stop. Please stop the story.|20" >/dev/null 2>&1
    # shellcheck disable=SC2012 # the kit names its own scene files: no odd characters
    scene=$(ls -t "$OUT"/*-"$name".txt 2>/dev/null | head -n 1)
    lamp "$READ" >"$work/events.jsonl"
    python3 - "$arm" "$i" "${scene:-}" "$work/events.jsonl" "$tally" <<'PY' | tee -a "$report"
import json, sys
arm, i, scene, events, tally = sys.argv[1:6]
plays = []
if scene:
    for line in open(scene, errors='replace'):
        if line.startswith('#PLAY_US '):
            plays.append(int(line.split()[1]))
def say(text, ok='', heard='', stopped='', gap=''):
    print('%-3s round %s: %s' % (arm, i, text))
    with open(tally, 'a') as f: f.write('\t'.join([arm, ok, heard, stopped, gap]) + '\n')
if len(plays) != 4:
    say('not played whole (%d of 4 lines); not a trial' % len(plays)); sys.exit()
ask, aside1, aside2, third = plays
rows = []
for line in open(events, errors='replace'):
    try: r = json.loads(line)
    except ValueError: continue
    rows.append(r)
rows.sort(key=lambda r: (r.get('at_us', 0), r.get('service_sequence', 0)))
policy = [r for r in rows if r.get('kind') == 'duplex_policy' and r.get('at_us', 0) <= third]
keeps = 'keeps_listening' in policy[-1] and policy[-1]['keeps_listening'] is not None if policy else None
story = next(((r.get('owner') or {}).get('turn') for r in rows
              if r.get('kind') == 'speaker_first_write' and ask <= r.get('at_us', 0) < aside1), None)
def kinds(a, b, *ks): return [r for r in rows if r.get('kind') in ks and a <= r.get('at_us', 0) < b]
end = float('inf')
early_end = [r for r in kinds(ask, third, 'turn_finished') if story is not None and r.get('turn') == story]
yields, resumes = len(kinds(aside1, third, 'duplex_yield')), len(kinds(aside1, third, 'duplex_resume'))
commits = len(kinds(aside1, third, 'duplex_commit'))
forgiven = len(kinds(aside2, third, 'duplex_fruitless_forgiven'))
setup = 'asides %d yields, %d resumes, %d commits, third line %.1f s after the second' % (yields, resumes, commits, (third - aside2) / 1e6)
if arm == 'on' and keeps is not True or arm == 'off' and keeps is True:
    say('%s; the switch was not in force (duplex_policy keeps_listening %s); not a trial' % (setup, keeps)); sys.exit()
if story is None:
    say('%s; the lamp never began the story before the first aside; not a trial' % setup); sys.exit()
if early_end:
    say('%s; the story ended (%s) before the third line; not a trial' % (setup, early_end[0].get('outcome'))); sys.exit()
if yields < 2 or resumes < 2 or commits or forgiven:
    say('%s%s; not the scene (two asides that pause and go on); not a trial' % (setup, ', a yield forgiven' if forgiven else '')); sys.exit()
after_yield = kinds(third, end, 'duplex_yield')
after_commit = kinds(third, end, 'duplex_commit')
admitted = kinds(third, end, 'input_admitted')
words = ' '.join((r.get('text') or '').strip() for r in kinds(third, end, 'transcript')
                 if r.get('turn') is None or r.get('role') == 'input')
heard = bool(after_yield or after_commit or admitted or words)
stop = next((r for r in kinds(third, end, 'turn_finished') if r.get('turn') == story), None)
stopped = stop is not None and stop.get('outcome') == 'user_interrupted'
gap = (stop['at_us'] - third) / 1e6 if stopped else None
how = ', '.join(x for x in ['yield' if after_yield else '', 'commit' if after_commit else '',
                             'admitted' if admitted else '', ('words "%s"' % words[:60]) if words else ''] if x) or 'no yield, no candidate'
said = 'story stopped %.2f s after the line\'s start' % gap if stopped else \
       'story not stopped (%s)' % (stop.get('outcome') if stop else 'went on')
verdict = ''
if arm == 'on':
    verdict = '  PASS' if heard and stopped and gap <= 1.2 else '  FAIL'
say('%s; third line heard %s (%s), %s%s' % (setup, 'yes' if heard else 'no', how, said, verdict),
    'yes', 'yes' if heard else 'no', 'yes' if stopped else 'no', '%.2f' % gap if stopped else '')
PY
    sleep 5
  done
done

python3 - "$tally" <<'PY' | tee -a "$report"
import sys
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1])]
for arm in ('off', 'on'):
    mine = [r for r in rows if r[0] == arm]
    ok = [r for r in mine if r[1] == 'yes']
    gaps = sorted(float(r[4]) for r in ok if r[4])
    print('%s: trials %d of %d; third line heard %d, story stopped %d; gaps to the stop %s' % (
        arm, len(ok), len(mine), sum(r[2] == 'yes' for r in ok), sum(r[3] == 'yes' for r in ok),
        ', '.join('%.2f s' % g for g in gaps) or 'none'))
PY
echo "cut-in-keeps.sh: the report is $report; each scene in full beside it"
