#!/bin/sh
# In the rig: the lamp put back as found (S0: build A, its units, state.json, the records,
# the production key.pub), then read back twice against S0's digest. On any doubt it stops
# and says what to do; it never guesses.
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
D=$OUT/putback
mkdir -p "$D"
S0=$K/s0
[ -f "$S0/digest.txt" ] || { echo "el-8400: PUTBACK-DOUBT: no S0 kept. The rig is a throwaway container: 'sh run.sh clean' removes it; nothing outside it was touched."; exit 3; }
# No lamp-update of the kit's may be at work.
if pgrep -f '/lamp-update (once|serve|install)' > "$D/busy.txt"; then
  echo "el-8400: PUTBACK-DOUBT: a lamp-update still runs (pids: $(tr '\n' ' ' < "$D/busy.txt")). Wait for it to end (a round is bounded by 30 min), then run putback again."
  exit 3
fi
mark "putback"
sh "$K/lamp/snap.sh" restore || { echo "el-8400: PUTBACK-DOUBT: the restore stopped (above). Run 'sh run.sh putback' once more; if it stops again, 'sh run.sh clean' removes the container."; exit 3; }
for n in 1 2; do
  if sh "$K/lamp/snap.sh" check > "$D/readback$n.txt" 2>&1; then
    say "READBACK $n: SAME as S0"
  else
    cat "$D/readback$n.txt"
    echo "el-8400: PUTBACK-DOUBT: read back $n differs from S0 (above, kept in out/putback/). Do not use this rig for another row; 'sh run.sh clean' removes it."
    exit 3
  fi
  k=$(fp /etc/lampOS/update/key.pub)
  [ "$k" = "$PROD_FP" ] || { echo "el-8400: PUTBACK-DOUBT: key.pub is $k, not the production $PROD_FP"; exit 3; }
  inst=$(python3 -I -c "import json;print(json.load(open('$REC/state.json'))['installed']['sequence'])")
  [ "$inst" = "$A_SEQ" ] || { echo "el-8400: PUTBACK-DOUBT: state.json says installed $inst, not $A_SEQ"; exit 3; }
  [ -e "$UNITS/lamp-npu.service" ] && { echo "el-8400: PUTBACK-DOUBT: lamp-npu.service is still there"; exit 3; }
  say "READBACK $n: build $inst, update/current $(readlink /opt/lampOS/update/current | sed 's#.*/##' | cut -c1-12), key.pub $k"
  [ "$n" = 1 ] && sleep 2
done
say "PUTBACK-OK: as found (build $A_SEQ, units, records, key), read back twice"
