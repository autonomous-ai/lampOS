#!/usr/bin/env bash
# Tests for cutin.sh with no lamp and no sound: a stand-in lamp_ssh, scenario.py and quiet_gate.sh, a stand-in
# scene that writes the session events a story with one yield in it would give, and a sleep that returns at once
# (it only skips the waits between scenes).
# Run: lampOS/tools/played/cutin-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/cutin.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=LAMP_STOP_WORDS=heard %s\nExecStart={ path=/opt/lampOS/voice/current/lamp-voice ; argv[]=/opt/lampOS/voice/current/lamp-voice serve --turn-taking %s ; }\n' "${FAKE_ENV:-}" "$FAKE_TT" ;;
  *sha256sum*) echo "0123456789abcdef0123  /opt/lampOS/voice/current/lamp-voice" ;;
  *"tee "*) echo on >"$FAKE_DIR/dropin" ;;
  *"rm -f "*) rm -f "$FAKE_DIR/dropin"; echo removed >>"$FAKE_DIR/put-back"; [[ -z ${FAKE_RESTORE_FAIL:-} ]] || exit 1 ;;
  *sessions*) [[ -z ${FAKE_JOURNAL_FAIL:-} ]] || exit 78; cat "$FAKE_DIR/events" ;;
  *) exit 1 ;;
esac
EOF

