#!/usr/bin/env bash
# make-prompts.sh — the routine run's spoken step prompts, made once on a Mac
# (lamps have no speech of their own without the cloud), as the bench test's
# mac/make_voice.sh does. Each is one short phrase, 24 kHz 16-bit mono, levelled
# to about -20 dBFS (the lamp's own speech), so no prompt is louder than the
# lamp's voice. They are not committed (binary): make them, then copy them.
#
#   bash make-prompts.sh [OUT_DIR]            # default ./prompts
#   scp -r prompts orangepi@<lamp>:/tmp/ && ssh orangepi@<lamp> \
#     'sudo mkdir -p /var/lib/lamp-test && sudo cp -r /tmp/prompts /var/lib/lamp-test/'
#
# Needs macOS `say` and `afconvert`; with ffmpeg the level is normalised,
# without it the phrases keep `say`'s own level and it says so.
set -euo pipefail
OUT="${1:-./prompts}"
VOICE="${VOICE:-Samantha}"
mkdir -p "$OUT"
command -v say >/dev/null || { echo "make-prompts: needs macOS say" >&2; exit 2; }
have_ffmpeg=0; command -v ffmpeg >/dev/null && have_ffmpeg=1
[ "$have_ffmpeg" -eq 1 ] || echo "make-prompts: no ffmpeg, so the level is not normalised" >&2

while IFS='|' read -r key text; do
  [ -n "$key" ] || continue
  say -v "$VOICE" -r 175 -o "$OUT/$key.aiff" "$text"
  if [ "$have_ffmpeg" -eq 1 ]; then
    ffmpeg -hide_banner -loglevel error -y -i "$OUT/$key.aiff" \
      -af "loudnorm=I=-20:TP=-6:LRA=7" -ar 24000 -ac 1 -c:a pcm_s16le "$OUT/$key.wav"
  else
    afconvert -f WAVE -d LEI16@24000 -c 1 "$OUT/$key.aiff" "$OUT/$key.wav"
  fi
  rm -f "$OUT/$key.aiff"
  echo "  $key.wav  - $text"
done <<'EOF'
start|Starting the lamp test.
inspection|Checking the assembly. Look the lamp over.
pads|Testing the head pads. Touch each pad twice.
selftest|Running the self-test. Touch the head pads and press the button when asked.
ring|Testing the light ring. Watch every pixel.
privacy|Testing the privacy switch. Slide it closed, then open.
buzz|Listen to the lamp's voice for buzz or rattle.
exchange|Final check. Say: Hey Lamp, what time is it?
done_pass|Test complete. All checks passed.
done_fail|Test complete. Some checks did not pass.
EOF
