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
# every leg (23 Nov): LAMP_NAME_NEAR=off, a /run drop-in laid by save and removed by unrow. The clips are the Mac's
# ElevenLabs voice, which lamp-name may not take for the lamp's name, so an ask is held as "nobody spoke to the lamp"
# (#8318; #8180's e82d run, 22 Nov). The first builder's fix for Mac-voice kits (#6449, 23 Nov 07:59).
ND_NAMENEAR=/run/systemd/system/lamp-voice.service.d/zz-row-7901-namenear.conf
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
# Leg K's fixture goes in by the product's push install, as lamp-control's door keeps a push it took, with only the
# network door (the Mac's pairing) skipped (the #8180 ee17 kit's, twelve/kit-8180-ee17 9c7e115ad1):
# /var/lib/lampos-control/push/{push.json, package.tar} (skills package.rs PUSH_DIR, read within 60 s), then
# {"install_pushed": SHA} to root's manage helper; lamp-skills' install_pushed_at puts the skill in skills-dev
# (lib.rs PUSHED_DIR). Removed by {"remove_pushed": {client, name}} (push.lamp.sh OP=remove, unrow).
PUSHD=/var/lib/lampos-control/push; DEV=/var/lib/lampOS/skills-dev; YESF=/var/lib/lampos-control/task-yes.json
MSOCK=/run/lampos-skills-manage/manage.sock
PCLIENT=${PCLIENT:-c_row7901}                       # the stand-in computer's id (the helper's c_... shape)
PUSHED=$R/pushed.names                              # the row's pushed fixtures, one name each, for unrow
sha(){ sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
mask(){ python3 -I - "$MSOCK" "$1" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(40); s.connect(sys.argv[1]); s.sendall(sys.argv[2].encode() + b"\n")
b = b""
while not b.endswith(b"\n"):
    c = s.recv(65536)
    if not c: break
    b += c
print(b.decode(errors="replace").strip())
PY
}
okreply(){ python3 -I -c 'import json,sys; v=json.loads(sys.argv[1]); sys.exit(0 if "done" in v and "refused" not in v else 1)' "$1" 2>/dev/null; }
# The helper takes pushed requests from lamp-control's cgroup only (package.rs asker_of). For the one request, its own
# test switch LAMP_SKILLS_ANY_ASKER=on (main.rs "asked": said in the journal each time) by a /run drop-in on
# lamp-skills-manage@ (socket-activated, so the next connection reads it), removed right after the answer.
AD=/run/systemd/system/lamp-skills-manage@.service.d/zz-row-7901-anyasker.conf
any_on(){ mkdir -p ${AD%/*}; printf '[Service]\nEnvironment=LAMP_SKILLS_ANY_ASKER=on\n' > $AD; systemctl daemon-reload; n "added $AD"; }
any_off(){ [ -f $AD ] && { rm -f $AD; rmdir ${AD%/*} 2>/dev/null; systemctl daemon-reload; n "removed $AD"; }; [ ! -f $AD ]; }
mhelper(){ any_on; r=$(mask "$1"); any_off || r="$r (ANY_ASKER drop-in NOT removed)"; echo "$r"; }
