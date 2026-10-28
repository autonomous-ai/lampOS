#!/usr/bin/env bash
# "An owner makes their own skill" (#1685, #2424; the first builder on #2602, 22 Oct 05:34, and on #2745, 05:43),
# end to end with a stand-in Harness: a made-up voice asks the lamp for a small skill, says yes to the hand-over, and
# the stand-in "makes" it, answering with a fixed skill (stand-up-nudge: once an hour while the voice rests, a warm
# breath of the light, asked of the Policy as a nudge; a made skill may use the light and the head only, never
# speech). The lamp keeps it and asks "Shall I put it on the lamp?"; the script answers. Nothing reaches a real
# Harness or agent: lamp-tasks gets lamp-tasks-harness-fixture as its Harness (stand-in.sh) and a targets file for
# the run, one alias, the voice's own, on the fixture's machine; the voice gets LAMP_MAKE_SKILL=on. All through /run
# drop-ins put back when this ends (and by a reboot), with stand-up-nudge and any other skill this run put on removed.
# The voice's made skills (/var/lib/lampOS/voice/made-skills) and its told news (tasks-told) are saved first, in
# /usr/local/lib/played-make-skill-saved (outside the stand-in's folder, which `out` and the lamp's guard remove); each
# arm starts from the saved made skills, so an ask left by one arm is never answered by the next, and both are put
# back when it ends, with lamp-voice stopped (so it writes neither while they are), checked entry by entry, each
# folder and each file's SHA-256, against the saved copy, which only then is removed. A put-back that fails or
# differs exits 8 and keeps the copy, printing its path; the lamp's guard (a Mac that never came back) runs the same
# put-back first, with lamp-voice stopped too. Neither puts anything back unless the stop worked, and both start it
# again straight after, the put-back passed or not. The copy is made in $SAVED.part and renamed into place only once
# whole, so a save cut short is never taken for a copy. A copy left by an earlier run, or a save cut short, stops
# the next one before any change (exit 3), saying how to recover.
# Hand-overs fail while it runs.
#
#   LAMP_HOST=<lamp> lampOS/tools/played/make-skill.sh LABEL FIXTURE [yes|no|both]
#
# FIXTURE: an ARM64 lamp-tasks-harness-fixture (from a lampOS ARM64 run's artifact). TASKS=PATH (optional): an ARM64
# lamp-tasks from the same build, for a lamp whose own is older (as calendar.sh).
# Each arm prints PASS or FAIL: asked to confirm (the lamp's own question before a hand-over, answered "Yes, please."
# once it has played; passed too when it offers at once), offered (make_skill), handed over (the stand-in's `new`),
# the answer read (its `show`), made (the voice keeps the skill, its SHA-256), asked to put it on, and then:
# - yes ("Yes, go ahead.", a plain yes: the voice puts it on only at one, `known_faces::plain_agreement`, so never at
#   "Yes, put it on."): nothing on before the yes, that SHA-256 put on, enabled (`lamp-skills list`), loaded by
#   the running lamp-skills, and removed (`lamp-skills remove`, gone from the list);
# - no ("No, not now."): nothing put on within 30 s, and the list unchanged.
# PLAYS SOUND IN THE ROOM. About 4 minutes an arm. Needs `sudo -n` on the lamp and its voice quiet for 45 s. Not
# between 12:00 and 14:00 on weekdays, when the office rests. Bash 3.2 (macOS's) or later.
# With MAKE_FROM=app (#2813), the ask comes as the owner's app sends it, typed: CLOUD=PATH, an ARM64 lamp-cloud from the
# same build, is put beside the stand-in, and its `make-skill make|made`, run as lampos (lamp-cloud's own user, so the
# socket's group is what lets it in), asks lamp-voice's app-make socket with the very
# calls the app's skills.make and skills.made make (`LAMP_APP_MAKE_SKILL=on` on the stand-in's drop-in too), with
# no spoken ask and no yes to the hand-over; the app's view is read back (`{"made": ...}`) as kept with the same SHA-256,
# then on (yes arm) or still off (no arm). The lamp still asks aloud, and only the spoken yes puts it on. The running
# lamp-cloud gets the switch too (the stand-in's PLAYED_CLOUD_ENV), so the owner's own app shows "Make a skill" while
# the stand-in is in, and a person can make one from the app (fourteen-P11 on #2814).
# Settings: as the kit's (README.md), and MAKE_FROM (voice, or app), MAKE_SETTLE (20 s after the stand-in is in), NOT_NOW_WAIT (30 s), LAMP_SSH,
# QUIET_GATE, SAY and AFPLAY (the kit's own, or the tests' stand-ins), and for the tests MAKE_SAVED and MAKE_VOICE_DIR
# (where the copy and the voice's state are). Its stand-in test is make-skill-test.sh.
set -u
cd "$(dirname "$0")" || exit 9
. ./settings.sh
L=${1:?LABEL}
FIXTURE=${2:?FIXTURE: an ARM64 lamp-tasks-harness-fixture}
ARMS=${3:-both}
case $ARMS in yes) ARMS="yes" ;; no) ARMS="no" ;; both) ARMS="yes no" ;; *) echo "usage: make-skill.sh LABEL FIXTURE [yes|no|both]" >&2; exit 2 ;; esac
[ -f "$FIXTURE" ] || { echo "no file at $FIXTURE" >&2; exit 2; }
[ -z "${TASKS:-}" ] || [ -f "$TASKS" ] || { echo "no file at TASKS=$TASKS" >&2; exit 2; }
LAMP_SSH=${LAMP_SSH:-./lamp_ssh}
QUIET_GATE=${QUIET_GATE:-./quiet_gate.sh}
SAY=${SAY:-say}
AFPLAY=${AFPLAY:-afplay}
RATE=${LAMP_SAY_RATE:-185}
OUT="$PLAYED_OUT/scenarios/make-skill-$L.txt"
REMOTE=/usr/local/lib/played-stand-in   # /run may be noexec; this folder is removed when the run ends
SKILLS=/opt/lampOS/skills/current/lamp-skills
VOICE_DIR=${MAKE_VOICE_DIR:-/var/lib/lampOS/voice}
SAVED=${MAKE_SAVED:-/usr/local/lib/played-make-skill-saved}   # kept outside REMOTE: `out` and the guard remove REMOTE
REQUEST="Hey Lamp, make me a skill that reminds me to stand up every hour."
FROM=${MAKE_FROM:-voice}
case $FROM in voice|app) ;; *) echo "make-skill.sh: MAKE_FROM is voice or app" >&2; exit 2 ;; esac
APP_WORDS="a skill that reminds me to stand up every hour"
[ "$FROM" = voice ] || [ -f "${CLOUD:-}" ] || { echo "make-skill.sh: MAKE_FROM=app needs CLOUD=PATH, an ARM64 lamp-cloud" >&2; exit 2; }

