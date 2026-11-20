#!/usr/bin/env bash
# work-records-row.py's dry run on the Mac against work-records-stand-in.py: no lamp, no key, no sound.
# Checks every leg passes, the put-back restores the installed binaries and removes the drop-in, a lamp already
# on OpenClaw gets no drop-in, a loud Mac fails L4 alone, and a run left unput-back is put back first.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
fails=0
check() { local what=$1; shift; if "$@" >/dev/null 2>&1; then echo "ok: $what"; else echo "FAIL: $what"; fails=$((fails + 1)); fi; }
# Unix socket paths are at most 104 bytes on macOS: a short root.
T=$(mktemp -d /tmp/wr7930.XXXXXX)
trap 'kill $(cat "$T"/*/run/serve.pid 2>/dev/null) 2>/dev/null; rm -rf "$T"' EXIT
printf '#!/bin/sh\nexec python3 %s ssh "$*"\n' "$HERE/work-records-stand-in.py" >"$T/ssh"
printf '#!/bin/sh\nexec python3 %s play "$1"\n' "$HERE/work-records-stand-in.py" >"$T/play"
# The shared lamp lock, stood in: $WORK_ROOT/lock holds "holder: …" and "session: …" lines (lamp-lock.sh's
# own), status exits 1 with them when held and 0 when free; renew works only on our own.
cat >"$T/lock" <<'LOCK'
#!/bin/sh
f=$WORK_ROOT/lock
case $2 in
  status) echo status >>"$WORK_ROOT/lock.calls"; [ -e "$f" ] || exit 0; cat "$f"; exit 1 ;;
  renew) grep -qx "holder: $3" "$f" && grep -qx "session: $4" "$f" ;;
esac
LOCK
chmod +x "$T/ssh" "$T/play" "$T/lock"
export WORK_LOCK_SCRIPT=$T/lock LAMP_LOCK_HOLDER=builder LAMP_LOCK_SESSION=session
mkdir -p "$T/clips" "$T/out"
for c in en-ask en-yes en-how vi-ask vi-yes vi-how en-long-do; do : >"$T/clips/$c.wav"; done
printf '#!/bin/sh\necho "lamp-tasks ours"\n' >"$T/lamp-tasks"
printf '#!/bin/sh\necho "lamp-voice ours"\n' >"$T/lamp-voice"
FAST=(--poll 0.5 --readback-wait 0 --how-wait 0.3 --reply-wait 0.3 --gap 0 --cap 30)

lamp() { # lamp NAME ENGINE: a fresh stand-in lamp, its env exported
  export WORK_ROOT=$T/$1 WORK_SSH=$T/ssh WORK_PLAY=$T/play PLAYED_OUT=$T/out WORK_MAC_VOLUME=${VOL:-50}
  python3 "$HERE/work-records-stand-in.py" init "$WORK_ROOT" "$2"
  printf 'holder: builder\nsession: session\n' >"$WORK_ROOT/lock"
}
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }

