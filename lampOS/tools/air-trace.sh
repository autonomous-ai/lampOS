#!/usr/bin/env bash
# Record one real air event on a lamp as a skill test case (#1684): a kettle, toast, a candle blown out, a match.
# Run on the Mac, beside the lamp. On the lamp it runs only `lamp-bus record` of the skill's topics, as LAMP_USER (no
# sudo), into a temp folder it removes when it ends: no unit is stopped or started, no setting or file outside that
# folder is changed, and lamp-update is never touched. The sensor is the SEN63C (`lamp-air`, `air/state` every 5 s).
#
#   LAMP_HOST=<lamp> lampOS/tools/air-trace.sh [--dry-run] [--skill smoky-air|dusty-air] [--force] EVENT [NOTE...]
#
# EVENT is the case's folder name (kettle-steam-50cm, toast, candle-blown-out, match): the case is written to
# lampOS/skills/<skill>/tests/EVENT/. NOTE (optional) goes into its expect.toml, for what was done ("kettle 50 cm from
# the base, window shut, fan off"). How it goes:
#   1. It starts the recording and waits for the first air/state reading (15 s); none means no sensor answers on this
#      lamp, and it stops there (exit 4).
#   2. 2 minutes of quiet first (AIR_PRE_S), counted down. Then it asks for Enter when the event starts (the kettle
#      switched on, the candle blown out, the match struck), and Enter again when it ends (the kettle off; for a
#      blink of an event, at once).
#   3. 5 minutes after the end (AIR_POST_S), counted down; then the recording stops and is copied back.
#   4. The case folder: each topic's `.jsonl` exactly as `lamp-bus record` wrote it (only a line cut short by the
#      stop is left out), and an expect.toml with `clock` (the local time of its first message), the two marks in
#      seconds from the first message, the PM2.5 before and after, and the verdict left for the person who saw the
#      event to fill in. Until they do, `lamp-skills test` fails the case, so it can't pass unread.
# A folder that is there already is kept (exit 3), unless it is one of the placeholders (an expect.toml marked
# PLACEHOLDER and no .jsonl), whose event lines are carried into the new expect.toml, or --force is given.
# Ctrl-C, or any failure before the recording is stopped, stops it and removes the lamp's temp folder; nothing is
# written then. Once it is stopped, the recording is copied to the Mac ($PLAYED_OUT/air-trace/EVENT-<time>.raw/), and
# that copy and the lamp's are removed only after the case folder is written and read back the same. A failure from
# there on (a file not read back, a case not written) keeps both, says where they are, and exits nonzero (6 or 7); the
# lamp removes its copy AIR_LINGER_S after the recording ended. A Mac that drops off mid-run leaves the recording to
# end by itself (AIR_CAP_S, 1 hour) and its folder removed 15 minutes later.
# --dry-run says what it would run and write, and runs and writes nothing.
# Plays no sound, so the office's quiet hours do not apply; the person's events do make smoke and steam.
# Settings, from the environment:
#   LAMP_HOST       the lamp (required, as for the played kit: lampOS/tools/played/README.md)
#   LAMP_SSH        runs one command on the lamp, ssh-style (default: played/lamp_ssh: BatchMode, by key only, with
#                   LAMP_USER and LAMP_SSH_KEY as in played/README.md). A lamp that still takes only a password gets
#                   your key once, by the person at the Mac: `ssh-copy-id orangepi@<lamp>.local` (lampOS/docs/joining.md,
#                   "SSH by key only"); no password is ever typed into this tool.
#   LAMP_BUS        lamp-bus on the lamp (/usr/local/bin/lamp-bus)
#   AIR_PRE_S, AIR_POST_S, AIR_CAP_S, AIR_FIRST_S   120, 300, 3600, 15
#   AIR_REMOTE_TMP  where the lamp's temp folder goes (/tmp); AIR_LINGER_S, how long after its end it removes it if the Mac never does (900)
#   AIR_SKILLS_DIR  where the skills are (this checkout's lampOS/skills)
#   PLAYED_OUT      the log goes to $PLAYED_OUT/air-trace/EVENT-<time>.log ($HOME/lamp-played)
# Its stand-in test is air-trace-test.sh. Bash 3.2 (macOS's) or later; python3.
set -u
HERE=$(cd "$(dirname "$0")" && pwd) || exit 9
DRY=no
FORCE=no
SKILL=smoky-air
while [ $# -gt 0 ]; do
  case $1 in
    --dry-run) DRY=yes; shift ;;
    --force) FORCE=yes; shift ;;
    --skill) SKILL=${2:-}; shift 2 || shift ;;
    --) shift; break ;;
    -*) echo "air-trace.sh: unknown option $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