if [ "$(date +%u)" -le 5 ] && [ "$(date +%H)" -ge 12 ] && [ "$(date +%H)" -lt 14 ] && [ "${LAMP_PLAY_AT_LUNCH:-}" != yes ]; then
  echo "make-skill.sh: not between 12:00 and 14:00 local, when the office rests" >&2
  exit 2
fi
lamp() { "$LAMP_SSH" "$@" </dev/null; }
# The save and put-back, kept in the saved copy as $SAVED/restore and run on the lamp as root, by this script and by the
# lamp's guard. The save writes into $S.part, beside it, and renames that to $S only once the copy is whole and
# checked: a save cut short leaves only $S.part, and the voice was not changed before it (El's P3 on #3889).
# `check`: each of the voice's two paths is a regular file, a folder of only folders and regular files, or not there
# at all (neither -e nor -L); anything else (a symbolic link, dangling or not, a fifo, a device) stops
# the run before any change, and no link is ever copied (Third's P2 on 93f0584c2). `save`: the same check, the
# copies, and each path's kind recorded in `kinds`. `reset` puts the made skills back as saved; with no argument,
# both. A put-back first checks every path it would replace: only a plain file or folder, or one recorded absent and
# so this run's, is ever removed. Each is then checked against the copy, its kind, every folder in it and each file's
# SHA-256, and only a put-back of both that matches removes the copy. Anything else exits 1 and leaves the copy where
# it is. The caller stops lamp-voice first, so it writes neither path meanwhile (El's P3 on #3889).
restore_script() {
  sed -e "s#@SAVED@#$SAVED#" -e "s#@VOICE@#$VOICE_DIR#" <<'EOF'
#!/bin/sh
# make-skill.sh's saved copy of lamp-voice's made skills and told news; `sh restore` puts both back.
S=@SAVED@
V=@VOICE@
NAMES="made-skills tasks-told"
kind() {  # PATH: file, dir or none on stdout, or a failure saying what else it is
  if [ -L "$1" ]; then echo "$1 is a symbolic link" >&2; return 1
  elif [ -f "$1" ]; then echo file
  elif [ -d "$1" ]; then
    odd=$(find "$1" ! -type f ! -type d) || { echo "$1 could not be read whole" >&2; return 1; }
    [ -z "$odd" ] || { echo "$1 holds more than folders and regular files: $odd" >&2; return 1; }
    echo dir
  elif [ -e "$1" ]; then echo "$1 is neither a regular file nor a folder" >&2; return 1
  else echo none; fi
}
# No step's failure is hidden in a pipeline (Third's HOLD on 1c4d27e02): each tool's output is taken whole and its
# own status read, and a manifest is only one whose every step succeeded.
sha() {  # FILE: its SHA-256, 64 hex digits, or a failure
  if command -v sha256sum >/dev/null 2>&1; then o=$(sha256sum "$1") || return 1; else o=$(shasum -a 256 "$1") || return 1; fi
  h=${o%% *}
  case $h in '' | *[!0-9a-f]*) return 1 ;; esac
  [ ${#h} -eq 64 ] || return 1
  printf '%s\n' "$h"
}
# A folder's manifest names every folder in it as well as every file, so a copy that drops an empty folder differs
# (Third's P3 on #3889); `kind` has already refused anything else inside.
sums() {  # DIR NAME...: each NAME's kind, every folder and file in it (a file with its SHA-256), in order, then the
          # count of entries; or a failure
  d=$1; shift
  cd "$d" || return 1
  n=0
  for x in "$@"; do
    if [ -d "$x" ]; then
      echo "dir $x"
      found=$(find "$x" \( -type f -o -type d \)) || return 1
      entries=$(printf '%s\n' "$found" | sort) || return 1
      while IFS= read -r f; do
        if [ -d "$f" ]; then echo "d $f"
        elif [ -f "$f" ]; then h=$(sha "$f") || return 1; echo "f $f $h"
        else return 1; fi
        n=$((n + 1))
      done <<ENTRIES
$entries
ENTRIES
    elif [ -e "$x" ]; then
      h=$(sha "$x") || return 1
      echo "file $x $h"; n=$((n + 1))
    else echo "none $x"; fi
  done
  echo "entries $n"
}
case ${1:-all} in
  check) for x in $NAMES; do kind "$V/$x" >/dev/null || exit 1; done; exit 0 ;;
  save)  # into $S.part, renamed to $S once whole
    P=$S.part
    [ ! -e "$S" ] && [ ! -L "$S" ] || { echo "save: $S holds a copy already" >&2; exit 1; }
    [ -d "$P" ] && [ ! -e "$P/done" ] && [ ! -e "$P/kinds" ] || { echo "save: $P is not a fresh folder" >&2; exit 1; }
    for x in $NAMES; do
      k=$(kind "$V/$x") || exit 1
      if [ "$k" != none ]; then
        cp -a "$V/$x" "$P/" || { echo "save: $V/$x could not be copied" >&2; exit 1; }
        c=$(kind "$P/$x") || exit 1
        [ "$c" = "$k" ] || { echo "save: the copy of $V/$x is a $c, not a $k" >&2; exit 1; }
      fi
      echo "$x $k" >> "$P/kinds" || exit 1
    done
    a=$(sums "$P" $NAMES) && b=$(sums "$V" $NAMES) && [ "$a" = "$b" ] || { echo "save: the copy in $P is not as $V" >&2; exit 1; }
    touch "$P/done" && mv "$P" "$S" || { echo "save: $P could not be put in place as $S" >&2; exit 1; }
    exit 0 ;;
  reset) names=made-skills ;;
  all) names=$NAMES ;;
  *) echo "usage: restore [check|save|reset]" >&2; exit 2 ;;