# 1. A Hermes lamp: the drop-in comes and goes, every leg passes.
lamp a hermes
before=$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")
python3 "$HERE/work-records-row.py" dry-a --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" "${FAST[@]}" >"$T/a.log" 2>&1
V=$T/out/scenarios/work-records-dry-a
for leg in L0 L1 L2 L4 putback; do check "hermes lamp: $leg PASS" grep -q "^$leg PASS" "$V/verdict.txt"; done
check "L1 saw queued, working and done" python3 -c "import json,sys; s=json.load(open('$V/L1.json'))['states']; sys.exit(s!=['queued','working','done'])"
check "L1 kept the answer" python3 -c "import json,sys; sys.exit(not json.load(open('$V/L1.json'))['answer'])"
check "L2: the task newest during and after" python3 -c "import json,sys; c=json.load(open('$V/L2.json'))['checks']; sys.exit(not (c['during_newest_is_the_task'] and c['after_newest_is_the_task']))"
check "L4 heard check_agent_task in both languages" python3 -c "import json,sys; l=json.load(open('$V/L4.json'))['legs']; sys.exit(not all(l[x]['check_agent_task_called'] for x in ('en','vi')))"
check "the drop-in was logged" grep -q 'drop-in .*LAMP_LAMP_ENGINE=openclaw' "$WORK_ROOT/home/orangepi/changes.txt"
check "no hard-asks switch written" test -z "$(grep LAMP_HARD_ASKS "$WORK_ROOT/home/orangepi/changes.txt")"
check "the drop-in is gone" test ! -e "$WORK_ROOT/run/systemd/system/lamp-tasks.service.d/zz-7930-row-engine.conf"
check "the installed lamp-tasks is back" test "$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")" = "$before"
check "the state is cleared" test ! -e "$WORK_ROOT/var/tmp/work-records-7930"
check "the put-back was logged" grep -q 'put back, read twice' "$WORK_ROOT/home/orangepi/changes.txt"
check "the drop-in went under /run, which a reboot drops" grep -q 'drop-in .*/run/systemd/system/lamp-tasks.service.d/zz-7930-row-engine.conf' "$WORK_ROOT/home/orangepi/changes.txt"
check "no drop-in under /etc" test -z "$(grep 'drop-in .*/etc/systemd' "$WORK_ROOT/home/orangepi/changes.txt")"

# 1b. An older kit run's lamp-voice hard-asks drop-in, under /etc or /run, is removed by the put-back too.
lamp a2 hermes
python3 "$HERE/work-records-row.py" dry-a2 --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 1 >/dev/null 2>&1
for place in etc run; do
  mkdir -p "$WORK_ROOT/$place/systemd/system/lamp-voice.service.d"
  printf '[Service]\nEnvironment=LAMP_HARD_ASKS_TO_AGENT=on\n' >"$WORK_ROOT/$place/systemd/system/lamp-voice.service.d/zz-7930-row-hard-asks.conf"
done
python3 "$HERE/work-records-row.py" dry-a2 --put-back >"$T/a2.log" 2>&1
check "an older /etc drop-in: removed at put-back" test ! -e "$WORK_ROOT/etc/systemd/system/lamp-voice.service.d/zz-7930-row-hard-asks.conf"
check "an older /run drop-in: removed at put-back" test ! -e "$WORK_ROOT/run/systemd/system/lamp-voice.service.d/zz-7930-row-hard-asks.conf"

# 2. A lamp already on OpenClaw: no drop-in.
lamp b openclaw
python3 "$HERE/work-records-row.py" dry-b --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >"$T/b.log" 2>&1
V=$T/out/scenarios/work-records-dry-b
check "openclaw lamp: L1 PASS" grep -q '^L1 PASS' "$V/verdict.txt"
check "openclaw lamp: no drop-in written" test -z "$(grep drop-in "$WORK_ROOT/home/orangepi/changes.txt")"
check "no clips skips L4" grep -q '^L4 SKIP' "$V/verdict.txt"
check "openclaw lamp: put back" grep -q '^putback PASS' "$V/verdict.txt"

# 3. A loud Mac: L4 fails before any clip, the put-back still runs.
VOL=80 lamp c hermes
python3 "$HERE/work-records-row.py" dry-c --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" "${FAST[@]}" >"$T/c.log" 2>&1
V=$T/out/scenarios/work-records-dry-c
check "a Mac past 70% fails L4" grep -q '^L4 FAIL' "$V/verdict.txt"
check "it says why" grep -q 'past 70%' "$V/L4.json"
check "nothing was played" test -z "$(grep -c check_agent_task "$WORK_ROOT/journal.log" | grep -v '^0$')"
check "a loud Mac: put back" grep -q '^putback PASS' "$V/verdict.txt"

