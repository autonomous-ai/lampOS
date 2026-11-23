#!/bin/bash
# unit.sh <test> <work dir> <kit>: one kit helper run on the Mac against a fake lamp tree (lamp paths rewritten under
# <work>/root; systemctl stubbed). Prints "UNIT PASS <what>" when the helper does what the test asks, else "UNIT FAIL".
set -u; t=$1; W=$2; KIT=$3; RT=$W/root; mkdir -p $RT/bin $RT/var/lib/lampos-tasks/store $RT/var/lib/lampOS/voice/mute $RT/home $RT/run $RT/proc
# systemctl: lamp-tasks is pid 4243, lamp-voice 4244 (fake /proc/<pid>/environ); no ExecStart
cat > $RT/bin/systemctl <<'S'
#!/bin/sh
case "$*" in *MainPID*lamp-tasks*) echo 4243;; *MainPID*lamp-voice*) echo 4244;; *MainPID*) echo 0;; esac
S
chmod +x $RT/bin/systemctl; export PATH=$RT/bin:$PATH
# a lamp body, its absolute lamp paths moved under $RT
# (LAMPSH_SOCK: the manage helper's socket moved to a short path, as AF_UNIX paths are at most 104 bytes on macOS)
lampsh(){ { printf 'LAMP_USER=orangepi; LOCK_WHO=unit\n'; cat $KIT/row/common.lamp.sh; cat "$@"; } |
  sed -e "s#/run/lampos-skills-manage/manage.sock#${LAMPSH_SOCK:-$RT/run/lampos-skills-manage/manage.sock}#g" -e "s#/tmp/t8180#$RT/tmp/t8180#g" -e "s#/var/lib/#$RT/var/lib/#g" -e "s#/run/#$RT/run/#g" -e "s#/proc/#$RT/proc/#g" -e "s#/home/#$RT/home/#g" -e "s#/opt/#$RT/opt/#g" | sh 2>&1; }
penv(){ mkdir -p $RT/proc/$1; printf 'PATH=/usr/bin\0%s\0' "$2" > $RT/proc/$1/environ; }
app(){ printf '{"engine": "%s", "at": 1}\n' "$1" > $RT/var/lib/lampos-tasks/store/engine-default.json; }
bus(){ mkdir -p $RT/run/lampos-bus/voice/mute; printf '{"at_unix_ms": %s, "seq": 3}\n{"muted": %s, "reason": "user"}' "$(python3 -c 'import time;print(int(time.time()*1000))')" "$1" > $RT/run/lampos-bus/voice/mute/latest; }
rec(){ printf '{"version":1,"clock_id":"wall","muted":true,"through_us":%s,"reason":"timed"}\n' "$1" > $RT/var/lib/lampOS/voice/mute/record.json; }
nowus(){ python3 -c 'import time;print(int(time.time()*1e6))'; }
v(){ if eval "$2"; then echo "UNIT PASS $1"; else echo "UNIT FAIL $1: $(echo "$o" | tr '\n' ' ' | cut -c1-150)"; fi; }
E=$W/engine.sh; echo 'echo "ENGINE=$(engine)"' > $E
case $t in
  engine-env)  penv 4243 LAMP_LAMP_ENGINE=openclaw; o=$(lampsh $E); v "no app file: engine from LAMP_LAMP_ENGINE" '[ "$o" = ENGINE=openclaw ]';;
  engine-app)  penv 4243 LAMP_LAMP_ENGINE=openclaw; app hermes; o=$(lampsh $E); v "the app's choice wins over the env" '[ "$o" = ENGINE=hermes ]';;
  engine-none) penv 4243 OTHER=1; o=$(lampsh $KIT/row/save.lamp.sh); v "no app file and no env: save stops" 'echo "$o" | grep -q "SAVE STOP: no engine-default"';;
  mute-oldstamp) rec $(( $(nowus) - 60000000 )); o=$(lampsh $KIT/row/mute.lamp.sh); v "live unread, a muted record with an old stamp stays on" 'echo "$o" | grep -q "^MUTE FAIL"';;
  mute-timed)  rec $(( $(nowus) + 600000000 )); o=$(lampsh $KIT/row/mute.lamp.sh); v "live unread, record's timed mute still running: on" 'echo "$o" | grep -q "^MUTE FAIL"';;
  mute-live)   rec $(( $(nowus) - 60000000 )); bus true; o=$(lampsh $KIT/row/mute.lamp.sh); v "live voice/mute muted: on" 'echo "$o" | grep -q "^MUTE FAIL: live"';;
  mute-liveoff) rec $(( $(nowus) + 600000000 )); bus false; o=$(lampsh $KIT/row/mute.lamp.sh); v "live voice/mute unmuted wins over the record" 'echo "$o" | grep -q "^MUTE OFF OK (live"';;
  ssh-*)
    # fake ssh: the first $DENY tries print $ERR and exit $RC; then it prints the stdin it got. Calls in $W/sshn.
    cat > $W/fakessh <<'S'
