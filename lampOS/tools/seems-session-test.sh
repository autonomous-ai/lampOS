#!/usr/bin/env bash
# Tests for seems-session.sh with no lamp, no camera and no person: key
# presses from a pipe, stand-ins for systemctl, systemd-run and the measure,
# and made-up reads and Mood states.
# Run: lampOS/tools/seems-session-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/seems-session.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then
    echo "ok   $1"
  else
    echo "FAIL $1"
    failures=$((failures + 1))
  fi
}

# labels: the stretches in order, Space as the stretch's own label, p, n and
# u as the person says, s to the next stretch, q to stop.
printf '  p' | STRETCH=1 STRETCHES="$T/stretches.tsv" "$SCRIPT" labels >"$T/labels" 2>"$T/labels.err"
check "Space is the stretch's own label, p the person's" \
  '[ "$(sed "s/\"t\": [0-9]*/\"t\": T/" "$T/labels")" = "$(printf "%s\n" "{\"t\": T, \"says\": \"neutral\"}" "{\"t\": T, \"says\": \"neutral\"}" "{\"t\": T, \"says\": \"positive\"}")" ]'
check "five stretches, each said first" \
  '[ "$(wc -l <"$T/stretches.tsv" | tr -d " ")" = 5 ] && [ "$(grep -c "^seems-session: next" "$T/labels.err")" = 5 ]'
check "the stretches in order, reading before talking" \
  '[ "$(cut -f1 "$T/stretches.tsv" | tr "\n" "|")" = "resting, looking at the lamp|smiling|a long face, as when tired or low|reading or working at the desk, concentrating|talking, as to someone at the desk|" ]'
check "a stretch's times, begun then ended" \
  'awk -F"\t" "\$3 < \$2 { bad = 1 } END { exit bad }" "$T/stretches.tsv"'

printf ' s nuq' | STRETCH=3 STRETCHES="$T/stretches-q.tsv" "$SCRIPT" labels >"$T/labels-q" 2>/dev/null
check "s goes to the next stretch: Space there is a smile" \
  '[ "$(grep -o "\"says\": \"[a-z]*\"" "$T/labels-q" | tr "\n" "|")" = "\"says\": \"neutral\"|\"says\": \"positive\"|\"says\": \"negative\"|\"says\": \"neutral\"|" ]'
check "q stops the session at its stretch" '[ "$(wc -l <"$T/stretches-q.tsv" | tr -d " ")" = 2 ]'

# on and off, with stand-ins: the units' own command lines kept, the flags and
# switches added, the tap started, and everything taken back.
mkdir -p "$T/bin" "$T/dropins" "$T/models"
cat >"$T/bin/systemctl" <<'SH'
#!/usr/bin/env bash
echo "$*" >>"$CALLS"
case "$1" in
  cat) printf '# /etc/systemd/system/%s\n[Service]\nExecStart=/opt/lampOS/%s/current/%s --own-flag%s\n' "$2" "${2%.service}" "${2%.service}" "${OWN_EXTRA:-}" ;;
  is-active) echo "${MOOD_WAS:-inactive}"; [ "${MOOD_WAS:-inactive}" = active ] ;;
esac
SH
cat >"$T/bin/systemd-run" <<'SH'
#!/usr/bin/env bash
echo "systemd-run $*" >>"$CALLS"
SH
chmod +x "$T/bin/systemctl" "$T/bin/systemd-run"
for model in ferplus mesh blend; do : >"$T/models/$model.onnx"; done
env_on() {
  CALLS="$T/calls" PATH="$T/bin:$PATH" DROPINS="$T/dropins" DIR="$T/run" SEEMS_LOG="$T/seems.log" \
    FERPLUS="$T/models/ferplus.onnx" MESH="$T/models/mesh.onnx" BLEND="$T/models/blend.onnx" BUS=/bin/lamp-bus "$@"
}
env_on "$SCRIPT" on 2>/dev/null
eyes=$(cat "$T/dropins/lamp-eyes.service.d/zz-seems-session.conf" 2>/dev/null)
mood=$(cat "$T/dropins/lamp-mood.service.d/zz-seems-session.conf" 2>/dev/null)
check "the eyes' own command line, with both readers and the log added" \
  '[ "$(printf "%s\n" "$eyes" | sed -n 3p)" = "ExecStart=/opt/lampOS/lamp-eyes/current/lamp-eyes --own-flag --seems $T/models/ferplus.onnx --mouth-model $T/models/mesh.onnx --seems-blendshapes $T/models/blend.onnx --seems-log $T/seems.log" ] && [ "$(printf "%s\n" "$eyes" | sed -n 2p)" = "ExecStart=" ]'