usage="usage: air-trace.sh [--dry-run] [--skill smoky-air|dusty-air] [--force] EVENT [NOTE...]"
EVENT=${1:-}
[ -n "$EVENT" ] || { echo "$usage" >&2; exit 2; }
shift
NOTE="$*"
case $EVENT in *[!a-z0-9-]*|-*) echo "air-trace.sh: EVENT is a folder name: a-z, 0-9 and -" >&2; exit 2 ;; esac
# The topics each skill's cases hold, as its existing cases do.
case $SKILL in
  smoky-air) TOPICS="air/state voice/state" ;;
  dusty-air) TOPICS="air/state voice/state humans/faces/tracked" ;;
  *) echo "air-trace.sh: --skill is smoky-air or dusty-air" >&2; exit 2 ;;
esac
if [ -z "${LAMP_HOST:-}" ]; then
  echo "air-trace.sh: LAMP_HOST is not set. Set it to your own lamp's address (see lampOS/tools/played/README.md). Nothing was recorded." >&2
  exit 2
fi
LAMP_SSH=${LAMP_SSH:-$HERE/played/lamp_ssh}
BUS=${LAMP_BUS:-/usr/local/bin/lamp-bus}
PRE=${AIR_PRE_S:-120}
POST=${AIR_POST_S:-300}
CAP=${AIR_CAP_S:-3600}
FIRST=${AIR_FIRST_S:-15}
RTMP=${AIR_REMOTE_TMP:-/tmp}
SKILLS=${AIR_SKILLS_DIR:-$(cd "$HERE/../skills" && pwd)}
CASE=$SKILLS/$SKILL/tests/$EVENT
[ -d "$SKILLS/$SKILL/tests" ] || { echo "air-trace.sh: no $SKILLS/$SKILL/tests" >&2; exit 2; }

