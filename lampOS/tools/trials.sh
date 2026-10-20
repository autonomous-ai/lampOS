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
#   TRIALS_CALL        sends a call: COMMAND SOCKET LINE (socat, else nc -U;
#                      for send and sendfile, socat)
#   TRIALS_ETC         where the lamp's settings are read (/etc)
#   LAMP_BUS_BIN       lamp-bus (on PATH, else /usr/local/bin/lamp-bus)
#   TRIALS_MOOD_BIN    lamp-mood, for the mood row (/opt/lampOS/mood/current/lamp-mood)
set -euo pipefail

usage() { sed -n '7,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

HERE=$(cd "$(dirname "$0")" && pwd)
SKILLS_SRC=${TRIALS_SKILLS_SRC:-$HERE/../skills}
UNIT_DIR=${TRIALS_UNIT_DIR:-/run/systemd/system}
ETC=${TRIALS_ETC:-/etc}
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
MOOD_BIN=${TRIALS_MOOD_BIN:-/opt/lampOS/mood/current/lamp-mood}

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
#            "keepfile PATH TEXT" (TEXT written to PATH every second until
#            the end, as a file another part keeps fresh), "call NAME JSON"
#            (a skill's function called on lamp-skills' call.sock, as the
#            voice calls it), "send SOCKET LINE" (one line to a part's
#            socket as root with socat; a reply without "done" fails, and
#            only done or the refusal is printed), "run COMMAND", or "play
#            LINE". AFTER may also "sendfile SOCKET FILE": each line of FILE
#            sent so, if FILE is there (what SETUP saved to put back), with
#            neither the lines nor the replies printed, since they may hold
#            a person's name
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
#   PROOFS   optional, in place of PROOF, EXAMPLE and IN, for a row that
#            shows its behaviour in more than one place: one a line,
#            "IN|REGEX|EXAMPLE", each looked for as PROOF is in its IN. The
#            row passes when every one has been seen, in any order. REGEX may
#            hold "|"; EXAMPLE may not
#   ABSENT   optional: lines as PROOFS's, each a line or message that must
#            not have come: judged once, when every proof has been seen, over
#            the journal since the switches went on, or a bus topic's newest
#            message. Its EXAMPLE is the line that would fail the row
# In SETASIDE, SETUP and AFTER, @SKILLS_DIR@, @SKILLS_USER@ and @SKILLS_GROUP@
# are the running lamp-skills' --dir, user and group, @MEMORY_DIR@ and
# @MEMORY_SOCKET@ are lamp-memory's --dir and --socket, @EYES_KNOWN_FILE@ is
# lamp-eyes' --known-file, @SRC@ is TRIALS_SKILLS_SRC, @ASIDE@ is the
# trial's own root-only set-aside folder, @UNIT_DIR@ is where drop-ins go
# (units a trial installs go there too, gone at a reboot), @ETC@ is
# TRIALS_ETC, @EXPRESSION_ARGV@, @EXPRESSION_USER@, @EXPRESSION_GROUP@ and
# @BUS_ROOT@ are lamp-expression's command line, user, group and --bus-root,
# @MOOD_BIN@ is TRIALS_MOOD_BIN, and @BUS@ is lamp-bus. CAUSE's keepfile
# takes them too.
# ---------------------------------------------------------------------------
ROWS="light-out scenes who-was-here activity forget thread reports nudges smoky dusty light-show settings look point sip-of-water stand-up wind-down mood guard"
# Rows not here, and why: printed by `list`.
NOT_HERE="bedtime    Bedtime and Morning (#1732): a draft, not merged; three switches, --zone and its daily state, and it moves the body
mood       Mood's minute with the body moving: lampOS/tools/mood-minute.sh --clap (#1956); the mood row here moves nothing
updates    the update stack (#1680: #1788 to #1945, open): updates.md's rehearsal, a file channel and a test key, not a switch"

define() {
  ABOUT= NOTE= SETASIDE= SETUP= PAUSE= SWITCHES= CAUSE= AFTER= FIRST= THEN= FIRSTEX= PROOF= EXAMPLE= IN= WAIT= PROOFS= ABSENT=
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
    thread)
      # #1733 (lamp-voice), #1930 (#1712's second route): an open thread at
      # least 6 h old is told once, to a session opened for its person. Its
      # line: remember.rs THREAD_TOLD (#2023), counts only, no words.
      # The person is made up, "Trial Person", whose memory id is p- and the
      # first 16 hex of sha256("trial person"), as remember::person_id makes
      # it. The setup sends three lines to lamp-memory's socket (socat, else
      # nc -U): an exchange 7 h ago, its conversation's end, and one thread
      # observation citing it.
      ABOUT="a thread brought up once: a session for its person is told one open thread (#1733, #2023)"
      NOTE="the second route, LAMP_VOICE_ROUTE=second, with LAMP_MEMORY_DAYS=on in the voice, and a build with #2023 (its proof line); socat or nc -U on the lamp for the setup. The person is made up: a name kept fresh in the eyes' known file while the eyes are paused, never a camera. The memory store is set aside and put back"
      SETASIDE="copy @MEMORY_DIR@ lamp-memory.service"
      SETUP='s=@MEMORY_SOCKET@; p=p-$(printf %s "trial person" | sha256sum | cut -c1-16); at=$(( $(date +%s) * 1000 - 25200000 )); send() { local a i; for i in 1 2 3 4 5 6 7 8 9 10; do if command -v socat >/dev/null; then a=$(printf "%s\n" "$1" | socat -t 10 - "UNIX-CONNECT:$s"); else a=$(printf "%s\n" "$1" | nc -U -w 10 "$s"); fi; [ -n "$a" ] && { printf "%s\n" "$a"; return 0; }; sleep 1; done; return 1; }; id=$(send "{\"op\":\"append\",\"person\":\"$p\",\"at_unix_ms\":$at,\"heard\":\"my sister wedding is on Saturday\",\"said\":\"That sounds lovely\"}" | sed -n "s/.*\"id\":\([0-9]*\).*/\1/p"); [ -n "$id" ] || exit 1; job=$(send "{\"op\":\"conversation_end\",\"person\":\"$p\",\"conversation\":\"trial-thread\",\"source_ids\":[$id]}" | sed -n "s/.*\"job\":\"\(j[0-9]*\)\".*/\1/p"); [ -n "$job" ] || exit 1; send "{\"op\":\"tidy_apply\",\"person\":\"$p\",\"job\":\"$job\",\"observations\":[{\"text\":\"Their sister wedding is on Saturday\",\"category\":\"thread\",\"source_ids\":[$id],\"supersedes\":null}]}" | grep -q "\"ok\":true"'
      PAUSE="lamp-eyes.service"
      SWITCHES="lamp-voice.service|LAMP_MEMORY_THREADS=on"
      CAUSE="keepfile @EYES_KNOWN_FILE@ Trial Person
play Hey Lamp, good morning."
      AFTER="run rm -f @EYES_KNOWN_FILE@"
      PROOF='told one open thread to ask about'
      EXAMPLE='lamp-voice provider: a session for someone known was told one open thread to ask about (#1693)'
      IN="lamp-voice.service"
      WAIT=120 ;;
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
    light-show)
      # #1730 (on lamp-voice): light_show called as the voice calls it. The
      # run waits up to 5 s for the model's line after a call (lamp-skills
      # main.rs, SPEAK_BEGIN), which a call from here never brings, then
      # plays the show. Party first, stopped by someone speaking: a show
      # looks every 200 ms (SHOW_LOOKS) whether voice/state says listening,
      # kept up here. Then calm, played to its end (five frames of 10 s).
      # Each run's line is the skill's Done, which says how its show went
      # and that the light was given back.
      ABOUT="light show: light_show's party stops when someone speaks, then calm plays through (#1730)"
      NOTE="a build with #1730, socat or nc -U on the lamp for the calls, the voice resting and nobody speaking to it. Nothing moves: the ring plays a party show, stops within a fifth of a second of the trial saying the voice listens (voice/state; Expression takes it only as a warning), then plays calm for about 50 s, and gives the person's light back after each"
      SETASIDE="copy @SKILLS_DIR@/light-show"
      SETUP="rm -rf @SKILLS_DIR@/light-show && cp -a @SRC@/light-show @SKILLS_DIR@/light-show
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/light-show"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on"
      CAUSE='call light_show {"show":"party"}
wait 9
keep voice/state latest {"state":"listening"} lamp_msgs/VoiceState'
      FIRST='light-show Done: .*show party: stopped in frame [0-9]+ of [0-9]+, the voice is listening to someone'
      # The why is the run's steps' reports joined by "; " (main.rs
      # perform): the show's own is certain, the rest left out.
      FIRSTEX='lamp-skills: light-show Done: …; show party: stopped in frame 2 of 10, the voice is listening to someone; …'
      THEN='pub voice/state latest {"state":"resting"} lamp_msgs/VoiceState
call light_show {"show":"calm"}'
      AFTER='pub voice/state latest {"state":"resting"} lamp_msgs/VoiceState'
      PROOF='light-show Done: .*show calm: 5 frames shown'
      EXAMPLE='lamp-skills: light-show Done: …; show calm: 5 frames shown; …'
      IN="lamp-skills.service"
      WAIT=90 ;;
    settings)
      # #1982 (on lamp-voice): root's helper for changes after setup, as
      # lamp-cloud asks it with LAMP_SETTINGS_CHANGE=on (the first builder's
      # (a), #1722 03:16): its two units, in deploy/product/ and on no lamp
      # yet, installed for the trial in the transient unit folder, and the
      # lines lamp-cloud sends (cloud settings.rs `line`) sent to its socket.
      # The app's half, over the broker, stays with lamp-cloud's host tests.
      # The trial's copy of the socket is root's group: lamp-4ace has no
      # lampos, and only the trial connects. The zone tried is Asia/Bangkok,
      # the lamp's own offset, so nothing's day shifts. The old name and zone
      # are saved first and sent back at the end; the helper's waiting
      # instance is stopped, so lamp-schedule isn't restarted onto a zone it
      # already has, and the units are removed and checked gone. Its lines:
      # setup main.rs, "lamp-setup: change: {}", the change's `said`.
      ABOUT="settings changed after setup: root's helper sets the person's name and the zone, both put back (#1982)"
      NOTE="the checkout's lampOS/deploy/product beside its skills folder (the two units are copied from it), a build with #1982's lamp-setup at /opt/lampOS/setup/current/lamp-setup, /etc/lampOS/setup/serve.json, setup done (/var/lib/lampos-setup/done, the socket's condition), and socat. Neither the saved name nor a reply is printed. For a minute the person's name is \"Trial\" and the zone Asia/Bangkok (the same offset), then both are as they were. No Wi-Fi"
      SETUP="! systemctl cat lamp-setup-change.socket >/dev/null 2>&1 || { echo 'lamp-setup-change.socket is already on this lamp: not touched'; exit 1; }
