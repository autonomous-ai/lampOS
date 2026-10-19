#!/usr/bin/env bash
# Tests for speaking-ab.sh with no lamp: ssh runs the lamp's commands here,
# with stand-ins for sudo, systemctl, journalctl, setsid, lamp-bus and say.
# Run: lampOS/tools/speaking-ab-test.sh
set -uo pipefail

AB=$(cd "$(dirname "$0")" && pwd)/speaking-ab.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}

mkdir -p "$T/bin" "$T/lamp/bin" "$T/etc" "$T/incoming"
# Here: ssh runs the command with the lamp's stand-ins first on the PATH; scp
# copies into the lamp's incoming folder; say only says it was asked.
cat >"$T/bin/ssh" <<EOF
#!/usr/bin/env bash
shift
PATH=$T/lamp/bin:\$PATH bash -c "\$1"
EOF
cat >"$T/bin/scp" <<EOF
#!/usr/bin/env bash
args=()
for a in "\$@"; do [[ \$a == -q ]] || args+=("\$a"); done
n=\${#args[@]}
cp "\${args[@]:0:n-1}" "$T/incoming/"
EOF
cat >"$T/bin/say" <<EOF
#!/usr/bin/env bash
echo "say \$*" >>"$T/log"
EOF
# The lamp's.
cat >"$T/lamp/bin/sudo" <<'EOF'
#!/usr/bin/env bash
while [[ $1 == -* ]]; do [[ $1 == -p ]] && shift; shift; done
"$@"
EOF
cat >"$T/lamp/bin/setsid" <<'EOF'
#!/usr/bin/env bash
"$@"
EOF
cat >"$T/lamp/bin/sha256sum" <<'EOF'
#!/usr/bin/env bash
shasum -a 256 "$@"
EOF
cat >"$T/lamp/bin/systemctl" <<EOF
#!/usr/bin/env bash
echo "systemctl \$*" >>"$T/log"
case \$1 in
  is-active) [[ \${FAKE_INACTIVE:-} == "\$2" ]] && { echo inactive; exit 3; }; echo active ;;
  cat) echo "ExecStart=/opt/lampOS/expression/current/lamp-expression serve --enable --bus-root /run/lampos-bus --catalogue $T/etc/expression.json --zone system --kernel-clock"
       cat "$T/units/lamp-expression.service.d/thirteen-ab.conf" 2>/dev/null ;;
  show) echo PrivateTmp=yes ;;
  restart) cat "$T/units/lamp-expression.service.d/thirteen-ab.conf" >>"$T/dropins" 2>/dev/null || echo "(none)" >>"$T/dropins" ;;
esac
exit 0
EOF
# Three speaking steps: the base out to one side, back, and to the other.
cat >"$T/lamp/bin/journalctl" <<'EOF'
#!/usr/bin/env bash
echo "lamp-body: step 10 Resting goals [2000, 2044, 2015] positions [2000, 2044, 2015] loads [0, 0, 0]"
echo "lamp-body: step 11 Speaking goals [1990, 2044, 2060] positions [1990, 2044, 2058] loads [0, 0, 0]"
echo "lamp-body: step 12 Speaking goals [1990, 2044, 2015] positions [1990, 2044, 2016] loads [0, 0, 0]"
echo "lamp-body: step 13 Speaking goals [1990, 2044, 1972] positions [1990, 2044, 1974] loads [0, 0, 0]"
EOF
# The bus tool, as copied from "the build", printing as lamp-bus tap does:
# the header's JSON, a space, the payload's (the reviewer on #1759). Two
# states naming behaviours; VoiceSound flattened, its level at the top.
cat >"$T/lamp-bus" <<'EOF'
#!/usr/bin/env bash
h() { printf '{"topic":"%s","seq":%s,"source":"voice","type":"lamp_msgs/X"} ' "$1" "$2"; }
case $2 in
  expression/state)
    echo "$(h expression/state 1){\"behaviours\":[\"nodding on a stressed word\"]}"
    echo "$(h expression/state 2){\"behaviours\":[\"swaying between sentences\",\"nodding on a stressed word\"]}" ;;
  voice/sound)
    n=0
    for l in 0.05 0.2 0.4 0.1; do n=$((n + 1)); echo "$(h voice/sound $n){\"session_id\":\"s\",\"turn\":1,\"level\":$l,\"audio_generation\":1}"; done ;;
  voice/timing)
    echo "$(h voice/timing 1){\"turn\":1,\"state\":\"speaking\",\"event\":\"state\"}"
    echo "$(h voice/timing 2){\"turn\":1,\"state\":\"speaking\",\"event\":\"state\"}"
    echo "$(h voice/timing 3){\"turn\":1,\"state\":\"resting\",\"event\":\"finished\"}"
    echo "$(h voice/timing 4){\"turn\":2,\"state\":\"listening\",\"event\":\"state\"}" ;;
  expression/body)
    echo "$(h expression/body 1){\"offsets\":{\"base_yaw\":-0.08,\"shoulder\":0,\"elbow\":0,\"head_roll\":0,\"head_pitch\":0.035}}"
    echo "$(h expression/body 2){\"offsets\":{\"base_yaw\":0.02,\"shoulder\":0,\"elbow\":0,\"head_roll\":0,\"head_pitch\":-0.01}}" ;;
