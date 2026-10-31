#!/usr/bin/env bash
# row-cutin-words.sh: lampOS #3997 step 2 on the lamp L names (first run on lamp-52e6; the first builder's yes, 31 Oct
# 07:07): an A/B of the product unit as installed against the same installed lamp-voice with LAMP_STOP_WORDS=heard
# + LAMP_BACKCHANNEL_HOLDS=on, the
# wordless cut-in hold (coordinator.rs:6616-6640, duplex.rs:548-581: a due commit waits up to 500 ms for the yield's
# words and resumes the answer if they come back empty). Mac voices, no person, a quiet room, no bed.
# Based on tools/played/cutin.sh (its yield/decision attribution and per-episode scoring, here score-cutin-words.py),
# wrapped in row-3967d.sh's kit (lamp-lock, PLAYING/QUIET, hard end, dead-man, systemd-run put-back, gates).
# Arms: P = the product unit untouched. W = a /run drop-in with only the two Environment lines (no ExecStart override,
# the installed binary), proved by its startup lines plus /proc/<pid>/exe and environ.
# Rows per arm (the fewest that decide the verdict):
#   C real cut-ins: "Stop." / "Wait, hold on." CUT_AFTER s into a long answer, C_TRIALS valid (4);
#   O the own tail: a long answer, nobody talking, O_RUN s, then an end line; every duplex_commit before it is wordless;
#   time to yield: row C's #PLAY_US -> duplex_yield, compared across arms (no extra row).
# Verdict (plan-cutin-words.md): VOID / FAIL (cut-ins, wordless, yield time) / NO REPRO / PASS, printed at the put-back.
# HARD_END=HH:MM is required: the kit refuses to start with under NEED_MIN left, stops between tries when under 3 min
# remain before HARD_END - 2 min, and a lamp-side dead-man puts the lamp back at HARD_END - 2 min if the kit dies.
#   DRY=1 HARD_END=HH:MM L=... OUT=... SEQ=x NO_PERSON=yes ./row-cutin-words.sh   # walks every gate, touches no lamp,
#   plays nothing; the lamp's events come from DRY_CASE (canned-cutin/<case>/<arm><row>.jsonl); the clock runs on the
#   naps and waits it skips. canned-cutin/matrix.sh runs every DRY case.
# Runs from a checkout on any Mac (plan-cutin-words.md, "Running it"): it needs only L, SEQ, HARD_END, NO_PERSON=yes and OUT. Everything
# else is next to it: tools/played's scenario.py and lamp_ssh (KIT), tools/lamp-lock.sh, tts.sh (clips), other-lamp.sh.
#   ALLOW_DROPINS="zz-product-u.conf"  /run drop-ins on lamp-voice that may stay (else any refuses); never removed
#   WAVS_LEVELLED=yes  the clips in WAVS are already levelled (no ffmpeg needed either way: see wav())
#   L="ssh orangepi@lamp-4ace.local"  the lamp is the host's first label (LAMP= overrides it)
#   WHO=Sixth   who the posts and the lock name     WAVS=dir  the clip cache (default ~/.cache/lampos-cutin-words)
#   C_TRIALS/O_TRIALS valid tries per arm in rows C/O (4/3); C_MAX/O_MAX the most tries (C_TRIALS+2, O_TRIALS+1)
set -uo pipefail
: "${L:?set L to the lamp ssh command, e.g. ssh orangepi@lamp-4ace.local}" "${OUT:?set OUT to a new output dir}"
DRY=${DRY:-}
HERE=$(cd "$(dirname "$0")" && pwd)
host=${L##* }; LAMP=${LAMP:-$(h=${host#*@}; echo "${h%%.*}")} WHO=${WHO:-Sixth}
[[ $LAMP =~ ^lamp-[0-9a-z]+$ ]] || { echo "FAIL the lamp name '$LAMP' (from L's host) is not lamp-xxxx: set LAMP=lamp-xxxx"; exit 2; }
if [[ -z ${LOCKED:-} && -z $DRY ]]; then
  LOCKED=1 exec "$HERE/../../lamp-lock.sh" "$LAMP" run "$(echo "$WHO" | tr A-Z a-z)" "rowcutin-$$" "#3997 wordless cut-in hold A/B (Mac voices, no person)" -- "$0" "$@"
fi
KIT=${KIT:-$(cd "$HERE/.." && pwd)} TTS=${TTS:-$HERE/tts.sh} VOICE_ID=${VOICE_ID:-21m00Tcm4TlvDq8ikWAM}
[[ -f $KIT/scenario.py && -x $KIT/lamp_ssh ]] || { echo "FAIL no scenario.py and lamp_ssh in KIT=$KIT (tools/played)"; exit 2; }
WAVS=${WAVS:-$HOME/.cache/lampos-cutin-words}; mkdir -p "$WAVS" # tts.sh writes into it: a fresh Mac has none
export LAMP_SSH_TIMEOUT=${LAMP_SSH_TIMEOUT:-10} # scenario.py's connect time: 5 s once returned '' and crashed a clip
C_TRIALS=${C_TRIALS:-4} O_TRIALS=${O_TRIALS:-3}; C_MAX=${C_MAX:-$(( C_TRIALS + 2 ))} O_MAX=${O_MAX:-$(( O_TRIALS + 1 ))} ARMS=${ARMS:-P W}
CUT_AFTER=${CUT_AFTER:-4} O_RUN=${O_RUN:-90} YIELD_TOL_MS=${YIELD_TOL_MS:-100}
# The asker at the kit's usual level: file -18 at Mac 90 read mic p90 -31 on 30 Oct (out-3967d8 preflight). No bed.
SYSVOL=${SYSVOL:-90} LEVEL_DB=${LEVEL_DB:--18}
SEQ=${SEQ:?set SEQ to the installed sequence (jq .installed.sequence /var/lib/lampos-update/state.json)}
HARD_END=${HARD_END:?set HARD_END=HH:MM, the end of the slot}
# Both arms at C_MAX and O_MAX tries (76 s and 150 s each, 90 s an arm's switch), 1 min setup, 5 min margin
# (plan-cutin-words.md "Lamp minutes"): 44 at the defaults.
NEED_MIN=${NEED_MIN:-$(( (60 + 2 * (C_MAX * 76 + O_MAX * 150 + 90) + 300) / 60 ))}
DRY_CASE=${DRY_CASE:-$HERE/canned-cutin/pass} DRY_ASK_US=100000000000 # DRY: canned events per arm and row; the ask's #PLAY_US
# DRY gate knobs (canned-cutin/matrix.sh): DRY_SEQFAIL=n empty seq reads; DRY_SEQ=x the seq read; DRY_FOREIGN="names" /run
# drop-ins; DRY_TTS=yes render clips with TTS; DRY_SWITCHED=line lamp-update switched; DRY_EXE=path lamp-voice's exe; DRY_ENVIRON="a b" switch counts in
# environ (STOP_WORDS HOLDS); DRY_STARTUP=text W's startup lines; DRY_HELLO=fail; DRY_NOANSWER="PC WO" rows whose asks go
# unanswered; DRY_STRAY=pid a stray afplay; DRY_PUTBACK=text the lamp's put-back answer; DRY_REALNAP=1 real time (TERM)
BACK=/run/sixth-cutin-putback.sh DEAD=sixth-cutin-deadman
DROPIN=/run/systemd/system/lamp-voice.service.d/zzzz-sixth-cutin.conf
SW_WORDS=LAMP_STOP_WORDS=heard SW_HOLDS=LAMP_BACKCHANNEL_HOLDS=on
export LAMP_HOST=${host#*@} LAMP_USER=${host%@*} PLAYED_OUT=$OUT LAMP_PLAY_AT_LUNCH=yes
mkdir -p "$OUT"
lamp() { if [[ -n $DRY ]]; then echo "[dry] $L $*" >&2; [[ -p /dev/stdin ]] && cat >/dev/null; return 0; fi; $L "$@"; }
on() { lamp "sudo bash -c $(printf %q "$1")"; }
run() { if [[ -n $DRY ]]; then echo "[dry] $*" >&2; else "$@"; fi; }
SIM=0 # DRY: seconds the kit would have spent in naps and waits it skips
zz() { sleep "$1" & wait $!; } # a wait the trap can interrupt
nap() { if [[ -n $DRY ]]; then SIM=$(( SIM + ${1%.*} )); [[ -z ${DRY_REALNAP:-} ]] || zz "$1"; else zz "$1"; fi; }
# Every background player (the asks' scenario.py and its afplay) is started directly, kept in PLAYS, and stopped with its
# children and waited (row-3967d's 17:40 lesson). The trap stops them all. No pkill.
PLAYS=() BG=
bgplay() { if [[ -n $DRY ]]; then echo "[dry] $* &" >&2; BG=; return 0; fi; "$@" >/dev/null 2>&1 & BG=$!; PLAYS+=("$BG"); }
killtree() { local c; for c in $(pgrep -P "$1" 2>/dev/null); do killtree "$c"; done; kill "$1" 2>/dev/null; }
drop() { local p k=(); for p in ${PLAYS[@]+"${PLAYS[@]}"}; do [[ $p == "$1" ]] || k+=("$p"); done; PLAYS=(${k[@]+"${k[@]}"}); }
reap() { [[ -n ${1:-} ]] || return 0; wait "$1" 2>/dev/null; drop "$1"; return 0; }
stopplay() { [[ -n ${1:-} ]] || return 0; jobs -p | grep -qx "$1" && killtree "$1"; wait "$1" 2>/dev/null; drop "$1"; return 0; }
stopplays() { local p; for p in ${PLAYS[@]+"${PLAYS[@]}"}; do stopplay "$p"; done; PLAYS=(); }
stray() { # stray WHERE: no afplay on this Mac at all before a try (there is no bed); else a hard stop with put-back.
  # DRY plays nothing, so it reads DRY_STRAY instead of pgrep (31 Oct 07:21: another builder's afplay stopped a DRY).
  local o; if [[ -n $DRY ]]; then o=${DRY_STRAY:-}; else o=$(pgrep -x afplay | tr '\n' ' '); fi
  [[ -z $o ]] && return 0
  FAIL_WHY="a stray afplay before $1 (pid $o)"; echo "FAIL $FAIL_WHY"; log "FAIL $FAIL_WHY"; exit 1; }
other() { # other WHERE: refuse (DRY too, through the put-back and QUIET) while another lamp is PLAYING on #3336, or #3336
  # is unreadable: row-4558's gate, Seventh's otherplaying (other-lamp.sh). DRY reads gh from PATH (a stub).
  ( SELF_LAMP=$LAMP; source "$HERE/other-lamp.sh"; otherplaying ) 2>>"$OUT/log.txt" || return 0
  FAIL_WHY="another lamp is PLAYING on #3336 (or #3336 unreadable) before $1"; echo "FAIL $FAIL_WHY"; log "FAIL $FAIL_WHY"; exit 1; }
now_s() { echo $(( $(date +%s) + SIM )); }
END_S=$(date -j -f "%Y-%m-%d %H:%M" "$(date +%Y-%m-%d) $HARD_END" +%s) || { echo "FAIL HARD_END $HARD_END is not HH:MM"; exit 1; }
hard_end() { # hard_end WHERE: between tries, stop (to the put-back) when under 3 min remain before the dead-man
  (( END_S - 120 - $(now_s) >= 180 )) && return 0
  ABORTED=1; log "stopped at the hard end $HARD_END (sim +${SIM}s) before $1"; exit 0; }
log() { echo "$(date +%H:%M:%S) $*" >>"$OUT/log.txt"; [[ -z $DRY ]] || echo "$*" >&2; }
fail() { if [[ -n $DRY ]]; then log "[dry] would stop here: $*"; return 0; fi; echo "FAIL $*"; log "FAIL $*"; FAIL_WHY=$*; exit 1; }
ASKS=("Hey Lamp, tell me a long story about a lighthouse keeper."
      "Hey Lamp, tell me a long story about a fox who lived in a library."
      "Hey Lamp, tell me a long story about a sailor lost at sea."
      "Hey Lamp, tell me a long story about a baker in a small mountain town."
      "Hey Lamp, tell me a long story about a girl who trained a dragon.")
CUTS=("Stop." "Wait, hold on." "Stop, stop." "Wait, hold on.") # try i plays CUTS[(i-1)%4]: the same on both arms
ENDLINE="Stop, stop." # row O's end, after the measured window
HELLO="Hey Lamp, are you there?"
# Levelling: ffmpeg (volumedetect + alimiter) when it is on PATH; else afconvert (macOS) to 16-bit WAV and a python3
# stdlib gain to the same mean volume, hard-limited at 0.97. WAVS_LEVELLED=yes: the clips in WAVS are already at level,
# only converted to 16-bit WAV. A TTS stand-in may write AIFF (say) or WAV to the .wav path: both are converted.
LEVELLER=ffmpeg; command -v ffmpeg >/dev/null && command -v ffprobe >/dev/null || LEVELLER=python3
[[ ${WAVS_LEVELLED:-} == yes ]] && LEVELLER=none
LEVEL_PY='import array,math,sys,wave
src,dst,lev=sys.argv[1],sys.argv[2],sys.argv[3]
with wave.open(src) as w: p=w.getparams(); a=array.array("h",w.readframes(p.nframes))
if p.sampwidth!=2: sys.exit("level: %s is not 16-bit PCM" % src)
if sys.byteorder=="big": a.byteswap()
if lev!="none":
  m=20*math.log10(max(math.sqrt(sum(x*x for x in a)/max(len(a),1)),1)/32768); g=10**((float(lev)-m)/20); c=0.97*32767
  a=array.array("h",(int(max(-c,min(c,x*g))) for x in a)) # ponytail: hard limit, not alimiter; TTS peaks sit well under it
if sys.byteorder=="big": a.byteswap()
with wave.open(dst,"wb") as w: w.setparams(p); w.writeframes(a.tobytes())'
wav() { local lev=${2:-$LEVEL_DB} h raw f m t; h=$(printf '%s|%s' "$VOICE_ID" "$1" | shasum -a 256 | cut -c1-16); raw="$WAVS/$h.wav" f="$WAVS/lev$lev/$h.wav"
  [[ $LEVELLER == none ]] && f="$WAVS/levelled/$h.wav"
  [[ -s $raw ]] || [[ -n $DRY && -z ${DRY_TTS:-} ]] || "$TTS" "$raw" "$1" "$VOICE_ID" >/dev/null || fail "tts failed: $1"
  [[ -s $raw || -s $f ]] || { [[ -n $DRY ]] || fail "no clip for '$1' ($raw)"; echo "$f"; return; }
  if [[ ! -s $f ]]; then mkdir -p "${f%/*}"
    if [[ $LEVELLER == ffmpeg ]]; then m=$(ffmpeg -nostats -i "$raw" -af volumedetect -f null - 2>&1 | grep -oE 'mean_volume: [-0-9.]+' | cut -d' ' -f2)
      ffmpeg -loglevel error -y -i "$raw" -af "volume=$(echo "$lev - ($m)" | bc)dB,alimiter=limit=0.97" "$f" || fail "level: $1"
    else t="$f.tmp.wav"; afconvert -f WAVE -d LEI16 "$raw" "$t" && python3 -c "$LEVEL_PY" "$t" "$f" "$([[ $LEVELLER == none ]] && echo none || echo "$lev")"
      [[ -s $f ]] || { rm -f "$t"; fail "level ($LEVELLER): $1"; }; rm -f "$t"; fi; fi
  echo "$f"; }
dur_us() { [[ -n $DRY && ! -s $1 ]] && { echo 1000000; return; } # DRY without clips: 1 s
  if [[ $LEVELLER == ffmpeg ]]; then python3 -c 'import sys; print(int(float(sys.argv[1])*1e6))' "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$1")"
  else python3 -c 'import sys,wave; w=wave.open(sys.argv[1]); print(w.getnframes()*1000000//w.getframerate())' "$1"; fi; } # the clips are 16-bit WAV here
ask_at() { local s; s=$(ls -t "$OUT"/scenarios/*-"$1-ask".txt 2>/dev/null | head -1); s=$( [[ -n $s ]] && awk '/^#PLAY_US /{print $2; exit}' "$s"); echo "${s:-0}"; }
for t in "$HELLO" "${ASKS[@]}" "${CUTS[@]}" "$ENDLINE"; do wav "$t" >/dev/null; done
log "clips: levelled by $LEVELLER (WAVS=$WAVS)"
# /run drop-ins on lamp-voice other than the kit's: refused unless named in ALLOW_DROPINS (space-separated), and an
# allowed one must sort before the kit's (systemd applies drop-ins in name order), so the kit's Environment lines win.
found=$(on "ls /run/systemd/system/lamp-voice.service.d 2>/dev/null | grep -vx ${DROPIN##*/}"); [[ -n $DRY ]] && found=${DRY_FOREIGN:-}
foreign= allowed=
for d in $found; do
  if [[ " ${ALLOW_DROPINS:-} " == *" $d "* ]]; then allowed+="$d "
    [[ $(printf '%s\n' "$d" "${DROPIN##*/}" | LC_ALL=C sort | head -1) == "$d" ]] || fail "allowed drop-in $d sorts after the kit's ${DROPIN##*/}: its lines would win"
  else foreign+="$d "; fi; done
foreign=${foreign% } allowed=${allowed% }
# The ExecStart the unit runs, ignoring only the kit's own drop-in (an allowed drop-in's ExecStart counts).
exec_line=$(on 'systemctl cat lamp-voice | awk '"'"'/^# \//{f=$2} /^ExecStart=./ && f != "'"$DROPIN"'"{e=$0} END{print e}'"'")
[[ -n $DRY && -z $exec_line ]] && exec_line="ExecStart=/opt/lampOS/voice/current/lamp-voice serve --turn-taking duplex (dry)"
[[ $exec_line == *"--turn-taking duplex"* ]] || fail "lamp-voice is not duplex"
log "unit: $exec_line; other /run drop-ins: ${foreign:-none}; allowed /run drop-ins (kept, ALLOW_DROPINS): ${allowed:-none}"
cur=; for tok in $exec_line; do tok=${tok#ExecStart=}; [[ $tok == */lamp-voice ]] && { cur=$tok; break; }; done
[[ -n $cur ]] || fail "no lamp-voice path in $exec_line"
INST=$(on "readlink -f $cur"); [[ -n $DRY && -z $INST ]] && INST=/opt/lampOS/voice/releases/dry/lamp-voice
log "installed lamp-voice: $cur -> ${INST:-unread}"; [[ -n $INST ]] || fail "the installed lamp-voice path did not resolve"
voice_is() { # voice_is P|W: lamp-voice runs the installed path, with (W) both switches or (P) neither in its environ
  local got path nw nh; got=$(on "p=\$(systemctl show -p MainPID --value lamp-voice); e=\$(tr '\\0' '\\n' </proc/\$p/environ); echo \$(readlink /proc/\$p/exe) \$(grep -c '^LAMP_STOP_WORDS=' <<<\"\$e\") \$(grep -c '^LAMP_BACKCHANNEL_HOLDS=' <<<\"\$e\") \$(grep -cx '$SW_WORDS' <<<\"\$e\") \$(grep -cx '$SW_HOLDS' <<<\"\$e\")")
  if [[ -n $DRY ]]; then local d=${DRY_ENVIRON:-}; [[ -z $d ]] && { [[ $1 == W ]] && d="1 1" || d="0 0"; }
    got="${DRY_EXE:-$INST} $d $d"; fi
  local aw ah; read -r path nw nh aw ah <<<"$got"
  log "arm $1: lamp-voice runs ${path:-unread}; in environ LAMP_STOP_WORDS= ${nw:-?} (=heard ${aw:-?}), LAMP_BACKCHANNEL_HOLDS= ${nh:-?} (=on ${ah:-?})"
  [[ $path == "$INST" ]] || fail "arm $1: lamp-voice runs ${path:-unread}, not the installed $INST"
  if [[ $1 == W ]]; then [[ $aw == 1 && $ah == 1 && $nw == 1 && $nh == 1 ]] || fail "arm W: $SW_WORDS and $SW_HOLDS are not both (and only) in lamp-voice's environ"
  else [[ $nw == 0 && $nh == 0 ]] || fail "arm P: lamp-voice has LAMP_STOP_WORDS or LAMP_BACKCHANNEL_HOLDS in its environ: not the product set"; fi; }
[[ -z $foreign ]] || fail "another /run drop-in on lamp-voice: $foreign"
PLAYED=
# The lamp-side put-back, run by the kit's EXIT trap and by the dead-man timer alike: the drop-in (one restart), Speaker
# 50%; then it checks, writes the QUIET line to loans.txt, and exits non-zero on anything left (keeping the dead-man armed).
cat >"$OUT/putback.sh" <<PUTBACK
#!/bin/bash
systemctl reset-failed lamp-voice 2>/dev/null
if [ -f $DROPIN ]; then rm -f $DROPIN; systemctl daemon-reload; systemctl restart lamp-voice; sleep 3; fi
amixer -q -c device_cmedia sset Speaker 50%
bad=; exe=\$(readlink /proc/\$(systemctl show -p MainPID --value lamp-voice)/exe); echo "lamp-voice runs \$exe"
systemctl is-active -q lamp-voice || bad="\$bad lamp-voice-not-active"
[ "\$exe" = "$INST" ] || bad="\$bad not-the-installed-lamp-voice(\$exe)"
tr '\\0' '\\n' </proc/\$(systemctl show -p MainPID --value lamp-voice)/environ 2>/dev/null | grep -qE '^LAMP_(STOP_WORDS|BACKCHANNEL_HOLDS)=' && bad="\$bad switch-still-in-environ"
[ -e $DROPIN ] && bad="\$bad drop-in-left"
echo "\$(date +%H:%M) $WHO: #3997 wordless cut-in hold A/B: \$2; put back by \$1 (drop-in removed, Speaker 50%)\${bad:+; PUT-BACK FAILED:\$bad}; loan ended" >>~orangepi/loans.txt
[ -z "\$bad" ] || { echo "PUT-BACK FAILED:\$bad (the dead-man $DEAD and $BACK are kept)"; exit 1; }
[ "\$1" = deadman ] || systemctl stop $DEAD.timer $DEAD.service 2>/dev/null
rm -f $BACK; echo PUTBACK-OK
PUTBACK
ran() { # what the tally holds, per arm and row: "P C 4/5" (valid/tries)
  [[ -s ${tally:-} ]] || { echo "no tries"; return; }
  awk -F'\t' '{k=$1" "$2; if(!(k in n)) o[++m]=k; n[k]++; if($4=="yes") v[k]++} END{for(i=1;i<=m;i++) printf "%s%s %d/%d", (i>1?", ":""), o[i], v[o[i]], n[o[i]]}' "$tally"; }
verdicts() { [[ -s ${tally:-} ]] || return 0; python3 -I "$HERE/score-cutin-words.py" verdict "$tally" "$C_TRIALS" "$O_TRIALS" "$YIELD_TOL_MS" | tee -a "$OUT/log.txt"; }
# quiet_poster SAID: the QUIET post, started first in the trap and detached (its own session: lamp-lock TERMs the kit's
# process group and KILLs it 10 s later). It waits up to QUIET_WAIT s for the put-back result in $OUT/putback.res, then
# posts what is known, gh capped at 15 s. DRY posts through DRY_GH (a stub) when set, else logs "would post".
QUIET_HEAD=
quiet_poster() {
  QUIET_HEAD="$WHO: QUIET $LAMP $(date +%H:%M) (GMT+7): #3997's wordless cut-in hold A/B: $1."
  [[ -n $DRY || -n ${QUIET_OWED:-} ]] || return 0
  local gh=gh; [[ -n $DRY ]] && gh=${DRY_GH:-}
  rm -f "$OUT/putback.res"
  nohup python3 -c 'import os,sys,time,subprocess
os.setsid()
out,head,gh,wait=sys.argv[1],sys.argv[2],sys.argv[3],float(sys.argv[4]); res=None; end=time.time()+wait
while time.time()<end:
  try: res=open(out+"/putback.res").read().strip(); break
  except OSError: time.sleep(0.2)
body=head+" Put-back: "+(res or "result not read before the kit ended (the lamp runs its put-back in its own unit; the dead-man stays armed)")+"."
log=lambda m: open(out+"/log.txt","a").write(time.strftime("%H:%M:%S")+" "+m+"\n")
if not gh: log("[dry] would post the QUIET on #3336: "+body); sys.exit()
try: r=subprocess.run([gh,"issue","comment","3336","-R","autonomous-ai/lampOS","--body",body],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15).returncode
except Exception as e: r=type(e).__name__
log(("QUIET posted on #3336: " if r==0 else "QUIET not posted on #3336 (%s): post it by hand: " % r)+body)' "$OUT" "$QUIET_HEAD" "$gh" "${QUIET_WAIT:-12}" >/dev/null 2>&1 &
}
put_back() { # the EXIT trap; TERM/INT/HUP reach it through exit. Runs once, with TERM/INT/HUP ignored.
  trap '' INT TERM HUP; trap - EXIT
  [[ -n ${PUTBACK_RAN:-} ]] && return 0; PUTBACK_RAN=1
  local said="aborted, nothing ran" res; [[ -n $PLAYED ]] && said="done"; [[ -z ${DONE:-} && -n $PLAYED ]] && said="stopped early (${FAIL_WHY:-killed})"; [[ -n ${ABORTED:-} ]] && said="stopped at the hard end $HARD_END"
  [[ -n ${SIG:-} ]] && said="stopped by $SIG${FAIL_WHY:+ ($FAIL_WHY)}"
  said="$said; ran: $(ran)"
  quiet_poster "$said" # first: the QUIET goes out even if lamp-lock KILLs the kit's group mid put-back
  stopplays
  [[ -n ${MACVOL:-} ]] && run osascript -e "set volume output volume $MACVOL"
  if [[ -n ${SIG:-} ]]; then # the lamp's put-back in its own systemd unit; its result read for at most 2 s
    res=$(on "if [ -f $BACK ]; then systemd-run -q --collect --unit sixth-cutin-putback-now /bin/bash $BACK kit '${said//\'/}' && for i in 1 2; do sleep 1; systemctl is-active -q sixth-cutin-putback-now || break; done; journalctl -u sixth-cutin-putback-now --no-pager -o cat | grep -E 'PUTBACK|lamp-voice runs' | tail -2; systemctl is-active -q sixth-cutin-putback-now && echo 'put-back still running in its own unit (the dead-man stays armed until it ends)'; else echo 'no put-back script on the lamp: nothing staged'; fi" 2>&1)
  else
    res=$(on "if [ -f $BACK ]; then bash $BACK kit '${said//\'/}'; else echo 'no put-back script on the lamp: nothing staged'; fi" 2>&1)
  fi
  [[ -n $DRY ]] && { log "[dry] put-back: $res"; res=${DRY_PUTBACK:-"PUTBACK-OK (dry: ${res:0:120})"}; }
  log "put-back: $res"
  local pb="FAILED ($res); the lamp-side dead-man stays armed"
  [[ $res == *"nothing staged"* ]] && pb="nothing was staged"
  [[ $res == *"still running"* ]] && pb="running on the lamp in its own unit, result not read before the kit ended (the dead-man stays armed until it ends)"
  [[ $res == *PUTBACK-OK* ]] && pb="drop-in removed, lamp-voice on the installed path without the switches, Speaker 50%"
  printf '%s' "$pb" >"$OUT/putback.res.tmp" && mv "$OUT/putback.res.tmp" "$OUT/putback.res"
  log "QUIET: $QUIET_HEAD Put-back: $pb. (posted by the detached poster)"
  verdicts
  if [[ $res != *PUTBACK-OK* && $res != *"nothing staged"* && $res != *"still running"* ]]; then
    echo "$(date +%H:%M) $WHO: #3997 PUT-BACK FAILED from the Mac ($said): $res; the lamp-side dead-man $DEAD stays armed" | lamp "sudo tee -a ~orangepi/loans.txt >/dev/null"
    echo "PUT-BACK FAILED: $res"; exit 3
  fi
}
trap put_back EXIT; trap 'SIG=TERM; exit 143' TERM; trap 'SIG=INT; exit 130' INT; trap 'SIG=HUP; exit 129' HUP
[[ -n $DRY ]] || caffeinate -d -w $$ & # ends with the kit
[[ ${NO_PERSON:-} == yes ]] || fail "manual check: read the Mac camera and set NO_PERSON=yes when nobody is in view"
log "manual check: no person in view at start (Mac camera)"
switched=$(on "journalctl -u lamp-update --since -3min --no-pager -o cat | grep -iE 'switch|promot|install' | tail -1"); [[ -n $DRY ]] && switched=${DRY_SWITCHED:-}
[[ -z $switched ]] || fail "lamp-update switched in the last 3 min: $switched"
other "the start"
left=$(( (END_S - $(now_s)) / 60 ))
(( left >= NEED_MIN )) || fail "the hard end $HARD_END leaves $left min, under the $NEED_MIN min both arms need"
dead=$(( END_S - 120 - $(now_s) ))
run scp -q "$OUT/putback.sh" ${host}:/tmp/sixth-cutin-putback.sh \
  && on "install -m 0700 /tmp/sixth-cutin-putback.sh $BACK && rm -f /tmp/sixth-cutin-putback.sh && systemd-run -q --unit $DEAD --on-active=${dead}s /bin/bash $BACK deadman 'the slot ended'" \
  || fail "arming the dead-man"
log "dead-man armed: the lamp puts itself back in ${dead}s (2 min before $HARD_END)"
PLAYING_TEXT="$WHO: PLAYING on $LAMP $(date +%H:%M) (GMT+7), #3997 step 2, the wordless cut-in hold A/B: Mac speaker clips, no person, a quiet room; arm P the installed product unit untouched, arm W the same installed lamp-voice with only a /run drop-in adding $SW_WORDS and $SW_HOLDS (no binary staged). Hard end $HARD_END; a lamp-side dead-man puts back at $HARD_END - 2 min."
if [[ -n $DRY ]]; then log "[dry] would post on #3336: $PLAYING_TEXT"
else gh issue comment 3336 -R autonomous-ai/lampOS --body "$PLAYING_TEXT" >/dev/null && QUIET_OWED=1 || fail "PLAYING not posted on #3336"; fi
echo "$(date +%H:%M) $WHO: #3997 wordless cut-in hold A/B on the product unit (arm P untouched, arm W the installed lamp-voice + /run zzzz-sixth-cutin with the two switches only; dead-man $DEAD at $HARD_END - 2 min), Mac voices, Speaker 50%, no person" | lamp "sudo tee -a ~orangepi/loans.txt >/dev/null"
MACVOL=$(osascript -e "output volume of (get volume settings)")
SPKWAS=$(lamp "amixer -c device_cmedia sget Speaker" | grep -om1 '[0-9]*%'); [[ -n $DRY ]] && SPKWAS=dry; on "amixer -q -c device_cmedia sset Speaker 50%"
log "Speaker found at ${SPKWAS:-unread}; set to 50% (the rule), and put back at 50%"
tally=$OUT/tally.tsv; : >"$tally"
# Per try, only the kinds the score reads (the whole file was 5.7 MB, ~7 s a read, on 30 Oct); the whole file and the
# journal are fetched once per arm at its end (keep()).
KINDS_RE='"kind":"(duplex_yield|duplex_yield_end|duplex_commit|duplex_resume|stop_words|stop_heard|speaker_first_write|turn_finished|input_admitted|transcript)"'
READ='S=$(systemctl show lamp-voice -p InvocationID --value); grep -hE '"'$KINDS_RE'"' /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl'
# story_done ASK_US: wait for the story's turn_finished after ASK_US, at most 30 s, then 3 s (row-3967d's held stream).
STORY_TAIL='S=$(systemctl show lamp-voice -p InvocationID --value); timeout 30 tail -n 3000 -F /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl 2>/dev/null | grep --line-buffered turn_finished'
story_done() {
  (( ${1:-0} )) || { log "story_done: no ask time, waiting 60 s instead of matching an old turn"; nap 60; return; }
  local got t=$SECONDS
  got=$(python3 -c 'import sys,json
a=int(sys.argv[1])
for l in iter(sys.stdin.readline, ""):
  try: r=json.loads(l)
  except ValueError: continue
  if r.get("kind")=="turn_finished" and r.get("at_us",0)>a: print(r["at_us"], r.get("outcome")); sys.exit(0)
sys.exit(1)' "$1" < <(if [[ -n $DRY ]]; then cat "$DRY_CASE"/feed.jsonl 2>/dev/null; else $L "sudo bash -c $(printf %q "$STORY_TAIL")"; fi))
  if [[ -n $got ]]; then log "story_done: turn_finished at_us ${got% *} (${got#* }) after ask $1, $(( SECONDS - t )) s"
  else log "story_done: no turn_finished after ask $1 within 30 s"; [[ -z $DRY ]] || nap 30; fi
  nap 3; }
seqread() { SEQNOW=$(on "jq .installed.sequence /var/lib/lampos-update/state.json")
  if [[ -n $DRY ]]; then SEQNOW=${DRY_SEQ:-$SEQ}; (( ${SEQFAILS:=0} < ${DRY_SEQFAIL:-0} )) && { SEQFAILS=$(( SEQFAILS + 1 )); SEQNOW=; }; fi; }
dryscene() { # DRY: the scene scenario.py would have written, with its #PLAY_US
  [[ -n $DRY ]] || return 0; mkdir -p "$OUT/scenarios"; printf '#PLAY_US %s wav: %s\n' "$2" "$3" >"$OUT/scenarios/dry-$1.txt"; }
trial() { # trial ARM ROW I: ROW C (a cut-in CUT_AFTER s into the answer) or O (O_RUN s of answer, then the end line)
  local arm=$1 kind=$2 i=$3 ask phrase n began askpid ask_us scene wait
  ask=${ASKS[$(( (i - 1) % 5 ))]}
  if [[ $kind == C ]]; then phrase=${CUTS[$(( (i - 1) % 4 ))]} wait=$CUT_AFTER; else phrase=$ENDLINE wait=$O_RUN; fi
  n=rcw-$arm$kind-$i-$(date +%H%M%S)
  seqread; [[ -n $SEQNOW ]] || { log "$arm$kind $i: the seq read came back empty, retrying once"; nap 2; seqread; }
  [[ $SEQNOW == "$SEQ" ]] || fail "the lamp left $SEQ (now $SEQNOW) before $arm$kind $i: tries after this are dropped"
  JT=$(( $(date +%s) - 1 ))
  stray "$arm$kind $i"
  bgplay python3 "$KIT/scenario.py" "$n-ask" "wav|$(wav "$ask")|0|$ask"; askpid=$BG
  dryscene "$n-ask" "$DRY_ASK_US" "$ask"
  if [[ -n $DRY ]]; then began=0; [[ " ${DRY_NOANSWER:-} " == *" $arm$kind "* ]] && began=1; nap 8
  else grep -m1 -q 'first answer audio' < <($L "sudo timeout 35 journalctl -u lamp-voice -f --since @$JT --no-pager"); began=$?; fi
  if (( began != 0 )); then reap $askpid; log "$arm$kind $i no answer within 35 s; not a try"
    printf '%s\t%s\t%s\tno\n' "$arm" "$kind" "$i" >>"$tally"; story_done "$(ask_at "$n")"; return 1; fi
  nap "$wait"
  run python3 "$KIT/scenario.py" "$n" "wav|$(wav "$phrase")|8|$phrase" >/dev/null
  dryscene "$n" "$(( DRY_ASK_US + (wait + 6) * 1000000 ))" "$phrase" # DRY: the ask's ~6 s to first audio, then the wait
  reap $askpid
  ask_us=$(ask_at "$n")
  story_done "$ask_us"
  scene=$(ls -t "$OUT"/scenarios/*-"$n".txt 2>/dev/null | head -1)
  if [[ -n $DRY ]]; then cp "$DRY_CASE/$arm$kind.jsonl" "$OUT/events.jsonl" 2>/dev/null || : >"$OUT/events.jsonl"
  else on "$READ" >"$OUT/events.jsonl"; fi
  if [[ $kind == C ]]; then python3 -I "$HERE/score-cutin-words.py" C "$arm" "$i" "$ask_us" "${scene:-}" "$OUT/events.jsonl" "$tally" "$(dur_us "$(wav "$phrase")")" >>"$OUT/log.txt"
  else python3 -I "$HERE/score-cutin-words.py" O "$arm" "$i" "$ask_us" "${scene:-}" "$OUT/events.jsonl" "$tally" >>"$OUT/log.txt"; fi
  [[ -n $DRY ]] && tail -1 "$OUT/log.txt" >&2
  return 0
}
row() { # row ARM ROW NEED MAX: up to MAX tries for NEED valid ones
  local arm=$1 kind=$2 need=$3 max=$4 i=0 valid=0
  while (( valid < need && i < max )); do
    hard_end "$arm$kind try $((i + 1))"
    i=$((i + 1))
    trial "$arm" "$kind" "$i"
    valid=$(awk -F'\t' -v a="$arm" -v k="$kind" '$1 == a && $2 == k && $4 == "yes"' "$tally" | wc -l | tr -d ' ')
    [[ -z $DRY ]] && on "journalctl -u lamp-voice --since -2min --no-pager | grep -q 'HTTP 429'" && fail "relay HTTP 429"
    :
  done
  log "arm $arm row $kind: $valid valid of $i tries"
}
keep() { # keep ARM: the arm's whole events file (gzip) and journal, read-only, to the Mac, with the journal's word lines
  [[ -n $DRY ]] && { log "[dry] would fetch arm $1's events and journal"; return 0; }
  on 'S=$(systemctl show lamp-voice -p InvocationID --value); cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl | gzip -c' >"$OUT/arm-$1-events.jsonl.gz"
  on "journalctl -u lamp-voice --since @$ARM_AT --no-pager -o short-iso" >"$OUT/arm-$1-journal.txt"
  log "arm $1 kept: events $(gzip -dc "$OUT/arm-$1-events.jsonl.gz" | wc -l | tr -d ' ') lines, journal $(wc -l <"$OUT/arm-$1-journal.txt" | tr -d ' ') lines; journal: $(grep -c "words came: 0 words" "$OUT/arm-$1-journal.txt") yields with 0 words, $(grep -c "commit was held .*: committed" "$OUT/arm-$1-journal.txt") held then committed, $(grep -c "held as a yield with no words" "$OUT/arm-$1-journal.txt") held as wordless"; }
CUR_ARM=P
arm() { # arm NAME: put lamp-voice in arm NAME, prove it, hello, then rows C and O
  local name=$1 got
  hard_end "arm $name"
  other "arm $name"
  if [[ $name == W && $CUR_ARM != W ]]; then # the Environment lines only: the installed ExecStart stays
    printf '%s\n' '# #3997 wordless cut-in hold A/B (Sixth), removed after' '[Service]' "Environment=$SW_WORDS" "Environment=$SW_HOLDS" | base64 | tr -d '\n' >"$OUT/dropin.b64"
    DROPIN_AT=$(on "date +%s"); DROPIN_AT=${DROPIN_AT:-0}
    on "mkdir -p ${DROPIN%/*} && echo $(cat "$OUT/dropin.b64") | base64 -d > $DROPIN && systemctl daemon-reload && systemctl restart lamp-voice"
    nap 20
    got=$(on "journalctl -u lamp-voice --since @$DROPIN_AT --no-pager -o cat | grep -E 'stop words:|backchannel holds|backchannels are held only' | head -4")
    [[ -n $DRY ]] && got=${DRY_STARTUP-"lamp-voice: stop words: heard, in a yield, from the processed blocks, through the second voice's speech-to-text at relay
lamp-voice: backchannel holds: a yield's commit waits for its words, up to 500 ms after they are asked for, and a backchannel resumes the answer"}
    log "arm W startup: ${got:-none}"
    [[ $got == *"stop words: heard, in a yield"* ]] || fail "arm W: no 'stop words: heard, in a yield' line after the restart"
    [[ $got == *"backchannel holds: a yield's commit waits"* && $got != *"backchannels are held only"* ]] || fail "arm W: no 'backchannel holds' line after the restart"
    CUR_ARM=W
  fi
  voice_is "$name"
  ARM_AT=$(on "date +%s"); ARM_AT=${ARM_AT:-0}
  run osascript -e "set volume output volume $SYSVOL"
  if [[ -n $DRY ]]; then [[ ${DRY_HELLO:-} != fail ]] || fail "arm $name: hello not heard"
  else run python3 "$KIT/scenario.py" "rcw-$name-hello" "wav|$(wav "$HELLO")|8|hello" | grep -q ' HEARD .*[A-Za-z]' || fail "arm $name: hello not heard"; fi
  PLAYED=1; nap 10
  row "$name" C "$C_TRIALS" "$C_MAX"
  row "$name" O "$O_TRIALS" "$O_MAX"
  keep "$name"
}
for a in $ARMS; do arm "$a"; log "arm $a done"; done
DONE=1
