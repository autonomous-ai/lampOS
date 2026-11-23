#!/bin/sh
# In the rig, after leg 1: the owner's hand, as leg 2 asks.
#   D1  deletes lamp-npu.service, the unit leg 1 added: `systemctl disable --now`, the file
#       removed, daemon-reload (what an owner who does not want it does).
#   E1  edits lamp-apps-start.service, also added by leg 1 (else the first other unit leg 1
#       added), by one comment line at its end.
#   E0  edits lamp-eyes.service, a unit the lamp had on build A, the same way.
# The edited bytes are kept in out/leg2/ to compare after each later round.
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
D=$OUT/leg2
[ -e "$D" ] && die "$D is there already"
mkdir -p "$D"
[ -f "$UNITS/lamp-npu.service" ] || die "lamp-npu.service is not there: leg 1 did not add it"
mark "leg2 edits"
cp "$UNITS/lamp-npu.service" "$D/D1.lamp-npu.service.before-delete"
systemctl disable --now lamp-npu.service
rm -f "$UNITS/lamp-npu.service" "$UNITS/lampos.target.wants/lamp-npu.service"
systemctl stop lamp-npu.service 2>/dev/null
systemctl daemon-reload
[ -e "$UNITS/lamp-npu.service" ] || [ -e "$UNITS/lampos.target.wants/lamp-npu.service" ] && die "lamp-npu.service is still there"
echo "lamp-npu.service" > "$D/D1"
e1=""
if [ -f "$UNITS/lamp-apps-start.service" ]; then e1=lamp-apps-start.service; else
  e1=$(grep -o 'adds [^:]*' "$OUT"/r1*/journal.txt | head -1 | sed 's/^.*adds //' | tr ',' '\n' | tr -d ' ' \
       | grep -v '^lamp-npu.service$' | grep '\.service$' | grep -v '@' | head -1)
fi
[ -n "$e1" ] && [ -f "$UNITS/$e1" ] || die "no second added unit to edit"
for pair in "E1 $e1" "E0 lamp-eyes.service"; do
  tag=${pair% *}; u=${pair#* }
  [ -f "$UNITS/$u" ] && [ ! -L "$UNITS/$u" ] || die "$u is not a regular file"
  printf '# el-8400 leg 2: the owner edited this unit (%s)\n' "$(date -u +%FT%TZ)" >> "$UNITS/$u"
  cp "$UNITS/$u" "$D/$tag.$u.edited"
  echo "$u" > "$D/$tag"
  say "$tag: $u edited, now $(sha256sum "$UNITS/$u" | cut -c1-16)"
done
say "D1: lamp-npu.service and its lampos.target.wants link removed"
