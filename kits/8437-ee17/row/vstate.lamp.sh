# lamp, root sh (row common first). READ ONLY (#8437 leg L, First's bar on #8437, 24 Nov 05:14: the yes-ask within
# 1.5 s of the lamp's own reply ending). MODE=start: a detached logger of `voice/state` (lamp_msgs/VoiceState, said
# on change and once a second) for VS_FOR s (default 90), one line per change: "VSTATE <at_unix_s> <state>", into
# /run/t8437-vstate.log. MODE=read: prints that log, then removes it. Nothing else on the lamp is touched.
F=/run/t8437-vstate.log
case "${MODE:-}" in
start)
  rm -f $F
  setsid nohup python3 -I - "${VS_FOR:-90}" "$F" >/dev/null 2>&1 <<'PY' &
import json, sys, time
most, out = float(sys.argv[1]), sys.argv[2]; t0 = time.time(); last = None
with open(out, "a", buffering=1) as f:
    while time.time() - t0 < most:
        try:
            h, _, p = open("/run/lampos-bus/voice/state/latest", "rb").read().partition(b"\n")
            h = json.loads(h); st = json.loads(p).get("state", "?"); at = h.get("at_unix_ms", 0) / 1000.0
            if st != last: f.write("VSTATE %.3f %s\n" % (at, st)); last = st
        except Exception:
            pass
        time.sleep(0.05)
PY
  echo "VSTATE STARTED for ${VS_FOR:-90}s";;
read)
  [ -f $F ] && cat $F || echo "VSTATE NONE"; rm -f $F;;
*) echo "VSTATE: MODE=start|read"; exit 2;;
esac
