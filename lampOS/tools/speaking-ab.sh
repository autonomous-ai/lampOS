#!/usr/bin/env bash
# #933's A/B on one lamp (#1759): the same spoken request twice, Expression's
# catalogue without its `speaking` section (off) and then with it (on), and
# what the lamp did each time, as counts. Run on the computer beside the lamp
# (its `say` speaks the request), with the lamp's person at the arm.
#
#   speaking-ab.sh --ssh TARGET --expression FILE --bus FILE --speaking FILE
#                  [--seconds 150] [--voice Samantha] [--line TEXT] [--stage DIR]
#                  [--arms off,on] [--levels-only]
#
#   --ssh         where ssh reaches the lamp (an alias, or user@host)
#   --expression  the lamp-expression binary to run for both (one that reads
#                 #1067's sway_yaw), copied to the lamp
#   --bus         a lamp-bus binary for the lamp, from the same ARM64 build
#   --speaking    a catalogue whose `speaking` section is added for "on"
#                 (deploy/product/settings/expression.json from #1759)
#   --seconds     how long each run is watched after the request (150)
#   --arms        which runs, in order (off,on); one may be run again alone,
#                 when a run's answer was held back and nothing was said
#   --levels-only nothing moves: Expression runs in shadow (its unit's
#                 --enable left out), so it plans and names each nod and sway
#                 on expression/state but asks the body for nothing; lamp-body
#                 need not run and is never touched. For a night with nobody
#                 at the arm (the first builder, #1722 23:42): the voice's
#                 levels against the bars and how often it would move
#   --stage       where the files go on the lamp (/var/lib/thirteen-ab: the
#                 unit's PrivateTmp hides /tmp and /var/tmp, ProtectHome the
#                 home, and /run is mounted noexec, as on lamp-c0a4). It
#                 stays after a run, the binaries, both catalogues and the
#                 taps with it, so the counts can be read again; remove it
#                 by hand (sudo rm -r /var/lib/thirteen-ab) when done
#
# Off is the catalogue lamp-expression runs with now; on is that plus the
# section, checked to differ by nothing else. Both run the same binary, put in
# by a runtime drop-in (/run/systemd/system/lamp-expression.service.d/), so the
# unit keeps every setting of its own and nothing in /opt or /etc changes.
# lamp-body and lamp-voice are not touched. Each run: Expression restarted,
# `lamp-bus tap expression/state` kept on the lamp, the request said here,
# then counted from the lamp:
#   speaking steps   lamp-body's "Speaking goals" lines
#   base yaw         the lowest and highest yaw goal among them (counts)
#   behaviours       expression/state samples naming each speaking behaviour
#   sound            voice/sound frames, how many over the speaking (0.12)
#                    and stressed-word (0.35) levels, and the loudest
#   offsets          the most Expression asked of the base's yaw and the
#                    head's pitch on expression/body (radians)
#   Each tap line is lamp-bus's: the header's JSON, a space, then the
#   payload's; VoiceSound's playout is flattened, so its level is `.level`.
#   starts           how many times each speaking behaviour began
#   ends             the voice's last turns as voice/timing said them, each
#                    change of turn, state or event in order: whether the
#                    answer finished (a settle is owed) or was cut or
#                    followed at once by a new turn (none is; #955)
#   bars             the voice's mean level over frames with sound, and how
#                    many frames reach the speaking and stressed-word bars as
#                    Expression scales them by that mean (#1953, #1976: each
#                    bar times mean/0.34, the stress bar at most 0.9)
# On any exit the drop-in goes and lamp-expression restarts on its own
# catalogue. Ctrl-C (or SIGTERM) first sends lamp-body SIGTERM, which eases the
# arm back (it then stays stopped), for when the person says stop.
#
# Root on the lamp: sudo, its password from LAMP_SUDO_PASS here if set (never
# written anywhere), else sudo must not ask. For a test, SSH, SCP and SAY
# replace ssh, scp and say; UNIT_DIR the lamp's /run/systemd/system, INCOMING
# its /tmp, and SPEAKING_AB_WAIT the seconds each run is watched.
set -uo pipefail

usage() { sed -n '2,61p' "$0" | sed 's/^# \{0,1\}//'; }