# The scene: each line's #PLAY_US and a 1 s clip of it, and the events the voice would write. The story's first write
# is at 4 s, the line at 7 s; a yield 0.15 s in on frame 1015 after an over run from 1008, then a resume 0.5 s later
# (an aside, or the idle story's false yield) or a commit 0.1 s later and the answer's first write 1.4 s after the
# line's end, heard by the microphone as the lamp's own voice 0.1 s after that (a cut-in). FAKE_CASE=expired: the
# yield's hearing expires 0.1 s in, and a separate yield at 8.5 s resumes at 9 s. FAKE_CASE=laterstop: the answer
# finishes at 15 s with no block of its own voice heard; only the stop's answer, at 31.4 s, is heard, from 31.51 s.
# FAKE_SCENE_FAIL: the scene exits 77; FAKE_NO_SCENE: it exits 0 and writes no scene.
cat >"$T/scenario.py" <<'EOF'
#!/usr/bin/env python3
import hashlib, json, os, sys, wave
name, steps = sys.argv[1], sys.argv[2:]
out, d = os.environ['PLAYED_OUT'], os.environ['FAKE_DIR']
case = os.environ.get('FAKE_CASE', '')
if os.environ.get('FAKE_SCENE_FAIL'): sys.exit(77)
if os.environ.get('FAKE_NO_SCENE'): sys.exit(0)
plays = []
for i, step in enumerate(steps):
    voice, text, _ = step.split('|')
    key = hashlib.sha256(('%s|185|%s' % (voice, text)).encode()).hexdigest()[:16]
    with wave.open('%s/prompts/generated/%s.wav' % (out, key), 'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000); w.writeframes(b'\0\0' * 16000)
    plays.append('#PLAY_US %d %s: %s' % ([1_000_000, 7_000_000, 30_000_000][i] if len(steps) == 3 else [1_000_000, 90_000_000][i], voice, text))
open('%s/scenarios/20261022T000000Z-%s.txt' % (out, name), 'w').write('\n'.join(plays) + '\n')
ev = [{"kind": "duplex_policy", "at_us": 500_000, "fruitless_yields_before_fallback": 2, "keeps_listening": None},
      {"kind": "speaker_first_write", "at_us": 4_000_000, "owner": {"turn": 5}},
      # 1,000 blocks of the story from frame 700, the trail written at its last block, so frame 1014 is at 7.15 s;
      # then 50 blocks of the lamp's own voice from frame 1900, the first at 9.51 s.
      {"kind": "duplex_hearing", "at_us": 14_000_000, "first_frame_sequence": 700,
       "heard": '3' * 308 + '4' * 7 + '3' * 30 + '0' * 655},
      {"kind": "duplex_hearing", "at_us": 10_000_000, "first_frame_sequence": 1900, "heard": '3' * 50}]
yat = 7_150_000
ev.append({"kind": "duplex_yield", "at_us": yat, "frame_sequence": 1014, "service_sequence": 40})
if case == 'laterstop':
    ev[-2] = {"kind": "duplex_hearing", "at_us": 32_000_000, "first_frame_sequence": 5000, "heard": '3' * 50}
    ev += [{"kind": "turn_finished", "turn": 6, "at_us": 15_000_000, "outcome": "completed"},
           {"kind": "speaker_first_write", "at_us": 31_400_000, "owner": {"turn": 7}}]
if case == 'expired':
    ev += [{"kind": "duplex_yield_end", "started_at_us": yat, "at_us": yat + 100_000, "outcome": "hearing_expired",
            "last_frame_sequence": 1024, "service_sequence": 42},
           {"kind": "duplex_yield", "at_us": 8_500_000, "frame_sequence": 1150},
           {"kind": "duplex_resume", "at_us": 9_000_000, "frame_sequence": 1199},
           {"kind": "duplex_yield_end", "started_at_us": 8_500_000, "at_us": 9_000_000, "outcome": "resumed",
            "ending_frame_sequence": 1199}]
elif '-cutin-' in name:
    ev += [{"kind": "duplex_commit", "at_us": yat + 100_000, "frame_sequence": 1024},
           {"kind": "duplex_yield_end", "started_at_us": yat, "at_us": yat + 100_000, "outcome": "commit_decision",
            "ending_frame_sequence": 1024},
           {"kind": "stop_words", "stop": False, "words": 6, "at_us": yat + 400_000},
           {"kind": "turn_finished", "turn": 5, "at_us": yat + 120_000, "outcome": "user_interrupted"},
           {"kind": "transcript", "at_us": 8_500_000, "role": "input", "text": "Wait, what was the keeper's name?"},
           {"kind": "speaker_first_write", "at_us": 9_400_000, "owner": {"turn": 6}}]
else:
    ev += [{"kind": "duplex_resume", "at_us": yat + 500_000, "frame_sequence": 1064},
           {"kind": "duplex_yield_end", "started_at_us": yat, "at_us": yat + 500_000, "outcome": "resumed",
            "ending_frame_sequence": 1064},
           {"kind": "capture_gap", "at_us": 7_400_000, "stalled_us": 160_000, "dropped_frames": 0, "service_sequence": 41}]
with open(d + '/events', 'w') as f:
    for e in ev: f.write(json.dumps(e) + '\n')
    f.write('not json\n')
EOF
printf '#!/usr/bin/env bash\nexit 0\n' >"$T/quiet_gate.sh"
mkdir -p "$T/bin"; printf '#!/usr/bin/env bash\nexit 0\n' >"$T/bin/sleep"
chmod +x "$T/lamp_ssh" "$T/scenario.py" "$T/quiet_gate.sh" "$T/bin/sleep"

fail=0
out=
run() { # run NAME EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings, one idle story and one round
  local name=$1 want=$2 got
  shift 2
  rm -rf "$T/d"; mkdir -p "$T/d"
  [[ -n ${KEEP_OUT:-} ]] || rm -rf "$T/out"
  out=$(PATH=$T/bin:$PATH FAKE_DIR=$T/d LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh CUTIN_SCENARIO=$T/scenario.py \
    CUTIN_QUIET_GATE=$T/quiet_gate.sh PLAYED_OUT=$T/out LAMP_PLAY_AT_LUNCH=yes "$SCRIPT" "$name" 1 1 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  [[ ! -e $T/d/dropin ]] || { echo "FAIL $name: the drop-in was left on"; fail=1; }
}

export FAKE_TT=duplex
run normal 0 \
  'build: /opt/lampOS/voice/current/lamp-voice sha256 0123456789abcdef' \
  'flags: LAMP_STOP_WORDS=heard' \
  'normal-idle-1: policy {"fruitless_yields_before_fallback":2}' \
  'normal-idle-1: idle 0.17 min of reply heard (1050 blocks), 1 false yields, 0 commits, 1 resumes' \
  'window frames 1008..1064: 444444433333333333333333333333333333300000000000000000000 [still 20, own 30, over 7]' \
  'discontinuity: capture_gap (service_sequence 41, stalled 160 ms, dropped 0)' \
  'normal-aside-3-1: aside at 3.0 s (asked 3 s); onset->yield 0.15 s, yield->resume 0.50 s; 0 false yields before it' \
  'normal-cutin-8-1: cutin at 3.0 s (asked 8 s); onset->yield 0.15 s, yield->commit 0.10 s' \
  'hearing, frames 1008..1024 (yield at 1014, ended at 1024): 44444443333333333 [own 10, over 7]' \
  "end of words->first answer audio: accepted PCM 1.40 s; acoustic (mic heard own voice) 1.51 s" \
  'stop words: stop_words stop=False words=6' \
  'story: user_interrupted' \
  'idle: 1 of 1 stories; 1 false yields in 0.17 min of reply heard: 5.71 per min' \
  'cutin 25 s: commit (onset->yield 150 ms, ->decision 100 ms, answer accepted 1400 / acoustic 1510 ms)'
[[ ! -e $T/d/put-back ]] || { echo "FAIL normal: it put back what it never changed"; fail=1; }
[[ -s $T/out/scenarios/cutin-normal.txt ]] || { echo "FAIL normal: no report file"; fail=1; }

# A switch for the run rides on a /run drop-in, removed once at the end.
CUTIN_ENV="LAMP_CUT_IN_COUNTS_REAL=on" run switched 0 'lamp-voice restarted with LAMP_CUT_IN_COUNTS_REAL=on' \
  'drop-in removed, lamp-voice restarted without it'
grep -qF 'Environment=LAMP_CUT_IN_COUNTS_REAL=on' "$T/d/calls" || { echo "FAIL switched: the drop-in never put on"; fail=1; }
[[ $(grep -c removed "$T/d/put-back") == 1 ]] || { echo "FAIL switched: not put back exactly once"; fail=1; }
CUTIN_ENV="rm -rf" run bad-env 2 "is not LAMP_NAME=value"

# A failed scene or journal read is not a clean measurement: the run exits 1, and still takes the drop-in off.
CUTIN_ENV="LAMP_CUT_IN_COUNTS_REAL=on" FAKE_SCENE_FAIL=1 run scene-fail 1 \
  'scene-fail-idle-1: the scene failed (exit 77); not measured' 'drop-in removed, lamp-voice restarted without it' \
  'some episodes failed (above); not a clean measurement'
FAKE_JOURNAL_FAIL=1 run journal-fail 1 "journal-fail-cutin-25-1: the lamp's journal could not be read; not measured"
# A put-back that fails is not clean either, and never hides an earlier failure's code.
CUTIN_ENV="LAMP_CUT_IN_COUNTS_REAL=on" FAKE_RESTORE_FAIL=1 run restore-fail 6 'putting lamp-voice back FAILED'
CUTIN_ENV="LAMP_CUT_IN_COUNTS_REAL=on" FAKE_RESTORE_FAIL=1 FAKE_SCENE_FAIL=1 run both-fail 1 'putting lamp-voice back FAILED'
# A scene that writes no file is never read from a same-label scene of an earlier run.
run stale 0
KEEP_OUT=1 FAKE_NO_SCENE=1 run stale 1 'stale-aside-8-1: the scene wrote no file; not measured'
grep -qF 'onset->yield' <<<"$out" && { echo "FAIL stale: measured from an earlier run's scene"; fail=1; }

# A yield whose hearing expired is not the later, separate yield's resume.
FAKE_CASE=expired run expired 0 \
  'expired-aside-3-1: aside at 3.0 s (asked 3 s); onset->yield 0.15 s, yield->hearing_expired, no decision 0.10 s' \
  'hearing, frames 1008..1024 (yield at 1014, ended at 1024)' \
  'no decision (hearing_expired) after 0.10 s' 'resume after 0.50 s' \
  'yield ended hearing_expired (service_sequence 42, frames None..None)'
grep -qF 'yield->resume 1.85' <<<"$out" && { echo "FAIL expired: the later yield's resume was taken as this one's"; fail=1; }
# An answer with none of its own voice heard is unmeasured, not timed by the stop's later answer.
FAKE_CASE=laterstop run laterstop 0 \
  'end of words->first answer audio: accepted PCM 1.40 s; acoustic (mic heard own voice) unmeasured' \
  'cutin 25 s: commit (onset->yield 150 ms, ->decision 100 ms, answer accepted 1400 / acoustic - ms)'
grep -qF '23.51' <<<"$out" && { echo "FAIL laterstop: the stop's answer timed this answer"; fail=1; }

# Not duplex: nothing changed. No LAMP_HOST: nothing at all.
FAKE_TT=v1-baseline run baseline 3 "not on --turn-taking duplex"
out=$(env -u LAMP_HOST "$SCRIPT" x 2>&1); [[ $? == 2 && $out == *"LAMP_HOST is not set"* ]] || { echo "FAIL no-host: $out"; fail=1; }

[[ $fail == 0 ]] && echo "cutin-test: all passed"
exit $fail
