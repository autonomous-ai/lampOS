#!/bin/sh
# App tools row v1 (lampOS #8386): Gym Coach (beta) 2.0.0 (gym-coach-beta) on a lamp-voice train build carrying
# #8491 (the tools file read, apps/tools) and #8496 (app.call / app.answer / app.call.stopped, apps/called).
# LAMP_APP_TOOLS on through /run drop-ins, and the journal and trace reads for each leg of the row.
#
#   sudo sh row-app-tools-v1.sh on        write the drop-ins, reload, restart lamp-voice and lamp-control
#   sudo sh row-app-tools-v1.sh show      each unit's state, drop-ins and switch values in its process,
#                                         the beta's install, unit state and tools lines
#   sudo sh row-app-tools-v1.sh off       remove every drop-in of mine, reload, restart: back to stock
#   sudo sh row-app-tools-v1.sh mark LEG  one journal line "row-app-tools-v1 LEG begins" (a, b, ... h)
#   sudo sh row-app-tools-v1.sh read LEG  the lines that leg is judged on, since its mark
#   sudo sh row-app-tools-v1.sh times [N] the last N turns' end of speech to first audio (default 10),
#                                         and each Gym Coach (beta) say's wait from the turn before it
#   sudo sh row-app-tools-v1.sh freeze | thaw   leg h: SIGSTOP / SIGCONT the beta's process
#
# Mine only: /run/systemd/system/<unit>.d/96-app-only-app-tools.conf (gone at reboot too), for lamp-voice,
# lamp-control and the apps helper's template lamp-apps-manage@ (its package reader refuses a manifest naming
# `tools` with the switch off, so the beta can't be installed before `on`). It never writes or reads a key,
# a credential, second.json or anything under /etc; it installs nothing (README: install).
# Reads only: the journal, and the voice's own session trace (times, no words: day-stages.sh's fields).
set -eu
NAME=96-app-only-app-tools.conf
APP=gym-coach-beta
FN=gym_coach_beta
APP_UNIT=lamp-app@$APP.service
UNITS="lamp-voice.service lamp-control.service"
HELPER=lamp-apps-manage@.service
TAG=row-app-tools-v1
SESSIONS=/var/lib/lampOS/voice/sessions

envof() {
  p=$(systemctl show -p MainPID --value "$1" 2>/dev/null || echo 0)
  if [ "${p:-0}" != 0 ]; then
    tr '\0' '\n' < "/proc/$p/environ" 2>/dev/null | grep -E '^LAMP_(APP_TOOLS|STORE_APPS|APP_TRIGGERS|CARE_LINE|VOICE_ROUTE)=' | tr '\n' ' '
  else
    echo "(not running)"
  fi
}

show() {
  for u in $UNITS; do
    echo "-- $u: $(systemctl is-active "$u" 2>/dev/null || true)"
    echo "   drop-ins: $(systemctl show -p DropInPaths --value "$u" 2>/dev/null)"
    echo "   process: $(envof "$u")"
  done
  if [ -f "/run/systemd/system/$HELPER.d/$NAME" ]; then
    echo "-- $HELPER: LAMP_APP_TOOLS=on for each install, update and start (drop-in present)"
  else
    echo "-- $HELPER: no drop-in of mine (a manifest naming tools is refused at install)"
  fi
  echo "   lamp-control: $(journalctl -u lamp-control.service -b --no-pager -o cat 2>/dev/null | grep -E '^lamp-control: app tools are on \(LAMP_APP_TOOLS\)' | tail -1)"
  echo "-- $APP_UNIT: $(systemctl is-active "$APP_UNIT" 2>/dev/null || true)"
  if [ -f "/opt/lampOS/apps/$APP/current/lamp-skill.json" ]; then
    python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print("   installed:", d["name"], d["version"], "min_build", d["min_build"], "tools", d.get("tools"))' "/opt/lampOS/apps/$APP/current/lamp-skill.json"
  else
    echo "   not installed (README: install)"
  fi
  echo "   tools read: $(journalctl -u lamp-control.service -b --no-pager -o cat 2>/dev/null | grep -E "^lamp-control: app $APP: ([0-9]+ tools read from its tools file|no tools: |its tools are left out)" | tail -1)"
  echo "   the app: $(journalctl -u "$APP_UNIT" -b --no-pager -o cat 2>/dev/null | grep -E 'app tools: (on|off)' | tail -1)"
}

