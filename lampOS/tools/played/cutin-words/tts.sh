#!/bin/bash
# tts.sh OUT.wav "TEXT" [VOICE_ID]: renders one clip of row-cutin-words.sh (ElevenLabs eleven_v4_turbo through the
# Autonomous relay, 24 kHz mono). The relay key is read from LAMP_RELAY_KEY_FILE (default ~/.config/lamp-relay.key,
# mode 600) and never printed. The kit calls it once per missing clip and caches the result in WAVS.
set -e
K=${LAMP_RELAY_KEY_FILE:-$HOME/.config/lamp-relay.key}
[ -s "$K" ] || { echo "tts.sh: no relay key in $K (set LAMP_RELAY_KEY_FILE)" >&2; exit 1; }
H=$(mktemp); trap 'rm -f "$H" "$H.pcm"' EXIT
printf 'xi-api-key: %s\n' "$(cat "$K")" > "$H"
body=$(python3 -c 'import json,sys; print(json.dumps({"text":sys.argv[1],"model_id":"eleven_v4_turbo","voice_settings":{"speed":1.0}}))' "$2")
code=$(curl -s -o "$H.pcm" -w '%{http_code}' -H @"$H" -H 'content-type: application/json' -d "$body" \
  "https://campaign-api.autonomous.ai/api/v1/ai/v1/elevenlabs/text-to-speech/${3:-21m00Tcm4TlvDq8ikWAM}/stream?output_format=pcm_24000")
[ "$code" = 200 ] || { echo "http $code"; exit 1; }
python3 -c 'import wave,sys; d=open(sys.argv[1],"rb").read(); w=wave.open(sys.argv[2],"wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(24000); w.writeframes(d); w.close()' "$H.pcm" "$1"
echo "ok $1"
