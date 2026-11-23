# lamp, root sh (common first; then HV= CLI_SHA= CLI_SIZE= NOTIFY_SHA= NOTIFY_SIZE= from the Mac's hfetch step: harness/meta.env).
# #8162 put-back: Harness back to stable's pin (HV) by hand after round 2 (lamp-update never steps down, #5957), as lamp-update moves it
# (harness_install.rs upgrade): lamp-tasks stopped, `harness stop` as lampos, the files swapped with a copy kept, the
# record set `installed` 0.6.75, lamp-tasks started (it starts the daemon). 1 start. Prints HDOWN OK | HDOWN FAIL <why>.
cd /
I=/tmp/t8162h; HD=$HHOME/.harness/cli
chk(){ [ "$(sha256sum $1 | cut -d' ' -f1)" = "$2" ] && [ "$(stat -c %s $1)" = "$3" ]; }
chk $I/cli.js $CLI_SHA $CLI_SIZE && chk $I/notify.mjs $NOTIFY_SHA $NOTIFY_SIZE || { echo "HDOWN FAIL: copied files' sha256/size differ"; exit 3; }
echo "ok on the lamp: cli.js $CLI_SIZE, notify.mjs $NOTIFY_SIZE"
OWN=$(stat -c %U:%G $HD/cli.js); U=${OWN%%:*}; echo "HARNESS before $(hver); files owned by $OWN"; hrec
cat $HD/package.json 2>/dev/null | tr -d '\n' | cut -c1-120; echo
B=$R/harness-before; mkdir -p $B; cp -p $HD/cli.js $HD/notify.mjs $B/; cp -p $REC/harness-install.json $B/
systemctl stop lamp-tasks; hcli stop 2>&1 | tail -1 | cut -c1-120
n "hdown: lamp-tasks stopped and harness stop (as lamp-update moves Harness); copies in $B"
chmod 755 $I; chmod 644 $I/cli.js $I/notify.mjs
for f in cli.js notify.mjs; do runuser -u $U -- install -m 644 $I/$f $HD/$f.t8162 && runuser -u $U -- mv $HD/$f.t8162 $HD/$f || { echo "HDOWN FAIL: $f not swapped"; why=swap; }; done
chk $HD/cli.js $CLI_SHA $CLI_SIZE && chk $HD/notify.mjs $NOTIFY_SHA $NOTIFY_SIZE || why="${why:-} installed-files-differ"
python3 -I - $REC/harness-install.json "$HV" <<'P'
import json, os, sys, time
p, v = sys.argv[1], sys.argv[2]; st = os.stat(p); r = json.load(open(p))
r.update(install="installed", version=v, pinned=True, revision=int(r.get("revision", 0)) + 1, at_unix_ms=int(time.time() * 1000),
         message="Harness is installed. Sign in to hand tasks over.")
for k in ("error", "from_version"): r.pop(k, None)
t = p + ".t8162"; fd = os.open(t, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600); os.write(fd, json.dumps(r).encode()); os.fsync(fd); os.close(fd)
os.chown(t, st.st_uid, st.st_gid); os.replace(t, p)
P
hrec
CUR=$(journalctl -n 0 --show-cursor -o cat | sed -n 's/^-- cursor: //p')
systemctl start lamp-tasks
n "hdown: Harness cli.js+notify.mjs set to $HV (sha256 and size checked on the Mac and here), record installed $HV, lamp-tasks started"
s=0; while [ $s -lt 120 ]; do sleep 5; s=$((s + 5))
  journalctl -u lamp-tasks --after-cursor "$CUR" --no-pager -o cat | grep -qE "one session: kept session (live|new|reused)" && break; done
v=$(hver); echo "HARNESS after $v"
K=$(journalctl -u lamp-tasks --after-cursor "$CUR" --no-pager -o cat | grep -E "one session: kept session (live|new|reused)" | tail -1 | cut -c1-140); echo "J ${K:-no kept session line in 120 s}"
echo "$v" | grep -qF "$HV" || why="${why:-} version-not-$HV"
[ -n "$K" ] || why="${why:-} kept-session-not-reopened"
rm -rf $I
[ -z "${why:-}" ] && echo "HDOWN OK" || echo "HDOWN FAIL:$why"
