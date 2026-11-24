# lamp-side prelude (root sh) of the ee17 promote gate, cat'ed before each gate/*.lamp.sh by kit.sh's LAMP (which
# puts LAMP_USER and LOCK_WHO first). Defines only. Never a volume, never lamp-cloud, never a reset or forget-all.
set -u
LAMP_USER=${LAMP_USER:-orangepi}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-gate} gate-ee17: $*" >> $C; }
R=$LH/tgate; mkdir -p $R; chmod 700 $R                 # the gate's records on the lamp
UF=/var/lib/lampos-update; UST=$UF/state.json
UPDBIN=/opt/lampOS/update/current/lamp-update
pid(){ systemctl show -p MainPID --value $1; }
# lamp-update's records (records.rs State): "installed=<seq> previous=<seq|none> refused=<seq,...>"
stateval(){ python3 -I - "$UST" <<'P'
import json, sys
try: d = json.load(open(sys.argv[1]))
except Exception as e: print("installed=unread previous=unread refused=unread"); sys.exit(0)
def seq(b): return str(b.get("sequence")) if isinstance(b, dict) else "none"
r = ",".join(str(x.get("sequence")) for x in (d.get("refused") or []) if isinstance(x, dict))
print("installed=%s previous=%s refused=%s" % (seq(d.get("installed")), seq(d.get("previous")), r or "none"))
P
}
updbusy(){ w=; [ -e $UF/journal.json ] && w="$w journal.json"; [ -e $UF/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
updsha(){ sha256sum "$(readlink -f $UPDBIN)" 2>/dev/null | cut -d' ' -f1; }
GW=lamp-openclaw-gateway; GWPORT=4965                 # 18789 = 0x4965, as /proc/net/tcp writes a local port
listening(){ cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk -v p=":$GWPORT" '$2 ~ p"$" && $4 == "0A" {f=1} END {exit !f}'; }
gwlisten(){ i=0; while [ $i -lt $1 ]; do listening && { echo "GATEWAY listening after ${i}s"; return 0; }; sleep 2; i=$((i+2)); done; echo "GATEWAY not listening after $1 s"; return 1; }
CTL=/run/lampos-control/control.sock; GWCTL=${GWCTL:-$CTL}
# The speaker's volume, READ ONLY (the 8180-ee17 kit's volnow): lamp-actions' kept level and the bus's speaker/volume.
KVOL=/var/lib/lampos-output-caps/kept-volume; BVOL=/run/lampos-bus/speaker/volume/latest