check "the brows' switch on for the eyes" 'printf "%s\n" "$eyes" | grep -qx "Environment=LAMP_SEEMS_BROW_LOW=on"'
check "Mood on with the face's input, its own command line kept" \
  'printf "%s\n" "$mood" | grep -qx "Environment=LAMP_MOOD=on LAMP_MOOD_SEEMS=on" && printf "%s\n" "$mood" | grep -qx "ExecStart=/opt/lampOS/lamp-mood/current/lamp-mood --own-flag"'
check "Mood's state tapped to the session's folder, and off in an hour" \
  'grep -q "systemd-run .*StandardOutput=file:$T/run/mood.txt /bin/lamp-bus --root /run/lampos-bus tap mood/state" "$T/calls" && grep -q "systemd-run .*--on-active=1h .* off" "$T/calls"'
check "the folder is the session's own" '[ "$(stat -c %a "$T/run")" = 700 ]'
: >"$T/seems.log"
env_on "$SCRIPT" off 2>/dev/null
check "off: no drop-in, no log, no folder left" \
  '[ -z "$(ls -A "$T/dropins")" ] && [ ! -e "$T/seems.log" ] && [ ! -e "$T/run" ]'
check "off: Mood stopped, as it was" 'tail -2 "$T/calls" | grep -qx "stop lamp-mood.service"'
: >"$T/calls"
env_on env MOOD_WAS=active "$SCRIPT" on 2>/dev/null
env_on env MOOD_WAS=active "$SCRIPT" off 2>/dev/null
check "off: Mood running again, as it was" 'tail -1 "$T/calls" | grep -qx "restart lamp-mood.service"'
# With no FER+ model (it isn't shipped), Blendshape alone.
env_on env FERPLUS= "$SCRIPT" on 2>/dev/null
check "no FER+: on runs, Blendshape alone" \
  '[ $? = 0 ] && grep -qx "ExecStart=/opt/lampOS/lamp-eyes/current/lamp-eyes --own-flag --mouth-model $T/models/mesh.onnx --seems-blendshapes $T/models/blend.onnx --seems-log $T/seems.log" "$T/dropins/lamp-eyes.service.d/zz-seems-session.conf"'
printf '{"t":1001,"kind":"positive","confidence":0.9,"from":"blendshapes"}\n' >"$T/seems.log"
printf 'smiling\t1000\t1010\n' >"$T/stretches1.tsv"
printf '{"t": 1001, "says": "positive"}\n' >"$T/labels1"
cat >"$T/bin/measure" <<'SH'
#!/usr/bin/env bash
rm -f "$1" "$2"
printf 'reader\tat\tjudged\twrong\trate\tunder the bar\nfer_plus\t0.5\t0\t0\t0.000\ttoo few\nblendshapes\t0.5\t1\t0\t0.000\ttoo few\n'
SH
chmod +x "$T/bin/measure"
CALLS="$T/calls" DIR="$T/run" SEEMS_LOG="$T/seems.log" MEASURE="$T/bin/measure" \
  "$SCRIPT" read "$T/labels1" "$T/stretches1.tsv" >"$T/read1" 2>&1
code=$?
check "no FER+: read prints Blendshape's rows only" \
  '[ $code = 0 ] && grep -qx "smiling	blendshapes	1	0	0	0	0" "$T/read1" && grep -q "^blendshapes	0.5" "$T/read1" && ! grep -q fer_plus "$T/read1" && [ ! -e "$T/run/no-fer-plus" ]'
