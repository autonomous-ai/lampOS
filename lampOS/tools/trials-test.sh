#!/usr/bin/env bash
# Tests for trials.sh with no lamp: stand-ins for systemctl, journalctl and
# lamp-bus keep the lamp's side in a temporary directory. A restart gives the
# unit a new process whose environment is its drop-ins' Environment= lines and
# whose command line is the last ExecStart= among its unit and drop-ins; the
# journal is one file of "unit<TAB>line", and a topic's messages a file each.
# Every row is tried: its proof against its example, a pass, a failure, a
# switch its process doesn't carry; then what is set aside, the arguments
# added, the after-steps, a stopped unit, a failed cause, a failed put back
# and a stop mid-wait. Run: lampOS/tools/trials-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/trials.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/units" "$T/proc" "$T/pids" "$T/state" "$T/skills" "$T/memory" "$T/run"

# Each unit's own command line, as systemctl show prints ExecStart.
cat >"$T/argv" <<EOF
lamp-skills.service /opt/lampOS/skills/x/lamp-skills --dir $T/skills --bus-root /run/lampos-bus --tools-dir /run/lampos-skills
lamp-memory.service /opt/lampOS/memory/x/lamp-memory --dir $T/memory --socket /run/lampos-memory/memory.sock
lamp-eyes.service /opt/lampOS/eyes/x/lamp-eyes --known-file $T/eyes/known
lamp-expression.service /opt/lampOS/expression/x/lamp-expression serve --enable --head-touch --bus-root /run/lampos-bus --zone system
EOF

