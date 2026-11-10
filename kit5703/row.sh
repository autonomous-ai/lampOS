#!/bin/sh
# Kit 5703: one power-up row, played by a person at the lamp. As root, after setup.sh:
#   sh /root/kit5703/row.sh 1a          # drooped head, arm high, elbow past its stand: folds, stands
#   sh /root/kit5703/row.sh 1b          # a nearly folded arm: folds, stands
#   sh /root/kit5703/row.sh 2           # a fold that would tip: waits for a hand, then stands
#   sh /root/kit5703/row.sh 1a power    # the same pose, with a real power cut (lamp-setup shutdown)
#   sh /root/kit5703/row.sh 1a after    # after that power cut, once the lamp has booted
# The default stand-in for a power cut: lamp-body stopped (a standing lamp folds and holds), then the
# product's park (lamp-body-measure let-go: torque off on all five, only for a lamp that is down),
# the pose set by hand with the servos limp, then lamp-body started. lamp-body then finds every joint
# torque-off where a hand left it, as at power-up (RUN.md, "Why the stand-in counts").
# Never stops lamp-update, never takes its lock; refuses while an update round runs.
. /root/kit5703/common.sh
ROW=${1:-}
MODE=${2:-letgo}
[ "$(id -u)" = 0 ] || { echo "not root"; exit 1; }
[ -e $B/setup.done ] || { echo "setup.sh has not finished: no $B/setup.done"; exit 1; }
[ ! -e $B/putback.done ] || { echo "already put back"; exit 1; }
# The poses, in lamp-52e6's counts (El's 572-start sweep on #5703 used 52e6's poses):
# yaw shoulder elbow roll pitch | expect | plain words
case "$ROW" in
  1a) P="2035 2593 2899 2035 2420"; EXPECT="stand"; SIM="about 103 s to READY in the simulator (El's sweep: 108.7 s, the slowest)"
      WORDS="Arm up near its standing height, the elbow bent well past where it stands, the head hanging down hard (a lamp that drooped)";;
  1b) P="2035 2113 2024 2035 1771"; EXPECT="stand"; SIM="about 4 s to READY in the simulator"
      WORDS="Nearly folded: the lamp at rest, but the shoulder lifted a little (about 5 degrees) and the elbow pushed a little past its rest (about 4 degrees); head at home";;
  2)  P="2035 2833 2524 2035 2030"; EXPECT="wait"; SIM="READY at once, then it waits for a hand (simulator: 19 mm off its base's axis against 18 mm)"
      WORDS="Arm raised past its standing height (about 15 degrees past), the elbow just short of where it stands, the head tipped down to where it meets the base when folded";;
  *) echo "usage: sh $B/row.sh 1a|1b|2 [letgo|power|after]"; exit 1;;
esac
case "$MODE" in letgo|power|after) ;; *) echo "mode is letgo, power or after"; exit 1;; esac
L=$B/logs/row-$ROW-$MODE-$(date +%H%M%S)
mkdir -p $B/logs
log() { echo "$*"; echo "$*" >> $L.log; }
ask() { printf '%s ' "$1"; read -r ANS; echo "Q: $1 A: $ANS" >> $L.log; }
yes_no() { while :; do ask "$1 [y/n]"; case "$ANS" in y|Y) return 0;; n|N) return 1;; esac; done; }

# ---- this lamp's targets: 52e6's pose moved onto this lamp's own home, folded arm and stand ----
H=$(pose_arg home); F=$(pose_arg folded-arm); S=$(pose_arg stand)
HP=$(field "$H" 1); HR=$(field "$H" 2); HY=$(field "$H" 3)
FS=$(field "$F" 1); FE=$(field "$F" 2); SS=$(field "$S" 4); SE=$(field "$S" 5)
set -- $P
sgn() { [ "$1" -ge "$2" ] && echo 1 || echo -1; }
TY=$(( HY + $1 - 2035 )); TR=$(( HR + $4 - 2035 )); TP=$(( HP + $5 - 1771 ))
TS=$(( FS + $(sgn $SS $FS) * ($2 - 2053) )); TE=$(( FE + $(sgn $SE $FE) * ($3 - 2069) ))
TARGET="$TY $TS $TE $TR $TP"
log "==== kit5703 row $ROW ($MODE) $(date -Iseconds) on $(uname -n)"
log "pose (lamp-52e6 counts, yaw shoulder elbow roll pitch): $P"
log "this lamp: home $H, folded arm $F, stand $S -> target $TARGET"
log "in words: $WORDS"
log "expected: $SIM"

