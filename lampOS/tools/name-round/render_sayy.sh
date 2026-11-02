#!/bin/zsh
# More negatives: everyday sentences with no name, each in five of the Mac's English `say` voices (four at a time).
O=/Users/autonomous/lamp-name-round/work/sayy; mkdir -p $O; : > $O/index.tsv
tr '\t' '\n' < /Users/autonomous/lamp-name-round/work/sayy_plan5.tsv | tr '\n' '\0' | xargs -0 -n 3 -P 4 /Users/autonomous/lamp-name-round/render_one.sh
print "rendered $(wc -l < $O/index.tsv)"