env_on env FERPLUS= "$SCRIPT" off 2>/dev/null
# The product's unit already runs the mesh and Blendshape (#2799): neither is
# given twice, which lamp-eyes would refuse.
env_on env FERPLUS= OWN_EXTRA=" --mouth-model /m.onnx --seems-blendshapes /b.onnx" "$SCRIPT" on 2>/dev/null
check "the product's readers are not given twice" \
  'grep -qx "ExecStart=/opt/lampOS/lamp-eyes/current/lamp-eyes --own-flag --mouth-model /m.onnx --seems-blendshapes /b.onnx --seems-log $T/seems.log" "$T/dropins/lamp-eyes.service.d/zz-seems-session.conf"'
env_on env FERPLUS= "$SCRIPT" off 2>/dev/null

# read: each stretch's reads by reader and kind, Mood's smiles and quiets,
# then the measure with --delete; nothing of the session left.
mkdir -p "$T/run"
printf 'smiling\t1000\t1010\nreading or working at the desk, concentrating\t1020\t1030\n' >"$T/stretches2.tsv"
{
  for t in 1001 1003 1005; do printf '{"t":%s,"kind":"positive","confidence":0.9,"from":"blendshapes"}\n' "$t"; done
  printf '{"t":1002,"kind":"unsure","confidence":0.1,"from":"fer_plus"}\n'
  printf '{"t":1022,"kind":"negative","confidence":0.85,"from":"blendshapes"}\n'
  printf '{"t":1024,"kind":"unsure","confidence":0.2}\n'
} >"$T/seems.log"
{
  echo "Started seems-session-mood.service"
  for t in 1000 1004 1005 1021 1023 1024; do
    case $t in 1004) e=0.40 w=0.63 ;; 1005) e=0.39 w=0.63 ;; 1023) e=0.20 w=0.63 ;; 1021 | 1024) e=0.30 w=0.63 ;; *) e=0.30 w=0.60 ;; esac
    printf '{"topic":"mood/state","at_unix_ms":%s} {"energy":%s,"warmth":%s}\n' "$((t * 1000))" "$e" "$w"
  done
} >"$T/run/mood.txt"
cat >"$T/bin/measure" <<'SH'
#!/usr/bin/env bash
echo "measure $*" >>"$CALLS"
rm -f "$1" "$2"
echo "reader	at	judged	wrong	rate	under the bar"
SH
chmod +x "$T/bin/measure"
printf '{"t": 1001, "says": "positive"}\n' >"$T/labels2"
CALLS="$T/calls" DIR="$T/run" SEEMS_LOG="$T/seems.log" MEASURE="$T/bin/measure" \
  "$SCRIPT" read "$T/labels2" "$T/stretches2.tsv" >"$T/read" 2>&1
code=$?
check "read runs" '[ $code = 0 ]'
check "smiling: Blendshape's three smiles, FER+'s unsure, Mood's one smile" \
  'grep -qx "smiling	blendshapes	3	0	0	1	0" "$T/read" && grep -qx "smiling	fer_plus	0	0	1	1	0" "$T/read"'
check "reading: Blendshape's one long face, a read with no from is FER+'s, Mood's one quiet" \
  'grep -qx "reading or working at the desk, concentrating	blendshapes	0	1	0	0	1" "$T/read" && grep -qx "reading or working at the desk, concentrating	fer_plus	0	0	1	0	1" "$T/read"'
check "then the measure, with --delete" 'grep -qx "measure $T/seems.log $T/labels2 --delete" "$T/calls" && grep -q "^reader	at	judged" "$T/read"'
check "nothing of the session left" '[ ! -e "$T/seems.log" ] && [ ! -e "$T/labels2" ] && [ -z "$(ls -A "$T/run")" ]'

"$SCRIPT" read "$T/nothing-here" >/dev/null 2>&1
check "an unreadable file is refused" '[ $? = 2 ]'
"$SCRIPT" >/dev/null 2>&1
check "no mode prints the use and fails" '[ $? = 2 ]'

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
