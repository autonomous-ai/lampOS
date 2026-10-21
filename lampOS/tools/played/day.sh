#!/bin/zsh
# The V2 MVP end to end, played: a person's day at the lamp, every piece the voice can reach, one transcript.
# Waits for quiet first (quiet_gate.sh). Writes $PLAYED_OUT/scenarios/<time>-day-LABEL.txt via scenario.py.
# Since 2026-10-15 08:05 also: a look ("what do you see"), a yes in two breaths, a reminder "in two minutes"
# (its due line is the last thing heard) and a long result. argv: LABEL
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:-$(date +%m%d-%H%M)}
until ./quiet_gate.sh 45 >/dev/null 2>&1; do sleep 10; done
./scenario.py day-$L \
  "Samantha|Hey Lamp, good morning!|14" \
  "Samantha|Can you make your light a little warmer?|14" \
  "Samantha|What do you see right now?|18" \
  "Samantha|Remember that my favourite colour is green.|14" \
  "Samantha|Hey Lamp, can you ask Claude Code on my office computer to write a short story, about two hundred words, about a desk lamp that learns to sing?|14" \
  "Samantha|Yes. [[slnc 700]] Please start it.|100" \
  "Samantha|How is that task going?|16" \
  "Samantha|Remind me in two minutes to stretch.|14" \
  "Samantha|What's my favourite colour?|14" \
  "Samantha|What time is it?|14" \
  "Samantha|Set a timer for one minute.|80" \
  "Samantha|Thanks, Lamp. See you later.|60"
