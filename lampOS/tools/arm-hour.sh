#!/usr/bin/env bash
# The morning's arm hour on a lamp, row by row, for the person at the arm and
# the builder who runs it from the Mac (the first builder on #1722, 20 Oct
# 2026 02:59: "the person at the arm should not be reading PRs"). The rows are
# the plan on #1711 (comment 6276088878), each with its run sheet's steps.
#
#   arm-hour.sh               # every row, in order
#   arm-hour.sh --from N      # from row N
#   arm-hour.sh --only N      # row N alone (12b, for a row added after 12)
#   arm-hour.sh --dry         # print every row's lines and steps, run nothing
#   arm-hour.sh --preflight   # read-only, before the hour: is each row ready?
#
# Each row:
#  1. says, in two lines, what the person does and what they watch for;
#  2. waits: Enter to start it, s to skip it, q to stop;
#  3. if it moves the body, asks "Hand near the arm? (y)", and nothing moves
#     without that y;
#  4. runs its steps in order: on the lamp, on the Mac, a trials.sh row, a
#     line for the person to say, a wait;
#  5. shows its proof, and asks whether it passed (y, n, or a few words);
#  6. puts the lamp back before the next row starts: on a pass, a fail, a
#     failed step, q or ^C. A trials.sh row that is stopped is ended on the
#     lamp itself (TERM to trials.sh, whose own trap puts it back), and the
#     tool waits for it to be gone before it says "Stopped": a killed ssh
#     alone leaves it running there (the first builder, #2034, 03:29).
# What was asked and answered goes to a log, one line a row, to post from.
#
# Settings:
#   ARM_LAMP    a command that runs its one argument, a shell string, on the
#               lamp (the builder's own lamp.sh); needed unless --dry. This
#               script holds no address and no credential.
#   ARM_TRIALS  trials.sh's path on the lamp (/home/orangepi/lampos-tools/trials.sh):
#               the morning build's lampOS/tools/trials.sh, with its lampOS/skills
#               beside it, copied there before the hour. Checked at the start
#               when a row to run has a trial.
#   ARM_LOG     where the log goes (./arm-hour-YYYYmmdd-HHMM.log)
#   ARM_LAMP_HOST  the lamp for a file copied from the Mac, as scp takes it
#               (orangepi@lamp-4ace.local)
#   ARM_SCP     the command that copies a file to the lamp (scp), so one with
#               its own key and known hosts can be given
#   ARM_PRESENCE_CHECK  #1677's presence-check.sh on the Mac, in a checkout of
#               the morning build's commit (row 19)
#   ARM_ROWS_FILE  rows to use in place of the table below (arm-hour-test.sh)
set -uo pipefail

