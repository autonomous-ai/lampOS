#!/bin/sh
# In the rig: the lamp on build A (14976), as a factory image makes it (imager Phase 4 at A's
# commit: the build's own lamp-update `install --setup on` from a local channel). The lamp's
# key.pub is the PRODUCTION public key (fingerprint 54213f8d3aa2f241), as the rig recipe says;
# the test key is only ever passed with --key-file (#8369), never written into key.pub.
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
mkdir -p "$OUT/install"
[ -e /opt/lampOS/update/current ] && die "lamp-update is installed already: not a fresh rig"
[ -e "$REC/state.json" ] && die "$REC/state.json is there already"
install -D -m 0644 "$K/prod.pub" /etc/lampOS/update/key.pub
[ "$(fp /etc/lampOS/update/key.pub)" = "$PROD_FP" ] || die "key.pub is not the production key"
mark "install A $A_SEQ"
"$K/chanA/lamp-update" install --channel-url "file://$K/chanA/" --channel lamp-voice \
  --key-file "$K/test.pub" --setup on > "$OUT/install/install.log" 2>&1
code=$?
tail -n 15 "$OUT/install/install.log"
[ "$code" = 0 ] || die "install of A ended $code (whole log: out/install/install.log)"
cp "$REC/state.json" "$OUT/install/state.json"
inst=$(python3 -I -c "import json;print(json.load(open('$REC/state.json'))['installed']['sequence'])")
[ "$inst" = "$A_SEQ" ] || die "state.json says installed $inst, not $A_SEQ"
want=$(python3 -I -c "import json;print([f['sha256'] for f in json.load(open('$K/chanA/manifest.json'))['files'] if f['name']=='lamp-update'][0])")
cur=$(readlink -f /opt/lampOS/update/current)
[ "$cur" = "/opt/lampOS/update/$want" ] || die "update/current is $cur, not A's lamp-update $want"
[ -f "$UNITS/lamp-update.service" ] && [ ! -L "$UNITS/lamp-update.service" ] || die "lamp-update.service is not a regular file"
grep -q '^ExecStart=/opt/lampOS/update/current/lamp-update serve' "$UNITS/lamp-update.service" || die "lamp-update.service does not start update/current"
unit_env | grep -q 'LAMP_UPDATE_SELF=on' || die "lamp-update.service lacks LAMP_UPDATE_SELF=on"
[ -x /usr/local/lib/lampos/lamp-update-back ] || die "no /usr/local/lib/lampos/lamp-update-back"
[ "$(systemctl show --property=UnitFileState --value -- lamp-update-back.timer)" = enabled ] || die "lamp-update-back.timer is not enabled"
for u in lamp-npu.service lamp-apps-start.service lamp-apps-folders.service lamp-apps-manage.socket; do
  [ -e "$UNITS/$u" ] && die "$u is there on build A: A must lack it"
done
[ "$(fp /etc/lampOS/update/key.pub)" = "$PROD_FP" ] || die "install changed key.pub"
say "INSTALL-OK: build $A_SEQ, lamp-update $(echo "$want" | cut -c1-12), key.pub $PROD_FP, no lamp-npu/apps units"
