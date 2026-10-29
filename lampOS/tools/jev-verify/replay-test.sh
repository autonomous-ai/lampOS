#!/usr/bin/env bash
# replay-test.sh: runs replay.sh in DRY mode against a stub ssh (canned lamp answers, no lamp, no sound,
# no real sleep): the all-ok preflight and a one-pass run, then one failing case per check, each of
# which must ABORT with its reason and "nothing ran". Prints PASS and exits 0, or FAIL lines and exits 1.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 1
T=$(mktemp -d) || exit 1; trap 'rm -rf "$T"' EXIT
cat > "$T/ssh" <<'EOF'
#!/usr/bin/env bash
# Stub ssh: answers by what the command asks; FAIL names the one check to fail. Reads stdin unless -n.
a="$*"; [ "$1" = -n ] || in=$(cat); a="$a ${in:-}"
f=${FAIL:-}
case $a in
  *"loans.txt"*) echo "$a" >> "$STUBLOG" ;;
  *"lampos-update/state.json"*) echo '"build":{"sequence":8652,"commit":"f756b8678"}' ;;
  *"systemctl cat lamp-policy"*) echo 'ExecStart=/opt/lampOS/policy/current/lamp-policy --key-file %d/jev.key --log-dir /var/lib/lampos-policy' ;;
  *"tee /run/systemd"*) [ "$f" = sudo ] && exit 1; echo "DROPIN ${in:-}" >> "$STUBLOG" ;;
  *"rm -f /run/systemd"*) echo "DROPIN-REMOVED" >> "$STUBLOG" ;;
  *"is-active lamp-policy && sudo"*) echo active; echo "/opt/lamp-policy --key-file /k --hello --progress" ;;
  *"is-active lamp-policy"*) echo active ;;
  *"bash -s -- status"*) [ "$f" = lock ] && h=someone-else || h=jev-verifier
    printf 'holder: %s\nsession: s1\nmachine: mac\n' "$h"; exit 10 ;;
  *"date +%s%3N"*) echo 1761700000123 ;;
  *"is-active lamp-voice"*) echo active; echo active; echo active; echo active ;;
  *"ls -d /run/systemd"*) [ "$f" = dropin ] && echo /run/systemd/system/lamp-voice.service.d/x.conf; true ;;
  *"muted by the restart guard"*) [ "$f" = muted ] && echo 1 || echo 0 ;;
  *"voice/mute/latest"*) echo '{"muted":false}' ;;
  *"/cmdline"*) [ "$f" = act ] && echo "/opt/lamp-policy --shadow --act hello" || echo "/opt/lamp-policy --shadow" ;;
  *"ls -l /var/lib/lampos-policy"*) echo "-rw-r----- 1 lamp lamp 812 Oct 29 06:00 decisions-2026-10-29.jsonl"; echo READABLE ;;
  *"for b in lamp-bus"*) [ "$f" = bus ] && exit 1; echo lamp-bus ;;
  *"latest room/state"*) # busy while the colleagues' talk is among the last 3 clips played
    [ "$f" != roomnotbusy ] && tail -3 "$DRY_PLAYLOG" 2>/dev/null | grep -q t02 && echo '"busy"' || echo '"quiet"' ;;
  *"heard [1-9]"*) echo 0 ;;
  *"a face in"*) case $f in face) echo 2 ;; noeyes) ;; *) echo 0 ;; esac ;;
  *"sget Speaker"*) echo "  Front Left: Playback 26 [52%] [on]" ;;
  *"sset Speaker"*) echo "$a" >> "$STUBLOG" ;;
  *"sh -s -- 'lamp-voice'"*) echo 1 ;;              # jwait: reached
  *"voice/state"*) echo 3 ;;                       # speech_done: spoke 3 s
  *"grep -cE"*) echo 0 ;;                          # nv
  *"grep -oE 'request"*) echo "request 7: its answer is played" ;;
  *"stand [0-9.]"*) echo "stand 0.82" ;;
  *"decisions-"*"sort -u"*) [ "$f" = policyfailed ] && fl=',"failed":"timed_out"'
    echo "{\"at_unix_ms\":1761700000000,\"about\":\"attention\",\"request\":7${fl:-}}" ;;
  *"short-unix"*) echo "1761700001.0 request 7: its answer is played" ;;
  *) echo "STUB-UNKNOWN $a" >&2; exit 99 ;;
esac
EOF
chmod +x "$T/ssh"
fails=0
r() { rm -f "$T/plays.log"; DRY_PLAYLOG=$T/plays.log STUBLOG=$T/stub.log DRY=1 DRY_SSH=$T/ssh LAMP=lamp-test "$HERE/replay.sh" "$@" 2>&1; }
want() { local name=$1 pat=$2 out=$3; printf '%s\n' "$out" | grep -qE -- "$pat" || { echo "FAIL $name: no /$pat/"; printf '%s\n' "$out" | tail -5; fails=$((fails+1)); }; }

o=$(export FAIL=; r preflight "$T/p"); want preflight-ok 'preflight done' "$o"
want preflight-noplay '^ok woken' "$o"; printf '%s\n' "$o" | grep -qE 'afplay clips/[qb][0-9]' && { echo "FAIL preflight played a labelled line"; fails=$((fails+1)); }
[ -e "$T/p/aborted" ] && { echo "FAIL preflight wrote aborted"; fails=$((fails+1)); }
want busy-probe '^busy probe: room/state=busy' "$o"
for c in clips lock units "no /run drop-in" "not muted" shadow "policy log" room/state "room quiet" "no face" Speaker "Mac volume" "policy decided"; do
  want "check $c" "^ok .*$c" "$o"; done
