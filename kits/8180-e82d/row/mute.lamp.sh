# lamp, root sh (common first). READ ONLY. The software mute must be off before a clip plays (r7680's mute check, short).
# The live voice/mute value first (mute_live, as micon reads it). Only when it is unread, the record: a timed mute whose
# through_us (microseconds) is already past counts as off.
L=$(mute_live); echo "$L" | head -1
python3 -I - "$MR" "$L" <<'P'
import json, re, sys, time
m = re.search(r"VOICE_MUTE muted=(\S+) reason=(\S+)", sys.argv[2])
if m and m.group(1) in ("True", "False"):
    print("MUTE OFF OK (live voice/mute)" if m.group(1) == "False" else "MUTE FAIL: live voice/mute muted=True reason=%s" % m.group(2))
    sys.exit(0)
try: r = json.load(open(sys.argv[1]))
except Exception as e: print("MUTE FAIL: live voice/mute unread, record unreadable (%s)" % e); sys.exit(1)
t = r.get("through_us"); now_us = int(time.time() * 1e6)
if r.get("muted") is True and isinstance(t, (int, float)) and t < now_us:
    print("MUTE OFF OK (live voice/mute unread; record muted=true but its through_us %d is past, now %d)" % (t, now_us))
elif r.get("muted") is False: print("MUTE OFF OK (live voice/mute unread; record muted=false)")
else: print("MUTE FAIL: muted=%s reason=%s through_us=%s" % (r.get("muted"), r.get("reason"), t))
P