pre() {
  upd_busy && { log "STOP: lamp-update is busy (a round or a switch): wait and run again"; exit 2; }
  [ "$(links_now)" = "$(cat $B/links.txt)" ] || { log "STOP: a current link moved since setup (an update installed?): put back"; exit 2; }
}
show() {  # $1 = "yaw sh el roll pitch torque=..." ; prints each joint against the target
  echo "$1" | awk -v t="$TARGET" '{ split(t, g, " "); n = split("yaw shoulder elbow roll pitch", nm, " ")
    for (i = 1; i <= 5; i++) { d = $i - g[i]; ok = (d <= 40 && d >= -40) ? "ok" : (d > 0 ? "counts high by " d : "counts low by " (-d))
      if (i == 1 || i == 4) ok = (d <= 40 && d >= -40) ? "ok" : "(leave it)"
      printf "  %-8s now %5s  target %5s  %s\n", nm[i], $i, g[i], ok }
    print "  " $6 }'
}
far() {   # shoulder, elbow or pitch more than 80 from the target
  echo "$1" | awk -v t="$TARGET" '{ split(t, g, " "); f = 0; for (i = 2; i <= 5; i++) if (i != 4) { d = $i - g[i]; if (d > 80 || d < -80) f = 1 } exit !f }'
}

# ---- the start, watched: READY against the start, the journal lines as they come ----
watch_start() {  # $1 = journal --since; sets READY_S, STATE
  i=0; seen=0
  while :; do
    STATE=$(systemctl show -p ActiveState --value lamp-body)
    n=$(body_lines "$1" | wc -l)
    if [ "$n" -gt "$seen" ]; then body_lines "$1" | tail -n $((n - seen)) | cut -c1-260; seen=$n; fi
    case "$STATE" in active|failed|inactive) [ $i -gt 1 ] && break;; esac
    i=$((i+1)); [ $i -gt 200 ] && break
    [ $((i % 10)) = 0 ] && echo "  ... ${i} s, $STATE"
    sleep 1
  done
  READY_S=$(secs "$(mono InactiveExitTimestampMonotonic)" "$(mono ActiveEnterTimestampMonotonic)")
  log "lamp-body: $STATE; READY $READY_S s after its start (systemd's InactiveExit to ActiveEnter, monotonic)"
}

judge() {  # $1 = journal --since
  J=$(body_lines "$1"); echo "$J" > $L.journal
  FOUND=$(echo "$J" | grep -o 'found shoulder [0-9]* elbow [0-9]* head pitch [0-9]*' | head -1)
  FOLD=$(echo "$J" | grep -c 'folds first'); WAITL=$(echo "$J" | grep -c 'waits for a hand')
  NOT=$(echo "$J" | grep -c 'will not stand'); STRAIGHT=$(echo "$J" | grep -c 'set straight: taken hold')
  STOPS=$(stop_lines "$1"); NR=$(systemctl show -p NRestarts --value lamp-body)
  ACTIVE=$(systemctl is-active lamp-body); RESULT=$(systemctl show -p Result --value lamp-body)
  SHA_OK=no; [ "$(body_exe_sha)" = "$STAGED_SHA" ] && SHA_OK=yes
  log "journal: ${FOUND:-no 'found shoulder' line}; folds first x$FOLD; waits for a hand x$WAITL; will not stand x$NOT; set straight x$STRAIGHT"
  [ -n "$STOPS" ] && log "STOP LINES:" && echo "$STOPS" | tee -a $L.log
  log "lamp-body now: $ACTIVE, Result=$RESULT, NRestarts=$NR (setup: $(cat $L.nr 2>/dev/null || echo ?)), staged binary: $SHA_OK"
}

