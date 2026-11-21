# lamp, root sh. READ ONLY. The software mute must be off before a clip plays (r7680's mute check, short).
M=/var/lib/lampOS/voice/mute/record.json
python3 -I - "$M" <<'P'
import json, sys
try: r = json.load(open(sys.argv[1]))
except Exception as e: print("MUTE FAIL: record unreadable (%s)" % e); sys.exit(1)
print("MUTE OFF OK" if r.get("muted") is False else "MUTE FAIL: muted=%s reason=%s" % (r.get("muted"), r.get("reason")))
P
