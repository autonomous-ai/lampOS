#!/bin/sh
# Kit 5194: read-only questions the Mac's driver asks over ssh (root). Changes nothing on the lamp
# except files in /run/kit5194/logs.
#   sh /run/kit5194/q.sh now            "WALL MONO_US": the lamp's wall clock and CLOCK_MONOTONIC (µs)
#   sh /run/kit5194/q.sh vstate         voice/state's state ("speaking", "listening", ...)
#   sh /run/kit5194/q.sh mute           voice/mute ('"muted":false' ...)
#   sh /run/kit5194/q.sh asleep         expression/state's asleep ('"asleep":false' ...)
#   sh /run/kit5194/q.sh events PASS    the voice session's transcript events -> logs/events-PASS.jsonl
#   sh /run/kit5194/q.sh mark TEXT      appends "WALL MONO TEXT" to logs/marks.txt
#   sh /run/kit5194/q.sh pack           logs + state -> /tmp/kit5194-logs.tgz (GNU tar: no ._ files)
. /run/kit5194/common.sh
now() { python3 -c 'import time; print("%.3f %d" % (time.time(), time.monotonic_ns() // 1000))' 2>/dev/null \
  || echo "$(date +%s.%N) $(awk '{ printf "%d", $1 * 1000000 }' /proc/uptime)"; }
case "${1:-}" in
  now) now;;
  vstate) vstate_now | cut -d'"' -f4;;
  mute) mute_now;;
  asleep) asleep_now;;
  events)
    P=${2:-x}
    SD=$(ls -td /var/lib/lampOS/voice/sessions/*/ 2>/dev/null | head -1)
    [ -n "$SD" ] && cat "$SD"metadata/events-*.jsonl 2>/dev/null \
      | grep -E '"kind":"(transcript|turn_finished|speaker_first_write|input_admitted)"' > $L/events-$P.jsonl
    echo "events-$P: $(wc -l < $L/events-$P.jsonl) lines from $SD";;
  mark) shift; echo "$(now) $*" >> $L/marks.txt;;
  pack) tar -czf /tmp/kit5194-logs.tgz -C $R logs state && chmod 644 /tmp/kit5194-logs.tgz && echo packed;;
  *) echo "usage: q.sh now|vstate|mute|asleep|events PASS|mark TEXT|pack"; exit 1;;
esac