cat >"$T/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "systemctl $*" >>"$FAKE/calls"
argv_of() { # the last ExecStart= of the unit's drop-ins, else its own
  # In byte order, as systemd reads them; a drop-in with no ExecStart= of
  # its own leaves the one before standing.
  local line= found conf
  for conf in $(LC_ALL=C ls "$TRIALS_UNIT_DIR/$1.d"/*.conf 2>/dev/null); do
    found=$(sed -n 's/^ExecStart=\(..*\)/\1/p' "$conf" | tail -1)
    [[ -n $found ]] && line=$found
  done
  [[ -n $line ]] && { printf '%s\n' "${line//%%/%}"; return; }
  sed -n "s|^$1 ||p" "$FAKE/argv"
}
case $1 in
  daemon-reload) ;;
  restart | start)
    # $FAKE/slow-restarts: each restart takes a second (a put-back long
    # enough to be signalled in the middle of).
    [[ -e $FAKE/slow-restarts ]] && sleep 1
    # The Nth restart and later fail when $FAKE/restarts-before-failing holds N-1.
    n=$(( $(cat "$FAKE/restarts" 2>/dev/null || echo 0) + 1 ))
    echo "$n" >"$FAKE/restarts"
    [[ -e $FAKE/restarts-before-failing && $n -gt $(cat "$FAKE/restarts-before-failing") ]] && exit 1
    # Every process its own number, as on a lamp.
    pid=$(( $(cat "$FAKE/last-pid" 2>/dev/null || echo 1000) + 1 ))
    echo "$pid" >"$FAKE/last-pid"
    echo "$pid" >"$FAKE/pids/$2"
    echo active >"$FAKE/state/$2"
    mkdir -p "$FAKE/proc/$pid"
    # The program it runs: $FAKE/builds/<unit>, made on its first start.
    mkdir -p "$FAKE/builds"
    [[ -e $FAKE/builds/$2 ]] || echo "the $2 build" >"$FAKE/builds/$2"
    ln -sf "$FAKE/builds/$2" "$FAKE/proc/$pid/exe"
    : >"$FAKE/proc/$pid/environ"
    if [[ ! -e $FAKE/env-ignored ]]; then
      for conf in "$TRIALS_UNIT_DIR/$2.d"/*.conf; do
        [[ -e $conf ]] || continue
        sed -n 's/^Environment=//p' "$conf" | tr -d '"' | tr '\n' '\0' >>"$FAKE/proc/$pid/environ"
      done
    fi
    # $FAKE/env-ignored: a build that ignores the trial's switches runs its
    # own command line too.
    if [[ -e $FAKE/env-ignored ]]; then
      sed -n "s|^$2 ||p" "$FAKE/argv" | tr ' ' '\0' >"$FAKE/proc/$pid/cmdline"
    else
      argv_of "$2" | tr ' ' '\0' >"$FAKE/proc/$pid/cmdline"
    fi
    printf '%s\tstarted\n' "$2" >>"$FAKE/journal"
    ;;
  stop) echo inactive >"$FAKE/state/$2"; echo 0 >"$FAKE/pids/$2" ;;
  is-active) cat "$FAKE/state/$2" 2>/dev/null || echo active ;;
  show)
    case $4 in
      MainPID) cat "$FAKE/pids/$2" 2>/dev/null || echo 0 ;;
      ExecStart) a=$(argv_of "$2"); [[ -n $a ]] && echo "{ path=${a%% *} ; argv[]=$a ; ignore_errors=no ; start_time=[n/a] ; }" ;;
      User) id -un ;;
      Group) id -gn ;;
    esac
    ;;
  *) exit 1 ;;
esac
EOF

cat >"$T/bin/journalctl" <<'EOF'
#!/usr/bin/env bash
after= units=() show_cursor=
while [[ $# -gt 0 ]]; do
  case $1 in
    --after-cursor=*) after=${1#--after-cursor=} ;;
    -u) units+=("$2"); shift ;;
    --show-cursor) show_cursor=1 ;;
  esac
  shift
done
touch "$FAKE/journal"
if [[ -n $show_cursor ]]; then
  tail -1 "$FAKE/journal" | cut -f2-
  echo "-- cursor: $(wc -l <"$FAKE/journal" | tr -d ' ')"
  # The trial's mark, after its set-aside and set-up, once it is counted:
  # after_mark waits for it.
  touch "$FAKE/marked"
  exit 0
fi
tail -n +"$((${after:-0} + 1))" "$FAKE/journal" | while IFS=$'\t' read -r unit line; do
  for u in "${units[@]}"; do [[ $u == "$unit" ]] && echo "$line"; done
done
EOF

# lamp-bus: each pub is recorded, and then the behaviour's line comes when
# $FAKE/answer names it ("unit<TAB>line"). A topic's messages are the file
# $FAKE/bus/<topic, / as _>: hz succeeds while it is there, latest prints it.
cat >"$T/bin/lamp-bus" <<'EOF'
#!/usr/bin/env bash
file=$FAKE/bus/${2//\//_}
case $1 in
  pub)
    echo "lamp-bus $*" >>"$FAKE/calls"
    # $FAKE/pub-fails: the next pub fails, once.
    [[ -e $FAKE/pub-fails ]] && { rm -f "$FAKE/pub-fails"; exit 1; }
    if [[ -e $FAKE/answer ]]; then cat "$FAKE/answer" >>"$FAKE/journal"; rm -f "$FAKE/answer"; fi
    # $FAKE/answer2, "match<TAB>unit<TAB>line": a second line, at the pub
    # whose arguments hold match (a behaviour done when its cause ends).
    if [[ -e $FAKE/answer2 ]] && [[ $* == *"$(cut -f1 "$FAKE/answer2")"* ]]; then
      cut -f2- "$FAKE/answer2" >>"$FAKE/journal"; rm -f "$FAKE/answer2"
    fi
    ;;
  hz) [[ -e $file ]] ;;
  # As lamp-bus prints it: the bus's header, new with every send, then the
  # value (bus main.rs line()).
  latest) [[ -e $file ]] && printf '{"topic":"%s","seq":%s} %s\n' "$2" "$RANDOM" "$(cat "$file")" ;;
  *) exit 2 ;;
esac
EOF

# sudo: runs the command, and removes as root would what its owner can't
# write (a set-aside of a folder the runner doesn't own).
cat >"$T/bin/fake-sudo" <<'EOF'
#!/usr/bin/env bash
echo "sudo $*" >>"$FAKE/sudo-calls"
if [[ $1 == rm ]]; then
  for a in "${@:2}"; do [[ $a != -* && -e $a ]] && chmod -R u+w "$a" 2>/dev/null; done
fi
exec "$@"
EOF

# lamp-memory's socket, as the thread row's setup sends to it: each line
# recorded, and answered as the memory answers it, with ids to carry on.
cat >"$T/bin/socat" <<'EOF'
#!/usr/bin/env bash
read -r line
echo "$line" >>"$FAKE/memory-sent"
case $line in
  *'"op":"append"'*) echo '{"ok":true,"id":7}' ;;
  *'"op":"conversation_end"'*) echo '{"ok":true,"input":{"job":"j3","person":"p-x","generation":1,"source_ids":[7],"records":[],"previous":[]}}' ;;
  *'"op":"tidy_apply"'*) echo '{"ok":true,"ids":[8]}' ;;
  *) echo '{"ok":false}' ;;
esac
EOF

# systemd-run: recorded, as the mood row starts Mood with it.
cat >"$T/bin/systemd-run" <<'EOF'
#!/usr/bin/env bash
echo "systemd-run $*" >>"$FAKE/calls"
EOF

# A skill's call: recorded, and the behaviour's line follows.
cat >"$T/bin/fake-call" <<'EOF'
#!/usr/bin/env bash
echo "call $1 $2" >>"$FAKE/calls"
if [[ -e $FAKE/answer ]]; then cat "$FAKE/answer" >>"$FAKE/journal"; rm -f "$FAKE/answer"; fi
# As lamp-bus's pub: a second line at the call whose arguments hold match.
if [[ -e $FAKE/answer2 ]] && [[ $* == *"$(cut -f1 "$FAKE/answer2")"* ]]; then
  cut -f2- "$FAKE/answer2" >>"$FAKE/journal"; rm -f "$FAKE/answer2"
fi
# A part's socket other than lamp-skills' answers as root's helper does;
# $FAKE/send-refused: it refuses the next line, once.
if [[ $1 == */call.sock ]]; then
  echo '{"ok":true}'
elif [[ -e $FAKE/send-refused ]]; then
  rm -f "$FAKE/send-refused"; echo '{"refused":"no"}'
else
  echo '{"done":{}}'
fi
EOF
chmod +x "$T/bin/"*

export TRIALS_THEN_WAIT=0
export PATH="$T/bin:$PATH" FAKE=$T TRIALS_UNIT_DIR=$T/units TRIALS_ASIDE_DIR=$T/run TRIALS_PROC=$T/proc TRIALS_SUDO= \
  LAMP_BUS_BIN=$T/bin/lamp-bus TRIALS_CALL=$T/bin/fake-call TRIALS_WAIT=2 TRIALS_ETC=$T/etc
