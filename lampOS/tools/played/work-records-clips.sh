#!/usr/bin/env bash
# L4's clips for work-records-row.py (#7930), ElevenLabs only (the played-voice rule; no say, no Deepgram):
# rendered through the lamp's own voice relay (its credential is read as root into curl's input headers and
# never printed or copied), the same URL, voice and model as the earlier played kits, then made into
# 24 kHz mono WAVs here and checked: each one's length, and that none is silent.
#
#   work-records-clips.sh [--only NAME,...] OUT_DIR   # needs the lamp (lamp_ssh, or WORK_SSH's stand-in);
#                                        # about 20 s; --only renders (and checks) those clips alone
#   work-records-clips.sh --check OUT_DIR  # only the check, no lamp
#   work-records-clips.sh --trim OUT_DIR   # only the trim (quiet before/after 50 ms, pauses 200 ms), then the check
#
# Clips: en-ask, en-yes, en-how, en-what and the same in vi (the -what pair is the "what's it doing now?"
# variant, --how-clip what), and, only when --only names it, HW's en-long-do (a long do-task).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
VOICE=${VOICE:-ErXwobaYiN019PkySvjV}
SSH=${WORK_SSH:-$HERE/lamp_ssh}
NAMES='en-ask en-yes en-how en-what vi-ask vi-yes vi-how vi-what'
if [ "${1:-}" = --only ]; then NAMES=$(printf %s "${2:?--only NAME,...}" | tr ',' ' '); shift 2; fi
CLIPS='en-ask	Hey Lamp, ask the agent on the lamp to research the best houseplants for a dark bedroom.
en-yes	Yes, please.
en-how	Hey Lamp, how far is it?
en-what	Hey Lamp, what is it doing now?
vi-ask	Hey Lamp, nhờ trợ lý trên đèn tìm hiểu giúp mình những loại cây trồng trong phòng ngủ tối.
vi-yes	Ừ, làm đi.
vi-how	Hey Lamp, tới đâu rồi?
vi-what	Hey Lamp, nó đang làm gì vậy?
en-long-do	Hey Lamp, go through the five biggest tech news sites, pick today'\''s top three AI stories, and save a one-page summary for me.'
for n in $NAMES; do printf '%s\n' "$CLIPS" | cut -f1 | grep -qx "$n" || { echo "--only: no clip $n"; exit 2; }; done

trim() { # trim DIR: each clip's leading and trailing quiet cut to 50 ms and every pause inside to 200 ms,
  # so the lamp's speech to text never hears "Hey Lamp" as a line of its own (c0a4-5 vi); the words untouched.
  python3 -I - "$1" <<'PY'
import array, os, sys, wave
d = sys.argv[1]
for name in sorted(os.listdir(d)):
    if not name.endswith('.wav'):
        continue
    path = os.path.join(d, name)
    with wave.open(path) as w:
        params, rate = w.getparams(), w.getframerate()
        samples = array.array('h', w.readframes(w.getnframes()))
    win = rate // 100
    loud = [max((abs(s) for s in samples[i:i + win]), default=0) > 600 for i in range(0, len(samples), win)]
    if True not in loud:
        continue
    first, last = loud.index(True), len(loud) - 1 - loud[::-1].index(True)
    keep, quiet = [], 0
    for at in range(max(0, first - 5), min(len(loud), last + 6)):
        quiet = 0 if loud[at] else quiet + 1
        if quiet <= 20:
            keep.append(samples[at * win:(at + 1) * win])
    with wave.open(path, 'wb') as w:
        w.setparams(params)
        w.writeframes(b''.join(part.tobytes() for part in keep))
PY
}

