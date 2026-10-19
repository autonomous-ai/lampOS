#!/usr/bin/env bash
# One lamp trial for a build row, run on the lamp itself (the first builder on
# #1722, 19 Oct 2026 21:51: "a row is built when it's merged and tried on a
# lamp"). The row's switches on, the behaviour caused, its own journal line or
# bus message looked for, and everything put back.
#
#   trials.sh list            # the rows: what each tries, and the rows not here
#   trials.sh ROW             # the trial: PASS (exit 0) or FAIL (exit 1)
#   trials.sh ROW --dry       # print the steps, change nothing
#
# Each trial:
#  1. Whatever the row changes is set aside first (a skill's folder, the
#     memory store), and its setup runs (a skill copied in).
#  2. Its switches go on in a drop-in, /run/systemd/system/<unit>.d/
#     zz-trial.conf, named to apply after the lamp's own and gone at a
#     reboot: environment settings, and arguments added to the unit's own
#     command line. Each unit is restarted, and its running process is
#     checked to carry each one. The build each runs is said: its program's
#     path and the start of its SHA-256, read from the process itself, to
#     set beside the build's SHA256SUMS.
#  3. The behaviour is caused: a bus message (lamp-bus pub), one kept up
#     every second, a command, or a line printed as "PLAY: ..." for whoever
#     runs the trial to play from the Mac's speaker.
#  4. The journal since step 2 is watched for the row's own line, or the bus
#     for the row's own message, for at most the row's wait. PASS when it
#     comes; FAIL with the unit's last lines or the topic's newest message.
#  5. Always, after a pass, a failure or ^C: the drop-ins removed, what was
#     set aside put back exactly, each unit restarted, or stopped again if it
#     wasn't running before, and the row's own after-steps run.
#
# The journal is read with grep for the proof, never dumped, except the
# unit's last 15 lines on a FAIL.
#
# Exit: 0 pass, 1 fail, 2 usage, 3 the lamp could not be set up or put back.
#
# Settings (the lamp needs none; the tests in trials-test.sh set them):
#   TRIALS_SKILLS_SRC  the build's lampOS/skills folder (../skills beside this)
#   TRIALS_UNIT_DIR    where drop-ins go (/run/systemd/system)
#   TRIALS_ASIDE_DIR   where set-asides are kept (/run: tmpfs, so a reboot
#                      never leaves a copy of what was set aside)
#   TRIALS_PROC        where a process's environ and cmdline are read (/proc)
#   TRIALS_SUDO        the command to run as root (sudo, or nothing as root)
#   TRIALS_WAIT        overrides every row's wait, in seconds
#   TRIALS_CALL        sends a call: COMMAND SOCKET LINE (socat, else nc -U)
#   LAMP_BUS_BIN       lamp-bus (on PATH, else /usr/local/bin/lamp-bus)
set -euo pipefail