esac
[ -e "$S/done" ] && [ -f "$S/kinds" ] || { echo "restore: no complete saved copy at $S" >&2; exit 1; }
# Every path is checked before any is removed.
for x in $names; do
  saved_kind=$(sed -n "s/^$x //p" "$S/kinds") || { echo "restore: $S/kinds could not be read; the copy is kept" >&2; exit 1; }
  case $saved_kind in file | dir | none) ;; *) echo "restore: $S/kinds records no kind for $x; the copy is kept" >&2; exit 1 ;; esac
  c=$(kind "$S/$x") || { echo "restore: the copy of $x in $S is not as saved; it is kept" >&2; exit 1; }
  [ "$c" = "$saved_kind" ] || { echo "restore: the copy of $x in $S is a $c, recorded a $saved_kind; it is kept" >&2; exit 1; }
  kind "$V/$x" >/dev/null || { echo "restore: $V/$x is not a plain file or folder, so not this run's to replace; the copy in $S is kept" >&2; exit 1; }
done
for x in $names; do
  rm -rf "$V/$x" && { [ ! -e "$S/$x" ] || cp -a "$S/$x" "$V/"; } || { echo "restore: $x could not be put back from $S" >&2; exit 1; }
done
saved=$(sums "$S" $names) || { echo "restore: the saved copy in $S could not be read whole; it is kept" >&2; exit 1; }
now=$(sums "$V" $names) || { echo "restore: $V could not be read whole; the copy in $S is kept" >&2; exit 1; }
case $saved in *"entries "[0-9]*) ;; *) echo "restore: no whole manifest of $S; it is kept" >&2; exit 1 ;; esac
[ "${saved##*entries }" = "${now##*entries }" ] && [ "$saved" = "$now" ] || { echo "restore: $V is not as saved in $S" >&2; exit 1; }
[ "${1:-all}" = reset ] || rm -rf "$S"
EOF
}
voice_state() {  # save | reset | restore: the voice's made skills and told news, saved; made skills back as saved;
                 # both, with lamp-voice stopped first
  case $1 in
    save) restore_script | "$LAMP_SSH" "sudo -n sh -c 'mkdir -m 0700 $SAVED.part && cat > $SAVED.part/restore && sh $SAVED.part/restore save'" ;;
    reset) lamp "sudo -n sh $SAVED/restore reset" ;;
    # lamp-voice writes both: stopped, so none of its writes lands while they are put back and checked (a false
    # differ, exit 8; El's P3 on #3889). Nothing needs it (no unit Requires or BindsTo it). The stop is required, and
    # proved: a stop that succeeded and then `is-active` printing exactly inactive, or failed (no process left: a stop
    # that timed out and killed it). A failed stop or any other answer (active, activating, deactivating, unknown,
    # nothing, a query error) puts nothing back and keeps the copy, a failure (Third's HOLDs on a8962558b and
    # b2d2dfb0b). Either way the same root shell on the lamp starts it again right after, and that shell ignores a
    # hang-up and a closed pipe, so an ssh or a Mac lost meanwhile never leaves the voice stopped (El's P3 on #3936).
    restore) lamp "sudo -n sh -c 'trap \"\" HUP PIPE; rc=1; if systemctl stop lamp-voice; then st=\$(systemctl is-active lamp-voice 2>/dev/null); else st=\"not stopped\"; fi; case \"\$st\" in inactive | failed) sh $SAVED/restore; rc=\$? ;; *) echo \"restore: lamp-voice could not be stopped, so nothing was put back (its state: \${st:-no answer}); the copy in $SAVED is kept\" >&2 ;; esac; systemctl start lamp-voice; exit \$rc'" ;;
  esac
}

