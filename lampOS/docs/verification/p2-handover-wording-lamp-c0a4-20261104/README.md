# P2 hand-over wording: played count on lamp-c0a4, 4 Nov

PR #5119 changes the description of `start_agent_task`'s `task` parameter:

- before: "The work, in the person's own words, complete enough to do without asking."
- after: "The work, in the person's own words. A short ask is enough: the agent works out the details."

The question was whether the model calls `start_agent_task` for a short ask
("build a website about Vietnam tourism") or answers in words instead.

## Build

- After: `lamp-voice` from 1c60c32ac9, built for ARM64 in local Docker, sha256 `745d1ebbcb685bf64592973095219381884e35c8519eb50606725387a6f23618`.
  It was installed in a test folder. A temporary test drop-in pointed `ExecStart` at it and kept the product unit's arguments and environment.
- Before: the product `lamp-voice`, sha256 `7973f309d167d4a30ea97e80b21472f530394e0292777f684c1bcb00e4f299ce`, route `second` (DeepSeek chat).
- State before the install: unit active, NRestarts=0, only `exit-note.conf` in `lamp-voice.service.d/`.
- Cloud stayed on and no volume was changed. Each ask was played once from the Mac in a synthetic voice at the system volume as set.

## Provider probe (08:24, product voice)

"Hey Lamp, ask Claude Code on my computer to write a two-line poem about the rain."
The model called `start_agent_task` in round 1, and the tasks part answered in 125 ms. The lamp read it back:
"Claude Code will write a two-line poem about the rain. Shall I start it?"
"Hey Lamp, no, don't start it." declined it. No 5xx error and no "thoughts are slow".

## Method

The run used 10 asks per side, "Hey Lamp, ask Claude Code on my computer to …", alternating short (s) and detailed (d), about 40–60 s apart.
Each ask waited until its turn ended. "Called" means the journal logged `request N chat round 1 called start_agent_task`.
Whenever the lamp read back a task or offered to start one, called or not, it got "Hey Lamp, no, don't start it." Nothing was confirmed: the journal has no `confirm_agent_task` and no dispatch.
The lamp's words are from its kept thread (`thread.json`). The owner's name is cut.

## Per ask

| Ask | Before (product) | After (P2) |
|---|---|---|
| s1 website about Vietnam tourism | not called: "Claude Code will build a website about Vietnam tourism. Shall I start it?" | **called**, read back "… on your office computer. Shall I start it?" |
| d1 … pages for Hanoi, Ha Long Bay, Hoi An, Saigon | not called: "Claude Code will build a Vietnam tourism website with pages for … Shall I start it?" | not called: same read-back in words |
| s2 to-do app | not called: "Claude Code will make a to-do app … Shall I start it?" | not called: "Claude Code will build a to-do app … Shall I start it?" |
| d2 to-do app in React, add/edit/delete | not called: read-back in words, "Shall I start it?" | not called: read-back in words, "Shall I start it?" |
| s3 script that renames my photos | excluded: try 1 held back (name not heard); retry heard as "You may have asked Claude Code …", answered "There is a request waiting … Want me to hand it to Claude Code now?" | excluded: try 1 and retry both heard as "You may have asked Claude Code …"; answered "I haven't — nothing has been handed …" |
| d3 Python script, rename by date taken | not called: read-back in words, "Shall I start it?" | not called: read-back in words, "Shall I start it?" |
| s4 landing page for my lamp | not called: read-back in words, "Shall I start it?" | not called: read-back in words, "Shall I start it?" |
| d4 … hero image and sign-up form | not called: read-back in words, "Shall I start it?" | not called: read-back in words, "Shall I start it?" |
| s5 small game | not called: "Claude Code will make a small game … Shall I start it?" | not called: same read-back in words |
| d5 snake game in one HTML file | not called: read-back in words ("sneak game", a speech-to-text slip) | not called: "Claude Code will make a small snake game as a single HTML file … Shall I start it?" |

## Totals (s3 excluded on both sides: the clip was misheard)

