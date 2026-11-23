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
# every leg (23 Nov): LAMP_NAME_NEAR=off, a /run drop-in laid by save and removed by unrow. The clips are the Mac's
# ElevenLabs voice, which lamp-name may not take for the lamp's name, so an ask was held as "nobody spoke to the
# lamp" (#8318; e82d 22 Nov: A, B, K, H). The first builder's fix for Mac-voice kits (#6449, 23 Nov 07:59).
ND_NAMENEAR=/run/systemd/system/lamp-voice.service.d/zz-row-8180-namenear.conf
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
# The lamp's engine: the app's choice (engine-default.json) when there is one, else the units' LAMP_LAMP_ENGINE
# (lamp-tasks', then lamp-voice's). Empty only when both are missing.
engine(){ local e u; e=$(grep -oE '"engine" *: *"[a-z]+"' $EF 2>/dev/null | grep -oE '"[a-z]+"$' | tr -d '"')
  for u in lamp-tasks lamp-voice; do [ -n "$e" ] && break; e=$(envval "$(pid $u)" LAMP_LAMP_ENGINE); done; echo "$e"; }
# The Google mail links lamp-services holds (leg D's mail make needs one): its store, /var/lib/lampos-services/<code>.json
# (crates/services/src/store.rs:20 DIR, :292 path, :319 load: the file's connector is its code), for gmail and
# gmail-app (catalog.rs:70, :92); one is linked unless store.rs:358 list() would call it expired (refused, or past its
# expires_at with no refresh token). Prints "google <code ...>" for the usable ones, or "google none"; never a secret.
SVC=/var/lib/lampos-services
linked(){ python3 -I -c 'import json,sys,time,os
ok=[]; seen=[]
for c in ("gmail","gmail-app"):
    p=os.path.join(sys.argv[1],c+".json")
    try: e=json.load(open(p))
    except Exception: continue
    if e.get("connector")!=c: continue
    x=int(e.get("expires_at") or 0); gone=bool(e.get("refused")) or (x!=0 and x<time.time() and not e.get("refresh_token"))
    seen.append(c+("=expired" if gone else "")); ok+=[] if gone else [c]
print("google %s (files: %s)" % (" ".join(ok) or "none", " ".join(seen) or "none"))' $SVC; }
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
# The row's fixtures (legs C, H, P) go in by the product's push install, as lamp-control's door keeps a push it took,
# with only the network door (the Mac's pairing) skipped (the #8237 kit's stage.lamp.sh, thirteen/kit-8237 5b5e8e4291):
# /var/lib/lampos-control/push/{push.json, package.tar} (skills package.rs PUSH_DIR, read within 60 s), then
# {"install_pushed": SHA} to root's manage helper; lamp-skills' install_pushed_at puts the skill in skills-dev
# (lib.rs PUSHED_DIR), where the loader lets it carry a script (task.rs: pushed or from the Store). A hand install
# (`lamp-skills install`) is refused that ("only a skill from the Skills Store or pushed by the owner's own computer
# may carry a script"). Removed by {"remove_pushed": {client, name}} (probe.lamp.sh OP=remove, unrow).
PUSHD=/var/lib/lampos-control/push; DEV=/var/lib/lampOS/skills-dev; YESF=/var/lib/lampos-control/task-yes.json
MSOCK=/run/lampos-skills-manage/manage.sock
PCLIENT=${PCLIENT:-c_row8180}                       # the stand-in computer's id (the helper's c_... shape)
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
AD=/run/systemd/system/lamp-skills-manage@.service.d/zz-row-8180-anyasker.conf
any_on(){ mkdir -p ${AD%/*}; printf '[Service]\nEnvironment=LAMP_SKILLS_ANY_ASKER=on\n' > $AD; systemctl daemon-reload; n "added $AD"; }
any_off(){ [ -f $AD ] && { rm -f $AD; rmdir ${AD%/*} 2>/dev/null; systemctl daemon-reload; n "removed $AD"; }; [ ! -f $AD ]; }
mhelper(){ any_on; r=$(mask "$1"); any_off || r="$r (ANY_ASKER drop-in NOT removed)"; echo "$r"; }
# The speaker's volume, READ ONLY (save records it, unrow compares; the kit never sets a volume): lamp-actions' kept
# level (actions kept_volume.rs FILE: one line, the raw level 1..100, written after each change the person asks for)
# and its bus value (speaker/volume, msgs speaker_state.rs SpeakerVolume: volume 0..100 of the lamp's ceiling, muted).
# One line: "kept=<n|none> bus=<n|none> muted=<true|false|none>".
KVOL=/var/lib/lampos-output-caps/kept-volume; BVOL=/run/lampos-bus/speaker/volume/latest
volnow(){ python3 -I - "$KVOL" "$BVOL" <<'PY'
import json, sys
try: k = open(sys.argv[1]).read().strip() or "none"
except Exception: k = "none"
try:
    _, _, p = open(sys.argv[2], "rb").read().partition(b"\n"); v = json.loads(p)
    b = str(v.get("volume")); m = str(v.get("muted")).lower()
except Exception: b = m = "none"
print("kept=%s bus=%s muted=%s" % (k, b, m))
PY
}
