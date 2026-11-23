# lamp, root sh. Step 1: an ORDINARY stable round, nothing staged: lamp-update on its own product line (real stable
# feed, product key, no drop-in) is restarted so serve rounds at once, e.g. 14521 -> 14653 (promote run 44509609454).
# Needs STABLE_SEQ (prepended). Prints STABLE0 OK or STABLE0 FAIL <why>. 1 service start (0 if already there).
cd /
MAXS=${MAXS:-1500}
product_line || { echo "STABLE0 FAIL: lamp-update's line is not the product's: $(line)"; exit 3; }
D=$(dropins); [ -z "$D" ] || { echo "$D"; echo "STABLE0 FAIL: drop-ins present"; exit 3; }
[ ! -e $CH ] || { echo "STABLE0 FAIL: $CH exists"; exit 3; }
b=$(build); echo "STATE before $b"
if [ "${b%% *}" = "$STABLE_SEQ" ]; then echo "STABLE0 OK (already on $STABLE_SEQ, no round needed)"; exit 0; fi
CUR=$(journalctl -n 0 --show-cursor -o cat 2>/dev/null | sed -n 's/^-- cursor: //p')
T0=$(date '+%F %T'); systemctl restart lamp-update.service
n "step 1: ordinary stable round: lamp-update restarted on its product line (no drop-in), from ${b%% *}"
say "stable round to $STABLE_SEQ started $T0"
s=0; ended=""
while [ $s -lt $MAXS ]; do
  J=$(journalctl --no-pager -o cat -u lamp-update.service --after-cursor "$CUR" 2>/dev/null)
  [ -z "$ended" ] && echo "$J" | grep -qE "switched to build $STABLE_SEQ|up to date: build $STABLE_SEQ|refused, nothing switched|put back|failed: |not switched: |cannot be reached" && ended=$s
  if [ -n "$ended" ]; then
    if echo "$J" | grep -q "lamp-update switches to its build"; then
      echo "$J" | grep -qE "not confirmed: " && break
      echo "$J" | grep -q "confirmed: lamp-update runs its new build" && echo "$J" | grep -q "up to date: build $STABLE_SEQ" && break
      [ $((s - ended)) -ge 660 ] && break
    else break; fi
  fi
  sleep 5; s=$((s + 5))
done
say "stable round waited ${s}s"
for u in lamp-update lamp-update-apply lamp-update-writer lamp-update-back lamp-update-harness; do
  journalctl --no-pager -o short-iso -u $u.service --after-cursor "$CUR" 2>/dev/null | grep -v '^-- ' | sed "s/^/J $u: /"
done
b=$(build); echo "STATE after $b"
echo "FAILED $(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
[ "${b%% *}" = "$STABLE_SEQ" ] && echo "STABLE0 OK" || echo "STABLE0 FAIL: state ${b%% *}, not $STABLE_SEQ"
