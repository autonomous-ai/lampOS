#!/bin/bash
# All mock cases, 6 at a time; one line each, then TOTAL. Exit 0 only when every case gives its expected result.
# Also bash -n on every Mac script and sh -n on every lamp body (with common.lamp.sh before it).
M=$(cd "$(dirname "$0")" && pwd); KIT=$(cd $M/.. && pwd); bad=0
for f in $KIT/run.sh $KIT/row/row.sh $KIT/row/kit.sh $KIT/row/render.sh $M/*.sh; do bash -n $f || { echo "SYNTAX $f"; bad=1; }; done
for f in $KIT/row/*.lamp.sh; do cat $KIT/row/common.lamp.sh $f | sh -n || { echo "SYNTAX $f"; bad=1; }; done
[ $bad = 0 ] && echo "SYNTAX OK (bash -n, sh -n)"
rm -rf $M/work; ls $M/fx | xargs -P ${J:-6} -n1 $M/run-case.sh | sort | tee $M/results.txt
ok=$(grep -c ' OK ' $M/results.txt); n=$(ls $M/fx | wc -l | tr -d ' ')
echo "TOTAL $n cases: OK $ok MISMATCH $((n - ok))" | tee -a $M/results.txt; [ "$ok" = "$n" ] && [ $bad = 0 ]