# 4. A run left unput-back: L0 refuses to stage, and --put-back alone restores it.
lamp d hermes
before=$(sha "$WORK_ROOT/opt/lampOS/voice/r1/lamp-voice")
python3 "$HERE/work-records-row.py" dry-d --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 1 >/dev/null 2>&1 &
pid=$!
# Killed as soon as its state is on the lamp, before any put-back.
for _ in $(seq 1 100); do [ -e "$WORK_ROOT/var/tmp/work-records-7930/state.json" ] && break; sleep 0.1; done
kill -9 $pid 2>/dev/null; wait $pid 2>/dev/null
check "a killed run leaves its state" test -e "$WORK_ROOT/var/tmp/work-records-7930/state.json"
python3 "$HERE/work-records-row.py" dry-d --put-back >"$T/d.log" 2>&1
check "--put-back restores the installed lamp-voice" test "$(sha "$WORK_ROOT/opt/lampOS/voice/r1/lamp-voice")" = "$before"
check "--put-back clears the state" test ! -e "$WORK_ROOT/var/tmp/work-records-7930"


# 5. SIGTERM and SIGINT mid-L1, and --deadline-in: each stops the legs and the put-back runs whole.
for how in TERM INT deadline; do
  lamp "s$how" hermes
  before=$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")
  extra=(); [ $how = deadline ] && extra=(--deadline "$(python3 -c 'import time; print(time.strftime("%H:%M:%S", time.localtime(time.time() + 14)))')")
  python3 "$HERE/work-records-row.py" "dry-s$how" --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 60 ${extra[@]+"${extra[@]}"} >"$T/s$how.log" 2>&1 &
  pid=$!
  if [ $how != deadline ]; then
    for _ in $(seq 1 200); do grep -q 'handed over' "$T/s$how.log" 2>/dev/null && break; sleep 0.1; done
    kill -$how $pid
  fi
  wait $pid
  V=$T/out/scenarios/work-records-dry-s$how
  check "$how: the run says it stopped" grep -q '^stopped STOPPED' "$V/verdict.txt"
  check "$how: put back" grep -q '^putback PASS' "$V/verdict.txt"
  check "$how: the installed lamp-tasks is back" test "$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")" = "$before"
  check "$how: the drop-in is gone" test ! -e "$WORK_ROOT/run/systemd/system/lamp-tasks.service.d/zz-7930-row-engine.conf"
done


# 6. A lamp-update round mid-L1: the legs stop, the round's build is left running, ours is out of the old folder.
lamp r hermes
before=$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")
python3 "$HERE/work-records-row.py" dry-r --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 60 >"$T/r.log" 2>&1 &
pid=$!
for _ in $(seq 1 200); do grep -q 'handed over' "$T/r.log" 2>/dev/null && break; sleep 0.1; done
python3 "$HERE/work-records-stand-in.py" round "$WORK_ROOT"
wait $pid
V=$T/out/scenarios/work-records-dry-r
check "round: the legs stop on it" grep -q 'a lamp-update round' "$V/stopped.json"
check "round: put back" grep -q '^putback PASS' "$V/verdict.txt"
check "round: the round's lamp-tasks is left running" grep -q 'round 14732' "$WORK_ROOT/opt/lampOS/tasks/current/lamp-tasks"
check "round: the round's lamp-voice is left running" grep -q 'round 14732' "$WORK_ROOT/opt/lampOS/voice/current/lamp-voice"
check "round: the old folder has the installed build back" test "$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")" = "$before"
check "round: the drop-in is gone" test ! -e "$WORK_ROOT/run/systemd/system/lamp-tasks.service.d/zz-7930-row-engine.conf"
check "round: the state is cleared" test ! -e "$WORK_ROOT/var/tmp/work-records-7930"

