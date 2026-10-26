#!/usr/bin/env bash
# factory-lamp.sh — one lamp's factory steps (#3530 4b–4g, 4c4) from the Mac at
# the bench, with each step spoken on the lamp (factory-voice.py). The same
# run for every lamp: the QC person answers at the laptop, the lamp says what
# to do.
#
#   factory-lamp.sh unset ID        lamp not set up, over its hotspot lamp-ID:
#                                   checks (r2, camera 4b, 4e–4g), steps 1–3
#                                   (4c1–4c3) spoken, then 4d (--gaze)
#   factory-lamp.sh selftest ID IP  lamp set up from the app, on the bench
#                                   Wi-Fi at IP: step 4 (4c4) spoken, then
#                                   the reset it asks for
#
# The Mac keeps its internet on Ethernet (above Wi-Fi in the service order,
# as on #3530) while its Wi-Fi joins lamp-ID. Every unset lamp is at
# 192.168.100.1, so each gets its own known_hosts and its hostname is read
# before anything runs. Nothing is written to a lamp before its hostname,
# image and steps.sh hash are checked.
#
# Needs, beside this script: steps.sh (lampOS/deploy/factory/steps.sh at the
# commit the lead names), factory-voice.py, head-nudge.py and the prompts made
# by make-prompts.sh (PROMPTS, default ./prompts). Environment: LAMP_PASS (the
# lamp's orangepi password), STEPS_SHA (the steps.sh sha256 to insist on),
# GAZE (default 755,555), NUDGE_TARGET (head pitch a nudge aims at, default
# 2010: a servo under a small push holds about 16 counts short, so it rests
# inside the body's 1760..2020 home range).
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
MODE=${1:-}; ID=${2:-}; IP=${3:-}
PASS=${LAMP_PASS:?set LAMP_PASS to the lamp password}
PROMPTS=${PROMPTS:-$HERE/prompts}
STEPS=${STEPS:-$HERE/steps.sh}
GAZE=${GAZE:-755,555}
STATE=${STATE:-$HOME/.factory-lamp}
mkdir -p "$STATE"
say() { echo "factory-lamp: $*"; }
die() { echo "factory-lamp: FAILED: $*" >&2; exit 1; }
[[ $MODE == unset || $MODE == selftest ]] && [[ $ID =~ ^[0-9a-f]{4}$ ]] || {
  sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[[ -f $STEPS ]] || die "no steps.sh at $STEPS"
[[ -z ${STEPS_SHA:-} || $(shasum -a 256 "$STEPS" | cut -c1-64) == "$STEPS_SHA" ]] || die "steps.sh is not $STEPS_SHA"
LAMP=lamp-$ID
HOST=${IP:-192.168.100.1}
KH=$STATE/known_hosts_$LAMP
SSHO=(-o UserKnownHostsFile="$KH" -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no -o ConnectTimeout=10)

# A command for the lamp, carried as base64 so no quoting reaches the shell.
carried() { echo "echo $(printf '%s\n' "$1" | base64 | tr -d '\n') | base64 -d > /tmp/factory-lamp.sh && sudo ${2:-} -p SUDOPW: bash /tmp/factory-lamp.sh"; }
# expect drives ssh's and sudo's password prompts (the lamp has no key for
# this Mac, and none is left on a shipped lamp). Arguments come in through
# the environment, so nothing is quoted twice.
# One command on the lamp as root; its output.
lamp() {
  P="$PASS" C="$(carried "$1" -S)" H="$HOST" KH="$KH" expect <<'EOF' | tr -d '\r' | grep -vE "assword:|SUDOPW|^Warning: Permanently"
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
# Copy files to the lamp's /tmp.
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
# An interactive root command at a real terminal (steps.sh asks the person).
live() {
  local x; x=$(mktemp)
  cat > "$x" <<'EOF'
set timeout 30
spawn -noecho ssh -t -o UserKnownHostsFile=$env(KH) -o StrictHostKeyChecking=accept-new -o PubkeyAuthentication=no orangepi@$env(H) $env(C)
expect -re "assword:" { send "$env(P)\r" }
expect "SUDOPW:" { send "$env(P)\r" }
set timeout -1
interact
catch wait r
exit [lindex $r 3]
EOF
  P="$PASS" C="$(carried "$1")" H="$HOST" KH="$KH" expect "$x"
  local rc=$?; rm -f "$x"; return $rc
}

if [[ $MODE == unset ]]; then
  say "joining $LAMP's hotspot (Ethernet keeps the internet)"
  route -n get default 2>/dev/null | grep -q 'interface: en0' && die "the default route is Wi-Fi: put Ethernet above Wi-Fi first"
  for _ in 1 2 3; do networksetup -setairportnetwork en0 "$LAMP" >/dev/null 2>&1 && break; sleep 3; done
  for _ in $(seq 1 30); do [[ $(ipconfig getifaddr en0) == 192.168.100.* ]] && break; sleep 1; done
  [[ $(ipconfig getifaddr en0) == 192.168.100.* ]] || die "could not join $LAMP"
  for _ in $(seq 1 20); do nc -z -G 2 "$HOST" 22 2>/dev/null && break; sleep 2; done
fi

name=$(lamp hostname | tail -1)
[[ $name == "$LAMP" ]] || die "the lamp at $HOST says it is '$name', not $LAMP"
say "$LAMP reached at $HOST"

say "copying steps.sh, factory-voice.py, head-nudge.py and the prompts"
put "$STEPS" "$HERE/factory-voice.py" "$HERE/head-nudge.py" "$PROMPTS"/factory_*.wav || die "copy failed"
lamp "mkdir -p /var/lib/lamp-test/prompts && mv /tmp/factory_*.wav /var/lib/lamp-test/prompts/ &&
      mv /tmp/steps.sh /tmp/factory-voice.py /tmp/head-nudge.py /home/orangepi/ && chmod 0755 /home/orangepi/steps.sh" >/dev/null

if [[ $MODE == selftest ]]; then
  lamp "test -e /var/lib/lampos-setup/done" >/dev/null || die "$LAMP is not set up: set it up from the app first"
  say "step 4 (4c4), spoken: answer at this terminal, touch at the lamp when it says"
  live "cd /home/orangepi && python3 factory-voice.py -- ./steps.sh --unit-id $ID --write --from 4"
  rc=$?
  [[ $rc == 0 ]] && say "4c4 PASS on $LAMP" || say "4c4 FAIL on $LAMP (exit $rc): run it again"
  exit $rc
fi

say "checks: image, camera (4b), 4e–4g"
lamp '
  echo "r2       $(grep -q ^country_code /etc/hostapd/hostapd.conf && echo FAIL old image || echo PASS)"
  v=$(cat /sys/bus/usb/devices/3-1.1.4/idVendor 2>/dev/null):$(cat /sys/bus/usb/devices/3-1.1.4/idProduct 2>/dev/null)
  echo "4b       $([ "$v" = 01da:5875 ] && echo PASS || echo "FAIL camera at 3-1.1.4 is [$v]")"
  echo "4e       $(jq -c ".installed | {sequence, commit}" /var/lib/lampos-update/state.json)"
  echo "4f       $(systemctl cat lamp-update | grep -q lampos-stable && echo PASS || echo FAIL)"
  r=""; for u in bootstrap os-server hal; do systemctl is-active -q $u && r="$r $u"; done
  echo "4g       $([ -z "$r" ] && echo PASS || echo "FAIL running:$r")"
  echo "unset    $(test -e /var/lib/lampos-setup/done && echo "FAIL it is set up" || echo PASS)"
  echo "lamp.env $(test -e /etc/lampOS/body/lamp.env && echo "already there (4c1 done before)" || echo none)"'

# steps.sh never measures over a lamp.env: a lamp whose step 1 is done goes on
# from step 2 (as steps.sh says to: "go on with --from 2").
FROM=1
if lamp "grep -q ^BODY_POSES= /etc/lampOS/body/lamp.env" >/dev/null; then
  FROM=2; say "lamp.env is already there (step 1 done before): going on from step 2"
else
say "the body at rest (read only)"
read_rest() { lamp 'cd /home/orangepi && setpriv --reuid lampos --regid lampos --init-groups -- /opt/lampOS/body/current/lamp-body-measure --device /dev/ttyACM0 --unit-id '"$ID"' --lock-directory /run/lampos-locks/motor 2>&1 | grep -v ^BODY_POSES | tail -3'; }
rest=$(read_rest); echo "$rest"
if grep -q 'a head off its stop' <<<"$rest"; then
  read -r -p "factory-lamp: the head is off its stop. Nudge it down slowly (torque capped at 50%, stops on load)? [y/N] " a
  if [[ $a == y || $a == Y ]]; then
    lamp "python3 /home/orangepi/head-nudge.py --target ${NUDGE_TARGET:-2010}"
    rest=$(read_rest); echo "$rest"
  fi
  grep -q 'a head off its stop' <<<"$rest" && die "the head still rests off its stop: press it down by hand and run again"
fi
fi

say "steps ${FROM}–3 (4c${FROM}–4c3), spoken: answer at this terminal, follow the lamp"
live "cd /home/orangepi && python3 factory-voice.py -- ./steps.sh --unit-id $ID --write --from $FROM --to 3"
rc=$?
[[ $rc == 0 ]] || die "steps.sh stopped (exit $rc): fix what it says and run again"
# The exit status alone is not proof: what steps 1 to 3 write must be there.
lamp "grep -q ^BODY_POSES= /etc/lampOS/body/lamp.env && test -s /etc/lampOS/controls/controls-pins.json && test -s /etc/lampOS/update/key.pub" >/dev/null \
  || die "steps.sh ended but lamp.env, the pins file or key.pub is missing: 4c is not done"

say "4d: --gaze $GAZE inside BODY_POSES"
lamp "F=/etc/lampOS/body/lamp.env; test -f \$F || exit 3; grep -q -- --gaze \$F || { cp -p \$F \$F.before-4d && sed -i 's/^\\(BODY_POSES=\".*\\)\"\$/\\1 --gaze $GAZE\"/' \$F; }; grep ^BODY_POSES \$F"
lamp "grep -q -- '--gaze $GAZE' /etc/lampOS/body/lamp.env" >/dev/null || die "4d: --gaze $GAZE is not in lamp.env"
say "done on $LAMP: 4c1–4c3 and 4d, read back. Next: set it up from the app, then: factory-lamp.sh selftest $ID <its IP>"