usage() { sed -n '7,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

HERE=$(cd "$(dirname "$0")" && pwd)
SKILLS_SRC=${TRIALS_SKILLS_SRC:-$HERE/../skills}
UNIT_DIR=${TRIALS_UNIT_DIR:-/run/systemd/system}
ASIDE_DIR=${TRIALS_ASIDE_DIR:-/run}
PROC=${TRIALS_PROC:-/proc}
if [[ -n ${TRIALS_SUDO+set} ]]; then
  SUDO=$TRIALS_SUDO
elif [[ $(id -u) == 0 ]]; then
  SUDO=
else
  SUDO=sudo
fi
BUS=${LAMP_BUS_BIN:-$(command -v lamp-bus || echo /usr/local/bin/lamp-bus)}
DROPIN=zz-trial.conf

# ---------------------------------------------------------------------------
# The rows. Each sets:
#   ABOUT    one line: what is tried, and the pull request(s) it comes from
#   NOTE     optional: what it needs that it doesn't set up, printed first
#   SETASIDE optional: "copy PATH [UNIT]" a line: PATH (a file or a folder)
#            copied aside, with UNIT stopped for the copy, and put back
#            exactly at the end: what the trial wrote there is removed, and a
#            PATH that wasn't there is removed
#   SETUP    optional: shell lines run as root after the set-aside
#   PAUSE    optional: one unit a line, stopped for the trial if it is
#            running (a sensor whose own readings would cross the cause's)
#            and started again at the end; one that wasn't running is left
#   SWITCHES one a line: "unit|NAME=value" for the unit's environment, or
#            "unit|+ARG..." for arguments added to its command line (left
#            out when the unit already has the first of them)
#   CAUSE    one a line: "pub TOPIC latest|event JSON TYPE", "keep TOPIC
#            latest JSON TYPE" (published every second until the end),
#            "call NAME JSON" (a skill's function called on lamp-skills'
#            call.sock, as the voice calls it), "run COMMAND", or "play LINE"
#   AFTER    optional: lines as CAUSE's, run at the end before the units are
#            restarted (a light given back, a presence ended)
#   FIRST    optional: for a behaviour that is done only when its cause
#            ends, an extended regex in IN's journal that shows it began, and
#   THEN     the lines, as CAUSE's, run once FIRST is seen (the keepers are
#            stopped first); PROOF is then looked for after them. "wait N"
#            lets N seconds pass, so the behaviour can be seen
#   FIRSTEX  FIRST's line as the code writes it, as EXAMPLE is PROOF's
#   PROOF    an extended regex: the line or message that shows the behaviour
#   EXAMPLE  that line as the code writes it (trials-test.sh checks PROOF
#            matches it); the comment above the row names where it is written
#   IN       the unit(s) whose journal holds PROOF, space-separated; or
#            "bus:TOPIC": PROOF is matched against the topic's newest
#            message once `lamp-bus hz` has seen messages come, so a value
#            kept from before the trial never passes alone
#   WAIT     seconds to wait for PROOF after the cause
# In SETASIDE, SETUP and AFTER, @SKILLS_DIR@, @SKILLS_USER@ and @SKILLS_GROUP@
# are the running lamp-skills' --dir, user and group, @MEMORY_DIR@ is
# lamp-memory's --dir, and @SRC@ is TRIALS_SKILLS_SRC.
# ---------------------------------------------------------------------------
ROWS="light-out scenes who-was-here activity forget reports nudges smoky dusty"
# Rows not here, and why: printed by `list`.
NOT_HERE="bedtime    Bedtime and Morning (#1732): a draft, not merged; three switches, --zone and its daily state, and it moves the body
nudges     the other three nudges (#1957): sip-of-water, stand-up and wind-down are tried by hand, as rest-your-eyes is here
mood       Mood on: lampOS/tools/mood-minute.sh --clap (#1956), its own minute with a clap played from the Mac
updates    the update stack (#1680: #1788 to #1945, open): updates.md's rehearsal, a file channel and a test key, not a switch"

define() {
  ABOUT= NOTE= SETASIDE= SETUP= PAUSE= SWITCHES= CAUSE= AFTER= FIRST= THEN= FIRSTEX= PROOF= EXAMPLE= IN= WAIT=
  case $1 in
    light-out)
      # #1721 and #1747 (on lamp-voice and #1712). The skill's trigger is
      # room/presence's nobody_s above 599, kept up here; its [quiet] hours
      # (23:00 to 06:00) are taken out of the trial's copy, so it can be
      # tried at night. Its lines: lamp-skills main.rs, "lamp-skills: {} {:?}:
      # {}" (the skill, Triggered or Done, why). It holds while nobody is
      # there and says Done only when its trigger ends (the first builder,
      # #1722, 20 Oct 00:28): Triggered, the fade watched, someone back,
      # then Done with the light asked of the ring.
      ABOUT="light out when nobody's there: light-out puts the ring's light out (#1721, #1747)"
      NOTE="a build with #1721 and #1747, the voice resting, and the eyes' presence off or the room empty: the trial says nobody_s 600 itself. The light fades to the ring's ember over 30 s, and comes back when the trial ends"
      SETASIDE="copy @SKILLS_DIR@/light-out"
      SETUP="rm -rf @SKILLS_DIR@/light-out && cp -a @SRC@/light-out @SKILLS_DIR@/light-out
sed '/^\[quiet\]/,/^to = /d' @SRC@/light-out/skill.toml >@SKILLS_DIR@/light-out/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/light-out"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on
lamp-skills.service|+--zone system"
      CAUSE='keep room/presence latest {"someone":false,"nobody_s":600} lamp_msgs/RoomPresence'
      FIRST='light-out Triggered'
      FIRSTEX='lamp-skills: light-out Triggered: room/presence has held for 0 s'
      THEN='wait 35
pub room/presence latest {"someone":true,"nobody_s":0} lamp_msgs/RoomPresence'
      AFTER='pub room/presence latest {"someone":true,"nobody_s":0} lamp_msgs/RoomPresence'
      PROOF='light-out Done: light out: asked of the ring'
      EXAMPLE='lamp-skills: light-out Done: light out: asked of the ring, faded into over 30000 ms; held until its trigger ended; light given back: asked again with a time already passed'
      IN="lamp-skills.service"
      WAIT=90 ;;
    scenes)
      # #1730 (on lamp-voice and #1712): light_scene called as the voice
      # calls it, "movie" (reading and focus bring the head down). Its line:
      # lamp-skills main.rs, the skill's Done.
      ABOUT="light scenes: light_scene's movie scene, called as the voice calls it (#1730)"
      NOTE="a build with #1730, and socat or nc -U on the lamp for the call (else play \"Hey Lamp, set the light for a movie.\"). The lamp says a line that the light is set, and the movie light is kept afterwards: ask for the light you want after. The light show (light_show) isn't tried here"
      SETASIDE="copy @SKILLS_DIR@/light-scenes"
      SETUP="rm -rf @SKILLS_DIR@/light-scenes && cp -a @SRC@/light-scenes @SKILLS_DIR@/light-scenes
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/light-scenes"
      # No switch of its own: the marker restarts lamp-skills, which loads
      # skills as it starts, and shows the trial's process is the one running.
      SWITCHES="lamp-skills.service|LAMP_TRIAL=scenes"
      CAUSE='call light_scene {"scene":"movie"}'
      PROOF='light-scenes Done'
      EXAMPLE="lamp-skills: light-scenes Done: light warm, the person's, kept"
      IN="lamp-skills.service"
      WAIT=30 ;;
    who-was-here)
      # #1989 (on lamp-voice): an installable skill whose one function,
      # who_was_here, answers from the last 12 h of room/presence. Called as
      # the voice calls it; its line: lamp-skills main.rs, the skill's Done.
      ABOUT="who was here: who_was_here answers from the room's record (#1989)"
      NOTE="a build with #1989, and socat or nc -U on the lamp for the call (else play \"Hey Lamp, was anyone here while I was out?\"). The lamp says a line. With the eyes' presence off (LAMP_PRESENCE) there is no record, and it says it can't tell: the call and its Done are what's tried here, not the record"
      SETASIDE="copy @SKILLS_DIR@/who-was-here"
      SETUP="rm -rf @SKILLS_DIR@/who-was-here && cp -a @SRC@/who-was-here @SKILLS_DIR@/who-was-here
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/who-was-here"
      # No switch of its own: the marker restarts lamp-skills to load it.
      SWITCHES="lamp-skills.service|LAMP_TRIAL=who-was-here"
      CAUSE='call who_was_here {}'
      PROOF='who-was-here Done'
      # The why is its steps' own reports (lamp-skills main.rs): left out.
      EXAMPLE='lamp-skills: who-was-here Done: …'
      IN="lamp-skills.service"
      WAIT=30 ;;
    activity)
      # #1760, #1811 and #1889 (on lamp-voice): the eyes say what the person
      # at the desk is doing on room/activity, once a second while they look
      # ("away" in an empty room), eyes activity.rs.
      ABOUT="activity and posture: the eyes say room/activity (#1760, #1811, #1889)"
      NOTE="a build with #1760, #1811 and #1889, the eyes running, and the microphone permitted (the eyes look only then). Sitting and posture need a person at the desk for 2 minutes; this shows the switch and the topic"
      SWITCHES="lamp-eyes.service|LAMP_BUS=on
lamp-eyes.service|LAMP_ACTIVITY=on"
      PROOF='"state": ?"[a-z_]+"'
      EXAMPLE='{"state":"away","for_s":12,"sitting_s":0,"baseline":false}'
      IN="bus:room/activity"
      WAIT=60 ;;
    forget)
      # #1723 (lamp-voice) and #1930 (#1712's second route). Forgetting has
      # no line of its own: the chat's call names it, second_provider.rs,
      # "request {} chat round {} called {}".
      ABOUT="forget on request: 'Forget that.' calls forget (#1723, #1930)"
      NOTE="the second route, LAMP_VOICE_ROUTE=second, as lamp-4ace runs: the proof is its chat's line, which Live never writes; and LAMP_MEMORY_DAYS=on in the voice (lamp-4ace has it). Play the four lines in order, each when the answer before has ended. 'Forget everything' needs a face the eyes know, so it isn't tried here. The memory store is set aside and put back"
      SETASIDE="copy @MEMORY_DIR@ lamp-memory.service"
      SWITCHES="lamp-voice.service|LAMP_MEMORY_FORGET_ALL=on"
      CAUSE="play Hey Lamp, remember my violet umbrella is in the car.