usage() { sed -n '7,11p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

TRIALS=${ARM_TRIALS:-/home/orangepi/lampos-tools/trials.sh}
LOG=${ARM_LOG:-./arm-hour-$(date '+%Y%m%d-%H%M').log}

# ---------------------------------------------------------------------------
# The rows. A row begins "row | N | TITLE | moves|still | about", then lines
# "KIND | VALUE", until "end":
#   do     what the person does, one line
#   watch  what they watch for, one line
#   lamp   a command run on the lamp (through ARM_LAMP)
#   mac    a command run on the Mac
#   trial  a trials.sh row, run on the lamp (its own put-back included)
#   say    a line for the person to say; Enter once it's said
#   tell   what the person does now; Enter once they've begun
#   wait   seconds to let pass
#   proof  a command on the lamp whose output is the row's proof
#   back   a command on the lamp that puts it back ("mac: COMMAND" on the
#          Mac); all of them run, in order, after the row, whatever happened
#   sheet  where the run sheet is
#   question  the row's own question after its proof, in place of "Did it
#          pass?"; the answer is logged as it is given
#   need   for --preflight only, and read-only: what the row needs before
#          the hour. "running | UNIT ..." (each active on the lamp),
#          "skill | NAME ..." (in the running lamp-skills' folder), "merged |
#          PR ..." (merged), "run | RUN ARTIFACT" (a green run that still
#          has it), or "WHAT | COMMAND" (on the lamp, "mac: COMMAND" on the
#          Mac), there when it exits 0
# A row with no steps is done by hand: its two lines, then the question.
# ---------------------------------------------------------------------------
# Read with read, not $(cat <<...): bash 3.2 counts quotes inside a here-doc
# in $( ), and a row's apostrophe would end the script.
IFS= read -r -d '' ROWS_TABLE <<'ROWS' || true
row | 1 | The safe fold before an unplug | moves | 25 min
do | Hands near the arm but off it, desk clear; say each line when asked; never touch the cable.
watch | The arm folds slowly (under 30 s) to its rest and stays held, no sag; then the bottom LEDs breathe dim amber and it says "My arm is folded. You can unplug me now."; asked to look or dance after, it doesn't move.
sheet | #1722, Fifth, 20 Oct 04:43, row 1 without the reboot (any lamp; scripted, fifth-rows.sh)
need | running | lamp-body lamp-voice lamp-expression lamp-skills
mac | d=/tmp/fifth-row1; rm -rf $d && mkdir -p $d && gh api repos/autonomous-ai/lampOS/issues/comments/6278063826 --jq .body | awk '/^```bash$/{f=1;next} f&&/^```$/{exit} f' >$d/fifth-rows.sh && echo "d8a06babff6bf85043dadefc69039451d57951b43d52bdd52aef50c78b115b70  $d/fifth-rows.sh" | shasum -a 256 -c && gh run download 39313641196 -R autonomous-ai/lampOS -n lampos-arm64-396e8a2b3eea57322bf38a27ec453e2d0cebb74c -D $d/a && mv $d/a/lamp-voice $d/a/lamp-expression $d/a/lamp-body $d/a/lamp-skills $d/ && rm -rf $d/a && for f in skill.toml SKILL.md; do gh api -H 'Accept: application/vnd.github.raw' "repos/autonomous-ai/lampOS/contents/lampOS/skills/prepare-unplug/$f?ref=396e8a2b3eea57322bf38a27ec453e2d0cebb74c" >$d/$f || exit 1; done
lamp | rm -rf /tmp/fifth-row1 && mkdir -p /tmp/fifth-row1
mac | ${ARM_SCP:-scp} -q /tmp/fifth-row1/* "${ARM_LAMP_HOST:-orangepi@lamp-4ace.local}":/tmp/fifth-row1/
lamp | sudo bash /tmp/fifth-row1/fifth-rows.sh 1 preflight
tell | On the camera: is the arm folded and still? If not, q now.
lamp | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 install
tell | Hand near the arm: the body restarts on #1699's build and comes up as on any start.
lamp | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 body
say | Lamp, what time is it?
wait | 15
proof | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 proof
tell | Answered on the second route and the lamp still now? If not, q (nothing is folded yet: the put-back gives everything back).
say | Lamp, get ready, I'm going to unplug you.
wait | 45
proof | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 proof
say | Lamp, look at me.
say | Lamp, dance.
wait | 15
proof | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 proof
lamp | sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 back
tell | The arm stays folded and held with torque; only the next start of the lamp clears it. Nothing here reboots: ask the owner now. A no: it stays folded until the lamp next starts.
back | [ ! -d /var/lib/lampos-fifth/row1 ] || sudo bash /var/lib/lampos-fifth/row1/fifth-rows.sh 1 back
back | rm -rf /tmp/fifth-row1
back | mac: rm -rf /tmp/fifth-row1
end
row | 2 | Moves with its words while it speaks | moves | 10 min
do | Listen to the same two-minute story twice, A then B, hands near the arm. Say "stop" at any jerk.
watch | A nods on most words; B about once a sentence, on a word that stands out. Each nod about 2 degrees, a small roll as a sentence begins, one soft settle when the story ends. Nothing jerks.
sheet | #2034, Thirteen, 20 Oct 04:13 (#2030 in the build; #1759's speaking-ab.sh, 7b19b8bba or later)
need | merged | 2030
tell | The build has #2030. Take the lamp's own speaking section out of its catalogue first, as Thirteen's sheet says.
tell | Arm A, dense: jq '.speaking += {"sentence_nods":0,"stress_step":0,"nod_spacing_us":0}' crates/expression/alive-behaviours.json >dense.json, then speaking-ab.sh --arms on --seconds 200 --speaking dense.json with "Hey Lamp, tell me a two-minute story about a cat who loved the rain."
tell | Arm B, sparse: speaking-ab.sh --arms on --seconds 200 --speaking crates/expression/alive-behaviours.json, the same line. Note each arm's starts=, ends= and offsets= lines.
question | Which reads as alive, A or B? (and both arms' starts=)
end
row | 3 | "Look for my keys" | moves | 5 min
do | Hand near the arm. Keys on the desk off to one side, not in front. When the screen says PLAY, say "Hey Lamp, look for my keys." Say "stop" to end it, or let it finish.
watch | The head comes down and looks at the desk first, then a little left or right, each place said; it stops at the keys or after five looks; "stop" ends it at once and the head is up about 2 s later.
sheet | #2034, Nine, 20 Oct 04:15; tools/trials.sh look (#2040, merged and in the build on the lamp)
need | running | lamp-body
trial | look
end
row | 4 | Pointing the light ("light my book") | moves | 5 min
do | Hand near the arm. A book on the desk a little to one side. When the screen says PLAY, say "Hey Lamp, light my book." Afterwards ask for the light you want: the white light is kept.
watch | The head comes down over the desk, the light goes a clear white, it looks; the base turns toward the book once if it is off to one side; the pose holds about two minutes, then the head goes back.
sheet | #2034, Nine, 20 Oct 04:15; tools/trials.sh point (#2040, merged and in the build on the lamp)
need | running | lamp-body
trial | point
end
row | 5 | Light scenes that move the head (reading, focus) | moves | 3 min
do | Hands near the head, not under it; say "Lamp, reading light please.", later "Lamp, light for focus please."
watch | The head tips down smoothly and the arm leans in; the light goes white and brighter; it holds 60 s, then the head returns.
sheet | #1730, Fifth, 19 Oct 19:44Z
need | skill | light-scenes
need | running | lamp-body
proof | sudo journalctl -u lamp-skills --since -5min -o cat | grep -E 'light-scenes (Triggered|Done)' | tail -3
proof | lamp-bus latest light/state
end
row | 6 | Touch pads | still | 20 min
do | Hands off for 11 s counting aloud, then each head pad: a tap, a 1 s stroke and a 3 s hold; then hands away 60 s.
watch | Each touch gives its own event, and nothing comes while hands are away.
sheet | #1722, Thirteen, 19 Oct 16:18Z (its steps.sh --write keeps a new pins file: by hand)
need | running | lamp-controls
end
row | 7 | Bedtime and Morning (the droop at night) | moves | 5 min
do | Not run this morning: #1732 is a draft on no build, and at 08:00 only its morning window is open. Skip it.
watch | Nothing.
sheet | #2034, Nine, 20 Oct 04:15: why it can't be shown at 08:00, and what five minutes would need once it lands
end
row | 8 | Not feeling well (the droop) | moves | 20 min
do | Move any cable from under the arm; stand beside it; nobody talks to the lamp. When told (about 4:45 after the eyes stop), hold an open hand 5 cm under the head, not touching, until the droop is over; say "stop" only if it would hit something.
watch | About 305 s after the stop the head droops slowly once and the ring goes pale and low, breathing slowly; after the eyes start again the light is back within 12 s, with no second droop.
sheet | #1722, Fifth, 20 Oct 03:57 (any lamp; scripted, fifth-rows.sh)
need | running | lamp-body lamp-eyes
mac | d=/tmp/fifth-row8; rm -rf $d && mkdir -p $d && gh api repos/autonomous-ai/lampOS/issues/comments/6277160673 --jq .body | awk '/^```bash$/{f=1;next} f&&/^```$/{exit} f' >$d/fifth-rows.sh && echo "1be88030af34ef0096781a0103c69a6451e795024706d67fc7aa35de3c182133  $d/fifth-rows.sh" | shasum -a 256 -c && gh run download 39299260822 -R autonomous-ai/lampOS -n lampos-arm64-782e6ef34547008443828158fe3540fd2cd23cec -D $d/a && mv $d/a/lamp-skills $d/a/lamp-health $d/ && rm -rf $d/a && for f in skill.toml SKILL.md; do gh api -H 'Accept: application/vnd.github.raw' "repos/autonomous-ai/lampOS/contents/lampOS/skills/not-feeling-well/$f?ref=782e6ef34547008443828158fe3540fd2cd23cec" >$d/$f || exit 1; done
lamp | rm -rf /tmp/fifth-row8 && mkdir -p /tmp/fifth-row8
mac | ${ARM_SCP:-scp} -q /tmp/fifth-row8/* "${ARM_LAMP_HOST:-orangepi@lamp-4ace.local}":/tmp/fifth-row8/
lamp | sudo bash /tmp/fifth-row8/fifth-rows.sh 8 preflight
lamp | sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 install
lamp | sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 stop-eyes
wait | 285
tell | Hand 5 cm under the head now, not touching, until the droop is over; then watch the ring for 60 s.
wait | 80
proof | sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 proof
lamp | sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 start-eyes
wait | 15
proof | sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 proof
back | [ ! -d /var/lib/lampos-fifth/row8 ] || sudo bash /var/lib/lampos-fifth/row8/fifth-rows.sh 8 back
back | rm -rf /tmp/fifth-row8
back | mac: rm -rf /tmp/fifth-row8
end
row | 9 | Faces from a photo, confirmed at the lamp | still | 5 min
do | A consenting person's photo is added while they are out of view; then they sit facing the lamp and answer "Yes, remember me."
watch | The lamp asks once, by the name chosen, and keeps them.
sheet | #1968, Ten, 19 Oct 20:04Z (the photo is copied from the Mac: by hand)
need | python3, and the eyes' who.sock open to this user | command -v python3 >/dev/null && test -w /run/lampos-eyes/who.sock
proof | python3 -c 'import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(10);s.connect("/run/lampos-eyes/who.sock");s.sendall(b"{\"list\":true}\n");print(s.makefile().readline().strip())'
proof | python3 -c 'import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(10);s.connect("/run/lampos-eyes/who.sock");s.sendall(b"{\"who\":true}\n");print(s.makefile().readline().strip())'
end
row | 10 | The first five minutes | moves | 15 min
do | A new person in the chair, a hand near the arm. Once they sit, the lamp starts its first five minutes on its own; they answer each step as it asks. Say "stop" at any jerk.
watch | In order: where they sit (still for 3 s, face seen); their name, and whether it may remember their face (kept only on a yes); "Hey Lamp" called from the chair up to 5 times, with no answer to those; privacy, not available here; giving way (they push the head gently, more than a nudge, hold 1 to 2 s, let go: it says it felt that and nods); preferences, one question at a time.
sheet | #1964, Seventh, 20 Oct 04:34 and 04:39 (build c334d25ae: #1792, #2004, #1964, #1998; ARM64 39328531309)
need | run | 39328531309 lampos-arm64-c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d
need | running | lamp-voice lamp-name lamp-ears lamp-body lamp-eyes
need | the voice's body socket (LAMP_BODY_MOOD_SOCKET) | sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-voice)/environ | tr '\0' '\n' | grep -q '^LAMP_BODY_MOOD_SOCKET='
mac | R=39328531309 S=c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d; [ "$(gh run view $R -R autonomous-ai/lampOS --json conclusion -q .conclusion)" = success ] || { echo "run $R is not green: skip this row"; exit 1; }; mkdir -p /tmp/seventh-$S && gh run download $R -R autonomous-ai/lampOS -n lampos-arm64-$S -D /tmp/seventh-$S
mac | S=c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d; ${ARM_SCP:-scp} -q /tmp/seventh-$S/lamp-voice "${ARM_LAMP_HOST:-orangepi@lamp-4ace.local}":/tmp/lamp-voice-$S
lamp | systemctl cat lamp-voice | grep -E '^(ExecStart|Environment)='
tell | Is that today's voice, with LAMP_BODY_MOOD_SOCKET set? If not, q now.
lamp | systemctl is-active lamp-name lamp-ears
lamp | b=/var/lib/lampos-seventh/row10; sudo mkdir -p $b && for p in /var/lib/lampOS/voice/onboarding /var/lib/lampOS/voice/near-call.json; do if [ -e $p ]; then sudo cp -a $p $b/ && echo "set aside: $p" || exit 1; else echo "not there: $p"; fi; done && sudo touch $b/taken && sudo rm -rf /var/lib/lampOS/voice/onboarding /var/lib/lampOS/voice/near-call.json && sudo install -d -m 0700 /var/lib/lampOS/voice/onboarding
lamp | S=c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d; sudo install -D -m 0755 /tmp/lamp-voice-$S /var/lib/lampos-seventh/$S/lamp-voice && sudo mkdir -p /run/systemd/system/lamp-voice.service.d && { printf '[Service]\nExecStart=\n'; systemctl cat lamp-voice | grep '^ExecStart=' | tail -1 | sed -E "s#^ExecStart=[^ ]+#ExecStart=/var/lib/lampos-seventh/$S/lamp-voice#"; printf 'Environment=LAMP_ONBOARDING=on\nEnvironment=LAMP_ONBOARDING_FILE=/var/lib/lampOS/voice/onboarding/progress.json\nEnvironment=LAMP_VOICE_ROUTE=second\nEnvironment=LAMP_MEMORY_DAYS=on\nEnvironment=LAMP_MEMORY_RULES=on\n'; } | sudo tee /run/systemd/system/lamp-voice.service.d/60-seventh.conf >/dev/null && sudo systemctl daemon-reload && sudo systemctl restart lamp-voice && sudo cat /proc/$(systemctl show -p MainPID --value lamp-voice)/environ | tr '\0' '\n' | grep -E '^LAMP_(ONBOARDING|VOICE_ROUTE)'
tell | The new person sits in the chair. Time each step from the lamp's ask to its answer. If nothing is asked within 30 s: "Lamp, let's redo the setup."
tell | Enter once the lamp says the walk is finished, or after 10 minutes.
proof | sudo sh -c 'cat /var/lib/lampOS/voice/sessions/$(systemctl show -p InvocationID --value lamp-voice)/metadata/events-*.jsonl' | grep -E 'onboarding_(step|near_call|giving_way_try|stopped|redo|finished)|setup_step' | tail -30
tell | If they said yes to being remembered: "Hey Lamp, forget my face." Any rule they gave in preferences: ask the lamp to forget it.
question | Each step: outcome and seconds from the ask (step, outcome, s)? Did anything jerk or read as a machine?
back | S=c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d b=/var/lib/lampos-seventh/row10; ok=1; sudo rm -f /run/systemd/system/lamp-voice.service.d/60-seventh.conf; sudo systemctl daemon-reload; if [ -e $b/taken ]; then sudo systemctl stop lamp-voice; sudo rm -rf /var/lib/lampOS/voice/onboarding /var/lib/lampOS/voice/near-call.json; for n in onboarding near-call.json; do [ ! -e $b/$n ] || sudo cp -a $b/$n /var/lib/lampOS/voice/ || ok=0; done; fi; sudo systemctl restart lamp-voice || ok=0; [ $ok = 1 ] && sudo rm -rf $b /var/lib/lampos-seventh/$S /tmp/lamp-voice-$S && echo "put back: the voice on its own build, its onboarding as it was"; [ $ok = 1 ]
back | mac: rm -rf /tmp/seventh-c334d25aeb0f5c1c7c1d86ec142d04ae4fee740d
end
row | 11 | Setup for a home, the 5 s reset | still | 3 min
do | Nothing at the lamp: a read-only check only. The reset itself waits for the owner (it wipes lamp-4ace's person and key and takes it off the office network).
watch | Nothing changes on the lamp. A 0 on the last line means the button would do nothing.
sheet | #2034, Nine, 20 Oct 04:46 (the reset run itself, with a way back, is there for the owner's yes)
need | running | lamp-controls
proof | for u in lamp-setup.service lamp-setup-reset.service lampos-setup-done.path; do printf '%s: ' $u; systemctl is-enabled $u 2>&1; done
proof | for f in /etc/lampOS/setup/serve.json /etc/lampOS/setup/reset-plan.json /var/lib/lampos-setup/done; do [ -e $f ] && echo "$f: present" || echo "$f: missing"; done
proof | tr '\0' ' ' </proc/$(systemctl show -p MainPID --value lamp-controls)/cmdline | grep -o -- '--pins [^ ]*' | cut -d' ' -f2 | xargs grep -c '"reset"'
end
row | 12 | The wellbeing nudges (rest your eyes) | moves | 3 min
do | Nobody at the desk; hands clear of the head.
watch | The head tilts once (3 s) and the light dims a shade.
sheet | #1957, tools/trials.sh nudges
need | the eyes' LAMP_ACTIVITY off (an empty room's sitting_s holds the nudges back) | ! sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-eyes)/environ | tr '\0' '\n' | grep -qx 'LAMP_ACTIVITY=on'
trial | nudges
end
row | 12b | A sip of water | moves | 2 min
do | Hand near the arm, nobody at the lamp: the trial says a face near the lamp itself.
watch | The head looks toward the middle of the desk once and the light breathes a slow blue for about 6 s, then is as it was.
sheet | #2034, Nine, 20 Oct 05:14; tools/trials.sh sip-of-water (#2050)
need | merged | 2050
need | the eyes' LAMP_ACTIVITY off (an empty room's sitting_s holds the nudges back) | ! sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-eyes)/environ | tr '\0' '\n' | grep -qx 'LAMP_ACTIVITY=on'
trial | sip-of-water
end
row | 12c | Stand up | moves | 2 min
do | Hand near the arm, nobody at the lamp: the trial says a face near the lamp itself.
watch | The head looks toward the middle of the desk, then one small tilt (curious): shall we?
sheet | #2034, Nine, 20 Oct 05:14; tools/trials.sh stand-up (#2050)
need | merged | 2050
need | the eyes' LAMP_ACTIVITY off (an empty room's sitting_s holds the nudges back) | ! sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-eyes)/environ | tr '\0' '\n' | grep -qx 'LAMP_ACTIVITY=on'
trial | stand-up
end
row | 12d | Wind down | moves | 2 min
do | Hand near the arm, nobody at the lamp: the trial says a face near the lamp, then that it has gone.
watch | One slow droop (sleepy) and the light a shade dimmer; about 10 s after the trial says the face has gone, the light is back as it was.
sheet | #2034, Nine, 20 Oct 05:14; tools/trials.sh wind-down (#2050)
need | merged | 2050
need | the eyes' LAMP_ACTIVITY off (an empty room's sitting_s holds the nudges back) | ! sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-eyes)/environ | tr '\0' '\n' | grep -qx 'LAMP_ACTIVITY=on'
trial | wind-down
end
row | 13 | Giving way, the first five minutes' step 6 | moves | 5 min
do | Shown in row 10: giving way is a step of its walk, and #1998 is in its build. Here only if row 10 was skipped, by hand.
watch | The lamp says it felt that, and the body nods.
sheet | #1998, Seventh; #1964, Seventh, 20 Oct 04:34 (row 10's step 5)
end
row | 14 | Watches you leave | moves | 3 min
do | Sit facing the lamp for 3 minutes, then stand and walk off to one side.
watch | One short turn of the base toward the side they left (about 3 s), then back.
sheet | #2012, Sixth, 19 Oct 18:15Z; #2022
need | skill | watch-you-leave
need | running | lamp-body lamp-eyes
proof | sudo journalctl -u lamp-skills --since -10min -o cat | grep watch-you-leave | tail -3
proof | lamp-bus latest room/stood_up
end
row | 15 | Two people at the desk | still | 10 min
do | Two people sit at the desk and talk to each other; each line to the lamp is said when the screen asks, the rest is ordinary talk.
watch | The lamp says nothing while they talk; it answers A's time question, A's follow-up, and B's timer, and nothing of B's aside or the closing chat.
sheet | #2034, Eighth, 20 Oct 04:27 (second route; lamp-4ace's unit as it is, nothing switched)
need | the voice on the second route, busy-near, spoken-to on | e=$(sudo -n cat /proc/$(systemctl show -p MainPID --value lamp-voice)/environ | tr '\0' '\n'); for v in LAMP_VOICE_ROUTE=second LAMP_ATTENTION=busy-near LAMP_SPOKEN_TO=on; do printf '%s\n' "$e" | grep -qx "$v" || exit 1; done
proof | sudo journalctl -u lamp-voice --since -1min -o cat | grep -c 'lamp-voice second: request' || true
tell | A and B talk to each other for about a minute, not to the lamp: what they did this morning, lunch plans. Normal voices, turned to each other.
say | A, turned to the lamp: "Hey Lamp, what time is it?"
wait | 6
say | B, to A, not the lamp: "Is it really that late already?"
wait | 4
say | A, to the lamp, no name, within 10 s of its answer: "And what's the weather like tomorrow?"
wait | 8
say | B, turned to the lamp: "Hey Lamp, set a timer for five minutes."
wait | 6
tell | A and B talk to each other again for about 30 seconds, not to the lamp.
proof | sudo journalctl -u lamp-voice --since -4min -o cat | grep -E 'lamp-voice second: request [0-9]+(: its answer is (played|held back)| first answer audio)' | sed -E 's/ [0-9]+ ms after.*//' | tail -40
proof | sudo journalctl -u lamp-voice --since -4min -o cat | grep -c 'its answer is played' ; sudo journalctl -u lamp-voice --since -4min -o cat | grep -c 'its answer is held back'
question | Did it answer exactly the three lines to it (time, weather, timer), and nothing else? (played=, held back=, and any line answered that wasn't to it)
end
row | 16 | Change the Wi-Fi from the app | still | 5 min
do | From the app: first a network that doesn't exist, then the person's own, typed by them.
watch | The old network is back within 60 s after the wrong one; the right one joins.
sheet | #1995, El (no run sheet yet)
end
row | 17 | A mute closes the camera at once | still | 10 min
do | Five rounds on today's voice, then five on #1941's: mute (the button, or a 3 s hand on the head), wait 3 s, unmute. Only the real button or hand.
watch | The camera closes each time. Pass: on #1941's voice, mute to "camera closed" is under 100 ms at the median and under 500 ms every round (today's is about 1.0 to 1.5 s).
sheet | #1941, Seventh, 19 Oct 18:57Z; #1722, Seventh, 20 Oct 03:27
need | running | lamp-eyes lamp-voice
tell | A, today's voice: five rounds of mute, 3 s, unmute.
proof | sudo journalctl -u lamp-eyes --since -5min -o short-monotonic | grep 'camera closed' | tail -10
mac | mkdir -p /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063 && gh run download 39266127252 -R autonomous-ai/lampOS -n lampos-arm64-05f8290d316825f0b6f5a9d4a72af0d093e7d063 -D /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063
mac | ${ARM_SCP:-scp} /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice "${ARM_LAMP_HOST:-orangepi@lamp-4ace.local}":/tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063
lamp | systemctl cat lamp-voice | grep '^ExecStart=' | tail -1
tell | Is that today's voice? If not, q now. Otherwise Enter to put #1941's voice in.
lamp | sudo install -D -m 0755 /tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063 /var/lib/lampos-seventh/05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice && sudo mkdir -p /run/systemd/system/lamp-voice.service.d && systemctl cat lamp-voice | grep '^ExecStart=' | tail -1 | sed -E 's#^ExecStart=[^ ]+#ExecStart=/var/lib/lampos-seventh/05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice#' | { printf '[Service]\nExecStart=\n'; cat; } | sudo tee /run/systemd/system/lamp-voice.service.d/60-seventh.conf >/dev/null && sudo systemctl daemon-reload && sudo systemctl restart lamp-voice
tell | B, #1941's voice: five rounds of mute, 3 s, unmute.
proof | sudo journalctl -u lamp-eyes --since -5min -o short-monotonic | grep 'camera closed' | tail -10
proof | sudo journalctl -u lamp-voice --since -5min -o short-monotonic | grep 'the eyes are told no' | tail -10
back | sudo rm -f /run/systemd/system/lamp-voice.service.d/60-seventh.conf && sudo systemctl daemon-reload && sudo systemctl restart lamp-voice && sudo rm -rf /var/lib/lampos-seventh/05f8290d316825f0b6f5a9d4a72af0d093e7d063 /tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063
back | mac: rm -rf /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063
end
row | 18 | Smoky air | moves | 3 min
do | Nobody near the head.
watch | A smoke reading said: the head lifts a little, an amber breath, the line said once.
sheet | #1836, tools/trials.sh smoky
trial | smoky
end
row | 19 | The light goes out when you leave, for real | still | 25 min
do | Run 1: sit 2 minutes, leave view and earshot for 10, come back. Run 2: sit 30 s, leave within earshot, and once the light is out say "Hey Lamp, what time is it?" from out of view.
watch | The light fades to the ember over about 30 s with no step, after 10 minutes (run 2: 1) with nobody, and comes back within a second of a face or the voice.
sheet | #1677, Nine, 19 Oct 18:59Z; #1722, Nine, 20 Oct 03:26 (from the Mac: presence-check.sh, a phone filming from the doorway)
need | presence-check.sh on the Mac (ARM_PRESENCE_CHECK) | mac: test -x "${ARM_PRESENCE_CHECK:-/nonexistent}"
tell | Run 1: sit 2 minutes, then leave view and earshot for 10 minutes, then come back.
mac | p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && SECONDS_FOR=960 ./presence-check.sh lamp-4ace quiet
tell | Run 2: sit 30 s, leave within earshot; once the light is out, say "Hey Lamp, what time is it?" from out of view.
mac | p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && NOBODY_S=60 SECONDS_FOR=300 ./presence-check.sh lamp-4ace quiet
back | mac: p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && ./presence-check.sh lamp-4ace restore
end
ROWS
[[ -n ${ARM_ROWS_FILE:-} ]] && ROWS_TABLE=$(cat "$ARM_ROWS_FILE")

# ---------------------------------------------------------------------------

say_() { printf '%s\n' "$*"; }
log() { printf '%s\t%s\n' "$(date '+%H:%M:%S')" "$*" >>"$LOG"; }

# The rows' numbers, in order: a number, and a letter for a row added after
# one (12b).
row_numbers() { printf '%s\n' "$ROWS_TABLE" | sed -n 's/^row *| *\([0-9][0-9]*[a-z]*\) *|.*/\1/p'; }

# Row N's lines, from its "row" line to its "end".
row_lines() {
  printf '%s\n' "$ROWS_TABLE" | awk -v n="$1" '
    /^row *\|/ { split($0, f, "|"); gsub(/ /, "", f[2]); on = (f[2] == n) }
    on { print }
    on && /^end *$/ { exit }'
}

field() { # KIND, from row_lines on stdin: the values, one a line
  sed -n "s/^$1 *| *//p"
}

header() { # N: title, moves, about
  local line
  line=$(row_lines "$1" | head -1)
  IFS='|' read -r _ n title moves about <<EOF
$line
EOF
  TITLE=$(echo "$title" | sed 's/^ *//; s/ *$//')
  MOVES=$(echo "$moves" | tr -d ' ')
  ABOUT=$(echo "$about" | sed 's/^ *//; s/ *$//')
}

on_lamp() { # COMMAND
  if [[ $DRY == 1 ]]; then
    say_ "    lamp: $1"
    return 0
  fi
  "$ARM_LAMP" "$1" </dev/null
}

# An answer from the person, read from the terminal (fd 3, opened at the
# start, so a step list being read never takes it); its prompt on stderr, so
# it isn't taken for the answer. No answer at all (the input ended) is q.
ask() { # PROMPT
  local answer=
  printf '%s ' "$1" >&2
  IFS= read -r answer <&3 || answer=q
  printf '%s' "$answer"
}

BACK_PENDING=""
put_back() {
  [[ -z $BACK_PENDING ]] && return 0
  local line failed=0
  say_ "  putting back:"
  while IFS= read -r line; do
    [[ -z $line ]] && continue
    say_ "    $line"
    case $line in
      mac:*)
        if [[ $DRY == 1 ]]; then say_ "    mac: ${line#mac: }"; else bash -c "${line#mac: }" </dev/null || failed=1; fi
        ;;
      *) on_lamp "$line" || failed=1 ;;
    esac
  done <<EOF
$BACK_PENDING
EOF
  BACK_PENDING=""
  if [[ $failed == 1 ]]; then
    say_ "  PUT BACK FAILED: look at the lamp before the next row"
    log "row $ROW put back FAILED"
    return 1
  fi
  return 0
}