since() {  # @<unix seconds> of LEG's mark, or -b for the boot
  t=$(journalctl -t "$TAG" -b --no-pager -o short-unix 2>/dev/null | grep "$TAG $1 begins" | tail -1 | cut -d' ' -f1 | cut -d. -f1)
  [ -n "$t" ] && echo "@$t" || echo "-b"
}

j() {  # j UNIT PATTERN: the unit's lines since the leg's mark matching PATTERN
  s=$(since "$LEG")
  if [ "$s" = "-b" ]; then journalctl -u "$1" -b --no-pager -o short-precise; else journalctl -u "$1" --since "$s" --no-pager -o short-precise; fi 2>/dev/null | grep -E "$2" || echo "   (none)"
}

# lamp-voice (crates/voice/src/app_tools.rs, second_provider.rs on lamp-voice), exact texts:
#   lamp-voice second: request N: app gym-coach-beta tool <tool>: <ok|error|not done> in <ms> ms (LAMP_APP_TOOLS)
#   lamp-voice second: request N: app tool gym_coach_beta_<tool> refused: not the owner's turn, private, the care
#     hold, an answer awaited, or narrowed to another app (LAMP_APP_TOOLS)
#   lamp-voice second: request N: app tool gym_coach_beta_<tool> refused: its arguments don't fit its schema (LAMP_APP_TOOLS)
#   lamp-voice second: request N: the app gym-coach-beta's tools not offered: they don't fit beside the apps running longer (LAMP_APP_TOOLS)
#   lamp-voice second: app tool gym_coach_beta_<tool> not offered: the name is taken | past the N functions (LAMP_APP_TOOLS)
#   lamp-voice second: request N is a care turn: the care line follows its answer, or the next one if this is cut (LAMP_CARE_LINE)
V="lamp-voice second: (request [0-9]+: (app $APP tool [a-z0-9_]+: |app tool ${FN}_[a-z0-9_]+ refused: |the app $APP's tools not offered: )|app tool ${FN}_[a-z0-9_]+ not offered: )"
CARE='lamp-voice second: request [0-9]+ is a care turn: '
# lamp-control (#8491 app_tools.rs, #8496 app_calls.rs), exact texts:
#   lamp-control: app tools are on (LAMP_APP_TOOLS)
#   lamp-control: app gym-coach-beta: N tools read from its tools file | no tools: its tools file <why> |
#     its tools are left out: N apps' tools are told at most
#   lamp-control: app gym-coach-beta: call <id>: <sent|not running|unknown tool|busy>
#   lamp-control: app gym-coach-beta: an answer not told, unfit | a call's end not told to the voice: <error>
#   lamp-control: no app tools are told | the apps' tools were not told | apps/tools can't be read |
#     no app's tools are read | apps/call: a message not read | apps/call can't be read | no app call is sent |
#     no app call's end is told | no app call's wait is kept
# A timeout (4.5 s) has no lamp-control line: the app is sent app.call.stopped and the voice told "timeout".
C="lamp-control: (app tools are on|app $APP: |no app tools are told|the apps' tools were not told|apps/tools can't be read|no app's tools are read|apps/call|no app call)"
CALL="lamp-control: app $APP: call app-[0-9]+-[0-9]+-[0-9]+: "
# The app (gym-coach-beta, gym-coach/src/gym_coach/app_tools.py on gym-coach-2): app tools: on | off (...),
#   app tools: <tool> ok | error, app tools: <tool> again (<id>); answered as before,
#   app tools: call <id> stopped (timeout) before it ran; dropped | while it ran; ... | after its answer; ...
A='app tools: '

