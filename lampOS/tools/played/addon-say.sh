#!/usr/bin/env bash
# An add-on's quoted say on a lamp (#2530 part 2c, #2675; docs/addons.md): the voice's side of a say the judge and
# the policy kept. It asks the say service for one quoted line, as lamp-skills does for a kept say, on the second
# route with LAMP_ADDON_SAY=on, and compares the words the lamp said with the kept text. Run from the played kit:
#
#   LAMP_HOST=<lamp> lampOS/tools/played/addon-say.sh LABEL ["the kept text"]
#
# - The switch: a /run drop-in on lamp-voice (zz-addon-say-check.conf, Environment=LAMP_ADDON_SAY=on), the voice
#   restarted with it; on any exit the drop-in is removed and the voice restarted without it. Nothing else changes.
# - The say: {"skill":"addon-say-check","quoted":TEXT} on the voice's say socket, and its outcome (spoken, dropped,
#   expired or refused, with why).
# - The words: the lamp's transcript of that turn, from its session's events, against the kept text, both lowercased
#   with punctuation dropped. "kept in order" is the longest run of the kept words the lamp said in the same order
#   (not necessarily together); "changed" is the kept words it left out or put another way; "added" its own words.
#   A quoted line may be said in the lamp's own words or left unsaid (say::quoted_turn): the counts say which.
#
# It plays sound in the room: the lamp speaks the line. Not between 12:00 and 14:00 on weekdays, when the office rests.
# Settings: as the kit's (README.md), and LAMP_SSH (this kit's lamp_ssh), ADDON_SAY_WAIT (90 s for the outcome).
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
LAMP_SSH=${LAMP_SSH:-$HERE/lamp_ssh}
OUT=${PLAYED_OUT:-$HOME/lamp-played}/scenarios
WAIT=${ADDON_SAY_WAIT:-90}
DROPIN=/run/systemd/system/lamp-voice.service.d/zz-addon-say-check.conf
LABEL=${1:-}
TEXT=${2:-The rain should stop by four this afternoon.}

if [[ -z ${LAMP_HOST:-} ]]; then
  echo "addon-say.sh: LAMP_HOST is not set. Set it to your own lamp's address (see README.md). Nothing was changed." >&2
  exit 2
fi
if [[ -z $LABEL ]]; then
  echo "usage: addon-say.sh LABEL [\"the kept text\"]" >&2
  exit 2
fi
if [[ $(date +%u) -le 5 && $(date +%H) -ge 12 && $(date +%H) -lt 14 && ${LAMP_PLAY_AT_LUNCH:-} != yes ]]; then
  echo "addon-say.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi
if [[ -z $TEXT || ${#TEXT} -gt 300 || $TEXT == *$'\n'* ]]; then
  echo "addon-say.sh: the kept text is 1 to 300 bytes on one line (MOST_QUOTED_BYTES)" >&2
  exit 2
fi

lamp() { "$LAMP_SSH" "$@" </dev/null; }

# The voice as it runs: active, on the second route, with the say service.
env=$(lamp 'systemctl show -p ActiveState -p Environment lamp-voice') || { echo "addon-say.sh: no answer from the lamp" >&2; exit 3; }
[[ $env == *ActiveState=active* ]] || { echo "addon-say.sh: lamp-voice is not active; nothing changed" >&2; exit 3; }
[[ $env == *LAMP_VOICE_ROUTE=second* ]] || { echo "addon-say.sh: lamp-voice is not on the second route; nothing changed" >&2; exit 3; }
[[ $env == *LAMP_SAY=on* ]] || { echo "addon-say.sh: lamp-voice's say service is off (LAMP_SAY); nothing changed" >&2; exit 3; }
[[ $env == *LAMP_ADDON_SAY=* ]] && { echo "addon-say.sh: LAMP_ADDON_SAY is already set on lamp-voice; nothing changed" >&2; exit 3; }
socket=$(sed -n 's/.*LAMP_SAY_SOCKET=\([^ ]*\).*/\1/p' <<<"$env")
socket=${socket:-/run/lampos-say/say.sock}

put_back() {
  lamp "sudo -n rm -f $DROPIN && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
    && echo "addon-say.sh: drop-in removed, lamp-voice restarted without it" \
    || echo "addon-say.sh: putting lamp-voice back FAILED: remove $DROPIN, daemon-reload and restart lamp-voice by hand" >&2
}
trap put_back EXIT
lamp "sudo -n mkdir -p ${DROPIN%/*} && printf '[Service]\nEnvironment=LAMP_ADDON_SAY=on\n' | sudo -n tee $DROPIN >/dev/null && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-voice" \
  || { echo "addon-say.sh: the drop-in could not be put on (sudo -n)" >&2; exit 4; }
echo "addon-say.sh: lamp-voice restarted with LAMP_ADDON_SAY=on"

# Ready: the socket there again, then a settle for the voice's own first steps.
for _ in $(seq 60); do
  lamp "sudo -n test -S $socket" && break
  sleep 1
done
lamp "sudo -n test -S $socket" || { echo "addon-say.sh: the say socket never came back" >&2; exit 5; }
sleep "${ADDON_SAY_SETTLE:-10}"

start=$(lamp "python3 -c 'import time; print(time.monotonic_ns()//1000)'")
ask=$(python3 -c 'import json,sys; print(json.dumps({"skill":"addon-say-check","quoted":sys.argv[1],"expires_in_ms":60000}))' "$TEXT")
# One line in, every line out until the outcome; the connection is the say service's to close.
outcome=$(lamp "sudo -n python3 -c '
import socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout($WAIT); s.connect(\"$socket\")
s.sendall(sys.stdin.readline().encode())
print(s.makefile().read(), end=\"\")
' <<'ASK'
$ask
ASK")
outcome=$(grep '"outcome"' <<<"$outcome" | tail -n 1)
sleep 2
mkdir -p "$OUT"
events=$(mktemp)
trap 'rm -f "$events"; put_back' EXIT
lamp 'S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"' >"$events"

report=$(python3 - "$start" "$TEXT" "$outcome" "$events" <<'PY'
import json, re, sys
start, kept, outcome = int(sys.argv[1] or 0), sys.argv[2], sys.argv[3]
said = []
for line in open(sys.argv[4]):
    try:
        r = json.loads(line)
    except ValueError:
        continue
    if r.get('kind') == 'transcript' and r.get('at_us', 0) >= start and r.get('turn') is not None and r.get('role', 'output') != 'input':
        said.append((r.get('text') or '').strip())
words = lambda s: re.findall(r"[a-z0-9']+", s.lower())
k, s = words(kept), words(' '.join(said))
# The longest common subsequence: kept words said in their order.
row = [0] * (len(s) + 1)
for a in k:
    prev = 0
    for j, b in enumerate(s, 1):
        prev, row[j] = row[j], prev + 1 if a == b else max(row[j], row[j - 1])
same = row[-1]
try:
    o = json.loads(outcome)
except ValueError:
    o = {'outcome': 'none'}
print('outcome %s%s' % (o.get('outcome'), (' (%s)' % o['why']) if o.get('why') else ''))
print('kept:  %s' % kept)
print('said:  %s' % (' '.join(said) or '(nothing)'))
print('kept words %d, kept in order %d, changed %d, added %d, refused %s' % (
    len(k), same, len(k) - same, len(s) - same, 'yes' if o.get('outcome') == 'refused' else 'no'))
PY
)
echo "$report" | tee "$OUT/addon-say-$LABEL.txt"