TARGET='' EXPRESSION='' BUS='' SPEAKING='' SECONDS_EACH=150 VOICE=Samantha STAGE=/var/lib/thirteen-ab
ARMS=off,on LEVELS_ONLY=0
LINE='Hey Lamp, tell me a two-minute story about a cat who loved the rain.'
SSH=${SSH:-ssh} SCP=${SCP:-scp} SAY=${SAY:-say}
while [[ $# -gt 0 ]]; do
  case $1 in
    --ssh) TARGET=${2:-}; shift 2 ;;
    --expression) EXPRESSION=${2:-}; shift 2 ;;
    --bus) BUS=${2:-}; shift 2 ;;
    --speaking) SPEAKING=${2:-}; shift 2 ;;
    --seconds) SECONDS_EACH=${2:-}; shift 2 ;;
    --voice) VOICE=${2:-}; shift 2 ;;
    --line) LINE=${2:-}; shift 2 ;;
    --stage) STAGE=${2:-}; shift 2 ;;
    --arms) ARMS=${2:-}; shift 2 ;;
    --levels-only) LEVELS_ONLY=1; shift ;;
    --help) usage; exit 0 ;;
    *) echo "speaking-ab: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
done
[[ -n $TARGET && -r $EXPRESSION && -r $BUS && -r $SPEAKING ]] \
  || { echo "speaking-ab: --ssh, and a readable --expression, --bus and --speaking, are required" >&2; exit 2; }
[[ $SECONDS_EACH =~ ^[0-9]+$ && $SECONDS_EACH -ge 10 && $SECONDS_EACH -le 600 ]] \
  || { echo "speaking-ab: --seconds is 10 to 600" >&2; exit 2; }
[[ $ARMS =~ ^(off|on)(,(off|on))?$ ]] || { echo "speaking-ab: --arms is off, on, off,on or on,off" >&2; exit 2; }
[[ $STAGE == /* && $STAGE != */.* ]] || { echo "speaking-ab: --stage is an absolute path" >&2; exit 2; }
command -v jq >/dev/null || { echo "speaking-ab: jq is needed here" >&2; exit 2; }
jq -e '.speaking | type == "object"' "$SPEAKING" >/dev/null \
  || { echo "speaking-ab: $SPEAKING has no speaking section" >&2; exit 2; }

say_() { echo "speaking-ab: $*"; }
# One command on the lamp, as root.
on_lamp() {
  if [[ -n ${LAMP_SUDO_PASS:-} ]]; then
    printf '%s\n' "$LAMP_SUDO_PASS" | "$SSH" "$TARGET" "sudo -S -p '' bash -c $(printf '%q' "$1")"
  else
    "$SSH" "$TARGET" "sudo -n bash -c $(printf '%q' "$1")" </dev/null
  fi
}
DROPIN=${UNIT_DIR:-/run/systemd/system}/lamp-expression.service.d/thirteen-ab.conf
INCOMING=${INCOMING:-/tmp}

restore() {
  trap - EXIT INT TERM
  say_ "restoring lamp-expression to its own catalogue"
  on_lamp "rm -f $DROPIN; systemctl daemon-reload; systemctl restart lamp-expression; sleep 2; systemctl is-active lamp-expression; systemctl cat lamp-expression | grep -c thirteen-ab || true" \
    || echo "speaking-ab: could not restore lamp-expression: remove $DROPIN on the lamp and restart it" >&2
}
stop_body() {
  trap - INT TERM
  if [[ $LEVELS_ONLY == 1 ]]; then
    echo "speaking-ab: stopped (levels only: lamp-body is not touched)" >&2
    exit 130
  fi
  echo "speaking-ab: stopped: lamp-body is sent SIGTERM (it eases the arm back and stays stopped)" >&2
  on_lamp "systemctl kill -s TERM lamp-body" || true
  exit 130
}
trap restore EXIT
trap stop_body INT TERM

# A wait the traps can cut short.
pause() {
  sleep "$1" &
  wait $!
}