lamp 'sudo -n true' || { echo "make-skill.sh: the lamp's user needs sudo -n" >&2; exit 2; }
env=$(lamp 'systemctl show -p ActiveState -p Environment lamp-voice') || { echo "make-skill.sh: no answer from the lamp" >&2; exit 3; }
case $env in *ActiveState=active*) ;; *) echo "make-skill.sh: lamp-voice is not active; nothing changed" >&2; exit 3 ;; esac
case $env in *LAMP_TASKS_DIR=*) ;; *) echo "make-skill.sh: lamp-voice has no tasks part (LAMP_TASKS_DIR); nothing changed" >&2; exit 3 ;; esac
case $env in *LAMP_MAKE_SKILL=*) echo "make-skill.sh: LAMP_MAKE_SKILL is already set on lamp-voice; nothing changed" >&2; exit 3 ;; esac
case $env in *LAMP_APP_MAKE_SKILL=*) echo "make-skill.sh: LAMP_APP_MAKE_SKILL is already set on lamp-voice; nothing changed" >&2; exit 3 ;; esac
lamp 'systemctl is-active -q lamp-skills-manage.socket' || { echo "make-skill.sh: the skills helper's socket is not active; nothing changed" >&2; exit 3; }
# The run's skill is removed on exit, so one already on is never the run's to take off. Only a list that succeeded
# and is whole (its header first) says it is not on; any other answer stops here (Third's P2 on #3889).
skills=$(lamp "sudo -n $SKILLS list") || { echo "make-skill.sh: the lamp's skills could not be listed (exit $?); nothing changed" >&2; exit 3; }
case ${skills%%
*} in NAME*KIND*VERSION*) ;; *) echo "make-skill.sh: the lamp's skill list is not whole; nothing changed" >&2; exit 3 ;; esac
# grep's own status: 0 on, 1 not on, anything else stops here.
printf '%s\n' "$skills" | grep -qE '^stand-up-nudge +'
case $? in
  0) echo "make-skill.sh: a skill called stand-up-nudge is already on the lamp; nothing changed" >&2; exit 3 ;;
  1) ;;
  *) echo "make-skill.sh: the lamp's skill list could not be read; nothing changed" >&2; exit 3 ;;
esac
NUDGE=stand-up-nudge   # proved not on: this run's to remove
# A copy an earlier run could not put back is the only one of the voice's own state: never saved over (Fourteen's P3).
# A save cut short (the Mac gone before its rename) left only $SAVED.part: the voice was not changed before the save
# was whole, so that folder is safe to remove (El's P3 on #3889).
left=$(lamp "sudo -n sh -c 'if [ -e $SAVED ] || [ -L $SAVED ]; then echo left; elif [ -e $SAVED.part ] || [ -L $SAVED.part ]; then echo part; else echo none; fi'")
case $left in
  none) ;;
  part) echo "make-skill.sh: an earlier run's save was cut short at $SAVED.part, before anything was changed, so the voice's state is as it was; remove it with: sudo rm -rf $SAVED.part. Nothing changed" >&2; exit 3 ;;
  *) echo "make-skill.sh: an earlier run's saved copy of the voice's state is at $SAVED (${left:-no answer}); put it back with: sudo sh $SAVED/restore. Nothing changed" >&2; exit 3 ;;
