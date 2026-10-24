#!/usr/bin/env bash
# heat-steps.sh — how much CPU load this unit's cooling holds: stress on 2, 4
# and 8 cores in turn, the CPU zone and both clusters' clocks read every second.
# A qualification aid for #1852 (one fan or two), not a pass/fail check: the
# pass/fail is burn-in.sh's thermal soak. Silent; nothing moves.
#
#   sudo ./heat-steps.sh                 # 2, 4, 8 cores, 120 s each
#   sudo D=60 STEPS="4 8" ./heat-steps.sh
#
# The same safety guard as burn-in.sh, on the CPU zone (cpub_thermal, else the
# first "cpu" zone): pause the load at 85 °C, resume at 80 °C, stop at 88 °C, so
# the CPU never reaches 90 °C. Between steps it waits for the zone to cool to
# 66 °C (at most a minute). Needs root and `stress`.
set -uo pipefail
D=${D:-120}; STEPS=${STEPS:-"2 4 8"}
PAUSE_C=${PAUSE_C:-85}; RESUME_C=${RESUME_C:-80}; ABORT_C=${ABORT_C:-88}
[ "$(id -u)" -eq 0 ] || { echo "heat-steps: run as root" >&2; exit 2; }
command -v stress >/dev/null || { echo "heat-steps: stress not installed" >&2; exit 2; }

zone=""
for z in /sys/class/thermal/thermal_zone*; do
  case "$(cat "$z/type" 2>/dev/null)" in *cpub_thermal*) zone=$z; break ;; esac
done
if [ -z "$zone" ]; then
  for z in /sys/class/thermal/thermal_zone*; do
    case "$(cat "$z/type" 2>/dev/null)" in *cpu*) zone=$z; break ;; esac
  done
fi
[ -n "$zone" ] || { echo "heat-steps: no CPU thermal zone" >&2; exit 2; }
cpu_c() { awk '{printf "%d", $1/1000}' "$zone/temp"; }
clk() {  # little/big cluster clocks in MHz (cpu0 and the highest-numbered cpu)
  local last; last=$(ls -d /sys/devices/system/cpu/cpu[0-9]* | sort -V | tail -1)
  echo "$(( $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)/1000 ))/$(( $(cat "$last"/cpufreq/scaling_cur_freq)/1000 ))"
}
trap 'pkill -CONT -x stress 2>/dev/null; pkill -x stress 2>/dev/null' EXIT INT TERM

echo "zone $(cat "$zone/type") · guard pause ${PAUSE_C} resume ${RESUME_C} stop ${ABORT_C} °C · ${D}s a step"
for N in $STEPS; do
  echo "== $N cores, from $(cpu_c) °C"
  stress --cpu "$N" --timeout "${D}s" >/dev/null 2>&1 &
  P=$!; paused=0; peak=0; ps=0; stopped=""
  for s in $(seq 1 "$D"); do
    t=$(cpu_c); [ "$t" -gt "$peak" ] && peak=$t
    if [ "$t" -ge "$ABORT_C" ]; then pkill -CONT -x stress; pkill -x stress; stopped=" STOPPED at ${s}s ${t} °C"; break; fi
    if [ "$paused" -eq 0 ] && [ "$t" -ge "$PAUSE_C" ]; then pkill -STOP -x stress; paused=1; fi
    if [ "$paused" -eq 1 ] && [ "$t" -le "$RESUME_C" ]; then pkill -CONT -x stress; paused=0; fi
    [ "$paused" -eq 1 ] && ps=$((ps + 1))
    [ $((s % 20)) -eq 0 ] && echo "   ${s}s ${t} °C clocks $(clk) MHz"
    sleep 1
  done
  pkill -CONT -x stress 2>/dev/null; wait "$P" 2>/dev/null
  echo "   $N cores: peak ${peak} °C, guard paused ${ps}s of ${D}s${stopped}"
  for _ in $(seq 1 30); do [ "$(cpu_c)" -le 66 ] && break; sleep 2; done
done
echo "== end $(cpu_c) °C"
