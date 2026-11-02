#!/usr/bin/env bash
# cmd-test.sh: runs cmd.sh in DRY mode against a stub ssh that keeps the light and the Speaker as state
# (a played command changes them until the restore), then cmd-score.py on the run. The all-ok preflight
# and a one-pass run, a wrong action, then one failing case per guard. Prints PASS and exits 0, or FAIL.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
T=$(mktemp -d) || exit 1; trap 'rm -rf "$T"' EXIT
cat > "$T/ssh" <<'EOF'
#!/usr/bin/env bash
# Stub ssh. State: $STATE/light (light/state), $STATE/spk (Speaker raw), $STATE/timer (the dead-man).
a="$*"; [ "$1" = -n ] || in=$(cat); a="$a ${in:-}"
f=${FAIL:-}; S=$STATE; last=$(tail -1 "$DRY_PLAYLOG" 2>/dev/null)
case $a in
  *"loans.txt"*) echo "$a" >> "$STUBLOG" ;;
  *"lampos-update/state.json"*) echo '"build":{"sequence":9424,"commit":"0000000"}' ;;
  *"bash -s -- status"*) [ "$f" = lock ] && h=someone-else || h=jev-verifier
    printf 'holder: %s\nsession: s1\nmachine: mac\n' "$h"; exit 10 ;;
  *"date +%s%3N"*) echo 1761700000123 ;;
  *"is-active lamp-voice lamp-actions"*) echo active; echo active; echo active; echo active ;;
  *"ls -d /run/systemd"*) true ;;
  *"list-timers"*) [ "$f" = timer ] && echo "n/a n/a n/a n/a jev-cmd-deadman.timer jev-cmd-deadman.service"; true ;;
  *"/environ"*) [ "$f" = nosay ] || echo LAMP_SECOND_SAY_WITH_CALL=on; echo LAMP_SHORT_DONE=on ;;
  *"systemctl cat lamp-voice lamp-actions"*) echo "# /etc/systemd/system/lamp-voice.service" ;;
  *"muted by the restart guard"*) echo 0 ;;
  *"voice/mute/latest"*) echo '{"muted":false}' ;;
  *"heard [1-9]"*) echo 0 ;;
  *"a face in"*) echo 0 ;;
  *"stand [0-9.]"*) echo "stand 0.82" ;;
  *"tee /run/jev-cmd-restore.py"*) echo "RESTORE-WRITTEN" >> "$STUBLOG" ;;
  *"systemd-run"*) echo "ARMED $a" >> "$STUBLOG"; echo armed > "$S/timer" ;;
  *"stop jev-cmd-deadman.timer"*) echo "DEADMAN-STOPPED" >> "$STUBLOG"; rm -f "$S/timer" ;;
  *"python3 /run/jev-cmd-restore.py"*)   # the restore: the start light back, then the Speaker raw
    raw=$(printf '%s' "$a" | sed -E "s/.*' '([0-9]+)'.*/\1/"); echo "$raw" > "$S/spk"
    echo "RESTORED $raw" >> "$DRY_PLAYLOG"
    if [ "$f" = norestore ]; then echo '{"on":true,"colour":"red","level":3}'; else cat "$S/light0"; fi ;;
  *"sset Speaker 50%"*) echo 26 > "$S/spk" ;;
  *"sset Speaker"*) echo "$a" >> "$STUBLOG"; echo "$a" | grep -oE '[0-9]+$' > "$S/spk" ;;
  *"sget Speaker"*) echo "  Front Left: Playback $(cat "$S/spk") [52%] [on]" ;;
  *"light/state/latest"*) case $last in    # a played light command shows until the restore
      clips/c01.wav) echo '{"on":true,"colour":"red","level":3}' ;;
      clips/o01.wav) [ "$f" = wrong ] && echo '{"on":true,"colour":"red","level":3}' || cat "$S/light0" ;;
      *) cat "$S/light0" ;; esac ;;
  *"voice/state/latest' '"*) echo 1761700002000 ;;    # first_sound: the header's at_unix_ms
  *"sh -s -- 'lamp-voice'"*) echo 1 ;;                 # jwait: reached
  *"voice/state"*) echo 3 ;;                           # speech_done
  *"grep -cE"*) echo 0 ;;                              # nv
  *"asked for"*"short-unix"*|*"short-unix"*) echo "1761700001.0 request 7: its answer is played" ;;
  *"asked for"*) case $last in clips/c01.wav) echo light_colour ;; clips/c05.wav) echo volume_up ;;
      clips/o01.wav) [ "$f" = wrong ] && echo light_colour ;; esac; true ;;
  *"grep -oE 'request"*) echo "request 7: its answer is played" ;;
  *"ls -td /var/lib/lampOS/voice/sessions"*) echo /var/lib/lampOS/voice/sessions/s1/ ;;
  *"turn-times"*) echo '{"turn":7,"kind":"function","first_sound_ms":1500.0}'; echo '{"turns":1}' ;;
  *) echo "STUB-UNKNOWN $a" >&2; exit 99 ;;