zone=\$(cat @ETC@/timezone 2>/dev/null || readlink @ETC@/localtime | sed 's|.*/zoneinfo/||'); [ -n \"\$zone\" ] || { echo 'the zone now is not known: nothing changed'; exit 1; }; name=\$(sed 's/[\\\\\"]/\\\\&/g' @ETC@/lampOS/person/name 2>/dev/null); printf '{\"person_name\":\"%s\"}\n{\"timezone\":\"%s\"}\n' \"\$name\" \"\$zone\" >@ASIDE@/settings.back
sed 's/^SocketGroup=lampos/SocketGroup=root/' @SRC@/../deploy/product/lamp-setup-change.socket >@UNIT_DIR@/lamp-setup-change.socket
cp @SRC@/../deploy/product/lamp-setup-change@.service @UNIT_DIR@/lamp-setup-change@.service
systemctl daemon-reload && systemctl start lamp-setup-change.socket"
      # No part's switch: the helper takes --enable from its own unit, and
      # lamp-cloud isn't in the path. The marker restarts lamp-skills and
      # shows the trial's machinery runs, as scenes' does.
      SWITCHES="lamp-skills.service|LAMP_TRIAL=settings"
      CAUSE='send /run/lampos-setup-change/change.sock {"person_name":"Trial"}
send /run/lampos-setup-change/change.sock {"timezone":"Asia/Bangkok"}'
      AFTER="sendfile /run/lampos-setup-change/change.sock @ASIDE@/settings.back
run systemctl stop lamp-setup-change.socket 'lamp-setup-change@*.service'; rm -f @UNIT_DIR@/lamp-setup-change.socket @UNIT_DIR@/lamp-setup-change@.service; systemctl daemon-reload; ! systemctl cat lamp-setup-change.socket >/dev/null 2>&1"
      FIRST="change: the person's name was changed"
      FIRSTEX="lamp-setup: change: the person's name was changed (5 characters)"
      PROOF='change: the time zone is now Asia/Bangkok'
      EXAMPLE='lamp-setup: change: the time zone is now Asia/Bangkok'
      IN='lamp-setup-change@*'
      WAIT=30 ;;
    look)
      # #1871 (on lamp-voice, installable): "Look for my keys", asked out
      # loud, for the arm hour's row 3. The model calls look_for with the
      # desk first, then left, right, far left, far right, five looks at
      # most, each with the head down over the desk (the reading pose) and
      # the base turned toward the place; someone speaking ("stop") ends the
      # search within a fifth of a second (lamp-skills main.rs, wait_out),
      # and the head is up about 2 s later. Its line: the skill's Done, when
      # the search ends (spoken over, or two minutes after the last look).
      ABOUT="look for my keys: look_for searches the desk, then a few places either side, until it's found or told to stop (#1871)"
      NOTE="a build with #1871, the voice's full look (LAMP_LOOK_FULL=on, lamp-eyes --full-look; lamp-4ace has both) and the second route giving every function (no LAMP_SECOND_FEW_TOOLS). It MOVES: the head comes down over the desk and the base turns, so a hand near the arm. Put keys on the desk to one side first. Say the line; say \"stop\" to end it, or wait two minutes after it finds them"
      SETASIDE="copy @SKILLS_DIR@/look-for-things"
      SETUP="rm -rf @SKILLS_DIR@/look-for-things && cp -a @SRC@/look-for-things @SKILLS_DIR@/look-for-things
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/look-for-things"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on"
      CAUSE="play Hey Lamp, look for my keys."
      PROOF='look-for-things Done'
      # The why is the run's steps' reports joined by "; " (main.rs
      # perform): left out here rather than guessed.
      EXAMPLE='lamp-skills: look-for-things Done: …'
      IN="lamp-skills.service"
      WAIT=240 ;;
    point)
      # #1730 (on lamp-voice, held out of the product): "light my book", for
      # the arm hour's row 4. The model calls point_light: the head down over
      # the desk (the reading pose) and a clear white light, kept as the
      # person's; one look; and, only if the book is off to one side, a
      # second call that turns the base toward it. The pose holds two
      # minutes, then the run ends. Its line: the skill's Done.
      ABOUT="point the light: point_light brings the head over the desk and turns toward the book (#1730)"
      NOTE="a build with #1730, the voice's full look (lamp-4ace has it) and every function given. It MOVES: the head comes down over the desk and the base may turn, so a hand near the arm. Put a book on the desk a little to one side first. The white light is kept as the person's afterwards: ask for the light you want after"
      SETASIDE="copy @SKILLS_DIR@/point-light"
      SETUP="rm -rf @SKILLS_DIR@/point-light && cp -a @SRC@/point-light @SKILLS_DIR@/point-light
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/point-light"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on"
      CAUSE="play Hey Lamp, light my book."
      PROOF='point-light Done'
      EXAMPLE='lamp-skills: point-light Done: …'
      IN="lamp-skills.service"
      WAIT=240 ;;
    sip-of-water)
      # #1957 (on lamp-voice, installable): a slow blue breath of light and a look at the face after 90 minutes at the desk. The trial's copy waits
      # 30 s in place of 90m and has no [active] and [quiet] hours, so it
      # can be tried now. The face is said near the lamp by the trial. With
      # the eyes' room/activity off its two conditions hold (unknown =
      # "holds"). Its line: lamp-skills main.rs, the skill's Done.
      ABOUT="the wellbeing nudges: the head looks at the face once and the light breathes blue for 6 s (#1957)"
      NOTE="a build with #1957, the voice resting, nobody at the lamp (the trial says a face near the lamp itself), and the eyes' LAMP_ACTIVITY off. It MOVES: the head looks at the face once and the light breathes blue for 6 s; keep the arm clear"
      SETASIDE="copy @SKILLS_DIR@/sip-of-water"
      SETUP="rm -rf @SKILLS_DIR@/sip-of-water && cp -a @SRC@/sip-of-water @SKILLS_DIR@/sip-of-water
