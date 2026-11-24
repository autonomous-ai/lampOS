#!/bin/bash
# Mac, lock held (../run.sh lock). #7901's ROW on a lamp that stays on its installed stable build (ee17, from the
# #8437 kit's swap): #7901's four runtime parts (lamp-tasks, lamp-control, lamp-skills, lamp-voice) are bound over the
# installed ones, from #7901's CI ARM64 artifact (ART=a folder with the four binaries and their SHA256SUMS). #7901
# changes no unit line; the drop-ins it lays are #8180's (stable without #8180). Checks each, copies
# them to /tmp/t7901 on the lamp, runs swap.lamp.sh. Ends SWAPPED OK | SWAP NOT OK (then ./restore.sh at once). Run by ../row7901-ee17.sh.
set -u; O=$(cd "$(dirname "$0")" && pwd); D=$O/../row; . $D/kit.sh
ART=${ART:-$O/art}; BINS="lamp-tasks lamp-control lamp-skills lamp-voice"
RENEW || exit 1; UNITS_MAC="lamp-voice lamp-skills lamp-control lamp-tasks lamp-openclaw-gateway"
for b in $BINS; do
  [ -f $ART/$b ] || { echo "no $ART/$b"; exit 1; }
  want=$(awk -v b=$b '$2 == b || $2 == "*"b {print $1}' $ART/SHA256SUMS); got=$(shasum -a 256 $ART/$b | cut -d' ' -f1)
  [ -n "$want" ] && [ "$want" = "$got" ] || { echo "$b sha $got is not SHA256SUMS' ${want:-<none>}"; exit 1; }
  file $ART/$b | grep -q 'ARM aarch64' || { echo "$b is not ARM64"; exit 1; }
  echo "ART $b $got"; SHAS="${SHAS:-} $b=$got"
done
GATE $UNITS_MAC || exit 1
SSHR -n $H 'mkdir -p /tmp/t7901' || exit 1
for b in $BINS; do $SCP -q $ART/$b $H:/tmp/t7901/$b || { echo "copy $b failed"; exit 1; }; done
LAMP "SHAS='${SHAS# }'" $O/swap-common.lamp.sh $O/swap.lamp.sh 2>&1 | tee $D/logs/swap.txt
grep -q '^SWAPPED OK' $D/logs/swap.txt && echo "next: ../run.sh check" || echo "SWAP NOT OK: ./restore.sh now"
