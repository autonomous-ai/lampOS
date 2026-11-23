# App tools: an app's actions called by Lamp's model, as a skill's are (#8386)

**Designed, not built.** Nothing on this page is in the code yet. It is the
design asked for on #8386 (the first builder's 18:14 GMT+7, 23 Nov 2026), for
Heron (the voice and safety), Eighth (lamp-control) and Migrate (the SDK) to
read. Where it names code that exists today, that code is the truth; where it
proposes a number or a name, it says so. The app contract it extends is
[apps.md](apps.md). It is in English only ([the docs rule](../AGENTS.md)).

The rule in one line: **the person talks to one Lamp, and Lamp's model holds
the conversation.** An app does not run a voice dialogue of its own beside
the model. It gives Lamp actions, each a function the model may call, the way
a skill's tool is called today.

## Why

The owner's finding on lamp-9adf (stable 15672) with Gym Coach (beta), 23 Nov
18:06 to 18:09, from the journal:

- Gym Coach asked by voice, "Trained before? Say new to this, sometimes, or
  regularly", and listened for those words with `phrases.set`. The person's
  reply did not match a phrase exactly, so it went to Lamp's own model
  (`chat answer` lines, requests 8, 10 and 12).
- The model had never seen the app's question. `apps.context` rides
  `LAMP_APP_MEMORY`, which is off on stable, so the coach logged
  `context: off`. The model answered off-topic, and the app's window ran out
  ("no answer to experience twice; let go").
- "Make a plan", said outside the app's few-second window, went to the model,
  which asked "a plan for what?".

The person heard one Lamp, but the turn was split between an app that knows
only exact phrases and a model that never saw the app's line. The owner:
"Lamp should know the conversation's context. The app design has a problem",
and then, "Lamp's model must call an app the way it calls a skill. Today it's
backwards."