# A trial still running on the lamp: ended there (TERM, so its trap puts the
# lamp back), and waited for, up to 60 s. The pattern's [t] keeps it from
# matching the shell that runs it.
TRIAL_RUNNING=0
end_trial() {
  [[ $TRIAL_RUNNING == 1 ]] || return 0
  say_ "  ending the trial on the lamp (its own put-back runs)"
  if on_lamp "sudo pkill -TERM -f '[t]rials\.sh ' ; for i in \$(seq 1 60); do pgrep -f '[t]rials\.sh ' >/dev/null || exit 0; sleep 1; done; exit 1"; then
    say_ "  the trial has ended"
    log "row $ROW trial ended on the lamp"
  else
    say_ "  THE TRIAL IS STILL RUNNING ON THE LAMP: keep hands clear and end it there"
    log "row $ROW trial still running after 60 s"
  fi
  TRIAL_RUNNING=0
}

stop() {
  trap '' INT TERM
  end_trial
  put_back
  say_ ""
  say_ "Stopped at row $ROW. The log: $LOG"
  log "stopped at row $ROW"
  exit 0
}

run_row() { # N
  ROW=$1
  header "$ROW"
  local lines kind value step failed=0 answer
  lines=$(row_lines "$ROW")
  say_ ""
  say_ "Row $ROW: $TITLE ($ABOUT)"
  say_ "  Do:    $(printf '%s\n' "$lines" | field do | head -1)"
  say_ "  Watch: $(printf '%s\n' "$lines" | field watch | head -1)"
  local sheet
  sheet=$(printf '%s\n' "$lines" | field sheet | head -1)
  [[ -n $sheet ]] && say_ "  Run sheet: $sheet"
  local steps runs
  steps=$(printf '%s\n' "$lines" | grep -E '^(lamp|mac|trial|say|tell|wait|proof) *\|' || true)
  runs=$(printf '%s\n' "$lines" | grep -E '^(lamp|mac|trial|say|tell|wait) *\|' || true)
  [[ -z $runs ]] && say_ "  By hand: the builder runs this row from its run sheet; the tool shows its proof and asks."

  if [[ $DRY == 1 ]]; then
    [[ $MOVES == moves ]] && say_ "    ask: Hand near the arm? (y)"
    printf '%s\n' "$lines" | grep -E '^(need|lamp|mac|trial|say|tell|wait|proof|back) *\|' | sed 's/^/    /'
    return 0
  fi

  answer=$(ask "  Enter to start, s to skip, q to stop:")
  case $answer in
    q) stop ;;
    s) say_ "  skipped"; log "row $ROW skipped"; return 0 ;;
  esac
  if [[ $MOVES == moves ]]; then
    answer=$(ask "  The body moves. Hand near the arm? (y)")
    if [[ $answer != y ]]; then
      say_ "  not started: nothing moves without y"
      log "row $ROW not started: no hand at the arm"
      return 0
    fi
  fi
  log "row $ROW started: $TITLE"
  BACK_PENDING=$(printf '%s\n' "$lines" | field back)
  if [[ -z $runs ]]; then
    answer=$(ask "  Run it from the run sheet now. Enter once the try is done, q to stop:")
    [[ $answer == q ]] && stop
  fi

  while IFS= read -r step; do
    [[ -z $step ]] && continue
    kind=$(echo "$step" | sed 's/ *|.*//')
    value=$(echo "$step" | sed 's/^[a-z]* *| *//')
    case $kind in
      lamp) say_ "  lamp: $value"; on_lamp "$value" || failed=1 ;;
      mac) say_ "  mac: $value"; bash -c "$value" </dev/null || failed=1 ;;
      trial)
        say_ "  trial: trials.sh $value"
        TRIAL_RUNNING=1
        on_lamp "sudo $TRIALS $value" || failed=1
        TRIAL_RUNNING=0
        ;;
      say)
        answer=$(ask "  SAY: \"$value\"  (Enter once said, q to stop)")
        [[ $answer == q ]] && stop
        ;;
      tell)
        answer=$(ask "  NOW: $value  (Enter, q to stop)")
        [[ $answer == q ]] && stop
        ;;
      wait) say_ "  waiting $value s"; sleep "$value" ;;
      proof) say_ "  proof:"; on_lamp "$value" | sed 's/^/    /' || true ;;
    esac
    if [[ $failed == 1 ]]; then
      say_ "  FAILED at: $kind | $value"
      log "row $ROW step failed: $kind | $value"
      break
    fi
  done <<EOF
