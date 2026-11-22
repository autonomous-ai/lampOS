# lamp-side prelude (root sh) of the #7901 row, cat'ed before each row/ lamp body by row/kit.sh's LAMP (which puts
# LAMP_USER and LOCK_WHO first). Defines only. The PR build is the runner's: installed by a real round from a test
# channel before the kit, and rounded back to stable's bytes after it. This kit never installs or puts back a build.
set -u
LAMP_USER=${LAMP_USER:-row}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-row} #7901 row: $*" >> $C; }
R=$LH/t7901; mkdir -p $R                               # the row's baselines and records
# part:unit:binary of the four parts #7901 changes
PARTS="voice:lamp-voice:lamp-voice skills:lamp-skills:lamp-skills control:lamp-control:lamp-control tasks:lamp-tasks:lamp-tasks"
pid(){ systemctl show -p MainPID --value $1; }
envval(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null | sed -n "s/^$2=//p"; }
# LAMP_MADE_NOTES is off in the product ("until" its lamp check, docs/switches.md); legs A and B need it in lamp-voice
# (the make is told lamp.notes) and lamp-control (a run's notes kept under its skill's client n_<16 hex>, so run 2
# reads run 1's). /run drop-ins (gone at reboot), laid by `notes` (MODE=on), removed by MODE=off and by unrow.
ND_VOICE=/run/systemd/system/lamp-voice.service.d/zz-row-7901-notes.conf
ND_CONTROL=/run/systemd/system/lamp-control.service.d/zz-row-7901-notes.conf
# product switches the row rests on (lamp-voice's environment must have them =on)
NEED="LAMP_TASK_NET LAMP_TASK_NET_GATEWAY LAMP_SKILL_TASKS LAMP_MADE_SCRIPTS"
MS=/var/lib/lampOS/voice/made-skills
SK=/var/lib/lampOS/skills; SKST=/var/lib/lampos-skills-state
NOTES=/var/lib/lampos-control/notes
SKBIN=/opt/lampOS/skills/current/lamp-skills
EF=/var/lib/lampos-tasks/store/engine-default.json
UF=/var/lib/lampos-update; UST=$UF/state.json
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence", ""))' $UST 2>/dev/null; }
updbusy(){ w=; [ -e $UF/journal.json ] && w="$w journal.json"; [ -e $UF/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
# The lamp's engine: the app's choice (engine-default.json) when there is one, else the units' LAMP_LAMP_ENGINE
# (lamp-tasks', then lamp-voice's). Empty only when both are missing.
engine(){ local e u; e=$(grep -oE '"engine" *: *"[a-z]+"' $EF 2>/dev/null | grep -oE '"[a-z]+"$' | tr -d '"')
  for u in lamp-tasks lamp-voice; do [ -n "$e" ] && break; e=$(envval "$(pid $u)" LAMP_LAMP_ENGINE); done; echo "$e"; }
skillnames(){ ls -1 $SK 2>/dev/null | LC_ALL=C sort; }
madenames(){ python3 -I -c 'import json,sys
try: print("\n".join(e.get("name","") for e in json.load(open(sys.argv[1]))))
except FileNotFoundError: pass' $MS/made.json 2>/dev/null; }
notesfiles(){ ls -1 $NOTES 2>/dev/null | LC_ALL=C sort; }
cgunit(){ systemctl show -p ControlGroup --value lamp-tasks; }
cgof(){ sed -n 's/^0:://p' /proc/$1/cgroup 2>/dev/null; }
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
