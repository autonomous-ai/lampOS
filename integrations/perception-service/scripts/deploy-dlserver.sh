#!/usr/bin/env bash
# Zero-downtime dlserver deploy on one GPU: two slots, switch when the new one is ready.
#
#   git pull && make deploy-dlserver
#
# Starts the new code on the idle slot, waits until it serves every model the
# active slot serves, points lbserver at it (state file + SIGHUP, confirmed by the
# ack lbserver writes), drains, then stops the old slot. Every refusal or failure
# before the switch leaves traffic where it was.
#
# Not for dependency changes: if pyproject.toml changed since the active slot
# started, pip would rewrite the venv the old slot serves from, and a TensorRT /
# ONNX Runtime bump rebuilds engines that may not fit next to two loaded copies.
# Use the in-place restart (make start-runpod-master) for those.
#
# Run it under nohup or tmux: an SSH drop sends SIGHUP, which aborts the deploy.
# Env knobs: DEPLOY_READY_TIMEOUT (900 s), DEPLOY_DRAIN_SECONDS (35 s, must exceed
# lb.http_timeout 30 s), DEPLOY_MIN_FREE_GPU_MB (12000; one loaded copy is ~9.7 GB).
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

source scripts/dlserver-slots.sh

# Two-slot mode: drop to one slot, deploy exactly as in single mode, then start
# the second slot again on the new code. One slot serves throughout, and old and
# new code never serve side by side.
if [ "$(slot_count)" -gt 1 ]; then
    log "two-slot mode: dropping to single, deploying, then back to two slots"
    bash scripts/dlserver-mode.sh single || abort "could not drop to a single slot"
    bash scripts/deploy-dlserver.sh || { log "deploy failed; staying in single mode (make dlserver-two-slot to retry)"; exit 1; }
    exec bash scripts/dlserver-mode.sh two
fi

exec 9>"$RUN_DIR/dlserver-deploy.lock"
flock -n 9 || abort "another deploy is running"

active=$(slot_of "$STATE"); active=${active:-$DLSERVER_SLOT_A}
if [ "$active" = "$DLSERVER_SLOT_A" ]; then idle=$DLSERVER_SLOT_B; else idle=$DLSERVER_SLOT_A; fi
log "=== deploy $(git rev-parse --short HEAD 2>/dev/null): active=$active idle=$idle gpu(used,free,total MB)=$(gpu)"

# --- preflight: refuse before touching anything --------------------------------
[ -n "$(lb_pid)" ] || abort "nothing listens on :$LBSERVER_PORT (lbserver down)"
acked_on "$active" || abort "lbserver (pid $(lb_pid)) has not acked $STATE, or its ack does not name slot $active: it predates the switch or runs without LB__STATE_FILE; restart it with make start-runpod-lbserver"
[ -n "$(listener "$active")" ] || abort "active slot $active is not running"
[ -z "$(listener "$idle")" ] || abort "idle port $idle is in use; stop it first: make stop-runpod-dlserver DLSERVER_PORT=$idle"
rev=$(cat "$(rev_file "$active")" 2>/dev/null)
[ -n "$rev" ] || abort "no $(rev_file "$active"): unknown commit on slot $active; do one in-place restart (make start-runpod-master) first"
# Compare against the working tree, not HEAD: HEAD only sees committed edits,
# so an uncommitted pyproject.toml change would sail through and rewrite the
# venv the old slot serves from mid-deploy. Distinguish "changed" (exit 1)
# from a git failure (any other exit): swallowing the latter behind the same
# message would refuse a healthy deploy without saying why.
git diff --quiet "$rev" -- pyproject.toml 2>/dev/null
diff_rc=$?
if [ "$diff_rc" = 1 ]; then
    abort "pyproject.toml changed since slot $active started ($rev): use the in-place restart (make start-runpod-master)"
elif [ "$diff_rc" != 0 ]; then
    abort "cannot compare pyproject.toml with $rev (git exit $diff_rc)"
fi
free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ' ')
if [ -z "$free" ]; then
    log "WARNING: nvidia-smi unavailable, skipping the GPU memory check"