# The lamp as it runs now: units up, and the catalogue Expression reads.
say_ "preflight"
pre=$(on_lamp 'for u in lamp-body lamp-voice lamp-expression; do printf "%s=%s " $u "$(systemctl is-active $u)"; done; echo
line=$(systemctl cat lamp-expression | grep "^ExecStart=/" | tail -1)
echo "exec=${line#ExecStart=}"
echo "catalogue=$(grep -oE -- "--catalogue [^ ]+" <<<"$line" | cut -d" " -f2)"
systemctl show -p PrivateTmp lamp-expression') || { echo "speaking-ab: preflight failed" >&2; exit 1; }
echo "$pre"
if [[ $LEVELS_ONLY == 1 ]]; then
  grep -q "lamp-voice=active lamp-expression=active" <<<"$pre" \
    || { echo "speaking-ab: lamp-voice and lamp-expression must be active" >&2; exit 1; }
else
  grep -q "lamp-body=active lamp-voice=active lamp-expression=active" <<<"$pre" \
    || { echo "speaking-ab: lamp-body, lamp-voice and lamp-expression must all be active" >&2; exit 1; }
fi
EXEC=$(sed -n 's/^exec=//p' <<<"$pre")
CATALOGUE=$(sed -n 's/^catalogue=//p' <<<"$pre")
[[ -n $EXEC && -n $CATALOGUE ]] || { echo "speaking-ab: lamp-expression's ExecStart or --catalogue not found" >&2; exit 1; }

# The files, and the two catalogues built from the lamp's own.
jq '.speaking' "$SPEAKING" >"${TMPDIR:-/tmp}/speaking-ab-section.json"
on_lamp "install -d -m 0755 $STAGE" || exit 1
"$SCP" -q "$EXPRESSION" "$BUS" "${TMPDIR:-/tmp}/speaking-ab-section.json" "$TARGET:$INCOMING/" || { echo "speaking-ab: copy failed" >&2; exit 1; }
built=$(on_lamp "install -m 0755 $INCOMING/$(basename "$EXPRESSION") $STAGE/lamp-expression
install -m 0755 $INCOMING/$(basename "$BUS") $STAGE/lamp-bus
install -m 0644 $INCOMING/speaking-ab-section.json $STAGE/section.json
install -m 0644 $CATALOGUE $STAGE/off.json
jq --slurpfile s $STAGE/section.json '.speaking = \$s[0]' $STAGE/off.json >$STAGE/on.json && chmod 0644 $STAGE/on.json
cmp -s <(jq -S 'del(.speaking)' $STAGE/off.json) <(jq -S 'del(.speaking)' $STAGE/on.json) && echo same-but-speaking
jq -r 'has(\"speaking\")' $STAGE/off.json
sha256sum $STAGE/lamp-expression $STAGE/lamp-bus $STAGE/off.json $STAGE/on.json | cut -c1-16") \
  || { echo "speaking-ab: could not build the catalogues on the lamp" >&2; exit 1; }
echo "$built"
grep -q same-but-speaking <<<"$built" || { echo "speaking-ab: off and on differ by more than speaking" >&2; exit 1; }
grep -qx false <<<"$built" || say_ "note: the lamp's catalogue already has a speaking section; off keeps it"

# One run: Expression on CATALOGUE, the request said, then counted.
run() {
  local name=$1 exec_line since
  exec_line=$(sed -E "s#^[^ ]+#$STAGE/lamp-expression#; s#--catalogue [^ ]+#--catalogue $STAGE/$name.json#" <<<"$EXEC")
  # Levels only: shadow, so Expression asks the body for nothing.
  [[ $LEVELS_ONLY == 1 ]] && exec_line=$(sed -E 's/ --enable( |$)/\1/g' <<<"$exec_line")
  on_lamp "install -d -m 0755 $(dirname $DROPIN)
printf '[Service]\nExecStart=\nExecStart=%s\n' $(printf '%q' "$exec_line") >$DROPIN
systemctl daemon-reload && systemctl restart lamp-expression && sleep 4 && systemctl is-active lamp-expression" \
    || { echo "speaking-ab: lamp-expression did not start for $name" >&2; return 1; }
  since=$(on_lamp 'date +%s') || return 1
  local topic
  for topic in expression/state voice/sound expression/body voice/timing; do
    on_lamp "rm -f $STAGE/$name.${topic#*/}; setsid $STAGE/lamp-bus tap $topic --seconds $((SECONDS_EACH + 10)) --root /run/lampos-bus >$STAGE/$name.${topic#*/} 2>&1 </dev/null &" || return 1
  done
  say_ "$name: saying the request"
  "$SAY" -v "$VOICE" "$LINE" || say_ "note: $SAY failed; say the request aloud now"
  pause "${SPEAKING_AB_WAIT:-$SECONDS_EACH}"
  on_lamp "echo \"$name speaking_steps=\$(journalctl -u lamp-body --since @$since --no-pager -o cat | grep -c ' Speaking goals ')\"
echo \"$name base_yaw=\$(journalctl -u lamp-body --since @$since --no-pager -o cat | grep -oE ' Speaking goals \\[[0-9]+, [0-9]+, [0-9]+\\]' | sed -E 's/.*, ([0-9]+)\\]/\\1/' | sort -n | sed -n '1p;\$p' | paste -sd- -)\"
for b in 'nodding on a stressed word' 'swaying between sentences' 'settling as it finishes'; do echo \"$name \$b=\$(grep -c \"\$b\" $STAGE/$name.state)\"; done
echo \"$name sound=\$(jq -R 'sub(\"^[{][^ ]*[}] \"; \"\") | fromjson? | .level? // empty' $STAGE/$name.sound | awk '{n++; if (\$1>=0.12) s++; if (\$1>=0.35) t++; if (\$1>m) m=\$1} END {printf \"frames %d, over 0.12 %d, over 0.35 %d, loudest %.2f\", n, s, t, m}')\"
echo \"$name starts=\$(jq -r -R 'sub(\"^[{][^ ]*[}] \"; \"\") | fromjson? | .behaviours? // empty | [(index(\"nodding on a stressed word\") != null), (index(\"swaying between sentences\") != null), (index(\"settling as it finishes\") != null)] | map(if . then 1 else 0 end) | @tsv' $STAGE/$name.state | awk '{for (i=1;i<=3;i++) {if (\$i && !w[i]) c[i]++; w[i]=\$i}} END {printf \"nod %d, sway %d, settle %d\", c[1], c[2], c[3]}')\"
echo \"$name bars=\$(jq -R 'sub(\"^[{][^ ]*[}] \"; \"\") | fromjson? | .level? // empty' $STAGE/$name.sound | awk '\$1>=0.01 {v[++n]=\$1; t+=\$1} END {if (!n) {print \"no sound\"; exit} m=t/n; k=m/0.34; if (k<0.25) k=0.25; if (k>3) k=3; sp=0.12*k; if (sp<0.03) sp=0.03; st=0.35*k; if (st>0.9) st=0.9; if (st<0.06) st=0.06; for (i=1;i<=n;i++) {if (v[i]>=sp) a++; if (v[i]>=st) b++} printf \"mean %.2f, speaking bar %.2f reached %d, stress bar %.2f reached %d, of %d\", m, sp, a, st, b, n}')\"
echo \"$name ends=\$(jq -r -R 'sub(\"^[{][^ ]*[}] \"; \"\") | fromjson? | select(.turn != null) | \"\\(.turn) \\(.state) \\(.event)\"' $STAGE/$name.timing | uniq | tail -6 | paste -sd ';' -)\"
echo \"$name offsets=\$(jq -r -R 'sub(\"^[{][^ ]*[}] \"; \"\") | fromjson? | .offsets? // empty | [.base_yaw, .head_pitch] | @tsv' $STAGE/$name.body | awk '{y=(\$1<0?-\$1:\$1); p=(\$2<0?-\$2:\$2); if (y>my) my=y; if (p>mp) mp=p; n++} END {printf \"samples %d, base yaw up to %.3f, head pitch up to %.3f\", n, my, mp}')\""
}

for arm in ${ARMS//,/ }; do
  case $arm in
    off) say_ "off: the lamp's catalogue as it is" ;;
    on) say_ "on: the same with the speaking section" ;;
  esac
  run "$arm" || exit 1
done
say_ "done; ask the person which read as alive"
