#!/bin/zsh
# #829: two people at the lamp, the four cases of the issue, played with the first builder's scenario.py from this
# Mac's speakers (one place: what needs facing or two places needs people). Each scene opens with three lines between
# the two, so that the room counts as busy. PLAYS SOUND IN THE ROOM. About 4 minutes a round.
# argv: ROUNDS LABEL. Prints, per line, whether the lamp spoke before the next line, and what it said.
cd "${0:A:h}" || exit 9
. ./settings.sh
N=$1; L=$2
busy=("Daniel|Did you get my message about the budget?|0.7" "Samantha|Yes, I read it this morning. The numbers look high.|0.7" "Daniel|I know. We need to cut something before Friday.|1.5")
said() { LC_ALL=en_US.UTF-8 /usr/bin/awk -v w=$1 '/^#/{next} /PLAY/{ if (p != "") printf "%-*s | %s\n", w, p, said; p=substr($0, index($0,"PLAY")+6, w); said=""; next } /LAMP/{ s=$0; sub(/.*LAMP +/, "", s); said=said " " s } END { printf "%-*s | %s\n", w, p, said }' $2 | LC_ALL=en_US.UTF-8 cut -c1-200 }
latest() { ls -t "$PLAYED_OUT"/scenarios/*.txt | head -1 }
for i in $(seq 1 $N); do
  echo "== round $i, 1: A asks the lamp while B talks to A (expected: A answered, B's talk left alone)"
  ./scenario.py $L-829-asks-$i "${busy[@]}" \
    "Samantha|Maybe the trip to Hanoi, it is the biggest line.|0.4" \
    "Daniel|Hey Lamp, how much is one hundred dollars in euros?|8" \
    "Samantha|Anyway, if we drop the trip we save almost half.|0.7" \
    "Daniel|True, but the client wants to meet in person.|6" > /dev/null 2>&1
  said 60 $(latest)
  echo "== round $i, 2: the lamp named in passing (expected: nothing said)"
  ./scenario.py $L-829-passing-$i "${busy[@]}" \
    "Samantha|Did you see the lamp turn its head this morning?|0.7" \
    "Daniel|Yes, the lamp looked at me when I came in.|0.7" \
    "Samantha|I left the lamp on in the kitchen yesterday.|6" > /dev/null 2>&1
  said 60 $(latest)
  echo "== round $i, 3: A asks, B follows up, then they talk (expected: A answered; B's follow-up answered only when facing or named; their talk left alone)"
  ./scenario.py $L-829-follow-$i "${busy[@]}" \
    "Daniel|Hey Lamp, what is the weather like in Hanoi today?|9" \
    "Samantha|And what about tomorrow?|9" \
    "Samantha|I should bring an umbrella then.|0.7" \
    "Daniel|Yes, and we should leave a bit early.|0.7" \
    "Samantha|Okay, I will book the car for eight.|6" > /dev/null 2>&1
  said 60 $(latest)
  echo "== round $i, 4: B interrupts the lamp's answer to A (expected: recorded as it is today)"
  ./scenario.py $L-829-interrupt-$i "${busy[@]}" \
    "Daniel|Hey Lamp, tell me a short story about a clever fox.|3.5" \
    "Samantha|Daniel, are you coming to the meeting at two?|0.7" \
    "Daniel|Yes, give me five minutes.|8" > /dev/null 2>&1
  said 60 $(latest)
done
