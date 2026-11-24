# lamp, root sh (common first). READ ONLY: the speaker's kept level and bus value, and the voice's live mute.
# Prints "VOL kept=<n|none> bus=<n|none> muted=<b|none>" and "MUTE live muted=<True|False|None>". Never sets anything.
python3 -I - "$KVOL" "$BVOL" <<'PY'
import json, sys
def bus(path):
    try:
        _, _, p = open(path, "rb").read().partition(b"\n"); return json.loads(p)
    except Exception: return None
try: k = open(sys.argv[1]).read().strip() or "none"
except Exception: k = "none"
v = bus(sys.argv[2]) or {}
print("VOL kept=%s bus=%s muted=%s" % (k, v.get("volume", "none"), str(v.get("muted", "none")).lower()))
m = bus("/run/lampos-bus/voice/mute/latest") or {}
print("MUTE live muted=%s reason=%s" % (m.get("muted"), m.get("reason")))
PY
