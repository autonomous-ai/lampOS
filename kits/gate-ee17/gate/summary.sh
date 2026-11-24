#!/bin/bash
# Mac, no lamp. The gate line's draft from gate/logs: "GATE <CAND_SEQ> on <lamp>: PASS|FAIL", round 1's updater named,
# then each step's and each check's result. What is promoted is CAND_SEQ; round 2's build only proves CAND_SEQ's
# updater. PASS needs RESTORE OK, round 1 by stable's updater, AFTER PASS|FINDING after each round and CORE PASS. A
# FINDING (#8429: one gateway exit 1 that listens again within 96 s) is named, not a FAIL. #8417's read is its own line.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh >/dev/null; L=$D/logs
g(){ grep -h -E "$1" $L/$2 2>/dev/null | head -1; }
v=PASS; why=
r1=$(sed -n 's/^UPD before \([0-9a-f]*\).*/\1/p' $L/round1.txt 2>/dev/null | head -1)
case "$r1" in $STABLE_UPD*) ;; *) v=FAIL; why="$why round 1's updater ${r1:-unread} is not $STABLE_UPD;";; esac
x=$(g '^RESTORE OK' restore.txt); [ -n "$x" ] || { v=FAIL; why="$why no RESTORE OK;"; }
for n in 1 2; do a=$(g '^AFTER ' after$n.txt); case "$a" in "AFTER PASS"*|"AFTER FINDING"*) ;; *) v=FAIL; why="$why after round $n: ${a:-not run};";; esac; done
c=$(g '^CORE ' core.txt); case "$c" in "CORE PASS"*) ;; *) v=FAIL; why="$why core rows: ${c:-not run};";; esac
echo "GATE $CAND_SEQ on $LAMP: $v"
echo "  round 1's updater: ${r1:0:12} (stable $STABLE_SEQ's, want $STABLE_UPD) took the lamp $STABLE_SEQ -> $CAND_SEQ; round 2's updater: $(sed -n 's/^UPD before \([0-9a-f]*\).*/\1/p' $L/round2.txt 2>/dev/null | head -1 | cut -c1-12) ($CAND_SEQ's, want $CAND_UPD) took it $CAND_SEQ -> ${NEXT_SEQ:-NEXT}"
echo "  restore: ${x:-not OK}"
for n in 1 2; do echo "  after round $n: $(g '^AFTER ' after$n.txt)"; grep -h '^CHECK ' $L/after$n.txt 2>/dev/null | sed "s/^CHECK /    $n /"; done
echo "  core rows: ${c:-not run}"; grep -h -E '^ROW |^  QUOTE ' $L/core.txt 2>/dev/null | sed 's/^/    /'
echo "  #8417's own read (before on $CAND_SEQ, after on ${NEXT_SEQ:-NEXT}; not a condition of $CAND_SEQ):"; for f in peek-before.txt peek-after.txt peek-8417.txt; do g '^PEEK ' $f | sed 's/^/    /'; done
echo "  putback: $(g '^PUTBACK OK' putback.txt || true)"
for n in 1 2; do grep -h '^CHECK .* FINDING ' $L/after$n.txt 2>/dev/null | sed "s/^CHECK /  FINDING (#8429) after round $n: /"; done
[ -n "$why" ] && echo "  why FAIL:$why"
exit 0
