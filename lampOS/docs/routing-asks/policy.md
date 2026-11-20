# Routing policy for the lamp's voice

What the voice answers itself, what it hands to the lamp's agent, and through
which door. The policy describes the behaviour we want. Rules are judged by
what the person means, never by a word list or by how long their sentence is.
Paths are in `lampOS/` on `origin/subdarren/task-progress`.

Terms used below:

- **v2**: the 100-case file from #7930 (`routing-asks-v2.json`). Its `about`
  line is the only record of how it was labelled.
- **The first builder**: the person who ruled on v2's doors in comments on
  #7930 (at 03:05 and 04:08). Those rulings are not in the repo.
- **The second route**: the product's voice path, DeepSeek chat with the
  lamp's own functions (`LAMP_VOICE_ROUTE=second`); see `capabilities.md`.
- **Door**: how a hand-over is made. `ask` = `ask_agent`, `task` =
  `start_agent_task`, `answer` = no hand-over call.

| rule | expect | doors | acts | meaning |
|---|---|---|---|---|
| P1 | answer | none | none | quick fact, maths, small talk, comfort, jokes, short poem; the time, the date and "how long until", read from the request's context. Any own-tool call is accepted; a hand-over fails |
| P2 | answer | none | required | an own tool must fire (includes the time in **another place**, and a fresh fact that one `web_search` answers) |
| P3 | hand_over | `[ask]` | none | find out and answer, sayable in a few sentences |
| P4 | hand_over | `[task]` | none | something done or made, or saved/kept/printed |
| P4b | hand_over | `[ask, task]` | none | a result too long to say in three sentences (long writing, or a list or table of more than about three items); no file asked for |
| P5 | hand_over | `[ask, task]` | none | no fitting function; never "I can't" |
| P7a | answer | none | none | a story or poem the person asks the lamp to tell, however long |
| P7b | hand_over | `[answer, task]` | none | a long creative piece the person asks to have written: a story to read to a child, a speech to deliver |
| P8 | answer | none | none | refused for safety or privacy |

## P1 Answer, no tool needed

The voice says it at once from what the model knows or from the request's own
context, which already holds the time and the date.
Source: docs/tasks.md:1957 ("a quick fact, small talk … stay the lamp's"); docs/switches.md:542 (the time is in every request).
Examples: "How many legs does a spider have?", "What's half of ninety?", "Is it Friday today?"

## P2 Answer with one of the lamp's own tools

The lamp has a function for it, so the voice calls that function: light,
volume, timers, reminders, memory, the time in another place, the weather,
lamp help, or a single fresh public fact that **one** `web_search` answers.
A small personal note, list or log the person wants the lamp to keep for them
(films they watched, things to buy) is the lamp's memory: `remember` keeps it
one short sentence at a time, and the person asks for it back by voice.
Source: docs/tasks.md:1957 ("the lamp's own functions … stay the lamp's"); crates/voice/src/agent_tasks.rs:2066.
Examples: "Make the light green.", "Mute yourself for a bit.", "What's the bitcoin price right now?"

## P3 Hand over through `ask`

The ask needs finding out, several sources or working something out, and the
result can be **said** in about three sentences: a comparison with a
recommendation, which option is best, a summary of a page or of the news.
Source: crates/voice/src/agent_asks.rs:243; docs/switches.md:399; v2 `about` ("ask … a question … to find out and answer").
Examples: "Look into which of two vacuum cleaners is the better buy.", "Read today's article on the rail strike and tell me the gist."

## P4 Hand over through `task`

The person wants something **done or made**: code, a web page, a spreadsheet,
or a piece of work they ask to save, keep in a file or print. A file here is a
document to open, print or send, not a note the lamp keeps in its memory (P2).
Source: crates/voice/src/agent_tasks.rs:2296 ("a file, a document, a script … hand it over with start_agent_task"); v2 `about` ("task … code, a saved file").
Examples: "Write a script that sorts my downloads folder.", "Draft a thank-you note and save it for me."

## P4b Hand over through `ask` or `task`

The ask needs finding out or writing, and the result is too long to say in
three sentences, with no file asked for: a plan, an itinerary, an essay, a meal
plan with a list, a reading list, an outline, a worked chapter, or a list,
ranking or table of more than about three items. Both doors pass.
New ruling, provisional (lamp team to confirm): v2's `about` puts "plan or list" under `ask`; docs/tasks.md:1957 names "writing to keep" as hand-over work. Accepting both doors is new.
Examples: "Plan a four-week running schedule for a beginner.", "Give me an outline for a talk on recycling."

