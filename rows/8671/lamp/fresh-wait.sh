#!/bin/sh
# Run as root (copied from rows/8933, from rows/8432). Waits (max 90 s) for the running lamp-voice's listening_ready; fails on the restart guard's mute.
t0=$(systemctl show -p ActiveEnterTimestamp --value lamp-voice); p=$(systemctl show -p MainPID --value lamp-voice)
i=0
until journalctl -u lamp-voice --since "$t0" -o cat | grep -q listening_ready; do
  if journalctl -u lamp-voice --since "$t0" -o cat | grep -q 'muted by the restart guard'; then
    echo "MUTED by the restart guard: one deliberate head tap clears it (needs the person's hands)"; exit 1; fi
  i=$((i+1)); [ $i -gt 90 ] && { echo NOT-READY; exit 1; }; sleep 1
done
echo "listening_ready: pid $p after ${i}s"
