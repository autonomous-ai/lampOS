#!/usr/bin/env bash
# render.sh: renders every line of lines.txt (and the wake and busy-room lines below) to
# clips/<id>.wav, levelled to -20 LUFS, skipping clips already there. Mac only.
# TTS is a script `TTS OUT.wav TEXT VOICE_ID` (default: the Mac kit's ElevenLabs relay script); it
# never prints its key. The TV is a colleague's voice through a small speaker's band, 6 dB under.
set -u
cd "$(dirname "$0")" || exit 1
TTS=${TTS:-$HOME/lamp-rows/fifth/tts.sh}
mkdir -p clips
voice() { case $1 in Daniel) echo pNInz6obpgDQGcFmaJgB ;; Samantha|TV) echo 21m00Tcm4TlvDq8ikWAM ;;
  Karen) echo XB0fDUnXU5powFXDhCwa ;; *) return 1 ;; esac; }
bad=0
render() { # render <id> <speaker> <text>
  local f=clips/$1.wav raw=clips/$1.raw.wav v af=loudnorm=I=-20:TP=-1.5:LRA=11
  [ -s "$f" ] && return 0
  v=$(voice "$2") || { echo "BAD speaker $2 ($1)"; bad=1; return; }
  "$TTS" "$raw" "$3" "$v" >/dev/null || { echo "FAILED $1"; rm -f "$raw"; bad=1; return; }
  [ "$2" = TV ] && af="highpass=f=300,lowpass=f=3400,aecho=0.8:0.6:40:0.25,$af,volume=-6dB"
  ffmpeg -hide_banner -loglevel error -y -i "$raw" -af "$af" -ar 44100 "$f" && rm -f "$raw" && echo "rendered $1"
}
render w00 Daniel "Lamp, are you there?"
render t01 Samantha "So did you see the numbers from the last sprint? I thought the review went fine but the deadline moved again."
render t02 Karen "Yeah, the review was fine, I just hope finance signs off on the budget this time."
while IFS='|' read -r id _ _ _ _ who text _; do
  case $id in ''|'#'*) continue ;; esac
  render "$id" "$who" "$text"
done < lines.txt
exit $bad
