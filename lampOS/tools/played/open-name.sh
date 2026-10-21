#!/bin/zsh
# A name said after a pause (21 Oct 03:1x, #2355 LAMP_SECOND_OPEN_NAME): "Hey Lamp, my name is" then 0.4, 0.6 or 0.8 s of quiet,
# then "<Name>. Please remember it.", then "Lamp, what's my name?". At 21:25 on 20 Oct the name after the pause was refused as an
# interruption and the lamp answered "my name is" alone. Prints, per round, whether the lamp's first answer and its answer to
# "what's my name?" have the name; then asks it to forget each name (the room's memory). PLAYS SOUND IN THE ROOM. About 3 minutes.
# argv: LABEL. Made-up names only.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=$1; n=0; got1=0; got2=0
latest() { ls -t "$PLAYED_OUT"/scenarios/*.txt | head -1 }
for pair in "Ossa 0.4" "Wrenna 0.6" "Bramble 0.8"; do
  N=${pair% *}; P=${pair#* }; n=$((n+1))
  ./scenario.py $L-open-$n "Daniel|Hey Lamp, my name is|$P" "Daniel|$N. Please remember it.|12" "Daniel|Lamp, what's my name?|10" > /dev/null 2>&1
  F=$(latest)
  a1=$(awk '/ PLAY /{p++} p==2 && / LAMP /' $F | grep -ci "$N"); a2=$(awk '/ PLAY /{p++} p==3 && / LAMP /' $F | grep -ci "$N")
  [ $a1 -gt 0 ] && got1=$((got1+1)); [ $a2 -gt 0 ] && got2=$((got2+1))
  echo "round $n, $P s before \"$N\": first answer has the name: $([ $a1 -gt 0 ] && echo yes || echo no); \"what's my name?\" has it: $([ $a2 -gt 0 ] && echo yes || echo no)"
  grep -E " PLAY | LAMP " $F | cut -c1-140
  sleep 20
done
echo "open-name $L: the name in the first answer $got1 of $n, asked back $got2 of $n"
for N in Ossa Wrenna Bramble; do ./scenario.py $L-forget-$N "Daniel|Hey Lamp, please forget the name $N.|10" > /dev/null 2>&1; done
echo "asked to forget the three names"