$steps
EOF

  if [[ $failed == 0 ]]; then
    local question
    question=$(printf '%s\n' "$lines" | field question | head -1)
    answer=$(ask "  ${question:-Did it pass? (y, n, or a few words)} (q to stop)")
    [[ $answer == q ]] && stop
    case $answer in
      y) log "row $ROW passed" ;;
      n) log "row $ROW did not pass" ;;
      *) log "row $ROW: $answer" ;;
    esac
  fi
  put_back || {
    answer=$(ask "  Enter to go on anyway, q to stop:")
    [[ $answer == q ]] && stop
  }
}

# ---------------------------------------------------------------------------
# --preflight: read-only, before the hour (the first builder, #1722, 04:51).
# One line a row: ready, by hand, or what it is missing. None of a row's steps
# runs; what is checked is its need lines, and what its steps show it needs:
# a trial is a row of ARM_TRIALS's trials.sh (its --dry), with the skills its
# setup copies beside it and the units it switches running; a build a mac
# step downloads (gh run download RUN ... -n ARTIFACT) is from a green run
# that still has it; a script a mac step fetches from a comment still has the
# SHA-256 the step checks. First, each running lamp part's program and
# SHA-256, to compare with the morning build.
REPO=autonomous-ai/lampOS

IFS= read -r -d '' PARTS_CHECK <<'CHECK' || true
for u in $(systemctl list-units --type=service --state=running --plain --no-legend 'lamp-*' | awk '{print $1}'); do
  p=$(systemctl show -p MainPID --value "$u")
  [ "${p:-0}" -gt 0 ] || continue
  e=$(sudo -n readlink "/proc/$p/exe" 2>/dev/null)
  h=$(sudo -n sha256sum "/proc/$p/exe" 2>/dev/null | cut -c1-12)
  printf '  %s: %s %s\n' "$u" "${e:-?}" "${h:-?}"