grep -qE 'sset Speaker 26( |$)' "$T/stub.log" || { echo "FAIL Speaker not put back"; fails=$((fails+1)); }

grep -q 'DROPIN \[Service\]' "$T/stub.log" && grep -q 'DROPIN-REMOVED' "$T/stub.log" || { echo "FAIL policy drop-in not put in and removed"; fails=$((fails+1)); }
want policy-extra '^ok lamp-policy restarted in shadow with --hello --progress' "$o"
grep -q 'preflight only, no labelled line played' "$T/stub.log" || { echo "FAIL preflight loans line"; fails=$((fails+1)); }

: > "$T/stub.log"; o=$(export FAIL=; PASSES=1 r run "$T/r"); want run-ok 'pulled: 42 lines played' "$o"
grep -q 'done (42 lines played)' "$T/stub.log" || { echo "FAIL run loans done line"; fails=$((fails+1)); }
[ "$(head -1 "$T/r/plays.tsv")" = "$(printf 'pass\tid\tlamp_start_ms\tlamp_end_ms\tvoice_seen\tmacvol\troom_read')" ] || { echo "FAIL plays.tsv header"; fails=$((fails+1)); }
[ "$(awk -F'\t' 'NR>1 && $1==1 && $5=="played" && $6==45' "$T/r/plays.tsv" | wc -l | tr -d ' ')" = 42 ] || { echo "FAIL plays.tsv rows"; fails=$((fails+1)); }
# Each follow-up right after its parent, and every busy-block line read room/state busy before it.
for pf in q02:f01 q04:f02 q06:f03 b04:f04 b02:f05; do
  awk -F'\t' -v p=${pf%:*} -v f=${pf#*:} 'prev == p && $2 == f {ok=1} {prev=$2} END {exit !ok}' "$T/r/plays.tsv" ||
    { echo "FAIL $pf: the follow-up is not right after its parent"; fails=$((fails+1)); }; done
[ "$(awk -F'\t' 'NR>1 && ($2 ~ /^b/ || $2 == "f04" || $2 == "f05") && $7 != "busy"' "$T/r/plays.tsv" | wc -l | tr -d ' ')" = 0 ] ||
  { echo "FAIL a busy line's room_read is not busy"; fails=$((fails+1)); }
o=$(export FAIL=; PASSES=1 DEADLINE=1 r run "$T/dl"); want deadline '^ABORT the deadline has passed already; nothing ran' "$o"
grep -q 'aborted, nothing ran' "$T/stub.log" || { echo "FAIL deadline abort loans line"; fails=$((fails+1)); }
o=$(export FAIL=; DRY_MACMUTED=true r preflight "$T/mm"); want mac-muted "^ABORT the Mac's output is muted" "$o"
o=$(export FAIL=sudo; r preflight "$T/su"); want sudo-noted 'hello/progress NOT MEASURED: the drop-in could not be put in' "$o"; want sudo-goes-on 'preflight done' "$o"
o=$(export FAIL=; POLICY_EXTRA='--hello --act hello' r preflight "$T/pa"); want extra-act '^ABORT POLICY_EXTRA has --act' "$o"
o2=$(export FAIL=; PASSES=1 r run "$T/r2"); cmp -s <(cut -f2 "$T/r/plays.tsv") <(cut -f2 "$T/r2/plays.tsv") || { echo "FAIL order not repeatable"; fails=$((fails+1)); }
o3=$(export FAIL=; PASSES=2 ONLY='^q0[1-4]$' r run "$T/r3")
[ "$(awk -F'\t' '$1==1{printf "%s ",$2}' "$T/r3/plays.tsv")" != "$(awk -F'\t' '$1==2{printf "%s ",$2}' "$T/r3/plays.tsv")" ] || { echo "FAIL passes share an order"; fails=$((fails+1)); }

o=$(export FAIL=roomnotbusy; PASSES=1 r run "$T/rb"); want busy-abort 'ABORT busy block \(pass 1\)' "$o"
[ "$(awk -F'\t' 'NR>1' "$T/rb/plays.tsv" | wc -l | tr -d ' ')" = 27 ] || { echo "FAIL busy abort kept quiet results"; fails=$((fails+1)); }

for c in "muted:muted by the restart guard" "act:runs with --act" "face:saw a face" "lock:not held by jev-verifier" \
         "bus:no lamp-bus answers" "dropin:foreign /run drop-in" "noeyes:no report in 15 s" \
         "policyfailed:decision on the wake failed .timed_out."; do
  o=$(export FAIL=${c%%:*}; r preflight "$T/f-${c%%:*}")
  want "fail-${c%%:*}" "^ABORT .*${c#*:}.*; nothing ran\$" "$o"
  grep -q "${c#*:}" "$T/f-${c%%:*}/aborted" 2>/dev/null || { echo "FAIL fail-${c%%:*}: OUT/aborted"; fails=$((fails+1)); }
done
[ $fails = 0 ] && echo PASS || { echo "FAIL ($fails)"; exit 1; }
