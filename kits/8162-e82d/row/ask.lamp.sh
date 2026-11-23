# lamp, root sh. ask.lamp.sh "<words>"  (or, through the Mac's LAMP: ASK_B64=<base64 words> first).
# One spoken task handed to lamp-tasks exactly as lamp-voice hands it, minus the voice (the #7906 and #8138 kits):
#   1. {"op":"status"} on request.sock -> store_id, revision, summary.session_id
#   2. {"op":"prepare","guard":{...},"text":<words>,"target":"lamp","engine":"openclaw"} on request.sock
#      (lamp-voice agent_tasks.rs `preparation`; engine.rs Prepare; ASK_ENGINE= to omit the engine)
#   3. {"op":"status"} again, then {"op":"confirm","guard":{...},"id","prepared_digest","session_id"} on
#      voice.sock (the trusted route; protocol.rs Request::trusted; no caller check on that socket)
# From there it is lamp-tasks' normal path: the store queues it, Engine::launch -> one_session::spoken_launch
# -> Turn::send -> open_kept / reply_kept into the kept OpenClaw session (for_the_pane: target "lamp" on the
# lamp's machine, its default engine OpenClaw, no session named).
#   4. get_task / get_observation polled on request.sock until the answer (observation idle, answer set):
#      ASK_ANSWER. If still untold ASK_TELL_AFTER s later (default 20; the voice tells it only with someone
#      there), {"op":"told"} on voice.sock as lamp-voice's task_news send_told does, so the pane is released
#      (runtime.rs one_session_holds) without waiting UNTOLD_CLOSE_MS (60 s).
# Prints ASK_* lines only; the words and the answer are cut to 200 bytes. Read-write on the task store only.
# Exit 0 answered, 4 no answer within ASK_WAIT (default 480 s), 5 refused / not sent, 6 asking.
cd /
W=${ASK_B64:+$(echo "$ASK_B64" | base64 -d)}; W=${W:-${1:-}}
[ -n "$W" ] || { echo "ASK REFUSED: no words"; exit 2; }
n "ask: one spoken task by the sockets: $(echo "$W" | cut -c1-60)"
ASK_ENGINE=${ASK_ENGINE-openclaw} ASK_WAIT=${ASK_WAIT:-480} ASK_TELL_AFTER=${ASK_TELL_AFTER:-20} \
python3 -I - "$W" <<'P'
import json, os, socket, sys, time
D = "/run/lampos-tasks"; RQ, VO = D + "/request.sock", D + "/voice.sock"
words = sys.argv[1]; engine = os.environ.get("ASK_ENGINE", "")
wait, tell_after = float(os.environ["ASK_WAIT"]), float(os.environ["ASK_TELL_AFTER"])
def ask(path, line, within=3.0):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.settimeout(within)
    try:
        s.connect(path); s.sendall((json.dumps(line) + "\n").encode()); b = b""
        while not b.endswith(b"\n"):
            c = s.recv(65536)
            if not c: break
            b += c
        return json.loads(b or b"{}")
    except Exception as e:
        return {"ok": False, "error": "client_" + type(e).__name__}
    finally:
        s.close()
def out(*a): print(*a, flush=True)
def cut(t): return " ".join(str(t).split())[:200]
nonce = "%d-%d" % (time.time() * 1000, os.getpid())
def guard(tag):
    st = ask(RQ, {"op": "status"})
    if not st.get("ok"): out("ASK_REFUSED status", cut(st)); sys.exit(5)
    return st, {"store_id": st["store_id"], "revision": st["revision"], "request_id": "row8162-%s-%s" % (tag, nonce)}
w0 = time.time()
while time.time() - w0 < 120:  # lamp-tasks answers "lamp": "starting" until its first targets settle (~60 s after a start)
    if '"lamp": "starting"' not in json.dumps(ask(RQ, {"op": "status"})): break
    time.sleep(3)
out("ASK_WAITED %.1f s for the targets to settle" % (time.time() - w0))
st, g = guard("prepare")
pre = {"op": "prepare", "guard": g, "text": words, "target": "lamp"}
if engine: pre["engine"] = engine
p = ask(RQ, pre)
if not p.get("ok"): out("ASK_REFUSED prepare", cut(p)); sys.exit(5)
tid, digest = p["id"], p["prepared_digest"]
st, g = guard("confirm")
c = ask(VO, {"op": "confirm", "guard": g, "id": tid, "prepared_digest": digest, "session_id": st["summary"]["session_id"]})
t0 = time.time()
if not c.get("ok"): out("ASK_REFUSED confirm", cut(c)); sys.exit(5)
label = ask(RQ, {"op": "get_task", "id": tid}).get("label", "?")
out("ASK_AT %.6f" % t0); out("ASK_CONFIRMED id=%s label=%s engine=%s words=%s" % (tid, label, engine or "(default)", cut(words)))
last, answered_at = None, None
while time.time() - t0 < wait:
    t = ask(RQ, {"op": "get_task", "id": tid}); m = t.get("task") or {}
    d = m.get("dispatch") or {}; state = d.get("state")
    now = (state, m.get("queued"), m.get("observations"))
    if now != last:
        out("ASK_STATE %.1f s dispatch=%s queued=%s observations=%s" % (time.time() - t0, state, m.get("queued"), m.get("observations"))); last = now
    if state in ("not_sent", "uncertain", "cancelled", "invalidated"):
        out("ASK_NOT_SENT", cut(d)); sys.exit(5)
    obs = t.get("observations") or []
    if obs:
        o = obs[-1]; g2 = ask(RQ, {"op": "get_observation", "id": tid, "observation_id": o["id"]})
        data = g2.get("data") or {}
        if data.get("state") == "idle" and str(data.get("answer", "")).strip():
            if answered_at is None:
                answered_at = time.time(); out("ASK_ANSWER %.1f s %s" % (answered_at - t0, cut(data["answer"])))
            if o.get("told") or (g2.get("observation") or {}).get("told"):
                out("ASK_TOLD by the voice %.1f s" % (time.time() - t0)); sys.exit(0)
            if time.time() - answered_at >= tell_after:
                st, g = guard("told")
                r = ask(VO, {"op": "told", "guard": g, "id": tid, "observation_id": o["id"], "session_id": st["summary"]["session_id"]})
                out("ASK_TOLD by the kit %.1f s %s" % (time.time() - t0, "ok" if r.get("ok") else cut(r))); sys.exit(0)
        elif data.get("state") == "needs-input":
            out("ASK_ASKING %.1f s %s" % (time.time() - t0, cut(data.get("answer", "")))); sys.exit(6)
    time.sleep(2)
out("ASK_NO_ANSWER within %d s" % wait); sys.exit(4)
P
