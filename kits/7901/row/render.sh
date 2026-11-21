#!/bin/bash
# Mac, lock held if there is one (#7901). Renders clips/clips.txt lines with no matching
# clips/<id>-el.wav + sidecar, through the lamp's own ElevenLabs relay (the credential stays on the lamp; this
# kit's clips are rendered on the runner's Mac, needs ffmpeg and ffprobe there), voice ErXwobaYiN019PkySvjV, eleven_flash_v2_5, speed 1.0, pcm 24 kHz, 0.4 s of
# digital silence in front (no level change). No lamp state changed, no lamp-voice start.
set -u; D=$(cd "$(dirname "$0")" && pwd); . $D/kit.sh; RENEW || exit 1
OUT=/tmp/row-7901-el; CL=/tmp/row-7901-clips.txt; PAD=${PAD:-0.4}; PADMS=$(python3 -c "print(round($PAD*1000))")
: > $D/logs/render-clips.txt
while IFS=$'\t' read -r id text gain; do [ -z "$id" ] && continue
  [ -f $A/$id-el.wav ] && [ "$(cut -f1 $A/$id-el.txt 2>/dev/null)" = "$text" ] && continue
  printf '%s\t%s\n' "$id" "$text" >> $D/logs/render-clips.txt; done < $A/clips.txt
[ -s $D/logs/render-clips.txt ] || { echo "RENDER nothing to do"; exit 0; }
echo "RENDER $(cut -f1 $D/logs/render-clips.txt | tr '\n' ' ')"
$SCP -q $D/logs/render-clips.txt $H:$CL || exit 1
LAMP "CLIPS=$CL VOICE=ErXwobaYiN019PkySvjV OUT=$OUT" $D/render.lamp.sh | tee $D/logs/render.log
grep -q '^RENDER-BAD 0$' $D/logs/render.log || { echo "RENDER FAILED (logs/render.log)"; $SSH -n $H "rm -rf $OUT"; exit 1; }
for id in $(cut -f1 $D/logs/render-clips.txt); do
  $SCP -q "$H:$OUT/$id.pcm" $A/ || exit 1
  ffmpeg -nostdin -loglevel error -y -f s16le -ar 24000 -ac 1 -i $A/$id.pcm -af "adelay=${PADMS}:all=1" -c:a pcm_s16le $A/$id-el.wav || exit 1; rm -f $A/$id.pcm
  printf '%s\tpad=%s\n' "$(awk -F'\t' -v c=$id '$1 == c {print $2}' $A/clips.txt)" "$PAD" > $A/$id-el.txt
  echo "CLIP $id $(ffprobe -v error -show_entries format=duration -of csv=p=0 $A/$id-el.wav) s"
done
$SSH -n $H "rm -rf $OUT"; echo "RENDER OK"
