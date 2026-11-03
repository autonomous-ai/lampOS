# Shared by the afternoon kit's scripts (sourced, not run). lamp-52e6 only; /run drop-ins only. From the morning kit's lib.sh.
# Binaries and models go to /opt/lampOS/releases/<dir> (/run is noexec) and every `back` removes that dir.
# $L never reads stdin (-n), so an ssh inside a `while read` loop can't eat the loop's input (arms-ac.sh's DAY
# heredoc lost 11 of 12 lines that way). $LI keeps stdin for the two callers that feed the lamp (dropin, records).
# One shared connection (ControlMaster): a fresh ssh to lamp-52e6.local takes ~5.6 s to connect (measured 24 Oct
# 01:46), which put #2970's stop at first audio + 11 s instead of + 1.5 s. Reused, a call takes ~0.1 s.
CM="-o ControlMaster=auto -o ControlPath=$HOME/.ssh/cm-seventh-%C -o ControlPersist=15m"
L="ssh -n -o BatchMode=yes -o ConnectTimeout=15 $CM orangepi@lamp-52e6.local"
LI="ssh -o BatchMode=yes -o ConnectTimeout=15 $CM orangepi@lamp-52e6.local"
REL=/opt/lampOS/releases

# stage <dir> <file>...: copy files from the Mac into $REL/<dir> (0644 for .onnx/.json, else 0755).
stage() {
  local d=$1 n names=; shift
  scp -q -o BatchMode=yes "$@" orangepi@lamp-52e6.local:/tmp/ || { echo "ABORT scp"; return 1; }
  for n in "$@"; do names="$names ${n##*/}"; done
  $L "sudo -n mkdir -p $REL/$d && cd /tmp && for n in $names; do case \$n in *.onnx|*.json) m=0644;; *) m=0755;; esac; sudo -n install -m \$m \$n $REL/$d/\$n && rm -f \$n; done" || { echo "ABORT install"; return 1; }
}

