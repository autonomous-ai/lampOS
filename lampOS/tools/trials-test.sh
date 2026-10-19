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
EOF

cat >"$T/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "systemctl $*" >>"$FAKE/calls"
argv_of() { # the last ExecStart= of the unit's drop-ins, else its own
  local line=
  for conf in "$TRIALS_UNIT_DIR/$1.d"/*.conf; do
    [[ -e $conf ]] || continue
    line=$(sed -n 's/^ExecStart=\(..*\)/\1/p' "$conf" | tail -1)
  done
  [[ -n $line ]] && { printf '%s\n' "${line//%%/%}"; return; }
  sed -n "s|^$1 ||p" "$FAKE/argv"
}
case $1 in
  daemon-reload) ;;
  restart | start)
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
    ;;
  hz) [[ -e $file ]] ;;
  latest) cat "$file" 2>/dev/null ;;
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

# A skill's call: recorded, and the behaviour's line follows.
cat >"$T/bin/fake-call" <<'EOF'
#!/usr/bin/env bash
echo "call $1 $2" >>"$FAKE/calls"
if [[ -e $FAKE/answer ]]; then cat "$FAKE/answer" >>"$FAKE/journal"; rm -f "$FAKE/answer"; fi
echo '{"ok":true}'
EOF
chmod +x "$T/bin/"*

export PATH="$T/bin:$PATH" FAKE=$T TRIALS_UNIT_DIR=$T/units TRIALS_ASIDE_DIR=$T/run TRIALS_PROC=$T/proc TRIALS_SUDO= \
  LAMP_BUS_BIN=$T/bin/lamp-bus TRIALS_CALL=$T/bin/fake-call TRIALS_WAIT=2

pass=0 fail=0
ok() { pass=$((pass + 1)); }
bad() { fail=$((fail + 1)); echo "FAIL: $*"; }
check() { if [[ $2 == "$3" ]]; then ok; else bad "$1 (expected exit $2, got $3)"; sed 's/^/    | /' "$T/out"; fi; }
has() { if grep -qF -- "$3" "$2"; then ok; else bad "$1 (no '$3' in $2)"; sed 's/^/    | /' "$2"; fi; }
hasnt() { if grep -qF -- "$3" "$2"; then bad "$1 ('$3' in $2)"; else ok; fi; }
no_dropin() { if ls "$T/units"/*.d/zz-trial.conf >/dev/null 2>&1; then bad "$1: a drop-in is left"; else ok; fi; }
reset() {
  rm -rf "$T/units"/* "$T/calls" "$T/journal" "$T/answer" "$T/env-ignored" "$T/restarts" \
    "$T/restarts-before-failing" "$T/bus" "$T/state"/* "$T/pub-fails" "$T/skills"/*
  mkdir -p "$T/bus"
  : >"$T/calls"
}
dry() { "$SCRIPT" "$1" --dry; }
field() { dry "$1" | sed -n "s/^  $2: //p"; }
in_of() { dry "$1" | sed -n 's/^  proof in \([^,]*\),.*/\1/p'; }
# Makes the row's proof come: its line in the journal at its pub or call, or a
# second after the trial begins; or its message on the bus.
answer() { # ROW
  local in example
  in=$(in_of "$1" | awk '{print $1}')
  example=$(field "$1" example)
  if [[ $in == bus:* ]]; then
    local topic=${in#bus:}
    (sleep 1; printf '%s\n' "$example" >"$T/bus/${topic//\//_}") &
  else
    printf '%s\t%s\n' "$in" "$example" >"$T/answer"
    if ! dry "$1" | grep -qE '^  cause: (pub|keep|call) '; then
      (sleep 1; cat "$T/answer" >>"$T/journal") &
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
  # Each row's proof matches its example, the line as the code writes it.
  proof=$(dry "$row" | sed -n 's/^  proof in .*: \/\(.*\)\/$/\1/p')
  example=$(field "$row" example)
  if [[ -n $example ]] && printf '%s\n' "$example" | grep -qE -- "$proof"; then ok; else bad "$row: /$proof/ doesn't match its example '$example'"; fi

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

# light-out: --zone added to lamp-skills' own command line and carried,
# nobody_s kept up every second until the end, then someone said back; an
# earlier copy of the skill put back as it was.
reset
mkdir -p "$T/skills/light-out"
echo "the lamp's own" >"$T/skills/light-out/skill.toml"
cp "$T/skills/light-out/skill.toml" "$T/before.toml"
(sleep 2; printf '%s\t%s\n' lamp-skills.service "$(field light-out example)" >>"$T/journal") &
TRIALS_WAIT=10 "$SCRIPT" light-out >"$T/out" 2>&1; check "light-out with an earlier copy" 0 $?
wait
has "light-out: --zone carried" "$T/out" "on: lamp-skills.service has +--zone system"
[[ $(grep -c 'lamp-bus pub room/presence latest {"someone":false' "$T/calls") -ge 2 ]] && ok ||
  bad "light-out: nobody_s not kept up: $(grep -c 'someone":false' "$T/calls")"
has "light-out: someone said back after" "$T/calls" 'lamp-bus pub room/presence latest {"someone":true,"nobody_s":0}'
cmp -s "$T/skills/light-out/skill.toml" "$T/before.toml" && ok || bad "light-out: the lamp's own copy not put back"
after=$(grep -c 'someone":false' "$T/calls"); sleep 2
[[ $(grep -c 'someone":false' "$T/calls") == "$after" ]] && ok || bad "light-out: still publishing after the end"
# The copy the trial ran: no [quiet], its trigger as the build's; gone after.
reset
(sleep 1; cp "$T/skills/light-out/skill.toml" "$T/trial.toml") &
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

# nudges: the trial's copy of rest-your-eyes waits 30 s and has no hours;
# a near face kept up, then none said after; gone after.
reset
(sleep 1; cp "$T/skills/rest-your-eyes/skill.toml" "$T/nudge.toml") &
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
(sleep 1; echo "changed" >"$T/memory/days.json"; echo "new" >"$T/memory/new.json") &
TRIALS_WAIT=2 "$SCRIPT" forget >"$T/out" 2>&1; check "forget without its line" 1 $?
wait
[[ $(cat "$T/memory/days.json") == kept && ! -e $T/memory/new.json ]] && ok || bad "forget: the memory store not put back"
has "forget: lamp-memory stopped for the copy" "$T/calls" "systemctl stop lamp-memory.service"
has "forget: the lines to play" "$T/out" 'PLAY: "Forget that."'

# The memory store's copy is kept in a 0700 folder under the set-aside
# place, and gone after: even when the runner can't remove it alone, as
# the person's memories under lamp-memory's own 0700 folder.
reset
rm -rf "$T/memory"; mkdir -p "$T/memory/private"
echo "theirs" >"$T/memory/private/days.json"
chmod 0500 "$T/memory/private"
(sleep 1; for d in "$T/run"/lampos-trial-*; do stat -f '%Lp' "$d" 2>/dev/null || stat -c '%a' "$d"; done >"$T/aside-mode") &
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

echo "trials-test: $pass passed, $fail failed"
[[ $fail == 0 ]]
