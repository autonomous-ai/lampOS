---
name: sun-times
description: When I say sunset time, it looks up today's sunset here on the web and says it.
when: When the person asks when the sun sets today.
made: here
---

# Sun times

Said "sunset time", the lamp fetches today's sunset for where it is from a
public web service and says it in one sentence.

## Uses

- scripts/main.py, no arguments: a public web service through the run's proxy, then `lamp.say`.

## Result

Nothing is kept or printed: the time is said.

## Answer

The script says "The sun sets at 17:42 today." with lamp.say, or, when the web can't be reached, "I couldn't reach the sunset time just now."