done
sudo -n true 2>/dev/null && echo "  sudo without a password: yes" || echo "  sudo without a password: NO, and the rows need it"
CHECK

# On the lamp: what trial @ROW@ of trials.sh @TRIALS@ is missing, if anything.
IFS= read -r -d '' TRIAL_CHECK <<'CHECK' || true
t=@TRIALS@ r=@ROW@
[ -x "$t" ] || { printf 'no trials.sh at %s; ' "$t"; exit 0; }
out=$("$t" "$r" --dry 2>/dev/null) || { printf 'trials.sh has no row %s; ' "$r"; exit 0; }
src=$(dirname "$t")/../skills
for s in $(printf '%s\n' "$out" | grep -o '@SRC@/[A-Za-z0-9_-]*' | sort -u); do
  [ -d "$src/${s#@SRC@/}" ] || printf 'skill %s not beside trials.sh; ' "${s#@SRC@/}"
done
for u in $(printf '%s\n' "$out" | sed -n 's/^  switch: \([^|]*\)|.*/\1/p' | sort -u); do
  systemctl is-active --quiet "$u" || printf '%s not running; ' "$u"
done
CHECK

# On the lamp: which of the skills @NAMES@ the running lamp-skills lacks.
IFS= read -r -d '' SKILL_CHECK <<'CHECK' || true
p=$(systemctl show -p MainPID --value lamp-skills)
d=$(tr '\0' '\n' <"/proc/${p:-0}/cmdline" 2>/dev/null | grep -A1 -x -- --dir | sed -n 2p)
[ -n "$d" ] || { printf 'lamp-skills not running; '; exit 0; }
for n in @NAMES@; do sudo -n test -d "$d/$n" || printf 'skill %s not in %s; ' "$n" "$d"; done
CHECK

