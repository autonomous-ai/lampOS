#!/bin/bash
# Mac, lock held (../run.sh lock). The #6449 ROW's way in, as the first builder asked on #6449 (07:59): the lamp
# stays on its installed stable build, and lamp-voice alone is bound over by #6449's head's lamp-voice (a local ARM64
# build: arm64-local/out/<name>/lamp-voice and its SHA256SUMS, ART=that folder). Checks it, copies it to /tmp/t6449 on
# the lamp, runs swap.lamp.sh. Ends SWAPPED OK | SWAP NOT OK (then ./restore.sh at once). Run by ../run.sh `swap`.
set -u; O=$(cd "$(dirname "$0")" && pwd); D=$O/../row; . $D/kit.sh
ART=${ART:-$O/art}; BINS="lamp-voice"
RENEW || exit 1
for b in $BINS; do
  [ -f $ART/$b ] || { echo "no $ART/$b"; exit 1; }
  want=$(awk -v b=$b '$2 == b || $2 == "*"b {print $1}' $ART/SHA256SUMS); got=$(shasum -a 256 $ART/$b | cut -d' ' -f1)
  [ -n "$want" ] && [ "$want" = "$got" ] || { echo "$b sha $got is not SHA256SUMS' ${want:-<none>}"; exit 1; }
  file $ART/$b | grep -q 'ARM aarch64' || { echo "$b is not ARM64"; exit 1; }
  echo "ART $b $got"; SHAS="${SHAS:-} $b=$got"
done
GATE lamp-voice || exit 1
SSHR -n $H 'mkdir -p /tmp/t6449' || exit 1
for b in $BINS; do $SCP -q $ART/$b $H:/tmp/t6449/$b || { echo "copy $b failed"; exit 1; }; done
LAMP "SHAS='${SHAS# }'" $O/swap-common.lamp.sh $O/swap.lamp.sh 2>&1 | tee $D/logs/swap.txt
grep -q '^SWAPPED OK' $D/logs/swap.txt && echo "next: ../run.sh check" || echo "SWAP NOT OK: ./restore.sh now"
