# lamp, root sh (common first). #6449 `micon` step, on the lamp's person's yes for THIS ROW only (run.sh MIC_YES=yes): records the person's mute,
# then lifts it in software. The app cannot (cloud answer.rs: mic.set refused) and the hand controls need a hand, so:
# lamp-voice stopped cleanly (it writes its live latch), the record kept at $R/mute-record.saved, written unmuted with
# its watermark at now (as MuteRecord::clear_after_explicit_user_request does), lamp-voice started (1 start).
# Never the speaker volume. Prints MUTE NOT MUTED | MUTE LIFTED OK | MUTE STOP <why> | MUTE LIFT FAIL <why>.
cd /
echo "MIC_BEFORE $(tr -d "\n" < $MR 2>/dev/null)"
L0=$(mute_live); echo "$L0"
echo "$L0" | grep -q 'physical_muted=True' && { echo "MUTE STOP: the physical slide is closed (a hand only)"; exit 3; }
if echo "$L0" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.'; then
  # A resumed run (micon again after a lift by this kit): the person's muted record is still kept and not put back yet,
  # so it is the kit's lift, and micoff must still put it back. micoff renames the record once it is back.
  if [ -s $R/mute-record.saved ] && [ ! -e $R/mute-lifted.none ] && grep -qE '"muted" *: *true' $R/mute-record.saved; then
    echo "MUTE LIFTED OK (lifted earlier; record kept at $R/mute-record.saved; nothing changed now)"; exit 0; fi
  echo "MUTE NOT MUTED (live voice/mute says unmuted; nothing changed)"; exit 0; fi
for x in pending replacement; do [ -e $MR.$x ] && { echo "MUTE STOP: $MR.$x present (a save in flight or cut)"; exit 3; }; done
systemctl stop lamp-voice; n "mute step: lamp-voice stopped cleanly to read the person's mute (the yes for this ROW)"
# A muted record already kept and not put back yet (micoff renames it once back) is the person's own, from this ROW's
# first micon: a lift again after the reboot leg must not take the kit's own state for the person's.
if [ -s $R/mute-record.saved ] && [ ! -e $R/mute-lifted.none ] && grep -qE '"muted" *: *true' $R/mute-record.saved; then
  echo "RECORD_KEPT the person's record from this ROW's first micon stays at $R/mute-record.saved"
else
  rm -f $R/mute-lifted.none $R/mute-record.back   # stale markers from an earlier run must not make micoff skip this lift
  cp -p $MR $R/mute-record.saved 2>/dev/null || : > $R/mute-record.saved
fi
echo "RECORD_BEFORE $(tr -d '\n' < $R/mute-record.saved)"
if [ -s $R/mute-record.saved ] && ! grep -qE '"muted" *: *true' $R/mute-record.saved; then
  echo "LIFTED none: the stopped record is unmuted"; touch $R/mute-lifted.none
else
  python3 -I - $MR <<'P' || { systemctl start lamp-voice; echo "MUTE LIFT FAIL: record not written; lamp-voice started as it was"; exit 4; }
import json, os, sys, time
p = sys.argv[1]; r = json.load(open(p)); st = os.stat(p)
now = time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1000
r["muted"] = False; r.pop("reason", None); r["through_us"] = max(int(r.get("through_us", 0)), now)
t = p + ".t6449"; fd = os.open(t, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.write(fd, json.dumps(r, separators=(",", ":")).encode()); os.fsync(fd); os.close(fd)
os.chown(t, st.st_uid, st.st_gid); os.replace(t, p)
d = os.open(os.path.dirname(p), os.O_RDONLY); os.fsync(d); os.close(d)
print("RECORD_NOW " + open(p).read())
P
  n "mute step: $MR written muted:false (copy $R/mute-record.saved); the yes for this ROW only"
fi
CUR=$(journalctl -n 0 --show-cursor -o cat | sed -n 's/^-- cursor: //p')
systemctl start lamp-voice; n "mute step: lamp-voice started"
s=0; while [ $s -lt 60 ]; do sleep 2; s=$((s + 2)); L1=$(mute_live)
  echo "$L1" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.' && break; done
echo "$L1"; journalctl -u lamp-voice --after-cursor "$CUR" --no-pager -o cat | grep -iE "mute|restart guard" | tail -3 | cut -c1-160 | sed 's/^/J /'
echo "$L1" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.' && echo "MUTE LIFTED OK after ${s}s" || echo "MUTE LIFT FAIL: voice/mute not unmuted within 60 s"
echo "MIC_AFTER $(tr -d "
" < $MR 2>/dev/null)"