esac
EOF
chmod +x "$T/ssh"
# otherplaying stub: FAIL=other is a lamp PLAYING throughout; FAIL=otherpass2 only from pass 2's check.
cat > "$T/oplib.sh" <<'EOF'
otherplaying() { case ${FAIL:-} in other) return 0 ;; otherpass2) grep -q 'clips/c' "$DRY_PLAYLOG" 2>/dev/null ;; *) return 1 ;; esac; }
EOF
export OPLIB=$T/oplib.sh
fails=0
r() { mkdir -p "$T/st"; echo '{"on":true,"colour":"warm","level":3}' > "$T/st/light0"; echo 30 > "$T/st/spk"; rm -f "$T/st/timer" "$T/plays.log"
  STATE=$T/st DRY_PLAYLOG=$T/plays.log STUBLOG=$T/stub.log DRY=1 DRY_SSH=$T/ssh LAMP=lamp-test "$HERE/cmd.sh" "$@" 2>&1; }
want() { local name=$1 pat=$2 out=$3; printf '%s\n' "$out" | grep -qE -- "$pat" || { echo "FAIL $name: no /$pat/"; printf '%s\n' "$out" | tail -5; fails=$((fails+1)); }; }
bad() { echo "FAIL $*"; fails=$((fails+1)); }

o=$(export FAIL=; r preflight "$T/p"); want preflight-ok 'preflight done' "$o"
grep -qE 'clips/[cvno][0-9]' "$T/plays.log" && bad "preflight played a labelled line"
for c in clips lock units "no /run drop-in" "LAMP_SECOND_SAY_WITH_CALL=on" "not muted" "room quiet" "no face" "dead-man armed" "Speaker 50%" "restore checked once" "Mac volume" woken; do
  want "check $c" "^ok .*$c" "$o"; done
grep -q "ARMED .*on-active=75m .*'{\"on\":true,\"colour\":\"warm\",\"level\":3}' '30'" "$T/stub.log" || bad "dead-man not armed with the start state"
grep -q DEADMAN-STOPPED "$T/stub.log" || bad "dead-man not stopped after the exit restore"
[ "$(cat "$T/st/spk")" = 30 ] || bad "Speaker not back to its start raw value (30): $(cat "$T/st/spk")"
want light-back 'light and Speaker back' "$o"

: > "$T/stub.log"; o=$(export FAIL=; PASSES=1 ONLY='c01|c05|n01|o01' r run "$T/r"); want run-ok '^pulled: 4 lines played' "$o"
[ "$(grep -c '^RESTORED 26$' "$T/plays.log")" -ge 5 ] || bad "not restored to the run's Speaker after every line"
awk -F'\t' '$2 == "c01" && $6 == "light_colour" && $7 ~ /red/' "$T/r/plays.tsv" | grep -q . || bad "c01: call or light_after not recorded"
s=$(python3 "$HERE/cmd-score.py" "$T/r"); want score-wrong0 'Wrong actions: 0 of 4' "$s"
want score-c01 '\| 1 \| c01 \| clear \| light_colour:red \| played \| light_colour \|.*\| right \|' "$s"
want score-n01 '\| n01 \|.*\| right \|' "$s"; want score-c05 '\| c05 \|.*\| right \|' "$s"
want score-decision '1500' "$s"

o=$(export FAIL=wrong; PASSES=1 ONLY='o01' r run "$T/w"); s=$(python3 "$HERE/cmd-score.py" "$T/w")
want wrong-action 'Wrong actions: 1 of 1' "$s"

for c in lock:'lamp lock is not held' nosay:'no LAMP_SECOND_SAY_WITH_CALL=on' timer:'timer is already there'; do
  o=$(export FAIL=${c%%:*}; r preflight "$T/f"); want "guard ${c%%:*}" "^ABORT .*${c#*:}.*nothing ran" "$o"
  grep -q 'ARMED' "$T/stub.log" 2>/dev/null; done
: > "$T/stub.log"; o=$(export FAIL=norestore; r preflight "$T/n")
want norestore-abort '^ABORT the restore did not give back the start state' "$o"
want norestore-armed 'LIGHT OR SPEAKER NOT PUT BACK: the dead-man' "$o"
grep -q DEADMAN-STOPPED "$T/stub.log" && bad "norestore: the dead-man was stopped though the restore failed"

o=$(export FAIL=other; r preflight "$T/o"); want other-preflight '^ABORT another lamp is PLAYING on #3336 after waiting 600 s \(preflight\).*nothing ran' "$o"
grep -qE 'clips/' "$T/plays.log" 2>/dev/null && bad "other: something played while another lamp was PLAYING"
o=$(export FAIL=otherpass2; PASSES=2 ONLY='c01' r run "$T/o2"); want other-pass2 '^ABORT another lamp is PLAYING .*before pass 2' "$o"
[ "$(grep -c 'clips/c01' "$T/plays.log")" = 1 ] || bad "otherpass2: c01 should have played once (pass 1 only)"
[ "$fails" = 0 ] && echo PASS || { echo "FAIL ($fails)"; exit 1; }
