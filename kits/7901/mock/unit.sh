#!/bin/bash
# unit.sh <test> <work dir> <kit>: one kit helper run on the Mac against a fake lamp tree (lamp paths rewritten under
# <work>/root; systemctl stubbed). Prints "UNIT PASS <what>" when the helper does what the test asks, else "UNIT FAIL".
# Ported with the #8180 e82d kit's fixes: engine() falls back to LAMP_LAMP_ENGINE, the mute check reads live voice/mute
# first and never takes through_us for an end, SSHR retries a refused login only; plus the Mac volume is only read.
set -u; t=$1; W=$2; KIT=$3; RT=$W/root; mkdir -p $RT/bin $RT/var/lib/lampos-tasks/store $RT/var/lib/lampOS/voice/mute $RT/home $RT/run $RT/proc
# systemctl: lamp-tasks is pid 4243, lamp-voice 4244 (fake /proc/<pid>/environ); nothing active
cat > $RT/bin/systemctl <<'S'
#!/bin/sh
case "$*" in *MainPID*lamp-tasks*) echo 4243;; *MainPID*lamp-voice*) echo 4244;; *MainPID*) echo 0;; esac
S
chmod +x $RT/bin/systemctl; export PATH=$RT/bin:$PATH
# a lamp body, its absolute lamp paths moved under $RT
lampsh(){ { printf 'LAMP_USER=mockuser; LOCK_WHO=unit\n'; cat $KIT/row/common.lamp.sh; cat "$@"; } |
  sed -e "s#/var/lib/#$RT/var/lib/#g" -e "s#/run/#$RT/run/#g" -e "s#/proc/#$RT/proc/#g" -e "s#/home/#$RT/home/#g" -e "s#/opt/#$RT/opt/#g" | sh 2>&1; }