sed -e 's/^for = \"90m\"/for = \"30s\"/' -e '/^\[active\]/,/^to = /d' -e '/^\[quiet\]/,/^to = /d' @SRC@/sip-of-water/skill.toml >@SKILLS_DIR@/sip-of-water/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/sip-of-water"
      SWITCHES="lamp-skills.service|+--zone system"
      CAUSE='keep humans/faces/tracked latest {"faces":[{"id":"trial","xmin":0.35,"ymin":0.2,"xmax":0.65,"ymax":0.6,"confidence":0.9}]} lamp_msgs/Faces'
      AFTER='pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces'
      PROOF='sip-of-water Done'
      # The why is its steps' own reports joined by "; " (lamp-skills
      # main.rs): left out here rather than guessed.
      EXAMPLE='lamp-skills: sip-of-water Done: …'
      IN="lamp-skills.service"
      WAIT=90 ;;
    stand-up)
      # #1957 (on lamp-voice, installable): a look at the face and a small tilt toward them after 100 minutes. The trial's copy waits
      # 30 s in place of 100m and has no [active] and [quiet] hours, so it
      # can be tried now. The face is said near the lamp by the trial. With
      # the eyes' room/activity off its two conditions hold (unknown =
      # "holds"). Its line: lamp-skills main.rs, the skill's Done.
      ABOUT="the wellbeing nudges: the head looks at the face, then tilts toward them once (#1957)"
      NOTE="a build with #1957, the voice resting, nobody at the lamp (the trial says a face near the lamp itself), and the eyes' LAMP_ACTIVITY off. It MOVES: the head looks at the face, then tilts toward them once; keep the arm clear"
      SETASIDE="copy @SKILLS_DIR@/stand-up"
      SETUP="rm -rf @SKILLS_DIR@/stand-up && cp -a @SRC@/stand-up @SKILLS_DIR@/stand-up