check() { # check DIR: each clip's seconds, loudness and longest pause; fails on a missing, short or silent
  # one, more than 0.3 s of quiet before its first word, or a pause over 0.4 s inside it
  python3 -I - "$1" $NAMES <<'PY'
import array, math, os, sys, wave
d, bad = sys.argv[1], 0
for name in sys.argv[2:]:
    path = os.path.join(d, name + '.wav')
    if not os.path.exists(path):
        print(f'{name}: MISSING'); bad += 1; continue
    with wave.open(path) as w:
        frames = w.readframes(w.getnframes()); rate = w.getframerate()
    samples = array.array('h', frames)
    seconds = len(samples) / rate
    rms = math.sqrt(sum(s * s for s in samples) / max(1, len(samples)))
    db = 20 * math.log10(max(rms, 1) / 32768)
    loud = sum(1 for i in range(0, len(samples), rate // 10)
               if max((abs(s) for s in samples[i:i + rate // 10]), default=0) > 1000) / 10
    win = rate // 100
    tens = [max((abs(s) for s in samples[i:i + win]), default=0) > 600 for i in range(0, len(samples), win)]
    lead = (tens.index(True) if True in tens else len(tens)) / 100
    gap, run = 0, 0
    if True in tens:
        for v in tens[tens.index(True):len(tens) - tens[::-1].index(True)]:
            run = 0 if v else run + 1
            gap = max(gap, run)
    gap /= 100
    ok = seconds >= 0.5 and db > -45 and loud >= 0.3 and lead <= 0.3 and gap <= 0.4
    print(f'{name}: {seconds:.2f} s, {db:.1f} dBFS, {loud:.1f} s of voice, {lead:.2f} s before the first word, '
          f'longest pause {gap:.2f} s, {"ok" if ok else "BAD"}')
    bad += not ok
print(f'{bad} bad clip(s)')
sys.exit(bad > 0)
PY
}

if [ "${1:-}" = --check ]; then check "$2"; exit; fi
if [ "${1:-}" = --trim ]; then trim "$2" && (cd "$2" && shasum -a 256 ./*.wav >SHA256SUMS) && check "$2"; exit; fi
OUT=${1:?work-records-clips.sh OUT_DIR}
mkdir -p "$OUT"
# The render runs on the lamp in one command, one sudo: the clips go as base64 in the command itself.
list=$(printf '%s\n' "$CLIPS" | while IFS="$(printf '\t')" read -r id text; do
  printf ' %s ' "$NAMES" | grep -q " $id " && printf '%s\t%s\n' "$id" "$text"; done | base64 | tr -d '\n')
remote="sudo -n sh -c 'D=/tmp/work-records-clips; rm -rf \$D; mkdir -p \$D; echo $list | base64 -d > \$D/list; \
URL=https://campaign-api.autonomous.ai/api/v1/ai/v1/elevenlabs/text-to-speech/$VOICE/stream?output_format=pcm_24000; \
while IFS=\"\$(printf \"\\t\")\" read -r id text; do [ -z \"\$id\" ] && continue; \
body=\$(python3 -c \"import json,sys;print(json.dumps({\\\"text\\\":sys.argv[1],\\\"model_id\\\":\\\"eleven_flash_v2_5\\\",\\\"voice_settings\\\":{\\\"speed\\\":1.0}}))\" \"\$text\"); \
code=\$(tr -d \"\\n\" < /etc/lampOS/voice/credential | sed \"s/^/xi-api-key: /\" | curl -s -m 30 -o \$D/\$id.pcm -w %{http_code} -H @- -H Content-Type:application/json -X POST --data \"\$body\" \"\$URL\"); \
echo RENDER \$id http=\$code; sleep 1; done < \$D/list; rm -f \$D/list; chmod -R a+rX \$D'"
"$SSH" "$remote" </dev/null || { echo 'render failed on the lamp'; exit 1; }
for id in $NAMES; do
  "$SSH" "cat /tmp/work-records-clips/$id.pcm" </dev/null >"$OUT/$id.pcm" || exit 1
  python3 -I - "$OUT/$id.pcm" "$OUT/$id.wav" <<'PY'
import sys, wave
pcm = open(sys.argv[1], 'rb').read()
with wave.open(sys.argv[2], 'wb') as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(24000); w.writeframes(pcm)
PY
  rm -f "$OUT/$id.pcm"
done
"$SSH" 'rm -rf /tmp/work-records-clips' </dev/null
trim "$OUT"
(cd "$OUT" && shasum -a 256 ./*.wav >SHA256SUMS)
check "$OUT"