run_missing() { # RUN ARTIFACT
  local conclusion
  conclusion=$(gh run view "$1" -R "$REPO" --json conclusion -q .conclusion </dev/null 2>/dev/null)
  if [[ $conclusion != success ]]; then
    printf 'run %s not green (%s); ' "$1" "${conclusion:-unknown}"
    return
  fi
  gh api "repos/$REPO/actions/runs/$1/artifacts?per_page=100" --jq '.artifacts[] | select(.expired | not) | .name' </dev/null 2>/dev/null |
    grep -qx -- "$2" || printf 'run %s no longer has %s; ' "$1" "$2"
}

need_missing() { # VALUE: "KIND | REST"
  local what rest n check
  what=$(printf '%s' "${1%%|*}" | sed 's/^ *//; s/ *$//')
  rest=$(printf '%s' "${1#*|}" | sed 's/^ *//; s/ *$//')
  case $what in
    running) on_lamp "for u in $rest; do systemctl is-active --quiet \$u || printf '%s not running; ' \$u; done" 2>/dev/null ;;
    skill) check=${SKILL_CHECK//@NAMES@/$rest}; on_lamp "$check" 2>/dev/null ;;
    merged)
      for n in $rest; do
        [[ $(gh pr view "$n" -R "$REPO" --json state -q .state </dev/null 2>/dev/null) == MERGED ]] || printf '#%s not merged; ' "$n"
      done
      ;;
    run) run_missing ${rest%% *} ${rest#* } ;;
    *)
      if [[ $rest == mac:* ]]; then
        bash -c "${rest#mac: }" </dev/null >/dev/null 2>&1 || printf '%s; ' "$what"
      else
        on_lamp "$rest" >/dev/null 2>&1 || printf '%s; ' "$what"
      fi
      ;;
  esac
}