esac
EOF
printf '#!/bin/sh\n' >"$T/lamp-expression"
chmod +x "$T"/bin/* "$T"/lamp/bin/* "$T/lamp-bus" "$T/lamp-expression"
echo '{"version":1,"limits":{}}' >"$T/etc/expression.json"
echo '{"version":1,"speaking":{"nod_us":360000,"sway_yaw":0.08}}' >"$T/product.json"

run() {
  rm -rf "$T/stage" "$T/units" && mkdir -p "$T/units" && : >"$T/log" && : >"$T/dropins"
  SSH=$T/bin/ssh SCP=$T/bin/scp SAY=$T/bin/say UNIT_DIR=$T/units INCOMING=$T/incoming SPEAKING_AB_WAIT=${WAIT:-0} \
    bash "$AB" --ssh lamp --expression "$T/lamp-expression" --bus "$T/lamp-bus" --speaking "$T/product.json" \
    --stage "$T/stage" "$@" >"$T/out" 2>&1
}

run; status=$?
check "both runs go, off then on, and it ends" '[[ $status == 0 ]] && grep -q "done; ask the person" "$T/out" && [[ $(grep -c "^say -v Samantha Hey Lamp, tell me" "$T/log") == 2 ]]'
check "on is the lamp's catalogue with the speaking section, off without" '[[ $(jq -c .speaking "$T/stage/on.json") == "{\"nod_us\":360000,\"sway_yaw\":0.08}" ]] && [[ $(jq -c .speaking "$T/stage/off.json") == null ]] && grep -q same-but-speaking "$T/out"'
check "each run swaps in the build and its catalogue by a runtime drop-in" '[[ $(grep -c "systemctl restart lamp-expression" "$T/log") == 3 ]]'
check "the drop-in runs the copied build, on off.json then on.json, with the unit's own options" '[[ $(grep -c "^ExecStart=$" "$T/dropins") == 2 ]] && grep "^ExecStart=/" "$T/dropins" | head -1 | grep -q "^ExecStart=$T/stage/lamp-expression serve --enable --bus-root /run/lampos-bus --catalogue $T/stage/off.json --zone system --kernel-clock$" && grep "^ExecStart=/" "$T/dropins" | tail -1 | grep -q -- "--catalogue $T/stage/on.json --zone" && [[ $(tail -1 "$T/dropins") == "(none)" ]]'
check "the counts: speaking steps, the base yaw range, each behaviour" 'grep -q "^off speaking_steps=3$" "$T/out" && grep -q "^on base_yaw=1972-2060$" "$T/out" && grep -q "^on nodding on a stressed word=2$" "$T/out" && grep -q "^off swaying between sentences=1$" "$T/out" && grep -q "^on settling as it finishes=0$" "$T/out"'
check "the voice's levels and Expression's offsets are counted" 'grep -q "^on sound=frames 4, over 0.12 2, over 0.35 1, loudest 0.40$" "$T/out" && grep -q "^off offsets=samples 2, base yaw up to 0.080, head pitch up to 0.035$" "$T/out"'
check "at the end the drop-in is gone and Expression restarted on its own" '[[ ! -e $T/units/lamp-expression.service.d/thirteen-ab.conf ]] && grep -q "restoring lamp-expression" "$T/out" && tail -3 "$T/log" | grep -q "systemctl restart lamp-expression"'
check "the lamp's own catalogue is never written" '[[ $(cat "$T/etc/expression.json") == "{\"version\":1,\"limits\":{}}" ]]'

# One run alone, again (a run whose answer was held back).
run --arms on; status=$?
check "--arms on runs only on, and restores" '[[ $status == 0 ]] && [[ $(grep -c "^say -v" "$T/log") == 1 ]] && grep -q "^on speaking_steps=" "$T/out" && ! grep -q "^off speaking_steps=" "$T/out" && [[ ! -e $T/units/lamp-expression.service.d/thirteen-ab.conf ]]'
run --arms both; status=$?
check "--arms takes only off and on" '[[ $status == 2 ]] && ! grep -q "^say" "$T/log"'

# Levels only (the first builder, #1722 23:42): Expression in shadow, the
# body neither needed nor touched, and the starts and the bars counted.
# A second for the taps started in the background to write.
WAIT=1 FAKE_INACTIVE=lamp-body run --levels-only --arms on; status=$?
check "--levels-only runs with lamp-body down, Expression without --enable" '[[ $status == 0 ]] && grep "^ExecStart=/" "$T/dropins" | head -1 | grep -q "^ExecStart=$T/stage/lamp-expression serve --bus-root /run/lampos-bus --catalogue $T/stage/on.json --zone system --kernel-clock$"'
check "--levels-only says how the voice's last turns ended" 'grep -q "^on ends=1 speaking state;1 resting finished;2 listening state$" "$T/out"'
check "--levels-only counts each behaviour's starts and the voice against the scaled bars" 'grep -q "^on starts=nod 1, sway 1, settle 0$" "$T/out" && grep -q "^on bars=mean 0.19, speaking bar 0.07 reached 3, stress bar 0.19 reached 2, of 4$" "$T/out"'
rm -rf "$T/stage" "$T/units" && mkdir -p "$T/units" && : >"$T/log"
set -m
SSH=$T/bin/ssh SCP=$T/bin/scp SAY=$T/bin/say UNIT_DIR=$T/units INCOMING=$T/incoming SPEAKING_AB_WAIT=30 \
  bash "$AB" --ssh lamp --expression "$T/lamp-expression" --bus "$T/lamp-bus" --speaking "$T/product.json" \
  --stage "$T/stage" --levels-only >"$T/out" 2>&1 &
ab=$!
set +m
for _ in $(seq 100); do grep -q "^say" "$T/log" && break; sleep 0.1; done
kill -INT -- -"$ab"; wait "$ab"; status=$?
check "--levels-only Ctrl-C never touches lamp-body, and restores Expression" '[[ $status == 130 ]] && ! grep -q "lamp-body" <(grep "systemctl kill" "$T/log") && [[ ! -e $T/units/lamp-expression.service.d/thirteen-ab.conf ]]'

# A unit that is down: nothing is said, and the restore still runs.
FAKE_INACTIVE=lamp-voice run; status=$?
check "lamp-voice down: refused before anything is said" '[[ $status == 1 ]] && grep -q "must all be active" "$T/out" && ! grep -q "^say" "$T/log" && grep -q "restoring lamp-expression" "$T/out"'

# A catalogue without a speaking section to add is refused here.
echo '{"version":1}' >"$T/bad.json"
SSH=$T/bin/ssh SCP=$T/bin/scp SAY=$T/bin/say bash "$AB" --ssh lamp --expression "$T/lamp-expression" --bus "$T/lamp-bus" \
  --speaking "$T/bad.json" >"$T/out" 2>&1; status=$?
check "a --speaking file with no section is refused" '[[ $status == 2 ]] && grep -q "has no speaking section" "$T/out"'

# Stop on a word: Ctrl-C sends lamp-body SIGTERM, then the restore.
rm -rf "$T/stage" "$T/units" && mkdir -p "$T/units" && : >"$T/log"
set -m
SSH=$T/bin/ssh SCP=$T/bin/scp SAY=$T/bin/say UNIT_DIR=$T/units INCOMING=$T/incoming SPEAKING_AB_WAIT=30 \
  bash "$AB" --ssh lamp --expression "$T/lamp-expression" --bus "$T/lamp-bus" --speaking "$T/product.json" \
  --stage "$T/stage" >"$T/out" 2>&1 &
ab=$!
set +m
for _ in $(seq 100); do grep -q "^say" "$T/log" && break; sleep 0.1; done
kill -INT -- -"$ab"; wait "$ab"; status=$?
check "Ctrl-C: lamp-body gets SIGTERM, Expression is restored, no second run" '[[ $status == 130 ]] && grep -q "systemctl kill -s TERM lamp-body" "$T/log" && [[ ! -e $T/units/lamp-expression.service.d/thirteen-ab.conf ]] && [[ $(grep -c "^say" "$T/log") == 1 ]]'

echo "$failures failed"
[[ $failures == 0 ]]
