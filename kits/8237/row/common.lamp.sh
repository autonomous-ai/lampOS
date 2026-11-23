# lamp-side prelude (root sh) of the #8237 row, cat'ed before each row/ lamp body by row/kit.sh's LAMP (which puts
# LAMP_USER and LOCK_WHO first). Defines only.
set -u
LAMP_USER=${LAMP_USER:-row}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-row} #8237 row: $*" >> $C; }
R=$LH/t8237; mkdir -p $R                               # the row's records (stable's backups and sha256s)
# part:unit:binary of the two parts #8237 changes. Installed at /opt/lampOS/<part>/current/<bin>, each unit's
# ExecStart (deploy/product/lamp-skills.service:144, deploy/product/lamp-tasks.service:104).
PUTP="skills:lamp-skills:lamp-skills tasks:lamp-tasks:lamp-tasks"
inst(){ readlink -f /opt/lampOS/$1/current/$2; }
pid(){ systemctl show -p MainPID --value $1; }
envval(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null | sed -n "s/^$2=//p"; }
sha(){ sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
EF=/var/lib/lampos-tasks/store/engine-default.json
UF=/var/lib/lampos-update; UST=$UF/state.json
# lamp-update's apply held while #8237's bytes are on (as the #7901 kit's swap does): a round mid-row would write
# over them, and putback would then put older bytes back over a newer release.
HD=/run/systemd/system/lamp-update.service.d/zz-row-8237-hold.conf
# LAMP_NAME_NEAR=off for lamp-voice while the Mac's speaker asks (msgs/src/lib.rs:1714 NEAR_ENVIRONMENT; on in the
# product, deploy/product/lamp-voice.service:148). A /run drop-in (gone at reboot), laid by near MODE=on, removed by
# MODE=off (unrow).
ND=/run/systemd/system/lamp-voice.service.d/zz-row-8237-namenear.conf
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence", ""))' $UST 2>/dev/null; }
updbusy(){ w=; [ -e $UF/journal.json ] && w="$w journal.json"; [ -e $UF/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
# The lamp's engine: the app's choice (engine-default.json) when there is one, else the units' LAMP_LAMP_ENGINE.
engine(){ local e u; e=$(grep -oE '"engine" *: *"[a-z]+"' $EF 2>/dev/null | grep -oE '"[a-z]+"$' | tr -d '"')
  for u in lamp-tasks lamp-voice; do [ -n "$e" ] && break; e=$(envval "$(pid $u)" LAMP_LAMP_ENGINE); done; echo "$e"; }
# waits until each unit has a pid and is active, at most $1 s
upwait(){ local s=0 u ok lim=$1; shift; while [ $s -lt $lim ]; do ok=1
  for u in "$@"; do [ "$(systemctl is-active $u)" = active ] && [ "$(pid $u)" != 0 ] || ok=0; done
  [ $ok = 1 ] && return 0; sleep 1; s=$((s + 1)); done; return 1; }
# The voice's software (privacy) mute, live (read only).
MR=/var/lib/lampOS/voice/mute/record.json
mute_live(){ python3 -I - <<'PY'
import json, time
def kept(t):
    try:
        h, _, p = open("/run/lampos-bus/%s/latest" % t, "rb").read().partition(b"\n"); h = json.loads(h)
        return json.loads(p), (time.time() * 1000 - h.get("at_unix_ms", 0)) / 1000.0
    except Exception:
        return None, None
m, ma = kept("voice/mute"); p, pa = kept("voice/privacy")
print("VOICE_MUTE muted=%s reason=%s" % ((m or {}).get("muted"), (m or {}).get("reason")))
print("PRIVACY physical_muted=%s" % (p or {}).get("physical_muted"))
PY
}
# The lamp's speaker volume, read only, never set (amixer card 0's first playback control, or SPK_CTL).
speaker(){ c=${SPK_CTL:-}
  [ -n "$c" ] || c=$(amixer -c 0 scontrols 2>/dev/null | sed -n "s/^Simple mixer control '\(.*\)',\([0-9]*\)$/\1,\2/p" | while IFS= read -r x; do amixer -c 0 sget "$x" 2>/dev/null | grep -q 'pvolume' && { echo "$x"; break; }; done)
  p=$([ -n "$c" ] && amixer -c 0 sget "$c" 2>/dev/null | grep -E 'Playback.*\[[0-9]+%\]' | grep -oE '\[[0-9]+%\]' | head -1 | tr -d '[]%')
  [ -n "$p" ] && echo "SPEAKER $(echo "$c" | tr ' ' _) $p%" || echo "SPEAKER unread"; }
