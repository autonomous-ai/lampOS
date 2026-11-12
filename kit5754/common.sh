# Kit 5754: shared pieces, sourced by setup.sh, row3.sh, row5.sh and putback.sh.
# Plain POSIX sh (dash on the lamp). Never stops, restarts or masks lamp-update or
# lamp-update-back, never takes the updater's lock, never touches a `current` link.
B=/root/kit5754
ON=kit5754
DD=/run/systemd/system
SK=/var/lib/lampOS/skills
STATE=/var/lib/lampos-skills-state
OFF=$STATE/off
DROPIN=$DD/lamp-skills.service.d/zz-$ON.conf
CLOCK_SOCK=/run/lampos-skills/$ON-clock.sock
SKILLS_BIN=/opt/lampOS/skills/current/lamp-skills
BUS="$B/lamp-bus --root /run/lampos-bus"

lock_held() { i=$(stat -c %i /var/lib/lampos-update/lock 2>/dev/null) || return 1; grep -q ":$i " /proc/locks; }
upd_busy() {
  [ -e /var/lib/lampos-update/self.json ] || systemctl is-active -q lamp-update-back || lock_held \
    || case "$(systemctl show -p ActiveState --value lamp-update)" in activating|deactivating) true;; *) false;; esac
}
# Environment compared SAME/DIFF: systemd's per-start values masked.
envof() { tr '\0' '\n' < /proc/$1/environ | grep -v -E '^(INVOCATION_ID|JOURNAL_STREAM|SYSTEMD_EXEC_PID|MAINPID|MANAGERPID|WATCHDOG_PID|WATCHDOG_USEC)=' | sort | tr '\n' ' '; }
skills_pid() { systemctl show -p MainPID --value lamp-skills; }
now_ms() { date +%s%3N; }
hhmm() { date +%H%M; }
# Local minute of the day from HHMM, so 0930 -> 570.
mins() { h=$(echo "$1" | cut -c1-2); m=$(echo "$1" | cut -c3-4); echo $(( ${h#0} * 60 + ${m#0} )); }
now_min() { mins "$(hhmm)"; }
# Never in the skills' quiet hours (22:00-08:00), whatever the row.
not_quiet() { n=$(now_min); [ "$n" -ge 480 ] && [ "$n" -lt 1320 ]; }
# The newest humans/faces/tracked value as "faces near_faces max_height" (near: height >= 0.14,
# the skills' `near`). Faces serialise id, xmin, ymin, xmax, ymax in that order.
faces_now() {
  $BUS latest humans/faces/tracked 2>/dev/null | awk '
    index($0, "\"faces\"") {
      s = $0; n = 0; near = 0; m = 0
      while (match(s, /"ymin":-?[0-9.]+(e-?[0-9]+)?/)) {
        ymin = substr(s, RSTART + 7, RLENGTH - 7) + 0; s = substr(s, RSTART + RLENGTH)
        if (match(s, /"ymax":-?[0-9.]+(e-?[0-9]+)?/)) {
          ymax = substr(s, RSTART + 7, RLENGTH - 7) + 0; s = substr(s, RSTART + RLENGTH)
          h = ymax - ymin; n++; if (h >= 0.14) near++; if (h > m) m = h
        }
      }
      found = 1; printf "%d %d %.2f\n", n, near, m
    }
    END { if (!found) print "? ? ?" }'
}
mute_now() { $BUS latest voice/mute 2>&1 | grep -o '"muted":[a-z]*' | head -1; }
vstate_now() { $BUS latest voice/state 2>&1 | grep -o '"state":"[a-z_]*"' | head -1; }
# The voice's listening_ready said by its current run (the voice is never restarted by this kit).
voice_ready() {
  # The run's start on the wall clock, from its monotonic start (µs since boot) and the uptime.
  mono=$(systemctl show -p ExecMainStartTimestampMonotonic --value lamp-voice)
  up=$(cut -d. -f1 /proc/uptime)
  since=""
  case "$mono" in ''|0|*[!0-9]*) ;; *) since=$(( $(date +%s) - up + mono / 1000000 - 2 ));; esac
  if [ -n "$since" ]; then journalctl -u lamp-voice --since "@$since" --no-pager 2>/dev/null | grep -c listening_ready
  else journalctl -u lamp-voice -b --no-pager 2>/dev/null | grep -c listening_ready; fi
}
# The newest "loaded [...]" and "kept in ...: [...]" lines lamp-skills journaled since $1 (journalctl --since).
loaded_since() { journalctl -u lamp-skills --since "$1" --no-pager -o cat 2>/dev/null | grep 'lamp-skills: loaded \[' | tail -1; }
offkept_since() { journalctl -u lamp-skills --since "$1" --no-pager -o cat 2>/dev/null | grep -E 'lamp-skills: the skills a person turned off kept in|lamp-skills: the skills turned off read again' | tail -1; }
installed() { [ -f "$SK/$1/skill.toml" ]; }
version_of() { grep -m1 '^version' "$SK/$1/skill.toml" 2>/dev/null | cut -d'"' -f2; }
in_off() { [ -f "$OFF" ] && grep -qx "$1" "$OFF"; }
