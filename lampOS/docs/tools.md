# What the model may do other than speak

Scope: `crates/voice/src/tools.rs`, the provider worker's `ToolDesk`
(`crates/voice/src/provider_worker.rs`), and function calls in the provider
link ([gemini-session-reliability.md](gemini-session-reliability.md),
"Function calls"). It is step 3 of [brain-build.md](brain-build.md), and the
written interface between the Voice and the Executive.

A Live session can be told of functions. The model asks for one by name; it
never supplies anything but that name and arguments that fit what was
declared. The voice declares three kinds and runs none itself.

## `not_for_me`, the voice's own

A session has had one thing to do with what it hears: answer it. On
lamp-4ace on 2026-10-13, 23 new sessions each heard two people talking to
each other: 7 spoke words to them and 3 more made a sound, and telling the
session how the room was changed nothing (4 of 11 without, 3 of 12 with).
`not_for_me` gives the model something else to do with speech that is not
for it. It takes no arguments. When the model calls it for a request:

- the call is answered at once (`{"ok":true}`);
- whatever sound the service still sends for that request is dropped in the
  provider worker, before the coordinator and the speaker, and its words are
  not kept as something the lamp said;
- the request ends when the service completes the turn it asked in.

It is declared as a function nothing is to be said on account of
(`Tool::silent`: the service need not wait for it, and its answer is taken
note of without generating). That was chosen on what the real service did
with written requests on lamp-4ace on 2026-10-13, each on a session of its
own, with sentences said to someone else:

| Declared as | Requests | Called it | What followed a call |
|---|---|---|---|
| An ordinary function | 5 | 3 | Once "I'm sorry, I missed that. Could you please repeat it?", once a few words of nonsense in French, once the same call eleven times in a row, each answer bringing the next |
| A silent one | 7 | 2 | Nothing said, and the request over within 4 ms of the call (0.63 s and 0.85 s after the request) |

The requests in which it was not called were not answered at all, and were
closed by the four-second wait as before. None of the twelve was answered
as if it had been said to the lamp. These were written requests; what the
model does with the same sentences heard as speech is not known from them.

