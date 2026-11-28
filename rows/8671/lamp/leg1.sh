#!/bin/sh
# Run as root on lamp-75c9 after install.sh: sh leg1.sh. Leg 1, read-only dry runs of the PR's lamp-setup:
#   check --plan <the PR's plan>, check --plan <the lamp's stock plan>, forget-agents --dry-run,
#   forget-engine-homes --dry-run, forget-logs --dry-run, and the two new commands WITHOUT --dry-run (refused).
# Each command's output is kept in $K/leg1-<name>.txt (paths and key names only: the dry runs print no contents)
# and copied to the Mac; here only counts are printed. The top of /var/lib/lampos-agents and the log files'
# names and inodes are read before and after: a dry run changes nothing.
. "$(dirname "$0")/common.sh"
host_ok; root_ok
[ -x $BIN ] || { echo "REFUSE: no $BIN: install.sh first"; exit 1; }
top_entries > $K/leg1-top-before.lst; log_inodes | sort > $K/leg1-logs-before.lst
run() { # run NAME ARGS...
  n=$1; shift
  "$BIN" "$@" > $K/leg1-$n.txt 2> $K/leg1-$n.err; r=$?
  echo "DRY $n exit=$r lines=$(wc -l < $K/leg1-$n.txt | tr -d ' ') $(awk '{c[$1]++} END{for (k in c) printf "%s=%d ", k, c[k]}' $K/leg1-$n.txt)"
  echo "DRY_ERR $n lines=$(wc -l < $K/leg1-$n.err | tr -d ' ') refused_line=$(grep -c 'runs only with --dry-run' $K/leg1-$n.err)"
}
run check-pr check --plan $PLAN
run check-stock check --plan $STOCK_PLAN
run agents forget-agents --dry-run
run homes forget-engine-homes --dry-run
run logs forget-logs --dry-run
run homes-nodry forget-engine-homes
run logs-nodry forget-logs
# Shape: every engine-homes line is `empty|remove /var/lib/lampos-agents/<name>`; every logs line names a path in
# /var/log or /var/log.hdd; the homes listed are the folder's own entries.
echo "HOMES_BAD_LINES=$(grep -cvE "^(empty|remove) $HOMES/[^/]+\$" $K/leg1-homes.txt)"
echo "LOGS_BAD_LINES=$(grep -cvE '^(truncate|remove|empty) /var/log(\.hdd)?/' $K/leg1-logs.txt)"
cut -d' ' -f2- $K/leg1-homes.txt | sort > $K/leg1-homes-paths.lst
cmp -s $K/leg1-homes-paths.lst $K/leg1-top-before.lst && echo "HOMES_LISTED=same" || echo "HOMES_LISTED=differ"
top_entries > $K/leg1-top-after.lst; log_inodes | sort > $K/leg1-logs-after.lst
cmp -s $K/leg1-top-before.lst $K/leg1-top-after.lst && echo "UNCHANGED_HOMES=yes" || echo "UNCHANGED_HOMES=no"
cmp -s $K/leg1-logs-before.lst $K/leg1-logs-after.lst && echo "UNCHANGED_LOGS=yes" || echo "UNCHANGED_LOGS=no"
echo LEG1-DONE
