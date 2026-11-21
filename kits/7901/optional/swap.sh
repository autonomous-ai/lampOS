#!/bin/bash
# OPTIONAL PATH ONLY: a binary swap, for a lamp where no test-channel round can be made. The ROW's way in is the
# runner's real round (RUN-7901.md); a swap is no ROW of the update path. Mac, lock held (../run.sh lock).
# art/ (this folder) holds lamp-voice, lamp-skills, lamp-control, lamp-tasks and SHA256SUMS from #7901's CI ARM64
# artifact. Checks them, copies them to /tmp/t7901 on the lamp, runs swap.lamp.sh. Ends SWAPPED OK | SWAP NOT OK (then
# ./restore.sh at once). Then go on at ../run.sh check.
set -u; O=$(cd "$(dirname "$0")" && pwd); D=$O/../row; . $D/kit.sh
ART=${ART:-$O/art}; BINS="lamp-voice lamp-skills lamp-control lamp-tasks"
RENEW || exit 1
for b in $BINS; do
  [ -f $ART/$b ] || { echo "no $ART/$b"; exit 1; }
  want=$(awk -v b=$b '$2 == b || $2 == "*"b {print $1}' $ART/SHA256SUMS); got=$(shasum -a 256 $ART/$b | cut -d' ' -f1)
  [ -n "$want" ] && [ "$want" = "$got" ] || { echo "$b sha $got is not SHA256SUMS' ${want:-<none>}"; exit 1; }
  file $ART/$b | grep -q 'ARM aarch64' || { echo "$b is not ARM64"; exit 1; }
  echo "ART $b $got"; SHAS="${SHAS:-} $b=$got"
done
GATE lamp-voice lamp-skills lamp-control lamp-tasks || exit 1
$SSH -n $H 'mkdir -p /tmp/t7901' || exit 1
for b in $BINS; do $SCP -q $ART/$b $H:/tmp/t7901/$b || { echo "copy $b failed"; exit 1; }; done
LAMP "SHAS='${SHAS# }'" $O/swap-common.lamp.sh $O/swap.lamp.sh 2>&1 | tee $D/logs/swap.txt
grep -q '^SWAPPED OK' $D/logs/swap.txt && echo "next: ../run.sh check (with PR_*_SHA from art/SHA256SUMS)" || echo "SWAP NOT OK: ./restore.sh now"