esac
# The voice's two paths only as a regular file, a folder of plain files, or not there: a link (dangling or not), a fifo
# or a device stops here, before any change (Third's P2 on 93f0584c2).
restore_script | "$LAMP_SSH" "sudo -n sh -s -- check" || { echo "make-skill.sh: the voice's made skills or told news is not a plain file or folder (above); nothing changed" >&2; exit 3; }
"$QUIET_GATE" 45 || { echo "make-skill.sh: the lamp is not quiet: nothing played" >&2; exit 3; }

# The stand-in's answer: the two files of one small skill, as an agent asked for one writes them
# (`lamp_voice::made_skill::files`), within what a made skill may use and through `lamp-skills pack`.
SKILL_TOML='kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["voice/state"]
light = true

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
cooldown = "1h"

[ask]
policy = "nudge"

[[do]]
light = { colour = "warm", level = 2, pattern = { kind = "breathe", period = "8s" } }
'
SKILL_MD='---
name: stand-up-nudge
description: Once an hour, a slow warm breath of the light, a nudge to stand up.
---

# Stand-up nudge

Once an hour while the lamp is quiet, its light breathes warm for a moment: time to stand up.
'
ANSWER=$(printf 'Here is the skill.\n\n```toml skill.toml\n%s```\n\n```markdown SKILL.md\n%s```\n' "$SKILL_TOML" "$SKILL_MD")
# Its plan: its own machine for `machines`, an agent for `new`, and for `show` that answer, from the agent `new` made
# (another id is read as agent_mismatch, never as the answer); `wait` and `ps` say that agent is idle, as Harness does
# (with no step the fixture prints nothing: wait_invalid_reply, status_invalid_reply).
PLAN=$(python3 -c '
import json, sys
agent = "11111111-1111-4111-8111-111111111111"
show = {"id": agent, "state": "idle", "ask": "Write one lamp skill",
        "answer": sys.argv[1], "truncated": False, "tools": 2}
waited = {"harnesses": [{"name": "lamp-task-1", "id": agent, "state": "idle", "next": []}]}
listed = {"harnesses": [{"id": agent, "state": "idle", "lastActive": None, "about": "Wrote skill.toml and SKILL.md"}]}
print(json.dumps({"machines": {"default": True}, "new": {"default": True}, "show": {"stdout": json.dumps(show)},
                  "wait": {"stdout": json.dumps(waited), "delay_ms": 2000}, "ps": {"stdout": json.dumps(listed)}}))
' "$ANSWER")

echo "make-skill $L  $(date -u +%Y-%m-%dT%H:%M:%SZ)  arms: $ARMS  fixture $(shasum -a 256 "$FIXTURE" | cut -c1-16)" | tee "$OUT"
# Until the stand-in is in, a failed upload leaves nothing behind (the Reviewer-lead on #2745).
trap 'lamp "sudo -n rm -rf $REMOTE"' EXIT
"$LAMP_SSH" "sudo -n mkdir -p $REMOTE && sudo -n tee $REMOTE/harness-fixture >/dev/null && sudo -n chmod 0755 $REMOTE/harness-fixture" < "$FIXTURE" || exit 9
printf '%s\n' "$PLAN" | "$LAMP_SSH" "sudo -n tee $REMOTE/plan.json >/dev/null" || exit 9
SETUP_ENV="PLAYED_REMOTE=$REMOTE PLAYED_RESTORE=$SAVED/restore PLAYED_TARGET=auto PLAYED_VOICE_ENV=LAMP_MAKE_SKILL=on"
[ "$FROM" = app ] && SETUP_ENV="PLAYED_REMOTE=$REMOTE PLAYED_RESTORE=$SAVED/restore PLAYED_TARGET=auto PLAYED_VOICE_ENV='LAMP_MAKE_SKILL=on LAMP_APP_MAKE_SKILL=on' PLAYED_CLOUD_ENV=LAMP_APP_MAKE_SKILL=on"
if [ -n "${TASKS:-}" ]; then
  echo "lamp-tasks for this run: $(shasum -a 256 "$TASKS" | cut -c1-12)" | tee -a "$OUT"
  "$LAMP_SSH" "sudo -n tee $REMOTE/lamp-tasks >/dev/null && sudo -n chmod 0755 $REMOTE/lamp-tasks" < "$TASKS" || exit 9
  SETUP_ENV="$SETUP_ENV PLAYED_TASKS=$REMOTE/lamp-tasks"
fi
if [ "$FROM" = app ]; then
  echo "lamp-cloud for this run: $(shasum -a 256 "$CLOUD" | cut -c1-12)" | tee -a "$OUT"
  "$LAMP_SSH" "sudo -n tee $REMOTE/lamp-cloud >/dev/null && sudo -n chmod 0755 $REMOTE/lamp-cloud" < "$CLOUD" || exit 9
fi
voice_state save || {
  lamp "sudo -n rm -rf $SAVED.part"   # a copy cut short is no copy, and nothing was changed
  echo "make-skill.sh: the voice's made skills and told news could not be saved; nothing played" | tee -a "$OUT" >&2; exit 9; }

# The stand-in in (stand-in.sh, on the lamp as root), and put back whatever happens, with stand-up-nudge (an arm
# whose ask the next arm's yes answered leaves it on with no line of its own) and any other skill this run put on, and
# the voice's made skills and told news as saved. A setup that fails says which step and stops here: nothing is played.
# A put-back that fails exits 8 and keeps the saved copy (outside REMOTE, so `out` leaves it), naming its path.
PUT_ON=""
put_back() {
  local rc=0 name done_names=" "
  for name in $NUDGE $PUT_ON; do   # each once, with no pipeline to lose a name
    case $done_names in *" $name "*) continue ;; esac
    done_names="$done_names$name "
    lamp "sudo -n $SKILLS remove $name" >/dev/null 2>&1 && echo "removed on exit: $name"
  done
  if ! voice_state restore; then
    echo "make-skill.sh: the voice's made skills and told news were not put back as saved; the saved copy is kept at $SAVED: put it back with: sudo sh $SAVED/restore"
    rc=8
  fi
  "$LAMP_SSH" "sudo -n env PLAYED_REMOTE=$REMOTE sh -s -- out" < stand-in.sh
  return $rc
}
trap 'put_back 2>&1 | tee -a "$OUT"; [ "${PIPESTATUS[0]}" = 0 ] || exit 8' EXIT
if ! "$LAMP_SSH" "sudo -n env $SETUP_ENV sh -s -- in" < stand-in.sh >> "$OUT" 2>&1; then
  tail -n 2 "$OUT" >&2
  echo "make-skill $L: the stand-in's setup failed; nothing was played (see $OUT)" | tee -a "$OUT" >&2
  exit 9