# What is in the case folder now: nothing, a placeholder, or a case.
PLACEHOLDER=
if [ -e "$CASE" ]; then
  if ! ls "$CASE"/*.jsonl >/dev/null 2>&1 && grep -q PLACEHOLDER "$CASE/expect.toml" 2>/dev/null; then
    PLACEHOLDER=$CASE/expect.toml
  elif [ "$FORCE" != yes ]; then
    echo "air-trace.sh: $CASE is there already, with a recording or a case of its own; kept. --force replaces it." >&2
    exit 3
  fi
fi

if [ "$DRY" = yes ]; then
  echo "air-trace.sh --dry-run: nothing is run on $LAMP_HOST and nothing is written."
  echo "on the lamp, as its own user, through $LAMP_SSH:"
  echo "  $BUS record $RTMP/air-trace.XXXXXX $TOPICS --seconds $CAP   (stopped by the Mac after the event)"
  echo "  then each file is read back and the folder removed; nothing else on the lamp is touched"
  echo "on the Mac: ${PRE} s of quiet, Enter at the event's start and end, ${POST} s after"
  if [ -n "$PLACEHOLDER" ]; then echo "would replace the placeholder $CASE"
  elif [ -e "$CASE" ]; then echo "would replace $CASE (--force)"
  else echo "would write $CASE"; fi
  for t in $TOPICS; do echo "  $(echo "$t" | tr / _).jsonl"; done
  echo "  expect.toml (the verdict left for the person to fill in)"
  exit 0
fi

PLAYED_OUT=${PLAYED_OUT:-$HOME/lamp-played}
mkdir -p "$PLAYED_OUT/air-trace" || exit 9
STAMP=$(date +%Y%m%d-%H%M%S)
LOG=$PLAYED_OUT/air-trace/$EVENT-$STAMP.log
say() { echo "$*"; echo "$(date +%H:%M:%S) $*" >>"$LOG"; }
# The recording as copied back, kept until the case is written and read back the same.
RAW=$PLAYED_OUT/air-trace/$EVENT-$STAMP.raw
mkdir "$RAW" || exit 9
LINGER=${AIR_LINGER_S:-900}

# The lamp's side, sent on stdin each time (POSIX sh). `start` prints the temp folder; the others take it back and
# refuse any other path.
REMOTE='set -u
cmd=$1; bus=$2; base=$3; shift 3
mine() { case $1 in "$base"/air-trace.??????) [ -d "$1" ] ;; *) false ;; esac; }
case $cmd in
  check) [ -x "$bus" ] || { echo "no lamp-bus at $bus" >&2; exit 4; }; date +%s ;;
  start)
    cap=$1; linger=$2; shift 2
    d=$(mktemp -d "$base/air-trace.XXXXXX") || exit 6
    ( "$bus" record "$d" "$@" --seconds "$cap" >"$d/record.log" 2>&1 &
      echo $! >"$d/pid.part" && mv "$d/pid.part" "$d/pid"
      wait; sleep "$linger"; rm -rf "$d" ) </dev/null >/dev/null 2>&1 &
    i=0; while [ ! -s "$d/pid" ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
    [ -s "$d/pid" ] || exit 6
    echo "$d" ;;
  count) mine "$1" || exit 2; n=$(grep -c "\"header\"" "$1/air_state.jsonl" 2>/dev/null); echo "${n:-0}" ;;
  stop)
    mine "$1" || exit 2; p=$(cat "$1/pid")
    kill -TERM "$p" 2>/dev/null
    i=0; while kill -0 "$p" 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
    cat "$1/record.log" >&2 ;;
  fetch) mine "$1" || exit 2; case $2 in */*|.*) exit 2 ;; esac; cat "$1/$2" ;;
  clean) mine "$1" || exit 2; rm -rf "$1" ;;
esac'
lamp() { "$LAMP_SSH" sh -s -- "$1" "$BUS" "$RTMP" "${@:2}" <<EOF
$REMOTE
EOF
}

D=
KEEP=no   # yes from the recording's stop until its case is written and read back
cleanup() {
  if [ "$KEEP" = yes ]; then
    say "air-trace.sh: the case is not written; the raw recording is kept: on this Mac in $RAW, and on the lamp in $LAMP_HOST:$D until ${LINGER} s after the recording ended"
    return
  fi
  if [ -n "$D" ]; then
    lamp stop "$D" >/dev/null 2>&1
    lamp clean "$D" >/dev/null 2>&1 && say "the lamp's temp folder $D is removed"
    D=
  fi
  rm -rf "$RAW"
}
trap 'cleanup' EXIT
trap 'say "stopped by Ctrl-C"; exit 5' INT TERM

say "air-trace: $EVENT for $SKILL on $LAMP_HOST ($TOPICS); log $LOG"
t0=$(date +%s)
remote_now=$(lamp check) || { say "air-trace.sh: the lamp does not answer, or has no lamp-bus at $BUS"; exit 4; }
t1=$(date +%s)
OFFSET=$((remote_now - (t0 + t1) / 2))   # the lamp's clock less the Mac's, in seconds
[ "$OFFSET" -ge -2 ] && [ "$OFFSET" -le 2 ] || say "note: the lamp's clock is ${OFFSET} s from the Mac's; the marks allow for it"
# shellcheck disable=SC2086 # the topics are words
D=$(lamp start "$CAP" "$LINGER" $TOPICS) && [ -n "$D" ] || { D=; say "air-trace.sh: the recording did not start"; exit 6; }
STARTED=$(date +%s)
say "recording into $LAMP_HOST:$D"
n=0
while [ $(( $(date +%s) - STARTED )) -lt "$FIRST" ]; do
  n=$(lamp count "$D") || n=0
  [ "${n:-0}" -gt 0 ] && break
  sleep 1
