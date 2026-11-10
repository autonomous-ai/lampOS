---
name: morning-mail
description: Once a morning, after you have been at the desk 20 minutes between 7 and 11, the lamp tells you who your five newest mails are from and their subjects, never their text. Needs Gmail linked in the app and your face known to the lamp.
---

# Morning mail

Once a morning, when you have been at your desk for 20 minutes between 7:00
and 11:00, the lamp tells you about your newest mail: how many, then who
each of the five newest is from and its subject ("You have 5 recent mails.
From Sam: Lunch on Friday. ..."). It needs Gmail linked in the app, under
Skills, then Connectors.

When it reads:

- The morning window determines when the read becomes due, not when it
  must finish speaking. It can wait beyond that window within the same
  one-hour wait from when it became due; the gates below still apply.
- Once per local day while lamp-skills keeps running. Restarting lamp-skills
  clears that daily bookkeeping, so it may become due again that day.
- It waits 20 minutes, so it doesn't come straight after Morning agenda.
- Only when you are alone with the lamp, it knows your face as its
  owner's, and nobody has spoken for a minute.
  If that doesn't happen within an hour, it lets the day go and says
  nothing.
- Never in the middle of a conversation, while the lamp is private,
  paused or muted, or at night.
- Never while another skill reads to you, or in the minute after.
- Someone coming into view stops it: "I'll tell you the rest later."

It reads only senders and subjects, never a message's text. Security mail
and one-time codes are left out: they are neither said nor counted. It
keeps nothing of what it read; the journal keeps only counts.

The lamp uses fixed words, without sending the read to a language model.
Its configured speech provider may still receive the exact spoken text
including senders and subjects to turn it into audio.

An installable skill: its owner puts it on from the Skills Store and takes
it off. Not in the product.

If asked about it, the lamp says something like: "In the morning, once
you've been at your desk a while, I'll tell you who your newest mail is
from."