sed -e 's/^for = \"100m\"/for = \"30s\"/' -e '/^\[active\]/,/^to = /d' -e '/^\[quiet\]/,/^to = /d' @SRC@/stand-up/skill.toml >@SKILLS_DIR@/stand-up/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/stand-up"
      SWITCHES="lamp-skills.service|+--zone system"
      CAUSE='keep humans/faces/tracked latest {"faces":[{"id":"trial","xmin":0.35,"ymin":0.2,"xmax":0.65,"ymax":0.6,"confidence":0.9}]} lamp_msgs/Faces'
      AFTER='pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces'
      PROOF='stand-up Done'
      # The why is its steps' own reports joined by "; " (lamp-skills
      # main.rs): left out here rather than guessed.
      EXAMPLE='lamp-skills: stand-up Done: …'
      IN="lamp-skills.service"
      WAIT=90 ;;
    wind-down)
      # #1957 (on lamp-voice, installable): one slow droop and the light a shade dimmer in the hour before bedtime, the dim held until they leave the desk, so it is done only when its trigger ends: FIRST is its Triggered, then the face goes and its gap (2 m, 10 s in the copy) runs out. The trial's copy waits
      # 30 s in place of 20m and has no [active] and [quiet] hours, so it
      # can be tried now. The face is said near the lamp by the trial. With
      # the eyes' room/activity off its two conditions hold (unknown =
      # "holds"). Its line: lamp-skills main.rs, the skill's Done.
      ABOUT="the wellbeing nudges: one slow droop and the light a shade dimmer, given back once the trial says the face is gone (#1957)"
      NOTE="a build with #1957, the voice resting, nobody at the lamp (the trial says a face near the lamp itself), and the eyes' LAMP_ACTIVITY off. It MOVES: one slow droop and the light a shade dimmer, given back once the trial says the face is gone; keep the arm clear"
      SETASIDE="copy @SKILLS_DIR@/wind-down"
      SETUP="rm -rf @SKILLS_DIR@/wind-down && cp -a @SRC@/wind-down @SKILLS_DIR@/wind-down
