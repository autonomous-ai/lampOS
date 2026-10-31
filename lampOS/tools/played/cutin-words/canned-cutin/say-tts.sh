#!/bin/bash
# say-tts.sh OUT.wav "TEXT" [VOICE_ID]: a TTS stand-in for row-cutin-words.sh's matrix, macOS `say` only. It writes
# AIFF to the .wav path on purpose: the kit must convert it (afconvert) before levelling.
set -e; say -o "$1.aiff" "$2" && mv "$1.aiff" "$1"
