---
name: welcome-back
description: When someone comes back to the desk after at least five minutes away, the lamp turns to them, warms its light and says a short hello, by name if it knows them.
---

# Welcome back

The lamp's hello on arrival, as a physical skill (`lampOS/docs/skills.md`).
It is as small as what a person at the next desk does when a colleague
comes back: look up, a word or two, and nothing more. It does not greet
people walking past or speak into a conversation.

- **Trigger:** a face on `humans/faces/tracked` where there was none, after
  at least five minutes with nobody (counted from when they left: a face
  glimpsed for a moment as they sit down does not restart it, #2825); at
  most once every twenty minutes, and once an hour to each person (the
  Policy's hello gate, asked even with `LAMP_SKILLS_ASK=skip`; the first
  builder, 30 Oct, on the launch testers' findings).
- **After a rested night:** with Bedtime and Morning on, the first face of
  the morning waits for its morning greeting, which is the welcome back;
  if that chance is dropped, the hello comes as usual (`yields_to`, #1672).
- **Ask:** the Policy's `hello` question, before anything is done, even
  with `LAMP_SKILLS_ASK=skip`: its hard rules hold every hello (never over
  talk, twenty minutes between any two, an hour between two to the same
  person, by the memory's id of a known face; every face not known shares
  one hour). No answer within its wait, no hello.
- **Do:** look at the face, warm the light (`lights/warm_glow.toml`), and
  give the voice a note to say a hello and the name only, if it knows it,
  in one short line: nothing else. With
  `who = true` in its permissions (no other skill has it), the voice tells
  the note a known face's name, and to be one beat warmer to someone who
  comes by often; the typed name is in its instruction.
- **The hello's life:** a minute, as the chance's `wait` (`expires =
  "60s"`). A room line the lamp answers as the hello is asked, and the
  voice's 15 s quiet after that turn, take more than a say's default 30 s,
  so it was spent before it began (#2861). Not begun in the minute, it is
  spent: one arrival never gets a late or a second hello.
- **The only hello:** the voice has none of its own (#2825), so one arrival
  gets one hello.

Asked about it, the lamp says it greets people when they come back to
their desk, and that it can be switched off.