steps_missing() { # LINES
  local step kind value check got
  local run_re='gh run download ([0-9]+) .*-n (lampos-arm64-[0-9a-f]+)'
  local sha_re='issues/comments/([0-9]+) .*echo "([0-9a-f]{64})  '
  while IFS= read -r step; do
    kind=$(echo "$step" | sed 's/ *|.*//')
    value=$(echo "$step" | sed 's/^[a-z]* *| *//')
    case $kind in
      trial)
        check=${TRIAL_CHECK//@TRIALS@/$TRIALS}
        on_lamp "${check//@ROW@/$value}" 2>/dev/null
        ;;
      mac)
        [[ $value =~ $run_re ]] && run_missing "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
        if [[ $value =~ $sha_re ]]; then
          local id=${BASH_REMATCH[1]} sha=${BASH_REMATCH[2]}
          got=$(gh api "repos/$REPO/issues/comments/$id" --jq .body </dev/null 2>/dev/null |
            awk '/^```bash$/{f=1;next} f&&/^```$/{exit} f' | shasum -a 256 | cut -d' ' -f1)
          [[ $got == "$sha" ]] || printf 'comment %s no longer gives %s; ' "$id" "${sha:0:8}"
        fi
        ;;
    esac
  done <<EOF
$(printf '%s\n' "$1" | grep -E '^(trial|mac) *\|' || true)
EOF
}