# 7. A stale state at start: its put-back first, then the run goes on.
lamp k hermes
before=$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")
python3 "$HERE/work-records-row.py" dry-k --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >/dev/null 2>&1 &
pid=$!
for _ in $(seq 1 600); do [ -e "$WORK_ROOT/var/tmp/work-records-7930/lamp-voice.installed" ] && break; sleep 0.1; done
sleep 0.5; kill -9 $pid 2>/dev/null; wait $pid 2>/dev/null
python3 "$HERE/work-records-row.py" dry-k2 --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >"$T/k2.log" 2>&1
V=$T/out/scenarios/work-records-dry-k2
check "stale state: its put-back ran first" grep -q 'its put-back first' "$T/k2.log"
check "stale state: then L0 and L1 pass" grep -q '^L1 PASS' "$V/verdict.txt"
check "stale state: put back at the end" grep -q '^putback PASS' "$V/verdict.txt"
check "stale state: the installed lamp-tasks is back" test "$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")" = "$before"

# 8. A stale state whose put-back fails (its backup is not the installed build): a clear stop, nothing staged.
lamp f hermes
python3 "$HERE/work-records-row.py" dry-f --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >/dev/null 2>&1 &
pid=$!
for _ in $(seq 1 600); do [ -e "$WORK_ROOT/var/tmp/work-records-7930/lamp-voice.installed" ] && break; sleep 0.1; done
sleep 0.5; kill -9 $pid 2>/dev/null; wait $pid 2>/dev/null
echo spoiled >"$WORK_ROOT/var/tmp/work-records-7930/lamp-tasks.installed"
python3 "$HERE/work-records-row.py" dry-f2 --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >"$T/f2.log" 2>&1; code=$?
check "failed stale put-back: exits non-zero" test "$code" -ne 0
check "failed stale put-back: says so" grep -q 'its put-back failed' "$T/f2.log"
check "failed stale put-back: L0 refused" grep -q '^L0 REFUSED' "$T/out/scenarios/work-records-dry-f2/verdict.txt"
check "failed stale put-back: the state is kept" test -e "$WORK_ROOT/var/tmp/work-records-7930/state.json"


# 9. L4 by request.sock plus --l4-spoken (L4s): each passes on its own verdict; a
# `conflict` on L4's confirm is read again and taken; lamp-update's routine "may be installed" line is no round.
lamp v hermes
echo 2 >"$WORK_ROOT/conflict-at"   # L1 confirms first, L4 second
echo 2 >"$WORK_ROOT/prepare-conflict-at"   # L4's prepare (c0a4-6 had it on vi's)
printf '%s lamp lamp-update[1]: lamp-update: OpenClaw may be installed now: a round reached, the setup walk not under way\n' "$(( $(date +%s) + 30 ))" >"$WORK_ROOT/journal-lamp-update.log"
python3 "$HERE/work-records-row.py" dry-v --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" --l4-spoken "${FAST[@]}" --how-wait 4 >"$T/v.log" 2>&1
V=$T/out/scenarios/work-records-dry-v
for leg in L4 L4s putback; do check "legs: $leg PASS" grep -q "^$leg PASS" "$V/verdict.txt"; done
check "L4 handed its task over by request.sock" grep -q 'handed over for the voice to ask about' "$T/v.log"
check "a confirm conflict is taken on a read again" grep -q 'confirm of .* taken after 1 conflict' "$T/v.log"
check "a prepare conflict is asked again" grep -q 'prepare taken after 1 conflict' "$T/v.log"
check "the routine may-be-installed line is no round" test ! -e "$V/stopped.json"
check "L4 kept the turn's journal lines by request" python3 -c "import json,sys; l=json.load(open('$V/L4.json'))['legs']; sys.exit(not all(l[x]['journal_by_request'] for x in ('en','vi')))"
check "L4s kept the yes-in-code line" grep -q 'yes taken in code' "$V/L4s.json"
check "no lamp-voice drop-in written" test -z "$(grep 'lamp-voice.service.d' "$WORK_ROOT/home/orangepi/changes.txt")"
check "the drop-in is gone" test ! -e "$WORK_ROOT/run/systemd/system/lamp-tasks.service.d/zz-7930-row-engine.conf"