stand_check() {
  echo
  echo "Now make it stand: say \"Lamp, what time is it?\" (or have the Mac say it). A lamp that has"
  echo "started rises to stand while it listens. Keep your hands clear."
  yes_no "Did the lamp rise and stand up (arm up, head up, looking out)?" && STOOD=y || STOOD=n
}
row3_check() {
  echo
  echo "Row 3, your own judgement of everything you saw since the start:"
  yes_no "Did the arm or the head touch the desk, your hand, or anything else on the way?" && TOUCH=y || TOUCH=n
  yes_no "Did it look like a lamp getting up: smooth, unhurried, no jerk, no sweep, not a machine homing?" && LOOK=y || LOOK=n
  ask "Anything else you noticed (one line, Enter for nothing):"; NOTE=$ANS
}
verdict() {
  r1=PASS; r2=-; r3=PASS; why=""
  [ "$SHA_OK" = yes ] || { r1=FAIL; why="$why; not the staged binary"; }
  [ -n "$STOPS" ] && { r1=FAIL; why="$why; a stop line"; }
  [ "$ACTIVE" = active ] || { r1=FAIL; why="$why; lamp-body $ACTIVE"; }
  [ "${NR:-0}" = "$(cat $L.nr 2>/dev/null || echo "${NR:-0}")" ] || { r1=FAIL; why="$why; systemd restarted it"; }
  ok_t=$(awk -v s="$READY_S" 'BEGIN { exit !(s != "?" && s + 0 < 180) }' && echo y || echo n)
  [ $ok_t = y ] || { r1=FAIL; why="$why; READY ${READY_S} s (180 s or more, or none)"; }
  [ "$NOT" = 0 ] || { r1=FAIL; why="$why; 'will not stand'"; }
  if [ $EXPECT = stand ]; then
    [ "$FOLD" -ge 1 ] || { r1=FAIL; why="$why; no 'folds first' line (found already on its way?)"; }
    [ "$WAITL" = 0 ] || { r1=FAIL; why="$why; it waited for a hand"; }
  else
    r2=PASS
    [ "$WAITL" -ge 1 ] || { r2=FAIL; why="$why; no 'waits for a hand' line"; }
    [ "$STRAIGHT" -ge 1 ] || { r2=FAIL; why="$why; no 'set straight' after the hand"; }
    [ "$STILL" = y ] || { r2=FAIL; why="$why; the lamp moved its arm before the hand"; }
    [ "$SPOKE_OK" = y ] || { r2=FAIL; why="$why; spoke/silent not as LAMP_BODY_ASK says"; }
  fi
  [ "$STOOD" = y ] || { [ $EXPECT = wait ] && r2=FAIL || r1=FAIL; why="$why; did not stand"; }
  [ "$TOUCH" = n ] || { r3=FAIL; why="$why; touched something"; }
  [ "$LOOK" = y ] || { r3=FAIL; why="$why; did not look like a lamp getting up"; }
  echo
  log "RESULT row $ROW ($MODE): row1 $r1 | row2 $r2 | row3 $r3 | READY $READY_S s | ${FOUND:-found ?} | stood $STOOD | touch $TOUCH | lamp-like $LOOK${why:+ | why:${why#;}}${NOTE:+ | note: $NOTE}"
  echo "$(date -Iseconds) row $ROW $MODE: row1 $r1 row2 $r2 row3 $r3 READY ${READY_S}s ${FOUND:-?} stood $STOOD touch $TOUCH lamplike $LOOK${why:+ why:${why#;}}${NOTE:+ note: $NOTE}" >> $B/logs/results.txt
  echo "Log: $L.log (journal lines: $L.journal)"
}

