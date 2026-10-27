#!/usr/bin/env bash
# make-prompts.sh — the routine run's and the factory steps' (factory-voice.py)
# spoken step prompts, made once on a Mac
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
factory_say_ready|Say ready when you are ready.
factory_auto_start|Starting in five seconds.
factory_stand_ready|Next, the arm stand. Rest one hand lightly on my arm.
factory_held|I am holding my arm. I will let it go now. Keep a hand by my arm and my head.
factory_stand_go|I am standing up now. Say stop at any time.
factory_stand_30|Standing to thirty percent.
factory_stand_100|Standing all the way up.
factory_stand_done|Arm measured. Folding down.
factory_head_off|My head is not on its stop. Press my head down, then run this step again.
factory_pads_ready|Next, the head pads. Hands off me, and do not touch until I say head pad one.
factory_pads_away|Listening now. Hands off for eleven seconds.
factory_pads_1|Now head pad one. You have ten seconds. Press and hold it for two seconds, twice.
factory_pads_2|Now head pad two. You have ten seconds. Press and hold it for two seconds, twice.
factory_pads_more|Any other head pad, the same way.
factory_pads_wait|Hands away now. Please wait.
factory_pads_done|Head pads done.
factory_key_done|Update key checked.
factory_selftest_ready|Next, the self-test. Keep a hand near my arm, not on it.
factory_selftest_brief|Self-test next. In about one minute I will stand up. When I am up, touch my head, then press my button. Keep doing it for twenty seconds.
factory_selftest_brief_strip|Self-test next. In about one minute I will stand up. When I am up, swipe my base, touch my head, then press my button. Keep doing it for twenty seconds.
factory_selftest_wait|Please wait about one minute.
factory_selftest_stand|Standing up for a small head move. Stand clear.
factory_touch_now|Now. Touch my head, then press my button.
factory_touch_now_strip|Now. Swipe my base, touch my head, then press my button.
factory_selftest_pass|Self-test passed.
factory_selftest_fail|Self-test failed. Please check the laptop.
factory_reset_ready|Say yes to reset me for packing.
factory_resetting|Resetting now. Remove me from the factory account, then pack me.
factory_failed|A step failed. Please check the laptop.
factory_all_done|All steps done. Thank you.
factory_inputs_wait|Input test. Touch any head pad or press my button to begin.
factory_inputs_touch|Thirty seconds. Touch each head pad, press my button, and press the reset button briefly. Any order, as many times as you like.
factory_padcheck_10|Ten seconds left.
factory_padcheck_rest|Now hands off me for thirty seconds. Do not touch.
factory_inputs_done|Inputs recorded.
factory_inputs_fail|Input test failed. Please check the laptop.
EOF
