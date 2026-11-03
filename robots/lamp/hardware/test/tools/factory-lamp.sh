#!/usr/bin/env bash
# factory-lamp.sh — the factory run for many lamps from the Mac at the bench
# (#3530). The lamp runs its own steps (factory-run.sh) and says each one on
# its speaker; the Mac only starts runs and collects results, so one person
# keeps several lamps going at once.
#
#   factory-lamp.sh start ID...         lamps not set up, one after another:
#                                       join lamp-ID's hotspot, copy the kit,
#                                       its servos (a lamp never calibrated is
#                                       calibrated here, by hand, ~3 min),
#                                       start its run on the lamp, leave (~20 s)
#   factory-lamp.sh follow ID           stay on lamp-ID's hotspot and print its
#                                       run as it goes, until PASS or FAIL
#   factory-lamp.sh selftest TARGET...  lamps set up from the app, on the bench
#                                       network, all at once: step 4 on each;
#                                       RESET=yes answers a passed one's reset
#   factory-lamp.sh collect TARGET...   every lamp's record and status in one
#                                       table ($STATE/factory-records.csv)
#   factory-lamp.sh calibrate ID        start's calibration on its own (after a
#                                       servo or horn is changed): V1's servo
#                                       settings, then a hand sweep of every
#                                       joint, the plan shown, written on a yes
#
# TARGET is a bench-network IP or a lamp ID (found as lamp-ID.local; an unset
# lamp is read over its hotspot). The unset run, on the lamp:
#   checks (r2, 4b camera, 4e build, 4f, 4g, the servos) → the head onto its stop →
#   step 1, the arm stand → every input in one window (input-check.py: 30 s
#   to touch both head pads, press the button and the reset briefly, any
#   order; then 30 s hands off) → step 3, the key → 4d, --gaze
# Each lamp keeps its record in /var/lib/lamp-test/factory.env (which the
# reset keeps): HEAD_PAD_LINES (two of 37, 96, 97, 98, differing by unit),
# the button, the reset, the privacy switch, the stand, the build, each step.
#
# The Mac keeps its internet on Ethernet (first in the service order) while
# its Wi-Fi joins each lamp's hotspot. Every unset lamp is at 192.168.100.1,
# so each gets its own known_hosts and its hostname is read before anything
# runs. Environment: LAMP_PASS (required), STEPS (steps.sh from lamp-voice),
# STEPS_SHA (its sha256, insisted on), PROMPTS (from make-prompts.sh),
# STATE (default ~/.factory-lamp), RESET=yes (selftest), GAZE, NUDGE_TARGET,
# SWEEP_S (calibrate, default 120).
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
MODE=${1:-}; shift || true
PASS=${LAMP_PASS:?set LAMP_PASS to the lamp password}
PROMPTS=${PROMPTS:-$HERE/prompts}
STEPS=${STEPS:-$HERE/steps.sh}
STATE=${STATE:-$HOME/.factory-lamp}
mkdir -p "$STATE/records"
say() { echo "factory-lamp: $*"; }
die() { echo "factory-lamp: FAILED: $*" >&2; exit 1; }
usage() { sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[[ $MODE =~ ^(start|follow|selftest|collect|calibrate)$ && $# -ge 1 ]] || usage
if [[ $MODE == start || $MODE == selftest || $MODE == calibrate ]]; then
  [[ -f $STEPS ]] || die "no steps.sh at $STEPS"
  [[ -z ${STEPS_SHA:-} || $(shasum -a 256 "$STEPS" | cut -c1-64) == "$STEPS_SHA" ]] || die "steps.sh is not $STEPS_SHA"
fi

# expect drives ssh's and sudo's password prompts (no key is left on a
# shipped lamp). HOST and KH name the lamp a call goes to.
carried() { echo "echo $(printf '%s\n' "$1" | base64 | tr -d '\n') | base64 -d > /tmp/factory-lamp.sh && sudo -S -p SUDOPW: bash /tmp/factory-lamp.sh"; }
ssh_once() {
  P="$PASS" C="$(carried "$1")" H="$HOST" KH="$KH" expect <<'EOF' | tr -d '\r' | grep -vE "assword:|SUDOPW|^Warning: Permanently"
set timeout 300
log_user 1
spawn -noecho ssh -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no -o ConnectTimeout=10 orangepi@$env(H) $env(C)
expect {
  -re "assword:" { send "$env(P)\r"; exp_continue }
  "SUDOPW:" { send "$env(P)\r"; exp_continue }
  "Permission denied" { exit 5 }
  eof
}
catch wait r
exit [lindex $r 3]
EOF
  return "${PIPESTATUS[0]}"
}
# One root command on the lamp. A changed host key (a re-flashed card) is
# forgotten once; the hostname check after every connect decides.
lamp() {
  local out rc
  out=$(ssh_once "$1"); rc=$?
  if grep -q 'REMOTE HOST IDENTIFICATION HAS CHANGED' <<<"$out"; then
    ssh-keygen -R "$HOST" -f "$KH" >/dev/null 2>&1
    out=$(ssh_once "$1"); rc=$?
  fi
  grep -vE '^@|IDENTIFICATION|eavesdropping|man-in-the-middle|fingerprint|SHA256:|administrator|known_hosts|Offending|Host key|strict checking|^IT IS POSSIBLE|^Someone|^Please contact|^Add correct' <<<"$out"
  return $rc
}
put() {
  P="$PASS" F="$*" H="$HOST" KH="$KH" expect <<'EOF'
set timeout 300
log_user 0
eval spawn -noecho scp -q -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no $env(F) orangepi@$env(H):/tmp/
expect {
  -re "assword:" { send "$env(P)\r"; exp_continue }
  eof
}
catch wait r
exit [lindex $r 3]
EOF
}
# The kit: steps.sh (as steps.sh, checked by hash there), the tools, the prompts.
kit() {
  local stage want; stage=$(mktemp -d); cp "$STEPS" "$stage/steps.sh"
  want=$(shasum -a 256 "$STEPS" | cut -c1-64)
  put "$stage/steps.sh" "$HERE/factory-run.sh" "$HERE/factory-voice.py" "$HERE/head-nudge.py" \
      "$HERE/input-check.py" "$HERE/ring-check.py" "$HERE/servo-calib.py" "$PROMPTS"/factory_*.wav >/dev/null || { rm -rf "$stage"; return 1; }
  rm -rf "$stage"
  lamp "mkdir -p /var/lib/lamp-test/prompts && mv /tmp/factory_*.wav /var/lib/lamp-test/prompts/ &&
        mv /tmp/steps.sh /tmp/factory-run.sh /tmp/factory-voice.py /tmp/head-nudge.py /tmp/input-check.py /tmp/ring-check.py /tmp/servo-calib.py /home/orangepi/ &&
        chmod 0755 /home/orangepi/steps.sh /home/orangepi/factory-run.sh &&
        sha256sum /home/orangepi/steps.sh | grep -q ^$want" >/dev/null
}
hotspot() {  # join lamp-ID's hotspot; HOST and KH for it
  local id=$1 ip=""
  route -n get default 2>/dev/null | grep -q 'interface: en0' && die "the default route is Wi-Fi: put Ethernet above Wi-Fi first"
  for _ in 1 2 3; do networksetup -setairportnetwork en0 "lamp-$id" >/dev/null 2>&1 && break; sleep 3; done
  for _ in $(seq 1 30); do ip=$(ipconfig getifaddr en0); [[ $ip == 192.168.100.* ]] && break; sleep 1; done
  [[ $ip == 192.168.100.* ]] || return 1
  HOST=192.168.100.1; KH=$STATE/known_hosts_lamp-$id
  for _ in $(seq 1 20); do nc -z -G 2 "$HOST" 22 2>/dev/null && return 0; sleep 2; done
  return 1
}
bench() {  # a set-up lamp on the bench network: an IP, or lamp-ID.local; HOST and KH for it
  local t=$1
  if [[ $t =~ ^[0-9.]+$ ]]; then HOST=$t
  else HOST=$(dscacheutil -q host -a name "lamp-$t.local" 2>/dev/null | awk '/ip_address/{print $2; exit}'); [[ -n $HOST ]] || return 1; fi
  KH=$STATE/known_hosts_bench-$HOST
  nc -z -G 3 "$HOST" 22 2>/dev/null
}
name() { lamp hostname | tail -1; }
# The servos' own settings, in their EEPROM (servo-calib.py), before the run:
# V1's two settings stored (nothing moves); a lamp never calibrated is
# calibrated here with its person, who moves each joint by hand and says yes
# to the plan. Nothing is driven. ID's kit is on the lamp; 0 when ready.
servos_ready() {
  local id=$1 a
  lamp "python3 /home/orangepi/servo-calib.py check" >/dev/null && return 0
  lamp "systemctl is-active -q lamp-body factory-run && echo BUSY" | grep -q BUSY && { say "lamp-$id: lamp-body or a factory run is driving the servos"; return 1; }
  lamp "python3 /home/orangepi/servo-calib.py configure" >/dev/null || { say "lamp-$id: V1's servo settings were not kept"; return 1; }
  lamp "python3 /home/orangepi/servo-calib.py check" >/dev/null && return 0
  say "lamp-$id: its servos were never calibrated. Fold it at rest, head on its stop, and keep it still; when the sweep starts, move each joint end to end by hand (base turn, shoulder, elbow, head roll, head pitch) and fold it back within ${SWEEP_S:-120} s"
  read -r -p "factory-lamp: start lamp-$id's sweep? [y/N] " a; [[ $a == y ]] || { say "lamp-$id: not calibrated"; return 1; }
  ssh_once "python3 -u /home/orangepi/servo-calib.py sweep ${SWEEP_S:-120}" || { say "lamp-$id: the sweep gave no plan that fits: sweep again"; return 1; }
  read -r -p "factory-lamp: write this plan into lamp-$id's servos? [y/N] " a; [[ $a == y ]] || { say "lamp-$id: nothing written"; return 1; }
  lamp "python3 /home/orangepi/servo-calib.py apply" || { say "lamp-$id: the servos did not keep the plan"; return 1; }
  lamp "python3 /home/orangepi/servo-calib.py check" >/dev/null || { say "lamp-$id: still not calibrated after the plan"; return 1; }
  say "lamp-$id: calibrated"
}
unit() { echo "systemctl reset-failed factory-run >/dev/null 2>&1; systemctl is-active -q factory-run && { echo ALREADY RUNNING; exit 3; }; $*"; }

case $MODE in
start)
  for id in "$@"; do
    [[ $id =~ ^[0-9a-f]{4}$ ]] || { say "$id: not a lamp ID"; continue; }
    hotspot "$id" || { say "lamp-$id: its hotspot can't be joined (set up already, or off?)"; continue; }
    n=$(name); [[ $n == "lamp-$id" ]] || { say "lamp-$id: the lamp at $HOST says it is '$n': skipped"; continue; }
    kit || { say "lamp-$id: the kit could not be copied: skipped"; continue; }
    servos_ready "$id" || { say "lamp-$id: skipped until its servos are calibrated (factory-lamp.sh calibrate $id)"; continue; }
    lamp "$(unit "systemd-run --unit=factory-run --collect --setenv=GAZE=${GAZE:-755,555} --setenv=NUDGE_TARGET=${NUDGE_TARGET:-2010} /home/orangepi/factory-run.sh unset $id && echo STARTED")" | grep -q STARTED \
      && say "lamp-$id: started; it says each step itself (follow: factory-lamp.sh follow $id)" \
      || say "lamp-$id: not started (a run is already going, or systemd-run failed)"
  done ;;
follow)
  # The bench network first: following a set-up lamp never moves the Wi-Fi.
  id=$1; bench "$id" || hotspot "$id" || die "lamp-$id can't be reached (bench network or hotspot)"
  [[ $(name) == "lamp-$id" ]] || die "the lamp at $HOST is not lamp-$id"
  seen=0
  while :; do
    # FOLLOW: marks the line count and status (lamp() drops lines starting with @).
    out=$(lamp "n=\$(wc -l < /var/lib/lamp-test/factory-run.log); tail -n +$((seen + 1)) /var/lib/lamp-test/factory-run.log; echo \"FOLLOW: \$n \$(cat /var/lib/lamp-test/factory-run.status)\"")
    grep -v '^FOLLOW: ' <<<"$out" | grep -vE '^\[factory-voice\]' | cut -c1-200
    st=$(sed -n 's/^FOLLOW: [0-9]* //p' <<<"$out"); n=$(sed -n 's/^FOLLOW: \([0-9]*\) .*/\1/p' <<<"$out"); seen=${n:-$seen}
    [[ $st == PASS* || $st == FAIL* ]] && { say "lamp-$id: $st"; break; }
    sleep 3
  done ;;
selftest)
  for t in "$@"; do (
    bench "$t" || { say "$t: not reachable on the bench network"; exit; }
    n=$(name); id=${n#lamp-}
    [[ $id =~ ^[0-9a-f]{4}$ ]] || { say "$t: not a lamp ($n)"; exit; }
    lamp "test -e /var/lib/lampos-setup/done" >/dev/null || { say "$n: not set up from the app"; exit; }
    kit || { say "$n: the kit could not be copied"; exit; }
    lamp "$(unit "systemd-run --unit=factory-run --collect --setenv=RESET=${RESET:-no} /home/orangepi/factory-run.sh selftest $id && echo STARTED")" | grep -q STARTED \
      && say "$n ($HOST): self-test started (reset: ${RESET:-no})" || say "$n ($HOST): not started"
  ) & done; wait ;;
collect)
  rows=()
  for t in "$@"; do
    if bench "$t" 2>/dev/null; then :
    elif [[ $t =~ ^[0-9a-f]{4}$ ]] && hotspot "$t"; then :
    else say "$t: not reachable (bench network or hotspot)"; continue; fi
    n=$(name)
    lamp "cat /var/lib/lamp-test/factory.env 2>/dev/null; echo \"STATUS=\$(cat /var/lib/lamp-test/factory-run.status 2>/dev/null)\"; echo \"SET_UP=\$(test -e /var/lib/lampos-setup/done && echo yes || echo no)\"" \
      | grep -E '^[A-Z_0-9]+=' > "$STATE/records/$n.env"
    say "$n: $(grep -c . "$STATE/records/$n.env") lines in records/$n.env"
    rows+=("$n")
  done
  cols=(LAMP_ID BUILD SERVOS HEAD_PAD_LINES BUTTON_PRIMARY RESET_BUTTON PRIVACY_SWITCH HEAD_REST FACTORY_4C1 FACTORY_4C2 FACTORY_4C3 FACTORY_4D RESULT_UNSET RESULT_SELFTEST SELFTEST_FAILED SET_UP STATUS)
  csv=$STATE/factory-records.csv
  (IFS=,; echo "${cols[*]}") > "$csv"
  printf '| %s ' "${cols[@]}"; echo '|'; printf '|---%.0s' "${cols[@]}"; echo '|'
  for n in "${rows[@]-}"; do
    [[ -n $n ]] || continue
    vals=(); for c in "${cols[@]}"; do vals+=("$(sed -n "s/^$c=//p" "$STATE/records/$n.env" | tail -1)"); done
    (IFS=,; echo "${vals[*]//,/;}") >> "$csv"
    printf '| %s ' "${vals[@]}"; echo '|'
  done
  say "table: $csv" ;;
calibrate)
  # The same calibration start does, on its own: before a run, or again
  # after a servo is replaced or a horn remounted.
  id=$1; [[ $id =~ ^[0-9a-f]{4}$ ]] || die "$id: not a lamp ID"
  hotspot "$id" || die "lamp-$id: its hotspot can't be joined (set up already, or off?)"
  [[ $(name) == "lamp-$id" ]] || die "the lamp at $HOST is not lamp-$id"
  kit || die "lamp-$id: the kit could not be copied"
  servos_ready "$id" || die "lamp-$id: not calibrated"
  lamp "python3 /home/orangepi/servo-calib.py check" | tail -2
  say "lamp-$id: next: factory-lamp.sh start $id" ;;
esac