play What do you remember about my umbrella?
play Forget that.
play What about my umbrella?"
      PROOF='chat round [0-9]+ called .*forget'
      EXAMPLE='lamp-voice second: request 3 chat round 1 called forget'
      IN="lamp-voice.service"
      WAIT=240 ;;
    reports)
      # #1755 and #1906 (on lamp-voice): reports/state, every 5 s, says
      # share true while the switch is on and the person's yes stands
      # (msgs reports.rs, ReportsState).
      ABOUT="health reports: lamp-cloud says reports are shared (#1755, #1906)"
      NOTE="lamp-cloud.service installed (deploy/lamp-4ace/cloud-on.sh) and the person's yes in /etc/lampOS/setup/reports.json. A report sent is seen only by a broker on the Mac; the first goes 5 minutes after the start"
      SWITCHES="lamp-cloud.service|LAMP_CLOUD=on
lamp-cloud.service|LAMP_REPORTS=on"
      PROOF='"share": ?true'
      EXAMPLE='{"share":true,"queued":0,"by":null,"held":false,"at_s":null}'
      IN="bus:reports/state"
      WAIT=30 ;;
    nudges)
      # #1957 (on lamp-voice): rest-your-eyes, the shortest of the four,
      # whose trigger is a near face (height at least 0.14 of the picture)
      # there 45 minutes. The trial's copy waits 30 s and has no [active]
      # and [quiet] hours, so it can be tried now; its line: lamp-skills
      # main.rs, the skill's Done.
      ABOUT="the wellbeing nudges: rest-your-eyes tilts the head after a sitting (#1957)"
      NOTE="a build with #1957, the voice resting, and nobody at the lamp: the trial says a face near the lamp itself. The head tilts once (3 s) and the light dims a shade; keep the arm clear"
      SETASIDE="copy @SKILLS_DIR@/rest-your-eyes"
      SETUP="rm -rf @SKILLS_DIR@/rest-your-eyes && cp -a @SRC@/rest-your-eyes @SKILLS_DIR@/rest-your-eyes
