#!/bin/zsh
# The played test (first the hourly "battery" on lamp-4ace, 16 Oct, the 80% day): two people (one round), the day, the hand-overs.
# Since 18 Oct 11:0x also unnamed follow-ups that need a tool (#1465). PLAYS SOUND IN THE ROOM. About 17 minutes. argv: LABEL. Writes a summary to $PLAYED_OUT/scenarios/battery-LABEL.txt
# (PLAYED_OUT defaults to ~/lamp-played). Needs LAMP_HOST, the lamp's address: see README.md.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=$1; O=$PLAYED_OUT/scenarios/battery-$L.txt
echo "battery $L $(date +%H:%M:%S)" > $O
./twopeople829.sh 1 bat-$L 2>&1 | grep -vE '^\s*$' >> $O
echo "-- day" >> $O
./day.sh bat-$L 2>&1 | grep -vE '^#PLAY' | awk '/PLAY|LAMP|turn finished: (no_audio|user_int)|lamp started/' | awk '/ LAMP /{sub(/.*LAMP +/,""); s=s" "$0; next} {if(s!=""){print "      LAMP:" substr(s,1,140); s=""} print}' >> $O
# How many of the day's lines got an answer: the number to compare hour by hour (12 of 12 at 09:09, 6 of 12 at 15:37).
/usr/bin/awk '/^-- day/{d=1; next} /^-- /{d=0} d && / PLAY /{n++; if (a) got++; a=0; next} d && /LAMP:/{a=1} END{if (a) got++; printf "day lines answered: %d of %d\n", got, n}' $O >> $O
echo "-- follow-ups (#1465)" >> $O
./followups.sh bat-$L 2>&1 | tail -8 >> $O
echo "-- hand-overs" >> $O
./handovers.sh bat-$L 2>&1 | tail -11 >> $O
echo "done $(date +%H:%M:%S)" >> $O
