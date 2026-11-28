#!/bin/sh
# Run as root on lamp-75c9 (../run.sh go runs it). #8671's setup, before any leg:
#   0. preflight (refuses before any change); ROW_START_WALL_US kept;
#   1. the dead-man FIRST: systemd-run --unit=lamp8671-deadman --on-active=DEADMAN_S (1800 s) running putback.sh
#      deadman, so the seeded markers and the tmpfs go whatever happens to the Mac before the reset;
#   2. a tmpfs at /run/el-8671 with the PR's CI ARM64 lamp-setup and lamp-voice (sha = the artifact's SHA256SUMS;
#      the voice runs only in leg 2b, through a /run drop-in, oldclips.sh) and the PR's
#      product plan (sha = RESET_PLAN_SHIPPED's first line at the PR head). Nothing installed under /opt or /etc:
#      the stock /etc/lampOS/setup/reset-plan.json is never written (the stock updater would keep a plan it
#      doesn't know as "this lamp's own").
# Never touches lamp-update, Wi-Fi or any unit. A reboot drops all of it (/run only). INSTALLED / STOPPED.
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ -e $K/installed.txt ] && { echo "REFUSE: already installed: putback.sh first"; exit 1; }
mkdir -p $K; chmod 700 $K
echo "ROW_START_WALL_US=$(wall_us)" > $K/row-start.txt
sh "$KIT/preflight.sh" > $K/preflight-before.txt 2>&1 || { cat $K/preflight-before.txt; echo "install: preflight refused, nothing done"; exit 1; }
grep -E 'PREFLIGHT|^BUILD=|^STOCK_|^SSH_VIA|^NOTE|^HOME |^rsyslog|^orangepi' $K/preflight-before.txt
fail() { echo "install: $*"; echo "install: STOPPED. Put back with: sh $KIT/putback.sh"; exit 1; }
# -- 1. the dead-man, before any change
systemd-run --quiet --unit=$DEADMAN --on-active=$DEADMAN_S --timer-property=AccuracySec=1s /bin/sh "$KIT/putback.sh" deadman \
  || { echo "install: the dead-man could not be armed: nothing done"; exit 1; }
systemctl is-active --quiet $DEADMAN.timer || { systemctl stop $DEADMAN.timer 2>/dev/null; echo "install: the dead-man timer is not active: nothing done"; exit 1; }
echo "DEADMAN_WALL_US=$(wall_us) DEADMAN_S=$DEADMAN_S (runs $KIT/putback.sh deadman)"
# -- 2. the tmpfs, the binary and the plan
(cd "$PAYLOAD" && sha256sum -c SHA256SUMS) || fail "payload sums"
want=$(awk '$2=="lamp-setup"{print $1}' "$PAYLOAD/SHA256SUMS"); pwant=$(awk '$2=="reset-plan.json"{print $1}' "$PAYLOAD/SHA256SUMS")
vwant=$(awk '$2=="lamp-voice"{print $1}' "$PAYLOAD/SHA256SUMS")
size=$(( ($(wc -c < "$PAYLOAD/lamp-setup") + $(wc -c < "$PAYLOAD/lamp-voice")) / 1048576 + 16 ))
mkdir -p $BIN_DIR
mount -t tmpfs -o "size=${size}m,mode=0755,nosuid,nodev,exec" el-8671 $BIN_DIR || fail "tmpfs on $BIN_DIR not mounted"
echo "$BIN_DIR" > $K/installed.txt
install -m 755 -o root -g root "$PAYLOAD/lamp-setup" $BIN || fail "the binary cannot be placed"
install -m 644 -o root -g root "$PAYLOAD/reset-plan.json" $PLAN || fail "the plan cannot be placed"
install -m 755 -o root -g root "$PAYLOAD/lamp-voice" $VBIN || fail "the voice binary cannot be placed"
vgot=$(sha_of $VBIN); [ -n "$vwant" ] && [ "$vgot" = "$vwant" ] || fail "$VBIN sha $vgot != SHA256SUMS $vwant"
got=$(sha_of $BIN); [ -n "$want" ] && [ "$got" = "$want" ] || fail "$BIN sha $got != SHA256SUMS $want"
pgot=$(sha_of $PLAN); [ -n "$pwant" ] && [ "$pgot" = "$pwant" ] || fail "$PLAN sha $pgot != $pwant"
echo "BIN_SHA=$got (the artifact's SHA256SUMS, head $(cut -c1-10 "$PAYLOAD/TAG.txt"))"
echo "VBIN_SHA=$vgot (the artifact's SHA256SUMS)"
echo "PLAN_SHA=$pgot (RESET_PLAN_SHIPPED's first line at the PR head)"
echo "PLAN_PATHS=$(python3 -I -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["wipe"]))' $PLAN)"
echo "INSTALL_WALL_US=$(wall_us)" | tee $K/install.txt
echo INSTALLED
