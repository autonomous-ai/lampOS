# lamp, root sh (common first). #8162 `micoff` step (the 8180 kit's micoff.lamp.sh): the mute recorded by micon.lamp.sh put back (1 start):
# lamp-voice stopped cleanly, $R/mute-record.saved written back (its watermark never older than the record's now),
# lamp-voice started, voice/mute read muted again. Prints MUTE BACK OK | MUTE BACK FAIL <why>.
cd /
echo "MIC_BEFORE $(tr -d "\n" < $MR 2>/dev/null)"
[ ! -e $R/mute-record.saved ] && [ -s $R/mute-record.back ] && { echo "MUTE BACK OK (already put back by an earlier micoff; $R/mute-record.back)"; exit 0; }
[ -s $R/mute-record.saved ] || { echo "MUTE BACK FAIL: no recorded state at $R/mute-record.saved"; exit 3; }
[ -e $R/mute-lifted.none ] && { echo "MUTE BACK OK (nothing was lifted)"; rm -f $R/mute-lifted.none; mv -f $R/mute-record.saved $R/mute-record.back; exit 0; }
systemctl stop lamp-voice; n "unmute-back: lamp-voice stopped cleanly"
python3 -I - $R/mute-record.saved $MR <<'P' || { systemctl start lamp-voice; echo "MUTE BACK FAIL: record not written"; exit 4; }
import json, os, sys
b = json.load(open(sys.argv[1])); p = sys.argv[2]; st = os.stat(p)
try: cur = json.load(open(p))
except Exception: cur = {}
b["through_us"] = max(int(b.get("through_us", 0)), int(cur.get("through_us", 0)))
t = p + ".t8162"; fd = os.open(t, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
os.write(fd, json.dumps(b, separators=(",", ":")).encode()); os.fsync(fd); os.close(fd)
os.chown(t, st.st_uid, st.st_gid); os.replace(t, p)
d = os.open(os.path.dirname(p), os.O_RDONLY); os.fsync(d); os.close(d)
print("RECORD_BACK " + open(p).read())
P
n "unmute-back: $MR put back as recorded (muted, as before the ROW)"
systemctl start lamp-voice; n "unmute-back: lamp-voice started"
s=0; while [ $s -lt 60 ]; do sleep 2; s=$((s + 2)); L1=$(mute_live)
  echo "$L1" | grep -qE 'VOICE_MUTE muted=True .*age_s=[0-4]\.' && break; done
echo "$L1"
# Once back, the kept record is retired, so a later micon never takes it for a lift still to put back.
echo "$L1" | grep -qE 'VOICE_MUTE muted=True .*age_s=[0-4]\.' && { mv -f $R/mute-record.saved $R/mute-record.back; echo "MUTE BACK OK after ${s}s"; } || echo "MUTE BACK FAIL: voice/mute not muted within 60 s"
echo "MIC_AFTER $(tr -d "
" < $MR 2>/dev/null)"