sed -e 's/^for = \"45m\"/for = \"30s\"/' -e '/^\[active\]/,/^to = /d' -e '/^\[quiet\]/,/^to = /d' @SRC@/rest-your-eyes/skill.toml >@SKILLS_DIR@/rest-your-eyes/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/rest-your-eyes"
      SWITCHES="lamp-skills.service|+--zone system"
      CAUSE='keep humans/faces/tracked latest {"faces":[{"id":"trial","xmin":0.35,"ymin":0.2,"xmax":0.65,"ymax":0.6,"confidence":0.9}]} lamp_msgs/Faces'
      AFTER='pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces'
      PROOF='rest-your-eyes Done'
      # The why is its steps' own reports joined by "; " (lamp-skills
      # main.rs): left out here rather than guessed.
      EXAMPLE='lamp-skills: rest-your-eyes Done: …'
      IN="lamp-skills.service"
      WAIT=90 ;;
    smoky)
      # #1836 (on lamp-voice, held out of the product): smoky-air's trigger
      # is pm2_5 at 150 or more and 100 above the lowest of the 3 minutes
      # before, held 30 s. lamp-air is paused so the office's own readings
      # (about 100) don't end the hold; the trial waits for lamp-skills to
      # listen, says one ordinary reading, then smoke every second. The
      # trial's copy has a 1 m cooldown in place of 30 m, so it can be tried
      # again. Its line: lamp-skills main.rs, the skill's Done.
      ABOUT="smoky air: smoky-air says its line with a lift and an amber breath (#1836)"
      NOTE="a build with #1836, the voice resting, and lamp-air paused by the trial (started again after). The lamp lifts its head a little, breathes amber and says \"The air reads like smoke. If something's burning, please check it.\" once"
      SETASIDE="copy @SKILLS_DIR@/smoky-air"
      SETUP="rm -rf @SKILLS_DIR@/smoky-air && cp -a @SRC@/smoky-air @SKILLS_DIR@/smoky-air
sed -e 's/^cooldown = \"30m\"/cooldown = \"1m\"/' @SRC@/smoky-air/skill.toml >@SKILLS_DIR@/smoky-air/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/smoky-air"
      PAUSE="lamp-air.service"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on"
      CAUSE='wait 5
pub air/state latest {"pm1_0":30.0,"pm2_5":40.0,"pm4_0":42.0,"pm10":45.0,"humidity":45.0,"temperature":30.0,"co2_ppm":700,"feel":"fresh"} lamp_msgs/AirState
keep air/state latest {"pm1_0":280.0,"pm2_5":300.0,"pm4_0":310.0,"pm10":320.0,"humidity":45.0,"temperature":30.0,"co2_ppm":700,"feel":"fresh"} lamp_msgs/AirState'
      AFTER='pub air/state latest {"pm1_0":30.0,"pm2_5":40.0,"pm4_0":42.0,"pm10":45.0,"humidity":45.0,"temperature":30.0,"co2_ppm":700,"feel":"fresh"} lamp_msgs/AirState'
      PROOF='smoky-air Done'
      # The why is its steps' own reports joined by "; " (lamp-skills
      # main.rs): left out here rather than guessed.
      EXAMPLE='lamp-skills: smoky-air Done: …'
      IN="lamp-skills.service"
      WAIT=150 ;;
    dusty)
      # #1836 (on lamp-voice, held out of the product): dusty-air's trigger
      # is pm2_5 at 125 or more for an hour, while the voice rests and a
      # face is in view. The trial's copy holds 30 s in place of an hour,
      # has no [quiet] hours and a 1 m cooldown in place of a day, so it can
      # be tried now and again; lamp-air is paused, and the trial says the
      # dust and a face near the lamp itself. Its line: the skill's Done.
      ABOUT="dusty air: dusty-air says its line after the air stays very dusty (#1836)"
      NOTE="a build with #1836, the voice resting, nobody at the lamp (the trial says a face itself), and lamp-air paused by the trial (started again after). The lamp says \"The air here has been very dusty for about an hour. I can't tell where it's coming from.\" once"
      SETASIDE="copy @SKILLS_DIR@/dusty-air"
      SETUP="rm -rf @SKILLS_DIR@/dusty-air && cp -a @SRC@/dusty-air @SKILLS_DIR@/dusty-air