#!/bin/bash
n=$(( $(cat $FW/sshn 2>/dev/null || echo 0) + 1 )); echo $n > $FW/sshn
for a in "$@"; do [ "$a" = -n ] && exec </dev/null; done
in=$(cat); echo "$in" > $FW/stdin.$n
[ $n -le $DENY ] && { echo "$ERR" >&2; exit $RC; }; echo "OK $(echo "$in" | tail -1)"
S
    chmod +x $W/fakessh; export FW=$W
    run(){ ( export SSH=$W/fakessh LAMP_HOST=mock-lamp KIT_ENV= D=$W/kitrow; mkdir -p $W/kitrow; cp $KIT/row/kit.sh $KIT/row/common.lamp.sh $W/kitrow/
      . $W/kitrow/kit.sh; echo "the body" > $W/body; LAMP "" $W/body; echo "rc=$?" ) 2>&1; }
    case $t in
      ssh-retry) export DENY=2 ERR="orangepi@mock-lamp: Permission denied (publickey,password)." RC=255; o=$(run)
        v "refused twice, then in: 3 tries, same body each" '[ "$(cat $W/sshn)" = 3 ] && echo "$o" | grep -q "^OK the body" && echo "$o" | grep -q "rc=0" && cmp -s $W/stdin.1 $W/stdin.3';;
      ssh-giveup) export DENY=9 ERR="orangepi@mock-lamp: Permission denied (publickey,password)." RC=255; o=$(run)
        v "refused every time: 3 tries, then its exit" '[ "$(cat $W/sshn)" = 3 ] && echo "$o" | grep -q "rc=255"';;
      ssh-other) export DENY=9 ERR="ssh: connect to host mock-lamp port 22: Connection refused" RC=255; o=$(run)
        v "another error: 1 try only" '[ "$(cat $W/sshn)" = 1 ] && echo "$o" | grep -q "rc=255" && echo "$o" | grep -q "Connection refused"';;
    esac;;
  # save's Google mail link read (linked, common.lamp.sh) against a fake /var/lib/lampos-services
  linked-*) S=$RT/var/lib/lampos-services; mkdir -p $S; L=$W/linked.sh; echo 'echo "LINKED $(linked)"' > $L
    case $t in
      linked-none) o=$(lampsh $L); v "no store file: google none" '[ "$o" = "LINKED google none (files: none)" ]';;
      linked-expired) printf '{"connector":"gmail-app","access_token":"x","expires_at":1000,"refresh_token":""}' > $S/gmail-app.json
        printf '{"connector":"gmail","refused":true}' > $S/gmail.json; o=$(lampsh $L)
        v "a refused gmail and an ended gmail-app with no refresh: google none" '[ "$o" = "LINKED google none (files: gmail=expired gmail-app=expired)" ]';;
      linked-ok) printf '{"connector":"gmail-app","access_token":"x","expires_at":1000,"refresh_token":"r"}' > $S/gmail-app.json; o=$(lampsh $L)
        v "an ended gmail-app that can be renewed is linked" '[ "$o" = "LINKED google gmail-app (files: gmail-app)" ]';;
    esac;;
  # the row's probe fixtures (C, H, P) load by the product's skill.toml rules (fixcheck.py cites them)
  vars-default)
    # Under set -u a bare self-assignment (NAME=$NAME) stops the kit when the runner left NAME unset (e82d, 22 Nov 15:49).
    o=$(python3 -I -c 'import re,sys
for f in sys.argv[1:]:
  for n,l in enumerate(open(f),1):
    if re.search(r"(^|[; ])([A-Z][A-Z0-9_]*)=\$\2(;|$)",l.rstrip("\n")): print("%s:%d:%s"%(f,n,l.strip()))' $KIT/row/*.sh $KIT/run.sh)
    v "every kit variable has a default (no bare NAME=\$NAME)" '[ -z "$o" ]';;
  idle-*)
    # row/idle.lamp.sh: voice/state on the bus (the voice says it each second): resting 3 s in a row is IDLE; speaking
    # throughout, or a stale state, is NOT IDLE once IDLE_MAX ends. A writer keeps the file fresh while it waits.
    st=${t#idle-}; mkdir -p $RT/run/lampos-bus/voice/state
    ( for i in $(seq 1 16); do s=$st; [ "$st" = turns ] && { [ $i -le 3 ] && s=speaking || s=resting; }
        ms=$(python3 -c 'import time;print(int(time.time()*1000))'); [ "$st" = stale ] && ms=$((ms - 10000)); s=${s%stale}; s=${s:-resting}
        printf '{"at_unix_ms": %s, "seq": %s}\n{"state": "%s"}' $ms $i $s > $RT/run/lampos-bus/voice/state/latest.n; mv $RT/run/lampos-bus/voice/state/latest.n $RT/run/lampos-bus/voice/state/latest; sleep 0.5; done ) &
    wp=$!; sleep 0.3; o=$(IDLE_FOR=2 IDLE_MAX=5 lampsh $KIT/row/idle.lamp.sh); wait $wp
    case $st in resting|turns) v "voice/state $st: IDLE once resting held 2 s" 'echo "$o" | grep -q "^IDLE resting after"';;
                *) v "voice/state $st: NOT IDLE at IDLE_MAX" 'echo "$o" | grep -q "^NOT IDLE"';; esac;;
  gateway-8320)
    # #8320 on stable (no #8323): swap and restore restart OpenClaw's gateway after lamp-control, and check (VERIFY)
    # fails when the gateway's namespace holds an older lamp-control socket than the host's.
    sw=$(grep -c '^gwrestart || ok=0$' $KIT/swap/swap.lamp.sh); rs=$(grep -c 'gwrestart || ok=0; }$' $KIT/swap/restore.lamp.sh)
    gt=$(grep -c 'lamp-openclaw-gateway"$' $KIT/swap/swap.sh); ck=$(grep -c 'why="$why control-sock-stale-in-gateway(#8320)"' $KIT/row/check.lamp.sh)
    dl=$(grep -c 'Delegate=memory cpu pids.*> \$UD_TASKS' $KIT/swap/swap.lamp.sh); dm=$(grep -c 'LAMP_MADE_CODE_ON_AT_ONCE=on.*> \$UD_CONTROL' $KIT/swap/swap.lamp.sh)
    dr=$(grep -c 'for f in \$UD_TASKS \$UD_CONTROL; do \[ -f \$f \] && { rm -f' $KIT/swap/restore.lamp.sh)
    # 23 Nov 11:15: a new gateway meeting the old lock's fresh heartbeat exits; one more start after 95 s from a reset
    # start limit, and a failed gateway is brought back (not skipped as "not active").
    lk=$(grep -c 'grep -q GatewayLockNamespaceError' $KIT/swap/swap-common.lamp.sh); rf=$(grep -c 'systemctl reset-failed \$GW' $KIT/swap/swap-common.lamp.sh)
    o="swap restarts $sw, restore restarts $rs, swap gates $gt, check fails stale $ck, drop-ins $dl $dm removed $dr, lock retry $lk reset $rf"
    v "gateway restarted after lamp-control, stale socket fails VERIFY, #8180's unit lines laid and removed, lock retry" '[ "$o" = "swap restarts 1, restore restarts 1, swap gates 1, check fails stale 1, drop-ins 1 1 removed 1, lock retry 1 reset 2" ]';;
  queue-*)
    # row/queue.lamp.sh (23 Nov batch 1): a kept session in lamp-tasks' close loop ("not closed: working; tried again
    # next") queues every make; it is closed with harness close --now and the loop must stop. No loop: QUEUE CLEAR.
    k=${t#queue-}; LP='tasks: one session: the kept session 27023c29 not closed: working; tried again next'
    printf '{"session": {"name": "lamp-one-hermes", "id": "27023c29"}}\n' > $RT/var/lib/lampos-tasks/store/one-session.json
    # queue-ps (ee17 15667): the record holds no name; Harness's list names the session by its id
    [ $k = ps ] && printf '{"agent": "27023c29-aaaa", "engine": "openclaw", "kept": true}\n' > $RT/var/lib/lampos-tasks/store/one-session.json
    touch $RT/var/lib/lampos-tasks/store/kept-session.token; : > $W/closed
    case $k in clear) : > $W/jl;; *) printf '%s\n%s\n%s\n%s\n' "$LP" "$LP" "$LP" "$LP" > $W/jl;; esac
    # journalctl: the loop lines, gone once harness closed it (unless k=stuck); harness records its call
    printf '#!/bin/sh\nif [ -s %s ] && [ "%s" != stuck ]; then exit 0; fi; cat %s\n' $W/closed $k $W/jl > $RT/bin/journalctl
    printf '#!/bin/sh\nif [ "$1" = ps ]; then echo "{\\"harnesses\\": [{\\"name\\": \\"lamp-one-hermes\\", \\"id\\": \\"27023c29-aaaa\\", \\"state\\": \\"failed\\"}]}"; exit 0; fi\necho "$*" >> %s; echo "{\\"closed\\":true}"\n' $W/closed > $RT/bin/harness
    printf '#!/bin/sh\nwhile [ "$1" != timeout ]; do shift; done; shift 2; exec "$@"\n' > $RT/bin/sudo
    printf '#!/bin/sh\nexit 0\n' > $RT/bin/logger; chmod +x $RT/bin/journalctl $RT/bin/harness $RT/bin/sudo $RT/bin/logger
    o=$(QUEUE_SETTLE=0 lampsh $KIT/row/queue.lamp.sh)
    case $k in
      clear) v "no close loop: QUEUE CLEAR, nothing closed" 'echo "$o" | grep -q "^QUEUE CLEAR" && [ ! -s $W/closed ]';;
      ps)    v "no name in the record: named by id from harness ps, closed, QUEUE CLOSED" 'echo "$o" | grep -q "^QUEUE CLOSED 27023c29 (lamp-one-hermes)" && grep -q "^close lamp-one-hermes " $W/closed';;
      loop)  v "a close loop: the session closed by name with --now, QUEUE CLOSED" 'echo "$o" | grep -q "^QUEUE CLOSED 27023c29 (lamp-one-hermes)" && grep -q "^close lamp-one-hermes --program-token-file .* --now --json$" $W/closed';;
      stuck) v "the loop goes on after the close: QUEUE STUCK" 'echo "$o" | grep -q "^QUEUE STUCK"';;
    esac;;
  namenear)
    # The Mac voice's "Hey Lamp" (#8318, e82d 22 Nov): save lays LAMP_NAME_NEAR=off as the row's /run drop-in and checks
    # it took; unrow removes that drop-in and checks lamp-voice is back to the product's value.
    sv=$(grep -c 'Environment=LAMP_NAME_NEAR=off.* > \$ND_NAMENEAR' $KIT/row/save.lamp.sh); ck=$(grep -c '\[ "\$nn" = off \] ||' $KIT/row/save.lamp.sh)
    ur=$(grep -c 'for f in \$ND_RUNON \$ND_NAMED \$ND_NAMENEAR; do' $KIT/row/unrow.lamp.sh); uc=$(grep -c 'FAIL LAMP_NAME_NEAR still off' $KIT/row/unrow.lamp.sh)
    nd=$(grep -c '^ND_NAMENEAR=/run/systemd/system/lamp-voice.service.d/' $KIT/row/common.lamp.sh)
    o="save lays $sv, checks $ck; unrow removes $ur, checks $uc; common defines $nd"
    v "LAMP_NAME_NEAR=off is laid by save and taken away by unrow" '[ "$sv$ck$ur$uc$nd" = 11111 ]';;
  keep-verdicts)
    # A split rerun (rows of 20 min or less, 23 Nov) keeps the earlier runs' verdicts: save clears verdicts.env only
    # without KEEP_VERDICTS=1, so a later G still reads A, B and D.
    o=$(awk '/^  save\)/,/;;$/' $KIT/run.sh | grep -n 'verdicts.env')
    v "save keeps verdicts.env with KEEP_VERDICTS=1, and clears it otherwise" \
      'echo "$o" | grep -q "KEEP_VERDICTS:-}\" = 1 \] || rm -f \$L/verdicts.env" && [ "$(echo "$o" | wc -l | tr -d " ")" = 1 ]';;
  fixtures) o=$(python3 -I $KIT/mock/row/fixcheck.py $KIT/row/probe); rc=$?
    v "the six probes (lemon, slow, fork; #8437 quick, pear, rest) load by the product's rules" '[ $rc = 0 ] && [ "$(echo "$o" | grep -c " OK$")" = 6 ]';;
  # every fresh-turn clip opens with the lamp's name said as a call: each clip row.sh ASKs or MAKEs, and every clip in
  # clips.txt but the answers to the lamp's own question (ANSWERS: YES)
  clips-call) ANSWERS=" YES "; CL=$KIT/row/clips/clips.txt
    ids=$( { grep -oE '(ASK|MAKE) [A-Z][A-Z0-9]+' $KIT/row/row.sh | awk '{print $2}'; cut -f1 $CL; } | sort -u)
    bad=; for id in $ids; do case "$ANSWERS" in *" $id "*) continue;; esac
      tx=$(awk -F'\t' -v c=$id '$1 == c {print $2}' $CL); case "$tx" in "Hey Lamp, "*) ;; *) bad="$bad $id='${tx:-no text}'";; esac; done
    o="checked $(echo $ids | wc -w | tr -d ' ') clips; not a call:${bad:- none}"
    v "every fresh-turn clip opens with 'Hey Lamp,'" '[ -z "$bad" ] && [ -n "$ids" ]';;
  push-probe)
    # Legs C, H, P's fixtures by the product's push install (thirteen/kit-8237's stage, ported): push.sh packs with the
    # SDK; probe.lamp.sh writes push.json + package.tar in lamp-control's push folder and asks the manage helper
    # {"install_pushed": SHA} with the one-request ANY_ASKER drop-in laid, then removed; unrow asks
    # {"remove_pushed": {client, name}} and checks the fixture and the drop-in are gone. A fake helper (python) on the
    # socket records each request, what it found in the push folder and whether the drop-in was there.
    SD=$(mktemp -d /tmp/u8180.XXXXXX); export LAMPSH_SOCK=$SD/m.sock; DEV=$RT/var/lib/lampOS/skills-dev
    AD=$RT/run/systemd/system/lamp-skills-manage@.service.d/zz-row-8180-anyasker.conf; Y=$RT/var/lib/lampos-control/task-yes.json
    mkdir -p $DEV $RT/var/lib/lampOS/skills $RT/var/lib/lampos-control $RT/tmp/t8180-probe $RT/home/orangepi; echo '{"yes":[]}' > $Y
    printf '#!/bin/sh\nshasum -a 256 "$@"\n' > $RT/bin/sha256sum; chmod +x $RT/bin/sha256sum; penv 4244 PATH=/usr/bin
    python3 -I $KIT/mock/row/fakehelper.py $LAMPSH_SOCK $RT/var/lib/lampos-control/push $DEV $AD $Y $W/helper.log &
    hp=$!; for i in 1 2 3 4 5 6 7 8 9 10; do [ -S $LAMPSH_SOCK ] && break; sleep 0.2; done
    m=$( (unset D; LAMP_HOST=mock-lamp KIT_ENV= bash $KIT/row/push.sh pack $KIT/row/probe/lemon-probe $RT/tmp/t8180-probe/package.tar) 2>&1 )
    printf 'OP=push SUM=%s NAME=%s\n' "${m#* }" "${m% *}" > $W/pre.sh
    o1=$(lampsh $W/pre.sh $KIT/row/probe.lamp.sh); ad1=$([ -f $AD ] && echo laid || echo gone)
    left=$(ls $RT/var/lib/lampos-control/push 2>/dev/null | tr '\n' ' '); pushed=$([ -d $DEV/lemon-probe ] && echo yes || echo no)
    o2=$(lampsh $KIT/row/unrow.lamp.sh); { kill $hp; wait $hp; } 2>/dev/null; rm -rf $SD
    r=$(python3 -I -c 'import json,sys
L=[json.loads(l) for l in open(sys.argv[1])]; nm, sm = sys.argv[2], sys.argv[3]
i=L[0]; p=i.get("push", {})
ok1 = i["q"] == {"install_pushed": sm} and i["dropin"] and i["tar"] == sm and p.get("name") == nm and p.get("client") == "c_row8180" and p.get("grants") == ["skills", "speak"] and isinstance(p.get("at_unix_ms"), int)
ok2 = len(L) == 2 and L[1]["q"] == {"remove_pushed": {"client": "c_row8180", "name": nm}} and L[1]["dropin"]
print("install %s remove %s" % (ok1, ok2))' $W/helper.log "${m% *}" "${m#* }" 2>&1)
    o="pack '$m'; $r; pushed $pushed; drop-in after push $ad1; push folder left '$left'; $(echo "$o1" | grep -E '^PROBE' | tr '\n' ' ')/ $(echo "$o2" | grep -E 'pushed fixtures|YES-FILE|^UNROW' | tr '\n' ' ')"
    v "fixture pushed (push.json + package.tar, install_pushed, drop-in laid then removed), unrow takes it off by remove_pushed" \
      '[ "$r" = "install True remove True" ] && [ $pushed = yes ] && [ $ad1 = gone ] && [ -z "$left" ] && echo "$o1" | grep -q "^PROBE INSTALLED" && echo "$o2" | grep -q "^PASS pushed fixtures gone" && echo "$o2" | grep -q "^YES-FILE put back" && echo "$o2" | grep -q "^UNROW OK" && [ ! -d $DEV/lemon-probe ] && [ ! -f $AD ] && grep -q "added $AD" $RT/home/orangepi/changes.txt && grep -q "removed $AD" $RT/home/orangepi/changes.txt && [ "$(cat $Y)" = "{\"yes\":[]}" ]';;
  volume-*)
    # save records the speaker's volume (kept-volume + the bus's speaker/volume, read only), unrow compares: the same
    # is PASS; a change is FAIL and UNROW NOT OK (never set back by the kit). Only the volume part of save runs here.
    K=$RT/var/lib/lampos-output-caps/kept-volume; mkdir -p ${K%/*} $RT/run/lampos-bus/speaker/volume; penv 4244 PATH=/usr/bin
    bv(){ printf '{"at_unix_ms": 1, "seq": 2}\n{"volume": %s, "muted": false}' $1 > $RT/run/lampos-bus/speaker/volume/latest; }
    echo 40 > $K; bv 40; grep -E 'volnow > \$R/volume.before' $KIT/row/save.lamp.sh > $W/vsave.sh
    o0=$(lampsh $W/vsave.sh); [ "${t#volume-}" = moved ] && { echo 70 > $K; bv 70; }
    o=$(lampsh $KIT/row/unrow.lamp.sh); o="$o0 / $(echo "$o" | grep -E 'volume|^UNROW' | tr '\n' ' ')"
    case $t in
      volume-same)  v "volume as saved: PASS, UNROW OK" 'echo "$o" | grep -q "VOLUME saved kept=40 bus=40 muted=false" && echo "$o" | grep -q "PASS volume kept=40 bus=40 muted=false = saved" && echo "$o" | grep -q "UNROW OK"';;
      volume-moved) v "volume moved: FAIL, UNROW NOT OK, not set back" 'echo "$o" | grep -q "FAIL volume moved kept=40 bus=40 muted=false -> kept=70 bus=70 muted=false (a leg" && echo "$o" | grep -q "UNROW NOT OK" && [ "$(cat $K)" = 70 ]';;
    esac;;
  *) echo "UNIT FAIL no such test $t";;
esac