penv(){ mkdir -p $RT/proc/$1; printf 'PATH=/usr/bin\0%s\0' "$2" > $RT/proc/$1/environ; }
app(){ printf '{"engine": "%s", "at": 1}\n' "$1" > $RT/var/lib/lampos-tasks/store/engine-default.json; }
bus(){ mkdir -p $RT/run/lampos-bus/voice/mute; printf '{"at_unix_ms": %s, "seq": 3}\n{"muted": %s, "reason": "user"}' "$(python3 -c 'import time;print(int(time.time()*1000))')" "$1" > $RT/run/lampos-bus/voice/mute/latest; }
rec(){ printf '{"version":1,"clock_id":"wall","muted":%s,"through_us":%s,"reason":"timed"}\n' "${2:-true}" "$1" > $RT/var/lib/lampOS/voice/mute/record.json; }
nowus(){ python3 -c 'import time;print(int(time.time()*1e6))'; }
v(){ if eval "$2"; then echo "UNIT PASS $1"; else echo "UNIT FAIL $1: $(echo "$o" | tr '\n' ' ' | cut -c1-150)"; fi; }
E=$W/engine.sh; echo 'echo "ENGINE=$(engine)"' > $E
case $t in
  engine-env)  penv 4243 LAMP_LAMP_ENGINE=openclaw; o=$(lampsh $E); v "no app file: engine from lamp-tasks' LAMP_LAMP_ENGINE" '[ "$o" = ENGINE=openclaw ]';;
  engine-voice) penv 4243 OTHER=1; penv 4244 LAMP_LAMP_ENGINE=openclaw; o=$(lampsh $E); v "no app file, none in lamp-tasks: engine from lamp-voice's" '[ "$o" = ENGINE=openclaw ]';;
  engine-app)  penv 4243 LAMP_LAMP_ENGINE=openclaw; app hermes; o=$(lampsh $E); v "the app's choice wins over the env" '[ "$o" = ENGINE=hermes ]';;
  engine-none) penv 4243 OTHER=1; penv 4244 OTHER=1; o=$(lampsh $KIT/row/save.lamp.sh); v "no app file and no env: save stops" 'echo "$o" | grep -q "SAVE STOP: the default agent is .., not openclaw"';;
  mute-oldstamp) rec $(( $(nowus) - 60000000 )); o=$(lampsh $KIT/row/mute.lamp.sh); v "live unread, a muted record with a past stamp stays on" 'echo "$o" | grep -q "^MUTE FAIL"';;
  mute-timed)  rec $(( $(nowus) + 600000000 )); o=$(lampsh $KIT/row/mute.lamp.sh); v "live unread, a muted record with a later stamp: on" 'echo "$o" | grep -q "^MUTE FAIL"';;
  mute-recoff) rec $(( $(nowus) - 60000000 )) false; o=$(lampsh $KIT/row/mute.lamp.sh); v "live unread, record muted=false: off" 'echo "$o" | grep -q "^MUTE OFF OK (live voice/mute unread"';;
  mute-live)   rec $(( $(nowus) - 60000000 )) false; bus true; o=$(lampsh $KIT/row/mute.lamp.sh); v "live voice/mute muted wins over an unmuted record" 'echo "$o" | grep -q "^MUTE FAIL: live"';;
  mute-liveoff) rec $(( $(nowus) + 600000000 )); bus false; o=$(lampsh $KIT/row/mute.lamp.sh); v "live voice/mute unmuted wins over the record" 'echo "$o" | grep -q "^MUTE OFF OK (live"';;
  namenear)
    # The Mac voice's "Hey Lamp" (#8318, #8180's e82d run 22 Nov): save lays LAMP_NAME_NEAR=off as the row's /run drop-in
    # and checks it took; unrow removes that drop-in and checks lamp-voice is back to the product's value.
    sv=$(grep -c 'Environment=LAMP_NAME_NEAR=off.* > \$ND_NAMENEAR' $KIT/row/save.lamp.sh); ck=$(grep -c '\[ "\$nn" = off \] ||' $KIT/row/save.lamp.sh)
    ur=$(grep -c 'for f in \$ND_VOICE \$ND_CONTROL \$ND_NAMENEAR; do' $KIT/row/unrow.lamp.sh); uc=$(grep -c 'FAIL LAMP_NAME_NEAR still off' $KIT/row/unrow.lamp.sh)
    nd=$(grep -c '^ND_NAMENEAR=/run/systemd/system/lamp-voice.service.d/' $KIT/row/common.lamp.sh)
    o="save lays $sv, checks $ck; unrow removes $ur, checks $uc; common defines $nd"
    v "save lays NAME_NEAR=off and checks it; unrow removes it and checks it" '[ "$o" = "save lays 1, checks 1; unrow removes 1, checks 1; common defines 1" ]';;
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
    run(){ ( export SSH=$W/fakessh LAMP=mock LAMP_HOST=mock-lamp LAMP_USER=mockuser KIT_ENV= KIT_LL= D=$W/kitrow; mkdir -p $W/kitrow; cp $KIT/row/kit.sh $KIT/row/common.lamp.sh $W/kitrow/
      . $W/kitrow/kit.sh; echo "the body" > $W/body; LAMP "" $W/body; echo "rc=$?" ) 2>&1; }
    DENIED="mockuser@mock-lamp: Permission denied (publickey,password)."
    case $t in
      ssh-retry) export DENY=2 ERR="$DENIED" RC=255; o=$(run)
        v "refused twice, then in: 3 tries, same body each" '[ "$(cat $W/sshn)" = 3 ] && echo "$o" | grep -q "^OK the body" && echo "$o" | grep -q "rc=0" && cmp -s $W/stdin.1 $W/stdin.3';;
      ssh-sshpass) export DENY=1 ERR="$DENIED" RC=5; o=$(run)
        v "sshpass refused once (exit 5), then in: 2 tries" '[ "$(cat $W/sshn)" = 2 ] && echo "$o" | grep -q "rc=0"';;
      ssh-giveup) export DENY=9 ERR="$DENIED" RC=255; o=$(run)
        v "refused every time: 3 tries, then its exit" '[ "$(cat $W/sshn)" = 3 ] && echo "$o" | grep -q "rc=255"';;
      ssh-other) export DENY=9 ERR="ssh: connect to host mock-lamp port 22: Connection refused" RC=255; o=$(run)
        v "another error: 1 try only" '[ "$(cat $W/sshn)" = 1 ] && echo "$o" | grep -q "rc=255" && echo "$o" | grep -q "Connection refused"';;
    esac;;
  # the Mac volume is read only: 70 plays, 71 stops, and no kit script sets a volume
  macvol) MB=$KIT/mock/bin; o=$( ( export LAMP=mock LAMP_HOST=mock-lamp LAMP_USER=mockuser KIT_ENV= KIT_LL= D=$W/kitrow SSH=true MOCK_LOG=$W/osa.log; mkdir -p $W/kitrow; cp $KIT/row/kit.sh $W/kitrow/
      . $W/kitrow/kit.sh; OSASCRIPT=$MB/osascript MOCK_MACVOL=70 VOLOK; echo "v70=$?"; OSASCRIPT=$MB/osascript MOCK_MACVOL=71 VOLOK; echo "v71=$?" ) 2>&1 )
    sets=$(grep -rlE 'set volume|output volume [0-9]|osascript .*-e .*set ' $KIT/run.sh $KIT/row $KIT/optional 2>/dev/null)
    v "Mac volume 70 plays, 71 stops, no script sets it" 'echo "$o" | grep -q "v70=0" && echo "$o" | grep -q "v71=2" && [ -z "$sets" ]';;
  # legs S and K's pushed fixtures load by the product's skill.toml rules (fixcheck.py cites them)
  fixtures) o=$(python3 -I $KIT/mock/fixcheck.py $KIT/row/fixtures); rc=$?; echo "$o" | sed 's/^/  /'
    v "every pushed fixture loads by the loader's rules: $(echo "$o" | tr '\n' ' ')" '[ $rc = 0 ] && [ "$(echo "$o" | grep -c " OK$")" = 2 ]';;
  push-k)
    # Leg K's fixture by the product's push install, no pairing (the #8180 ee17 kit's, ported): row.sh's K goes by PUSHL
    # (push.sh), not push.py; push.sh packs key-probe with this worktree's SDK; push.lamp.sh writes push.json +
    # package.tar in lamp-control's push folder and asks the manage helper {"install_pushed": SHA} with the one-request
    # ANY_ASKER drop-in laid, then removed; unrow asks {"remove_pushed": {client, name}} and checks the fixture and the
    # drop-in are gone. A fake helper (python) on the socket records each request, what it found in the push folder
    # and whether the drop-in was there.
    SD=$(mktemp -d /tmp/u7901.XXXXXX); SOCK=$SD/m.sock; DEV=$RT/var/lib/lampOS/skills-dev
    AD=$RT/run/systemd/system/lamp-skills-manage@.service.d/zz-row-7901-anyasker.conf; Y=$RT/var/lib/lampos-control/task-yes.json
    mkdir -p $DEV $RT/var/lib/lampOS/skills $RT/var/lib/lampos-control $RT/tmp/t7901-push $RT/home/mockuser; echo '{"yes":[]}' > $Y
    printf '#!/bin/sh\nshasum -a 256 "$@"\n' > $RT/bin/sha256sum; chmod +x $RT/bin/sha256sum; penv 4244 PATH=/usr/bin
    python3 -I $KIT/mock/fakehelper.py $SOCK $RT/var/lib/lampos-control/push $DEV $AD $Y $W/helper.log &
    hp=$!; for i in 1 2 3 4 5 6 7 8 9 10; do [ -S $SOCK ] && break; sleep 0.2; done
    m=$( (unset D; LAMP=mock LAMP_HOST=mock-lamp LAMP_USER=mockuser KIT_ENV= KIT_LL= bash $KIT/row/push.sh pack $KIT/row/fixtures/key-probe $RT/tmp/t7901-push/package.tar) 2>&1 )
    printf 'OP=push SUM=%s NAME=%s\n' "${m#* }" "${m% *}" > $W/pre.sh
    # (the helper's socket moved to a short path: AF_UNIX paths are at most 104 bytes on macOS)
    psh(){ { printf 'LAMP_USER=mockuser; LOCK_WHO=unit\n'; cat $KIT/row/common.lamp.sh "$@"; } |
      sed -e "s#/run/lampos-skills-manage/manage.sock#$SOCK#g" -e "s#/tmp/t7901-push#$RT/tmp/t7901-push#g" -e "s#/var/lib/#$RT/var/lib/#g" -e "s#/run/#$RT/run/#g" -e "s#/proc/#$RT/proc/#g" -e "s#/home/#$RT/home/#g" -e "s#/opt/#$RT/opt/#g" | sh 2>&1; }
    o1=$(psh $W/pre.sh $KIT/row/push.lamp.sh); ad1=$([ -f $AD ] && echo laid || echo gone)
    left=$(ls $RT/var/lib/lampos-control/push 2>/dev/null | tr '\n' ' '); pushed=$([ -d $DEV/key-probe ] && echo yes || echo no)
    o2=$(psh $KIT/row/unrow.lamp.sh); { kill $hp; wait $hp; } 2>/dev/null; rm -rf $SD
    r=$(python3 -I -c 'import json,sys
L=[json.loads(l) for l in open(sys.argv[1])]; nm, sm = sys.argv[2], sys.argv[3]
i=L[0]; p=i.get("push", {})
ok1 = i["q"] == {"install_pushed": sm} and i["dropin"] and i["tar"] == sm and p.get("name") == nm and p.get("client") == "c_row7901" and p.get("grants") == ["skills", "speak"] and isinstance(p.get("at_unix_ms"), int)
ok2 = len(L) == 2 and L[1]["q"] == {"remove_pushed": {"client": "c_row7901", "name": nm}} and L[1]["dropin"]
print("install %s remove %s" % (ok1, ok2))' $W/helper.log "${m% *}" "${m#* }" 2>&1)
    kr=$(sed -n '/^legK(){/,/^RT=/p' $KIT/row/row.sh | grep -cE '^  PUSHL \$sk \|\|'); kp=$(sed -n '/^legK(){/,/^RT=/p' $KIT/row/row.sh | grep -cE '^  PUSH \$sk')
    o="K route PUSHL=$kr push.py=$kp; pack '$m'; $r; pushed $pushed; drop-in after push $ad1; push folder left '$left'; $(echo "$o1" | grep -E '^PUSH' | tr '\n' ' ')/ $(echo "$o2" | grep -E 'pushed fixtures|YES-FILE|^UNROW' | tr '\n' ' ')"
    v "K pushed with no pairing (push.json + package.tar, install_pushed, drop-in laid then removed), unrow takes it off by remove_pushed" \
      '[ "$kr" -ge 1 ] && [ "$kp" = 0 ] && [ "$r" = "install True remove True" ] && [ $pushed = yes ] && [ $ad1 = gone ] && [ -z "$left" ] && echo "$o1" | grep -q "^PUSH INSTALLED" && echo "$o2" | grep -q "^PASS pushed fixtures gone" && echo "$o2" | grep -q "^YES-FILE put back" && echo "$o2" | grep -q "^UNROW OK" && [ ! -d $DEV/key-probe ] && [ ! -f $AD ] && grep -q "added $AD" $RT/home/mockuser/changes.txt && grep -q "removed $AD" $RT/home/mockuser/changes.txt && [ "$(cat $Y)" = "{\"yes\":[]}" ]';;
  *) echo "UNIT FAIL no such test $t";;
esac