sed -e 's/^for = \"60m\"/for = \"30s\"/' -e 's/^cooldown = \"24h\"/cooldown = \"1m\"/' -e '/^\[quiet\]/,/^to = /d' @SRC@/dusty-air/skill.toml >@SKILLS_DIR@/dusty-air/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/dusty-air"
      PAUSE="lamp-air.service"
      # No switch of its own: the marker restarts lamp-skills, which loads
      # skills as it starts, and shows the trial's process is the one running.
      SWITCHES="lamp-skills.service|LAMP_TRIAL=dusty"
      CAUSE='keep humans/faces/tracked latest {"faces":[{"id":"trial","xmin":0.35,"ymin":0.2,"xmax":0.65,"ymax":0.6,"confidence":0.9}]} lamp_msgs/Faces
keep air/state latest {"pm1_0":120.0,"pm2_5":140.0,"pm4_0":150.0,"pm10":200.0,"humidity":45.0,"temperature":30.0,"co2_ppm":700,"feel":"fresh"} lamp_msgs/AirState'
      AFTER='pub air/state latest {"pm1_0":30.0,"pm2_5":40.0,"pm4_0":42.0,"pm10":45.0,"humidity":45.0,"temperature":30.0,"co2_ppm":700,"feel":"fresh"} lamp_msgs/AirState
pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces'
      PROOF='dusty-air Done'
      EXAMPLE='lamp-skills: dusty-air Done: …'
      IN="lamp-skills.service"
      WAIT=120 ;;
    *) return 1 ;;
  esac
}

# ---------------------------------------------------------------------------

say() { printf '%s %s\n' "$(date '+%H:%M:%S')" "$*"; }

lines() { printf '%s\n' "$1" | sed '/^[[:space:]]*$/d'; }

units_of_switches() { lines "$SWITCHES" | cut -d'|' -f1 | sort -u; }

# A unit's command line as systemd runs it: ExecStart's argv.
exec_argv() { systemctl show "$1" -p ExecStart --value 2>/dev/null | sed -n 's/.*argv\[\]=\([^;]*\) ;.*/\1/p' | head -1; }

# The value after FLAG in a unit's command line.
arg_of() { # UNIT FLAG
  exec_argv "$1" | tr ' ' '\n' | grep -A1 -xF -- "$2" | sed -n 2p
}

placeholders() { # TEXT: with the @...@ names filled from the running units
  local text=$1 value
  if [[ $text == *@SKILLS_* ]]; then
    value=$(arg_of lamp-skills.service --dir)
    [[ -n $value ]] || { say "lamp-skills.service has no --dir" >&2; exit 3; }
    text=${text//@SKILLS_DIR@/$value}
    value=$(systemctl show lamp-skills.service -p User --value 2>/dev/null)
    text=${text//@SKILLS_USER@/${value:-root}}
    value=$(systemctl show lamp-skills.service -p Group --value 2>/dev/null)
    text=${text//@SKILLS_GROUP@/${value:-root}}
  fi
  if [[ $text == *@MEMORY_DIR@* ]]; then
    value=$(arg_of lamp-memory.service --dir)
    [[ -n $value ]] || { say "lamp-memory.service has no --dir" >&2; exit 3; }
    text=${text//@MEMORY_DIR@/$value}
  fi
  printf '%s\n' "${text//@SRC@/$SKILLS_SRC}"
}

# Where the journal is now, so the watch reads only what comes after it: its
# last entry's cursor, or, with none, the time a second ago.
mark_now() {
  local cursor
  cursor=$($SUDO journalctl -n 1 --show-cursor --no-pager -o cat 2>/dev/null | sed -n 's/^-- cursor: //p' | tail -1)
  if [[ -n $cursor ]]; then
    echo "cursor $cursor"
  else
    echo "since @$(($(date +%s) - 1))"
  fi
}

since_mark() { # MARK UNIT...
  local mark=$1 args=() unit
  shift
  for unit in "$@"; do args+=(-u "$unit"); done
  case $mark in
    cursor\ *) args+=("--after-cursor=${mark#cursor }") ;;
    since\ *) args+=(--since "${mark#since }") ;;
  esac
  $SUDO journalctl ${args[@]+"${args[@]}"} -o cat --no-pager -q 2>/dev/null || true
}

main_pid() { systemctl show "$1" -p MainPID --value 2>/dev/null || echo 0; }

# The build the unit's running process is: its program's path and the first
# 12 hex digits of its SHA-256, read through /proc/PID/exe, so a binary
# replaced since the start is still the one measured.
build_of() { # UNIT
  local pid path sum
  pid=$(main_pid "$1")
  [[ -n $pid && $pid != 0 ]] || { echo "no running process"; return; }
  path=$($SUDO readlink "$PROC/$pid/exe" 2>/dev/null || echo "?")
  if command -v sha256sum >/dev/null; then
    sum=$($SUDO sha256sum "$PROC/$pid/exe" 2>/dev/null | cut -c1-12)
  else
    sum=$($SUDO shasum -a 256 "$PROC/$pid/exe" 2>/dev/null | cut -c1-12)
  fi
  echo "$path (sha256 ${sum:-?})"
}

# Whether the unit's running process carries a switch: NAME=value in its
# environment, or +ARG... in its command line.
carries() { # UNIT SWITCH
  local pid
  pid=$(main_pid "$1")
  [[ -n $pid && $pid != 0 ]] || return 1
  case $2 in
    +*) $SUDO cat "$PROC/$pid/cmdline" 2>/dev/null | tr '\0' ' ' | grep -qF -- " ${2#+}" ;;
    *) $SUDO cat "$PROC/$pid/environ" 2>/dev/null | tr '\0' '\n' | grep -qxF -- "$2" ;;
  esac
}