# The lamp's settings, for the settings row: a person's name with a quote in
# it, and the zone.
mkdir -p "$T/etc/lampOS/person"
printf 'Saved "Name"\n' >"$T/etc/lampOS/person/name"
echo Asia/Ho_Chi_Minh >"$T/etc/timezone"

pass=0 fail=0
ok() { pass=$((pass + 1)); }
bad() { fail=$((fail + 1)); echo "FAIL: $*"; }
check() { if [[ $2 == "$3" ]]; then ok; else bad "$1 (expected exit $2, got $3)"; sed 's/^/    | /' "$T/out"; fi; }
has() { if grep -qF -- "$3" "$2"; then ok; else bad "$1 (no '$3' in $2)"; sed 's/^/    | /' "$2"; fi; }
hasnt() { if grep -qF -- "$3" "$2"; then bad "$1 ('$3' in $2)"; else ok; fi; }
no_dropin() { if ls "$T/units"/*.d/zz-trial.conf >/dev/null 2>&1; then bad "$1: a drop-in is left"; else ok; fi; }
reset() {
  rm -rf "$T/units"/* "$T/calls" "$T/journal" "$T/answer" "$T/env-ignored" "$T/restarts" \
    "$T/restarts-before-failing" "$T/bus" "$T/state"/* "$T/pub-fails" "$T/skills"/* "$T/answer2" \
    "$T/memory-sent" "$T/eyes" "$T/answer-all" "$T"/bus-[0-9]* "$T/marked" \
    "$T/send-refused" "$T/units"/lamp-setup-change* "$T/out"
  mkdir -p "$T/bus"
  # Mood isn't running on the stand-in lamp unless a test says so.
  echo inactive >"$T/state/lamp-mood.service"
  echo inactive >"$T/state/lamp-trial-mood.service"
  : >"$T/calls"
}
dry() { "$SCRIPT" "$1" --dry; }
# NAME COMMAND...: the command's lines into the array NAME (no mapfile: a
# Mac's bash is 3.2).
lines_into() {
  local name=$1 line
  shift
  eval "$name=()"
  while IFS= read -r line; do eval "$name+=(\"\$line\")"; done < <("$@")
}
field() { dry "$1" | sed -n "s/^  $2: //p"; }
in_of() { dry "$1" | sed -n 's/^  proof in \([^,]*\),.*/\1/p'; }
proofs_of() { dry "$1" | sed -n 's/^  proof in .*: \/\(.*\)\/$/\1/p'; }
absents_of() { dry "$1" | sed -n 's/^  absent in [^:]*: \/\(.*\)\/$/\1/p'; }
# Waits until the trial has taken its mark, however long its set-up ran: a
# line timed from the start lands before the mark when a set-up runs past it
# (the reviewer, 05:22 on #1722).
after_mark() { for _ in $(seq 1 400); do [[ -e $T/marked ]] && break; sleep 0.05; done; }
# Makes the row's proof come: its line in the journal at its pub or call, or a
# second after the trial takes its mark; or its message on the bus.
answer() { # ROW
  local in example
  if [[ $(in_of "$1" | wc -l) -gt 1 ]]; then
    # Several proofs: each example where its proof looks, a second in.
    local ins examples n
    lines_into ins in_of "$1"
    lines_into examples field "$1" example
    : >"$T/answer-all"
    for n in "${!ins[@]}"; do
      if [[ ${ins[$n]} == bus:* ]]; then
        printf '%s\n' "${examples[$n]}" >"$T/bus-$n"
        (sleep 1; cp "$T/bus-$n" "$T/bus/$(printf '%s' "${ins[$n]#bus:}" | tr / _)") &
      else
        printf '%s\t%s\n' "${ins[$n]}" "${examples[$n]}" >>"$T/answer-all"
      fi
    done
    # Once the trial has taken its mark.
    if [[ -s $T/answer-all ]]; then
      (after_mark; cat "$T/answer-all" >>"$T/journal") &
    fi
    return
  fi
  in=$(in_of "$1" | awk '{print $1}')
  example=$(field "$1" example)
  if [[ $in == bus:* ]]; then
    local topic=${in#bus:}
    (sleep 1; printf '%s\n' "$example" >"$T/bus/${topic//\//_}") &
  elif [[ -n $(field "$1" "first example") ]]; then
    printf '%s\t%s\n' "$in" "$(field "$1" "first example")" >"$T/answer"
    local then
    then=$(dry "$1" | sed -n 's/^  then: pub [^ ]* [^ ]* \([^ ]*\) .*/\1/p' | tail -1)
    printf '%s\t%s\t%s\n' "$then" "$in" "$example" >"$T/answer2"
  else
    printf '%s\t%s\n' "$in" "$example" >"$T/answer"
    if ! dry "$1" | grep -qE '^  cause: (pub|keep|call) '; then
      (after_mark; sleep 1; cat "$T/answer" >>"$T/journal") &
    fi
  fi
}

# Usage, and a row that isn't there.
"$SCRIPT" >"$T/out" 2>&1; check "no row: usage" 2 $?
"$SCRIPT" nonsense >"$T/out" 2>&1; check "unknown row" 2 $?
"$SCRIPT" list >"$T/out" 2>&1; check "list" 0 $?
has "list says what isn't here" "$T/out" "Not here:"

ROWS=$("$SCRIPT" list | sed '/^$/,$d' | awk '{print $1}')
[[ $(echo "$ROWS" | wc -w) -ge 5 ]] && ok || bad "list shows fewer than five rows: $ROWS"

for row in $ROWS; do
  reset
  # --dry changes nothing.
  dry "$row" >"$T/out" 2>&1; check "$row --dry" 0 $?
  [[ -s $T/calls ]] && bad "$row --dry called: $(cat "$T/calls")" || ok
  has "$row --dry names a switch" "$T/out" "switch: "
  # Each row's proofs match their examples, the lines as the code writes
  # them; what must not come matches the line that would fail it, and none
  # of the row's own examples.
  lines_into proofs proofs_of "$row"
  lines_into examples field "$row" example
  [[ ${#proofs[@]} -ge 1 && ${#proofs[@]} == "${#examples[@]}" ]] && ok ||
    bad "$row: ${#proofs[@]} proofs, ${#examples[@]} examples"
  for n in "${!proofs[@]}"; do
    if [[ -n ${examples[$n]:-} ]] && printf '%s\n' "${examples[$n]}" | grep -qE -- "${proofs[$n]}"; then ok; else
      bad "$row: /${proofs[$n]}/ doesn't match its example '${examples[$n]:-}'"
    fi
  done
  lines_into absents absents_of "$row"
  lines_into absent_examples field "$row" "absent example"
  for n in "${!absents[@]}"; do
    if printf '%s\n' "${absent_examples[$n]:-}" | grep -qE -- "${absents[$n]}"; then ok; else
      bad "$row: absent /${absents[$n]}/ doesn't match its example '${absent_examples[$n]:-}'"
    fi
    for example in "${examples[@]}"; do
      if printf '%s\n' "$example" | grep -qE -- "${absents[$n]}"; then bad "$row: its own example '$example' is absent /${absents[$n]}/"; else ok; fi
    done
  done

  # A pass: set up, the switches on and carried, the cause, the proof; then
  # put back.
  reset
  answer "$row"
  "$SCRIPT" "$row" >"$T/out" 2>&1; check "$row passes" 0 $?
  wait
  has "$row: PASS said" "$T/out" "PASS: $row"
  has "$row: a switch is on" "$T/out" "on: "
  has "$row: put back said" "$T/out" "put back: drop-ins removed"
  no_dropin "$row"
  [[ $(grep -c '^systemctl restart' "$T/calls") -ge 2 ]] && ok || bad "$row: not restarted twice"
  [[ -z $(ls "$T/skills") ]] && ok || bad "$row: left in the skills folder: $(ls "$T/skills")"

  # No proof: FAIL, and still put back.
  reset
  TRIALS_WAIT=1 "$SCRIPT" "$row" >"$T/out" 2>&1; check "$row without its proof fails" 1 $?
  has "$row: FAIL said" "$T/out" "FAIL: $row: no "
  has "$row: put back after a failure" "$T/out" "put back: drop-ins removed"
  no_dropin "$row after a failure"

  # A process that doesn't carry the switch fails before the cause.
  reset
  touch "$T/env-ignored"
  "$SCRIPT" "$row" >"$T/out" 2>&1; check "$row: switch not carried" 1 $?
  has "$row: says which switch" "$T/out" "doesn't carry"
  hasnt "$row: no cause without the switch" "$T/out" "cause: "
  hasnt "$row: no PLAY without the switch" "$T/out" "PLAY:"
  has "$row: put back after a refused switch" "$T/out" "put back: drop-ins removed"
done

# smoky and dusty pause lamp-air, whose own readings would cross the
# trial's, and start it again at the end; a lamp-air that wasn't running is
# left stopped. The trial waits, then says one ordinary reading before the
# smoke, so the rise has a reading to rise from.
reset
answer smoky
"$SCRIPT" smoky >"$T/out" 2>&1; check "smoky pauses lamp-air" 0 $?
wait
has "smoky: lamp-air stopped" "$T/calls" "systemctl stop lamp-air.service"
has "smoky: lamp-air started again" "$T/calls" "systemctl start lamp-air.service"
has "smoky: said started again" "$T/out" "put back: lamp-air.service started again"
[[ $(grep -n 'pm2_5":40.0' "$T/calls" | head -1 | cut -d: -f1) -lt $(grep -n 'pm2_5":300.0' "$T/calls" | head -1 | cut -d: -f1) ]] && ok ||
  bad "smoky: the ordinary reading doesn't come before the smoke"
reset
echo inactive >"$T/state/lamp-air.service"
TRIALS_WAIT=1 "$SCRIPT" dusty >"$T/out" 2>&1; check "dusty with lamp-air not running fails without its proof" 1 $?
has "dusty: lamp-air left as it is" "$T/out" "pause: lamp-air.service (not running: left as it is)"
hasnt "dusty: lamp-air not stopped" "$T/calls" "systemctl stop lamp-air.service"
hasnt "dusty: lamp-air not started" "$T/calls" "systemctl start lamp-air.service"

# settings installs root's helper's two units for the trial (the socket's
# group root), saves the old name and zone, sends the two changes, then sends
# the saved lines back, stops the helper and removes the units; a change
# refused fails the trial, and the put-back still runs and removes them.
reset
answer settings
"$SCRIPT" settings >"$T/out" 2>&1; check "settings passes" 0 $?
wait
has "settings: the name sent" "$T/calls" 'call /run/lampos-setup-change/change.sock {"person_name":"Trial"}'
has "settings: the zone sent" "$T/calls" 'call /run/lampos-setup-change/change.sock {"timezone":"Asia/Bangkok"}'
has "settings: the old zone sent back" "$T/calls" 'call /run/lampos-setup-change/change.sock {"timezone":"Asia/Ho_Chi_Minh"}'
has "settings: the old name sent back, escaped" "$T/calls" 'call /run/lampos-setup-change/change.sock {"person_name":"Saved \"Name\""}'
hasnt "settings: the saved name never printed" "$T/out" "Saved"
has "settings: the helper stopped" "$T/calls" "systemctl stop lamp-setup-change.socket"
ls "$T/units"/lamp-setup-change* >/dev/null 2>&1 && bad "settings: the units are left" || ok
reset
touch "$T/send-refused"
"$SCRIPT" settings >"$T/out" 2>&1; check "settings refused" 1 $?
has "settings: says refused" "$T/out" "refused it"
ls "$T/units"/lamp-setup-change* >/dev/null 2>&1 && bad "settings: the units are left after a refusal" || ok

# guard: a text asked for from the empty room fails the trial before it is
# turned off, and it is still put back.
reset
answer guard
printf '%s\t%s\n' lamp-skills.service "lamp-skills: guard-mode Done: text: sent on messages/out: Someone came to your desk." >"$T/journal"
"$SCRIPT" guard >"$T/out" 2>&1; check "guard with a text from the empty room fails" 1 $?
wait
has "guard: says the check failed" "$T/out" "FAIL: the then failed"
hasnt "guard: not turned off after the failure" "$T/calls" '"turn":"off"'
has "guard: put back after it" "$T/out" "put back: drop-ins removed"

# light-out: --zone added to lamp-skills' own command line and carried,
# nobody_s kept up every second until the end, then someone said back; an
# earlier copy of the skill put back as it was.
reset
mkdir -p "$T/skills/light-out"
echo "the lamp's own" >"$T/skills/light-out/skill.toml"
cp "$T/skills/light-out/skill.toml" "$T/before.toml"
# Triggered once nobody_s has been said twice, whatever the setup took.
(for _ in $(seq 1 100); do [[ $(grep -c 'someone":false' "$T/calls" 2>/dev/null) -ge 2 ]] && break; sleep 0.1; done
 printf '%s\t%s\n' lamp-skills.service "$(field light-out "first example")" >>"$T/journal") &
printf '%s\t%s\t%s\n' '"someone":true' lamp-skills.service "$(field light-out example)" >"$T/answer2"
TRIALS_WAIT=10 "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out with an earlier copy" 0 $?
wait
has "light-out: --zone carried" "$T/out" "on: lamp-skills.service has +--zone system"
[[ $(grep -c 'lamp-bus pub room/presence latest {"someone":false' "$T/calls") -ge 2 ]] && ok ||
  bad "light-out: nobody_s not kept up: $(grep -c 'someone":false' "$T/calls")"
has "light-out: someone said back after" "$T/calls" 'lamp-bus pub room/presence latest {"someone":true,"nobody_s":0}'
cmp -s "$T/skills/light-out/skill.toml" "$T/before.toml" && ok || bad "light-out: the lamp's own copy not put back"
after=$(grep -c 'someone":false' "$T/calls"); sleep 2
[[ $(grep -c 'someone":false' "$T/calls") == "$after" ]] && ok || bad "light-out: still publishing after the end"
# light-out holds while nobody is there and says Done only when its trigger
# ends (the first builder, #1722, 20 Oct 00:28): no Done until someone is
# said back, after Triggered; the trial says so, and Done passes.
reset
printf '%s\t%s\n' lamp-skills.service "$(field light-out "first example")" >"$T/answer"
printf '%s\t%s\t%s\n' '"someone":true' lamp-skills.service "$(field light-out example)" >"$T/answer2"
"$SCRIPT" light-out >"$T/out" 2>&1; check "light-out: Done after someone is back" 0 $?
has "light-out: began said" "$T/out" "began: lamp-skills: light-out Triggered"
first=$(grep -n 'began: ' "$T/out" | cut -d: -f1); back=$(grep -n 'then: lamp-bus pub room/presence latest {"someone":true' "$T/out" | cut -d: -f1); at_pass=$(grep -n "PASS: light-out" "$T/out" | cut -d: -f1)
[[ -n $first && -n $back && -n $at_pass && $first -lt $back && $back -lt $at_pass ]] && ok || bad "light-out: not Triggered, then someone back, then PASS: $first $back $at_pass"
# Triggered but never Done: FAIL, said.
reset
printf '%s\t%s\n' lamp-skills.service "$(field light-out "first example")" >"$T/answer"
TRIALS_WAIT=1 "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out: Triggered, no Done" 1 $?
has "light-out: no Done said" "$T/out" "FAIL: light-out: no line matching /light-out Done"
# Never Triggered: FAIL on the first line, and someone isn't said back early.
reset
TRIALS_WAIT=1 "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out: never Triggered" 1 $?
has "light-out: no Triggered said" "$T/out" "FAIL: light-out: no line matching /light-out Triggered/"
hasnt "light-out: no then-step without Triggered" "$T/out" "then: "

# The copy the trial ran: no [quiet], its trigger as the build's; gone after.
reset
# Read once the trial says a switch is on, after its setup is done.
(for _ in $(seq 1 100); do grep -q '^.* on: ' "$T/out" 2>/dev/null && break; sleep 0.05; done
 cp "$T/skills/light-out/skill.toml" "$T/trial.toml") &
TRIALS_WAIT=2 "$SCRIPT" light-out >"$T/out" 2>&1
wait
if [[ -e $T/trial.toml ]]; then
  hasnt "light-out: the trial's copy has no [quiet]" "$T/trial.toml" "[quiet]"
  has "light-out: the trial's copy keeps its trigger" "$T/trial.toml" "above = 599"
else
  bad "light-out: no trial copy seen"
fi
[[ ! -e $T/skills/light-out ]] && ok || bad "light-out: the trial's copy left behind"
# A lamp whose lamp-skills has a --zone already gets no second one.
reset
sed -i.bak 's|--dir |--zone Asia/Ho_Chi_Minh --dir |' "$T/argv"
TRIALS_WAIT=1 "$SCRIPT" light-out >"$T/out" 2>&1
has "light-out: an existing --zone kept" "$T/out" "doesn't carry +--zone system"
mv "$T/argv.bak" "$T/argv"

# The build each switched unit runs is said, from its process: the program's
# path and its SHA-256's first 12 hex digits.
reset
answer scenes
"$SCRIPT" scenes >"$T/out" 2>&1; check "scenes with its build said" 0 $?
wait
want=$( (command -v sha256sum >/dev/null && sha256sum "$T/builds/lamp-skills.service" || shasum -a 256 "$T/builds/lamp-skills.service") | cut -c1-12)
has "the build said, path and sha256" "$T/out" "build: lamp-skills.service runs $T/builds/lamp-skills.service (sha256 $want)"

# nudges: the trial's copy of rest-your-eyes waits 30 s and has no hours;
# a near face kept up, then none said after; gone after.
reset
(for _ in $(seq 1 100); do grep -q '^.* on: ' "$T/out" 2>/dev/null && break; sleep 0.05; done
 cp "$T/skills/rest-your-eyes/skill.toml" "$T/nudge.toml") &
TRIALS_WAIT=2 "$SCRIPT" nudges >"$T/out" 2>&1
wait
if [[ -e $T/nudge.toml ]]; then
  has "nudges: the trial's copy waits 30 s" "$T/nudge.toml" 'for = "30s"'
  hasnt "nudges: no 45 minutes" "$T/nudge.toml" 'for = "45m"'
  hasnt "nudges: no [active] hours" "$T/nudge.toml" "[active]"
  hasnt "nudges: no [quiet] hours" "$T/nudge.toml" "[quiet]"
  has "nudges: its trigger kept" "$T/nudge.toml" 'topic = "humans/faces/tracked"'
else
  bad "nudges: no trial copy seen"
fi
has "nudges: a face kept up" "$T/calls" 'lamp-bus pub humans/faces/tracked latest {"faces":[{"id":"trial"'
has "nudges: no face said after" "$T/calls" 'lamp-bus pub humans/faces/tracked latest {"faces":[]}'
[[ ! -e $T/skills/rest-your-eyes ]] && ok || bad "nudges: the trial's copy left behind"

# scenes: the call's line, and its answer printed.
reset
answer scenes
"$SCRIPT" scenes >"$T/out" 2>&1; check "scenes passes" 0 $?
wait
has "scenes: the call's line" "$T/calls" '{"op":"call","name":"light_scene","args":{"scene":"movie"}}'
has "scenes: the answer printed" "$T/out" 'answered: {"ok":true}'

# forget: the memory store set aside with lamp-memory stopped, and put back
# exactly, whatever the trial did to it.
reset
echo "kept" >"$T/memory/days.json"
# Changed once set aside: after the mark (Tests' P3 on #2074).
(after_mark; sleep 1; echo "changed" >"$T/memory/days.json"; echo "new" >"$T/memory/new.json") &
TRIALS_WAIT=2 "$SCRIPT" forget >"$T/out" 2>&1; check "forget without its line" 1 $?
wait
[[ $(cat "$T/memory/days.json") == kept && ! -e $T/memory/new.json ]] && ok || bad "forget: the memory store not put back"
has "forget: lamp-memory stopped for the copy" "$T/calls" "systemctl stop lamp-memory.service"
has "forget: the lines to play" "$T/out" 'PLAY: "Forget that."'

# thread: a made-up thread seeded over lamp-memory's socket (an exchange,
# its conversation's end with the exchange's id, the thread with the job
# and the id, for the made-up person), the eyes paused, the person's name
# kept fresh in the known file while the line is played, and the file gone
# after.
reset
answer thread
# Read once the trial has written it, however long its set-up took (Tests'
# P2 on #2029): not after a fixed time.
(for _ in $(seq 1 400); do [[ -s $T/eyes/known ]] && break; sleep 0.05; done
 cat "$T/eyes/known" >"$T/known-seen" 2>/dev/null) &
"$SCRIPT" thread >"$T/out" 2>&1; check "thread passes" 0 $?
wait
person=p-$(printf %s "trial person" | sha256sum | cut -c1-16)
[[ $(grep -c . "$T/memory-sent" 2>/dev/null) == 3 ]] && ok || bad "thread: not three lines to the memory"
has "thread: the exchange, the person's" "$T/memory-sent" "{\"op\":\"append\",\"person\":\"$person\","
has "thread: the conversation's end cites the exchange" "$T/memory-sent" '"source_ids":[7]}'
has "thread: the thread with the job" "$T/memory-sent" '"job":"j3","observations":[{'
has "thread: a thread, citing the exchange" "$T/memory-sent" '"category":"thread","source_ids":[7]'
has "thread: lamp-eyes paused" "$T/calls" "systemctl stop lamp-eyes.service"
has "thread: lamp-eyes started again" "$T/calls" "systemctl start lamp-eyes.service"
[[ $(cat "$T/known-seen" 2>/dev/null) == "Trial Person" ]] && ok || bad "thread: the known file wasn't kept: $(cat "$T/known-seen" 2>/dev/null)"
[[ ! -e $T/eyes/known ]] && ok || bad "thread: the known file was left"
has "thread: the line to play" "$T/out" 'PLAY: "Hey Lamp, good morning."'
# A memory that doesn't answer: the setup fails, and all is put back.
reset
mv "$T/bin/socat" "$T/bin/socat.off"
printf '#!/usr/bin/env bash\nexit 0\n' >"$T/bin/socat"; chmod +x "$T/bin/socat"
"$SCRIPT" thread >"$T/out" 2>&1; check "thread with no memory answering" 3 $?
mv "$T/bin/socat.off" "$T/bin/socat"
has "thread: the setup's failure said" "$T/out" "the setup failed"
no_dropin "thread after a failed setup"

# The memory store's copy is kept in a 0700 folder under the set-aside
# place, and gone after: even when the runner can't remove it alone, as
# the person's memories under lamp-memory's own 0700 folder.
reset
rm -rf "$T/memory"; mkdir -p "$T/memory/private"
echo "theirs" >"$T/memory/private/days.json"
chmod 0500 "$T/memory/private"
# The folder's mode, read once it is there, whatever the setup took.
(for _ in $(seq 1 100); do ls -d "$T/run"/lampos-trial-* >/dev/null 2>&1 && break; sleep 0.05; done
 for d in "$T/run"/lampos-trial-*; do stat -f '%Lp' "$d" 2>/dev/null || stat -c '%a' "$d"; done >"$T/aside-mode") &
TRIALS_SUDO=$T/bin/fake-sudo TRIALS_WAIT=2 "$SCRIPT" forget >"$T/out" 2>&1; check "forget with a folder the runner can't write" 1 $?
wait
[[ $(cat "$T/aside-mode" 2>/dev/null) == 700 ]] && ok || bad "the set-aside folder wasn't 0700: $(cat "$T/aside-mode" 2>/dev/null)"
[[ -z $(ls -A "$T/run") ]] && ok || bad "a set-aside copy was left: $(ls -A "$T/run")"
[[ $(cat "$T/memory/private/days.json" 2>/dev/null) == theirs ]] && ok || bad "the memory store not put back"
chmod -R u+w "$T/memory"

# A unit that wasn't running is stopped again, not left running.
reset
echo inactive >"$T/state/lamp-cloud.service"
answer reports
"$SCRIPT" reports >"$T/out" 2>&1; check "reports with lamp-cloud stopped before" 0 $?
wait
has "reports: lamp-cloud stopped again" "$T/out" "lamp-cloud.service stopped again, as it was"
[[ $(cat "$T/state/lamp-cloud.service") == inactive ]] && ok || bad "reports: lamp-cloud left running"

# No fresh message on the bus is no pass.
reset
TRIALS_WAIT=1 "$SCRIPT" activity >"$T/out" 2>&1; check "activity with no fresh message" 1 $?

# A failed bus cause is said, and put back.
reset
touch "$T/pub-fails"
TRIALS_WAIT=1 "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out: a failed pub" 1 $?
has "a failed pub is said" "$T/out" "FAIL: the cause: lamp-bus pub room/presence failed"
has "put back after a failed pub" "$T/out" "put back: drop-ins removed"
# A failed after-step leaves the put back incomplete: said, exit 3.
reset
printf '%s\t%s\n' lamp-skills.service "$(field light-out example)" >"$T/answer"
# A lamp-bus whose pub of someone back fails.
cat >"$T/bin/lamp-bus.after" <<'EOF'
#!/usr/bin/env bash
case $1 in
  pub)
    echo "lamp-bus $*" >>"$FAKE/calls"
    [[ $* == *'"someone":true'* ]] && exit 1
    if [[ -e $FAKE/answer ]]; then cat "$FAKE/answer" >>"$FAKE/journal"; rm -f "$FAKE/answer"; fi
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$T/bin/lamp-bus.after"
LAMP_BUS_BIN=$T/bin/lamp-bus.after "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out: a failed after-step" 3 $?
has "a failed after-step is said" "$T/out" "FAIL: the after: lamp-bus pub room/presence failed"
has "a failed after-step: PUT BACK FAILED" "$T/out" "PUT BACK FAILED"
no_dropin "after a failed after-step"

# A unit that won't come back when put back: exit 3, said.
reset
answer forget
echo 1 >"$T/restarts-before-failing"
"$SCRIPT" forget >"$T/out" 2>&1; check "a failed put back" 3 $?
wait
has "a failed put back is said" "$T/out" "PUT BACK FAILED"

# TERM again during the put-back (#2034, 03:40): it still finishes.
reset
TRIALS_WAIT=30 "$SCRIPT" forget >"$T/out" 2>&1 &
pid=$!
for _ in $(seq 1 100); do grep -q "PLAY: " "$T/out" 2>/dev/null && break; sleep 0.1; done
touch "$T/slow-restarts"
kill -TERM $pid
for _ in $(seq 1 50); do grep -q "put back: " "$T/out" 2>/dev/null && break; sleep 0.1; done
sleep 0.3
kill -TERM $pid 2>/dev/null
sleep 0.5
kill -TERM $pid 2>/dev/null
wait $pid
rm -f "$T/slow-restarts"
has "a twice-signalled put-back finishes" "$T/out" "put back: drop-ins removed"
no_dropin "after TERM in the put-back"

# Stopped mid-wait (TERM): still put back.
reset
TRIALS_WAIT=30 "$SCRIPT" forget >"$T/out" 2>&1 &
pid=$!
for _ in 1 2 3 4 5 6 7 8 9 10; do grep -q "PLAY: " "$T/out" 2>/dev/null && break; sleep 0.5; done
kill -TERM $pid
wait $pid
check "stopped mid-wait" 130 $?
has "put back after TERM" "$T/out" "put back: drop-ins removed"
no_dropin "after TERM"

# The mood row: Mood started as a transient unit and stopped after;
# Expression run in shadow, its own command line without --enable, from a
# drop-in removed after; the light read before and after.
reset
printf '{"on":true,"colour":"orange","level":3}\n' >"$T/bus/light_state"
answer mood
"$SCRIPT" mood >"$T/out" 2>&1; check "mood passes" 0 $?
wait
has "mood: Mood started" "$T/calls" "systemd-run --unit=lamp-trial-mood"
has "mood: as Expression's user" "$T/calls" "--uid=$(id -un)"
has "mood: Mood switched on" "$T/calls" "-E LAMP_MOOD=on"
has "mood: Mood stopped after" "$T/calls" "systemctl stop lamp-trial-mood"
has "mood: the light as it was" "$T/out" 'the light as it was: {"on":true,"colour":"orange","level":3}'
grep -lx 'lamp-expression serve --head-touch --bus-root /run/lampos-bus --zone system' \
  <(for f in "$T"/proc/*/cmdline; do tr '\0' ' ' <"$f" | sed 's/^[^ ]*\///; s/ $//'; echo; done) >/dev/null && ok ||
  bad "mood: Expression never ran without --enable"
[[ -e $T/units/lamp-expression.service.d/zz-trial-shadow.conf ]] && bad "mood: the shadow drop-in is left" || ok
# Run once more, Expression is back on its own line, with --enable.
tr '\0' ' ' <"$T/proc/$(cat "$T/pids/lamp-expression.service")/cmdline" | grep -q -- "--enable" && ok ||
  bad "mood: Expression not put back with --enable"

# Someone known: the familiar level is said, and the row fails with the
# message that says it.
reset
answer mood
# The message mood/state's proof will be, a familiar level in it (the third
# proof's: answer() copies $T/bus-2 there).
field mood "absent example" >"$T/bus-2"
"$SCRIPT" mood >"$T/out" 2>&1; check "mood fails on a familiar level" 1 $?
wait
has "mood: says what it must not" "$T/out" "said what it must not"
has "mood: the line itself" "$T/out" '"familiar":"regular"'
has "mood: put back after" "$T/out" "put back: drop-ins removed"

# The light not as it was after: the put back says so and fails.
reset
printf '{"on":true,"colour":"orange","level":3}\n' >"$T/bus/light_state"
answer mood
# The light changes once the setup has read it: when the cause begins.
(
  for _ in $(seq 100); do grep -qE '(cause|PLAY):' "$T/out" 2>/dev/null && break; sleep 0.1; done
  printf '{"on":false,"colour":"orange","level":3}\n' >"$T/bus/light_state"
) &
"$SCRIPT" mood >"$T/out" 2>&1; check "mood: a changed light fails the put back" 3 $?
wait
has "mood: says the light changed" "$T/out" "the light changed"

# Mood already running: refused before anything changes, nothing started.
reset
echo active >"$T/state/lamp-mood.service"
"$SCRIPT" mood >"$T/out" 2>&1; check "mood refused with Mood running" 3 $?
has "mood: says Mood already runs" "$T/out" "Mood already runs (lamp-mood.service): nothing done"
hasnt "mood: no second Mood" "$T/calls" "systemd-run"
[[ -e $T/units/lamp-expression.service.d/zz-trial-shadow.conf ]] && bad "mood: a shadow drop-in written though refused" || ok
no_dropin "mood refused"

echo "trials-test: $pass passed, $fail failed"
[[ $fail == 0 ]]