sed -e 's/^for = \"20m\"/for = \"30s\"/' -e 's/^gap = \"2m\"/gap = \"10s\"/' -e '/^\[active\]/,/^to = /d' -e '/^\[quiet\]/,/^to = /d' @SRC@/wind-down/skill.toml >@SKILLS_DIR@/wind-down/skill.toml
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/wind-down"
      SWITCHES="lamp-skills.service|+--zone system"
      CAUSE='keep humans/faces/tracked latest {"faces":[{"id":"trial","xmin":0.35,"ymin":0.2,"xmax":0.65,"ymax":0.6,"confidence":0.9}]} lamp_msgs/Faces'
      AFTER='pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces'
      FIRST='wind-down Triggered'
      FIRSTEX='lamp-skills: wind-down Triggered: humans/faces/tracked has held for 30 s'
      THEN='pub humans/faces/tracked latest {"faces":[]} lamp_msgs/Faces
wait 15'
      PROOF='wind-down Done'
      # The why is its steps' own reports joined by "; " (lamp-skills
      # main.rs): left out here rather than guessed.
      EXAMPLE='lamp-skills: wind-down Done: …'
      IN="lamp-skills.service"
      WAIT=90 ;;
    mood)
      # #1670's Mood with its readers: #1728 (the ring's breath, lamp-voice
      # ring_mood.rs), #1749 (Expression's liveliness, runtime.rs) and #1772
      # (the familiar level, mood input.rs). Mood runs as a transient unit as
      # mood-minute.sh starts it (lamp-4ace has no lamp-mood unit), as
      # Expression's user. Expression runs in shadow, its own command line
      # without --enable in a drop-in of its own, as speaking-ab.sh
      # --levels-only does: it plans and names each move on expression/state
      # and asks the body for nothing. Mood's pace is exactly 1 from 07:00 to
      # 22:00 with nothing happening (mood engine.rs), and leaves it at night
      # or when a sudden sound lifts its energy: a clap from the Mac, since
      # Mood takes ears/sudden only from the ears.
      ABOUT="Mood on, nothing moving: Mood says its state and breath, Expression takes it in shadow, nobody known (#1728, #1749, #1772)"
      NOTE="a build with #1728, #1749 and #1772; lamp-mood at TRIALS_MOOD_BIN (/opt/lampOS/mood/current/lamp-mood); lamp-voice, lamp-ears and lamp-expression running; nobody the eyes know at the lamp. Nothing moves: Expression is in shadow until the end. By day, clap once near the lamp when asked, 40 s in: Mood's pace is 1 with nothing happening from 07:00 to 22:00. The light is read before and after, and must be as it was"
      # The light's value alone: lamp-bus latest prints the bus's header,
      # which changes with every send, before it (bus main.rs line()).
      # A Mood already running (its unit, or a trial's left behind) is
      # refused before anything changes, as mood-minute.sh refuses it: two
      # would both say mood/state (Jarvis on #2042).
      SETUP='for u in lamp-mood.service lamp-trial-mood.service; do [ "$(systemctl is-active "$u" 2>/dev/null)" != active ] || { echo "Mood already runs ($u): nothing done"; exit 1; }; done
@BUS@ latest light/state 2>/dev/null | grep "^{" | cut -d" " -f2- >@ASIDE@/light || true
mkdir -p @UNIT_DIR@/lamp-expression.service.d && printf "[Service]\nExecStart=\nExecStart=%s\n" "$(printf "%s" "@EXPRESSION_ARGV@" | sed -E -e "s/ --enable( |$)/\1/g" -e "s/%/%%/g")" >@UNIT_DIR@/lamp-expression.service.d/zz-trial-shadow.conf'
      # No switch of its own: the marker restarts Expression into its shadow
      # drop-in, and shows the trial's process is the one running.
      SWITCHES="lamp-expression.service|LAMP_TRIAL=mood"
      # Mood starts after the trial's mark, so the ring's and Expression's
      # first lines are seen. By day they say 1.00 and 0.00, and a change is
      # said only a minute after (MoodTaken); one clap lifts the pace about
      # 25 s and the liveliness about 40 s, and only one counts in 120 s
      # (mood SUDDEN_COOLDOWN_US), so it's asked for 40 s in, to be still
      # there at the minute (the reviewer on #2042, P2-1).
      CAUSE='run systemd-run --unit=lamp-trial-mood --uid=@EXPRESSION_USER@ --gid=@EXPRESSION_GROUP@ -p NoNewPrivileges=yes -p ProtectSystem=strict -p ReadWritePaths=@BUS_ROOT@ -E LAMP_MOOD=on @MOOD_BIN@ serve --zone system --bus-root @BUS_ROOT@ && sleep 1 && systemctl is-active --quiet lamp-trial-mood
