# lamp, root sh (common first). The row's baselines: the state a try changes ($SNAP: the person's kept rules and
# memory, the schedule, the light kept), copied once to $R/snap, so each try starts as the lamp was (`fresh`) and
# unrow puts it back. Read only besides $R. Prints SAVED OK | SAVE FAIL.
set -e; rm -rf $R/snap; mkdir -p $R/snap
for d in $SNAP; do if [ -d $d ]; then mkdir -p $R/snap$(dirname $d); cp -a $d $R/snap$d; echo "SNAP $d $(du -sk $d | cut -f1) KiB"
  else echo "SNAP $d: none on this lamp"; fi; done
echo "BUILD $(build)"; echo "SAVED OK"
n "save: $SNAP copied to $R/snap (the row's baseline; put back by fresh and unrow)"
