# lamp, root sh (common first). LAMP_NAME_NEAR=off for lamp-voice in its own /run drop-in, for the ROW's voice legs
# (the product unit says LAMP_NAME_NEAR=on: deploy/product/lamp-voice.service). Modelled on legs8213's agent-first
# drop-in (x.sh `first on|off`): one lamp-voice restart each way, the running env read back. Needs ACT=on|off and TAG
# prepended. Removed in that piece's put-back. Prints NEAR OFF SET | NEAR REMOVED | NEAR none to remove | NEAR ... FAIL.
cd /
FDN=/run/systemd/system/lamp-voice.service.d/zz-sd-split-near.conf
envnow(){ p=$(systemctl show -p MainPID --value lamp-voice); tr '\0' '\n' < /proc/$p/environ 2>/dev/null | sed -n 's/^LAMP_NAME_NEAR=//p'; }
case $ACT in
on) [ -e $FDN ] && { echo "NEAR STOP: $FDN is already there (an earlier run not put back)"; exit 3; }
    echo "NEAR BEFORE LAMP_NAME_NEAR=$(envnow)"
    mkdir -p ${FDN%/*}
    printf '# split ROW (%s): LAMP_NAME_NEAR=off for this ROW only. /run only; removed by the piece put-back.\n[Service]\nEnvironment=LAMP_NAME_NEAR=off\n' "$TAG" > $FDN
    systemctl daemon-reload; systemctl restart lamp-voice; n "lamp-voice drop-in $FDN: LAMP_NAME_NEAR=off ($TAG); restarted"; sleep 15
    v=$(envnow); [ "$v" = off ] && echo "NEAR OFF SET $FDN lamp-voice $(systemctl is-active lamp-voice) env LAMP_NAME_NEAR=off" || echo "NEAR SET FAIL env LAMP_NAME_NEAR=${v:-unset}" ;;
off) [ -e $FDN ] || { echo "NEAR none to remove"; exit 0; }
    rm -f $FDN; systemctl daemon-reload; systemctl restart lamp-voice; n "lamp-voice drop-in $FDN removed ($TAG); restarted"; sleep 15
    v=$(envnow); [ ! -e $FDN ] && [ "$v" != off ] && echo "NEAR REMOVED lamp-voice $(systemctl is-active lamp-voice) env LAMP_NAME_NEAR=${v:-unset}" || echo "NEAR REMOVE FAIL env LAMP_NAME_NEAR=${v:-unset}" ;;
*) echo "NEAR ACT?"; exit 2 ;;
esac
