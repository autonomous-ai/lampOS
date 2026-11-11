# Replaying kit 5194's judge over real lamp journals

This is V2's replay rule for #5194: the judge is replayed over at least 3 real lamp captures, at least one of them noisy, and compared line by line with a person's verdict, before the kit gets a yes.

**The judge** is `door()` in `mac/analyze.py`. `mac/replay.py` imports it rather than copying it, so the replay judges exactly as the kit does.

**A capture** is a real second-route conversation on a lamp with `LAMP_OPENS_FURTHER` unset (the product default). No switch is changed and nothing is installed. On the lamp:

    mkdir -p ~/cap1 && cd ~/cap1
    lamp-bus tap voice/said  > said.txt  &
    lamp-bus tap voice/heard > heard.txt &
    # ... a natural conversation of about 10 turns ...
    kill %1 %2; echo quiet > room.txt     # "busy" for the noisy one

Take three captures: two in a quiet room, and one busy (other people talking, or `clips/talk.aiff` playing beside the lamp). Copy them to the Mac.

**Run** it over all three, then fill each `replay.tsv` file's `person` column with `y` (an opening) or `n` (none), line by line, and run it again for the agreement count:

    python3 -I mac/replay.py cap1 cap2 cap3

**What it shows:** the kit's doors next to the person's for each reply, and `agree` yes/NO for each line. With the switch off, every door the kit counts is either the model's own habit (which the person also reads as an opening) or a false count, and the NO lines say which.