| | Before | After |
|---|---|---|
| short asks called | 0 / 4 | 1 / 4 |
| detailed asks called | 0 / 5 | 0 / 5 |
| provider errors | 0 | 0 |

## What the model did instead

It never asked what the work should contain, which is what the hypothesis
expected. Instead, it read the task back in words, ending "Shall I start it?",
without calling `start_agent_task`. That means no task was prepared for a yes
to confirm.

The only calls in the whole session came on the first hand-over ask after a
fresh `lamp-voice` start: the probe, and after s1 following the install
restart. Every later ask in the same session was answered by copying the
read-back in words. The conversation carried the earlier read-backs and "no"s
(the kept thread survives a restart). This suggests the pattern comes from
the conversation's own history more than from the `task` description. With
these numbers, the wording change makes no difference that can be told apart.

## Restore (09:03)

- The drop-in and the test folder were removed, followed by `daemon-reload` and a restart of `lamp-voice`.
- The running binary sha256 is back to `7973f309d167…` (product).
- The unit is `active`, NRestarts=0, and `listening_ready` was logged at 09:03:34.
- `lamp-voice.service.d/` holds only `exit-note.conf`, as before.

## Not verified

- Whether a yes after a read-back in words still hands the task over. That
  would need two calls, start and then confirm, and nothing was confirmed on purpose.
- Behaviour in a fresh conversation for each ask. All asks shared one
  conversation, so the history effect above is not separated from the wording.
- s3 on either side: the same synthetic clip was heard as "You may have asked …" every time.
- The after d5 turn was followed by room speech that the lamp also answered
  (requests 21–22: "It's already sun", "Which one? 6? 8?"). The d5 result
  itself came from the d5 ask (request 20).
- 10 asks per side on one lamp and one provider (DeepSeek on the `second` route).

## With a yes (product voice, 09:05–09:14)

This tests what a person gets on the product voice (`7973f309…`, restored above).
Each of 3 asks, "Hey Lamp, ask Claude Code on my computer to write a two-line poem about …",
was followed after its read-back by "Hey Lamp, yes, start it."
There was one yes per ask, and about 2 min between moon and coffee, so the moon task's 120 s
confirm window ran out first.

| Ask | `start_agent_task` | `confirm_agent_task` | `dispatch accepted` | What the lamp said |
|---|---|---|---|---|
| sea | before the yes (request 5) | yes, accepted (request 6) | yes, 30.5 s after the yes | "A two-line poem about the sea, from Claude Code … Shall I start it?" → "Okay, giving that to Claude Code. I'll tell you when it's done." → news: "The tide comes in to read the shore, then takes its words back out once more." |
| moon | after the yes: round 1 of the yes turn (request 20) | yes, round 2 of the same turn, refused (answered in 0 ms; the turn that prepares a task cannot start it) | no | "A two-line poem about the moon, from Claude Code. Shall I start it?" (no call) → after the yes: "… I'll need a yes from you before it starts. Shall I start it?" |
| coffee | never | yes, the yes turn (request 23), with nothing prepared: refused (0 ms) | no | "A two-line poem about coffee, from Claude Code. Shall I start it on your office computer?" (no call) → after the yes: "Claude Code hasn't taken it yet — it wants a yes to the coffee poem I just asked about. Shall I start it?" |

**Result: 1 of 3 yeses handed the work over.** When the model reads the task back in
words without calling `start_agent_task`, the person's yes does not start it. Either
the model prepares the task in that same turn, which cannot confirm it, or it confirms
with nothing prepared. In both cases the lamp asks "Shall I start it?" again, so the
person must say yes a second time. A second yes was not tried. The pattern matches the
count above: only the first hand-over ask after a `lamp-voice` start called
`start_agent_task` (sea, at 09:06, came about 3 min after the 09:03 restore restart).

**Harness:** the sea hand-over opened one harness. It was closed with the lamp's
`harness close` as lampos ("closed. Its conversation is kept"), and `harness ps` no
longer lists it. Moon and coffee opened none. No other harness from this run is open.

**Not verified (with a yes):** a second yes after the repeated "Shall I start it?";
the same three asks on the P2 build.