fi
tail -n 2 "$OUT"
sleep "${MAKE_SETTLE:-20}"

line() {  # VOICE TEXT: say it from this Mac's speaker, made once and kept
  local f="$PLAYED_OUT/prompts/generated/make-skill-$(printf '%s' "$1|$2" | shasum | cut -c1-16).aiff"
  [ -s "$f" ] || "$SAY" -v "$1" -r "$RATE" -o "$f" "$2"
  echo "$(date +%H:%M:%S) PLAY $1: $2" | tee -a "$OUT"
  "$AFPLAY" "$f"
}
journal() {  # SINCE: the voice's, lamp-tasks' and lamp-skills' lines since then
  lamp "sudo -n journalctl -u lamp-voice -u lamp-tasks -u lamp-skills --since @$1 --no-pager -o cat"
}
called() {  # OPERATION: how many times the stand-in was called with it so far, one number
  local n
  n=$(lamp "sudo -n grep -c '\"args\":\[\"$1\"' $REMOTE/home/calls.jsonl || true" 2>/dev/null)
  echo "${n:-0}"
}
answered() {  # how many of the lamp's answers have played to their end in this session, one number
  local n
  n=$(lamp 'sudo -n sh -c '"'"'d=/var/lib/lampOS/voice/sessions; s=$(ls -t $d | head -n 1); cat $d/$s/metadata/events-*.jsonl | grep "\"kind\":\"turn_finished\"" | grep -c "\"outcome\":\"audio_written" || true'"'" 2>/dev/null)
  echo "${n:-0}"
}
said() {  # what the lamp said in its newest session, one line
  lamp 'sudo -n sh -c '"'"'d=/var/lib/lampOS/voice/sessions; s=$(ls -t $d | head -n 1); grep -h "\"kind\":\"transcript\"" $d/$s/metadata/events-*.jsonl'"'" 2>/dev/null \
    | grep -v '"role":"input"' | sed -n 's/.*"text":"\([^"]*\)".*/\1/p' | tr '\n' ' '
}
wait_for() {  # SINCE PATTERN SECONDS: until the journal since SINCE matches PATTERN; print the line
  local until=$(( $(date +%s) + $3 )) hit
  while [ "$(date +%s)" -lt "$until" ]; do
    hit=$(journal "$1" | grep -E -- "$2" | tail -n 1)
    [ -n "$hit" ] && { printf '%s\n' "$hit"; return 0; }
    sleep 1
  done
  return 1
}
wait_answered() {  # COUNT SECONDS: until more than COUNT of the lamp's answers have played to their end
  local until=$(( $(date +%s) + $2 ))
  while [ "$(date +%s)" -lt "$until" ]; do
    [ "$(answered)" -gt "$1" ] && { echo "the lamp's answer played to its end"; return 0; }
    sleep 1
  done
  return 1
}
wait_called() {  # OPERATION COUNT SECONDS: until the stand-in was called with OPERATION more than COUNT times
  local until=$(( $(date +%s) + $3 ))
  while [ "$(date +%s)" -lt "$until" ]; do
    [ "$(called "$1")" -gt "$2" ] && { echo "the stand-in's $1 was called"; return 0; }
    sleep 2
  done
  return 1
}
wait_said() {  # COUNT PATTERN SECONDS: until the lamp's words match PATTERN more than COUNT times
  local until=$(( $(date +%s) + $3 ))
  while [ "$(date +%s)" -lt "$until" ]; do
    [ "$(said | grep -oE -- "$2" | wc -l)" -gt "$1" ] && { echo "the lamp said it"; return 0; }
    sleep 1
  done
  return 1
}
FAILS=0
CHECKS=0
check() {  # NAME LINE-OR-EMPTY: PASS with the line, or FAIL
  CHECKS=$((CHECKS + 1))
  if [ -n "$2" ]; then
    echo "PASS $1: $(printf '%s' "$2" | cut -c1-200)" | tee -a "$OUT"
  else
    FAILS=$((FAILS + 1))
    echo "FAIL $1" | tee -a "$OUT"
  fi
}
listed() { lamp "sudo -n $SKILLS list"; }
app_make() {  # REQUEST_ID: the app's skills.make, by lamp-cloud's own call; `making`, or `refused: WHY`
  lamp "sudo -n -u lampos $REMOTE/lamp-cloud make-skill make '$APP_WORDS' $1"
}
app_made() {  # REQUEST_ID: the app's view of its ask, by lamp-cloud's own skills.made call: `STATE SHA256`
  lamp "sudo -n -u lampos $REMOTE/lamp-cloud make-skill made $1"
}
same() {  # VALUE WANTED...: VALUE when it is one of WANTED, else nothing (Bash 3.2: no case inside $( ))
  local value=$1 wanted
  shift
  for wanted in "$@"; do
    [ "$value" = "$wanted" ] && { printf '%s\n' "$value"; return 0; }
  done
  return 0
}

