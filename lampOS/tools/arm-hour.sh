#!/usr/bin/env bash
# The morning's arm hour on a lamp, row by row, for the person at the arm and
# the builder who runs it from the Mac (the first builder on #1722, 20 Oct
# 2026 02:59: "the person at the arm should not be reading PRs"). The rows are
# the plan on #1711 (comment 6276088878), each with its run sheet's steps.
#
#   arm-hour.sh               # every row, in order
#   arm-hour.sh --from N      # from row N
#   arm-hour.sh --only N      # row N alone
#   arm-hour.sh --dry         # print every row's lines and steps, run nothing
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

usage() { sed -n '7,10p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

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
# A row with no steps is done by hand: its two lines, then the question.
# ---------------------------------------------------------------------------
ROWS_TABLE=$(cat <<'ROWS'
row | 1 | The safe fold before an unplug | moves | 5 min
do | Hands near the arm but off it, desk clear; say "Lamp, get ready, I'm going to unplug you."; never touch the cable.
watch | The arm folds slowly (under 30 s) to its rest and stays held; the lamp says it is folded and safe to unplug.
sheet | #1699, Fifth, 19 Oct 19:23Z (written for lamp-52e6: its setup is by hand here)
end
row | 2 | Moves with its words while it speaks | moves | 5 min
do | Ask for a short story and listen.
watch | The head nods on stressed words and sways between sentences, never jerks.
sheet | #1067, #1953 (no run sheet yet)
end
row | 3 | "Look for my keys" | moves | 5 min
do | Say "Hey Lamp, look for my keys." Say "stop" to end it.
watch | The desk first, then a few places either side, each said; it stops when asked.
sheet | #1871 (no run sheet yet)
end
row | 4 | Pointing the light ("light my book") | moves | 5 min
do | Put a book on the desk and say "Hey Lamp, light my book."
watch | The head comes down over the desk, looks, and the base turns toward the book.
sheet | #1730 (no run sheet for this row: #1730's sheet is row 5's)
end
row | 5 | Light scenes that move the head (reading, focus) | moves | 3 min
do | Hands near the head, not under it; say "Lamp, reading light please.", later "Lamp, light for focus please."
watch | The head tips down smoothly and the arm leans in; the light goes white and brighter; it holds 60 s, then the head returns.
sheet | #1730, Fifth, 19 Oct 19:44Z
proof | journalctl -u lamp-skills --since -5min -o cat | grep -E 'light-scenes (Triggered|Done)' | tail -3
proof | lamp-bus latest light/state
end
row | 6 | Touch pads | still | 20 min
do | Hands off for 11 s counting aloud, then each head pad: a tap, a 1 s stroke and a 3 s hold; then hands away 60 s.
watch | Each touch gives its own event, and nothing comes while hands are away.
sheet | #1722, Thirteen, 19 Oct 16:18Z (its steps.sh --write keeps a new pins file: by hand)
end
row | 7 | Bedtime and Morning (the droop at night) | moves | 5 min
do | Leave the lamp alone in the evening light, then come back for the morning.
watch | Dimmer, warmer, quieter, a slow settle; in the morning the reverse.
sheet | #1732, Second (no run sheet yet)
end
row | 8 | Not feeling well (the droop) | moves | 20 min
do | The builder stops lamp-eyes; from about 4:50 after, hold an open hand 5 cm under the head without touching; say "stop" if it would hit.
watch | About 305 s after the stop the head droops slowly once and the ring goes pale and low; after the eyes start again the light is back within 12 s.
sheet | #1681, Fifth, 19 Oct 18:46Z (written for lamp-52e6: its setup is by hand here)
proof | journalctl -u lamp-skills --since -15min -o cat | grep not-feeling-well | tail -3
proof | lamp-bus latest lamp/health
end
row | 9 | Faces from a photo, confirmed at the lamp | still | 5 min
do | A consenting person's photo is added while they are out of view; then they sit facing the lamp and answer "Yes, remember me."
watch | The lamp asks once, by the name chosen, and keeps them.
sheet | #1968, Ten, 19 Oct 20:04Z (the photo is copied from the Mac: by hand)
proof | python3 -c 'import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(10);s.connect("/run/lampos-eyes/who.sock");s.sendall(b"{\"list\":true}\n");print(s.makefile().readline().strip())'
proof | python3 -c 'import socket;s=socket.socket(socket.AF_UNIX);s.settimeout(10);s.connect("/run/lampos-eyes/who.sock");s.sendall(b"{\"who\":true}\n");print(s.makefile().readline().strip())'
end
row | 10 | The first five minutes | moves | 10 min
do | A fresh setup: a new person meets the lamp for its first five minutes.
watch | Each step as onboarding.md has it, in order, nothing skipped.
sheet | #1964, Seventh (run sheet due 04:30)
end
row | 11 | Setup for a home, the 5 s reset | still | 10 min
do | Press for 5 s to reset setup.
watch | The lamp comes back ready to be set up.
sheet | #806 (no run sheet yet)
end
row | 12 | The wellbeing nudges (rest your eyes) | moves | 3 min
do | Nobody at the desk; hands clear of the head.
watch | The head tilts once (3 s) and the light dims a shade.
sheet | #1957, tools/trials.sh nudges
trial | nudges
end
row | 13 | Giving way, the first five minutes' step 6 | moves | 5 min
do | At onboarding step 6, when asked, push the head clearly (more than a nudge), hold 1 to 2 s, let go.
watch | The lamp says it felt that, and the body nods.
sheet | #1998, Seventh, 19 Oct 18:57Z (its install is by hand)
end
row | 14 | Watches you leave | moves | 3 min
do | Sit facing the lamp for 3 minutes, then stand and walk off to one side.
watch | One short turn of the base toward the side they left (about 3 s), then back.
sheet | #2012, Sixth, 19 Oct 18:15Z; #2022
proof | journalctl -u lamp-skills --since -10min -o cat | grep watch-you-leave | tail -3
proof | lamp-bus latest room/stood_up
end
row | 15 | Two people at the desk | still | 10 min
do | Two people talk; one of them speaks to the lamp.
watch | The lamp answers only the one who speaks to it.
sheet | #1895 (no run sheet here)
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
tell | A, today's voice: five rounds of mute, 3 s, unmute.
proof | journalctl -u lamp-eyes --since -5min -o short-monotonic | grep 'camera closed' | tail -10
mac | mkdir -p /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063 && gh run download 39266127252 -R autonomous-ai/lampOS -n lampos-arm64-05f8290d316825f0b6f5a9d4a72af0d093e7d063 -D /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063
mac | ${ARM_SCP:-scp} /tmp/seventh-05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice "${ARM_LAMP_HOST:-orangepi@lamp-4ace.local}":/tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063
lamp | systemctl cat lamp-voice | grep '^ExecStart=' | tail -1
tell | Is that today's voice? If not, q now. Otherwise Enter to put #1941's voice in.
lamp | sudo install -D -m 0755 /tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063 /var/lib/lampos-seventh/05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice && sudo mkdir -p /run/systemd/system/lamp-voice.service.d && systemctl cat lamp-voice | grep '^ExecStart=' | tail -1 | sed -E 's#^ExecStart=[^ ]+#ExecStart=/var/lib/lampos-seventh/05f8290d316825f0b6f5a9d4a72af0d093e7d063/lamp-voice#' | { printf '[Service]\nExecStart=\n'; cat; } | sudo tee /run/systemd/system/lamp-voice.service.d/60-seventh.conf >/dev/null && sudo systemctl daemon-reload && sudo systemctl restart lamp-voice
tell | B, #1941's voice: five rounds of mute, 3 s, unmute.
proof | journalctl -u lamp-eyes --since -5min -o short-monotonic | grep 'camera closed' | tail -10
proof | journalctl -u lamp-voice --since -5min -o short-monotonic | grep 'the eyes are told no' | tail -10
back | sudo rm -f /run/systemd/system/lamp-voice.service.d/60-seventh.conf && sudo systemctl daemon-reload && sudo systemctl restart lamp-voice && sudo rm -rf /var/lib/lampos-seventh /tmp/lamp-voice-05f8290d316825f0b6f5a9d4a72af0d093e7d063
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
tell | Run 1: sit 2 minutes, then leave view and earshot for 10 minutes, then come back.
mac | p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && SECONDS_FOR=960 ./presence-check.sh lamp-4ace quiet
tell | Run 2: sit 30 s, leave within earshot; once the light is out, say "Hey Lamp, what time is it?" from out of view.
mac | p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && NOBODY_S=60 SECONDS_FOR=300 ./presence-check.sh lamp-4ace quiet
back | mac: p=${ARM_PRESENCE_CHECK:?the path of presence-check.sh}; cd "$(dirname "$p")" && ./presence-check.sh lamp-4ace restore
end
ROWS
)
[[ -n ${ARM_ROWS_FILE:-} ]] && ROWS_TABLE=$(cat "$ARM_ROWS_FILE")

# ---------------------------------------------------------------------------

say_() { printf '%s\n' "$*"; }
log() { printf '%s\t%s\n' "$(date '+%H:%M:%S')" "$*" >>"$LOG"; }

# The rows' numbers, in order.
row_numbers() { printf '%s\n' "$ROWS_TABLE" | sed -n 's/^row *| *\([0-9]*\) *|.*/\1/p'; }

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
    printf '%s\n' "$lines" | grep -E '^(lamp|mac|trial|say|tell|wait|proof|back) *\|' | sed 's/^/    /'
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
    answer=$(ask "  Did it pass? (y, n, or a few words; q to stop)")
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

exec 3<&0
DRY=0 FROM=1 ONLY=
while [[ $# -gt 0 ]]; do
  case $1 in
    --dry) DRY=1 ;;
    --from) FROM=${2:?}; shift ;;
    --only) ONLY=${2:?}; shift ;;
    -h | --help) usage ;;
    *) usage ;;
  esac
  shift
done
if [[ $DRY == 0 ]]; then
  [[ -n ${ARM_LAMP:-} ]] || { echo "ARM_LAMP: the command that runs a shell string on the lamp" >&2; exit 2; }
  trap 'say_ ""; stop' INT TERM
  for n in $(row_numbers); do
    [[ -n $ONLY && $n != "$ONLY" ]] && continue
    [[ $n -lt $FROM ]] && continue
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
  [[ $n -lt $FROM ]] && continue
  run_row "$n"
done
[[ $DRY == 1 ]] || { say_ ""; say_ "Done. The log: $LOG"; log "arm hour done"; }
