# lamp, root sh (common.lamp.sh first, then CLIPS=<file on the lamp>, VOICE=, OUT=).
# Renders every "id<TAB>text" line of $CLIPS through the lamp's own ElevenLabs
# relay, as r7140's KEEP10-el.wav and e2e/s4's render-relay.sh were made: same
# URL, voice and model. The credential is read as root into curl's stdin
# headers and never printed. Output: $OUT/<id>.pcm (s16le 24 kHz mono).
URL="https://campaign-api.autonomous.ai/api/v1/ai/v1/elevenlabs/text-to-speech/$VOICE/stream?output_format=pcm_24000"
rm -rf $OUT; mkdir -p $OUT; bad=0
while IFS="$(printf '\t')" read -r id text; do
  [ -z "$id" ] && continue
  body=$(python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"model_id":"eleven_flash_v2_5","voice_settings":{"speed":1.0}}))' "$text")
  code=$(tr -d '\n' < /etc/lampOS/voice/credential | sed 's/^/xi-api-key: /' | curl -s -m 30 -o $OUT/$id.pcm -w '%{http_code}' -H @- -H 'Content-Type: application/json' -X POST --data "$body" "$URL")
  sz=$(stat -c %s $OUT/$id.pcm 2>/dev/null || echo 0)
  echo "RENDER $id http=$code bytes=$sz"
  { [ "$code" = 200 ] && [ "$sz" -gt 20000 ]; } || bad=$((bad+1))
  sleep 1
done < $CLIPS
chown -R $LAMP_USER $OUT; chmod -R a+rX $OUT; rm -f $CLIPS
n "rendered $(ls $OUT | wc -l) clips via the voice relay into $OUT (no state changed; $bad bad)"
echo "RENDER-BAD $bad"