# 10. A reply telling the done record's answer counts, in another language too; one that doesn't, doesn't.
check "an answer told counts" python3 -c "
import importlib.util, sys
spec = importlib.util.spec_from_file_location('k', '$HERE/work-records-row.py'); k = importlib.util.module_from_spec(spec); spec.loader.exec_module(k)
zh = 'wttr.in 预测明天最高 17°C、最低 15°C，降雨概率为 24%。Open-Meteo 预测最高 18.5°C、最低 14.3°C，降雨概率为 76%。'
r = 'both sites put tomorrow at roughly 15 to 18 degrees, but they split hard on rain, one at about a quarter chance and the other near three quarters.'
fr = 'one says about a quarter, the other three quarters'
en = 'Both sites put tomorrow in Hanoi at about 15 to 18 degrees, but they disagree on rain, 24% against 76%.'
sys.exit(not (k.answer_told(r, zh) and k.answer_told(fr, zh) and k.answer_told(r, en)
              and not k.answer_told('I am not sure what you mean.', zh) and not k.answer_told(r, None)
              and not k.answer_told('It is still working on step 3.', zh)))"


# 11. --only L4s: L0, L4s and the put-back run; every other leg is SKIP (--only).
lamp o hermes
python3 "$HERE/work-records-row.py" dry-o --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" --only L4s "${FAST[@]}" --how-wait 4 >"$T/o.log" 2>&1
V=$T/out/scenarios/work-records-dry-o
for leg in L0 L4s putback; do check "--only L4s: $leg PASS" grep -q "^$leg PASS" "$V/verdict.txt"; done
for leg in L1 L2 L4; do check "--only L4s: $leg SKIP" grep -q "^$leg SKIP" "$V/verdict.txt"; done
check "--only L4s: no task over request.sock" test -z "$(grep 'handed over' "$T/o.log")"


# 13. The lock: another's at the start, nothing is done on the lamp; taken by another mid-run, the legs stop
# at once and the put-back is not run (the state is kept for --put-back under the lock).
lamp x hermes
printf 'holder: other-builder\nsession: other\n' >"$WORK_ROOT/lock"
before=$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")
python3 "$HERE/work-records-row.py" dry-x --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" >"$T/x.log" 2>&1
check "another's lock at start: says so" grep -q 'lock not held: other-builder (other) (before L0)' "$T/x.log"
check "another's lock at start: nothing staged" test "$(sha "$WORK_ROOT/opt/lampOS/tasks/r1/lamp-tasks")" = "$before"
check "another's lock at start: no put-back" grep -q '^putback NOT RUN' "$T/out/scenarios/work-records-dry-x/verdict.txt"
lamp y hermes
python3 "$HERE/work-records-row.py" dry-y --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 60 >"$T/y.log" 2>&1 &
pid=$!
for _ in $(seq 1 600); do grep -q 'handed over' "$T/y.log" 2>/dev/null && break; sleep 0.1; done
printf 'holder: other-builder\nsession: other\n' >"$WORK_ROOT/lock"
wait $pid
V=$T/out/scenarios/work-records-dry-y
check "lock taken mid-run: the legs stop" grep -q 'lock not held: other-builder' "$V/stopped.json"
check "lock taken mid-run: the put-back is not run" grep -q '^putback NOT RUN' "$V/verdict.txt"
check "lock taken mid-run: the state is kept" test -e "$WORK_ROOT/var/tmp/work-records-7930/state.json"