preflight() {
  local row lines needs runs missing not_ready=0
  say_ "The lamp's parts as they run now (program, SHA-256 start), for the morning build:"
  on_lamp "$PARTS_CHECK" || say_ "  the lamp could not be read"
  say_ ""
  # Not n: header sets that one.
  for row in $(row_numbers); do
    [[ -n $ONLY && $row != "$ONLY" ]] && continue
    [[ ${row%%[a-z]*} -lt $FROM ]] && continue
    header "$row"
    lines=$(row_lines "$row")
    needs=$(printf '%s\n' "$lines" | field need)
    runs=$(printf '%s\n' "$lines" | grep -E '^(lamp|mac|trial|say|tell|wait) *\|' || true)
    missing=$(
      while IFS= read -r value; do
        [[ -n $value ]] && need_missing "$value"
      done <<EOF
$needs
EOF
      steps_missing "$lines"
    )
    if [[ -n $missing ]]; then
      say_ "row $row, $TITLE: MISSING: ${missing%; }"
      not_ready=1
    elif [[ -z $runs && -z $needs ]]; then
      say_ "row $row, $TITLE: by hand, nothing to check"
    else
      say_ "row $row, $TITLE: ready"
    fi
  done
  return $not_ready
}

exec 3<&0
DRY=0 FROM=1 ONLY= PREFLIGHT=0
while [[ $# -gt 0 ]]; do
  case $1 in
    --dry) DRY=1 ;;
    --preflight) PREFLIGHT=1 ;;
    --from) FROM=${2:?}; shift ;;
    --only) ONLY=${2:?}; shift ;;
    -h | --help) usage ;;
    *) usage ;;
  esac
  shift
done
if [[ $DRY == 0 ]]; then
  [[ -n ${ARM_LAMP:-} ]] || { echo "ARM_LAMP: the command that runs a shell string on the lamp" >&2; exit 2; }
fi
if [[ $PREFLIGHT == 1 ]]; then
  preflight
  exit $?
fi
if [[ $DRY == 0 ]]; then
  trap 'say_ ""; stop' INT TERM
  for n in $(row_numbers); do
    [[ -n $ONLY && $n != "$ONLY" ]] && continue
    [[ ${n%%[a-z]*} -lt $FROM ]] && continue
    if row_lines "$n" | grep -q '^trial *|'; then
      if ! on_lamp "test -x $TRIALS" >/dev/null 2>&1; then
        echo "No trials.sh at $TRIALS on the lamp: copy the morning build's lampOS/tools/trials.sh (and lampOS/skills beside it) there, or set ARM_TRIALS to where it is." >&2
        exit 2
      fi
      break
    fi
  done
  log "arm hour began, ARM_LAMP=$ARM_LAMP"
fi
ROW=0
for n in $(row_numbers); do
  [[ -n $ONLY && $n != "$ONLY" ]] && continue
  [[ ${n%%[a-z]*} -lt $FROM ]] && continue
  run_row "$n"
done
[[ $DRY == 1 ]] || { say_ ""; say_ "Done. The log: $LOG"; log "arm hour done"; }