restart() { # UNIT
  $SUDO systemctl restart "$1" || return 1
  local i
  for i in 1 2 3 4 5 6 7 8 9 10; do
    [[ $(systemctl is-active "$1" 2>/dev/null) == active ]] && return 0
    sleep 1
  done
  return 1
}

# The set-aside folder is root's and 0700 (it can hold a copy of the
# person's memories), so it is only looked into and written as root.
aside_has() { $SUDO test -e "$ASIDE/$1" || $SUDO test -L "$ASIDE/$1"; }
aside_mark() { $SUDO touch "$ASIDE/$1"; }

KEEPERS=""
PUT_BACK_DONE=0
put_back() {
  [[ $PUT_BACK_DONE == 1 ]] && return 0
  PUT_BACK_DONE=1
  local failed=0 unit path line pid
  for pid in $KEEPERS; do kill "$pid" 2>/dev/null || true; done
  while IFS= read -r line; do
    act "after" "$line" || failed=1
  done < <(lines "$AFTER")
  for unit in $(units_of_switches); do
    if [[ -e "$UNIT_DIR/$unit.d/$DROPIN" ]]; then
      $SUDO rm -f "$UNIT_DIR/$unit.d/$DROPIN" || failed=1
    fi
  done
  $SUDO systemctl daemon-reload || failed=1
  local n=0 stop
  while IFS= read -r line; do
    n=$((n + 1))
    set -- $(placeholders "$line")
    path=$2 stop=${3:-}
    aside_has "$n.taken" || continue
    [[ -n $stop ]] && { $SUDO systemctl stop "$stop" || failed=1; }
    $SUDO rm -rf "$path" || failed=1
    if aside_has "$n/x"; then
      $SUDO cp -a "$ASIDE/$n/x" "$path" || failed=1
    fi
    [[ -n $stop ]] && { $SUDO systemctl start "$stop" || failed=1; }
    say "put back: $path"
  done < <(lines "$SETASIDE")
  for unit in $(units_of_switches); do
    if aside_has "was-stopped.$unit"; then
      $SUDO systemctl stop "$unit" || failed=1
      say "put back: $unit stopped again, as it was"
    else
      restart "$unit" || { say "put back: $unit did not come back active"; failed=1; }
    fi
  done
  while IFS= read -r unit; do
    if aside_has "paused.$unit"; then
      $SUDO systemctl start "$unit" && say "put back: $unit started again" ||
        { say "put back: $unit did not start again"; failed=1; }
    fi
  done < <(lines "$PAUSE")
  if [[ $failed == 0 ]]; then
    say "put back: drop-ins removed, $(units_of_switches | tr '\n' ' ')as they were"
  else
    say "PUT BACK FAILED: look at the units above by hand"
    exit 3
  fi
}

