#!/bin/zsh
# The quiet-room scene for LAMP_SAME_VOICE_CALLER_ONLY (#2650/#2663, the first builder on #2650 at 01:11): the
# two-person scenes make the room busy by themselves, so the quiet-room rule the switch works with never comes into
# play. Here each trial is one caller's named question, then one line of a second voice that is a statement not for
# the lamp, said inside the conversation's window; trials are spaced past the busy-room count (two lines left
# unanswered within a minute make a quiet room busy), so the room stays quiet. Run it with the switch off and on, the
# same hour, and compare the second voice's lines answered. PLAYS SOUND IN THE ROOM. About 1.5 minutes a trial.
# argv: TRIALS LABEL. Prints, per trial, whether the lamp spoke after the second voice's line, then the count.
cd "${0:A:h}" || exit 9
. ./settings.sh
N=${1:-6}; L=${2:-$(date +%m%d-%H%M)}
asks=("Hey Lamp, what is the weather like in Hanoi today?" "Hey Lamp, what time is it in Tokyo?"
  "Hey Lamp, how far is the moon?" "Hey Lamp, what is a good name for a cat?"
  "Hey Lamp, how many days until Christmas?" "Hey Lamp, what is the capital of Peru?")
others=("I should bring an umbrella then." "I left my keys on the kitchen table."
  "The meeting moved to three this afternoon." "My sister is coming over on Sunday."
  "I think the printer is out of paper again." "We need to buy more coffee for the office.")
latest() { ls -t "$PLAYED_OUT"/scenarios/*.txt | head -1 }
# Whether the lamp said anything after the scene's last PLAY line.
spoke_after_last() { /usr/bin/awk '/ PLAY /{a=0; next} / LAMP /{if ($0 !~ /LAMP *$/) a=1} END{print (a ? "answered" : "left alone")}' "$1" }
answered=0
until ./quiet_gate.sh 45 >/dev/null 2>&1; do sleep 10; done
for i in $(seq 1 $N); do
  k=$(( (i - 1) % ${#asks} + 1 ))
  # The caller asks and is answered; the second voice's statement comes 10 s after the answer began.
  ./scenario.py $L-quiet-$i "Daniel|${asks[$k]}|10" "Samantha|${others[$k]}|8" > /dev/null 2>&1
  verdict=$(spoke_after_last "$(latest)")
  [[ $verdict == answered ]] && answered=$((answered + 1))
  printf 'trial %d: %-45s %s\n' $i "${others[$k]}" "$verdict"
  # Past the busy-room count and the conversation's 45 s, so the next trial starts in a quiet room.
  sleep 70
done
echo "second voice's lines answered: $answered of $N"
