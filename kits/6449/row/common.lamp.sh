# lamp-side prelude (root sh) of the #6449 row, cat'ed before each row/ lamp body by row/kit.sh's LAMP (which puts
# LAMP_USER and LOCK_WHO first). Defines only. The lamp stays on its installed stable build; only lamp-voice is
# bound over by #6449's head's lamp-voice (`swap`), and `restore` puts the installed one back.
set -u
LAMP_USER=${LAMP_USER:-row}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-row} #6449 row: $*" >> $C; }
R=$LH/t6449; mkdir -p $R                               # the row's baselines and records
# part:unit:binary swapped (#6449 changes the product unit only; its head's lamp-voice is what homes would run)
PARTS="voice:lamp-voice:lamp-voice"
pid(){ systemctl show -p MainPID --value $1; }
envval(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null | sed -n "s/^$2=//p"; }
# The switch #6449 turns on, laid by `on` as a /run drop-in (gone at reboot), removed by unrow. LAMP_NAME_NEAR=off
# there too: the clips are the Mac's voice, which lamp-name may not take for the lamp's name (the first builder, 07:59).
DROP=/run/systemd/system/lamp-voice.service.d/zz-row-6449-worded.conf
# What a try changes and the next must not see: the person's kept rules and memory, the schedule, the light kept.
# Saved once by `save`, put back before each try (`fresh`) and by unrow.
SNAP="/var/lib/lampos-memory /var/lib/lampos-schedule /var/lib/lampOS/light"
SNAPUNITS="lamp-memory lamp-schedule"
UF=/var/lib/lampos-update; UST=$UF/state.json
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence", ""))' $UST 2>/dev/null; }
updbusy(){ w=; [ -e $UF/journal.json ] && w="$w journal.json"; [ -e $UF/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
# The voice's software (privacy) mute record and its live bus values (read only; micon/micoff use them).
MR=/var/lib/lampOS/voice/mute/record.json
mute_live(){ python3 -I - <<'PY'
import json, time
def kept(t):
    try:
        h, _, p = open("/run/lampos-bus/%s/latest" % t, "rb").read().partition(b"\n"); h = json.loads(h)
        return json.loads(p), (time.time() * 1000 - h.get("at_unix_ms", 0)) / 1000.0, h.get("seq")
    except Exception as e:
        return None, None, type(e).__name__
m, ma, ms = kept("voice/mute"); p, pa, ps = kept("voice/privacy")
print("VOICE_MUTE muted=%s reason=%s muted_by=%s control=%s age_s=%s seq=%s" % (
    (m or {}).get("muted"), (m or {}).get("reason"), (m or {}).get("muted_by"), (m or {}).get("control"), "%.1f" % ma if ma is not None else "-", ms))
print("PRIVACY permitted=%s physical_muted=%s age_s=%s" % ((p or {}).get("permitted"), (p or {}).get("physical_muted"), "%.1f" % pa if pa is not None else "-"))
PY
}