wait 40
play one clap, close to the lamp, now (by day: Mood breathes at the plain pace until it; at night it need not)'
      AFTER='run systemctl stop lamp-trial-mood 2>/dev/null; systemctl reset-failed lamp-trial-mood 2>/dev/null; true
run rm -f @UNIT_DIR@/lamp-expression.service.d/zz-trial-shadow.conf
run now=$(@BUS@ latest light/state 2>/dev/null | grep "^{" | cut -d" " -f2- || true); was=$(cat @ASIDE@/light 2>/dev/null); if [ "$now" = "$was" ]; then echo "  the light as it was: $was"; else echo "  the light changed: was $was, now $now"; exit 1; fi'
      # Mood's breath away from the plain one's pace (1): mood.rs MoodBreath.
      # The ring and Expression each say what they took from it (msgs
      # MoodTaken: first, then a change of 0.05 at most once a minute):
      # voice ring_mood.rs taken_line, expression runtime.rs liveliness_line.
      # Expression in shadow, a drift asked for: alive.rs names "drifting".
      # Nobody known: mood.rs leaves familiar out with no humans/familiar.
      PROOFS='lamp-voice.service|the ring breathes with Mood: pace (0\.[0-9]{2}|1\.(0[1-9]|[1-9][0-9]))|lamp-voice ring: the ring breathes with Mood: pace 0.81
lamp-expression.service|Mood.s liveliness -?0\.(0[1-9]|[1-9][0-9])|lamp-expression: Mood'"'"'s liveliness -0.31
bus:mood/state|"breath": ?\{"phase": ?[0-9.e-]+, ?"pace": ?(0\.[0-9]+|1\.0*[1-9][0-9]*|1\.[1-9][0-9]*)\}|{"stamp":{"clock_id":"c","at_us":1,"sequence":1},"session_id":"s","awake":0.15,"energy":0.3,"warmth":0.5,"asleep":false,"familiarity":"unknown","breath":{"phase":0.42,"pace":0.81}}
bus:expression/state|"enabled": ?false.*"drifting"|{"stamp":{"clock_id":"c","at_us":1,"sequence":1},"enabled":false,"behaviours":["breathing","drifting"],"missing":[],"warnings":[],"body":false,"light":false}'
      ABSENT='bus:mood/state|"familiar": ?"|{"stamp":{"clock_id":"c","at_us":1,"sequence":1},"session_id":"s","awake":0.15,"energy":0.3,"warmth":0.5,"asleep":false,"familiarity":"unknown","familiar":"regular","breath":{"phase":0.42,"pace":0.81}}'
      # By day the first lines say 1.00 and 0.00 at once; a clap's change
      # is said a minute after them.
      WAIT=120 ;;
    guard)
      # #1907 (on lamp-voice, installable, not in the product): guard mode
      # turned on as the voice turns it on, with nothing moving. keep_watch
      # turns on only when someone was heard at the lamp within 30 s and the
      # ring can show its mark (lamp-skills watch.rs `turn`), so a line is
      # said on voice/heard first. Then the room is said empty on
      # room/presence: FIRST is the watch's own "the desk is empty: keeping
      # watch". THEN, 40 s on, a run step fails the trial if any text was
      # asked for meanwhile (a text goes on messages/out for lamp-cloud,
      # which this row never needs), and keep_watch is turned off after a
      # line heard: PROOF is its mark given back, which only a shown mark
      # can be. Lines: main.rs `watched`, the skill's Done.
      ABOUT="guard mode: on by voice with its mark, nothing texted from an empty room, off by voice and the mark gone (#1907)"
      NOTE="a build with #1907, socat or nc -U for the calls, nobody at the lamp, and the eyes' presence off or the room empty (the trial says the room is empty itself). Nothing moves: the ring shows guard mode's faint blue mark for about a minute, then the person's light again. No text is sent: the row checks that none is asked for. The \"a visit is texted\" half needs a person and the phone, on the arm hour"
      SETASIDE="copy @SKILLS_DIR@/guard-mode"
      SETUP="rm -rf @SKILLS_DIR@/guard-mode && cp -a @SRC@/guard-mode @SKILLS_DIR@/guard-mode
chown -R @SKILLS_USER@:@SKILLS_GROUP@ @SKILLS_DIR@/guard-mode"
      SWITCHES="lamp-skills.service|LAMP_LIGHT_REQUESTS=on"
      CAUSE='pub voice/heard event {"text":"Keep an eye on things while I am out.","finished":true} lamp_msgs/Speech
call keep_watch {"turn":"on"}
keep room/presence latest {"someone":false,"nobody_s":5} lamp_msgs/RoomPresence'
      FIRST='guard-mode Done: the desk is empty: keeping watch'
      FIRSTEX='lamp-skills: guard-mode Done: the desk is empty: keeping watch'
      THEN='keep room/presence latest {"someone":false,"nobody_s":45} lamp_msgs/RoomPresence