hand_wait() {  # row 2: the lamp let its arm go and waits; the person folds it
  VP=$(systemctl show -p MainPID --value lamp-voice)
  ASKON=$(tr '\0' '\n' < /proc/$VP/environ 2>/dev/null | grep -c '^LAMP_BODY_ASK=on$')
  echo
  if [ "$WAITL" -ge 1 ] || body_lines "$1" | grep -q 'waits for a hand'; then
    echo "The lamp has let its arm go and waits for a hand (the journal says so)."
  else
    echo "No 'waits for a hand' line yet. Look at the lamp: if it is folding by itself, that is a FAIL for row 2 (it judged the fold safe)."
  fi
  yes_no "Before you touched it, did the lamp leave its arm alone (still, or only sagging under its own weight)?" && STILL=y || STILL=n
  if [ "$ASKON" -ge 1 ]; then
    yes_no "LAMP_BODY_ASK is on here: did the lamp say \"My arm is not folded all the way down...\"?" && SPOKE_OK=y || SPOKE_OK=n
  else
    yes_no "LAMP_BODY_ASK is off (the product): did the lamp stay quiet about its arm?" && SPOKE_OK=y || SPOKE_OK=n
  fi
  ask "How did the arm drop when it was let go (stayed / sagged slowly / fell)?"; DROP=$ANS
  ask "Did the lamp look at risk of tipping in that pose (base rocking or lifting)? (y/n/unsure)"; TIPRISK=$ANS
  echo
  echo "Now fold the arm gently down to its rest by hand: shoulder and elbow folded (about $FS and $FE),"
  echo "head left as it is. Hold it still for a second, then take both hands away and step back."
  echo "Once it is still for a second it takes hold and starts by itself: the head may lift home slowly."
  ask "Press Enter once your hands are off the lamp."
  i=0; until body_lines "$1" | grep -q 'set straight: taken hold'; do
    i=$((i+1)); [ $i -gt 120 ] && { log "no 'set straight' 120 s after the hands came off"; break; }
    [ $((i % 10)) = 0 ] && echo "  ... ${i} s, still waiting (is the arm fully folded and still?)"
    sleep 1
  done
  body_lines "$1" | grep 'set straight' | tail -1
  sleep 5
  NOTE_DROP="drop: $DROP; tip risk: $TIPRISK"
}

NR0=$(systemctl show -p NRestarts --value lamp-body)
echo "$NR0" > $L.nr
STILL=y; SPOKE_OK=y; STOOD=n; TOUCH=n; LOOK=n; NOTE=""; NOTE_DROP=""

if [ "$MODE" = letgo ]; then
  pre
  [ "$(body_exe_sha)" = "$STAGED_SHA" ] || { log "STOP: lamp-body does not run the staged binary"; exit 2; }
  echo
  echo "STEP 1. The lamp now folds down if it is standing, then goes limp (torque off on all five)."
  echo "        Keep your hands clear until it says it is limp."
  ask "Press Enter to start (Ctrl-C to leave it as it is)."
  TS0=$(date '+%Y-%m-%d %H:%M:%S')
  systemctl stop lamp-body
  i=0; while systemctl is-active -q lamp-body; do i=$((i+1)); [ $i -gt 30 ] && break; sleep 1; done
  log "lamp-body stopped ($(systemctl is-active lamp-body)); its exit report:"
  exit_report_since "$TS0" | head -20 | tee -a $L.log
  sleep 1
  LG=$(let_go 2>&1); echo "$LG" > $L.letgo.json
  if ! echo "$LG" | tr -d ' ' | grep -q '"let_go":true'; then
    loud "NOT LET GO: the lamp is not down (or the park failed). Do not pose it: its servos hold."
    echo "$LG" | cut -c1-400
    echo "Starting lamp-body again as it is."; systemctl reset-failed lamp-body; systemctl start lamp-body
    exit 3
  fi
  NOWJ=$(read_joints); log "limp now: $NOWJ"
  echo
  echo "STEP 2. The lamp is limp. Hold the head with one hand and set the pose with the other:"
  echo "        $WORDS"
  echo "        Move slowly. If the arm will not stay where you put it, let it settle where it rests:"
  echo "        that is what a power cut leaves too."
  while :; do
    NOWJ=$(read_joints); show "$NOWJ"
    ask "Enter = read again, g = posed (go on), q = give up (lamp-body starts as it lies):"
    case "$ANS" in
      g) far "$NOWJ" && { yes_no "Shoulder, elbow or pitch is more than 80 counts from the target. Go on anyway?" || continue; }; break;;
      q) log "given up while posing"; break;;
    esac
  done
  log "posed: $NOWJ"
  echo
  echo "STEP 3. Take both hands off the lamp and step back to arm's length."
  [ $EXPECT = stand ] && echo "        Within a second it holds itself; then it lifts its head clear if it hangs, folds the elbow,
        then the shoulder, one at a time and slowly, then wakes. Expected: $SIM."
  [ $EXPECT = wait ] && echo "        Within a second it holds itself, then lets its arm go again and waits for a hand. Keep a hand
        near the head (not touching) in case the arm drops or the lamp rocks."
  echo "        To stop it at any time: in this terminal, Ctrl-C, then: systemctl stop lamp-body"
  ask "Press Enter when your hands are off and you are ready."
  T1=$(date '+%Y-%m-%d %H:%M:%S')
  systemctl reset-failed lamp-body 2>/dev/null || true    # a stop and start is a start for StartLimitBurst=3
  systemctl start --no-block lamp-body
  sleep 1
  watch_start "$T1"
  judge "$T1"
  [ $EXPECT = wait ] && { hand_wait "$T1"; judge "$T1"; }
  stand_check
  row3_check
  NOTE="${NOTE_DROP:+$NOTE_DROP; }$NOTE"
  judge "$T1"
  verdict
  exit 0
