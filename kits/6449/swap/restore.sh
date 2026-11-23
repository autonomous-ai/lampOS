#!/bin/bash
# Mac: puts back what swap.sh did (idempotent, safe on a partial swap, safe twice). Run after ../run.sh unrow
# (../run.sh `restore` does). Ends RESTORED OK | RESTORE NOT OK.
set -u; O=$(cd "$(dirname "$0")" && pwd); D=$O/../row; . $D/kit.sh
RENEW || exit 1; GATE lamp-voice || exit 1
LAMP "" $O/swap-common.lamp.sh $O/restore.lamp.sh 2>&1 | tee $D/logs/restore.txt
grep -q '^RESTORED OK' $D/logs/restore.txt || { echo "RESTORE NOT OK: keep the lock, read row/logs/restore.txt"; exit 1; }
