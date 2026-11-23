# lamp, root sh (row common first). READ ONLY. Before a fresh-turn ask (row.sh HUSH): waits until the voice's own
# `voice/state` on the bus (lamp_msgs/VoiceState, said on change and once a second) has said "resting" for IDLE_FOR s
# in a row (default 3), at most IDLE_MAX s (default 60). Migrate, 23 Nov 09:16: the asks were held because each one
# landed while the lamp still spoke its last answer, which the journal's last `say` line alone did not show.
# Prints IDLE <state> after <s>s | NOT IDLE <state> after <s>s (the ask goes on anyway; the leg reads its hold).
python3 -I - "${IDLE_FOR:-3}" "${IDLE_MAX:-60}" <<'PY'
import json, sys, time
need, most = float(sys.argv[1]), float(sys.argv[2]); t0 = time.time(); since = None; st = "unread"
while True:
    try:
        h, _, p = open("/run/lampos-bus/voice/state/latest", "rb").read().partition(b"\n")
        h = json.loads(h); age = time.time() - h.get("at_unix_ms", 0) / 1000.0
        st = json.loads(p).get("state", "?") if age < 3 else "stale"
    except Exception as e:
        st = "unread"
    now = time.time()
    if st == "resting":
        since = since or now
        if now - since >= need: print("IDLE resting after %.1fs" % (now - t0)); break
    else:
        since = None
    if now - t0 >= most: print("NOT IDLE %s after %.0fs" % (st, now - t0)); break
    time.sleep(0.5)
PY