for arm in $ARMS; do
  echo "--- arm: $arm" | tee -a "$OUT"
  # The made skills as saved: an ask an earlier arm left waiting is not this arm's to answer.
  voice_state reset || { echo "make-skill.sh: the voice's made skills could not be put back before the $arm arm" | tee -a "$OUT"; FAILS=$((FAILS + 1)); break; }
  START=$(lamp 'date +%s')
  news=$(called new); shows=$(called show)
  asks=$(said | grep -oE 'Shall I put it on the lamp' | wc -l)
  before=$(listed)
  RID="make-skill-$arm-$START"
  if [ "$FROM" = app ]; then
    # Typed in the app: the words to the voice's socket, as lamp-cloud's skills.make; no yes to the hand-over.
    asked=$(app_make "$RID")
    echo "$(date +%H:%M:%S) APP skills.make $RID: $asked" | tee -a "$OUT"
    check "asked by the app" "$(same "$asked" making)"
  else
  # The ask. The lamp may first ask whether to hand it over (its own confirm), or offer at once.
  heard=$(answered)
  line Samantha "$REQUEST"
  offered=$(wait_for "$START" 'asked for make_skill' 12)
  if [ -n "$offered" ]; then
    check "asked to confirm" "not needed: offered at once"
  else
    confirm=$(wait_answered "$heard" 40)
    check "asked to confirm" "$confirm"
    [ -n "$confirm" ] && { sleep 1; line Samantha "Yes, please."; }
    offered=$(wait_for "$START" 'asked for make_skill' 40)
  fi
  check "offered" "$offered"
  # Its read-back ends with its question: the yes once it has played to its end.
  if [ -n "$offered" ]; then
    heard=$(answered)
    wait_answered "$heard" 40 >/dev/null
    sleep 1
    line Samantha "Yes, please."
  fi
  fi
  check "handed over" "$(wait_called new "$news" 40)"
  check "the answer read" "$(wait_called show "$shows" 90)"
  kept=$(wait_for "$START" 'a skill made by voice is (kept|not kept)' 120)
  case $kept in *"is kept"*) check "made" "$kept" ;; *) check "made" ""; [ -n "$kept" ] && echo "  $kept" | tee -a "$OUT"; continue ;; esac
  sha=$(printf '%s' "$kept" | sed -n 's/.*sha256 \([0-9a-f]\{64\}\).*/\1/p')
  if [ "$FROM" = app ]; then
    view=$(app_made "$RID")
    check "the app told it kept, the same SHA-256" "$([ -n "$sha" ] && same "$view" "kept $sha" "asked $sha")"
  fi
  # The voice tells news only to someone there: seen, or heard asking within 120 s (El on #2889). Nothing was said
  # in this arm, so a greeting, and its answer to its end; or a face in view does the same. Only while the ask hasn't
  # come already: a face in view brings it soon after the keep, and the yes or no must answer the ask, not a greeting.
  if [ "$FROM" = app ] && [ "$(said | grep -oE 'Shall I put it on the lamp' | wc -l)" -le "$asks" ]; then
    heard=$(answered)
    line Samantha "Hey Lamp, I'm back at my desk."
    wait_answered "$heard" 40 >/dev/null
  fi
  check "asked to put it on" "$(wait_said "$asks" 'Shall I put it on the lamp' 120)"
  heard=$(answered)
  wait_answered "$heard" 30 >/dev/null
  ASKED=$(lamp 'date +%s')
  early=$(journal "$START" | grep -E 'a made skill is on the lamp' | tail -n 1)
  # One put on before the answer is taken off on exit, whatever comes after (the Reviewer-lead on #2745).
  wrong=$(printf '%s\n' "$early" | sed -n 's/.*is on the lamp: \([a-z0-9-]*\), sha256.*/\1/p' | tail -n 1)
  [ -n "$wrong" ] && PUT_ON="$PUT_ON $wrong"
  if [ "$arm" = yes ]; then
    check "nothing on before the yes" "$([ -z "$early" ] && echo "no install line before $ASKED")"
    sleep 1
    line Samantha "Yes, go ahead."
    on=$(wait_for "$ASKED" 'a made skill (is on the lamp|was not put on)' 40)
    name=$(printf '%s' "$on" | sed -n 's/.*is on the lamp: \([a-z0-9-]*\), sha256 .*/\1/p')
    on_sha=$(printf '%s' "$on" | sed -n 's/.*, sha256 \([0-9a-f]\{64\}\).*/\1/p')
    [ -n "$name" ] && PUT_ON="$PUT_ON $name"
    check "installed after the yes, the kept SHA-256" "$([ -n "$name" ] && [ "$on_sha" = "$sha" ] && echo "$on")"
    row=$([ -n "$name" ] && listed | grep -E "^$name +" | head -n 1)
    check "enabled" "$([ -n "$row" ] && ! printf '%s' "$row" | grep -qE 'disabled;|not loaded' && echo "$row")"
    check "loaded by lamp-skills" "$([ -n "$name" ] && wait_for "$ASKED" "lamp-skills: loaded .*\"$name\"" 30)"
    if [ "$FROM" = app ]; then
      view=$(app_made "$RID")
      check "the app told it on" "$([ -n "$sha" ] && same "$view" "on $sha")"
    fi
    if [ -n "$name" ] && lamp "sudo -n $SKILLS remove $name" >/dev/null 2>&1; then
      PUT_ON=${PUT_ON// $name/}
      # Gone only by a list that answered and a grep that found none (1), not by one that failed.
      if after=$(listed); then
        printf '%s\n' "$after" | grep -qE "^$name +"
        check "removed" "$([ $? -eq 1 ] && echo "$name is not listed")"
      else
        check "removed" ""
      fi
    else
      check "removed" ""
    fi
  else
    sleep 1
    line Samantha "No, not now."
    sleep "${NOT_NOW_WAIT:-30}"
    late=$(journal "$ASKED" | grep -E 'a made skill is on the lamp' | tail -n 1)
    # One put on all the same is taken off on exit.
    wrong=$(printf '%s\n' "$late" | sed -n 's/.*is on the lamp: \([a-z0-9-]*\), sha256.*/\1/p' | tail -n 1)
    [ -n "$wrong" ] && PUT_ON="$PUT_ON $wrong"
    check "nothing installed" "$([ -z "$early$late" ] && [ "$(listed)" = "$before" ] && echo "no install line, the list unchanged")"
    if [ "$FROM" = app ]; then
      view=$(app_made "$RID")
      check "the app told it still off" "$([ -n "$sha" ] && same "$view" "asked $sha" "left_off $sha")"
    fi
  fi
  journal "$START" | grep -E 'make_skill|put_on_made_skill|made skill|made by voice|telling news' | cut -c1-220 >> "$OUT"
done
echo "make-skill $L: $CHECKS checks, $FAILS failed. Lines and the journal's made-skill lines: $OUT" | tee -a "$OUT"
exit $(( FAILS > 0 ))