# 15. --langs vi: L4 and L4s play and score vi alone.
lamp w hermes
printf '#!/bin/sh\nbasename "$1" >>"%s"\nexec "%s" "$1"\n' "$T/w.played" "$T/play" >"$T/play-w"; chmod +x "$T/play-w"
WORK_PLAY=$T/play-w python3 "$HERE/work-records-row.py" dry-w --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" --only L4,L4s --langs vi "${FAST[@]}" --how-wait 4 >"$T/w.log" 2>&1
V=$T/out/scenarios/work-records-dry-w
for leg in L4 L4s; do check "--langs vi: $leg PASS, vi alone" python3 -c "import json,sys; d=json.load(open('$V/$leg.json')); sys.exit(not (d['verdict']=='PASS' and list(d['legs'])==['vi']))"; done
check "--langs vi: vi clips played, no en" sh -c 'grep -q "^vi-how" "$0" && ! grep -q "^en-" "$0"' "$T/w.played"
check "--langs fr: refused" sh -c '! python3 "$0/work-records-row.py" dry-f --langs fr >/dev/null 2>&1' "$HERE"
check "no lock holder: refused, said why" sh -c 'env -u LAMP_LOCK_HOLDER python3 "$0/work-records-row.py" dry-f --put-back 2>&1 | grep -q -- "--lock-holder (or LAMP_LOCK_HOLDER) is required"' "$HERE"
check "no lock session: refused, said why" sh -c 'env -u LAMP_LOCK_SESSION python3 "$0/work-records-row.py" dry-f --lock-holder builder --put-back 2>&1 | grep -q -- "--lock-session (or LAMP_LOCK_SESSION) is required"' "$HERE"

# 16. A skipped leg costs nothing: --only L4 looks at the lock before L0, before L4 and before the put-back
# alone (c0a4-12: each skipped leg took ~35 s of lock and round checks).
lamp s hermes
python3 "$HERE/work-records-row.py" dry-s --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" --only L4 --langs vi "${FAST[@]}" --how-wait 4 >"$T/s.log" 2>&1
check "--only L4: L4 PASS" grep -q '^L4 PASS' "$T/out/scenarios/work-records-dry-s/verdict.txt"
check "--only L4: three lock looks (L0, L4, put-back)" test "$(wc -l <"$WORK_ROOT/lock.calls" | tr -d ' ')" = 3

