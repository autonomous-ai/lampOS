# lamp-side prelude (root sh), cat'ed before each row/ lamp body by row/kit.sh's LAMP (which puts LAMP_USER, LOCK_WHO
# and PR first). Defines only. #8162 ROW kit. Never touches the speaker volume; lamp-cloud is never stopped; no factory
# reset, no forget-all; a pid is signalled only by its exact number, never by pattern.
set -u
LAMP_USER=${LAMP_USER:-orangepi}; LH=/home/$LAMP_USER
C=$LH/changes.txt; n(){ echo "$(date '+%F %T') ${LOCK_WHO:-row} #8162 row: $*" >> $C; chown $LAMP_USER: $C 2>/dev/null || true; }
R=$LH/t8162; mkdir -p $R                            # the row's records and copies
say(){ echo "[t8162 $(date +%H:%M:%S)] $*"; }
pid(){ systemctl show -p MainPID --value $1; }
VP(){ pid lamp-$1; }
envof(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null; }
envval(){ envof "$1" | sed -n "s/^$2=//p"; }
# --- the update round (lampOS docs/updates.md; the earlier #8138 round kit's helpers)
IN=/tmp/t8162                                       # what the Mac copied, channel mode only: ch1/ ch2/ test.pub SHAS
CH=/var/lib/lampos-t8162                            # channel mode: the staged channels, readable in lamp-update's sandbox
REC=/var/lib/lampos-update; UST=$REC/state.json
UPD=/opt/lampOS/update
DROP=/run/systemd/system/lamp-update.service.d/zz-t8162.conf
DROPA=/run/systemd/system/lamp-update-apply.service.d/zz-t8162.conf
DROPS="$DROP $DROPA"
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence",""))' $UST 2>/dev/null; }
# The highest sequence the lamp has seen: installed, previous or refused (a round takes only a higher one).
seqmax(){ python3 -I -c 'import json,sys
s=json.load(open(sys.argv[1])); q=[(s.get(k) or {}).get("sequence") for k in ("installed","previous")]
q+=[r.get("sequence") for r in (s.get("refused") or []) if isinstance(r,dict)]
print(max([int(x) for x in q if x is not None] or [0]))' $UST 2>/dev/null; }
updbusy(){ w=; [ -e $REC/journal.json ] && w="$w journal.json"; [ -e $REC/self.json ] && w="$w self.json"
  a=$(systemctl is-active lamp-update-apply 2>/dev/null || true); case "$a" in active|activating|reloading) w="$w lamp-update-apply=$a";; esac; echo "${w# }"; }
exesha(){ p=$(pid $1); [ "${p:-0}" != 0 ] && sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1 || echo none; }
line(){ systemctl show -p ExecStart --value lamp-update.service | grep -oE 'lamp-update serve [^;]*' | head -1; }
product_line(){ line | grep -qE -- '--channel-url https://(github.com/autonomous-ai/lampOS/releases/download/lampos-stable/|cdn.autonomous.ai/lampos/stable/) --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on'; }
dropins(){ for u in lamp-update lamp-update-apply lamp-update-writer lamp-update-back lamp-update-harness lamp-tasks; do d=$(systemctl show -p DropInPaths --value $u.service 2>/dev/null | tr " " "\n" | grep -v "/etc/systemd/system/$u.service.d/exit-note.conf$" | tr "\n" " " | sed "s/ *$//"); [ -n "$d" ] && echo "DROPIN $u: $d"; done; }
failed(){ systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | LC_ALL=C sort | tr '\n' ' '; }
# The lamp's speaker volume, READ ONLY (amixer -c 0 sget; never set): SPK_CTL, else the card's first control with a
# playback volume. Prints SPEAKER <control> <n>% | SPEAKER unread.
speaker(){ c=${SPK_CTL:-}
  [ -n "$c" ] || c=$(amixer -c 0 scontrols 2>/dev/null | sed -n "s/^Simple mixer control '\(.*\)',\([0-9]*\)$/\1,\2/p" | while IFS= read -r x; do amixer -c 0 sget "$x" 2>/dev/null | grep -q 'pvolume' && { echo "$x"; break; }; done)
  p=$([ -n "$c" ] && amixer -c 0 sget "$c" 2>/dev/null | grep -E 'Playback.*\[[0-9]+%\]' | grep -oE '\[[0-9]+%\]' | head -1 | tr -d '[]%')
  [ -n "$p" ] && echo "SPEAKER $(echo "$c" | tr ' ' _) $p%" || echo "SPEAKER unread"; }
