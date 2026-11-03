#!/usr/bin/env bash
# factory-lamp.sh — the factory run for many lamps from the Mac at the bench
# (#3530). The lamp runs its own steps (factory-run.sh) and says each one on
# its speaker; the Mac only starts runs and collects results, so one person
# keeps several lamps going at once.
#
#   factory-lamp.sh start ID...         lamps not set up, one after another:
#                                       join lamp-ID's hotspot, copy the kit,
#                                       start its run on the lamp, leave (~20 s)
#   factory-lamp.sh follow ID           stay on lamp-ID's hotspot and print its
#                                       run as it goes, until PASS or FAIL
#   factory-lamp.sh selftest TARGET...  lamps set up from the app, on the bench
#                                       network, all at once: step 4 on each;
#                                       RESET=yes answers a passed one's reset
#   factory-lamp.sh collect TARGET...   every lamp's record and status in one
#                                       table ($STATE/factory-records.csv)
#
# TARGET is a bench-network IP or a lamp ID (found as lamp-ID.local; an unset
# lamp is read over its hotspot). The unset run, on the lamp:
#   checks (r2, 4b camera, 4e build, 4f, 4g) → the head onto its stop →
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
# STATE (default ~/.factory-lamp), RESET=yes (selftest), GAZE, NUDGE_TARGET.
#
# JUMP=IP: the Mac's Wi-Fi stays on its internet and a bench OrangePi wired to
# the Mac (at IP, same LAMP_PASS) joins each lamp's hotspot with its own
# wlan0 (wpa_cli, nothing saved); every ssh and scp to the lamp goes through
# it. `factory-lamp.sh jump-home` puts the jump's Wi-Fi back on its saved
# network.
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
usage() { sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[[ $MODE == jump-home || ( $MODE =~ ^(start|follow|selftest|collect)$ && $# -ge 1 ) ]] || usage
JUMP=${JUMP:-}; JKH=$STATE/known_hosts_jump
# Through the jump, sshpass answers its password and expect the lamp's;
# ProxyCommand=none is a direct connection.
PC=none PCJ=""
[[ -n $JUMP ]] && PCJ="sshpass -p $PASS ssh -o UserKnownHostsFile=$JKH -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -W %h:%p orangepi@$JUMP"
jump() {  # one root command on the jump
  sshpass -p "$PASS" ssh -o UserKnownHostsFile="$JKH" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 \
    "orangepi@$JUMP" "echo $(printf %q "$PASS") | sudo -S -p '' bash -c $(printf %q "$1")" 2>/dev/null
}
if [[ $MODE == start || $MODE == selftest ]]; then
  [[ -f $STEPS ]] || die "no steps.sh at $STEPS"
  [[ -z ${STEPS_SHA:-} || $(shasum -a 256 "$STEPS" | cut -c1-64) == "$STEPS_SHA" ]] || die "steps.sh is not $STEPS_SHA"
fi

# expect drives ssh's and sudo's password prompts (no key is left on a
# shipped lamp). HOST and KH name the lamp a call goes to.
carried() { echo "echo $(printf '%s\n' "$1" | base64 | tr -d '\n') | base64 -d > /tmp/factory-lamp.sh && sudo -S -p SUDOPW: bash /tmp/factory-lamp.sh"; }
ssh_once() {
  P="$PASS" C="$(carried "$1")" H="$HOST" KH="$KH" PC="$PC" expect <<'EOF' | tr -d '\r' | grep -vE "assword:|SUDOPW|^Warning: Permanently"
set timeout 300
log_user 1
spawn -noecho ssh -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no -o ConnectTimeout=10 -o ProxyCommand=$env(PC) orangepi@$env(H) $env(C)
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
  P="$PASS" F="$*" H="$HOST" KH="$KH" PC="$PC" expect <<'EOF'
set timeout 300
log_user 0
eval spawn -noecho scp -q -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no [list -o ProxyCommand=$env(PC)] $env(F) orangepi@$env(H):/tmp/
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
      "$HERE/input-check.py" "$HERE/ring-check.py" "$PROMPTS"/factory_*.wav >/dev/null || { rm -rf "$stage"; return 1; }
  rm -rf "$stage"
  lamp "mkdir -p /var/lib/lamp-test/prompts && mv /tmp/factory_*.wav /var/lib/lamp-test/prompts/ &&
        mv /tmp/steps.sh /tmp/factory-run.sh /tmp/factory-voice.py /tmp/head-nudge.py /tmp/input-check.py /tmp/ring-check.py /home/orangepi/ &&
        chmod 0755 /home/orangepi/steps.sh /home/orangepi/factory-run.sh &&
        sha256sum /home/orangepi/steps.sh | grep -q ^$want" >/dev/null
}
hotspot() {  # join lamp-ID's hotspot; HOST and KH for it
  local id=$1 ip=""
  if [[ -n ${BRIDGE-} ]]; then
    # The Mac already reaches the hotspot (an ESP32-S3 USB-Wi-Fi bridge on it).
    HOST=192.168.100.1; KH=$STATE/known_hosts_lamp-$id; PC=none
    for _ in $(seq 1 20); do nc -z -G 2 "$HOST" 22 2>/dev/null && return 0; sleep 2; done
    return 1
  fi
  if [[ -n $JUMP ]]; then
    # The jump's wlan0 onto the lamp's open hotspot, for this session only.
    jump "wpa_cli -i wlan0 remove_network all >/dev/null; n=\$(wpa_cli -i wlan0 add_network | tail -1)
      wpa_cli -i wlan0 set_network \$n ssid '\"lamp-$id\"' >/dev/null && wpa_cli -i wlan0 set_network \$n key_mgmt NONE >/dev/null &&
      wpa_cli -i wlan0 select_network \$n >/dev/null
      for _ in \$(seq 1 45); do ip -4 -br addr show wlan0 | grep -q ' 192\\.168\\.100\\.' && exit 0; sleep 1; done; exit 1" || return 1
    HOST=192.168.100.1; KH=$STATE/known_hosts_lamp-$id; PC=$PCJ
    for _ in $(seq 1 20); do jump "timeout 2 bash -c '</dev/tcp/192.168.100.1/22'" && return 0; sleep 2; done
    return 1
  fi
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
  KH=$STATE/known_hosts_bench-$HOST; PC=none
  nc -z -G 3 "$HOST" 22 2>/dev/null
}
name() { lamp hostname | tail -1; }
unit() { echo "systemctl reset-failed factory-run >/dev/null 2>&1; systemctl is-active -q factory-run && { echo ALREADY RUNNING; exit 3; }; $*"; }

case $MODE in
jump-home)
  [[ -n $JUMP ]] || die "set JUMP to the jump's IP"
  jump "wpa_cli -i wlan0 reconfigure" >/dev/null && say "the jump's Wi-Fi is back on its saved network" || die "wpa_cli reconfigure failed on $JUMP" ;;
start)
  for id in "$@"; do
    [[ $id =~ ^[0-9a-f]{4}$ ]] || { say "$id: not a lamp ID"; continue; }
    hotspot "$id" || { say "lamp-$id: its hotspot can't be joined (set up already, or off?)"; continue; }
    n=$(name); [[ $n == "lamp-$id" ]] || { say "lamp-$id: the lamp at $HOST says it is '$n': skipped"; continue; }
    kit || { say "lamp-$id: the kit could not be copied: skipped"; continue; }
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
  cols=(LAMP_ID BUILD HEAD_PAD_LINES BUTTON_PRIMARY RESET_BUTTON PRIVACY_SWITCH HEAD_REST FACTORY_4C1 FACTORY_4C2 FACTORY_4C3 FACTORY_4D RESULT_UNSET RESULT_SELFTEST SELFTEST_FAILED SET_UP STATUS)
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
esac
