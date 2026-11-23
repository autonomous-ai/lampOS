# lamp, root sh (common first, then SECS=). READ ONLY (the #8138 kit's aw.lamp.sh). Follows the lamp's own agent's
# readiness for SECS s, and says AH <epoch> every 5 s so it ends soon after the Mac stops reading (a closed pipe):
#  AW <epoch> <state> <why> <since_ms> <seq>   the bus's kept value of tasks/agent (its `latest` file), read every 0.2 s,
#                                              printed on each new message (seq) — what lamp-voice and lamp-cloud read;
#  AO <epoch> <answer>                          lamp-tasks' {"op":"agent"} on request.sock, every 1 s, printed on change.
cd /
SECS=${SECS:-900} timeout $(( ${SECS:-900} + 5 )) python3 -I - "$AGENT_LATEST" <<'P'
import json, os, socket, sys, time
lat = sys.argv[1]; end = time.time() + float(os.environ["SECS"]); last_seq = None; last_ao = None; next_ao = 0; next_ah = 0
def ao():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.settimeout(2)
    try:
        s.connect("/run/lampos-tasks/request.sock"); s.sendall(b'{"op":"agent"}\n'); b = b""
        while not b.endswith(b"\n"):
            c = s.recv(65536)
            if not c: break
            b += c
        d = json.loads(b or b"{}"); a = d.get("agent", d.get("data", d))
        return " ".join(str(a).split())[:220]
    except Exception as e:
        return "unreachable:" + type(e).__name__
    finally:
        s.close()
while time.time() < end:
    now = time.time()
    try:
        h, _, p = open(lat, "rb").read().partition(b"\n"); hd = json.loads(h); d = json.loads(p)
        key = (hd.get("seq"), hd.get("boot"))
        if key != last_seq:
            last_seq = key
            print("AW %.3f %s %s %s %s" % (now, d.get("state"), d.get("why", "-"), d.get("since_unix_ms"), hd.get("seq")), flush=True)
    except FileNotFoundError:
        if last_seq != "none": last_seq = "none"; print("AW %.3f none - - -" % now, flush=True)
    except Exception as e:
        pass
    if now >= next_ao:
        next_ao = now + 1.0; a = ao()
        if a != last_ao: last_ao = a; print("AO %.3f %s" % (now, a), flush=True)
    if now >= next_ah:
        next_ah = now + 5.0; print("AH %.3f" % now, flush=True)
    time.sleep(0.2)
P