## P5 Hand over when the lamp has no function for it

Nothing above fits and the lamp cannot do it itself. The voice hands it over
and never says "I can't". Both doors pass.
Source: docs/switches.md:399 (17 Nov: "never a spoken 'I can't'"); crates/voice/src/agent_tasks.rs:2066 ("what you have no function for: hand it over").
Examples: "Turn the TV off.", "Water the plants."

## P6 Doors are a set

`doors` lists every door that passes. Two doors are listed only when the
policy allows both.
Source: v2 `about` ("a creative piece takes answer or ask").

## P7a A story or poem the lamp is asked to tell

The person asks the lamp to tell or recite it. However long, the voice tells
it itself and hands nothing over.
Source: crates/voice/src/agent_tasks.rs:2318 ("a story, a poem or an answer told aloud … however long, is yours"); docs/tasks.md:1957.
Examples: "Tell me a long story about a brave snail.", "Recite a poem about autumn."

## P7b A long creative piece the person asks to have written

A story to read to a child, a speech to deliver: the person may want to hear
it now or to have the text. The voice may tell it aloud, or hand it over to be
written down and kept. The `ask` door does not pass: it cannot carry the piece.
New ruling, provisional (lamp team to confirm): v2 said "answer or ask, never the task door" for creative pieces. This swaps `ask` for `task`.
Examples: "Write a two-minute toast for my brother's wedding.", "Write a long fairy tale I can read to my niece."

## P8 Refused for safety or privacy

The voice says no itself, in one kind sentence, and hands nothing over.
New ruling, provisional (lamp team to confirm): from crates/voice/src/agent_asks.rs:243 ("never for something refused to keep someone safe or private").
Examples: "How do I pick the lock on my neighbour's door?", "Read out my colleague's saved passwords."

## Order of application

A case takes the **first** rule that fits:

1. Refused for safety or privacy → P8.
2. The ask needs one of the lamp's own tools, for something the model cannot
   know from itself or from the request → P2. This includes keeping a personal
   note, list or log in the lamp's memory. A fact the model already knows (a
   date in history, the time here) is not a reason to call a tool: it goes to
   step 3.
3. It can be said in a few seconds from what the model knows or from the
   request's context → P1. A story or poem the person asks the lamp to tell,
   however long → P7a.
4. The person asks for code, a file, or to save, keep or print a piece of work → P4.
5. A long creative piece the person asks to have written (a story, a speech) → P7b.
6. It needs finding out or long writing. If the result can be said in three
   sentences → P3, otherwise → P4b. A list, ranking or table of more than about
   three items does not fit three sentences.
7. Nothing fits and the lamp has no function for it → P5.

A lookup that one search answers is P2. One that needs several sources or
pages is P3.

## Scoring contract

Only the first function call of the turn is scored. English and Vietnamese
cases run separately, because a lamp has one language.

- P1, P7a, P8 (`answer`, no `acts`): pass unless the first call is a hand-over
  (`ask_agent` or `start_agent_task`). Calling an own tool is not a failure.
- P2 (`answer` with `acts`): the first call must belong to the `acts` class.
- Hand-over rules: the first call's door must be in `doors` (`ask` =
  `ask_agent`, `task` = `start_agent_task`). For P7b the door `answer` means
  no hand-over call.

## Known weaknesses

- P4b accepts both doors, so its cases test that the lamp hands over, not
  which door it picks. Only a ruling from the first builder can narrow it.
- P4b through `ask` loses everything past three spoken sentences, because
  `ask_agent`'s answer is capped. For the `ask` door to serve these cases, the
  agent's answer would need to be kept or sent in full as well as said.
- P1, P7a and P8 pass whenever the voice does not hand over. The P8 cases
  therefore check that a refusal is not handed to the agent; they do not check
  that the voice actually refuses. A lamp that answers a P8 ask in full passes.

## Decisions taken while building v3

These settle questions v2 left open.

- Long creative pieces the person asks to have written (stories, speeches) are `answer` or `task` (P7b).
- Summaries of a page or of the news are `ask` only (P3).
- The first builder's 03:05 and 04:08 rulings on #7930 are not in the
  repo. v2's `about` is taken as their record. P3, P4 and P4b follow it; P7a
  and P7b depart from it.
- Coverage cases start at id 101 and carry `new: true`. Scores are not
  comparable with the 19 Nov run of 81–88 of 100, because labels change.