read_leg() {
  LEG=$1
  case "$LEG" in
  a)
    echo "== (a) plan by voice, EN and VI: lamp-voice: one call line each, outcome ok"
    j lamp-voice.service "$V"
    echo "== lamp-control: each call sent to the app (call <id>: sent)"
    j lamp-control.service "$C"
    echo "== the app: set_plan ok (its args are in the app's data only, never the journal)"
    j "$APP_UNIT" "$A|plan"
    echo "PASS when: one 'app $APP tool set_plan: ok' per language, each with a 'call ...: sent' and the app's"
    echo "'app tools: set_plan ok'. The plan's values: read them on the phone's Gym Coach page."
    ;;
  b)
    echo "== (b) start, first set counted"
    j lamp-voice.service "$V"
    j lamp-control.service "$CALL"
    j "$APP_UNIT" 'app tools: start_session|voice start|phrases: session|camera: on|pose: ready|rep|set [0-9]'
    echo "PASS when: 'tool start_session: ok' (or the start phrase taken), phrases: session, then a rep/set count line"
    ;;
  c)
    echo "== (c) 'stop workout' ends it, never through the model"
    j "$APP_UNIT" 'heard .*stop workout|heard .*dừng buổi tập|the person ended|stopping|phrases: idle|end_session'
    echo "== lamp-voice: must show NO 'app $APP tool end_session' line for this leg"
    j lamp-voice.service "$V"
    j lamp-control.service "$CALL"
    echo "PASS when: the app's 'heard [stop workout]' then the session ended, and lamp-voice shows no"
    echo "'app $APP tool end_session:' line (and lamp-control no call) in this leg"
    ;;
  d)
    echo "== (d) the care hold refuses tools"
    j lamp-voice.service "$CARE"
    echo "== lamp-voice app lines in the hold: none (not offered) or 'refused: not the owner's turn, private, the care hold ...'"
    j lamp-voice.service "$V"
    j lamp-control.service "$CALL"
    echo "PASS when: a 'request N is a care turn' line, then within the 30 min hold no 'app $APP tool ...: ok'"
    echo "line, no 'call ...: sent' and no app say; Lamp answers the plan ask plainly."
    ;;
  e)
    echo "== (e) a guest's turn: no tool runs; a call the model makes anyway is 'not now'"
    j lamp-voice.service "$V"
    j lamp-control.service "$CALL"
    j "$APP_UNIT" "$A"
    echo "PASS when: no 'app $APP tool ...: ok' line and no 'call ...: sent' for the guest's request; Lamp says"
    echo "not now or answers plainly. The tools aren't offered on a guest's turn, so a 'refused: not the"
    echo "owner's turn ...' line shows only if the model calls one anyway."
    echo "NOTE: the phrases 'start workout' / 'stop workout' are not owner-gated (phrase path): a guest saying"
    echo "them exactly is taken by the app. Use plain words for this leg (\"set me a plan, three days\")."
    ;;
  f|g)
    turn_times "${2:-10}"
    ;;
  h)
    echo "== (h) the app gone mid-call: one spoken line, never silence"
    j lamp-voice.service "$V"
    j lamp-control.service "$C"
    j "$APP_UNIT" "$A|Stopped|Started"
    echo "PASS when, frozen: 'tool get_plan: error in ~4500 ms' (lamp-control ends it as timeout at 4.5 s;"
    echo "  the voice's own wait is 5 s), 'call ...: sent', Lamp says '... did not answer in time ...'; after"
    echo "  thaw the app logs 'app tools: call <id> stopped (timeout) ...' and sends no answer."
    echo "PASS when, stopped: 'call ...: not running' and 'tool get_plan: error' (Lamp: '... is not running ...'),"
    echo "  or no call at all (its tools leave apps/tools once it stops) and a plain answer."
    echo "And: times shows an answer_first_audio for that turn (never silence)."
    ;;
  *) echo "legs: a b c d e f g h" >&2; exit 2 ;;
  esac
}

