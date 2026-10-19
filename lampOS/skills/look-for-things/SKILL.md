---
name: look-for-things
description: Call when someone asks the lamp to find something on or near the desk - "where are my keys", "can you find my glasses", "look for my phone". The lamp looks at the desk first, then a few places to either side, saying what it sees as it goes, and stays on the thing when it sees it. Give place = desk the first time; after each look, only if the thing is not yet seen, call once more with the next place: left, right, far_left, far_right. Only when asked.
---

# Look for things

Ask the lamp where your keys are and it looks for them, as you would ask a
friend at the desk to glance around: its head comes down over the desk and
its light brightens, it looks there first, then a little to the left, to the
right, and a little farther each way, five looks at most. It says what it
sees as it goes ("not on the desk... not here either"), and when it sees
them it says where, by something near them ("there, by the mug"), and stays
looking at them, its light on them. If they are nowhere it can see, it says
so kindly ("I can't see them from here") and stops. Talk to it while it
looks and it stops at once and listens.

What it can and cannot do:

- It finds things with the voice's look: the model is shown each picture
  and says what is in it. The lamp's eyes find faces only, so it cannot
  find a thing by itself, nor follow one.
- It keeps its head down over the desk the whole time and turns only its
  base, within the reach its gaze is given (`toward` in lamp-body's reading
  pose): it never lifts its head to sweep the room, and never looks a person
  over.
- It looks at most five times in one search, then lets it go; each look is
  asked for by the model after the one before, so a thing seen early ends
  the search early.
- It never starts on its own: only when someone asks.

An installable skill: its owner puts it on the lamp
(`lamp-skills install`) and takes it off. Not in the product yet.

If asked about it, the lamp says something like: "Ask me where your keys
or glasses are and I'll look around the desk for them. I can only see the
desk and a bit either side, though."
