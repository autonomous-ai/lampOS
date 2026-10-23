#!/bin/sh
# Runs only in a fresh, owner-labelled, offline ARM64 Docker container.
set -eu
[ "${LAMP_PAIR_DISPOSABLE-}" = 1 ]
[ -f /.dockerenv ]
[ "$(uname -s)" = Linux ]
[ "$(uname -m)" = aarch64 ]
for path in /opt/lampOS /var/lib/lampos-update /etc/lampOS; do
    [ ! -e "$path" ] && [ ! -L "$path" ]
done
for command in timeout setsid sha256sum readlink cmp cp mkfifo; do command -v "$command" >/dev/null; done
[ -x /bin/kill ]
sha256sum -c /case/pins > /case/out/artifact-checks
{
    IFS= read -r writer
    IFS= read -r reader
} < /case/roles
case "$writer:$reader" in 6408:6449|6449:6408) ;; *) exit 90 ;; esac
mkdir -p /etc/systemd/system /run/pair-bus /case/out
for part in air records schedule; do [ ! -e "/etc/systemd/system/lamp-$part.service" ]; done
cp -a /case/root/etc/lampOS /etc/
cp -a /case/root/etc/systemd/system/. /etc/systemd/system/
cp -a /case/root/opt/lampOS /opt/
cp -a /case/root/var/lib/lampos-update /var/lib/
mkfifo /case/ready /case/never-release
export PATH=/case/bin:/usr/bin:/bin HOME=/tmp
export LAMP_UPDATE_APPLY=on LAMP_UPDATE_SELF=off LAMP_UPDATE_IMAGE=off LAMP_UPDATE_MODELS=off

snapshot() {
    at=/case/out/$1
    mkdir -p "$at/etc" "$at/units"
    cp -a /etc/lampOS "$at/etc/"
    cp -a /var/lib/lampos-update "$at/records"
    for part in air records schedule; do
        cp -a "/etc/systemd/system/lamp-$part.service" "$at/units/"
        readlink "/opt/lampOS/$part/current" > "$at/$part.link"
        cp -p "/opt/lampOS/$part/current/lamp-$part" "$at/$part.selected"
    done
}

writer_pid=
cleanup() {
    if [ -n "$writer_pid" ]; then
        # The writer is our unreaped direct child and a verified new session.
        # Docker --init reaps its orphaned shim after the whole group is killed.
        /bin/kill -KILL -- "-$writer_pid" 2>/dev/null || :
        wait "$writer_pid" 2>/dev/null || :
    fi
}
trap cleanup EXIT HUP INT TERM
export PAIR_PHASE=forward PAIR_HELPER="$reader"
setsid "/case/$writer" once --channel-url file:///case/channel/ --lamp fixture \
    --apply on --watch-s 1 --quiet-wait-s 1 --keep-free-mb 1 --bus-root /run/pair-bus \
    > /case/out/writer.stdout 2> /case/out/writer.stderr &
writer_pid=$!
# The first ordinary restart cannot arrive until journal + helper result +
# candidate unit/link writes are complete. Bound the event wait independently.
timeout -k 2 30 /bin/sh -c 'IFS= read -r pid < /case/ready; printf "%s\n" "$pid" > /case/out/barrier-pid'
IFS= read -r barrier_pid < /case/out/barrier-pid
case "$barrier_pid" in ''|*[!0-9]*) exit 90 ;; esac
IFS=' ' read -r pid comm state parent group rest < "/proc/$writer_pid/stat"
[ "$pid" = "$writer_pid" ] && [ "$group" = "$writer_pid" ] && [ "$writer_pid" != "$$" ]
[ "$state" != Z ]
cp "/proc/$writer_pid/stat" /case/out/writer.stat
IFS=' ' read -r pid comm state parent group rest < "/proc/$barrier_pid/stat"
[ "$group" = "$writer_pid" ] && [ "$parent" = "$writer_pid" ]
cp "/proc/$barrier_pid/stat" /case/out/shim.stat
sha256sum "/proc/$writer_pid/exe" > /case/out/writer-executable
cmp "/proc/$writer_pid/exe" "/case/$writer"
[ -f /var/lib/lampos-update/journal.json ]
[ -f /var/lib/lampos-update/apply/back.json ]
[ ! -e /var/lib/lampos-update/apply/request.json ]
[ ! -e /var/lib/lampos-update/apply/done.json ]
for part in air records schedule; do
    cmp "/etc/systemd/system/lamp-$part.service" "/case/after/etc/systemd/system/lamp-$part.service"
    cmp "/opt/lampOS/$part/current/lamp-$part" "/case/channel/lamp-$part"
done
cmp /etc/lampOS/records.json /case/after/etc/lampOS/records.json
cmp /etc/lampOS/controls/controls.json /case/after/etc/lampOS/controls/controls.json
snapshot cut
/bin/kill -0 "$writer_pid"
/bin/kill -KILL -- "-$writer_pid"
set +e
wait "$writer_pid"
writer_status=$?
set -e
writer_pid=
printf '%s\n' "$writer_status" > /case/out/writer-wait-status
[ "$writer_status" = 137 ]
cmp /case/out/cut/records/journal.json /var/lib/lampos-update/journal.json
sha256sum /var/lib/lampos-update/journal.json > /case/out/journal-before-recovery.sha256

# This release really reads the other release's journal. No fixture writes or
# replaces journal/request/back/done/state between the kill and this command.
export PAIR_PHASE=recovery PAIR_HELPER="$writer"
set +e
timeout -k 2 20 "/case/$reader" once --channel-url file:///case/no-channel/ --lamp fixture \
    --apply on --watch-s 1 --quiet-wait-s 1 --keep-free-mb 1 --bus-root /run/pair-bus \
    > /case/out/recovery.stdout 2> /case/out/recovery.stderr
recovered=$?
set -e
printf '%s\n' "$recovered" > /case/out/recovery-status
[ "$recovered" = 1 ]
snapshot recovered

# A new process takes the ordinary path, refuses this already rolled-back
# sequence and changes no settings, links, state or restart calls.
export PAIR_PHASE=next
set +e
timeout -k 2 20 "/case/$reader" once --channel-url file:///case/channel/ --lamp fixture \
    --apply on --watch-s 1 --quiet-wait-s 1 --keep-free-mb 1 --bus-root /run/pair-bus \
    > /case/out/next.stdout 2> /case/out/next.stderr
next=$?
set -e
printf '%s\n' "$next" > /case/out/next-status
[ "$next" = 1 ]
snapshot next
[ ! -e /case/out/unexpected.log ]
sha256sum -c /case/pins >> /case/out/artifact-checks
printf 'real CLI release pair %s -> %s completed\n' "$writer" "$reader"
