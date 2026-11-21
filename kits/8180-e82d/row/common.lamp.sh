# lamp-side prelude (root sh) of the row legs, cat'ed before each row/ lamp body by row/kit.sh's LAMP (which puts
# LAMP_USER and LOCK_WHO first). Defines only. #8180 legs kit: the PR build is whatever the runner installed (their own
# /run drop-in of #8180's CI ARM64 artifact); this kit never installs or puts back a build.
set -u
LAMP_USER=${LAMP_USER:-orangepi}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-row} #8180 row: $*" >> $C; }
R=$LH/t8180; mkdir -p $R                               # the row's baselines and records
UNITS="lamp-voice lamp-skills lamp-control lamp-tasks"
pid(){ systemctl show -p MainPID --value $1; }
envval(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null | sed -n "s/^$2=//p"; }
# leg E only: LAMP_MAKE_MAY_RUN_ON=on, a /run drop-in (gone at reboot), removed by runon off and by unrow
ND_RUNON=/run/systemd/system/lamp-voice.service.d/zz-row-8180-runon.conf
# leg N only (#7269): LAMP_NAMED_AGENT_TAKES_LINE=on, a /run drop-in (gone at reboot), removed by named off, by unrow,
# by row.sh's EXIT trap and by run.sh's EXIT guard (NAMEDGUARD)
ND_NAMED=/run/systemd/system/lamp-voice.service.d/zz-row-8180-named.conf
# product switches the rows rest on (lamp-voice's environment must have them =on)
NEED="LAMP_MAKE_SKILL LAMP_MAKE_SKILL_AGENT LAMP_SKILL_TASKS LAMP_MADE_SCRIPTS LAMP_TASK_NET LAMP_TASK_NET_GATEWAY"
MS=/var/lib/lampOS/voice/made-skills
SK=/var/lib/lampOS/skills; SKST=/var/lib/lampos-skills-state
# The lamp-skills binary the unit runs now (the runner's drop-in may point it at the artifact), else the product's path.
SKBIN=$(systemctl show -p ExecStart --value lamp-skills 2>/dev/null | sed -n 's/.*path=\([^ ;]*\).*/\1/p' | head -1)
[ -x "${SKBIN:-/nonexistent}" ] || SKBIN=/opt/lampOS/skills/current/lamp-skills
EF=/var/lib/lampos-tasks/store/engine-default.json
UF=/var/lib/lampos-update; UST=$UF/state.json
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence", ""))' $UST 2>/dev/null; }
updbusy(){ w=; [ -e $UF/journal.json ] && w="$w journal.json"; [ -e $UF/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
engine(){ grep -oE '"engine" *: *"[a-z]+"' $EF 2>/dev/null | grep -oE '"[a-z]+"$' | tr -d '"'; }
skillnames(){ ls -1 $SK 2>/dev/null | LC_ALL=C sort; }
madenames(){ python3 -I -c 'import json,sys
try: print("\n".join(e.get("name","") for e in json.load(open(sys.argv[1]))))
except FileNotFoundError: pass' $MS/made.json 2>/dev/null; }
# lamp-tasks' cgroup (#8180): the unit's ControlGroup, and a pid's own cgroup path (v2 "0::<path>").
cgunit(){ systemctl show -p ControlGroup --value lamp-tasks; }
cgof(){ sed -n 's/^0:://p' /proc/$1/cgroup 2>/dev/null; }
# The voice's software (privacy) mute record and its live bus values (read only; micon/micoff/boot use them).
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