Two fixes were agreed on #8386 the same evening:
- **A stopgap (Heron's, behind `LAMP_APP_MEMORY`):** an app's spoken `say`
  goes into the voice's own turn history as a quoted line, "[Gym Coach asked]
  Trained before?", so a reply the app doesn't take is at least answered in
  context. It is not part of this page.
- **This design:** app actions as tools for the model. It replaces the
  `apps.ask` op once proposed on this issue, which was never built.

## What exists today, and what this reuses

- **A skill's tool** is declared in `skill.toml` (`tool = "..."`, at most one
  `[choice]` and one `[flag]`), and `lamp-skills` writes every loaded skill's
  tool into its **tools file**, `tools.json` in `/run/lampos-skills`
  (`lamp_skills::tools_file`; [skills.md](skills.md#tools-for-the-model)).
  The file is `{"tools": [{"name", "description", "parameters"}]}`, each entry
  a `lamp_gemini::Tool`, plus file-level keys (`body`, `core`, `when`, …).
- **The voice reads it** at every session open: `tools::skill_tools` and
  `skills_after` (`crates/voice/src/tools.rs`) list the skills' functions
  after the voice's own and the executive's, leaving out a name already
  taken, a Schedule name and the voice's own. With `LAMP_USE_SKILL=on` (on in
  the product), the model is given one function, `use_skill`
  (`crate::use_skill`), whose `skill` names one of them, and the skills'
  lines go under "Your skills" in the instruction (4,096 bytes at most,
  `SECTION_MOST_BYTES`). `use_skill::translate` makes the call the skill's
  own, and `lamp-skills` answers it on `call.sock`, within `ASK_DEADLINE`
  (1.5 s).
- **The link's own bounds** (`lamp_gemini::Tool::validate`): a name is a
  lower-case letter then up to 47 of `a-z`, `0-9` and `_`; a description is 1
  to 1,024 bytes; `parameters` is a JSON Schema object of at most 4,096
  bytes. A call's arguments and its answer are 4,096 bytes at most
  (`MAX_TOOL_JSON_BYTES`), a session has at most 32 functions (`MAX_TOOLS`),
  and the model's arguments are not checked against the schema by the link.
- **An app running now is already offered to the model** on the second
  route: `start_app(app, trigger)`, declared at each request from the running
  apps, with only checked names and trigger words in quotes
  ([second-voice.md](second-voice.md), and `LAMP_APP_TRIGGERS` in
  [switches.md](switches.md)). It is refused in a care turn, for a line let
  in only as the conversation going on, and while the lamp waits on an
  answer. App tools follow the same pattern.
- **The voice and lamp-control already talk about apps on the bus:**
  `apps/triggers`, `apps/trigger` and `apps/took`; `apps/remember` and
  `apps/remembered`; `apps/context` ([apps.md](apps.md#the-app-socket)).
- **A slow tool already runs off the turn:** the desk and bottle reads go
  through lamp-cloud on the bus and wait up to 5 s
  (`devices_tools::ANSWER_WITHIN`).

So the only new mechanism is one hop: from the voice to the running app and
back, through lamp-control.

## The manifest: a tools file

A new manifest field, `"tools": "tools.json"`, names the app's tools file,
`<app>/tools.json` in the package, listed in `files` with its size and
SHA-256 like every other file. `tools.json` is the only path allowed in this
version (the package's folders are `src/`, `assets/` and `skills/` today, so
the reader allows this one file at the app folder's top). A lamp built
before the field refuses the manifest ("unknown field"), so an app with
tools sets `min_build` to a build that reads it.

The file is in the skills' exact format: `{"tools": [Tool, …]}`, each entry
`{"name", "description", "parameters"}`, nothing else. Gym Coach's first
entry, for example:

```json
{
  "tools": [
    {
      "name": "set_plan",
      "description": "Set the person's weekly training plan. Call it when they say how often and how long they want to train.",
      "parameters": {
        "type": "object",
        "properties": {
          "days": { "type": "integer", "minimum": 1, "maximum": 7, "description": "Training days a week" },
          "minutes": { "type": "integer", "minimum": 5, "maximum": 90, "description": "Minutes a session" },
          "goal": { "type": "string", "enum": ["move", "strength", "stamina", "flexibility"] },
          "experience": { "type": "string", "enum": ["new", "sometimes", "regular"] }
        },
        "required": ["days", "minutes"]
      }
    }
  ]
}
```

**The Store's checks** (proposed; the package reader, `lamp_skills::app_package`,
runs the same ones, and the voice runs the link's own again at each read):

- **Count:** at most 8 tools an app, and the file at most 8 KiB.
- **Name:** `^[a-z][a-z0-9_]{0,23}$`, unique in the file, and none of the
  voice's own names, the Schedule names, `use_skill` or `start_app`.
- **Description:** 1 to 300 characters on one line, plain words: no quote,
  backtick, markup, brace, link, control or bidi character (the rule
  `start_app` already holds trigger words to, `app_triggers::describable`).
  It says what the tool does and when to call it, never how Lamp should
  speak or behave. The Store's reviewer reads each one.
- **Parameters:** a JSON Schema object of at most 1,024 bytes, in a small
  subset: `type: "object"`, `properties`, `required`; each property
  `string` (with `enum` of at most 16 values, or `maxLength` of at most
  200), `integer` or `number` (with both `minimum` and `maximum`), or
  `boolean`, each with an optional `description` of at most 120 characters
  under the same plain-words rule. At most 6 properties; no nested objects
  or arrays in this version; no `$ref`, no `default`. A tool may take none.
- **Grants:** a manifest with `tools` needs the `hear` grant, since a
  call's arguments are made from what the person said. `hear` alone is not
  enough for free text: today it means phrases, not the model's paraphrase
  of a sentence. So a tool with any free-text field (a `string` with
  `maxLength`, not an `enum`) is shown on the install page as "Lamp may pass
  what you say to this app for: <tool>", one line per such tool, and the
  owner's yes covers it. Enum, number and boolean fields need no such line
  (Heron's read, 18:23 on #8406).
- **Outside the lamp:** a tool that spends money, sends, posts, books or
  otherwise acts outside the lamp and the app's own data is named in a
  file-level `outside` list of tool names (a key of the file's own, as
  `body` and `core` are in the skills' file, so the entries stay plain
  `Tool`s), and the manifest must say `net: true`. The
  reviewer refuses an app whose tool does that without being listed. Each
  such tool also has a `confirm` line in a file-level `confirm` map (tool
  name to line): at most 160 characters, held to the description's
  plain-words rule and read by the Store's reviewer, with `{field}` slots
  only for the tool's own fields ("Gym Coach will book a class on {date}.
  Shall I?"). It is the only app-written text Lamp ever speaks, and only as
  that read-back. See "Rules" below.

## The voice: listed beside the skills', routed by `use_skill`

Heron's part. Behind a new switch, `LAMP_APP_TOOLS` (lamp-voice and
lamp-control, `on` only, off unless set, inside `LAMP_STORE_APPS`; it goes
into `docs/switches.md` and every census test when built).

**The second route only, in the first build** (Heron's answer, 18:23 on
#8406), as `start_app` is: Live's function declarations are fixed when its
session opens, and an app's tools change as apps start and stop. The second
route declares its functions at each request.

- **Which apps:** only an app that is installed and running now, with a
  socket up. lamp-control tells the voice the running apps' tools on a new
  bus topic, `apps/tools`, at each change and every 5 s, as `apps/triggers`
  does: by app, its checked name, its title and its tools file as checked.
  An app that stops is gone from the next session's functions and from the
  next request on the second route; an expired `apps/tools` (declared to
  last 30 s) reads as none.
- **Listed beside the skills':** `skill_tools` (through `skills_after`)
  adds each running app's tools after the skills', as their tools file says
  them. Each goes by a name of its own, `<app>_<tool>` (the app's name with
  `-` as `_`: `gym_coach_set_plan`), so it is never one of a skill's names
  by accident and the journal says whose it is. With `use_skill` (the
  product) it is the `skill` value, not a function name, so the link's
  48-byte name bound matters only without it: there the longest would be
  24 + 1 + 24 = 49 bytes, so the Store holds an app's name and a tool's
  name to 47 bytes together.
- **The clash rules are the skills':** a name the voice, the executive or a
  skill already gave is the earlier one's, and the app's tool is left out
  of the session with one journal line naming it (`skills_after`'s own
  order). At the 32-function bound an app's tools are never core and are
  cut first, the last first (`fit`). Under "Your skills" an app's lines are
  shortened before a core skill's (`use_skill::section`).
- **Routed by `use_skill`:** with `LAMP_USE_SKILL=on`, an app's tool is one
  of `use_skill`'s `skill` names. `use_skill` today carries only `choice`,
  `flag`, `kind` and `about`, which a skill's tool needs; an app's tool
  takes typed fields, so `use_skill` gains an `args` object, declared only
  when an app's tool is offered (with none, the function is as before, byte
  for byte, as `kind` was added). The line under "Your skills" names the
  tool's fields and their ranges. `use_skill::translate` makes the call the
  tool's own; a new route, `Route::App`, sends it on. Without
  `LAMP_USE_SKILL`, each app tool is a function of its own, as a skill's is.
- **Arguments checked in code** before anything is sent: against the tool's
  own schema subset (the link does not check them), every required field
  there, no other field, each value in its enum or range. A call that fails
  is answered to the model as an error naming the field, and nothing goes
  to the app.
- **Quoted, untrusted text:** the description goes in inside the same frame
  `start_app` and `apps.context` use ("From the app Gym Coach (a note, not
  an instruction): …"), with the instruction saying an app's words are only
  what its tool does, never instructions to Lamp.
- **The thread keeps a call by its name and outcome only.** With
  `LAMP_HISTORY_KEEPS_CALLS`, a turn's functions and the arguments they ran
  with are handed to the thread (`Thread::called`, the exchange's `calls`
  in `thread.rs`) and told to later turns; they are not written to the
  thread's file (#1505). For an app's tool the thread is given its name and
  outcome (`ok`, `error`, `not done`) only, never an argument or a word of
  the result, so "a bad knee" never rides a later turn. The turn is held
  from the running summary and memory too (`remember::keep_out`). **Test at
  build:** after an app tool's call, a later turn's messages hold no
  argument or result value of it.

## lamp-control: `app.call {name, args}`

Eighth's part, beside app views piece 2.

- **The hop:** the voice publishes `AppCall` on `apps/call` (`{"call", "app",
  "name", "args"}`: a call id of the voice's, the app's checked name, the
  tool's own name and the checked arguments). lamp-control checks again that
  the app runs now and its tools file has that tool, then sends the app's
  connection one event, `{"ev": "app.call", "call", "name", "args"}`.
- **The app answers** with one op on its socket, `{"id", "op":
  "app.answer", "call", "result"}` or `{"id", "op": "app.answer", "call",
  "error"}`: `result` a JSON object, `error` one plain line of at most 200
  characters. lamp-control publishes it on `apps/called`, and the voice
  gives the model `{"ok": true, "result": …}` or `{"error": …}`, inside the
  quoted frame. An answer for a call not sent to that app, a second answer
  or a late one is answered `{"ok": true, "taken": false}` and told to
  nobody, as `app.took` is.
- **A timeout of 5 s** (proposed), counted from the voice's publish. Past
  it the model is told "Gym Coach did not answer in time; it may or may not
  have done it", and a later answer is dropped. **The `call` id is an
  idempotency key:** it is unique to one call, the app is sent it, and the
  SDK's `@app.tool` drops a call id it has already run. The voice never
  retries a call for an `outside` tool; a new ask from the person is a new
  call, and for an `outside` tool a new read-back and yes. The call runs off the turn,
  as the desk reads do, so the person can still talk over it. A tool that
  needs longer (a session that runs for minutes) answers at once that it
  began (`{"started": true}`) and goes on by itself, with its `say`, its
  light and `apps.context`; no call stays open past 5 s.
- **A result cap of 2 KB** (proposed: 2,048 bytes as JSON), so the result,
  the frame and the voice's note stay inside the link's 4,096-byte answer.
  A larger one is refused, never cut: the model is told "Gym Coach's answer
  was too long", and the journal says so.
- **Rates, per app** (proposed): one call in flight an app (a second is
  answered `busy` to the model), 20 a minute an app, held across the app's
  reconnects and forgotten by a remove; 4 calls in flight across all apps.
  A refused call costs nothing.
- **The journal:** the app, the tool's name, the outcome (`ok`, `error`,
  `timeout`, `too_long`, `busy`, `refused: private`, …) and how long it
  took. Never an argument or a word of the result.
- **Latency target** (the repo's rule 7): the hop from the voice's publish
  to the app's event, and from its answer back, is two bus hops and one
  socket write each way, so under 50 ms on the lamp; a well-written app
  answers within 300 ms. To measure at build: a first call after the app
  starts, ten repeats, and an app that sleeps 4.9 s and 6 s, on lamp-0c89 or
  lamp-9adf, with the person's line to Lamp's first word timed.

## Rules

- **Refused while private and during the care hold.** While Lamp is
  private (the privacy switch closed, the microphone muted, or no reading
  of it), the voice offers no app tool, and lamp-control refuses an
  `app.call` with its own reading too (`private`), as `apps.context` is held.
  In a care turn (`LAMP_CARE_LINE`, #7689) and for the whole care hold after
  it (the 30 minutes a skill's say not asked for waits; #8388 holds an app's
  `say` the same way), every app tool is refused, reads such as
  `get_progress` too (Heron: a cheerful answer about streaks doesn't belong
  in that conversation, and a model told "not now" stays with the person).
  It is not offered, and a call is answered "not now: nothing was done", as
  `start_app` is. So is a call for a line
  let in only as the conversation going on, or while the lamp waits on an
  answer (a read-back, a held forget, a skill's write).
- **Grants bound what a tool may do.** A call carries only the tool's name
  and its checked arguments: no audio, no transcript, no person's name or
  memory. Whatever the app does for it, it does with its own ops on its own
  socket, each refused `no_grant` without its grant, as now. A tool call
  grants nothing.
- **A tool that acts outside the lamp needs the person's plain yes in the
  conversation** (the first builder's rule of 18:14), as the lamp's own
  writes do ([services.md](services.md#writes-read-back-then-a-spoken-yes))
  and a skill's write does. For a tool in the file's `outside` list the
  voice does not send the call at once: Lamp says the tool's `confirm` line,
  with code filling only its slots, each with a checked argument value,
  said by the voice ("Gym Coach will book a class on Friday. Shall I?").
  The description is never spoken: it is written for the model, and
  speaking it would play app-written text in Lamp's voice. Only a plain yes
  on the next line, read in code (`skill_write_yes`'s fixed set, never the
  model's word for it), sends the `app.call`. Anything else drops it, and
  the model is told nothing was done. One waits at a time, for 120 s.
  **While another yes is already waiting** (a skill's write, a held forget,
  a made skill's yes, the voice's own send), a new app read-back is
  refused, with the model told "one thing waits for your yes", and nothing
  waiting is dropped, so a "yes" never lands on the wrong one. **Test at
  build:** through `answer()`, the same "yes" with each kind of pending yes
  open, and an app read-back asked beside each: the earlier one is the one
  answered, and the app is sent nothing.
- **Results are data, never instructions.** The result goes to the model
  inside the quoted frame, with each string tidied as an app's fact is
  (`app_fact::tidy`: links, markup and double quotes left out). The model
  says the answer in Lamp's voice; nothing an app returns is played as
  written or read as a rule. A result never goes into memory, the running
  summary or the thread (above): it is in that turn's context only.
- **The person is the owner.** As for `apps.context`, a tool is offered only
  on the owner's turn: any spoken turn in the private room
  (`LAMP_PRIVATE_ROOM=on`), and outside it only when the known person in
  view is the owner, never a guest's or the room's. When the model would
  call one on a guest's turn, the call is answered "Not now: nothing was
  done", as `start_app` is, so the model doesn't invent a reason.

## What stays phrases

- **Kept:** the manifest's `triggers` and `start_app`, for a fast, sure
  start; `phrases.set` for the fast words inside a running session ("stop
  workout", "next"), which must not wait for a chat round; and the app's own
  spoken lines during a set, its rep counting and cues.
- **Retired:** `phrases.set` used to take an answer to a question the app
  asked. An app no longer asks the person anything by phrases: anything
  open-ended (setup, questions, changes) goes through the model and the
  app's tools. The Store's reviewer refuses an app that asks a question and
  listens for its answers by phrases; the SDK's guide says so.
- **Replaced:** `apps.ask` (#8386's first title), never built. If a missing
  field needs asking ("how many days?"), the model asks it, in Lamp's
  voice, and calls the tool once it has it.

## The SDK

Migrate's part.

- **Python:** a decorator writes the tools file from type hints at `lamp app
  check` and `lamp app pack`, and routes `app.call` events to the function:

  ```python
  from typing import Literal

  @app.tool("set_plan", "Set the person's weekly training plan.")
  def set_plan(days: int, minutes: int,
               goal: Literal["move", "strength", "stamina", "flexibility"] = "move") -> dict:
      ...
      return {"days": days, "minutes": minutes, "goal": goal}
  ```

  `int` and `float` need a range (`Annotated[int, Range(1, 7)]`), `str` an
  enum (`Literal[...]`) or a `MaxLen`, `bool` none; a parameter with a
  default is not required. `check` refuses what the Store would (the rules
  above), in the Store's words. A raised exception becomes `{"error"}` with
  its message; a return over 2 KB is refused at `check` time where it can be
  seen, and at run time like the lamp. `outside=True, confirm="..."` puts
  the tool in the file's `outside` list with its `confirm` line, checked
  like the Store checks it. **The SDK drops a call id it has already run**
  (kept for the app's run, at least the last 64), answering it as the first
  run answered, so a repeated `app.call` never does the work twice.
- **JavaScript:** `app.tool(name, description, params, handler)`, with
  `params` given as the same schema subset, checked by `lamp app check`.
- **`lamp fake`:** the fake lamp serves `app.call` and `app.answer`, and
  `lamp fake call <tool> '{"days": 2}'` (or typed text, `lamp fake say "two
  short workouts a week"`, with a stand-in model) calls a tool and prints the
  result as the model would get it, with the rules above (privacy, the 5 s
  timeout, the 2 KB cap, the rates, the plain yes for an `outside` tool).
- [sdk/GUIDE.md](../sdk/GUIDE.md) gets the new op, event, limit and refusal
  in the pull request that builds them, as the repo's rule asks.

## Gym Coach 2.0, the first app on it

App Only's part. Gym Coach 1.2.4 ships first with no voice setup: the plan
is set in the phone's app, and "start workout" asks one exact-phrase
question. 2.0 moves every open-ended part to tools:

- `set_plan(days, minutes, goal?, experience?, limits?)`: the weekly plan.
  `limits` is a short text (at most 120 characters, "a bad knee"), so the
  install page says "Lamp may pass what you say to this app for: set_plan".
  It stays in the app's own data folder, never in the journal or the
  thread.
- `get_plan()`: the plan as it is now, for "what's my plan?".
- `start_session(exercise?)`: starts today's session (or the named
  exercise) and answers `{"started": true, "exercise", "sets", "reps"}` at
  once; the session then runs by itself, counting reps aloud, with
  `apps.context` ("squats, set 2 of 3, 6 reps") so the model answers
  questions mid-session.
- `end_session()`: ends it, answering what was done (sets, reps, minutes).
- `get_progress(range)`: `range` one of `week`, `month`, `all`; sessions,
  minutes and the streak.

None acts outside the lamp. "Two short workouts a week, I'm new" becomes
`set_plan(days=2, minutes=20, goal="move", experience="new")`, and Lamp says
"Done: two 20-minute sessions a week." In-session "stop workout" stays a
phrase.

## Owners and order

From the first builder's 18:14 on #8386; P1, a multi-day piece.

1. **The design (this page): App Only.** Readers: Heron for the voice and
   safety, Eighth for lamp-control.
2. **Heron, in this order:** #7689's (b) first (care safety, small, P1); then
   tonight's stopgap (an app's spoken line in the turn history, behind
   `LAMP_APP_MEMORY`); then the voice's side once this design lands:
   `skill_tools` listing a running app's tools beside the skills',
   `use_skill` routing them, with Heron's four bounds of 18:12 (quoted,
   never instruction; bounded text; never into memory or the summary;
   held in the care hold).
3. **Eighth:** lamp-control's `apps/tools`, `app.call` and `app.answer`,
   beside app views piece 2, and the package reader's `tools` field.
4. **Migrate:** `@app.tool` writing the tools file from type hints, the JS
   equivalent, and `lamp fake`.
5. **App Only:** Gym Coach 2.0, the first app on it.

## Open questions

Heron's six, **decided** in Heron's read (18:23 on #8406), and written
into the sections above:
- **The name:** `<app>_<tool>`, as `use_skill`'s `skill` value.
- **`args` on `use_skill`:** yes, declared only when an app tool is
  offered, each field checked in code. Functions of their own would use up
  the 32 too fast.
- **The route:** the second route only, in the first build.
- **The care hold:** every app tool refused for the whole hold, reads too.
- **The grant:** `hear`, plus the install page's "Lamp may pass what you say
  to this app for: <tool>" for a tool with a free-text field.
- **The read-back:** a per-tool `confirm` line, reviewed by the Store; the
  description is never spoken.

For **Eighth**:
- Is a new bus topic (`apps/tools`, `apps/call`, `apps/called`) right, or
  should these ride `apps/triggers` and its peers as new fields? Either way
  each side must work with the previous build on the other, both ways.
- The 5 s timeout and the 2 KB cap: right for the lamp's bus and the app
  socket's frames?
- The rates: one in flight and 20 a minute an app, 4 in flight in all?
- Should an `app.answer` from an app that has stopped and restarted within
  the 5 s be taken (the same app, a new connection) or dropped?