# args <unit>: the unit's current ExecStart arguments, without its program.
args() { local e; e=$($L "grep '^ExecStart=/' \$(systemctl show $1 -p FragmentPath --value)" | tail -1); e=${e#ExecStart=}; printf '%s' "${e#* }"; }

# rs <unit>...: the remote commands that restart those units. lamp-voice is stopped and started instead, and
# while it is down a fault's restart guard is set aside: a record that reads muted with its .restart-guard
# marker beside it is the guard's mute after an unclean stop (controls.md, "A restored mute told from a chosen
# one"), not a person's, so both files move to *.aside-seventh-HHMMSS, as the other builders do. A chosen mute
# (no marker) is left shut. 23 Oct 12:20: #2970 ran on a guard-muted voice and heard 0 words.
rs() { local u o=; for u in "$@"; do
    if [ "$u" = lamp-voice ]; then o="$o sudo -n systemctl stop lamp-voice; m=/var/lib/lampOS/voice/mute/record.json; if sudo -n test -f \$m.restart-guard && sudo -n grep -q '\"muted\":true' \$m; then t=\$(date +%H%M%S); sudo -n mv \$m \$m.aside-seventh-\$t; sudo -n mv \$m.restart-guard \$m.restart-guard.aside-seventh-\$t; echo \"restart-guard mute set aside (\$t)\" >&2; fi; sudo -n systemctl start lamp-voice;"
    else o="$o sudo -n systemctl restart $u;"; fi; done; printf '%s' "$o"; }

# dropin <unit> <conf> <program|-> <env-line>...: a /run drop-in; with a program, ExecStart is replaced
# by it with the unit's current arguments (or $ARGS when set). Restarts the unit.
dropin() {
  local u=$1 c=$2 p=$3 a; shift 3
  a=${ARGS:-$(args "$u")}
  { echo '[Service]'; for e in "$@"; do echo "Environment=$e"; done
    [ "$p" != - ] && printf '%s\n' 'ExecStart=' "ExecStart=$p $a"; } \
    | $LI "sudo -n mkdir -p /run/systemd/system/$u.service.d && sudo -n tee /run/systemd/system/$u.service.d/$c >/dev/null && sudo -n systemctl daemon-reload && { $(rs $u) }"
}

# undrop <conf> <unit>...: remove that drop-in from each unit, reload, restart them.
undrop() { local c=$1 u r=; shift; for u in "$@"; do r="$r /run/systemd/system/$u.service.d/$c"; done
  $L "sudo -n rm -f $r; sudo -n systemctl daemon-reload; $(rs "$@")"; }

# where: each lamp unit we touch (lamp-memory too), active or not, and the program it runs.
where() { $L "for u in lamp-voice lamp-eyes lamp-controls lamp-memory; do printf '%s %s %s\n' \$u \$(systemctl is-active \$u) \$(systemctl show \$u -p ExecStart --value | grep -oE 'path=[^ ;]+' | head -1 | sed -E 's#.*/(releases/)?##; s#^(.{0,30}).*#\\1#'); done; ls /run/systemd/system/lamp-voice.service.d /run/systemd/system/lamp-eyes.service.d /run/systemd/system/lamp-controls.service.d /run/systemd/system/lamp-memory.service.d 2>/dev/null | grep -c seventh | sed 's/^/seventh drop-ins left: /'"; }

# session: the running voice's session metadata directory.
session() { echo "/var/lib/lampOS/voice/sessions/$($L 'systemctl show lamp-voice -p InvocationID --value')/metadata"; }

# records <metadata-dir> <kind-regex> [seconds]: new flat session records of those kinds, one per line, as
# they come, for at most that long (600 s), so the lamp-side loop never outlives the run.
# Runs on the lamp; prints only records past the newest one there at start. Kinds and numbers only
# (onboarding/privacy records carry no words or audio).
records() {
  $LI "sudo -n sh -s -- '$1' '$2' '${3:-600}'" <<'EOF'
cd "$1" 2>/dev/null || { i=0; while [ $i -lt 30 ] && ! cd "$1" 2>/dev/null; do sleep 1; i=$((i+1)); done; }
seq() { grep -o '"service_sequence":[0-9]*' | cut -d: -f2 | sort -n | tail -1; }
# Every file: the rotations are made ahead and stay empty, so the newest names are not the live one.
pick() { cat events-*.jsonl 2>/dev/null | grep -E "\"kind\":\"($2)\""; }
last=$(pick "$@" | seq); last=${last:-0}
i=0; while [ $i -lt "$3" ] && sleep 1; do i=$((i+1))
  new=$(pick "$@" | awk -v l="$last" 'match($0,/"service_sequence":[0-9]+/){ if (substr($0,RSTART+19,RLENGTH-19)+0 > l) print }')
  [ -n "$new" ] && { printf '%s\n' "$new"; last=$(printf '%s\n' "$new" | seq); }
done
EOF
}

# cue <words>: a Vietnamese cue for the person, from the Mac (only where the lamp's hearing doesn't count).
cue() { afplay /System/Library/Sounds/Submarine.aiff; say -v Linh -r 150 "Seventh đây. $*"; }
# ding [n]: a wordless go-sign while the lamp is listening, so no cue is heard as a line.
ding() { local i; for i in $(seq 1 "${1:-1}"); do afplay /System/Library/Sounds/Glass.aiff; done; }

# ---- added for the afternoon kit ----
# The lamp's clock now (take a session's start BEFORE its drop-ins: d-2341.sh lost its first lines).
now() { $L 'date +%s'; }
# Decision lines only: verdicts with their reasons, word counts, voice prints, the near bar, the same-voice child.
DEC='request [0-9]+: its answer is [a-z ]+: [^;]{0,140}|request [0-9]+ heard [0-9]+ words|request [0-9]+: its voice print[^;]{0,80}|not_for_me[^;:]{0,60}|the near rule.s bar is [^;]{0,100}|same.voice child[^;]{0,80}'
decisions() { $L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat" | LC_ALL=C grep -oE "$DEC"; }
V='its answer is (played|held back)'
# nv <since>: how many verdicts the voice has written since then.
nv() { $L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -cE '$V'"; }
# jwait <unit> <since> <ERE> [secs=30] [poll=1] [n=1]: waits until the unit's journal since then has n lines matching,
# in ONE ssh and one sudo with the loop on the lamp (polling journalctl over ssh every 0.5-2 s filled 52e6's 5 MB journal
# with sudo session lines and rotated out the evidence, 23 Oct). Prints the count; returns 1 if not reached in time.
jwait() { $LI "sudo -n sh -s -- '$1' '$2' '$3' '${4:-30}' '${5:-1}' '${6:-1}'" <<'EOF'
e=$(( $(date +%s) + $4 ))
while c=$(journalctl -u "$1" --since "$2" --no-pager -o cat 2>/dev/null | grep -cE "$3"); [ "$c" -lt "$6" ]; do
  [ "$(date +%s)" -ge "$e" ] && { echo "$c"; exit 1; }; sleep "$5"; done; echo "$c"
EOF
}
# settle <since>: wait until no new verdict comes for one 4 s poll (at most 40 s); prints the count. Loop on the lamp.
settle() { $LI "sudo -n sh -s -- '@$1' '$V'" <<'EOF'
n() { journalctl -u lamp-voice --since "$1" --no-pager -o cat | grep -cE "$2"; }
a=$(n "$@"); b=-1; i=0; while [ "$a" != "$b" ] && [ $i -lt 10 ]; do b=$a; sleep 4; a=$(n "$@"); i=$((i+1)); done; echo "$a"
EOF
}
# after <since> <count>: wait (the lamp's own pace) for a verdict past <count>, then for a played answer
# to be said (ANS s, default 9). Returns 1 with a note when none comes in WAITV s (default 30).
after() {
  local last
  jwait lamp-voice "@$1" "$V" "${WAITV:-30}" 2 $(( $2 + 1 )) >/dev/null || { echo "  (no verdict in ${WAITV:-30} s)"; return 1; }
  last=$($L "sudo -n journalctl -u lamp-voice --since @$1 --no-pager -o cat | grep -oE '$V' | tail -1")
  case $last in *played*) sleep "${ANS:-9}" ;; *) sleep 2 ;; esac
}
# ---- person lines: pre-rendered ElevenLabs clips (never macOS `say`; only cue() uses `say`, for the operator) ----
# A person line is keyed by <speaker> <text>; the speaker is the old Mac voice name, kept as a role key:
# Daniel = the person at the chair (male), Samantha = colleague / stand-in (female A), Karen = colleague (female B).
# The clip is clips/<slug>.wav, slug from clipslug; clips/lines.txt lists every line and render-clips.sh renders it.
CLIPS=${CLIPS:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/clips}
# clipslug <speaker> <text>: a short file name: the text's first words plus a hash of speaker and text.
clipslug() { local w; w=$(printf '%s' "$2" | LC_ALL=C tr 'A-Z' 'a-z' | LC_ALL=C sed -E 's/\[\[[^]]*\]\]//g; s/[^a-z0-9]+/-/g; s/^-+//; s/-+$//' | cut -c1-32)
  printf '%s-%s-%s' "$(printf '%s' "$1" | LC_ALL=C tr 'A-Z' 'a-z' | cut -c1-3)" "${w%-}" "$(printf '%s|%s' "$1" "$2" | md5 -q | cut -c1-6)"; }
# speak <speaker> <text>: plays that line's clip (blocking, like `say`). A missing clip aborts the row: a row
# with a silent line is not a run.
speak() { local f; f=$CLIPS/$(clipslug "$1" "$2").wav
  [ -s "$f" ] || { echo "ABORT no clip for $1: \"$2\" ($f): add it to clips/lines.txt and run ./render-clips.sh" >&2; exit 1; }
  afplay "$f"; }
# line <since> <speaker> <text>: the person line is played, then its verdict is waited for.
line() { local c; c=$(nv "$1"); speak "$2" "$3"; after "$1" "$c"; }
# turn <since> <english>: the person's line, played at the person's chair (Daniel; the person never
# speaks), then the verdict is waited for. Called only once the lamp has settled.
turn() { local c; c=$(nv "$1"); speak Daniel "$2"; after "$1" "$c"; }
# talk <n>: n exchanges of office talk between two other people (Samantha, Karen), in turns, never over a line.
# Defined here so it shadows the BSD `talk` command.
talk() { local k; for k in $(seq 1 "$1"); do
  speak Samantha "So did you see the numbers from the last sprint? I thought the review went fine but the deadline moved again."
  speak Karen "Yeah, we should book the meeting room for Thursday and ask finance about the budget for next quarter."; done; }

# otherplaying: true while another lamp plays, from Queue #3336 (comments of the last 6 h): its latest
# "PLAYING lamp-XXXX" (a comment that starts so, XXXX not 52e6) has no later "QUIET lamp-XXXX" and its window has
# not ended. The times may sit anywhere after the lamp ("PLAYING lamp-9adf · Fourteen · 16:1x until about 16:50 · …",
# 2 Nov: matching only right after the lamp fell to the default and freed 9adf at 16:39). The window's end:
# "HH:MM–HH:MM" -> the second time; "HH:MM–about +N min" -> start + N; "until (about) HH:MM" -> that time; else 60 min
# after the comment. "16:1x" (minute digit unknown) reads as 16:19 for a start and is ignored as an end.
# PYPLAY_NOW (epoch s) fixes the clock for kit/lib-pyplay-test.sh. Also true when #3336 can't be read (wait rather than play over a lamp).
# Reads every page (one JSON object per line): one page of 100 missed the latest PLAYING, 23 Oct 12:04.
# Prints the reason when it changes. Called in the runners' wait loops, before the lock.
read -r -d '' PYPLAY <<'PYEOF'
import json, os, re, sys, time, datetime as dt
ign = set(filter(None, os.environ.get("IGNORE_LAMPS", "").split(",")))  # lamps ruled not audible at 52e6
cs = [json.loads(l) for l in sys.stdin if l.strip()]; now = float(os.environ.get("PYPLAY_NOW") or time.time()); last = {}; quiet = {}
for c in cs:
    t = dt.datetime.strptime(c["created_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc).timestamp(); b = c.get("body") or ""
    q = re.match(r"\s*\**QUIET lamp-([0-9a-f]{4})\b", b)  # only a comment that starts so; a mention ("I'll post QUIET lamp-x") is not one
    if q: quiet[q.group(1)] = max(quiet.get(q.group(1), 0), t)
    m = re.match(r"\s*\**PLAYING lamp-([0-9a-f]{4})\b\**\s*(.*)", b, re.S)
    if m and m.group(1) != "52e6" and m.group(1) not in ign: last[m.group(1)] = (t, m.group(2))
T = r"(\d{1,2}):([0-5x][0-9x])"  # x = a minute digit not known
def at(loc, h, mm, start):  # epoch of h:mm on the comment's local day; None for an end with an unknown digit
    if "x" in mm:
        if not start: return None
        mm = mm.replace("x", "9") if mm[0] != "x" else "59"
    return loc.replace(hour=int(h), minute=int(mm), second=0).timestamp()
for x, (t, rest) in sorted(last.items()):
    if quiet.get(x, 0) >= t: continue
    loc = dt.datetime.fromtimestamp(t); st = e = None
    s = re.search(T, rest)
    if s:
        st = at(loc, s.group(1), s.group(2), True)
        if st < t - 12 * 3600: st += 86400
    r = re.search(T + r"\s*[–-]\s*(?:" + T + r"|about \+(\d+) ?min)", rest)
    u = re.search(r"\buntil (?:about |~)?" + T, rest)
    if r and r.group(3): e = at(loc, r.group(3), r.group(4), False)
    elif r and r.group(5): e = st + 60 * int(r.group(5))
    elif u: e = at(loc, u.group(1), u.group(2), False)
    if e is not None:
        while e < (st or t) - 60: e += 86400  # an end before its start (or before the comment) is the next day's
    end = e if e is not None else t + 3600
    if now < end: print("lamp-%s PLAYING until %s, no QUIET after it" % (x, time.strftime("%H:%M", time.localtime(end)))); sys.exit(0)
sys.exit(1)
PYEOF
OPLAST=
otherplaying() { local j r=0 msg
  # A failed read (gh non-zero) refuses; an empty read (gh ok, no comment in 6 h) is nobody PLAYING.
  if ! j=$(gh api --paginate "repos/autonomous-ai/lampOS/issues/3336/comments?per_page=100&since=$(date -u -v-6H +%Y-%m-%dT%H:%M:%SZ)" -q '.[] | {created_at, body}' 2>/dev/null); then
    msg="Queue #3336 not readable"; r=0
  elif [ -z "$j" ]; then r=1
  else msg=$(printf '%s' "$j" | python3 -I -c "$PYPLAY") || r=$?; fi
  [ $r = 0 ] || msg="no other lamp PLAYING"
  [ "$msg" != "$OPLAST" ] && echo "$(date +%H:%M) #3336: $msg" >&2; OPLAST=$msg
  return $r; }
# face: what the eyes saw in their last look (the journal's "a face in N"), for the record only.
face() { local n; n=$($L "sudo -n journalctl -u lamp-eyes --since -15s --no-pager -o cat | grep -oE 'a face in [0-9]+' | tail -1 | grep -oE '[0-9]+$'"); [ "${n:-0}" -gt 0 ] && echo "a face (in $n)" || echo "no face"; }
# attn: the voice unit's LAMP_ATTENTION, LAMP_SPOKEN_TO and LAMP_BUS values only (nothing else of its environment).
attn() { $L "systemctl show lamp-voice -p Environment --value | tr ' ' '\n' | grep -E '^LAMP_(ATTENTION|SPOKEN_TO|BUS)=' | tr '\n' ' '"; echo; }
# screenface on|off|check: the face in view is the Mac's screen facing the lamp, showing $FACE fullscreen
# (never the person). check reads the eyes' journal ("a face in N", last 15 s) and prints the face
# count; it fails when no face is seen. Callers: `screenface on && screenface check || ABORT`.
FACE=${FACE:-/tmp/fifth-face/face-crop.jpg}
screenface() {
  case $1 in
  on) [ -f "$FACE" ] || { echo "ABORT no face picture at $FACE"; return 1; }
      # A locked Mac shows no picture, and then the eyes may see (and enrol) a real person instead (23 Oct 13:0x).
      ioreg -n Root -d1 -a | grep -A1 IOConsoleLocked | grep -q "<true/>" && { echo "ABORT the Mac's screen is locked: no screen face"; return 1; }
      # Preview may still be quitting from the last `off`, or slow to show its window ("Can't get window 1.
      # Invalid index", 21 Oct): wait for the window up to 15 s, opening the picture again at 5 s and 10 s.
      # Built-in display only: bounds {0, 33, 1728, 1117}.
      local t=0 r
      open -a Preview "$FACE" || return 1
      while :; do sleep 1; t=$((t+1))
        r=$(osascript -e 'tell application "Preview"' -e 'if (count of windows) = 0 then return "none"' -e 'activate' -e 'set bounds of window 1 to {0, 33, 1728, 1117}' -e 'return "ok"' -e 'end tell' 2>/dev/null)
        [ "$r" = ok ] && break
        [ $t -ge 15 ] && { echo "ABORT Preview showed no window for $FACE in 15 s"; return 1; }
        [ $((t % 5)) = 0 ] && open -a Preview "$FACE"; done
      sleep 5 ;;
  off) osascript -e 'quit app "Preview"' 2>/dev/null
      local t=0; while pgrep -xq Preview && [ $t -lt 10 ]; do sleep 1; t=$((t+1)); done; true ;;
  check) local n k   # up to 20 s for the eyes to see the screen face, a try every 2 s (loop on the lamp)
      set -- $($LI "sudo -n sh -s -- '${FACEWAIT:-10}'" <<'EOF'
k=0; while n=$(journalctl -u lamp-eyes --since -15s --no-pager -o cat | grep -oE 'a face in [0-9]+' | tail -1 | grep -oE '[0-9]+$')
  [ "${n:-0}" -le 0 ] && [ $k -lt "$1" ]; do k=$((k+1)); sleep 2; done; echo "${n:-0} $k"
EOF
); n=${1:-0}; k=${2:-0}
      echo "faces seen in the eyes' last look count: ${n:-0} (after $((k*2)) s)"; [ "${n:-0}" -gt 0 ] || { echo "ABORT no face seen in $((${FACEWAIT:-10}*2)) s: check the screen faces the camera"; return 1; } ;;
  esac
}
# vol: the Mac at 50 and the lamp's Speaker (device_cmedia) at 50%, its current value kept in .vol-52e6 the
# first time (52e6's default is 52%); vol back restores it.
VOLF=.vol-52e6
vol() {
  if [ "${1:-}" = back ]; then [ -f $VOLF ] || return 0; local v; v=$(cat $VOLF); v=${v:-52%}
    $L "amixer -q -c device_cmedia sset Speaker $v" && echo "lamp Speaker back to $v" && rm -f $VOLF; return; fi
  osascript -e 'set volume output volume '${MACVOL:-${VOL:-50}}
  [ -f $VOLF ] || $L "amixer -c device_cmedia sget Speaker" | grep -oE '[0-9]+%' | head -1 > $VOLF
  $L "amixer -q -c device_cmedia sset Speaker $(cat .speaker-pin 2>/dev/null || echo ${VOL:-50}%)" && echo "Mac volume ${MACVOL:-${VOL:-50}}; lamp Speaker ${VOL:-50}% (asked at the lamp, 23 Oct 10:4x) (was $(cat $VOLF))"
}

# ---- added for own-line-2920.sh and hello-2923.sh ----
# prog <unit>: the unit's current ExecStart program (the first word of its last ExecStart=).
prog() { local e; e=$($L "grep '^ExecStart=/' \$(systemctl show $1 -p FragmentPath --value)" | tail -1); e=${e#ExecStart=}; printf '%s' "${e%% *}"; }
# revbins <art>: each lamp-* binary in the art whose sha256 the art's REV names, one name a line; an ABORT line
# (and exit 1) when a binary's sha256 is not in REV or there is no REV. The binary's name is its unit's.
revbins() { local b s; [ -f "$1/REV" ] || { echo "ABORT no $1/REV"; return 1; }
  for b in "$1"/lamp-*; do [ -x "$b" ] || continue; s=$(shasum -a 256 "$b" | cut -c1-64)
    grep -q "$s" "$1/REV" || { echo "ABORT ${b##*/}'s sha256 $s is not in $1/REV"; return 1; }; echo "${b##*/}"; done; }
# runsha <unit>: sha256 of the unit's running program, from /proc/<MainPID>/exe.
runsha() { $L "sudo -n sha256sum /proc/\$(systemctl show $1 -p MainPID --value)/exe" | cut -c1-64; }
# runenv <unit> <regex>: the running process's environment lines whose names match, sorted, on one line.
runenv() { $L "sudo -n cat /proc/\$(systemctl show $1 -p MainPID --value)/environ | tr '\\0' '\\n' | grep -E '^($2)='" | sort | tr '\n' ' '; }

# ---- the lamp-side dead-man for lang-mid.sh (walk3769.sh arms it when DEADMAN=1, lang-mid.sh disarms it).
# deadman_sh: the root script the dead-man runs (/run/seventh-mid/back.sh): walk3769's put-back without the Mac:
# progress, near-call and consent from onb-walk3's backup, the seventh /run drop-ins and row dir off, memory from the
# tar (mem_back's steps; the tar removed only once the restore worked), language en by the setup helper, a loans.txt
# line. Each step tests what is there first, so it is safe twice and after a finished Mac put-back.
DM=/run/seventh-mid; DMU=seventh-mid-deadman
deadman_sh() { : "${M:?M unset}" "${MB:?MB unset}"; cat <<EOF
#!/bin/sh
bad=
systemctl stop lamp-voice
# a staged lamp-voice (walk3769.sh / lang-done.sh VOICE_BIN): the bind mount off, so the installed voice starts below
VP=\$(readlink -f /opt/lampOS/voice/current)/lamp-voice
if mountpoint -q "\$VP"; then umount "\$VP" || bad="\$bad voice-unmount"; fi
mountpoint -q "\$VP" || rm -rf /opt/lampOS/voice/seventh-3769
P=\$(systemctl show lamp-voice -p Environment --value | tr ' ' '\n' | sed -n 's/^LAMP_ONBOARDING_FILE=//p'); P=\${P:-/var/lib/lampOS/voice/onboarding/progress.json}
BK=/var/lib/lampOS/seventh-walk3-backup; NC=/var/lib/lampOS/voice/near-call.json; CONS=/var/lib/lampOS/consent
if [ -d \$BK ]; then
  if [ -f \$BK/progress.json ]; then cp -p \$BK/progress.json "\$P"; else rm -f "\$P"; fi
  if [ -f \$BK/near-call.json ]; then cp -p \$BK/near-call.json \$NC; else rm -f \$NC; fi
  for f in room-sounds.json face-expressions.json; do
    if [ -f \$BK/\$f ]; then cp -p \$BK/\$f \$CONS/\$f; else rm -f \$CONS/\$f; fi; done
  rm -f "\$P.told"; rm -rf \$BK
fi
rm -f /run/systemd/system/*.service.d/*seventh*.conf; systemctl daemon-reload; rm -rf /run/seventh-walk3
# lang-done.sh's outage rule (FAILNOTICE), idempotent: removed whatever the Mac did; its own 90 s timer is the first net.
for t in iptables ip6tables; do \$t -S OUTPUT 2>/dev/null | grep -- "--comment seventh-langdone" | sed "s/^-A /-D /" | while read -r r; do \$t \$r; done; done   # by its comment, whatever cgroup path
systemctl stop seventh-langdone-fw.timer 2>/dev/null; rm -rf /run/seventh-langdone
spk=${SPK_WAS:-}; [ -z "\$spk" ] || amixer -q -c device_cmedia sset Speaker "\$spk" || bad="\$bad speaker"   # lang-done.sh: the Speaker as found
if [ -f $MB ]; then
  if tar -tzf $MB >/dev/null; then
    systemctl stop lamp-memory && rm -rf $M && tar -C /var/lib -xzpf $MB && rm -f $MB || bad="\$bad memory-restore"
  else bad="\$bad memory-tar-unreadable"; fi
fi
systemctl stop seventh-mem-start.timer 2>/dev/null
systemctl start lamp-memory || bad="\$bad lamp-memory"
systemctl restart lamp-eyes || bad="\$bad lamp-eyes"
systemctl start lamp-voice || bad="\$bad lamp-voice"
echo '{"language":"en"}' | /opt/lampOS/setup/current/lamp-setup change --config /etc/lampOS/setup/serve.json --bus-root /run/lampos-bus --enable || bad="\$bad language"
echo "\$(date +%H:%M) seventh: $DMU fired \$(date +%T): put back memory/progress/en\${bad:+; FAILED:\$bad (left for a look: $DM)}" >> ~orangepi/loans.txt
[ -z "\$bad" ] && rm -rf $DM
EOF
}
# deadman_arm: writes back.sh (0700, root) and arms the timer to fire at DEADMAN_AT (Mac epoch: LIMIT + 2 min).
deadman_arm() { local s=$(( ${DEADMAN_AT:?DEADMAN_AT unset} - $(date +%s) ))
  [ "$s" -gt 0 ] || { echo "dead-man: DEADMAN_AT is past"; return 1; }
  deadman_sh | $LI "sudo -n test ! -e $DM || { echo '$DM already there (an earlier run?)'; exit 1; }
    sudo -n install -d -m 0700 $DM && sudo -n tee $DM/back.sh >/dev/null && sudo -n chmod 0700 $DM/back.sh && sudo -n sh -n $DM/back.sh \
    && sudo -n systemd-run -q --unit=$DMU --on-active=${s}s /bin/sh $DM/back.sh && echo \"dead-man armed: $DMU in ${s}s, timer \$(systemctl is-active $DMU.timer)\""; }
# deadman_disarm: only after the Mac's put-back finished OK; the timer stopped, its units reset, $DM removed.
deadman_disarm() { $L "sudo -n systemctl stop $DMU.timer 2>/dev/null; sudo -n systemctl reset-failed $DMU.timer $DMU.service 2>/dev/null; sudo -n rm -rf $DM; echo \"dead-man disarmed: timer \$(systemctl is-active $DMU.timer), $DM \$(sudo -n test -e $DM && echo LEFT || echo gone)\""; }
