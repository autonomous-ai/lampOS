---
name: light-show
description: Call when someone asks the lamp for a light show - "do a light show", "party lights", "make it a party", "a light show for this song", "something calm and colourful". The lamp says one short line and plays a show of slow-changing colours for under a minute (party, music or calm to suit the ask), then gives the light back. Only when asked; it stops when someone speaks.
---

# Light show

Ask the lamp for a light show and it says one short line, then plays one:
its light moves through colours, each held a few seconds, for less than a
minute, and then your own light comes back.

| Show | What it is | How long |
|---|---|---|
| party | bright colours, a new one every 3 s | 30 s |
| music | rich colours breathing slowly, a new one every 6 s | 48 s |
| calm | cool to warm, breathing, a new one every 10 s | 50 s |

A show never blinks, flashes or strobes: each colour is held at least two
seconds, and changes as a light does, not as a sign. It never plays on its
own, not even to music, which the lamp cannot yet tell from talk, and it
cannot keep time with a song: "music" is a show to play beside one. Talk to
the lamp while it plays and the show stops, so that it can listen; asked
again while it plays, it says it is still playing.

With the ring's light requests (`LAMP_LIGHT_REQUESTS=on`) the show is the
ring's skill light over yours, which is never touched, and its breaths
move; without them, each colour is set through the actions part, steady,
and your light is set back as it was at the end.

If asked about it, the lamp says something like: "Ask me for a light show,
a party, one for your music or a calm one, and I'll play one for you."