# 18. HW, "how far is it?" while a long do-task runs. The rules on fake data first: VOID when the record
# was over as the how was heard; PASS only with the how tool, working then, and a running reply, not the answer.
rv() { python3 -c "
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location('row', '$HERE/work-records-row.py'); row = importlib.util.module_from_spec(spec); spec.loader.exec_module(row)
leg, heard, calls, reply, after = json.loads(sys.argv[1])
print(row.running_verdict(leg, heard, [{'calls': c} for c in calls], reply, after)['verdict'])" "$1"; }
W='{"state":"working","done":false,"step":{"n":3,"tool":"web_search"}}'
check "HW rules: working, check_agent_task, still working: PASS" test "$(rv "[\"HW\", $W, [\"check_agent_task:1\"], \"It's still working, on step 3.\", {}]")" = PASS
check "HW rules: the step named as words: PASS" test "$(rv "[\"HW\", $W, [\"check_agent_task:1\"], \"It is on a web search now.\", {}]")" = PASS
check "HW rules: done when heard: VOID" test "$(rv '["HW", {"state":"done","done":true}, ["check_agent_task:1"], "It is still working.", {}]')" = VOID
check "HW rules: no check_agent_task: FAIL" test "$(rv "[\"HW\", $W, [\"none\"], \"It's still working.\", {}]")" = FAIL
check "HW rules: queued when heard: FAIL" test "$(rv '["HW", {"state":"queued","done":false}, ["check_agent_task:1"], "It is still working.", {}]')" = FAIL
check "HW rules: no running words: FAIL" test "$(rv "[\"HW\", $W, [\"check_agent_task:1\"], \"Sure thing.\", {}]")" = FAIL
cat >"$T/told.json" <<'TOLD'
["HW", {"state": "working", "done": false}, ["check_agent_task:1"],
 "Still working: the Pro costs 30,990,000 dong at FPT Shop and 30,490,000 dong at CellphoneS.",
 {"answer": "The Pro costs 30,990,000 dong at FPT Shop and 30,490,000 dong at CellphoneS."}]
TOLD
check "HW rules: the reply tells the answer: FAIL" test "$(rv "$(cat "$T/told.json")")" = FAIL
# Then the leg on the stand-in lamp: it PASSes on a record still working when the how plays.
lamp h hermes
python3 "$HERE/work-records-row.py" dry-h --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" --clips "$T/clips" --only HW "${FAST[@]}" --how-wait 4 >"$T/h.log" 2>&1
V=$T/out/scenarios/work-records-dry-h
check "--only HW: HW PASS" grep -q "^HW PASS" "$V/verdict.txt"
check "--only HW: the record was working as the how was heard" python3 -c "import json,sys; sys.exit(json.load(open('$V/HW.json'))['at_heard']['state']!='working')"
for leg in L1 L2 L4; do check "--only HW: $leg SKIP" grep -q "^$leg SKIP" "$V/verdict.txt"; done
check "--only HW: unknown leg names are refused" sh -c '! python3 "$0/work-records-row.py" dry-q --only HX >/dev/null 2>&1' "$HERE"
for leg in L3 L5 HA; do check "--only $leg: refused (out of scope)" sh -c '! python3 "$0/work-records-row.py" dry-q --only "$1" >/dev/null 2>&1' "$HERE" "$leg"; done
check "--skip-make and --ask-leg: refused" sh -c '! python3 "$0/work-records-row.py" dry-q --skip-make >/dev/null 2>&1 && ! python3 "$0/work-records-row.py" dry-q --ask-leg >/dev/null 2>&1' "$HERE"

# 19. The running binaries read before and after every leg (/proc/<MainPID>/exe): either not the installed
# test build's, the leg is VOID, never PASS or FAIL, and the run stops for the put-back. Fake data first.
bc() { python3 -c "
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location('row', '$HERE/work-records-row.py'); row = importlib.util.module_from_spec(spec); spec.loader.exec_module(row)
print(','.join(row.binary_changed(*json.loads(sys.argv[1]))) or 'none')" "$1"; }
check "binaries: both ours: none changed" test "$(bc '[{"lamp-tasks":"aa","lamp-voice":"bb"}, {"lamp-tasks":"aa","lamp-voice":"bb"}]')" = none
check "binaries: lamp-voice promoted: it is named" test "$(bc '[{"lamp-tasks":"aa","lamp-voice":"bb"}, {"lamp-tasks":"aa","lamp-voice":"cc"}]')" = lamp-voice
check "binaries: one not read: it is named" test "$(bc '[{"lamp-tasks":"aa","lamp-voice":"bb"}, {"lamp-tasks":null,"lamp-voice":"bb"}]')" = lamp-tasks
# Then on the stand-in lamp: a stable promote mid-L1 (lamp-tasks' running file replaced, no round line and
# no moved link) makes L1 VOID; the run stops and the put-back runs.
lamp p hermes
python3 "$HERE/work-records-row.py" dry-p --tasks-bin "$T/lamp-tasks" --voice-bin "$T/lamp-voice" "${FAST[@]}" --cap 60 >"$T/p.log" 2>&1 &
pid=$!
for _ in $(seq 1 300); do grep -q 'handed over' "$T/p.log" 2>/dev/null && break; sleep 0.1; done
printf '#!/bin/sh\necho "lamp-tasks stable promote"\n' >"$WORK_ROOT/promoted"
ln -sf "$WORK_ROOT/promoted" "$WORK_ROOT/proc/4241/exe"
wait $pid
V=$T/out/scenarios/work-records-dry-p
check "promote mid-L1: L1 VOID" grep -q '^L1 VOID' "$V/verdict.txt"
check "promote mid-L1: says why" grep -q 'the binary changed mid-leg: lamp-update rolled the lamp' "$V/L1.json"
check "promote mid-L1: the run stops" grep -q 'rolled the lamp' "$V/stopped.json"
check "promote mid-L1: no leg after it" test -z "$(grep -E '^(L2|L4) ' "$V/verdict.txt")"
check "promote mid-L1: put back" grep -q '^putback PASS' "$V/verdict.txt"

echo "$fails failure(s)"
exit $((fails > 0))
