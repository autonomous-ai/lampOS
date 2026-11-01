# Building the brain: the proposed final record

Written 2026-10-13 by the owner of the voice runtime, in answer to the
"lampOS V2 brain architecture review", drafts 0.5 and 0.6 of the same day
(called "the page" below). This is the short record the build works
from; the page is the long reference and is amended to agree with it.

**First of all, and the product owner's standing word:** "always remember
the interface (voice) must be perfect ... if we can't talk / converse with
lamp there is no point of great capabilities ... lamp must be really a joy
to hang around with." Whatever is built here is put on a lamp only if the
conversation is as good with it as without it, and that is measured each
time.

**Status: settled by the product owner on 2026-10-13, at about 18:55
local.** Every decision below has their word, given through that evening:
one builder for all of it, with the page's author reviewing and writing
the failure tests; the experience first and setup, updates and the rest
"the last steps before shipping to customers"; the remaining decisions
"approve all" as they stand here; and four that they changed from what
was proposed, each marked where it is written (10, 12, 14 and 15). A
decision changes only on the product owner's word, and the change is
written here first. They asked for building to begin at once: "so you can
start buildign the next 24 hours".

Also theirs: the lamp is in Ho Chi Minh City, as its character says, and
a key for the decision model's provider, which is kept off the lamp's
repository and out of every record.

Everything said about the lamp is from lamp-4ace on 2026-10-12 and 13;
where a number is one run or a silent probe it says so.

## Decisions

Who builds what:

1. **One builder.** The voice owner builds all of it, end to end: what a
   person at the lamp experiences, through handing work to an agent and
   getting back to them, and then what nobody talks to (starting at
   power-up, setup, network repair, configuration, updates, reset, the
   operator's pages, V1's data). The page's author keeps the page,
   reviews each step after it is pushed and writes the failure tests
   without reading the code under test. That costs the build no waiting,
   and several faults reached the office on 2026-10-13 that a second
   reader might have caught.
2. **The rules that were to be an interface between two builders** hold as
   the runtime's own (below): who writes configuration, when the voice may
   be restarted, how an update hands over the hardware.

How a request is decided:

3. **Chat:** the Live model answers. No decision model.
4. **A clear device command:** the Live model's tool call, checked by the
   runtime. On the Live route a transcript is never a command.
5. **A task for an agent:** it goes to the agent the person named, or
   else to the one in focus in the Harness app on their computer, which is
   the Harness contract's own way of choosing. Only where that leaves a
   real choice does the runtime build the valid options and ask the
   decision model, which picks one or abstains. The voice does not wait
   for it.
6. **Nothing was said** (someone sat down, a task finished, the room went
   quiet): a rule or the decision model decides, and if the lamp is to
   speak the Live model speaks from a note.

The product:

7. **Addressing.** No wake word and no tap. In a room that is talking the
   lamp answers only when called; in a quiet room it answers what is said
   near it. Anything that leaves the lamp needs a spoken yes until a
   false-accept rate has been measured and accepted.
8. **Promises.** A neutral acknowledgement in the native voice; "sent",
   "done" and an agent's question are spoken from the receipt.
9. **The provider down, at launch:** an honest "voice is down" state with
   light and controls working. The second speech route (Deepgram, a text
   answer, speech synthesis) is later work and does not block launch.
10. **Senses: all of them, without limit.** Proposed was a camera kept to
    noticing a face. The product owner's answer: "you can use all the
    sensors with full permission ... including the camera. do everything
    and come up with amazing cool features with the camera. it's one of
    the most powerful sesnses. the eyes! don't limit it." So both
    microphones and the camera for whatever serves the experience: who is
    there, whether someone is looking at the lamp when they speak, what is
    shown to it, what is on the desk. What stays is a matter of speed and
    not of permission: a picture in a session slowed every later answer by
    about 0.45 s, so what can be seen on the lamp is seen on the lamp, and
    a picture goes to the provider when an answer needs it.
11. **Motion at launch:** yes, what runs today, plus starting at power-up
    and a hands-on check of what the servos do at power-up.