wait 40
run ! journalctl -u lamp-skills.service --since -2min -o cat --no-pager | grep -q "guard-mode Done: text"
pub voice/heard event {"text":"You can stop keeping watch now.","finished":true} lamp_msgs/Speech
call keep_watch {"turn":"off"}'
      AFTER='pub room/presence latest {"someone":true,"nobody_s":0} lamp_msgs/RoomPresence'
      PROOF='guard-mode Done: mark: light given back'
      EXAMPLE='lamp-skills: guard-mode Done: mark: light given back: asked again with a time already passed'
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
  if [[ $text == *@MEMORY_SOCKET@* ]]; then
    value=$(arg_of lamp-memory.service --socket)
    [[ -n $value ]] || { say "lamp-memory.service has no --socket" >&2; exit 3; }
    text=${text//@MEMORY_SOCKET@/$value}
  fi
  if [[ $text == *@EYES_KNOWN_FILE@* ]]; then
    value=$(arg_of lamp-eyes.service --known-file)
    [[ -n $value ]] || { say "lamp-eyes.service has no --known-file" >&2; exit 3; }
    text=${text//@EYES_KNOWN_FILE@/$value}
  fi
  text=${text//@ASIDE@/${ASIDE:-@ASIDE@}}
  text=${text//@UNIT_DIR@/$UNIT_DIR}
  text=${text//@ETC@/$ETC}
  if [[ $text == *@EXPRESSION_* || $text == *@BUS_ROOT@* ]]; then
    value=$(exec_argv lamp-expression.service)
    [[ -n $value ]] || { say "lamp-expression.service has no ExecStart" >&2; exit 3; }
    text=${text//@EXPRESSION_ARGV@/$value}
    value=$(arg_of lamp-expression.service --bus-root)
    text=${text//@BUS_ROOT@/${value:-/run/lampos-bus}}
    value=$(systemctl show lamp-expression.service -p User --value 2>/dev/null)
    text=${text//@EXPRESSION_USER@/${value:-root}}
    value=$(systemctl show lamp-expression.service -p Group --value 2>/dev/null)
    text=${text//@EXPRESSION_GROUP@/${value:-root}}
  fi
  text=${text//@MOOD_BIN@/$MOOD_BIN}
  text=${text//@BUS@/$BUS}
  printf '%s\n' "${text//@SRC@/$SKILLS_SRC}"
}

# One line to a part's socket as root, and its one-line reply: through
# TRIALS_CALL, else socat (lamp-4ace has no nc -U, and no Python is run on a
# lamp).
send_line() { # SOCKET LINE
  if [[ -n ${TRIALS_CALL:-} ]]; then
    $TRIALS_CALL "$1" "$2"
  elif command -v socat >/dev/null; then
    printf '%s\n' "$2" | $SUDO socat -t 15 - "UNIX-CONNECT:$1"
  else
    say "FAIL: socat is needed on the lamp to send to $1"
    return 1
  fi
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
    keepfile)
      # keepfile PATH TEXT: written now and every second until the end, as
      # the part that keeps it fresh would (a known face's name, which the
      # voice takes as seen for 3 s, known_faces.rs FRESH).
      local path text
      rest=$(placeholders "$rest")
      path=${rest%% *}; text=${rest#* }
      say "$when: $path kept as \"$text\" every second"
      local write='mkdir -p "$(dirname "$1")" && printf "%s\n" "$2" >"$1"'
      $SUDO bash -c "$write" _ "$path" "$text" || { say "FAIL: the $when could not write $path"; return 1; }
      (
        while sleep 1; do
          $SUDO bash -c "$write" _ "$path" "$text" 2>/dev/null || true
        done
      ) &
      KEEPERS="$KEEPERS $!"
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
    send | sendfile)
      # send SOCKET LINE, or sendfile SOCKET FILE: each line of FILE. A
      # reply is never printed, only done or the refusal (the helper's own
      # words, which never hold the value): a name's reply holds the name.
      # sendfile's lines are what SETUP saved and may hold a person's name,
      # so they aren't printed either.
      local socket=${rest%% *} what=${rest#* } line answered n=0
      what=$(placeholders "$what")
      if [[ $kind == sendfile ]]; then
        $SUDO test -e "$what" || { say "$when: nothing saved to send back"; return 0; }
        what=$($SUDO cat "$what")
      fi
      while IFS= read -r line; do
        [[ -n $line ]] || continue
        n=$((n + 1))
        if [[ $kind == send ]]; then say "$when: $line to $socket"; else say "$when: saved line $n sent back to $socket"; fi
        answered=$(send_line "$socket" "$line") || { say "FAIL: nothing answered on $socket"; return 1; }
        if [[ $answered == *'"done"'* ]]; then
          printf '  answered: done\n'
        else
          printf '  answered: %s\n' "$(printf '%s' "$answered" | sed -n 's/.*"refused": *"\([^"]*\)".*/refused: \1/p' | head -1)"
          say "FAIL: $socket refused it"
          return 1
        fi
      done <<<"$what"
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

# The row's proofs, one a line as "IN|REGEX|EXAMPLE": PROOFS, else the one
# PROOF in IN with its EXAMPLE.
proofs() {
  if [[ -n $PROOFS ]]; then lines "$PROOFS"; else printf '%s|%s|%s\n' "$IN" "$PROOF" "$EXAMPLE"; fi
}
proof_in() { printf '%s' "${1%%|*}"; }
proof_example() { local rest=${1#*|}; printf '%s' "${rest##*|}"; }
proof_regex() { local rest=${1#*|}; printf '%s' "${rest%|*}"; }

# The line matching REGEX in IN since MARK, or the newest message of a
# "bus:" topic seen fresh; nothing when there is none.
seen() { # MARK IN REGEX
  local mark=$1 in=$2 regex=$3
  if [[ $in == bus:* ]]; then
    # A topic said every 5 s shows within 6.
    if $SUDO "$BUS" hz "${in#bus:}" --seconds 6 >/dev/null 2>&1; then
      $SUDO "$BUS" latest "${in#bus:}" 2>/dev/null | grep -E -m1 -- "$regex" || true
    fi
  else
    # shellcheck disable=SC2086 # IN is a list of units
    since_mark "$mark" $in | grep -E -m1 -- "$regex" || true
  fi
}

# What a proof's IN last said, for a FAIL.
last_said() { # MARK IN
  local mark=$1 in=$2
  if [[ $in == bus:* ]]; then
    $SUDO "$BUS" latest "${in#bus:}" 2>&1 | tail -3 | sed 's/^/  /'
  else
    # shellcheck disable=SC2086
    since_mark "$mark" $in | tail -15 | sed 's/^/  /'
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
    while IFS= read -r line; do
      printf '  proof in %s, within %s s: /%s/\n' "$(proof_in "$line")" "$WAIT" "$(proof_regex "$line")"
    done < <(proofs)
    while IFS= read -r line; do
      printf '  absent in %s: /%s/\n' "$(proof_in "$line")" "$(proof_regex "$line")"
      printf '  absent example: %s\n' "$(proof_example "$line")"
    done < <(lines "$ABSENT")
    [[ -z $FIRST ]] || printf '  first: /%s/, then:\n' "$FIRST"
    [[ -z $FIRST ]] || printf '  first example: %s\n' "$FIRSTEX"
    lines "$THEN" | sed 's/^/  then: /'
    while IFS= read -r line; do
      printf '  example: %s\n' "$(proof_example "$line")"
    done < <(proofs)
    exit 0
  fi

  ASIDE=$($SUDO mktemp -d "$ASIDE_DIR/lampos-trial-$row.XXXXXX") || { say "no set-aside folder in $ASIDE_DIR"; exit 3; }
  $SUDO chmod 0700 "$ASIDE"
  # The put-back is never cut short: a second INT, TERM or HUP while it runs
  # is ignored (the first builder, #2034, 20 Oct 03:40: a second TERM ended
  # it half done). Its children inherit that, so a systemctl or sudo that
  # hangs in it ends only with KILL; the exit code the trial had is kept.
  trap 'trap "" INT TERM HUP; put_back; $SUDO rm -rf "$ASIDE"' EXIT
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
  # Every proof, in any order; then what must not have come, judged once.
  local all=() saw=() n in
  while IFS= read -r line; do all+=("$line"); saw+=(""); done < <(proofs)
  while :; do
    for n in "${!all[@]}"; do
      [[ -n ${saw[$n]} ]] && continue
      saw[$n]=$(seen "$mark" "$(proof_in "${all[$n]}")" "$(proof_regex "${all[$n]}")")
    done
    found=1
    for n in "${!saw[@]}"; do [[ -n ${saw[$n]} ]] || found=; done
    if [[ -n $found ]]; then
      local bad
      while IFS= read -r line; do
        bad=$(seen "$mark" "$(proof_in "$line")" "$(proof_regex "$line")")
        if [[ -n $bad ]]; then
          say "FAIL: $row: $(proof_in "$line") said what it must not, /$(proof_regex "$line")/:"
          printf '  %s\n' "$bad"
          exit 1
        fi
      done < <(lines "$ABSENT")
      say "PASS: $row"
      for n in "${!saw[@]}"; do printf '  %s\n' "${saw[$n]}"; done
      exit 0
    fi
    [[ $(date +%s) -ge $deadline ]] && break
    sleep 1
  done
  for n in "${!all[@]}"; do
    [[ -n ${saw[$n]} ]] && continue
    line=${all[$n]}
    if [[ $(proof_in "$line") == bus:* ]]; then
      in=$(proof_in "$line")
      say "FAIL: $row: no fresh ${in#bus:} matching /$(proof_regex "$line")/ within $WAIT s. Its newest:"
    else
      say "FAIL: $row: no line matching /$(proof_regex "$line")/ in $(proof_in "$line") within $WAIT s. Its last lines:"
    fi
    last_said "$mark" "$(proof_in "$line")"
  done
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