fi

if [ "$MODE" = power ]; then
  pre
  command -v lamp-setup >/dev/null || { log "STOP: no lamp-setup on this lamp: use the stand-in (letgo)"; exit 2; }
  echo
  echo "A REAL POWER CUT. The kit's drop-in and binary are on disk, so lamp-body comes back on the"
  echo "staged binary. The product's safe-off folds the lamp, lets it go and lights the ring green."
  echo "  1. Type 'shutdown' below: the lamp says \"Going to sleep now. Unplug me when my light turns green.\""
  echo "  2. When the ring is green, unplug the power. (Not green: keep it plugged in and tell the builder.)"
  echo "  3. With the power off, set the pose by hand: $WORDS"
  echo "     (counts on this lamp: yaw shoulder elbow roll pitch = $TARGET; no live read while it is off)"
  echo "  4. Hands off. Wait 5 s, plug the power in, step back to arm's length. lamp-body starts about"
  echo "     a minute later and moves the arm by itself."
  [ $EXPECT = wait ] && echo "     It should let its arm go and wait: fold it by hand only after you have run step 5."
  echo "  5. Once you can log in again: sh $B/row.sh $ROW after"
  ask "Type shutdown to power off now, anything else to leave:"
  [ "$ANS" = shutdown ] || { log "left without a power cut"; exit 0; }
  date -Iseconds > $B/logs/power-$ROW.pending
  log "lamp-setup shutdown at $(date -Iseconds)"
  lamp-setup shutdown
  exit 0
fi

# MODE=after: judge this boot's lamp-body start.
[ -e $B/logs/power-$ROW.pending ] || echo "NOTE: no power-$ROW.pending: was 'row.sh $ROW power' run before this boot?"
TB=$(date -d "@$(( $(date +%s) - $(up_us) / 1000000 ))" '+%Y-%m-%d %H:%M:%S')
log "booted at $TB; lamp-body InactiveExit at $(secs 1 "$(mono InactiveExitTimestampMonotonic)") s and READY at $(secs 1 "$(mono ActiveEnterTimestampMonotonic)") s after the kernel started"
echo 0 > $L.nr
i=0; while [ "$(systemctl show -p ActiveState --value lamp-body)" = activating ]; do
  i=$((i+1)); [ $i -gt 200 ] && break; [ $((i % 10)) = 0 ] && echo "  ... still starting, ${i} s"; sleep 1; done
READY_S=$(secs "$(mono InactiveExitTimestampMonotonic)" "$(mono ActiveEnterTimestampMonotonic)")
log "lamp-body: READY $READY_S s after its start (systemd, monotonic)"
judge "$TB"
[ $EXPECT = wait ] && { hand_wait "$TB"; judge "$TB"; }
stand_check
row3_check
NOTE="${NOTE_DROP:+$NOTE_DROP; }power cut; $NOTE"
judge "$TB"
verdict
rm -f $B/logs/power-$ROW.pending