12. **Memory across days** (proposed was one day). One memory to begin
    with; one for each person once the lamp can tell who is in front of
    it, and then it does not tell one person another's conversations. It
    keeps what was said with it and what it is told to remember, on the
    lamp, and forgets on being told to ("forget that", "forget
    everything").

Engineering:

13. **Audio leaves the lamp only after local admission.** No second cloud
    listens continuously.
14. **Agents run off the lamp, through Harness, and Harness runs on the
    lamp.** Proposed was a client on the lamp paired with one computer by
    V1's protocol. The product owner had Harness itself installed on the
    lamp and signed in to their account on 2026-10-13, so the lamp is one
    of that account's machines and reaches the agents on the others
    through Harness's own commands. Nothing of V1's pairing and encryption
    is ported. The first task goes to the computer the lamp sits beside. A
    permission prompt stays on the computer it appears on.
15. **Publish and subscribe for everything outside the audio path,** on
    one small local bus, built when the first thing that only observes
    needs it (the operator's pages, a recorder) and not before. Each topic
    has its written definition before it flows. The microphone, the
    provider and the speaker stay joined directly. Publishing never blocks
    or fails the publisher.
16. **No decision model on the spoken path** until it wins on recorded
    requests against the Live model's own tool call.
17. **Everything is a skill** (the owner, 2026-10-19 06:4x and 07:2x): "do it
    all no adhoc code everything is a skill", with "core skills and
    installable skills ... core skills like core apple apps right always
    installed on the phone and then installable skills like installable
    apps". Built in, below the skills, stay only the conversation, reflexes
    under 150 ms and system parts (setup, updates, health, the privacy
    switch). Each V1 capability is decided against V2's direction before it
    is built ([v1-verdicts.md](v1-verdicts.md), "The owner's scope
    decisions, 2026-10-19").
18. **No lamp has shipped, and updates carry every later version** (the
    owner, 2026-10-19 07:1x). V2 is installed on the current lamps before
    they ship. Nothing migrates from V1, and a shipped lamp must update in
    place to V3, V4 and V5: the units, the models, the updater itself,
    rollback, and every part's store read across versions (#1680).
19. **Privacy is the physical switch** (the owner, 2026-10-19 07:2x): "when it
    comes to privacy, the switch is definite". There is no camera-off or
    mute by voice.

## How a request is decided

Three things run side by side and none waits for another except where the
table says so.

| What happened | Who chooses | Decision model | Does the voice wait? |
|---|---|---|---|
| Chat | The Live model | No | No |
| "Lamp, dim a little" | The Live model, by calling one of the tools it was given; the runtime checks that the tool was offered, the numbers are in range and the lamp was addressed, runs it, and returns the result | No | Only for the result of a local call, which is milliseconds; "done" is said from it |
| "Ask my coding agent to fix the tests" | The Live model calls one general tool with the request as said; the runtime sends it to the agent that was named, or else to the one in focus in the Harness app. With no such agent, or more than one that fits, the decision model picks one, asks for the missing detail, or abstains | Only when there is a real choice | No. The voice says a neutral "on it" and stays available; the task waits for the choice |
| Nothing was said | A rule, or the decision model | Yes | There is no turn. If the lamp is to speak, the runtime gives the Live model a note to speak from |

One request has one chooser and is carried out once. The page's 0.6 text
also matches the Live model's transcript against V1's command catalog and
then guards against the tool call and the transcript both acting. With the
catalog given to the model as its tools there is nothing to guard: on
lamp-4ace that transcript rendered "Lamp" as "Jelani" and Vietnamese as
Spanish, and many turns had none. V1's matcher, with the decision model
behind it, stays for the second speech route and for typed requests, where
text is all there is.

Not built: the runtime rejects tool calls today. Not known: what declaring
tools costs ordinary answers (the provider's own search tool cost nothing,
two runs each way), and whether a model with tools acts more readily on
speech it only overheard, which is why decision 7 asks more of an action
than of chat.

## Components, processes and owners

The product owner's instruction for the build, 2026-10-13: "design them
by components and subsystems and clear runtime processes. good software
engineering practices. we'll assign team members to own different
components/subsystems. design it so that if one component fail it doesn't
affect the whole system."

Each component below is its own process under its own systemd unit, with
its own crate or directory, one written interface to each neighbour, and
a place for one person's name. A component is told what it needs over a
local socket or a small file and never reaches into another's hardware or
memory. The last column is the test each one has to pass on a lamp: it is
killed, and what the others do is counted.

| Component | Process | Owns | Owner | With it gone |
|---|---|---|---|---|
| Voice | `lamp-voice` and its workers (capture, speaker, ring, provider) | Both microphones, the speaker, the ring light; the conversation with the provider; whose turn it is | open | The body rests and the eyes go on looking. Running today |
| Body | `lamp-body` | The servo bus; standing, turning to a face, gestures; the force its joints may use | open | The lamp talks and does not move. Running today |
| Expression | `lamp-expression` ([expression.md](expression.md); built by the third builder) | The plan of what the lamp shows while it listens, thinks and speaks: posture, glances, nods and the light on one clock, as requests to the body and the ring's owner that run out unless renewed | open | The lamp moves and lights as it does without it, by the voice's own state. On lamp-4ace since 2026-10-14, watching only: nothing takes its requests yet |
| Eyes | `lamp-eyes` | The camera; whether a face is there, where, and later whose; the picture for a look | open | The lamp talks and moves without looking at anyone, says no hello, and says its eye is shut when asked to look. Running today |
| Actions (the page's executive) | `lamp-actions` ([actions.md](actions.md); what the voice expects of it is in [tools.md](tools.md)) | Every action the Live model asks for: checking it, asking "shall I?" where decision 7 requires it, sending it to the component that carries it out, and returning what was established so that it can be said | open | The lamp chats and can do nothing it is asked to do, and says so. Running today, for its volume and its light |
| Tasks | `lamp-tasks` (new) | Work handed to agents through Harness: what was sent, to whom, what came back, what has been said of it | open | The lamp chats, and says it cannot reach the agents just now |
| Harness | the Harness team's own service, installed on the lamp | The account's session and the way to its other machines | Harness team | As for Tasks |
| Memory | `lamp-memory` ([memory.md](memory.md)) | The journal, what it was told to keep, a store for each person, forgetting | open | The lamp talks without recalling earlier days, and says so. Running today; its next layer ([memory-days.md](memory-days.md)) is on in the product: the unit's `--memory-days`, and the voice's `LAMP_MEMORY_DAYS=on` |
| Controls | `lamp-controls` (being built by the third builder) | The touch pad and the button: stop, mute, privacy | open | The lamp is spoken to as before and cannot be tapped |
| Schedules | `lamp-schedule` ([schedule.md](schedule.md); built by Third, off unless enabled) | Durable reminders, elapsed timers and pending delivery receipts | open | The lamp says it cannot set one just now; pending work survives and is offered late after recovery. First still wires Voice to retrieve and acknowledge it |
| Name | `lamp-name` ([name.md](name.md)) | Hearing "Hey Lamp" and "Lamp ơi" in what the microphone hears, and saying so on the bus. It decides nothing | open | The lamp talks as before. A new session in a busy room goes by the service's text alone, and no second look is taken at a call that got nothing ([attention.md](attention.md)). Running today |
| Policy | `lamp-policy` ([policy.md](policy.md); this table's first form called it Decisions, `lamp-decide`) | The choices where there is a real choice or nobody spoke. One policy makes each choice; it may ask several decision models, each a judge behind one common way of asking (the `Decider` interface in `crates/policy/src/decide.rs`), and no judge acts by itself. Judges are not on the bus: the Policy alone listens to the lamp's topics, asks each judge within one deadline and publishes one decision on `policy/decisions`. The first judge is Jev; a local judge (#700) implements the same interface and is not yet usable (fitted on 89 played lines, it holds back every call). For now only a record: what it would have decided about each thing heard, beside what the lamp did | open | The rules alone decide, as they do today: it decides nothing yet. Running today, watching only |
| Health | `lamp-health` ([health.md](health.md)) | Every part's own report gathered into one answer to "how is the lamp", and the board's temperature, load, memory and disk | open | Each part goes on as it was, and nobody is told how the lamp is. Running today |
| The bus | No process of its own: `lamp-bus` ([bus.md](bus.md)) is a library in each publisher and subscriber and a tool; on lamp-4ace since 2026-10-13, with twenty topics on 2026-10-14 | Carrying what each component senses and derives to whoever subscribes | open | There is nothing to be gone: a publisher or subscriber that dies takes only its own topics or its own copies with it |
| The surround | setup, network, updates, reset, the operator's pages (last) | Getting a lamp onto a network and keeping its software current | open | The lamp goes on as it is until it next needs one of them |

What keeps one failure from becoming all of them, and each is a rule the
code is held to:

- **No component waits on another to do its own work.** What it sends is
  sent without blocking; what it needs and does not get, it does without,
  in a way a person at the lamp can tell.
- **One writer for each piece of hardware,** and that writer is the one
  that stops it. Two processes never hold the speaker, the ring, the
  servo bus or a microphone.
- **Each is restarted by itself.** Its unit restarts it; none is restarted
  because another was. A restart of the voice waits for a moment when
  nobody is speaking (below).
- **Each says whether it is ready,** and nothing is inferred from a
  process being alive.
- **Each keeps its own records** and none reads another's files.

Today the voice is one unit with its workers inside it, and a worker's
death ends the service. Step 2 changes that.

The Executive was not in the first form of this table. The product owner
asked whether every component of the page would be built, and going down
the page's list showed its "executive" had no process of its own here:
its work would have gone into the voice, which is the one thing that must
not become what everything else depends on. The page's fourteen roles map
onto these eleven processes; "attention" and the "voice router" are work
inside Voice, "perception" is work inside Eyes and Voice, "behaviours" are
work inside Body, and the "situation" begins as the facts told to the
voice (step 5) and becomes a process of its own with the bus.

## Rules between the runtime and what surrounds it

Written when two builders were proposed; kept as the rules the one
builder holds between the parts. "The surround" is everything that is
not the voice, body and eyes: setup, updates, the operator's pages.

- **Configuration and credentials.** The experience runtime reads, at
  start only: one provider file (endpoint, model, a reference to the key,
  language, switches), one character file, and its unit's options. The
  surround writes them, whole or not at all, with a schema version. The
  runtime never writes them.
- **Restarting the voice.** Whoever restarts it first checks that the lamp
  has neither heard nor said anything for 45 seconds, and does not restart
  it otherwise. On 2026-10-13 a restart and a link cut made without that
  check landed on a person speaking to the lamp, twice.
- **The bus** (decision 15). The runtime publishes to it and reads from
  it only what it has asked for. Until it exists, the items below are
  files.
- **The door for Harness.** The computer dials the lamp on port 80. The
  surround owns that port, nginx and the mDNS advertisement, and passes
  `/api/harness/ws` untouched to a local socket the runtime listens on. The operator's page starts pairing by asking the runtime, which
  answers with the code to show.
- **Ready.** The runtime states each capability separately, in one small
  file the surround may read: voice (listening, starting, or down and
  why), body (up, or stopped and why), eyes (looking or shut), tasks
  (paired, reconnecting, or unpaired). Nothing is inferred from a process
  being alive.
- **Updates.** Releases are directories named by the binary's hash, as
  now. The order is: voice stopped (after the check above), body stopped
  at rest by its own stop command, eyes stopped; binaries switched; body,
  eyes, voice started; any failure returns to the previous hash. The
  microphones, the speaker, the ring and the servo bus each have one
  writer at any moment, and a servo keeps its last goal after its process
  has gone, so the body is at rest before anything is switched.
- **Records.** The runtime writes its own bounded records and nobody else
  writes there. The surround may read and export them and owns how long
  they are kept. What they hold is in
  [serve-runtime.md](serve-runtime.md).
- **Forgetting.** For each store of the owner's that the runtime keeps
  (the conversation it carries, later the day's journal and the task
  records) it offers one command that empties it. Reset calls those.
- **Each side's own:** the surround never opens a microphone, the
  speaker, the ring or the servo bus; the runtime never writes network
  settings, update state or V1's data.

## Amendments to the page

By the page's own section numbers.

- **02, 03 (the loop, component roles).** Attention does not admit by
  meaning before the provider. Local evidence narrows what is sent; the
  Live model decides whether it was for the lamp. Add an addressing slice
  with a recorded set of office situations and two rates to meet, and the
  rule that the evidence asked for rises with the consequence (decision
  7).
- **04, 06 (services, walkthroughs).** A quick device call and a task
  proposal reach the runtime as the Live model's tool call (decisions 4
  and 5). Scenario 02 loses its "deterministic exact command path" on the
  Live route and keeps it on the second route.
- **05 (topics).** Providers subscribe to admitted speech, not to a
  microphone (decision 13). The voice microphone's own processing evens
  loudness out, so nearness cannot be judged from it; the plain microphone
  may carry it, hears the lamp's own voice at -13 to -17 dBFS, and is
  blanked while the lamp speaks. Observers run below the voice's real-time
  priority. The bus and what stays off it are decision 15.
- **07 (decision models).** Decisions 5, 6 and 16.
- **08 (voice).** The commitment policy is decision 8, by the three
  mechanisms under "For commitments" below. Add what a session may be
  told, and when, as an interface with a first-turn test for each item.
  The first fallback is decision 9.
- **11 (memory).** Decision 12.
- **13 (migration).** The state of the voice code, under "What the page's
  review of the voice code predates" below.
- **14 (build plan).** The two build orders below. Add to the acceptance
  measures the baseline to hold (first sound 1.3 s after the lamp decides a
  person has stopped, about 1.9 s after their last word) and a test of what
  every new session does with the first thing it hears.
- **15 (decisions).** Closed as above once approved.

## Who builds what

Since 2026-10-14 there are four builders. The first, with the lamp, is
the one this section was written about. Three more, without a lamp, build
parts to a written brief on branches of their own, and the first puts
each finished part on the lamp and mends there what fails: the list,
the rules and what the lamp showed of each part are in
[work-split.md](work-split.md). What follows is how it stood before.

The product owner's decision, in their words: "i'm leaning toward to have
you build the entire thing end to end so no communication overhead between
you and codex. you own the experience entirely", and, on why: "you have
full context. you understand lamp. you have everything setup mic speaker
permission etc."

What it costs, said to them before they decided: one builder works in
series. By estimate and not by measurement, the steps of the experience are
four to five weeks, two of them the Harness client; a first user-ready set
of what nobody talks to is three to four weeks more. On lamp-4ace none of
that set is running today: V1's services are gone from the unit and its
web server is disabled.

The Harness computer side, the agent engines, the provider and Jev stay
where they are. The lamp's Harness client is built to the Harness team's
existing contract ([../../docs/harness.md](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/main/docs/harness.md)); nothing
here invents a pairing flow, a transport or a credential.

## The evidence behind four of the changes

### 1. Whether speech is for the lamp is a measured problem with its own slice

The page draws attention as a component that admits input before the
provider and lists "how does Lamp know it is being addressed?" as a product
decision. On the lamp nothing can admit by meaning before the provider
except a wake word or a tap, which the product does not want. What decides
is the Live model's own judgment of the audio, narrowed by local evidence
that costs nothing: how loud, how long, whose turn it is.

What that gives, in a working office, from the lamp's own record:

| Setting | Heard | Answered |
|---|---|---|
| Level floor -40 dBFS, character answers what seems meant for it (about 50 minutes) | not counted | about 30, to colleagues |
| Floor -28, character answers only when called or already talking (16:05 to 16:37) | 87 | 6, of which 4 to someone who came and talked to it |
| The same with the fault described in change 2 (16:37 to 16:47) | 70 | 12 |
| The same, what it remembered cleared by hand (16:47 to 16:55) | 71 | 2, each the first thing a new session heard |
| The same, repaired build (16:55 to 17:15, the office emptying) | 25 | 0 |

Two things follow for the architecture.

- A wrong answer today is an unwanted sentence. Once a request can become
  work on someone's computer it is an unwanted task. So the evidence asked
  for must rise with the consequence: chat on the model's judgment; a
  change to the lamp itself (light, a look) when called by name in that
  utterance or inside an exchange; anything that leaves the lamp also on a
  spoken yes, until a measured false-accept rate says otherwise.
- The local evidence is not what the page assumes. The voice microphone's
  own processing evens loudness out: colleagues talking near the lamp (how
  near is not known) arrived at a median of -22 to -27 dBFS, and a person
  speaking to it at its desk at -19 to -24, so a level floor cannot tell
  them apart. The second, plain microphone rose only 2 to 8 dB over its
  resting -51.5 dBFS for those colleagues (ten minutes, levels only). It
  has not yet been compared with a person at the desk. If it separates them
  it is the proximity cue, and the reason to carry both microphones as
  topics.

Change: an addressing slice before the executive, with a recorded set of
office situations, the two rates to meet (uninvited answers per hour of
office talk; calls by name answered), and the rule above tying evidence to
consequence.

### 2. What a session is told is an interface, and every field of it is tested on a first turn

A Live session lasts 3 to 39 minutes here and its successor knows only what
it is given at opening: the character, the time of day and a note of the
last exchanges ([conversation-thread.md](conversation-thread.md)). The page
adds a shared situation, pending questions and task state to that. All of it
is read by the model as a cue to act.

What happened: the note said "this is the conversation you are in, carry
on". With a character that answers only when called or already talking,
five new sessions in a row answered the first or second thing they heard,
which was colleagues' talk. Each wrong answer became the newest exchange
told to the next session, and the fifth answered nine times in two minutes.
A safety net that renewed a session after eight unanswered requests made it
worse: in an office a lamp that rightly answers nobody is nothing but
unanswered requests, so it renewed every three minutes. Both are changed
(the note now tells an exchange older than 45 seconds as finished; the net
acts at most every fifteen minutes). Since then fourteen new sessions have
spoken to nobody, but only three of them heard two utterances before the
office went quiet, so this is not yet shown.

Two more facts of the same kind:

- Of six new sessions with nothing told to them at all, two answered the
  first sentence they heard, a colleague's to a colleague ("Está bien,
  gracias." got "You're welcome!"). Being new is itself a cue. A switch
  now lets each new session be told, in a turn that asks for no answer,
  that nobody is talking to the lamp at the moment
  (`crates/voice/src/session_fact.rs`); it is off, and what it does to a
  first turn is unmeasured.
- One picture anywhere in a session made every later answer about 0.45 s
  slower (medians 1.32 s and 1.78 s over six balanced runs). What is put
  into a session can cost speed for the rest of it.

Change: list what may be given to a session (at opening, and while it runs,
below), who may add to the list, and require for each item a test of what
the lamp does with the first thing it hears afterwards and a speed
comparison with and without it.

### 3. For commitments, split the speech; do not hold the audio

The page says strict commitments need the relevant speech "held or
generated from approved text before playback" and leaves the policy open.
Holding native audio until its meaning is known delays every answer; the
lamp answers 1.3 s after it decides a person has stopped (about 1.9 s after
their last word) and that is the experience to protect.

Three mechanisms already in the voice runtime do the job without a hold:

- **A turn the lamp begins from a note.** One complete written turn in
  place of speech (`InputSender::try_written`,
  [speaking-first.md](speaking-first.md)): the model says in its own voice
  what the note gives it to say. V1 delivers Harness results the same way
  ([../../docs/harness.md](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/main/docs/harness.md), "Spoken Harness
  updates"). Silent probe: a hello 0.91 s after the note.
- **The lamp's own opening words as a signal.** When its answer begins
  "Let me look", the runtime takes the look
  (`speak_up::asks_to_look`). Silent probes: 4 of 4 seeing questions opened
  that way. The same shape fits "on it": a fixed neutral phrase that
  promises nothing and tells the runtime a request was understood as work.
- **A fact given without asking for an answer.** One written turn left
  incomplete (`InputSender::try_context`, new on 2026-10-13). Silent probes,
  one run each: told a name this way the lamp said nothing back, then
  answered "do you know my name?" with it, 0.93 s after the question;
  without the fact it said it did not know.

Change: the policy is a neutral acknowledgement in the native voice, and
"sent", "it needs to know…" and "done" are spoken from the receipt through a
written turn. The test is a count: over N task requests, how often the
native voice claimed something before its receipt existed. None of the three
has yet run through the coordinator on a lamp with a person listening.

### 4. A small bus beside the audio path, and Jev starts where no utterance does

The page's topic graph (19 families, a 12-field envelope, a queue per
subscriber) is right as vocabulary. The lamp today has several processes that
already pass latest values and ordered events to one another by the
simplest means that fits each: a datagram for the body's mood, a small file
for whether someone is there, a bounded channel for audio, a record on disk
for outcomes. Each is a topic in the page's sense.

The first form of this change was to write each topic's definition and
give it a transport only when a second consumer existed. The product
owner asked why not publish and subscribe outright. What a real bus
gives: things that only observe (the operator's pages, a recorder, the
shared situation) subscribe without a custom output being added to the
voice code each time, and every topic can be recorded and played back
with one tool, which is how the fault of 16:37 on 2026-10-13 would have
been found sooner. With two builders it was also the cleanest join
between them, and was to be built first for that. With one builder that
reason is gone and the others stay, so the bus is built when its first
observer is.

Change, as it now stands (decision 15):

- One small local bus, with no ROS or DDS under it. Two kinds of topic
  only: the latest value (whether someone is there, the body's state, how
  the room is, what is ready, a sensor's reading) and ordered events (what
  was heard and said, a task's receipts and results, a touch).
- Each topic's definition is written first, as the page does: name,
  fields, how fresh, what is lost when a subscriber is slow and how that
  is known.
- The microphone, the provider and the speaker stay joined directly, and
  so do the voice's own workers. The bus is told what happened there; it
  is never in the way of it.
- Publishing never blocks and never fails the publisher. With the bus
  down or a subscriber slow, the voice carries on and the message is
  dropped and counted.
- What the lamp passes by simpler means today (the body's mood, whether
  someone is there) moves onto the bus when the bus exists, one topic at
  a time, with the old means kept until the new one has run on a lamp.

For Jev: a spoken request already reaches the runtime as the model's choice
among the tools declared to it. Putting a second model behind that choice
adds a hop and a second opinion, and should be shown to help on recorded
requests before it is built in. Where no utterance starts the decision
there is no such choice, and that is where a decision model has work from
the first day: whether to speak up, which agent gets a task, when a result
may interrupt. The lamp's rule for saying hello is the deterministic
baseline to beat ([speaking-first.md](speaking-first.md)).

## The page's seven open decisions, and where each stands on the lamp

| Decision | Proposed answer | State on the lamp |
|---|---|---|
| How does it know it is addressed? | No wake word, no tap. In a room that is talking: when called ("Lamp", "Hey Lamp") or inside an exchange. In a quiet room with someone at its desk: whatever is said near it. Audio reaches the provider before the decision, because the provider makes it; say so plainly to users. Targets to agree: at most one uninvited sentence an hour in a talking office, and 19 of 20 calls by name answered. | Called-only is running. Quiet-room listening is not built; as silent probes with a written line in place of speech, one run each, the fact "the office has emptied" made it answer an uncalled question it otherwise left, and "the office is full" got "..." with no sound. |
| Which senses at launch? | The voice microphone for speech; the plain one for how near a voice is, if the comparison holds; the camera on the lamp only, for whether a face is there and where, plus one picture sent when the lamp has said it will look. No recognition of who, no reading of mood. | Both running except the second microphone, which nothing reads yet. |
| Is motion at launch? | Yes, what runs now: it stands when spoken to and folds when left, turns up to 44° to a face, looks away to think, and makes six small gestures chosen from the first words of its answer. | Running. Not yet: started at power-up, behaviour at power-up checked by hand, a long hold measured (the shoulder warmed 42 to 46 °C in eight minutes). |
| Agents off the lamp? | Yes. No general agent on the lamp. | Nothing to change. |
| Voice commitment policy? | Change 3. | Pieces exist; unproven aloud. |
| First decision model and engines? | No decision model on the spoken path until it wins on recorded requests; Jev first for decisions no utterance starts. Engines as the page says: the V1 one first, a second through the same contract. | Not started. |
| First Harness scope? | One paired computer, the existing contract, permission prompts stay on the computer. | Not started in Rust. |

## Build order

Changed on 2026-10-13, the evening it was settled. The product owner asked
why the bus was not first ("that to me is the core of all the sensors we
have") and showed the page's drawing of it: each sensor publishing its
stream once, any number of subscribers, and consumers publishing what they
derive. So the bus is first and the sensors go onto it second. Every row
of the page is placed against these steps in
[brain-coverage.md](brain-coverage.md). Each step ends with something a
person can try at the lamp, or a component that can be killed on the lamp
while the others are counted.

| Step | What | In a line |
|---|---|---|
| 0 | The bus | Publish and subscribe with no process in the middle (`lamp-bus`, [bus.md](bus.md)) |
| 1 | The sensors on the bus | `mic1` from a driver of its own, a copy of `mic2`, the camera's newest frame, `servos`, `people` |
| 2 | The lamp outlives the provider | No exit for a refused quota, a stalled link or a dead worker |
| 3 | The executive and the tools | `lamp-actions`; V1's 21 commands as the Live model's tools. Try: "Lamp, dim a little." |
| 4 | Memory, and who is there | `lamp-memory`; faces recognised, one memory for each person |
| 5 | One task, out and back | `lamp-tasks` through Harness on the lamp. Try: "Lamp, ask my coding agent to…" |
| 6 | Questions and interruptions around a task | The agent's question asked at a pause; stopping speech never cancels work |
| 7 | The situation, and attention with eyes and both microphones | `lamp-situation`; whether someone faces the lamp when they speak; how near a voice is |
| 8 | Decisions, goals and the owner's feedback | `lamp-decide` with Jev behind it; what the owner teaches, corrects and approves; a second engine |
| 9 | Controls | `lamp-controls`: touch pad, button, privacy switch |
| 10 | Schedules | `lamp-schedule`: reminders and timers |
| 11 | The second speech route | Deepgram, a text answer, speech synthesis ([second-voice.md](second-voice.md)) |
| 12 | The surround | Starting at power-up, setup, network, configuration, updates, reset, the operator's pages, V1's data |

Where each stood at 20:10 on 2026-10-13: step 0's library and tool
written and passing their tests on the host and as a Linux ARM64 build
under emulation, with nothing publishing yet; Memory (part of step 4) the
same; step 2's kept link and its stall fix on lamp-4ace since 19:48, where
26 staged cuts of the link in two five-minute runs restarted nothing; step
3's first half written and tested on the host and not against the real
service: function calls in the provider link, and in the voice a router
that answers every call, hands the executive's to its socket with a 1.5 s
limit, and gives the model a `not_for_me` it can call instead of speaking
([tools.md](tools.md)), switched off; the room told to the voice
(part of step 7) on the lamp, where two played runs of twelve new sessions
each showed no difference between telling a session how the room is and
not telling it. Nothing else is started.

Where each stood at 06:40 on 2026-10-14, all of it on lamp-4ace: the bus
(step 0) has fourteen topics: what the voice is doing, heard and said,
how the room is, the body's joints, the faces the eyes track, each part's
own report and the lamp's health made of them, the board's state, the
light asked for and shown, the name heard, the Policy's decisions, and,
behind a switch of its own, what the second microphone hears (part of
step 1; the first microphone and the camera's frame are not on it yet). The executive (step 3) changes the lamp's own
light and volume when asked aloud. Memory (step 4) keeps and forgets what
it is told; nobody is recognised. Of step 7: the name is heard in the
sound by a part of its own, the voice lets an answer out on the strength
of it and tells the model when it stayed silent to someone who called
([attention.md](attention.md)), and the eyes report whether a face is
turned to the lamp, which only the watching Policy reads yet. Of step 8: a decision model
watches and is logged, and was measured too slow through its present
route to hold back an answer ([policy.md](policy.md)). Health is new and
outside the steps. Steps 5, 6 and 9 to 12 are not started.

What that night's recordings showed about the voice itself is in
[duplex-listening.md](duplex-listening.md): interrupting the lamp is
limited by its second microphone's own clock and processing, not by a
rule, and no figure for it yet comes from a person.

## What a session may be told

Change 2 asks for this list. A Live session knows only what is on it; each
item is the voice owner's to word, and none is added without a count of
what new sessions in a talking room do with the first thing they hear, and
a speed comparison with and without it.

| Item | When | State on lamp-4ace |
|---|---|---|
| The character | At opening | Running |
| The time of day | At opening | Running |
| The last exchanges, as going on (45 s) or finished | At opening | Running since 16:55 on 2026-10-13; first-turn count not yet enough |
| That nobody is talking to the lamp at the moment | At opening, unless a conversation is going on | Running since 17:29; unmeasured |
| How the room is: quiet or busy | When it changes, and at opening | Built, not installed |
| One picture, with the request to answer from it | When the lamp has said it will look; the session is renewed afterwards | Built, asleep |
| A note to say hello from | When someone sits down after a long absence | Built, recording only |
| A task's receipt, question or result, to speak from | When it arrives, at a pause | Not built (step 5) |
| The day's conversations | At opening, shortened | Not told at opening: decision 12 is built with a `recall` tool instead, and each session is told what it was asked to keep ([memory.md](memory.md)) |

The wording describes how things are and never tells the lamp to keep
quiet or what not to mention: the first did the one and got a spoken
"...", sixteen times; the second got "my connection dropped" said aloud.

## Step 2 in more detail: the lamp outlives the provider

Most of this was designed before the voice was shown to anyone and is in
the code, switched off or unfinished ([provider-availability.md](provider-availability.md),
[coordinator-outage-contract.md](coordinator-outage-contract.md),
[failure-notices.md](failure-notices.md)). What exists: the provider
worker reports its link as ready or unavailable apart from the answers it
carries; the coordinator keeps the microphone, the speaker and local
control through an unavailable link and records what was said then as not
delivered; a notice can be played from a recording kept on the lamp, and
no recording is provided.

What is left, in the order it will be done:

1. **The link is never given up.** Built and tested on the host on
   2026-10-13, not yet on a lamp. Until now four attempts inside fifteen
   seconds, or a fourth loss inside a minute, ended it
   (`RecoveryPolicy::default`), and the coordinator then left ("service
   stopped without retry"): my own cut test did that to lamp-4ace three
   times at 19:22. The voice service now runs its link as kept
   (`RecoveryPolicy::endless`, [gemini-session-reliability.md](gemini-session-reliability.md)).
   A failure that a new connection can cure is retried without end, every
   four seconds once the first waits have doubled that far; a link that
   keeps dropping is replaced after four seconds instead of at once; the
   first connection is retried like any other, so a lamp that starts
   before its network comes up by itself. A refusal for now (quota, rate
   limit) is asked again every two minutes. Only what no retry can cure
   (a wrong key, a wrong configuration, a message the code cannot read)
   still ends the service. The plan said the waits would grow to a
   minute; four seconds was built, because a lamp that stays deaf for up
   to a minute after its network returns is the worse fault for one lamp.
   A fleet behind one relay needs growing, jittered waits; that is not
   built.
2. **A stalled link fails one request, not the service.** Input that
   overruns its queue while a write is stuck ends the worker today
   ("provider unavailable while input was admitted"), as at 16:50 on
   2026-10-13. It becomes that request's failure, like a write that times
   out.
3. **A worker that dies is started again** by the coordinator, a bounded
   number of times, with the microphone, speaker, ring and body kept.
4. **What a person at the lamp notices.** The ring has a face for "I
   cannot hear you just now", which it has not had; spoken to while down,
   the lamp plays the recorded notice once and not again for some minutes;
   it does not stand up for speech it cannot answer. When the voice
   returns nothing is said. The recording, its words and its voice are the
   product owner's to approve.
5. **Ready is stated** in the file the interface above describes.

Gate, on the lamp: the network taken away for five minutes and given
back; the relay's refusal for now, staged; a worker killed. In each: the
service is not restarted by systemd, the down face is on within two
seconds of the link being seen to fail, the first answer after the return
comes within a counted number of seconds, nothing from before the outage
is played late, and the conversation is still known.

## Step 3 in more detail: the tools

Read from V1 on 2026-10-13 (`system/intent/`, `hal/realtime/`); two of the
points below were checked against the code by hand, the rest are one
reading.

V1 already gives its realtime voice model tools, as function declarations
on the Live session (`hal/realtime/orchestrator.py`): one to hand a request
on to the main agent with an instruction in the model's own words, one to
say a turn was not for the lamp, one to end the conversation, one to show
an emotion, one to look, one to search. It gives it no tool for a device
command: those go to the main agent as that instruction, and
`system/intent` matches the instruction's text against its catalog. So
decision 4 removes one hop from what V1 does; it does not invent the
route.

What V2's tool list takes from V1:

- **The catalog as it is:** 21 commands (`system/intent/jev/catalog.go`):
  light on, off, colour, brighter, dimmer; volume up, down, mute, unmute;
  stop the music, stop talking; seven scenes; follow something, stop
  following; the time. Only two take anything: a colour from ten and a
  thing to follow from twenty-three. No numbers: brighter, dimmer, louder
  and quieter are steps the code owns.
- **The model names a command and never a payload.** Each name maps to
  code-owned hardware calls, and whether the lamp can do it is checked
  again when it runs.
- **Read, change, read back** for light and volume, with V1's steps and
  limits, one adjustment at a time, and nothing changed while the lamp is
  asleep.
- **An honest result.** What comes back to the model is what was
  established ("the light is already at its brightest", "I couldn't
  confirm it"), never a success phrase for a failure, and a command that
  was tried is finished: it is not also handed on.
- **What V1 refuses to run locally** stays refused: a negated request,
  quoted speech, two requests in one, another room's device, a future
  time, a bare "brighter" with nothing before it. On V1 these go to the
  main agent; here the model is told the same in its tools' descriptions,
  and the count of wrong calls on V1's own regression cases
  (`system/intent/*_test.go`) is the test.

Two of V1's tools are worth more than their place in this list. The one
that says "not for me" would let the lamp close an overheard request at
once and on the model's word, where today it waits four seconds for no
answer. The one that hands on with an instruction in the model's words is
the general tool of decision 5. Both are to be tried in step 2 against the
same counts as everything else: what they cost an ordinary answer, and
what they do to the first thing a new session hears.

## Step 5 in more detail: tasks through Harness on the lamp

Decision 14 as the product owner settled it. On 2026-10-13 Harness 0.3.94
was installed on lamp-4ace by its own installer, signed in to the product
owner's account in the browser of the computer beside it, and started. The
lamp shows on the account as a machine, with the computer it sits beside
and the account's other machines. The voice was not disturbed. No task has
been sent.

What the Tasks component uses, all of it Harness's own and none of it
ported: starting an agent on a named machine with a first message
(`harness new <agent> @<machine> <project> -- <task>`), listing what is
there (`harness ps`), reading an agent's state and its last turn, what it
was asked and what it answered (`harness show`), waiting until it stops
working, where one exit code means that it needs the person
(`harness wait`), closing what it started (`harness close`), and the same
verbs as a server for programs (`harness mcp`). Harness keeps agents
starting agents off unless the account turns it on; on this account it is
on.

What carries over from the reading of V1 and of the device protocol
below, because it is about the experience and not the wire: a task is
never sent twice; "I cannot tell whether that went through" is a real
outcome with its own words; a result is said once; a prompt for
permission on a computer is said once and never answered by voice; a
spoken yes comes before anything is sent (decision 7).

Not yet known, and the first things step 3 finds out: how a result that
answers two requests looks through these verbs; whether a follow-up can
be given to a working agent from the lamp; what the lamp is told when the
agent asks a question; how long each verb takes from the lamp.

### The other route, not taken: V1's device protocol

Kept because it is how V1 worked and how a lamp could work with a
computer that is on the same network and no account between them. It is
not built.


Read from V1 on 2026-10-13 (`docs/harness.md` at the repository root,
`system/harness/`, about 5,900 lines of Go with its tests); four of the
points below were checked against the code by hand, the rest are one
reading. The contract belongs to the Harness team and its wire schema is in
their repository, which was not read. Nothing here changes it.

What the Rust client has to be:

- **A listener, not a dialer.** The computer finds the lamp by its
  existing mDNS advertisement and opens one WebSocket to the lamp's
  `/api/harness/ws` on port 80, behind nginx. So the join with the other
  builder has one more line: they own port 80, nginx and the
  advertisement, and pass that one path to a local socket the runtime
  listens on.
- **Pairing.** A six-character code from the lamp, good for sixty seconds;
  a password-authenticated exchange (CPace) so the code never crosses the
  socket; the result is the lamp's own signing key and the computer's
  public key, kept in one small file. One computer per lamp. The operator's
  page asks the runtime to start pairing and shows the code; whether the
  lamp may also say it aloud is a product question.
- **Every connection after that:** a signed hello checked against the
  pinned key, then every frame encrypted, with rekeying and a replay
  window. V1's test vectors (`system/harness/testdata`) are the Rust
  port's first acceptance test: the same bytes in, the same bytes out.
- **The calls needed for one task:** `agents.list`; `turn.send` with an
  explicit agent and a key that is written to disk before the send;
  `receipt.get` by that key; the events `turn.summary` (the result, in
  `fullText`), `question.open` and `question.close`; `question.answer`;
  `status`; `turn.stop`. A receipt is `queued`, `delivered`, `started`,
  `completed` or `rejected`. The Store calls can wait.
- **Never sent twice.** A send whose answer is lost is unknown, not
  failed: it is asked about by its key and never sent again. A result seen
  twice is one result. After a restart, something that was being spoken is
  marked uncertain and is not spoken again.
- **A permission prompt on the computer** arrives looking like a question
  and is not one: the lamp says once that the computer needs attention and
  never answers it.

What V1 does about speaking, kept unless a count on the lamp says
otherwise: a result or a question is spoken only when the lamp is neither
speaking nor listening and a second and a half after the last of either;
questions before results; one older than ten minutes is dropped; progress
is not spoken; the text reaches the voice model as material to tell from
in a sentence, never to read out. In V2 that last is the written turn of
decision 8.

What this changes in the estimate: the pairing and encryption are most of
the work, and "most of a week" for step 3 was too little. Two weeks is
nearer, with the first of them ending at a paired, encrypted connection
that can list the computer's agents.

The Harness team's own definition is public (`autonomous-ai/openharness`):
the contract in `docs/autonomous-device-integration.md` (last changed
2026-10-06), read in full on 2026-10-13, and the computer's side in
`cli/src/lib/autonomous-device` and `cli/src/lib/e2ee`, which carries test
vectors of its own. It agrees with the reading of V1 above and adds what
follows. Two of the three documents it points to were read the same
evening, on how a result is tied to its requests
(`autonomous-device-result-correlation.md`) and on input while an agent is
working (`in-flight-agent-input.md`); the Store
(`autonomous-device-store.md`) is not needed for a first task and is
unread. V1 also has an interoperation check
against the computer's code (`system/harness/testdata/direct_interop.go`)
that the Rust client should pass in the same way.

What the contract adds:

- **Which agent is not the lamp's to guess.** The Harness app on the
  computer says which agent is in focus (`focus.get`), and a task is sent
  with that agent and the focus's revision; if the focus has moved in the
  meantime the send is refused (`FOCUS_CHANGED`) and nothing runs. With
  nothing in focus when voice is first switched on, the lamp may ask for
  the first agent to be selected (`focus.ensure`), and a gesture on the
  lamp can step to the next or the previous one (`focus.step`). This is
  why decision 5 names the focus before the decision model.
- **The code is shown by the lamp.** The person picks the lamp on their
  computer and types the code the lamp displays. This lamp has no screen
  and its operator's pages come last, so until then the lamp says the
  six characters aloud when asked to pair. Whoever can hear it could pair,
  for the sixty seconds the code lasts, as whoever could read a screen
  could.
- **While an agent is working,** a further `turn.send` is taken as input
  to that work. Being taken is not being done, and a summary that is not
  tied to that message is never given as its answer.
- **A permission prompt:** said once, in the contract's own words ("Agent
  … needs permission. Open OpenHarness to review and approve or deny."),
  remembered across a reconnection so that it is not said twice, and
  cleared silently when it closes. Never answered by voice.
- **After a reconnection:** a request found queued is waited for; found
  delivered, started or completed, it is taken as sent; found rejected, it
  is reported; found unknown or not found at all, the person is asked
  before anything is sent again.
- **Sizes and rates:** a request to an agent is at most 16 KiB and is
  never cut; a result's full text at most 8,192 bytes; a frame 65,536; one
  request a second after a burst of twenty, four in flight.

What the two further documents add, and it is the larger part of steps 3
and 4:

- **One result can answer several requests.** If a second request reaches
  an agent while it is still on the first, the agent may answer both in
  one final message, and Harness then sends one result naming both. The
  lamp keeps a list of what it has sent (to which agent, with which key,
  under which delivery, to be told to whom), matches every request a
  result names against it, and applies the result only if all of them
  match. It is said once for the result, not once for each request. A
  third request the result does not name stays open.
- **A result is known by where it was first made and its own id,** not by
  the connection it arrived on. The same result arriving again after a
  reconnection or a restart of either side is the same result and is not
  said again. The lamp keeps what it has said on disk for that. A result
  that was being said when the lamp stopped is uncertain, and is not said
  again by itself.
- **Only the result ends a task.** An agent finishing a turn, a receipt
  changing, or a summary that names no request does none of that, and the
  agent's latest summary is never taken for the answer to a request it
  does not name.
- **Unknown is a real answer.** Harness proves that work completed. Work
  that failed or was cancelled, a second request that overlapped the
  first without the engine saying which was which, and anything in hand
  when the computer's side restarted come back as unknown. So beside
  "sent" and "done" (decision 8) the lamp needs words for "I cannot tell
  whether that went through; look in Harness", and says them only then.
- **A follow-up to a working agent** is typed into that agent as it
  works. Harness reports how (steered in, queued in the agent, or held
  until the turn ends) and whether it was taken. Taken is not done.
- **The acceptance tests are theirs:** the schema and the worked cases
  beside that document (one request, a group, and what the lamp must do
  on replay) are fixtures the Rust client has to pass. Their own note
  says the whole path, to the lamp speaking, has not been proven end to
  end by either side.

## Decision 12 in more detail: memory across days

A proposal for the product owner to answer, written the evening they chose
memory across days. It has since been built as `lamp-memory`
([memory.md](memory.md)) and is on in the product; what follows is the
proposal as written.

Three things, kept apart:

| What | How long | Where | How the voice gets it |
|---|---|---|---|
| The conversation going on | Minutes | Memory, and a file that is gone at power-off | Told to each new session, as going on or as finished (running today) |
| The journal: each exchange the lamp took part in, with its time. What it was asked, as the provider wrote it down, and what it said. Later, each task and how it ended | Days, to a limit to be set | A private directory on the lamp's disk, text only | Looked up when wanted: the model is given a tool to ask the lamp what was said about something or on some day, and gets the lines back |
| What it was asked to keep: "remember that I take my coffee black" | Until it is told to forget | The same directory | Told to each new session, whole, while it is small |

Why a tool and not a summary told at opening: the lamp has no model of its
own to write a summary with, a second cloud model would be one more thing
to fail, and whatever is told at opening is paid for in every answer and
acted on at the first turn (change 2). A look-up costs one exchange with
the lamp's own disk, and only when someone asks. It is the same route as
decision 4.

What the product owner is asked to settle:

- **One memory, or one for each person.** One, at launch, is proposed:
  the lamp does not know who is speaking (decision 10), so anyone at the
  lamp can ask what was said to it.
- **What goes in.** Proposed: both the journal and what it is asked to
  keep. The room's talk that it left alone never goes in.
- **Where it is.** Kept on the lamp only. To be said plainly to users: a
  line of it leaves the lamp, to the voice provider, whenever the lamp
  uses it in an answer. Kept on the lamp does not mean never sent.
- **Forgetting.** "Forget that" removes the last thing; "forget
  everything" empties it; reset empties it. Each is confirmed aloud only
  once it has been done (decision 8).

Known weakness: an answer the lamp gave to something that was not for it
goes into the journal like any other. On 2026-10-13 such answers, told
back to the next session as its conversation, fed on themselves. Here they
are only found when looked for, and "forget that" removes one, but a rule
for keeping them out is not yet known.

## What the page's review of the voice code predates

The page read the voice branch about a day before this was written. Since
then, on that branch: the conversation is carried across a new session and
across a restart of the service; web search is on; the lamp can begin a
turn itself and take a look when it says it will; a sound too short to be
speech never reaches the model; the voice has real-time priority (a load
that ended it four times in forty seconds ends it not at all). Still as the
page says: tool calls are rejected, what a person said is tied to an
answer only by timing, and a refused quota ends the service after a
two-minute wait.

## Not known

- Whether the changed note keeps a new session quiet in a talking office.
- Whether the second microphone separates a person at the desk from one
  two desks away.
- What tool declarations do to speed and to how readily the model acts on
  what it overhears.
- Whether a fact given mid-session changes how the lamp treats speech it
  hears, as it changed how it treated a written line.
- How long this takes. It is weeks; the voice itself is not finished
  (interrupting it, a group at the lamp, speaking first are open).
- Schedule's host checks do not establish lamp storage durability, clock
  accuracy, privacy/admission integration or due-to-speech latency. Its
  durable socket and bus handoff are built; no reminder is spoken by this
  component, and First's Voice wiring remains to be qualified on the lamp.
