---
name: music-sway
description: When music has been playing in the room for a while, the lamp sways its head a little from side to side, once, like someone enjoying the song; never while it is speaking or listening, never at night, and at most twice an hour.
---

# Music sway

When music has been playing in the room for about twenty seconds, the lamp
sways its head a little from side to side, about six seconds, as someone
enjoying the song would. Then it keeps still: once for a piece of music, and
again only after five minutes without music, and never more than twice an
hour, whatever plays.

It never sways while it is speaking, listening or thinking, nor with a hand
on it, and keeps still
at night (23:00 to 06:00, the lamp's local time). If the voice is in a turn
when the twenty seconds come (it sometimes takes music for speech), the sway
waits for the turn to end, up to a minute, and comes then only if the music
still plays. A short clip (a ringtone, a
video) is not music that goes on, and a quiet bar does not end it.

It never sways to music it plays itself (#2833). A recording someone asks
for (#1757) plays while the voice is listening, for "stop", so the
resting condition holds the sway back while it plays; and the voice must
have rested 2 s, since the ears' last window (about a second) still hears
the lamp's music once the voice rests. A window the ears mark as the lamp's
own voice (`own_voice` `present`, only while it speaks) holds it back too.
Room music that goes on after the lamp's own music still sways it, 2 s after
the voice rests: its twenty seconds were counted while the lamp's music
played, even if the room's music began later. The resting condition also
puts the sway that waited for a turn 2 s after the turn ends.

It hears music by the ears' room sound scores (`ears/classes`, with
`LAMP_EARS_CLASSES=on` in lamp-ears): a score from 0 to 1 the lamp works out
on the lamp itself, from the room microphone, keeping no sound. Without them
it does nothing.

If asked about it: "When there's music on, I sometimes sway along a little.
It's the music, not you; I'll stop if you'd rather."