done
if [ "${n:-0}" -le 0 ]; then
  say "air-trace.sh: no air/state reading in ${FIRST} s: is lamp-air running, and does its sensor answer? Nothing written."
  exit 4
fi
say "the sensor answers. Keep the room still: window shut, fan off."
while :; do
  left=$((PRE - ($(date +%s) - STARTED)))
  [ "$left" -le 0 ] && break
  printf '\r  quiet first: %3d s left ' "$left"
  if [ "$left" -gt 10 ]; then sleep 10; else sleep "$left"; fi
done
printf '\n'
# An Enter pressed during the quiet is not taken as the start.
if [ -t 0 ]; then while read -r -t 1 _; do :; done; fi
printf 'Press Enter as the event STARTS (kettle on, candle blown out, match struck): '
read -r _ || { say "no Enter: stopped, nothing written"; exit 5; }
MARK_START=$(date +%s)
say "event start marked"
printf 'Press Enter as the event ENDS (kettle off; at once for a blink of an event): '
read -r _ || { say "no Enter: stopped, nothing written"; exit 5; }
MARK_END=$(date +%s)
say "event end marked, $((MARK_END - MARK_START)) s after the start. Now ${POST} s after: keep the room still."
while :; do
  left=$((POST - ($(date +%s) - MARK_END)))
  [ "$left" -le 0 ] && break
  printf '\r  after: %3d s left ' "$left"
  if [ "$left" -gt 10 ]; then sleep 10; else sleep "$left"; fi
done
printf '\n'
lamp stop "$D" 2>>"$LOG"
KEEP=yes
for t in $TOPICS; do
  f=$(echo "$t" | tr / _).jsonl
  lamp fetch "$D" "$f" >"$RAW/$f" || { say "air-trace.sh: could not read $f back"; exit 6; }
done

python3 -I - "$RAW" "$CASE" "$SKILL" "$LAMP_HOST" "$(( (MARK_START + OFFSET) * 1000 ))" "$(( (MARK_END + OFFSET) * 1000 ))" \
  "$PRE" "$POST" "$NOTE" "$PLACEHOLDER" $TOPICS <<'PY' 2>&1 | tee -a "$LOG"
import base64, json, os, shutil, sys, time
work, case, skill, host, start_ms, end_ms, pre, post, note, placeholder = sys.argv[1:11]
topics = sys.argv[11:]
start_ms, end_ms = int(start_ms), int(end_ms)
kept = {}
messages = []
for topic in topics:
    name = topic.replace("/", "_") + ".jsonl"
    lines = []
    for text in open(os.path.join(work, name), encoding="utf-8").read().splitlines():
        try:
            line = json.loads(text)
        except ValueError:
            continue  # a line cut short by the stop
        lines.append(text)
        if "header" in line:
            messages.append((line["header"], line.get("payload_b64", "")))
    kept[name] = lines
if not any(h["topic"] == "air/state" for h, _ in messages):
    sys.exit("air-trace.sh: the recording holds no air/state reading; nothing written")
first = min(messages, key=lambda m: m[0]["at_us"])[0]
base = first["at_unix_ms"]
last = max(h["at_unix_ms"] for h, _ in messages)
rel = lambda ms: round((ms - base) / 1000)
pm = []
for h, payload in messages:
    if h["topic"] == "air/state":
        value = json.loads(base64.b64decode(payload) or b"{}").get("pm2_5")
        if value is not None:
            pm.append((h["at_unix_ms"], value))
def fmt(values, pick):
    return "%g" % pick(values) if values else "none"
