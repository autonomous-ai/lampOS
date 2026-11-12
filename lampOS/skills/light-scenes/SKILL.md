---
name: light-scenes
description: When asked for a light by its use ("reading light", "movie light", "energise me"); it stays.
---

# Light scenes

Ask the lamp for a light by what you are doing and it sets one for it:
the colour and how bright, together, and for reading and focus its head
comes down over the desk too. The light is yours, as if you had asked for
the colour and the level yourself: it stays as it is set until you change
it, and the lamp never changes it on its own.

| Scene | Light | Head |
|---|---|---|
| energise | white, brightest (5) | as it is |
| reading | white, 4 | down over the desk for a minute |
| focus | white, 3 | down over the desk for a minute |
| relax | warm, 2 | as it is |
| movie | warm, lowest (1) | as it is |
| night | orange, lowest (1) | as it is |

The six are V1's (`hal/presets.py`): their order of brightness, from
energise down to night, and their warmth, mapped onto this lamp's five
levels and its three warmths (white, the amber warm, orange). A scene never
touches the microphone, the camera or the speaker: in V1 a scene with the
microphone "on" unmuted one the person had muted (`docs/v1-verdicts.md`,
HAL-4.20).

Asked for another scene while the head is still down, the lamp sets the
new light at once; a scene without the pose lets the head come back up.

If asked about it, the lamp says something like: "Ask me for a reading,
focus, relaxing, movie, night or energising light and I'll set it for you.
It stays until you change it."
