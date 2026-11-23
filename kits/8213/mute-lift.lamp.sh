# lamp, root sh (common first). #8156 `mute` step, the user's yes for this ROW only (21 Nov): records the person's mute,
# then lifts it in software. The app cannot (cloud answer.rs: mic.set refused) and the hand controls need a hand, so:
# lamp-voice stopped cleanly (it writes its live latch), the record kept at $R/mute-record.before, written unmuted with
# its watermark at now (as MuteRecord::clear_after_explicit_user_request does), lamp-voice started (1 start).
# Never the speaker volume. Prints MUTE NOT MUTED | MUTE LIFTED OK | MUTE STOP <why> | MUTE LIFT FAIL <why>.
cd /
L0=$(mute_live); echo "$L0"
echo "$L0" | grep -q 'physical_muted=True' && { echo "MUTE STOP: the physical slide is closed (a hand only)"; exit 3; }
if echo "$L0" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.'; then echo "MUTE NOT MUTED (live voice/mute says unmuted; nothing changed)"; exit 0; fi
for x in pending replacement; do [ -e $MR.$x ] && { echo "MUTE STOP: $MR.$x present (a save in flight or cut)"; exit 3; }; done
systemctl stop lamp-voice; n "mute step: lamp-voice stopped cleanly to read the person's mute (user's yes for this ROW)"
cp -p $MR $R/mute-record.before 2>/dev/null || : > $R/mute-record.before
echo "RECORD_BEFORE $(tr -d '\n' < $R/mute-record.before)"
if [ -s $R/mute-record.before ] && ! grep -qE '"muted" *: *true' $R/mute-record.before; then
  echo "LIFTED none: the stopped record is unmuted"; touch $R/mute-lifted.none
else
  python3 -I - $MR <<'P' || { systemctl start lamp-voice; echo "MUTE LIFT FAIL: record not written; lamp-voice started as it was"; exit 4; }
import json, os, sys, time
p = sys.argv[1]; r = json.load(open(p)); st = os.stat(p)
now = time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1000
r["muted"] = False; r.pop("reason", None); r["through_us"] = max(int(r.get("through_us", 0)), now)
t = p + ".t8156"; fd = os.open(t, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.write(fd, json.dumps(r, separators=(",", ":")).encode()); os.fsync(fd); os.close(fd)
os.chown(t, st.st_uid, st.st_gid); os.replace(t, p)
d = os.open(os.path.dirname(p), os.O_RDONLY); os.fsync(d); os.close(d)
print("RECORD_NOW " + open(p).read())
P
  n "mute step: $MR written muted:false (copy $R/mute-record.before); the user's yes for this ROW only"
fi
CUR=$(journalctl -n 0 --show-cursor -o cat | sed -n 's/^-- cursor: //p')
systemctl start lamp-voice; n "mute step: lamp-voice started"
s=0; while [ $s -lt 60 ]; do sleep 2; s=$((s + 2)); L1=$(mute_live)
  echo "$L1" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.' && break; done
echo "$L1"; journalctl -u lamp-voice --after-cursor "$CUR" --no-pager -o cat | grep -iE "mute|restart guard" | tail -3 | cut -c1-160 | sed 's/^/J /'
echo "$L1" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.' && echo "MUTE LIFTED OK after ${s}s" || echo "MUTE LIFT FAIL: voice/mute not unmuted within 60 s"
