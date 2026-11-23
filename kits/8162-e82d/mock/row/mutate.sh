#!/bin/bash
# Mutation check: each verdict rule below is broken in a copy of row/verdict.py, and the FAIL cases meant to catch it
# are run against the copy. CAUGHT = at least one of them no longer gives its expected FAIL. Exit 0 when all are caught.
M=$(cd "$(dirname "$0")" && pwd); python3 -I $M/gen.py $M/fx >/dev/null || exit 1; all=0
while IFS='|' read -r name from to cases; do [ -n "$name" ] || continue
  r=$(for c in $cases; do MUT_FROM=$from MUT_TO=$to $M/run-case.sh $c; done)
  if echo "$r" | grep -q 'MUTATION NOT FOUND'; then w="NOT FOUND"; all=1
  elif echo "$r" | grep -q ' MISMATCH '; then w="CAUGHT by $(echo "$r" | awk '$6 == "MISMATCH" {print $1 " (now " $5 ")"}' | tr '\n' ' ')"
  else w="MISSED"; all=1; fi
  printf '%-34s %s\n' "$name" "$w"
done <<'T'
L1 refused-at-once dropped|ok = bool(tr) and not tk|ok = True|L1.fail-not-refused
L2 14 s end bound widened to 74 s|te - t0 <= B["L2_END"], "the task in flight|te - t0 <= B["L2_END"] + 60, "the task in flight|L2.fail-late-end
L8 cut-back line "once" -> "any"|check(n == 1, "'started again|check(n >= 1, "'started again|L8.fail-cutback-twice
ready-again "once" -> "at least once"|if n == 1: return check(True|if n >= 1: return check(True|L1.fail-told-twice L3.fail-told-none
L9 no-churn: ready-told not counted|check(nn == 0 and nt == 0|check(nn == 0|L9.fail-churn
L4 SIGCONT check dropped|check(has(".unfreeze", "^UNFREEZE OK")|check(True or has(".unfreeze", "^UNFREEZE OK")|L4.fail-not-resumed
T
rm -rf $M/work; exit $all
