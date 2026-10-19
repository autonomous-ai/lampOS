# Shared by deploy-dlserver.sh and dlserver-mode.sh (source it after cd to the
# service root): slot ports, the state file (one loopback URL per line, line 1 =
# primary) and lbserver's ack of it.
RUN_DIR=${RUN_DIR:-/tmp}
LOG_ROOT=${LOG_ROOT:-/workspace/logs}
DLSERVER_STATE_FILE=${DLSERVER_STATE_FILE:-$RUN_DIR/dlserver-active}
LBSERVER_PORT=${LBSERVER_PORT:-7999}
DLSERVER_SLOT_A=${DLSERVER_SLOT_A:-8001}
DLSERVER_SLOT_B=${DLSERVER_SLOT_B:-8002}
export RUN_DIR LOG_ROOT DLSERVER_STATE_FILE LBSERVER_PORT DLSERVER_SLOT_A DLSERVER_SLOT_B
STATE=$DLSERVER_STATE_FILE
READY_TIMEOUT=${DEPLOY_READY_TIMEOUT:-900}
DRAIN=${DEPLOY_DRAIN_SECONDS:-35}
MIN_FREE_MB=${DEPLOY_MIN_FREE_GPU_MB:-12000}
LOG="$LOG_ROOT/deploy/deploy.log"
mkdir -p "$(dirname "$LOG")" "$RUN_DIR"

log() { echo "[deploy $(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "$LOG"; }
abort() { log "ABORT: $* -- traffic unchanged"; exit 1; }
gpu() { nvidia-smi --query-gpu=memory.used,memory.free,memory.total --format=csv,noheader,nounits 2>/dev/null | head -1; }
listener() { ss -lntpH "sport = :$1" 2>/dev/null | grep -oE 'pid=[0-9]+' | head -1 | cut -d= -f2; }
rev_file() { if [ "$1" = "$DLSERVER_SLOT_A" ]; then echo "$RUN_DIR/dlserver.rev"; else echo "$RUN_DIR/dlserver-$1.rev"; fi; }
# Close fd 9 (the deploy lock) for make and everything it execs: the dlserver
# start target detaches a long-lived wrapper process (nohup setsid ... &), and an
# inherited fd 9 would keep that process holding the lock forever after this
# script exits, permanently blocking every later deploy.
mk() { make --no-print-directory "$@" 9>&- >>"$LOG" 2>&1; }
KEY=${DL_API_KEY:-$(grep -E '^DL_API_KEY=' .env 2>/dev/null | head -1 | cut -d= -f2- | tr -d "\"'" | tr -d '[:space:]')}
health() { curl -sf -m 10 -H "X-API-Key: $KEY" "http://127.0.0.1:$1/hal/api/dl/health"; }
# Models ready on the active slot but not on the new one, space-separated.
missing_models() {
    python - "$1" "$2" <<'EOF'
import json, sys
def ready(raw):
    m = json.loads(raw)["models"]
    out = {k for k, v in m.items() if v is True}
    out |= {f"object_detectors.{k}" for k, v in (m.get("object_detectors") or {}).items() if v is True}
    return out
print(" ".join(sorted(ready(sys.argv[1]) - ready(sys.argv[2]))))
EOF
}
# lbserver can be restarted by its watchdog at any moment, so always ask who
# listens on the LB port now instead of reusing a pid read earlier.
lb_pid() { listener "$LBSERVER_PORT"; }
acked() { local p; p=$(lb_pid); [ -n "$p" ] && grep -q "\"pid\": $p," "$STATE.applied" 2>/dev/null; }
acked_on() { acked && grep -q "127.0.0.1:$1\"" "$STATE.applied"; }

slots_in_state() { sed -n 's|^http://127.0.0.1:\([0-9][0-9]*\)/*$|\1|p' "$STATE" 2>/dev/null; }
slot_count() { slots_in_state | grep -c .; }
slot_of() { sed -n 's|^http://127.0.0.1:\([0-9][0-9]*\)/*$|\1|p' "$1" 2>/dev/null | head -1 | grep .; }
other_slot() { if [ "$1" = "$DLSERVER_SLOT_A" ]; then echo "$DLSERVER_SLOT_B"; else echo "$DLSERVER_SLOT_A"; fi; }
write_state() { local p; for p in "$@"; do printf 'http://127.0.0.1:%s\n' "$p"; done >"$STATE.tmp" && mv "$STATE.tmp" "$STATE"; }
hup_lb() { kill -HUP "$(lb_pid)" 2>/dev/null; }
# lbserver's current ack names exactly these local slots (order-free; remote
# backends in the ack are ignored).
acked_exactly() {
    local want got
    acked || return 1
    want=$(printf '%s\n' "$@" | sort | tr '\n' ' ')
    got=$(grep -oE '127\.0\.0\.1:[0-9]+' "$STATE.applied" | cut -d: -f2 | sort | tr '\n' ' ')
    [ "$want" = "$got" ]
}
wait_acked_exactly() { local i; for i in $(seq 1 20); do acked_exactly "$@" && return 0; sleep 0.25; done; return 1; }
# Poll a starting slot. Prints missing models on exit 1; exit 2 = not ready in time.
wait_ready() {
    local port=$1 want=$2 got missing deadline=$((SECONDS + READY_TIMEOUT))
    while :; do
        # dlserver binds its port only after every model loaded, so the first answer is final.
        if got=$(health "$port"); then
            missing=$(missing_models "$want" "$got") || { echo "unparseable health"; return 1; }
            [ -z "$missing" ] || { echo "$missing"; return 1; }
            return 0
        fi
        [ "$SECONDS" -lt "$deadline" ] || return 2
        sleep 5
    done
}