elif [ "$free" -lt "$MIN_FREE_MB" ]; then
    abort "only ${free} MB GPU free, a second copy needs $MIN_FREE_MB"
fi
want=$(health "$active") || abort "cannot read /hal/api/dl/health on active slot $active"

# --- start the new slot ------------------------------------------------------------
switched=0
# 1 from just before the state file first names $idle until the ack confirms it
# (or a rollback finishes): the window where a signal must undo the switch, not
# just stop $idle -- lbserver may already be sending it traffic.
switching=0
fail() { log "ABORT: $* -- stopping slot $idle, traffic unchanged"; mk stop-runpod-dlserver DLSERVER_PORT="$idle" || log "WARNING: slot $idle did not stop cleanly"; exit 1; }

# Undo a switch to $idle: point the state file back at $active, SIGHUP the current
# lbserver, and wait for it to ack $active (same protocol as the forward switch).
# Confirmed -> traffic is really back on $active, so stopping $idle (via fail) is
# safe. NOT confirmed -> lbserver may still be sending traffic to $idle, so $idle
# must stay up; report the exact state for a human instead of guessing.
rollback_switch() {
    trap '' INT TERM HUP
    local reason=$1 wrote=1 i
    printf 'http://127.0.0.1:%s\n' "$active" >"$STATE.tmp" && mv "$STATE.tmp" "$STATE" && wrote=0
    kill -HUP "$(lb_pid)" 2>/dev/null
    for i in $(seq 1 20); do acked_on "$active" && break; sleep 0.25; done
    if [ "$wrote" = 0 ] && acked_on "$active"; then
        fail "$reason; state file restored to $active"
    fi
    log "ABORT: $reason -- rollback NOT confirmed: state file names $(cat "$STATE" 2>/dev/null || echo unreadable), lbserver ack $(cat "$STATE.applied" 2>/dev/null || echo none). lbserver may still be sending traffic to $idle: NOT stopping it. Check by hand which slot ($active or $idle) actually answers, fix the state file if needed (printf 'http://127.0.0.1:<port>\n' >$STATE && kill -HUP <lbserver pid>), then stop the other slot with: make stop-runpod-dlserver DLSERVER_PORT=<port>"
    exit 1
}

on_signal() {
    trap '' INT TERM HUP
    if [ "$switched" = 1 ]; then
        log "interrupted after the switch: traffic is on $idle; stop the old slot with: make stop-runpod-dlserver DLSERVER_PORT=$active"
        exit 1
    fi
    if [ "$switching" = 1 ]; then
        rollback_switch "interrupted during the switch"
    fi
    fail "interrupted"
}
trap on_signal INT TERM HUP

mk start-runpod-dlserver-slot DLSERVER_PORT="$idle" || fail "make start-runpod-dlserver-slot failed"
log "slot $idle starting; waiting up to ${READY_TIMEOUT}s for its models"
missing=$(wait_ready "$idle" "$want"); rc=$?
case $rc in
    0) ;;
    1) fail "slot $idle is up but missing models: $missing" ;;
    *) fail "slot $idle not ready after ${READY_TIMEOUT}s" ;;
esac
log "slot $idle ready gpu=$(gpu)"

# --- switch --------------------------------------------------------------------------
switching=1
printf 'http://127.0.0.1:%s\n' "$idle" >"$STATE.tmp" && mv "$STATE.tmp" "$STATE" || fail "cannot write $STATE"
kill -HUP "$(lb_pid)" 2>/dev/null
for _ in $(seq 1 20); do acked_on "$idle" && break; sleep 0.25; done
if ! acked_on "$idle"; then
    rollback_switch "lbserver did not confirm the switch"
fi
switched=1
log "switched lbserver -> $idle; draining ${DRAIN}s"
sleep "$DRAIN"

# --- stop the old slot -------------------------------------------------------------
if ! mk stop-runpod-dlserver DLSERVER_PORT="$active"; then
    log "ERROR: old slot $active did not stop cleanly. Traffic is on $idle. Stop it by hand: make stop-runpod-dlserver DLSERVER_PORT=$active"
    exit 1
fi
trap - INT TERM HUP
log "done: serving from $idle gpu=$(gpu)"
