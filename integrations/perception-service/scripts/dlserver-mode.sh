#!/usr/bin/env bash
# Optional second dlserver slot on the same GPU, switched by hand (#612).
#
#   make dlserver-two-slot   start the idle slot on the active slot's code, serve from both
#   make dlserver-single     back to one slot (keeps 8001 when it is alive), drain, stop the other
#   make dlserver-mode       print the mode, the slots and GPU memory
#
# The state file lives in RUN_DIR (/tmp), so a container restart comes back in
# single mode. A switch counts only once lbserver's ack names exactly the wanted
# slots; until then the previous slots keep serving.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
source scripts/dlserver-slots.sh

lock() { exec 9>"$RUN_DIR/dlserver-deploy.lock"; flock -n 9 || abort "a deploy or mode switch is running"; }

cmd_status() {
    local n p
    n=$(slot_count)
    if [ "$n" -gt 1 ]; then echo "mode: two-slot"; else echo "mode: single"; fi
    for p in $(slots_in_state); do
        echo "slot $p: $([ -n "$(listener "$p")" ] && echo running || echo DOWN) rev=$(cat "$(rev_file "$p")" 2>/dev/null || echo ?)"
    done
    [ "$n" -gt 0 ] || echo "slot $DLSERVER_SLOT_A: (no state file, default)"
    echo "gpu(used,free,total MB)=$(gpu)"
}

cmd_two() {
    lock
    [ "$(slot_count)" -le 1 ] || abort "already in two-slot mode"
    local active idle rev diff_rc free want missing rc
    active=$(slot_of "$STATE"); active=${active:-$DLSERVER_SLOT_A}
    idle=$(other_slot "$active")
    log "=== two-slot on: active=$active idle=$idle gpu=$(gpu)"
    [ -n "$(lb_pid)" ] || abort "nothing listens on :$LBSERVER_PORT (lbserver down)"
    acked_on "$active" || abort "lbserver has not acked $STATE for slot $active; restart it with make start-runpod-lbserver"
    [ -n "$(listener "$active")" ] || abort "active slot $active is not running"
    [ -z "$(listener "$idle")" ] || abort "idle port $idle is in use; stop it first: make stop-runpod-dlserver DLSERVER_PORT=$idle"
    rev=$(cat "$(rev_file "$active")" 2>/dev/null)
    [ -n "$rev" ] || abort "no $(rev_file "$active"): unknown code on slot $active"
    # Both slots must run the same code: the new slot starts from this checkout.
    git diff --quiet "$rev" -- src scripts Makefile pyproject.toml 2>/dev/null
    diff_rc=$?
    [ "$diff_rc" != 1 ] || abort "checkout differs from slot $active ($rev): run make deploy-dlserver first"
    [ "$diff_rc" = 0 ] || abort "cannot compare the checkout with $rev (git exit $diff_rc)"
    free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ' ')
    if [ -z "$free" ]; then
        log "WARNING: nvidia-smi unavailable, skipping the GPU memory check"
    elif [ "$free" -lt "$MIN_FREE_MB" ]; then
        abort "only ${free} MB GPU free, a second slot needs $MIN_FREE_MB"
    fi
    want=$(health "$active") || abort "cannot read health on active slot $active"

    stop_idle() { log "ABORT: $* -- stopping slot $idle, traffic unchanged"; mk stop-runpod-dlserver DLSERVER_PORT="$idle" || log "WARNING: slot $idle did not stop cleanly"; exit 1; }
    trap 'stop_idle interrupted' INT TERM HUP
    mk start-runpod-dlserver-slot DLSERVER_PORT="$idle" || stop_idle "make start-runpod-dlserver-slot failed"
    log "slot $idle starting; waiting up to ${READY_TIMEOUT}s for its models"
    missing=$(wait_ready "$idle" "$want"); rc=$?
    case $rc in
        0) ;;
        1) stop_idle "slot $idle is up but missing models: $missing" ;;
        *) stop_idle "slot $idle not ready after ${READY_TIMEOUT}s" ;;
    esac

    trap '' INT TERM HUP
    write_state "$active" "$idle" || stop_idle "cannot write $STATE"
    hup_lb
    if wait_acked_exactly "$active" "$idle"; then
        trap - INT TERM HUP
        log "done: two-slot mode, serving from $active and $idle gpu=$(gpu)"
        return 0
    fi
    write_state "$active"
    hup_lb
    if wait_acked_exactly "$active"; then
        stop_idle "lbserver did not confirm two slots; state file restored to $active"
    fi
    log "ABORT: lbserver did not confirm two slots and the rollback is NOT confirmed: state file names $(cat "$STATE" 2>/dev/null), ack $(cat "$STATE.applied" 2>/dev/null || echo none). Slot $idle stays up. Check which slots lbserver uses, then make dlserver-single."
    exit 1
}

cmd_single() {
    lock
    local slots keep stop p
    slots=$(slots_in_state)
    if [ "$(slot_count)" -le 1 ]; then log "already single: $(slot_of "$STATE" || echo "$DLSERVER_SLOT_A")"; return 0; fi
    # Keep 8001 when it is alive, else the first live slot in the state file.
    keep=""
    for p in $DLSERVER_SLOT_A $slots; do
        if grep -qx "$p" <<<"$slots" && [ -n "$(listener "$p")" ]; then keep=$p; break; fi
    done
    [ -n "$keep" ] || abort "no slot in $STATE is running; restart with make start-runpod-master"
    stop=$(other_slot "$keep")
    log "=== two-slot off: keep=$keep stop=$stop"
    trap '' INT TERM HUP
    write_state "$keep" || abort "cannot write $STATE"
    hup_lb
    if ! wait_acked_exactly "$keep"; then
        # shellcheck disable=SC2086  # one port per word
        write_state $slots
        hup_lb
        abort "lbserver did not confirm the switch to $keep; state file restored"
    fi
    log "lbserver -> $keep only; draining ${DRAIN}s"
    sleep "$DRAIN"
    if ! mk stop-runpod-dlserver DLSERVER_PORT="$stop"; then
        log "ERROR: slot $stop did not stop cleanly. Traffic is on $keep. Stop it by hand: make stop-runpod-dlserver DLSERVER_PORT=$stop"
        exit 1
    fi
    trap - INT TERM HUP
    log "done: single mode on $keep gpu=$(gpu)"
}

case "${1:-}" in
    two) cmd_two ;;
    single) cmd_single ;;
    status) cmd_status ;;
    *) echo "usage: $0 two|single|status" >&2; exit 2 ;;
esac