**Not after acting.** A request in which the executive was asked to do
something is the lamp's: doing what was asked and then saying it was not
for the lamp cannot both be right, and what was done cannot be undone. A
call of `not_for_me` in such a request is answered and not taken, and
what the model says next is played. On lamp-4ace on 2026-10-14 the model
did exactly that three requests in a row, changing the light and saying
nothing, until its character said that changing its light is something
it does and says ([ring-choreography.md](ring-choreography.md), "The
lamp's own light").

Switched on with `LAMP_TOOLS=on` in the voice unit (the worker's `--tools`).
Off by default: what it does to those counts, and what a declared function
costs an ordinary answer's first audio, has not been measured.

## The executive's

The executive (`lamp-actions`, [actions.md](actions.md)) keeps two things in its
directory, which the voice is given as `LAMP_ACTIONS_DIR` (the worker's
`--actions-dir`, an absolute path):

    <dir>/tools.json    {"tools":[{"name":"light_on","description":"...","parameters":{...}}]}
    <dir>/call.sock     a Unix stream socket

`tools.json` is read each time a session opens, so the executive's
functions appear when it starts and go when its directory does. It is all
or nothing: a file over 64 KiB, one that is not that shape, a function the
provider link would refuse (a name that is not `[a-z][a-z0-9_]{0,47}`, a
description over 1,024 bytes, parameters that are not an object or are over
4,096 bytes) or a name twice leaves the session with none of them, and the
journal says why, by name and never a description's words (`lamp-voice: the
executive's functions are left out of the session: tools.json has what the
link would refuse: Light Off (a name the link does not take)`; #799). At most
32 functions reach a session, the voice's own included: a file with more is
not refused, and the first of it that fit are told (Sixth on #1020);
`not_for_me` in the file is ignored.

The executive's file says only what can be done now, so it changes while a
session lives: the light's functions drop out when the voice, whose ring
the light is, is down for longer than `lamp-actions`' 15 s grace (#1514;
before it, any restart over a second or so dropped them), and come back a
second or two after it starts. A session that opened in that gap would have
no light functions for its whole life (lamp-4ace, 18 Oct 11:22, #1472). So the voice keeps what each session
was given, looks at `tools.json` and `skills/tools.json` every 2 s (reading
them again only when one's time changed; a change found is checked again
every 20 ms until the next look, `RECHECK_EVERY`, so a gap between turns
shorter than a look is still taken, Jarvis on #1588), and when what it would declare now
differs from what the session was given, either way, it renews the session
between turns, on the person switch's path: no turn open, no output owed, no
conversation going on (`going_on`, except as below), no look owed, once a
session and at most once in 30 s (`src/session_tools.rs`). The session after it is opened fresh,
not resumed: Google's resumption with other functions is not tested on a
person's lamp (the first builder on #1472); the thread's recall carries the
conversation. Each session's setup says what it was given, with `; no light
functions: the ring is not up` when the light's are missing, or `; the
executive's catalog is not there`. On by default: nothing changes for a
session whose catalog never changes.


Seen on lamp-4ace on 18 Oct at 12:47–12:50 (#1484), the voice stopped for
20 s so lamp-actions dropped the light's functions, then started:

    lamp-voice: session functions: 24 declared; the executive's 10 (light 0, volume 4, other 6); no light functions: the ring is not up
    lamp-voice provider: the executive's functions changed (+7 (light_brighter, light_colour, light_cooler, …) -0); a new session is opened between turns
    lamp-voice: session functions: 31 declared; the executive's 17 (light 7, volume 4, other 6)

and then "make your light a little dimmer" and "… a little brighter" ran
`light_dimmer` and `light_brighter` (30 and 46 ms).

A session given none of the light's functions (while the voice takes an
executive's, its catalog there or not) is also told, after the rest of its
instruction (`session_tools::NO_LIGHT`):

    Your light
    You cannot change your light right now: it is not ready. If someone asks you to change it, say so in a few words of your own, not the same way twice. Never say you changed it.

The thread's recall leaves room for it, only for such a session, so it
always fits and a session with the light keeps its whole recall; a session
whose instruction is still full says so in the journal. Missing anything
but the light alone, it is told each thing it can't do right now in one line
(`session_tools::not_ready`): the light, the voice's volume, and, with
`LAMP_SCHEDULE_TOOLS`, setting, listing or changing timers and reminders,
as with `lamp-actions` down
and no catalog at all; there a session said "I've turned my voice down
just a little bit" for "turn your light down a little", having no function
(the first builder's lamp run on #1513, 18 Oct 16:27–16:32). The character
file's light and voice line says "when it is ready" and what to say when
told so ([character.md](character.md)). Without it, a session opened in the gap
answered "your light is all set to orange now" five times and made no call,
because its recall held the earlier turns where it had (the first builder's
lamp run on #1495, lamp-4ace, 18 Oct 13:57). On by default: it corrects a
wrong answer, and a session given the light's functions is told nothing of
it.

When the light, the volume or the reminders come back for a session told it
can't use them, the renewal does not wait for the conversation to end. The
lamp's own "I can't right now" keeps `going_on` true, so while the person
kept asking, the light would work and the lamp would keep refusing (Jarvis
on #1513). Such a change (`Change::came_back`: the kinds of added functions
the session was given none of) renews at the first real gap between turns
(the first builder on #1588): no turn owned (nobody's speech taken and the
lamp not speaking, by the turn owner's own state), none open, no output or
look owed, once a session and at most once in 30 s, and during a
conversation for the same kind at most once a minute (`BACK_AT_MOST_EVERY`),
so a light that comes and goes (#1514) does not renew it over and over. In a
quiet room a return renews on the 30 s bound, as any change does under
#1484. It need not wait out the 30 s after
a renewal that brought nothing back: lamp-actions stopped and started in a
quiet room renews for the light gone (a restart keeps its folder and its
catalog, so it renews nothing: El on #1588, [actions.md](actions.md)), and the light back seconds later renews at the
next gap rather than the lamp refusing for half a minute (Jarvis on #1588).
Two returns in a row still wait for it, so a catalog that comes and goes
renews at most twice in 30 s. The cost is one reconnect landing just after
the lamp's "I can't right now", when the person is likely to answer: a turn
begun during it is held for the new session (lamp-4ace took 1.9 s from
`server_go_away` to ready again, #960), once per return; not yet timed on a
lamp for this case. With `--early-end-ms` (off, and set nowhere
in `deploy/`), a turn's early piece and the rest that follows it could land
on two sessions; the renewal does not wait for such a continuation yet, so
that flag needs it before it is turned on (Jarvis on #1588). The session then ends only when
nothing is owed on its connection, so never with a function call open, one
the Policy holds included. The session after it is opened fresh with the
thread recalled, as any renewal is, so "and a bit warmer" right after "make
your light orange" is still understood. The journal says so in one line:

    lamp-voice provider: the executive's functions changed (+7 (light_brighter, light_colour, light_cooler, …) -0): the light is back, which the session was told it could not use; a new session is opened between turns, though a conversation is going on

Any other change, more of a kind the session had or one taken away, still
waits for the conversation to end, as before (#1484's rule (b)). Every
catalog renewal, not only these, now also waits for no turn owned.

A call is one line to the socket and one line back, on a connection of its
own:

    {"op":"call","name":"light_colour","args":{"colour":"blue"}}
    {"ok":true,"result":{"light":"blue"}}
    {"ok":false,"error":"The light is already at its brightest."}

`result` goes to the model as it is; `error` goes as `{"error": "..."}`,
and should be words the lamp can say. The answer may be at most 8,192
bytes on the line and about 4,000 as JSON to the model; a larger one is
treated as no answer.

### The skills'

`lamp-skills --tools-dir <skills>` ([skills.md](skills.md#tools-for-the-model))
keeps the same two things in a folder of its own, which the voice is given
as `LAMP_SKILLS_DIR` (the worker's `--skills-dir`, an absolute path; the
product's is `/run/lampos-skills`), and the voice reads and calls them the
same way, at the same session open and within the same deadline:

    <skills>/tools.json   each skill's tool, described with its SKILL.md description,
                          with the parameters of its choice and flag, if it has them;
                          a skill's [stop] function (#1685), its "how" one of snooze,
                          off or on; and beside the tools, "body": the tools whose skill asks
                          for the body (a text never runs one, #1673)
    <skills>/call.sock    {"op":"call","name":"dance","args":{"style":"calm"}} -> {"ok":true,"result":{"next":"..."}}
                          arguments that are not the tool's -> {"ok":false,"error":"..."}

Until 19 Oct they were in `<dir>/skills`, inside the executive's folder,
which systemd removes at each restart of `lamp-actions`: the voice then had
no skill functions until `lamp-skills` restarted (lamp-4ace, 04:33). That
folder is still read and called after `<skills>` for one release, so a
voice and skills from different builds still meet, but only while
`<skills>` has no socket: after the update, until actions restarts, it
keeps the older `lamp-skills`' catalog and a socket nobody answers (the
reviewer on #1656). A name `<skills>` gives is its. Both are looked at for
a changed catalog with the executive's, and a call to either is a skill's
(a skill's look, a held reply), known by its folder, not by its name.

They come after the executive's and within the same 32. A name the voice
or the executive already has, or a Schedule name, is left out of the
skills', and a call of it goes where it did before. A skill's call is the
executive's route in every other way: it waits for the Policy's hold like
the executive's, and the request counts as one in which the lamp acted.
No file, and nothing changes. Host test:
`the_skills_tools_are_told_after_the_executives_and_go_to_the_skills`.

**Requested local audio** (#1685). With exact `LAMP_REQUESTED_AUDIO=on`, a
tool-only skill may return one fixed recording after a short confirmation.
Voice adds private `requested_audio_context` beside the ordinary call's
`name` and `args`; it binds worker boot, provider session, request, privacy
generation, call id and original issue time. Skills returns a typed
`requested_audio` offer beside `next`. Voice removes that offer before
giving the tool result to the model and uses the existing Skills socket's
private `audio_authorize`/`audio_finish` operations for its bounded lease.
Unknown private fields and enum values are refused. Neither direct calls nor
`use_skill` can obtain authority through model arguments; both preserve the
original context and deadlines. Written and Messages requests, including
`speak=true`, cannot supply spoken context. Ordinary calls retain their
existing wire. See [requested audio](requested-audio.md) for default-off
advertisement, confirmation retirement, immutable asset checks and terminal
cancellation. This does not extend autonomous three-second sound cues.

**Many skills: `use_skill`** (#1668, `LAMP_USE_SKILL=on`, the worker's
`--use-skill`; off unless set). A session may be given 32 functions; the
voice's own and the executive's are 30 on a product lamp, so a third skill's
function is cut without a word to the model: `declared` keeps only the room
left. The setup line names what it cut (`; skills not given: show_verdict
(past 32: LAMP_USE_SKILL=on reaches them)`, the reviewer on #1696). With
the switch a session is given one function for every skill, `use_skill`,
and none of the skills' own (`crate::use_skill`):

- **`use_skill`'s arguments:** `skill`, one of the installed skills' names
  (an enum); `choice`, a word from the skill's choice; `flag`, its
  yes-or-no. A skill takes at most one of each (`tools_file` in
  `lamp-skills`). The enum names as many skills as fit the link's 4 KB for
  parameters (`use_skill::offered`, about 67 with the longest names), so a
  lamp with more keeps the function; the setup line names the rest (`;
  skills not given: … (past use_skill's bound)`).
- **What the session is told:** after the rest of its instruction, "Your
  skills": one line each, its name, its description, and its choice's name
  and words and its yes-or-no's name, each with what its parameter says.
  At most 4,096 bytes (`SECTION_MOST_BYTES`): skills past that are named
  alone while their names fit, then left out of the lines; the setup line
  counts both (`; use_skill's lines: N named alone, M left out`). The Live route keeps that
  room from the thread as it keeps the room for what it can't do right now;
  the cascade (`LAMP_VOICE_ROUTE=second`) tells it in each turn's system
  message.
- **A call:** routed as `Route::UseSkill` and made the skill's own call
  when it runs (`use_skill::translate`). The choice and the yes-or-no go
  under the skill's own parameters' names. The skill's own names are taken
  as well (`{"skill":"dance","style":"jazz"}`), as its line says them. Only
  the skills' socket is ever asked. So the skill's socket sees the
  same line as when it is called itself, and its answer goes to the model
  under `use_skill`.
  - **Told instead, never dropped** (the reviewer on #1696):
    - a skill that isn't installed ("There is no skill named juggle. Your
      skills are …");
    - a word not in its choice ("dance's choice is one of calm, upbeat,
      jazz.");
    - any other key ("use_skill takes skill, choice and flag, not mood: …");
    - a choice or a yes for a skill that takes none ("check_my_work takes no
      choice."). An empty choice or a "no" there is nothing.
  - **As a skill's call in every other way:** it waits for the Policy's hold,
    it never runs on a reply, its answer may ask for a look, and the request
    counts as one in which the lamp acted.
- **A skill installed or removed** changes no function's name, so each skill
  `use_skill` reaches is counted as `use_skill:<skill>` against what the
  session was given, and the session is renewed for it as for any change.
  The journal's setup line says `; N skills through use_skill`.
- **`use_skill` is the voice's own name, with the switch or not:** a
  catalog's function of that name is never told.
- **The cost on the host:** a call through it took 0.40 ms against 0.22 ms
  made itself (`use_skill_cost`, debug build, medians of 300). It reads the
  catalogs again. The model's own time for a turn through it is not measured
  yet: that's the lamp check, the same lines with the switch on and off
  against Live's 2.02–2.21 s tool turns.
- **Host tests:** `with_use_skill_every_skill_is_reached_through_one_function`,
  `a_use_skill_call_is_asked_of_its_skill_as_its_own` (the cascade's
  `answer` and the worker's `run_call`),
  `with_use_skill_a_session_is_told_its_skills_after_its_instruction`,
  `a_skill_reached_through_use_skill_asks_the_coordinator_for_its_look`,
  `a_skill_installed_behind_use_skill_is_a_change`, and `use_skill`'s own.

### Schedule tools and due speech

`LAMP_SCHEDULE_TOOLS=on` enables six additional Actions tools:
`set_timer`, `set_reminder`, `repeat_reminder`, `list_reminders`,
`cancel_reminder`, and `snooze_reminder`. Both Actions and Voice need that
exact value. Voice still needs `LAMP_ACTIONS_DIR`; it filters these names from
both declaration and dispatch when the switch is absent, even if an old
`tools.json` still lists them. `off`, `ON`, `true`, `1` and padded values do not
enable anything. The normal catalog and ordinary Written wire messages stay
unchanged with the switch off.

The enabled catalog tells the model to use `set_timer` for a duration from
now, including "remind me in two minutes", with the person's label and whole
seconds. `set_reminder` is reserved for a requested clock time; a relative
duration must not be rounded into its minute-only `HH:MM` field. See the
[relative-time examples and lamp check](schedule.md#relative-time-follow-up-on-the-lamp).

Actions validates arguments before IPC, owns Schedule's CAS guards, and calls
the socket selected by `LAMP_SCHEDULE_SOCKET` (enabled default
`/run/lampos-schedule/call.sock`). Text is 1–300 UTF-8 bytes without controls;
the adapter refuses unknown fields and invalid civil dates/zones. Model schemas
use the provider's supported `parameters` subset; runtime checks enforce byte
and shape limits omitted from those declarations. List replies are bounded
pages with complete text and an explicit next cursor. The model never supplies
store revisions, acknowledgement tokens or Unix-clock arithmetic. See
[Schedule](schedule.md#proposed-model-tools) and [Actions](actions.md).

Only an actual durable mutation receipt is success. An unavailable service or
proven refusal means the lamp cannot set it just now. A sent request with a
lost, malformed or oversized reply means confirmation is unknown, not that
nothing happened. Do not automatically issue a fresh mutation to resolve that
uncertainty; read current reminders first. The new exchange has a 1,250 ms
Actions deadline across all hops and a 1,500 ms Voice deadline; ordinary tools
keep their existing transport.

`LAMP_SCHEDULE_SPEAK=on` is independent of tool declaration. It starts Voice's
single optional delivery consumer, requiring an explicit persistent
`LAMP_SCHEDULE_DELIVERY_DIR`. The due text is JSON-quoted reminder data in a
fixed instruction. Every tool call on that autonomous request is denied before
the tool desk, so reminder text cannot change the light, memory, schedule or
another tool. Privacy, current input, normal readiness and final output
ownership remain authoritative. Unknown, Busy and Quiet room classifications
all allow a due reminder at the next local silence; they do not defer an
already requested timer or reminder. A deferral has no failure announcement and
does not stop ordinary intake. Whole playout evidence is persisted before Ack;
interrupted or uncertain speech remains pending. Provisioning, recovery and
the requested-reminder admission policy are described in
[the consumer contract](schedule.md#optional-voice-consumer).

The schema review used Google's official
[Live tool guide](https://ai.google.dev/gemini-api/docs/live-api/tools),
[Live protocol](https://ai.google.dev/api/live), and
[FunctionDeclaration reference](https://ai.google.dev/api/generate-content#FunctionDeclaration)
on 2026-10-14. It supports keeping the existing manual function-result path
and correlated tool names. Cancelling model work does not undo a local mutation
already committed. These references do not qualify the six declarations or
spoken confirmations on the model and lamp actually deployed; First runs those.

### A word that asked for nothing, and a guard that was withdrawn

On lamp-4ace on 2026-10-14 a played "Thank you." was written by the
service as "cambiar", and the model called `light_colour` and said "I've
turned my light to yellow for you." Earlier that night a misheard word
had turned the volume up. Nobody had asked for either.

A guard was built for it the same morning and taken out within the hour.
It refused a call to the executive when the text of what was heard in
that request was one to three words and none of them was about a light
or a voice or said yes. On its first minutes on the lamp, "Brighter."
was played; the service wrote it as "Rainer"; the model, which had heard
it rightly, called `light_brighter`; and the guard refused the call. The
two requests to change the light that followed got no answer at all.

The text is the worse witness of the two. The model hears the sound; the
text is a second reading of it, and for a word or two it is often
another word ([attention.md](attention.md) found the same of the lamp's
name). A rule that lets the text overrule the model's hearing undoes
right actions as readily as wrong ones, and nothing else on the lamp
knows what a short word was. What is left is the character file's line:
change the light or the voice only when someone has plainly asked, in
words you made out. Whether that line is enough is not shown: the
mishearing cannot be made to happen on demand.

## The memory's

With `LAMP_MEMORY_SOCKET` in the voice unit (the worker's
`--memory-socket`, an absolute path) a session is also told of three
functions, declared by the voice and run by `lamp-memory`
([memory.md](memory.md), "How the voice uses it"):

| Function | When the model calls it | What it is told |
|---|---|---|
| `remember {text}` | Someone asks it to keep something | `{"remembered":true}`, and it says back what it kept |
| `recall {about?, days_ago?}`; with `LAMP_MEMORY_RECALL_V2=on` also `days_ago_from?, days_ago_to?` (a span, at most 14 days, last week 7 to 1) | Someone asks about an earlier conversation or day | `{"found":[{"when":"yesterday","heard":"...","you_said":"..."},{"when":"last week","asked_to_remember":"..."}]}`, or an empty list and "Nothing about that in what I remember." |
| `forget {}` | Someone asks it to forget the last thing it was asked to keep | `{"forgotten":true}`, or `false` with "There was nothing I had been asked to remember." |
| `forget {source_id}` (with `LAMP_MEMORY_FORGET_ALL=on`) | "Forget that", about something recall just gave: its `source_id`, only one this session was shown, of the person's own memory when someone known is at the lamp, else the room's | `{"forgotten":true}`; an id it wasn't shown is refused and nothing is removed, `false` with a note to call recall first |
| `forget {about}` (with `LAMP_MEMORY_FORGET_ABOUT=on`) | "Forget what I said about my bicycle": one to four key words, without words like "my" or "the". Lists the person's own kept things when someone known is at the lamp (and the room's when none of theirs match), else the room's, and forgets each that has every key word, accents aside, at most eight. Of the room's, only those this session was shown or kept. It matches words, not synonyms ("bike" is not "bicycle") | `{"forgotten":true,"count":N,"about":"..."}`, never the kept texts; `left` with how many of the room's stay, not being from this conversation; stopped part way: `"all":false`, "Some of it may be forgotten"; no match, more than eight, a listing that may be cut or no key word: `false` with a note that nothing was forgotten, never the last kept thing instead |
| `forget_everything {}` (with `LAMP_MEMORY_FORGET_ALL=on`) | "Forget everything about me", after the lamp has asked once and they said yes; for someone known by face only | `{"forgotten":true,"everything":true}`, or `false` with "I was keeping nothing of them."; with nobody known `false` and nothing sent ("I can forget everything only for someone I know by face…"); a known person's line that was not seen spoken (the room's line under `LAMP_PERSON_SCOPE_SEEN`) empties nothing either, since a voice off camera beside them reads the same, and is told `{"forgotten":false,"note":"Nothing was forgotten: I couldn't see who asked. Ask them to look at me and say it again."}` (`remember::forget_everything_unseen`, #3564: on lamp-52e6 on 29 Oct the asker's mouth read still in 11 of 11 looks, and the model was told the lamp didn't know them) |

They take the same road as the executive's: a thread of its own, 1.5 s at
most, at most eight waiting, the answer only to the connection that
asked. While someone the eyes know is at the lamp they use that person's
memory, and a recall's entries from it carry `"theirs":true`, before the
room's ([memory.md](memory.md#each-persons-own-764)). A memory that does not answer is told to the model as "My memory
is not answering right now, so I could not do that." An executive that
lists a function of one of these names does not get it.

With the Days memory and `LAMP_MEMORY_RULES=on` (#813, off unless set) a
session is told of three more, for corrections and standing preferences
about how the lamp behaves with the person, never tasks or things to
remember ([memory.md](memory.md#what-a-person-teaches-and-corrects-813)):

| Function | When the model calls it | What it is told |
|---|---|---|
| `keep_rule {rule, speaking_up?, about?, never?, not_before?, not_from?, not_while_talking?}` | The person corrects how it behaves or says how they want it to behave from now on, also in passing ("from now on…", "always…", "never…", "stop…", "don't call me…"), even when it has already begun to do so; never for a one-off request about the answer at hand. `speaking_up` is `never_first` or `not_first_in_mornings`, only for a rule about speaking first; `about` (`greeting`, `news`, `both`) with its limits (`not_before` / `not_from` as `HH:MM`) only for a rule about the greeting or news said unprompted (#813) | `{"kept_rule":true,"note":"Follow it from now on, ..."}`; a rule over 200 bytes or with no words is refused, and so is a limit without `about` or a time not `HH:MM`, and a seventh rule (`"kept_rule":false`, ask which to drop) |
| `forget_rule {number}` | The person takes a rule back ("you can tell me the weather again", "forget that rule"). The number is required, as numbered in the session's rules or by `list_rules` | `{"forgotten":true}`; with no number or one that is not theirs nothing is removed, `false` with a note to call `list_rules` and ask which |
| `list_rules {}` | "What have I asked you not to do?", or to find a rule's number | `{"rules":[{"number":3,"rule":"...","when":"yesterday"}],"note":"The numbers are for forget_rule only: never say them."}`, or an empty list |

Without the switch none of these is declared and a call of one of these
names is not the memory's.

## `reports_stop`, a system function

With `LAMP_REPORTS=on` in the voice unit (the worker's `--reports-stop`,
after `--use-skill`) a session is told of `reports_stop`, with no
arguments: "stop sending reports" (#1683, [cloud.md](cloud.md), "Health and
crash reports"). It is the voice's own, never a skill, and comes before the
executive's and the skills' functions, so it is never the one cut past 32;
a skill cut for it is named on the setup line (#1706).

The switch declares two functions, `reports_stop` and `reports_state`
(below), so a session is told two more than without it. A product lamp
declares 31 without it (the voice's own 13, the actions part's 17 and
Dance's) and 33 with it. At the bound a core skill's function is never the
one cut (the owner's rule, on #2614; [install.md](install.md)), and
neither is `reports_stop` nor any of the actions part's (the first
builder, on #2644): the cut takes the second route's fetch, then its time
elsewhere, then `reports_state` (reading reports' state is rare), then the
skills' functions that are not a core skill's, the last first, then the
search. A Live session's bound is 32 (`lamp_gemini::MAX_TOOLS`); the second
route's chat requests have their own, 48 (`second_provider::MAX_TOOLS`,
22 Oct: Live's bound had left out the time, the light scenes and the search
on lamp-4ace). On the product's chat route nothing is left out; in a Live
session `reports_state` alone, and `dance` stays in both. With `LAMP_USE_SKILL=on`, `use_skill` carries every
core skill, so it is a core name and never the one cut. Whatever is cut is
said once, at start, on a plain journal line naming the bound (`past the
bound of 32, left out: …` on Live, 48 on the chat route); only the lamp's own
and core skills' functions alone past it are an error line (`alone are past
the bound of 32; left out: …`). Tests:
`with_reports_on_dance_stays_at_the_bound`
(`crates/update/tests/functions.rs`),
`reports_add_two_functions_and_the_last_skills_are_cut_past_32` and
`past_the_bound_use_skill_and_the_lamps_actions_stay`.

The voice puts `{"share": false}` on `reports/request` and answers from `lamp-cloud`'s
`reports/state` within 2 s: `{"ok": true, "say": "I've stopped sending
reports."}`, or "I'm not sending any reports." when they were off. With
nobody taking the ask (`lamp-cloud` not running), or no answer in time, it
says so and never claims a stop. Only a request a person said runs it.
It runs in a request whose answer the attention rule holds too, its
answer still held: the one call that does (`DONE_THOUGH_HELD`,
[attention.md](attention.md), "What happens to an answer that is held
back"). It then asks with `held: true`, which `lamp-cloud` keeps.
On the second route a line whose answer is held is asked about
`reports_stop` only when its heard words name `report`, `reports`,
`sharing`, `sending` or `data` (whole words, any case;
`attention::HELD_ASKED_WORDS`, #1925, [attention.md](attention.md)); any
other held line is sent nowhere.
There is no function to turn reports on: that is the person's own screen.

Beside it, `reports_state`, with no arguments, which the model is to call
only when the person asks about reports (#1880, the first builder on
#1722, 18:44). Answered at once in the voice from `reports/state`:
`{"sending": true, "say": ...}`, or `{"sending": false, "say": "I'm not
sending reports. Someone asked me to stop them at 3:04 pm today. They can
be turned back on in the app."}` for the voice's stop, "They were turned off
in the app at ..." for the app's, in the lamp's own time ("yesterday", or
the date, when older). Without `lamp-cloud` it says the part is not
running. Nothing about reports is said unasked: only this call's answer
carries how they were stopped.

## `the_time`, the voice's own

With `LAMP_CLOCK=on` in the voice unit (the worker's `--clock`, after
`--memory-socket`) a session is told of one more function, run in the
voice itself and answered at once: `the_time`, which the model is to call
whenever someone asks the time or the date or how long until something.
It is told `{"now":"It is 11:47 in the morning, Wednesday 14 October
2026. Said aloud: eleven forty-seven in the morning."}` from the lamp's clock and zone (`crates/voice/src/local_time.rs`),
or `{"error":"My clock could not be read just now."}`. Unlike
`not_for_me`, it is not silent: the model says what it was told. A whole five is said as past or to ("ten past six", "quarter to seven"), any other minute as the clock reads ("twelve forty-two", "six oh seven"): on lamp-4ace on 2026-10-16 the lamp said "eighteen to one at night", which nobody says.

With `LAMP_KERNEL_CLOCK=on` as well (#830, [offline.md](offline.md)) the
voice first asks the kernel whether the clock is set
(`lamp_ipc::clock_synced`, as `timedatectl`'s `NTPSynchronized`). While it
is not, `the_time` answers `{"error":"My clock has not been set yet, so I do
not know the time or the date right now."}` and a new session is told that
its clock has not been set, that it does not know the time or the date, and
never to guess one, in place of the time it woke: a lamp has no battery
clock, and after power with no internet its clock is its last saved time.
Off unless set; with it off both are as above.

**Another place's time** (#1547), with `LAMP_TIME_PLACES=on` as well (the
worker's `--time-places`, right after `--clock`; nothing without
`LAMP_CLOCK`). On lamp-4ace on 18 October at 16:40, in the same-voice A/B,
"Hey Lamp, what time is it in London right now?" got "It's twenty to five
in the afternoon in London.": the lamp's own time, in Ho Chi Minh City,
said as London's (London was at 10:40). The model knew only the lamp's
time, from its opening note, and nothing said whose it was.
- **The function takes a place:** `the_time` is declared with an optional
  `place`, the place's time zone by its IANA name (`Europe/London`,
  `America/New_York`), which the model picks. A city's name alone ("London",
  "New York") is looked for in the usual areas (Europe, America, Asia,
  Africa, Australia, Pacific, Atlantic, Indian), and a name with no area is
  taken only there or as `UTC`: the folder's top-level `EST`, `MST` and
  `HST` have no summer time, so "EST" for the East Coast would be an hour
  off New York's half the year (the reviewer on #1552). The zone file is read from
  `/usr/share/zoneinfo` and read by `lamp-local-time`, as every part
  reads the lamp's own zone (#820). Only
  `[A-Za-z0-9_+/-]` is taken, with no `..`, no `//` and no leading or
  trailing `/`, and the file must be a zone file. A slim zone file, which
  lists no changes past its last rule change, has its closing rule worked
  out, and since #820 so has the lamp's own: before, past the changes its
  file listed, its time was not told.
- **The answer names the place, and the lamp's own beside it:**
  `{"now":"In London it is 10:40 in the morning, Sunday 18 October 2026.
  Said aloud: twenty to eleven in the morning. Here in Ho Chi Minh it is
  4:40 in the afternoon."}`. The lamp's own place is its zone's last part,
  from where `/etc/localtime` links, which is the time the lamp keeps, and
  from `/etc/timezone` only when that is no link: `timedatectl
  set-timezone` changes the link and not the file (the reviewer on #1552).
  It is left out when neither reads. A call with no usable place (none,
  empty, `null`, a wrong key) is given the lamp's own time naming its
  place: "Here in Ho Chi Minh it is 7:32 in the evening, …" (the reviewer
  on #1589). A lamp whose zone is under no area of places (`UTC`,
  `Etc/GMT-7`, `CET`, `US/Pacific`, `Factory`) says only "Here it is …",
  in both answers: "Here in UTC+7" or "Here in Pacific" would sound like a
  machine's clock, not a place (Ten on #1602, the reviewer on #1609). The
  function's description asks the model to say the place only for another
  place, so a plain "what time is it?" is not answered "… here in Ho Chi
  Minh" (the reviewer on #1602). Asked for its own zone,
  or another name for the same zone file (`Asia/Saigon`), the answer ends "That is
  the time here too." A place it has no zone for answers
  `{"error":"I don't know the time in that place: it is not a time zone I
  know. Call again with the place's IANA time zone, such as Asia/Shanghai
  for Beijing or Asia/Ho_Chi_Minh for Hanoi."}`, never the lamp's own time:
  a city no zone is named after ("Beijing", "Hanoi") is asked again by its
  zone. So is a `place` that is not words. `Etc/GMT-7`, whose sign POSIX
  turns, is said "UTC+7". A `/etc/localtime` link outside a zone folder
  leaves `/etc/timezone` to name the lamp's zone (Jarvis on #1575).
- **The opening note says whose time it gives:** "… This is the time where
  you are, in Ho Chi Minh. For the time anywhere else, call the_time with
  that place, never this time." For a zone under no area of places, "This
  is the time where you are."
- **Off unless set;** without it the function, its answer and the note are
  as above, byte for byte. Both voice routes take it.
- **What it costs:** one zone file of a few kilobytes read in the voice,
  in the call the model already made for a question about the time. Not
  yet timed on the lamp; the check below measures first audio with and
  without it.
- **Beside `lamp-local-time`** (#820, #1380), which resolves zone names with
  a zone database of its own: the voice reads the system's zone files and
  works out a slim file's closing rule itself (`rule_offset`). Jarvis's 64
  moments across 2026's changes (London, New York, Sydney, Lord Howe,
  Kathmandu; fat and slim files) match Python's `zoneinfo`, but the two can
  differ where the system's zone data and the bundled one do. A later move
  to the shared crate would end that, if the voice's size allows.
- **Not shown on a lamp:** "what time is it in London / New York / Tokyo /
  here", five times each, checked against `TZ=… date`, with first audio
  with and without it.

Tests: `local_time::tests::a_zone_s_closing_rule_is_worked_out_across_its_changes`
(London, New York and Sydney across their 2026 changes, on zone files with
only the rule; the lamp's own zone still read as before),
`…::another_place_s_time_names_the_place_and_the_lamp_s_own_beside_it`,
`…::a_place_is_found_by_its_zone_or_its_name_and_never_outside_the_zones`
and `tools::tests::the_time_takes_a_place_only_with_its_switch`.

Why: a session is told the time when it opens, and may run for hours. On
lamp-4ace on 2026-10-14 a session opened at 10:48 told someone at 11:17
that it was "roughly around ten forty-five": it was doing as told ("this
may be up to an hour old: if someone asks the time, give it roughly").
With the switch on, a new session is told instead that it woke at that
time, that it may be much later now, and to call `the_time` first.

What it costs: one call and its answer before the model speaks, for a
question about the time only. On lamp-4ace since 11:52 on 2026-10-14.

Played at 13:46 that day, three questions: "what time is it?" at 13:46 got
"It's just before two in the afternoon", "what day is it today?" got
"It's Wednesday, October fourteenth", and "what time is it now?" at 13:47
got "It's twenty to two": each asked the clock first (the voice's log
says so), and the first word came 1.0 to 1.1 s after the end of speech
was decided, as for other questions. The last is rounder than a person
would say at 13:47 ("quarter to two"); the function's description asks
for a person's way of saying it, and the model rounded to ten minutes.
At 12:08 someone at the lamp asked it, and it said "It's a little past
twelve.", which was right.

Played at 18:23 that day (`checks.sh steady2-1014`), "Hey Lamp, how are
you feeling right now?" made the model call the clock, and it answered
"It's twenty-three past in the evening.": not what was asked, and the hour
lost from "6:23 in the evening". So since `97880dc57` on `lamp-voice`
the clock's answer carries the time in the words a person says it in
("Said aloud: twenty-three past six in the evening": o'clock, past up to
half past, then to the next hour, quarter and half by name), and the
function's description says it is not for "now" or "today" in a question
about something else, such as how the lamp is feeling right now. On
lamp-4ace since 19:36 (build `e3a92c070`). Played at 19:51: "Hey Lamp, how
are you feeling right now?" got "I'm all warmed up and ready to chat! How
are things with you?" with no call to the clock; "Hey Lamp, what time is
it?" and "Lamp, what time is it now?" each called it, were told "Said
aloud: nine to eight in the evening.", and said "It's just about nine to
eight in the evening." and "It's right around nine to eight in the
evening." (the hedge is the model's: the minute was exact). One run each.
In the soak at 20:30 that evening "Good night." got "Night! It's
twenty-eight past eight in the evening. I'll keep the desk company while
you rest.": the time, unasked. So the description now ends "and never say
the time unless you were asked for it" (not yet played).

## What the voice promises, and what it does not

- **Every call is answered.** The service generates nothing more for a
  request until its calls are, so a call nobody runs is answered too.
- **Executive calls have a 1.5 s budget** (`ASK_DEADLINE`) on a thread of
  their own. Schedule's client uses one nonblocking connect/read/write
  deadline; the older ordinary-tool client still uses a blocking initial
  connect, so its budget is not a hard bound on that OS operation. The
  worker's loop, the microphone and the speaker are never held. With the
  executive gone, late, cut off or answering something that is not an
  answer, the model is told "That part of me is not running right now, so
  I could not do it." and the conversation goes on. Schedule instead reports
  an unknown outcome after a possibly sent mutation, with a request to list
  current reminders before retrying. It never turns a missing answer into
  proof that nothing was committed.
- **At most eight calls wait at once.** A ninth is answered as unavailable.
- **An answer goes only to the connection that made the call.** If the
  link dropped meanwhile, the answer is not sent and the call is over.
- **Nothing is retried,** and the voice does not know whether the
  executive did what it was asked when no answer came. The executive owns
  that: a command that was tried is finished, and what it reports is what
  it established. One case says less than happened: a light call cut off
  by `lamp-actions` stopping is told it could not be done, though the light
  may already have changed. `lamp-actions` puts the change on
  `light/command` and then waits up to 600 ms for the ring to confirm it
  (`light.rs`), so a stop inside that wait leaves the change made and the
  model told otherwise (El on #1588).
- **Names are logged, arguments never.**

## Trying it without a sound

    lamp-voice provider-check /etc/lampOS/voice/provider.json --tools --written "NOTE"

opens one session told of `not_for_me` and of a sample `light_colour`,
sends the note as a written request, answers every call as done, and
prints which functions were called with what, when, what was said and how
the request ended. Nothing is captured, played or changed on the lamp.

### Where a turn with a function call spends its time (#799)

    lamp-voice provider-check /etc/lampOS/voice/provider.json --tool-turns 5 [--thinking TOKENS]

On lamp-4ace on 2026-10-15 a plain answer began 0.87 to 1.45 s after the
end of speech was decided, and a turn with a function call 1.8 to 2.9 s
after, though the tasks part answered each call in 38 to 52 ms. The voice's
own handling of a call is a few milliseconds: it is read on the provider
worker's 2 ms tick, a tasks call runs on a thread of its own, and the
answer is written as soon as it is picked up. Nothing waits for playout,
for the model's `turnComplete` or for an ownership check. The model makes
the call in one generation and speaks in another, after the answer; with
`provider.json`'s thinking budget of 128 each may think first.

This mode measures that against the model alone, with nothing on the lamp
touched (`crates/voice/src/tool_turns.rs`). It asks six written requests,
ROUNDS times (1 to 20), each on a connection of its own told of the
voice's `not_for_me`, a sample `light_colour` and the agent tasks'
three functions as on the lamp:

| Request | Function expected |
|---|---|
| "What's the capital of Australia?" | none |
| "Tell me one fun thing about octopuses." | none |
| "Turn your light blue, please." | `light_colour` |
| "Can you ask Claude Code on my office computer to write a short poem about a desk lamp?" | `start_agent_task` |
| the same, then "Yes, go ahead." (only the second is timed) | `confirm_agent_task` |
| "How is the work you gave Claude Code going?" | `check_agent_task` |

Every call is answered at once by a stand-in of the real answer's shape
(`tool_turns::stand_in`), so nothing reaches the tasks part or the
executive, and nothing is captured or played. One JSON line a request,
times in milliseconds from the request being written: `call_ms`,
`answer_sent_after_call_ms`, `first_audio_ms`, `audio_after_answer_ms`
(the model's second generation), `lead_in_ms` (audio said before the
call), what was said and how it ended. Then a `tool_turns_summary` line
with the medians for each request, for plain turns and for turns that
called the function expected. Sessions are opened one at a time, 3 s apart;
one that cannot be opened (on lamp-4ace at 16:20 the relay refused new
connections that came too fast on one key: `ConnectTimeout` after three) is
a line saying where it failed, the pause doubles up to 30 s after each in a
row, and the summary counts them (`asked`, `failed`, `failed_at`) apart from
the medians. So does a request whose turn the model did not complete: one
that timed out after 14 s, whose connection ended, or that the service left
unanswered keeps its times for reading, says how it ended in `failed`, and is
in no median (the reviewer on #810, 17:23). `--thinking TOKENS` sets the thinking budget
for that run alone (lamp-4ace's `provider.json` has 128), so `--thinking 0`
against none says what thinking costs each of the two generations; the
summary says which budget it ran with.

A written request is not speech. These times leave out the lamp's end of
speech, its 350 ms playout start and the speaker, and measure only what
the model adds; the first sound in the room comes later by those.

**The lead-in (`LAMP_TOOL_LEAD_IN=on`, off unless set).** The owner ruled
out fillers ("uh huh", "Sure, one sec"); the first builder asked on #799
(15:48) for words that carry the answer instead, said in the same turn
before the call, what a person says while they reach for something. With
`LAMP_TOOL_LEAD_IN=on` in the voice's unit (passed to the provider worker as
`--tasks-lead-in`), the three agent-task functions ask for that
(`crates/voice/src/agent_tasks.rs`):

| Function | Said before the call | Told after its answer (`next`) |
|---|---|---|
| `start_agent_task` | the read-back, as a statement: the task in one short sentence (with who will do it, when there is a choice); then the call at once | if that was said, only the question whether to start it ("Shall I start it?"); if not, the read-back and the question |
| `confirm_agent_task` | in a few words that it is giving it to whoever will do it ("Okay, giving that to Claude Code.") | they have heard that; add only, in a few words, that you will tell them when it is done, unless already said |
| `check_agent_task` | in a few words that it is looking ("Let me see how it's going.") | as without the switch |

For `start_agent_task` the lead-in is the read-back itself, not a word
before it, so the turn does not say the task twice. Its question comes after
the call, from the answer, while the read-back is still being played: a
question said before a call ends a spoken turn, and the call need not come.

**What went wrong on lamp-4ace on 2026-10-16 (00:03 to 00:11, #799).** With
the lead-in, work asked of the staff (`LAMP_TASKS_ASKED=on`) and `LAMP_TASKS_WHO=on`,
the model called no function in either round, not even `check_agent_task`,
and made up what became of the work. Two causes, both mended:
- With two of the staff to choose from (lamp-4ace's tasks part has Claude
  Code and Codex: work was prepared for Codex there on 2026-10-15),
  `start_agent_task`'s description was 1,036 bytes, and the link takes at
  most 1,024. A list with one
  function it would refuse left the session with **no function at all**
  (`thread::equipped`), and nothing said so. Now only that function is left
  out, the rest are kept, and the journal says so (`lamp-voice: left out of
  the session, which the link would refuse, keeping the other N:
  start_agent_task (1036 bytes described)`, `tools::admitted`; a name given
  twice and a function past 32 are left out the same way). A test holds
  every description under the limit for every combination of the switches
  with one, two or three of the staff
  (`every_description_fits_the_service_whatever_the_switches`), and another
  every function of the voice's own with all its switches on. The lead-in
  sentence is the shortest that says it (1,005 bytes with three of the staff).
- The read-back and its question were asked for before the call. Now only
  the read-back is, as a statement, "then call it at once", and the
  question comes from the answer. The functions, their
parameters and their order are the same either way (a test). Without the
switch every description and every `next` is byte for byte as before
(`without_the_switch_or_anyone_else_to_name_the_functions_are_exactly_as_before`,
`without_the_lead_in_what_the_model_is_told_after_a_call_is_as_before`).

`provider-check --tool-turns` declares and answers them the same way when
`LAMP_TOOL_LEAD_IN=on` is set for it, and its summary says `lead_in`; so the
two can be compared without a sound:

    lamp-voice provider-check /etc/lampOS/voice/provider.json --tool-turns 5
    LAMP_TOOL_LEAD_IN=on lamp-voice provider-check /etc/lampOS/voice/provider.json --tool-turns 5

`--hand-overs ROUNDS` runs #854's requests instead (`tool_turns::HAND_OVERS`):
a story asked of Claude Code, the comma version, the plain imperative, a
poem, one asked in the middle of a story the lamp is telling, one asked of
Claude Code after the lamp told a story itself, the two named poems as they
were heard on 2026-10-16 ("Claude Co", "Claude"), work on the office
computer with nobody named; and the lamp's own, which
it should not hand over (labels beginning `own_`): a short story, the
eight-sentence story offered to Claude Code on 2026-10-16, the same after a
hand-over and its yes in one session, and three stories in a row. Only the
last line of each request is recorded, the lines before it setting the
scene: three stories in a row shows the third, after two in the session.
Each summary line counts per request how often the model called what was
expected (`as_expected`), how many of those spoke before the call
(`lead_ins`), how often what the lamp said named Claude Code
(`offered`), and says `asked_switch` and `lead_in`:

    lamp-voice provider-check /etc/lampOS/voice/provider.json --hand-overs 5
    LAMP_TASKS_ASKED=on lamp-voice provider-check /etc/lampOS/voice/provider.json --hand-overs 5
    LAMP_TASKS_ASKED=on LAMP_TOOL_LEAD_IN=on lamp-voice provider-check /etc/lampOS/voice/provider.json --hand-overs 5

`provider-check` declares the functions with one of the staff (`who`
off), so the length that left lamp-4ace's sessions without functions does
not arise there; the test above covers it.

**From the lamp's own turns:**

    lamp-voice turn-times SESSIONS [FIRST_OUTPUT]

reads the session records under SESSIONS (`<session>/metadata/events-*.jsonl`)
and, if given, the provider worker's first-output files under FIRST_OUTPUT
(`LAMP_FIRST_OUTPUT`), and prints one line a turn, every time in
milliseconds after the voice decided the person had finished
(`local_endpoint`):
- the model's first output and what it was (`audio`, `output_transcript` or
  `tool_call`);
- the first function call and when our answer was sent (from the
  `voice_tool_call` lines, [attention.md](attention.md#the-record-of-whom-it-answered));
- the model's first audio (`provider_first_audio`);
- the first write the speaker accepted (`speaker_first_write`);
- how the turn ended.

Then medians for turns with a function call and for plain ones, with each
step of the first: the call, our answer, the model's audio after it, and the
sound. A turn whose first audio came before our answer was sent is marked
`audio_before_answer`: its audio after our answer is not in the records, so
it is left out of that median, and its first sound counts as any other's
(`audio_before_answer` in the summary counts them). One whose first audio
came before the call itself, a lead-in, is also marked `lead_in`
(`with_lead_in`); audio between the call and our answer is not a lead-in
and is left out all the same (the reviewer on #810, 18:33: taken for the
answer's delay it read -2 ms). The records are read in two passes, the turns
first and then what came of them, the earliest of each, so the order the
service's numbered files are read in does not matter. The time our answer
was sent is taken as it is sent, before its journal line. A turn whose only calls are `not_for_me` or `heard_my_name` is plain:
those say whom the model heard, not something to do. A first-output line is
matched to the turn of its number whose end of speech came at most a minute
before it, since turn numbers begin again each time the voice starts. Every
time is the lamp's monotonic clock, and nothing is read but numbers and
function names: no words heard or said are printed. Records written before
`voice_tool_call` lines existed still give each turn's first output (a call
when it came first) and its sound.

Each request the microphone heard is a row: one with an end of speech
(`local_endpoint`), and one admitted with heard audio
(`prefix_first_host_read_us`) but cut before its end of speech was decided.
A cut one is marked `cut`, has no times after an end of speech, and is
counted apart (`cut`) from the summary's `turns` and medians. The lamp's
own lines (its hello, a reminder, a look, back online, the head-tap hum)
are admitted with no heard audio and are no row, so the person's first
request after one is still the run's first. Each row also says whether it was its run's first
request admitted (`first_of_run`) and how long nothing went on before it
(`idle_ms`): from the end of the request before (its `turn_finished`, else
its first sound, else its end of speech; 0 if the request before had not
ended yet), or from the run's `listening_ready` for its first; none for a
first request in a run with no `listening_ready` line. `stalled_ms` is the
longest `capture_gap` it held. A gap is stamped when it ended and goes to
one request only, the latest admitted whose window holds it; a window runs
from admission to `turn_finished`, or, with none, to the next row's
admission; a run's last request with no `turn_finished` holds every gap to
the end of its run's records. The summary's `by_idle` keeps a run's
first requests, requests after at least five idle minutes (`long_idle_ms`)
and the rest apart, each with its requests, cut ones, answered ones, median
first sound, stalled ones and longest stall (#1608: the first request after
about six idle minutes stalled the microphone 135 and 172 ms). It records
only; nothing the voice does changes.

## Not shown

Task 31's new fixtures use synthetic provider events, real local Actions and
Schedule sockets, the durable consumer, and Voice's owner/playout handling.
They do not qualify the six new schemas on the installed model, faithful
spoken reminder wording, room admission or final acoustic completion on a
lamp. Its separate opt-in contract and lamp cases are in
[schedule.md](schedule.md#task-31-lamp-qualification). The older counts and
lamp observations below concern the previously existing tools.

Host tests: the router and the executive's client against a scripted
executive (six tests), the worker against the scripted provider with that
executive and with a memory on a socket (four), what a session is sent at
setup (one). Against the real service, silent written requests (the table
above, and one request to change the light's colour that called the
sample function with the colour asked for and then said "Got it! Blue it
is."), and on lamp-4ace on 2026-10-13 two spoken requests to the real
executive: "Hey Lamp, can you turn your volume down a bit?" called
`volume_down`, which answered in 84 ms, and the lamp said "Got it. How's
that?"; "Lamp, speak up a little, please." called `volume_up`. A request
that calls a function is answered about a second later than one that
does not: the model takes two turns. Not yet shown: that this model calls
`not_for_me` for overheard speech and not for speech meant for it. It
has been seen to call it for speech meant for the lamp: a new session's
first request, "Hey Lamp, how do I take my coffee?", before that session
had been told what the lamp was asked to remember. The memory's three
functions were each called once by the real service on the lamp
([memory.md](memory.md), "Played on the lamp").