turn_times() {
  n=${1:-10}
  d=$(ls -td "$SESSIONS"/*/ 2>/dev/null | head -1)
  [ -n "$d" ] || { echo "no session folder under $SESSIONS"; return; }
  echo "== newest voice session: $d (a voice restart starts a new one: on/off each restart it)"
  python3 -I - "$d" "$n" "$APP" <<'PY'
import glob, json, os, statistics, sys
folder, n, app = sys.argv[1], int(sys.argv[2]), sys.argv[3]
files = sorted(glob.glob(os.path.join(folder, "**", "events-*.jsonl"), recursive=True))
first, decided, says = {}, {}, []
for f in files:
    for line in open(f, errors="replace"):
        try:
            r = json.loads(line)
        except ValueError:
            continue
        k = r.get("kind")
        if k == "answer_first_audio" and r.get("after_decision_ms") is not None:
            first[r["turn"]] = r["after_decision_ms"]
        elif k == "answer_stages" and r.get("decided_at_us"):
            decided[r["turn"]] = r["decided_at_us"]
        elif k == "skill_says" and app in str(r.get("skill", "")):
            says.append(r["at_us"])
turns = sorted(first)[-n:]
print("(f) end of speech decided -> answer's first audio, ms, last %d turns:" % len(turns))
for t in turns:
    print("   turn %s: %s" % (t, first[t]))
if turns:
    v = [first[t] for t in turns]
    print("   median %d, min %d, max %d" % (statistics.median(v), min(v), max(v)))
print("(g) the turn's end of speech decided -> the next %s say begun (skill_says), ms:" % app)
for at in says[-n:]:
    before = [(d, t) for t, d in decided.items() if d <= at]
    if before:
        d, t = max(before)
        print("   after turn %s: %d" % (t, (at - d) // 1000))
if not says:
    print("   (no say of %s in this session's trace)" % app)
PY
}

case "${1:-}" in
on)
  for u in $UNITS $HELPER; do
    systemctl cat "$u" >/dev/null 2>&1 || { echo "REFUSED: no $u on this lamp" >&2; exit 3; }
  done
  case "$(envof lamp-control.service)" in *LAMP_STORE_APPS=on*) ;; *) echo "REFUSED: LAMP_STORE_APPS is not on in lamp-control (this kit never turns it on)" >&2; exit 3;; esac
  case "$(envof lamp-voice.service)" in *LAMP_VOICE_ROUTE=second*) ;; *) echo "REFUSED: lamp-voice is not on the second route (app tools are second route only)" >&2; exit 3;; esac
  for u in $UNITS $HELPER; do
    d=/run/systemd/system/$u.d; mkdir -p "$d"
    printf '# App Only app tools row v1 (#8386): removed by off; gone at reboot.\n[Service]\nEnvironment=LAMP_APP_TOOLS=on\n' > "$d/$NAME"
  done
  systemctl daemon-reload
  systemctl restart $UNITS
  logger -t "$TAG" "$TAG switch on"
  show
  ;;
off)
  for u in $UNITS $HELPER; do rm -f "/run/systemd/system/$u.d/$NAME"; done
  systemctl daemon-reload
  systemctl restart $UNITS
  logger -t "$TAG" "$TAG switch off"
  show
  ;;
show) show ;;
mark) [ -n "${2:-}" ] || { echo "usage: $0 mark LEG" >&2; exit 2; }; logger -t "$TAG" "$TAG $2 begins"; echo "marked $2" ;;
read) [ -n "${2:-}" ] || { echo "usage: $0 read LEG" >&2; exit 2; }; read_leg "$2" "${3:-}" ;;
times) turn_times "${2:-10}" ;;
freeze) systemctl kill --signal=SIGSTOP "$APP_UNIT"; logger -t "$TAG" "$TAG $APP frozen (SIGSTOP)"; echo "frozen: ask a tool now; then thaw" ;;
thaw) systemctl kill --signal=SIGCONT "$APP_UNIT"; logger -t "$TAG" "$TAG $APP thawed (SIGCONT)"; echo "thawed" ;;
*) echo "usage: $0 on|show|off|mark LEG|read LEG|times [N]|freeze|thaw" >&2; exit 2 ;;
esac