# One CAUSE or AFTER line. WHEN is "cause" or "after".
act() { # WHEN LINE
  local when=$1 kind=${2%% *} rest=${2#* }
  case $kind in
    pub | keep)
      # TOPIC latest|event JSON TYPE: the JSON is everything between the
      # kind and the last word.
      local topic kind_of json type
      topic=${rest%% *}; rest=${rest#* }
      kind_of=${rest%% *}; rest=${rest#* }
      type=${rest##* }; json=${rest% *}
      say "$when: lamp-bus $kind $topic $kind_of $json --type $type"
      $SUDO "$BUS" pub "$topic" "$kind_of" "$json" --type "$type" >/dev/null ||
        { say "FAIL: the $when: lamp-bus pub $topic failed"; return 1; }
      if [[ $kind == keep ]]; then
        (
          while sleep 1; do
            $SUDO "$BUS" pub "$topic" "$kind_of" "$json" --type "$type" >/dev/null 2>&1 || true
          done
        ) &
        KEEPERS="$KEEPERS $!"
      fi
      ;;
    call)
      # call NAME JSON: one line in, one line out, as the voice calls.
      local name=${rest%% *} args=${rest#* } tools line
      tools=$(arg_of lamp-skills.service --tools-dir)
      [[ -n $tools ]] || { say "FAIL: lamp-skills.service has no --tools-dir"; return 1; }
      line="{\"op\":\"call\",\"name\":\"$name\",\"args\":$args}"
      say "$when: $name $args on $tools/call.sock"
      local answered
      if [[ -n ${TRIALS_CALL:-} ]]; then
        answered=$($TRIALS_CALL "$tools/call.sock" "$line") || { say "FAIL: the call failed"; return 1; }
      elif command -v socat >/dev/null; then
        answered=$(printf '%s\n' "$line" | $SUDO socat -t 30 - "UNIX-CONNECT:$tools/call.sock") ||
          { say "FAIL: the call failed"; return 1; }
      elif nc -h 2>&1 | grep -q -- ' -U'; then
        answered=$(printf '%s\n' "$line" | $SUDO nc -U -w 30 "$tools/call.sock") ||
          { say "FAIL: the call failed"; return 1; }
      else
        say "FAIL: no socat or nc -U here to call $name: play the row's line instead (its needs: line)"
        return 1
      fi
      printf '  answered: %s\n' "$(printf '%s' "$answered" | head -1 | cut -c1-200)"
      ;;
    run)
      rest=$(placeholders "$rest")
      say "$when: $rest"
      $SUDO bash -c "$rest" || { say "FAIL: the $when failed: $rest"; return 1; }
      ;;
    play)
      say "PLAY: \"$rest\""
      ;;
    wait)
      say "$when: $rest s to see it"
      sleep "${TRIALS_THEN_WAIT:-$rest}"
      ;;
    *) say "unknown $when: $2"; return 1 ;;
  esac
}

# The drop-in's lines for one unit's switches.
dropin() { # UNIT
  local unit=$1 switch argv added=
  echo "[Service]"
  while IFS= read -r switch; do
    case $switch in
      +*)
        argv=$(exec_argv "$unit")
        if printf ' %s ' "$argv" | grep -qF -- " $(printf '%s' "${switch#+}" | cut -d' ' -f1) "; then
          continue
        fi
        added="$added ${switch#+}"
        ;;
      *) echo "Environment=$switch" ;;
    esac
  done < <(lines "$SWITCHES" | grep "^$unit|" | cut -d'|' -f2-)
  if [[ -n $added ]]; then
    argv=$(exec_argv "$unit")
    [[ -n $argv ]] || { say "no ExecStart to add$added to in $unit" >&2; return 1; }
    echo "ExecStart="
    # systemd reads % as a specifier: a literal one is %%.
    echo "ExecStart=${argv//%/%%}$added"
  fi
}

