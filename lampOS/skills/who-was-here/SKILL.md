---
name: who-was-here
description: Call when someone asks whether anyone was at the lamp while they were away - "was anyone here while I was out?", "did someone come by?", "has anyone been at my desk?". The lamp answers from its record of the room over the last twelve hours, times only: when someone was there and for about how long. It never knows who. Only when asked.
---

# Who was here

Back at your desk, ask the lamp whether anyone came by and it tells you
what it noticed: "someone was here for a few minutes around two", or
"nobody came while you were away". It goes by what its eyes and ears said
of the room (`room/presence`): when there was someone at the lamp, a face
it saw or a voice it heard, and when there was nobody.

What it keeps, and what it never does:

- **Times only.** When someone was at the lamp and for how long, kept in
  the skills part's memory for the last day and lost when it restarts.
  Never a picture, never a face, never who anyone was: the presence signal
  carries none of that, and nothing here asks the eyes for more.
- **Seen or only heard.** A time when its eyes saw nobody and only a voice
  was heard is said as that: someone heard near the lamp, not seen.
- **It cannot tell who.** The time going on now is yours; a visit before it
  may have been you too, coming and going. It says what it noticed and
  never guesses a name.
- **It says when it could not see.** While the camera was closed (the
  privacy switch, a mute) the lamp does not know who came, and says so.
- **Only when asked.** It never tells you on its own who came by.

It needs the eyes to say `room/presence` (`LAMP_PRESENCE=on`); without it
the lamp says it can't tell.

An installable skill: its owner puts it on the lamp (`lamp-skills
install`) and takes it off. Not in the product yet.

If asked about it, the lamp says something like: "Ask me if anyone came by
while you were out, and I'll tell you when I noticed someone here. I only
keep the times, never who it was."