before = [v for ms, v in pm if ms < start_ms]
after = [v for ms, v in pm if ms >= start_ms]
clock = time.strftime("%H:%M", time.localtime(base / 1000))
carried = []
if placeholder:
    for text in open(placeholder, encoding="utf-8"):
        if text.startswith("# The event:") or text.startswith("# Should it speak?"):
            carried.append(text.rstrip("\n"))
say = {
    "smoky-air": [
        ("body.gesture", '{ gesture = "curious" }'),
        ("light", '{ colour = "warm", level = 2 }'),
        ("say", '{ text = "The air reads like smoke. If something\'s burning, please check it.", priority = 8, expires_ms = 120000 }'),
    ],
    "dusty-air": [
        ("say", '{ text = "The air here has been very dusty for about an hour. I can\'t tell where it\'s coming from.", priority = 5, expires_ms = 30000 }'),
    ],
}[skill]
out = [
    "# Recorded on a lamp with lampOS/tools/air-trace.sh, %s (#1684)%s." % (time.strftime("%Y-%m-%d", time.localtime(base / 1000)), (": " + note) if note else ""),
    "# air/state every 5 s from the SEN63C, as `lamp-bus record` wrote it, not edited.",
    "# Marks, in seconds from the first message: the event started at %d s and ended at %d s;" % (rel(start_ms), rel(end_ms)),
    "# the recording ends at %d s (%s s of quiet asked before, %s s after)." % (rel(last), pre, post),
    "# PM2.5 (ug/m3): %s lowest and %s highest before the start; %s highest from the start on; %s at the end." % (
        fmt(before, min), fmt(before, max), fmt(after, max), "%g" % pm[-1][1] if pm else "none"),
] + carried + [
    'clock = "%s"' % clock,
    "",
    "# THE VERDICT, for the person who saw the event: uncomment ONE of the two below.",
    "# Until then `lamp-skills test` fails this case (\"none = true or lists [[expect]], one of them\").",
    "# It should say nothing:",
    "# none = true",
    "# It should speak once: set each `at` (seconds from the first message) and `within`:",
]
for call, args in say:
    out += ["# [[expect]]", '# at = "%ds"' % rel(start_ms), '# within = "120s"', '# call = "%s"' % call, "# args = %s" % args, "#"]
# Written beside the case as CASE.part, read back, then swapped in; an old folder is kept as CASE.old until then.
want = {name: "".join(text + "\n" for text in lines) for name, lines in kept.items()}
want["expect.toml"] = "\n".join(out).rstrip("#\n") + "\n"
def same(folder):
    if sorted(os.listdir(folder)) != sorted(want):
        return False
    for name, text in want.items():
        with open(os.path.join(folder, name), encoding="utf-8") as f:
            if f.read() != text:
                return False
    return True
part, old = case + ".part", case + ".old"
swapped = False
try:
    shutil.rmtree(part, ignore_errors=True)
    os.makedirs(part)
    for name, text in want.items():
        with open(os.path.join(part, name), "w", encoding="utf-8") as f:
            f.write(text)
    if not same(part):
        raise OSError("%s does not read back as written" % part)
    if os.path.exists(case):
        shutil.rmtree(old, ignore_errors=True)
        os.rename(case, old)
        swapped = True
    os.rename(part, case)
    if not same(case):
        raise OSError("%s does not read back as written" % case)
    shutil.rmtree(old, ignore_errors=True)
except Exception as e:
    shutil.rmtree(part, ignore_errors=True)
    if swapped:  # the folder that was there before goes back
        shutil.rmtree(case, ignore_errors=True)
        os.rename(old, case)
    sys.exit("air-trace.sh: could not write the case: %s" % e)
print("written: %s (%d air/state readings, %d s); fill in its verdict in expect.toml" % (
    case, sum(1 for h, _ in messages if h["topic"] == "air/state"), rel(last)))
PY
status=${PIPESTATUS[0]}
[ "$status" -eq 0 ] || exit 7   # the cleanup keeps the raw recording and says where
KEEP=no
lamp clean "$D" && say "the lamp's temp folder is removed"
D=
rm -rf "$RAW"