trial() { # ROW [--dry]
  local row=$1 dry=${2:-} line
  define "$row" || { echo "no row '$row'; trials.sh list shows them" >&2; exit 2; }
  WAIT=${TRIALS_WAIT:-$WAIT}
  say "row $row: $ABOUT"
  if [[ $dry == --dry ]]; then
    [[ -z $NOTE ]] || printf '  needs: %s\n' "$NOTE"
    lines "$SETASIDE" | sed 's/^/  set aside: /'
    lines "$SETUP" | sed 's/^/  setup: /'
    lines "$PAUSE" | sed 's/^/  pause: /'
    lines "$SWITCHES" | sed 's/^/  switch: /'
    lines "$CAUSE" | sed 's/^/  cause: /'
    lines "$AFTER" | sed 's/^/  after: /'
    printf '  proof in %s, within %s s: /%s/\n' "$IN" "$WAIT" "$PROOF"
    [[ -z $FIRST ]] || printf '  first: /%s/, then:\n' "$FIRST"
    [[ -z $FIRST ]] || printf '  first example: %s\n' "$FIRSTEX"
    lines "$THEN" | sed 's/^/  then: /'
    printf '  example: %s\n' "$EXAMPLE"
    exit 0
  fi

  ASIDE=$($SUDO mktemp -d "$ASIDE_DIR/lampos-trial-$row.XXXXXX") || { say "no set-aside folder in $ASIDE_DIR"; exit 3; }
  $SUDO chmod 0700 "$ASIDE"
  trap 'put_back; $SUDO rm -rf "$ASIDE"' EXIT
  trap 'exit 130' INT TERM
  [[ -z $NOTE ]] || say "needs: $NOTE"
  local unit
  for unit in $(units_of_switches); do
    [[ $(systemctl is-active "$unit" 2>/dev/null) == active ]] || aside_mark "was-stopped.$unit"
  done

  local path n=0 stop
  while IFS= read -r line; do
    n=$((n + 1))
    set -- $(placeholders "$line")
    path=$2 stop=${3:-}
    $SUDO mkdir -p "$ASIDE/$n"
    [[ -n $stop ]] && { $SUDO systemctl stop "$stop" || { say "could not stop $stop"; exit 3; }; }
    if [[ -e $path || -L $path ]]; then
      $SUDO cp -a "$path" "$ASIDE/$n/x" || { say "could not set aside $path"; exit 3; }
      say "set aside: $path"
    else
      say "set aside: $path (not there: removed after)"
    fi
    aside_mark "$n.taken"
    [[ -n $stop ]] && { $SUDO systemctl start "$stop" || { say "could not start $stop again"; exit 3; }; }
  done < <(lines "$SETASIDE")
  while IFS= read -r line; do
    line=$(placeholders "$line")
    say "setup: $line"
    $SUDO bash -c "$line" || { say "the setup failed: $line"; exit 3; }
  done < <(lines "$SETUP")
  while IFS= read -r unit; do
    [[ $(systemctl is-active "$unit" 2>/dev/null) == active ]] || { say "pause: $unit (not running: left as it is)"; continue; }
    aside_mark "paused.$unit"
    $SUDO systemctl stop "$unit" || { say "could not pause $unit"; exit 3; }
    say "pause: $unit stopped for the trial"
  done < <(lines "$PAUSE")

  local mark
  mark=$(mark_now)
  for unit in $(units_of_switches); do
    $SUDO mkdir -p "$UNIT_DIR/$unit.d"
    {
      echo "# trials.sh $row, $(date '+%F %T'): removed when the trial ends"
      dropin "$unit" || exit 3
    } | $SUDO tee "$UNIT_DIR/$unit.d/$DROPIN" >/dev/null
  done
  $SUDO systemctl daemon-reload
  for unit in $(units_of_switches); do
    restart "$unit" || { say "FAIL: $unit did not come back active with the switches on"; exit 1; }
  done
  local switch
  while IFS= read -r line; do
    unit=${line%%|*} switch=${line#*|}
    if carries "$unit" "$switch"; then
      say "on: $unit has $switch"
    else
      say "FAIL: $unit's process doesn't carry $switch"
      exit 1
    fi
  done < <(lines "$SWITCHES")
  for unit in $(units_of_switches); do
    say "build: $unit runs $(build_of "$unit")"
  done

  while IFS= read -r line; do
    act cause "$line" || exit 1
  done < <(lines "$CAUSE")

  local deadline found
  deadline=$(($(date +%s) + WAIT))
  if [[ -n $FIRST ]]; then
    # shellcheck disable=SC2086 # IN is a list of units
    until found=$(since_mark "$mark" $IN | grep -E -m1 -- "$FIRST") && [[ -n $found ]]; do
      if [[ $(date +%s) -ge $deadline ]]; then
        say "FAIL: $row: no line matching /$FIRST/ in $IN within $WAIT s. Its last lines:"
        # shellcheck disable=SC2086
        since_mark "$mark" $IN | tail -15 | sed 's/^/  /'
        exit 1
      fi
      sleep 1
    done
    say "began: $found"
    local pid
    for pid in $KEEPERS; do kill "$pid" 2>/dev/null || true; done
    KEEPERS=""
    while IFS= read -r line; do
      act then "$line" || exit 1
    done < <(lines "$THEN")
    deadline=$(($(date +%s) + WAIT))
  fi
  # shellcheck disable=SC2086 # IN is a list of units
  while :; do
    if [[ $IN == bus:* ]]; then
      found=
      # A topic said every 5 s shows within 6.
      if $SUDO "$BUS" hz "${IN#bus:}" --seconds 6 >/dev/null 2>&1; then
        found=$($SUDO "$BUS" latest "${IN#bus:}" 2>/dev/null | grep -E -m1 -- "$PROOF" || true)
      fi
    else
      found=$(since_mark "$mark" $IN | grep -E -m1 -- "$PROOF" || true)
    fi
    if [[ -n $found ]]; then
      say "PASS: $row"
      printf '  %s\n' "$found"
      exit 0
    fi
    [[ $(date +%s) -ge $deadline ]] && break
    sleep 1
  done
  if [[ $IN == bus:* ]]; then
    say "FAIL: $row: no fresh ${IN#bus:} matching /$PROOF/ within $WAIT s. Its newest:"
    $SUDO "$BUS" latest "${IN#bus:}" 2>&1 | tail -3 | sed 's/^/  /'
  else
    say "FAIL: $row: no line matching /$PROOF/ in $IN within $WAIT s. Its last lines:"
    # shellcheck disable=SC2086
    since_mark "$mark" $IN | tail -15 | sed 's/^/  /'
  fi
  exit 1
}

case ${1:-} in
  list)
    for row in $ROWS; do
      define "$row"
      printf '%-12s %s\n' "$row" "$ABOUT"
    done
    if [[ -n $NOT_HERE ]]; then
      echo
      echo "Not here:"
      lines "$NOT_HERE" | sed 's/^/  /'
    fi
    ;;
  "" | -h | --help) usage ;;
  *)
    [[ $# -le 2 ]] || usage
    trial "$@"
    ;;
esac
