#!/bin/sh
# Only these fixture units/commands exist. No real systemctl is ever called.
set -eu
die() { printf '%s: %s\n' "${PAIR_PHASE-unset}" "$*" >> /case/out/unexpected.log; exit 90; }
case "${PAIR_PHASE-}" in forward|recovery|next) ;; *) die "phase" ;; esac
printf '%s %s\n' "$PAIR_PHASE" "$*" >> /case/out/commands.log
state() {
    case "$1" in
        lamp-air.service) active=active; sub=running; pid=101 ;;
        lamp-records.service) active=active; sub=running; pid=102 ;;
        lamp-schedule.service|lamp-voice.service|lamp-update-apply.service) active=inactive; sub=dead; pid=0 ;;
        *) die "unit $1" ;;
    esac
}
if [ "$#" -ge 4 ] && [ "$1" = show ] && [ "$2" = --property=Id,ActiveState,SubState,MainPID,NRestarts ] && [ "$3" = -- ]; then
    shift 3
    for unit do
        state "$unit"
        printf 'Id=%s\nActiveState=%s\nSubState=%s\nMainPID=%s\nNRestarts=0\n\n' "$unit" "$active" "$sub" "$pid"
    done
elif [ "$#" = 5 ] && [ "$1" = show ] && [ "$2" = --property=MainPID ] && [ "$3" = --value ] && [ "$4" = -- ]; then
    state "$5"
    printf '%s\n' "$pid"
elif [ "$#" = 1 ] && [ "$1" = daemon-reload ]; then
    :
elif [ "$#" = 2 ] && [ "$1" = restart ] && [ "$2" = lamp-update-apply.service ]; then
    case "${PAIR_HELPER-}" in 6408|6449) ;; *) die "helper" ;; esac
    cp /var/lib/lampos-update/apply/request.json "/case/out/$PAIR_PHASE-request.json"
    printf '%s\n' "$PAIR_HELPER" > "/case/out/$PAIR_PHASE-helper"
    # Preserve the helper's own status and diagnostics. Timeout is a failure.
    timeout -k 2 15 "/case/$PAIR_HELPER" apply > "/case/out/$PAIR_PHASE-helper.stdout" 2> "/case/out/$PAIR_PHASE-helper.stderr"
    printf '0\n' > "/case/out/$PAIR_PHASE-helper-status"
    cp /var/lib/lampos-update/apply/back.json "/case/out/$PAIR_PHASE-back.json"
    cp /var/lib/lampos-update/apply/done.json "/case/out/$PAIR_PHASE-done.json"
elif [ "$#" = 2 ] && [ "$1" = restart ]; then
    case "$2" in lamp-air.service|lamp-records.service) ;; *) die "restart $2" ;; esac
    if [ "$PAIR_PHASE" = forward ]; then
        [ "$2" = lamp-records.service ] || die "unexpected first restart $2"
        # Event handshake: the real updater is waiting for this exact restart.
        # No sleep or guessed scheduling delay defines the cut.
        printf '%s\n' "$$" > /case/ready
        IFS= read -r ignored < /case/never-release
        die "forward barrier unexpectedly released"
    fi
else
    die "command $*"
fi