# The running exe's sha256 of the units the put-back must return to the same bytes.
exeshas(){ echo "EXESHA lamp-update=$(exesha lamp-update) lamp-tasks=$(exesha lamp-tasks) lamp-voice=$(exesha lamp-voice)"; }
# --- Harness (lamp-tasks' own; harness.rs Cli) and its install record (lamp-update harness_install)
HHOME=/var/lib/lampos-tasks
HBIN=$HHOME/.local/bin/harness
hver(){ runuser -u lampos -- env HOME=$HHOME $HBIN --version 2>/dev/null | head -1; }
hrec(){ python3 -I -c 'import json,sys
try: r=json.load(open(sys.argv[1]))
except Exception as e: print("HREC unreadable:", type(e).__name__); sys.exit()
print("HREC", " ".join("%s=%s" % (k, r.get(k)) for k in ("install","version","from_version","pinned","error")))' $REC/harness-install.json; }
# As lamp-tasks runs Harness (harness.rs Cli: its HOME and PATH, cwd HOME, the user bus when /run/user/<uid> has it).
hcli(){ ( cd $HHOME || cd /; u=$(id -u lampos); x=""
  [ -d /run/user/$u ] && x="XDG_RUNTIME_DIR=/run/user/$u"; [ -S /run/user/$u/bus ] && x="$x DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$u/bus"
  runuser -u lampos -- env -i HOME=$HHOME PATH=/usr/local/bin:/usr/bin:/bin $x /usr/local/bin/harness "$@" ); }
# Harness's sign-in, READ ONLY, as lamp-tasks reads it (harness.rs Cli::signed_in_here: `harness auth status --json`,
# parse_auth_here: loggedIn false = out, loggedIn + offline = offline, else in). If that read fails: lamp-tasks' own
# journal of the last 3 min ("tasks: Harness signed in: no", runtime.rs:2682, or state=signed-out). Prints one HAUTH line.
hauth(){ j=$(hcli auth status --json 2>/dev/null | python3 -I -c 'import json,sys
try: d=json.load(sys.stdin); print("HAUTH via=cli logged_in=%s offline=%s" % (bool(d["loggedIn"]), bool(d.get("offline", False))))
except Exception: pass')
  [ -n "$j" ] && { echo "$j"; return; }
  n=$(journalctl -u lamp-tasks --since -3min --no-pager -o cat 2>/dev/null | grep -cE 'Harness signed in: no|state=signed-out')
  y=$(journalctl -u lamp-tasks --since -3min --no-pager -o cat 2>/dev/null | grep -c 'Harness signed in: yes')
  echo "HAUTH via=journal signed_out_lines=$n signed_in_lines=$y"; }
# --- #8138/#8162: the lamp's own agent's readiness, read only: the bus's kept value of tasks/agent (lamp_msgs
# AgentReadiness::TOPIC; <root>/<topic>/latest, a header line then the JSON payload).
AGENT_LATEST=/run/lampos-bus/tasks/agent/latest
agent_now(){ python3 -I - "$AGENT_LATEST" <<'P'
import json, sys
try:
    h, _, p = open(sys.argv[1], "rb").read().partition(b"\n"); d = json.loads(p)
    print("AGENT state=%s why=%s engine=%s since_ms=%s seq=%s" % (d.get("state"), d.get("why", "-"), d.get("engine"), d.get("since_unix_ms"), json.loads(h).get("seq")))
except Exception as e:
    print("AGENT unreadable:%s" % type(e).__name__)
P
}
# The lamp's default engine: the app's choice (engine-default.json), else lamp-tasks' LAMP_LAMP_ENGINE.
EF=/var/lib/lampos-tasks/store/engine-default.json
engine(){ e=$(grep -oE '"engine" *: *"[a-z]+"' $EF 2>/dev/null | grep -oE '"[a-z]+"$' | tr -d '"'); [ -n "$e" ] || e=$(envval "$(VP tasks)" LAMP_LAMP_ENGINE); echo "$e"; }
# --- OpenClaw: as lamp-tasks runs its CLI (user lampos, cwd /: the openclaw-ns wrapper's nsenter needs it)
OCJ=/var/lib/lampos-openclaw/state/openclaw.json
OCW=/usr/local/bin/openclaw                          # openclaw_install.rs WRAPPER
ocli(){ ( cd / && runuser -u lampos -- env -i HOME=/var/lib/lampos-openclaw OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state PATH=/var/lib/lampos-openclaw/node/bin:/usr/bin:/bin SHELL=/bin/sh $OCW "$@" ); }
gw_health(){ ocli gateway call health --json --timeout 3000 >/dev/null 2>&1; }   # lamp-update's HEALTH_CALL
TS=/var/lib/lampos-tasks/store; OS=$TS/one-session.json  # one_session.rs STATE_FILE
KDIR=/var/lib/lampos-tasks/harnesses/lamp-tasks/one-session
# The kept session's record (one_session.rs keep_as): agent, machine, session.
kept(){ python3 -I -c 'import json,sys
try: d=json.load(open(sys.argv[1]))
except Exception: print("none none none"); sys.exit()
a=d.get("agent") or "none"; print(a, d.get("machine") or "none", d.get("session") or ("agent:main:harness-"+a))' $OS 2>/dev/null; }
ppid_of(){ awk '{print $4}' /proc/$1/stat 2>/dev/null; }
under(){ _u=$1; while [ -n "$_u" ] && [ "$_u" -gt 1 ] 2>/dev/null; do [ "$_u" = "$2" ] && return 0; _u=$(ppid_of $_u); done; return 1; }
args_of(){ tr '\0' ' ' < /proc/$1/cmdline 2>/dev/null | cut -c1-160; }
comm_of(){ cat /proc/$1/comm 2>/dev/null; }
cwd_of(){ readlink /proc/$1/cwd 2>/dev/null; }
# The ONE openclaw process of the kept pane (the #7906 kit's panepid, cwd and tmux ways): from the pane's own pid tree,
# never by a name pattern over all processes, never under the gateway. Prints PANEPID <pid> or PANEPID none (...).
panepid(){ gw=$(VP openclaw-gateway); u=$(id -u lampos); c=""; root=""; way=""; a8=$(echo $1 | cut -c1-8)
  topmost(){ for p in $1; do q=$(ppid_of $p); echo " $1 " | grep -q " $q " || echo $p; done; }
  LP=$(for d in /proc/[0-9]*; do [ "$(stat -c %u $d 2>/dev/null)" = "$u" ] && echo ${d#/proc/}; done)
  for p in $LP; do [ "$(cwd_of $p)" = "$KDIR" ] && c="$c $p"; done
  [ -n "$c" ] && { root=$(topmost "$c"); way=cwd; }
  if [ -z "$root" ]; then
    for t in $LP; do case "$(comm_of $t)" in tmux*) ;; *) continue;; esac
      for s in $(ss -xlpn 2>/dev/null | grep "pid=$t," | awk '{print $5}' | sort -u); do [ -S "$s" ] || continue
        pp=$(runuser -u lampos -- tmux -S "$s" list-panes -a -F '#{pane_pid} #{pane_current_path} #{session_name} #{window_name}' 2>/dev/null \
             | awk -v k="$KDIR" -v a="$a8" '$2 == k || index($0, a) { print $1 }' | head -1)
        [ -n "$pp" ] && { root=$pp; way=tmux; break 2; }; done; done; fi
  echo "PANEWAY ${way:-none}"
  pick=""; for r in $root; do for d in /proc/[0-9]*; do x=${d#/proc/}; under $x $r || continue; under $x $gw && continue
      case "$(comm_of $x)" in node|openclaw*) ;; *) continue;; esac
      args_of $x | grep -q openclaw && pick="$pick $x"; done; done
  pick=$(topmost "$pick")
  set -- $pick; [ $# = 1 ] && echo "PANEPID $1" || echo "PANEPID none ($# openclaw processes in the pane tree; way ${way:-none})"; }
# --- the privacy mute (the 8180 kit's micon/micoff): while lamp-voice runs, record.json is its guard copy; the live
# mute is the bus's voice/mute, the slide voice/privacy.
MR=/var/lib/lampOS/voice/mute/record.json
mute_live(){ python3 -I - <<'P'
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
P
}
# The kit's stopped pids (leg L4) are listed here, by number; nostop.lamp.sh resumes them.
STOPPED=$R/stopped.pids
stopped_lampos(){ ps -eo pid=,stat=,user= | awk '$2 ~ /^T/ && ($3=="lampos" || $3 ~ /openclaw/) {print $1}' | tr '\n' ' '; }
