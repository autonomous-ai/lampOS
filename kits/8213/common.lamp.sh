# lamp-side prelude (root sh), cat'ed before each lamp body by mac.zsh's LAMP. Defines only.
# #8156 ROW (first run on lamp-c0a4): the #7906 ROW kit's helpers (kept session, pane pid, ocli, hcli) plus the
# #7945 round kit's (build, hver, exesha, line, dropins). Never touches the speaker volume; lamp-cloud is
# never stopped; no factory reset; a pid is signalled only by its exact number.
set -u
C=/home/orangepi/changes.txt; n(){ echo "$(date '+%F %T') ROW kit #8156 row: $*" >> $C; chown orangepi: $C 2>/dev/null || true; }
R=/home/orangepi/t8156; mkdir -p $R
# --- round (from 7906round/common.lamp.sh, t7945 -> t8156)
IN=/tmp/t8156                                    # what the Mac copied (as orangepi): ch1/ ch2/ test.pub SHAS plan.env
CH=/var/lib/lampos-t8156                         # channels + test key; readable inside lamp-update's sandbox
REC=/var/lib/lampos-update; UST=$REC/state.json
UPD=/opt/lampOS/update
DROP=/run/systemd/system/lamp-update.service.d/zz-t8156.conf
DROPA=/run/systemd/system/lamp-update-apply.service.d/zz-t8156.conf
DROPS="$DROP $DROPA"
HHOME=/var/lib/lampos-tasks
HBIN=$HHOME/.local/bin/harness
say(){ echo "[t8156 $(date +%H:%M:%S)] $*"; }
build(){ python3 -I -c 'import json,sys; i=json.load(open(sys.argv[1])).get("installed") or {}; print(i.get("sequence",""), (i.get("commit") or "")[:10])' $UST 2>/dev/null; }
hver(){ runuser -u lampos -- env HOME=$HHOME $HBIN --version 2>&1 | head -1; }
hrec(){ python3 -I -c 'import json,sys
try: r=json.load(open(sys.argv[1]))
except Exception as e: print("HREC unreadable:", type(e).__name__); sys.exit()
print("HREC", " ".join("%s=%s" % (k, r.get(k)) for k in ("install","version","from_version","pinned","error")))' $REC/harness-install.json; }
exesha(){ p=$(systemctl show -p MainPID --value $1); [ "${p:-0}" != 0 ] && sha256sum /proc/$p/exe | cut -d' ' -f1 || echo none; }
line(){ systemctl show -p ExecStart --value lamp-update.service | grep -oE 'lamp-update serve [^;]*' | head -1; }
product_line(){ line | grep -qE -- '--channel-url https://(github.com/autonomous-ai/lampOS/releases/download/lampos-stable/|cdn.autonomous.ai/lampos/stable/) --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on'; }
dropins(){ for u in lamp-update lamp-update-apply lamp-update-writer lamp-update-back lamp-update-harness lamp-tasks; do d=$(systemctl show -p DropInPaths --value $u.service 2>/dev/null | tr " " "\n" | grep -v "/etc/systemd/system/$u.service.d/exit-note.conf$" | tr "\n" " " | sed "s/ *$//"); [ -n "$d" ] && echo "DROPIN $u: $d"; done; }
# --- #8156: the lamp's own agent's readiness, read only. The bus's kept value (docs/bus.md "Layout": <root>/<topic>/latest,
# a header line then the JSON payload; lamp-bus is not installed on c0a4) and lamp-tasks' {"op":"agent"} (protocol.rs Agent).
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
# OpenClaw's gateway health, as lamp-update's HEALTH_CALL (openclaw_install.rs): as lampos through the wrapper, 3 s.
gw_health(){ ocli gateway call health --json --timeout 3000 >/dev/null 2>&1; }
OCJ=/var/lib/lampos-openclaw/state/openclaw.json
OCW=/usr/local/bin/openclaw                         # openclaw_install.rs WRAPPER
# --- #7906 ROW kit's helpers (7906row/common.lamp.sh, unchanged below this line but for R and n above)
BINS="tasks"                                        # bound over /opt/lampOS/tasks/current/lamp-tasks
cur(){ echo "$(readlink -f /opt/lampOS/$1/current)/lamp-$1"; }
SS=/usr/local/lib/lampos/skill-start                # a copy of lamp-tasks
OC=/var/lib/lampos-openclaw/state/openclaw.json     # lamp-tasks merges V1's tools into it at start
OCDB=/var/lib/lampos-openclaw/state/agents/main/agent/openclaw-agent.sqlite   # harness.rs openclaw_record_path
MS=/var/lib/lampOS/voice/made-skills; SK=/var/lib/lampOS/skills; SKD=/var/lib/lampOS/skills-dev; SKST=/var/lib/lampos-skills-state
SKBIN=/opt/lampOS/skills/current/lamp-skills
TS=/var/lib/lampos-tasks/store; OS=$TS/one-session.json   # one_session.rs STATE_FILE, 0600
# harness.log: Harness's ADAPTER_DATA_DIR (cli/data) under lamp-tasks' HOME. UNCONFIRMED on the lamp: found, not assumed.
HL=/var/lib/lampos-tasks/.harness/cli/data/harness.log
[ -f $HL ] || HL=$(find /var/lib/lampos-tasks/.harness -maxdepth 4 -name harness.log 2>/dev/null | head -1)
VP(){ systemctl show -p MainPID --value lamp-$1; }
envof(){ tr '\0' '\n' < /proc/$1/environ 2>/dev/null; }
envval(){ envof "$1" | sed -n "s/^$2=//p"; }
speaker(){ amixer -c allwinnerhdmi sget Speaker 2>/dev/null | grep -oE '[0-9]+%' | head -1 | tr -d %; }   # READ only
madenames(){ python3 -I -c 'import json,sys
try: print("\n".join(e.get("name","") for e in json.load(open(sys.argv[1]))))
except FileNotFoundError: pass
except Exception as e: print("unreadable:" + type(e).__name__)' $MS/made.json 2>/dev/null; }
skillnames(){ ls -1 $SK 2>/dev/null | LC_ALL=C sort; }
UNITS="lamp-control lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update"
shas(){ for b in $BINS; do printf 'lamp-%s file=%s running=%s  ' $b "$(sha256sum "$(cur $b)" | cut -c1-12)" "$(sha256sum /proc/$(VP $b)/exe 2>/dev/null | cut -c1-12)"; done
  printf 'skill-start=%s\n' "$(sha256sum $SS 2>/dev/null | cut -c1-12)"; }
dev(){ echo "DEVICE $(date '+%F %T') build=$(build) speaker=$(speaker)% (read only)"; shas
  echo "  units: $(systemctl is-active $UNITS | tr '\n' ' ')"
  echo "  run drop-ins: $(ls -d /run/systemd/system/*.d/zz-* 2>/dev/null | tr '\n' ' ')"
  echo "  binds: $(findmnt -rn -o TARGET,SOURCE | grep -c t8156)"; }
# The kept session's record (one_session.rs keep_as: agent, machine, engine, signed_in, kept, session).
kept(){ python3 -I -c 'import json,sys
try: d=json.load(open(sys.argv[1]))
except Exception as e: print("none none none"); sys.exit()
a=d.get("agent") or "none"; print(a, d.get("machine") or "none", d.get("session") or ("agent:main:harness-"+a))' $OS 2>/dev/null; }
# As lamp-tasks runs OpenClaw's CLI (one_session.rs delete_openclaw_session: user lampos, env_clear +
# gateway.rs APPROVALS_ENV, current_dir "/"). The `cd /` matters: /usr/local/bin/openclaw is the
# openclaw-ns wrapper, whose nsenter fails in the ssh user's cwd ("nsenter: cannot open /home/orangepi").
ocli(){ ( cd / && runuser -u lampos -- env -i HOME=/var/lib/lampos-openclaw OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state PATH=/var/lib/lampos-openclaw/node/bin:/usr/bin:/bin SHELL=/bin/sh /usr/local/bin/openclaw "$@" ); }
# As lamp-tasks runs Harness (harness.rs Cli: lamp-tasks.service's HOME and PATH, cwd HOME, plus
# with_user_bus's XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS when /run/user/<uid> has them; as the
# person, so no AI_AGENT / HARNESS_AGENT_SESSION).
hcli(){ ( cd /var/lib/lampos-tasks || cd /; u=$(id -u lampos); x=""
  [ -d /run/user/$u ] && x="XDG_RUNTIME_DIR=/run/user/$u"; [ -S /run/user/$u/bus ] && x="$x DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$u/bus"
  runuser -u lampos -- env -i HOME=/var/lib/lampos-tasks PATH=/usr/local/bin:/usr/bin:/bin $x /usr/local/bin/harness "$@" ); }
# The kept session as lamp-tasks names it to Harness (one_session.rs name_for + TOKEN_FILE under --dir).
KNAME=lamp-one-openclaw; KTOK=$TS/kept-session.token
# Its folder (one_session.rs FOLDER under skill_task.rs lamp_project: $HOME/harnesses/lamp-tasks).
KDIR=/var/lib/lampos-tasks/harnesses/lamp-tasks/one-session
# #7906 sign 3, read only: does OpenClaw's record still hold harness-<id8>? (harness.rs openclaw_record's query)
ocrecord(){ python3 -I - "$OCDB" "$1" <<'P'
import sqlite3, sys
db, a = sys.argv[1], sys.argv[2]; p = "agent:main:harness-" + a[:8]
try:
    c = sqlite3.connect("file:%s?mode=ro" % db, uri=True, timeout=1)
    q = ("SELECT EXISTS(SELECT 1 FROM session_nodes n WHERE substr(n.session_key,1,?2)=?1 AND n.status IS NOT NULL) "
         "OR EXISTS(SELECT 1 FROM transcript_events t WHERE t.message_role IS NOT NULL AND t.session_id IN "
         "(SELECT w.session_id FROM session_windows w WHERE substr(w.session_key,1,?2)=?1 "
         "UNION SELECT n.current_session_id FROM session_nodes n WHERE substr(n.session_key,1,?2)=?1))")
    print("yes" if c.execute(q, (p, len(p))).fetchone()[0] else "no")
except Exception as e:
    print("unreadable:" + type(e).__name__)
P
}
ppid_of(){ awk '{print $4}' /proc/$1/stat 2>/dev/null; }
under(){ _u=$1; while [ -n "$_u" ] && [ "$_u" -gt 1 ] 2>/dev/null; do [ "$_u" = "$2" ] && return 0; _u=$(ppid_of $_u); done; return 1; }
args_of(){ tr '\0' ' ' < /proc/$1/cmdline 2>/dev/null | cut -c1-160; }
comm_of(){ cat /proc/$1/comm 2>/dev/null; }
cwd_of(){ readlink /proc/$1/cwd 2>/dev/null; }
# The ONE openclaw process of the kept pane of Harness agent $1, found from the pane's pid tree, never
# by a name pattern over all processes. The kept session runs on the lamp itself: lamp_place() gives it
# the lamp's own machine id (the `machine` Harness lists it under, e.g. 1fec98ed...), folder $KDIR.
# The pane's root, first way that finds one:
#  env  user-lampos processes with HARNESS_AGENT_ID=<id> in their environment (UNCONFIRMED; was 0 on c0a4);
#  cwd  user-lampos processes whose cwd is $KDIR, the kept folder Harness starts the pane in (top-most);
#  tmux the lampos tmux servers (any socket: found from the server's own listening socket with ss, not
#       assumed under /tmp/tmux-<uid>), their panes whose pane_current_path is $KDIR or whose names hold <id8>.
# Then, under that root and not under the gateway, the top-most process that is node or openclaw by
# comm and names openclaw in its argv (so the openclaw-ns sh/nsenter wrappers are skipped).
# Prints PANEWAY, PANEROOT and PANEOC lines, then "PANEPID <pid>" or "PANEPID none (...)".
panepid(){ a=$1; a8=$(echo $a | cut -c1-8); gw=$(VP openclaw-gateway); u=$(id -u lampos); c=""; root=""; way=""
  lampos_pids(){ for d in /proc/[0-9]*; do [ "$(stat -c %u $d 2>/dev/null)" = "$u" ] && echo ${d#/proc/}; done; }
  topmost(){ for p in $1; do q=$(ppid_of $p); echo " $1 " | grep -q " $q " || echo $p; done; }
  L=$(lampos_pids)
  for p in $L; do envof $p | grep -qx "HARNESS_AGENT_ID=$a" && c="$c $p"; done
  [ -n "$c" ] && { root=$(topmost "$c"); way=env; }
  if [ -z "$root" ]; then c=""; for p in $L; do [ "$(cwd_of $p)" = "$KDIR" ] && c="$c $p"; done
    [ -n "$c" ] && { root=$(topmost "$c"); way=cwd; }; fi
  if [ -z "$root" ]; then
    for t in $L; do case "$(comm_of $t)" in tmux*) ;; *) continue;; esac
      for s in $(ss -xlpn 2>/dev/null | grep "pid=$t," | awk '{print $5}' | sort -u); do [ -S "$s" ] || continue
        echo "PANETMUX server $t socket $s"
        pp=$(runuser -u lampos -- tmux -S "$s" list-panes -a -F '#{pane_pid} #{pane_current_path} #{session_name} #{window_name} #{pane_title}' 2>/dev/null \
             | awk -v k="$KDIR" -v a="$a8" '$2 == k || index($0, a) { print $1 }' | head -1)
        [ -n "$pp" ] && { root=$pp; way=tmux; break 2; }; done; done; fi
  echo "PANEWAY ${way:-none}"
  pick=""; for r in $root; do echo "PANEROOT $r cwd=$(cwd_of $r) $(args_of $r)"
    for d in /proc/[0-9]*; do x=${d#/proc/}; under $x $r || continue; under $x $gw && continue
      case "$(comm_of $x)" in node|openclaw*) ;; *) continue;; esac
      args_of $x | grep -q openclaw && pick="$pick $x"; done; done
  pick=$(topmost "$pick")
  for p in $pick; do echo "PANEOC $p ppid=$(ppid_of $p) comm=$(comm_of $p) $(args_of $p)"; done
  set -- $pick; [ $# = 1 ] && echo "PANEPID $1" || echo "PANEPID none ($# openclaw processes in the pane tree; way ${way:-none})"; }
# --- #8156 (21 Nov): the privacy mute. While lamp-voice runs, record.json is its CONSERVATIVE guard copy (muted on
# disk, hand_controls.rs: arm_active), not the person's mute: the live mute is voice/mute (said every second with
# LAMP_HEAD_TOUCH=on and --controls-mute) and the slide is voice/privacy. The record is the true state only while
# lamp-voice is stopped cleanly (SIGTERM writes the live latch). It has one owner: written here only with lamp-voice stopped.
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
