# The policy: deciding what the lamp does

`lamp-policy` (`crates/policy`) is the part that decides what the lamp
does. This is its first version. It makes one decision, "is this said to the
lamp?", and it makes it in shadow mode: it says what it would do, on
[the bus](bus.md) and in a log, and nothing acts on it.

The normal `lamp-policy` entry point keeps its existing behavior. Task 17
adds the explicitly enabled [local second judge](#the-local-second-judge-task-17)
through `lamp-policy-judges`; its deadlines, fallback and logs are described
separately below. `lamp-policy --progress` adds a second question, [say this
progress now?](#progress-news-say-this-progress-now-732), about the work the
lamp handed over, and `--act progress` makes that one question's decisions
act: [the first decision the policy really makes](#acting---act-progress-752).
`--hello` asks [say hello now?](#hello-on-arrival-say-hello-now-752) when the
voice holds a due hello, and `--act hello` makes those decisions act.
`--act attention` publishes "is this said to the lamp?" for real, for the
voice to [hold the lamp's actions](#acting-on-the-lamps-actions---act-attention-800)
by; speech is not held.
Nothing else authorizes Voice or Body to act.

## What it is for

Today the model that holds the conversation also decides, by answering or
not, whether it was spoken to, and on the lamp it has answered people who
were talking to each other ([attention.md](attention.md)). A decision model
is another kind of thing: it is shown a `state`, one JSON value that says
how things stand, and typed questions about it, and it answers with
probabilities and never with text. The first one is TypeSafe's Jev. The
owner's rules for this part:

- Several decision models may be used. Each is a judge.
- One policy combines their judgements and makes the single choice. No
  judge acts by itself.
- Every model sits behind one common way of asking, so that a second model
  is a second implementation and not a second design.
- A new model or a new decision starts in shadow mode: it decides and is
  written down, and does not act.
- Every decision is logged with what was known, what the model said and
  what was chosen. That log is how the thresholds are tuned again, and how
  a model of the lamp's own will be trained later.

## The process

    lamp-policy --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR [--endpoint URL] [--model NAME] [--deadline-ms N] [--bus-root ABSOLUTE_DIR] [--progress] [--hello] [--utc-offset-minutes N | --zone system|NAME] [--act progress] [--act hello] [--hello-by-rule] [--hello-quiet to-the-lamp] [--rules] [--act attention] [--gate]

| Option | What it is | If left out |
|---|---|---|
| `--key-file` | A file whose one line is the key for the decision model's service. Space around the line is taken off. The key is never printed | required |
| `--log-dir` | Where the decisions log is kept. Made with mode 0700 if it is not there | required |
| `--endpoint` | Where the decision model is asked: `https://`, or `http://` for a server on the same machine in a test (127.0.0.1, ::1 or localhost). Plain http to any other machine is refused, because the key and what was heard would cross the network in the clear | `https://openrouter.ai/api/alpha/decisions` |
| `--model` | The model asked for | `typesafe/jev-1.13` |
| `--deadline-ms` | How long one asking may take, from 1 to 60000 | 3000 |
| `--hedge-after-ms` | A question not answered in this many ms (1 to 60000) is asked again on a second connection, and the first answer is taken ([below](#a-second-asking-when-the-first-is-slow-1522)) | off |
| `--bus-root` | The bus's directory | `LAMP_BUS_ROOT`, or `/run/lampos-bus` |
| `--progress` | Also ask, in shadow, whether progress of the work handed over is worth telling now ([below](#progress-news-say-this-progress-now-732)) | off |
| `--utc-offset-minutes` | The lamp's local time east of UTC, for the hour of the day the progress and hello questions show, the same all year | the hour is null |
| `--zone` | In place of the offset: `system`, the lamp's own zone (`/etc/localtime`, as setup writes it), or a zone's name. The hour is taken at the decision's moment, across summer time, and `system` takes up a zone setup changes while the policy runs (#820). An unknown or unreadable zone ends the start (exit 1); with the offset too, exit 2 | the offset |
| `--act progress` | That question's decisions act: published with `shadow: false`, so that the voice tells a `say_now` ([below](#acting---act-progress-752)). Needs `--progress` | shadow |
| `--hello` | Also ask, when the voice holds a due hello, whether to say it now ([below](#hello-on-arrival-say-hello-now-752)) | off |
| `--act hello` | Those decisions act. Needs `--hello` | shadow |
| `--hello-by-rule` | The hello is decided by the hard rules alone: a pass is `say_now` at once, with no wait for the model; the judge is asked only in shadow, its answer written on a `hello_judge` line and never acted on ([below](#by-the-hard-rules-alone-4625)). Needs `--hello` | the judge decides a pass |
| `--hello-quiet to-the-lamp` | The hello's 15 s quiet counts only talk to the lamp or by it: a turn the voice held back as not said to the lamp (`voice/turn`) is taken back from it ([below](#the-quiet-counts-talk-to-the-lamp-1155)). Needs `--hello` | all talk counts |
| `--act attention` | "Is this said to the lamp?" is published with `shadow: false`, for the voice to hold the lamp's actions by ([below](#acting-on-the-lamps-actions---act-attention-800)) | shadow |
| `--rules` | The progress and hello questions, and with `--gate` the moment question, obey what the people at the lamp asked about it speaking first (`memory/rules`, [above](#a-persons-rules-about-speaking-first-813)). Needs `--progress` or `--hello` | not listened to |
| `--gate` | Also answer a skill that asks (`skills/ask`) whether the lamp may say hello, tell news or start something itself ([below](#hello-on-arrival-say-hello-now-752)). Needs `--hello` and `--act hello` | not listened to |
| `--addons` | Also answer an add-on's say past its judge (`addon/ask`, [addons.md](addons.md), #2530 part 2) by the moment question ([below](#hello-on-arrival-say-hello-now-752)), `about` `addon`. Needs `--gate` | not listened to |
| `--act addon` | An add-on's say decisions act (`shadow` false). Needs `--addons` | shadow |

A wrong or missing option prints the usage and exits with 2. So does a
`--log-dir` that cannot be made, or that its group or everyone can enter. A
key file that cannot be read or does not hold one key, and a bus that
cannot be subscribed to, exit with 1 before anything is listened to.
`--help` prints the options.

It is its own process: with it gone the lamp talks on as before. It runs
until it is killed. It cannot catch a signal (the standard library has no
way and the crate takes in nothing else), so SIGTERM ends it where it
stands: a question that was out is not decided, and its sockets on the bus
are left for the voice's next message to remove.

It subscribes, each with its type from [lamp-msgs](msgs.md), to:

| Topic | What it takes from it |
|---|---|
| `voice/heard` | What was heard, a piece at a time |
| `voice/said` | What the lamp said: the pieces of its newest answer, put end to end |
| `voice/state` | Whether the lamp is speaking; whether the person is still being listened to, which sets how long an utterance waits before it is judged; and the moment someone begins to speak, when the model is told to get ready |
| `room/state` | How the room is: quiet, busy or not known. Not known until the voice has said, and again once the voice has gone |
| `voice/turn` | With `--hello-quiet to-the-lamp` only: each turn's verdict, answered or held back as not said to the lamp, for the hello's quiet ([below](#the-quiet-counts-talk-to-the-lamp-1155)) |
| `name/heard` | Each time the lamp's name was heard in its own microphone's sound, with a score ([name.md](name.md)). Whether anything is publishing there at all is looked at once a second |
| `humans/faces/tracked` | How many faces the eyes see, how many of them are near (a box at least 15% of the picture's height), and how many of those are turned to the lamp. Written down with each decision (`seen`) when the eyes' list is no more than two seconds old, and **not used to make the decision yet**: nobody had stood in front of the lamp when this was written, so there is nothing to weigh it by |

It publishes `policy/decisions` (`lamp_msgs/Decision`) under the name
`policy`, and says once a second that it is alive there, because the topic
is silent between decisions for longer than the bus's lease.

<a id="a-persons-rules-about-speaking-first-813"></a>
**A person's rules about speaking first (#813, `--rules`).** The voice
keeps `memory/rules` (`lamp_msgs::Rules`, [msgs.md](msgs.md)), kept until
the next, with typed facts from what the person at the lamp asked of it,
for `person`, the memory's id of the known person or `everyone`:
- `quiet` (never speak first) and `quiet_mornings` (not in the mornings);
- per kind, `hello` (the greeting on arrival) and `news` (progress and
  results of work), the seventh's #904 with every typed rule merged and
  the strictest winning: `never`, `not_before_minute`, `not_from_minute`
  (the local minute of the day) and `not_while_talking` ("on a call, or
  with someone").

With `--rules` (which needs `--progress` or `--hello`) the policy listens
to it, newest only, and each question that may speak first obeys it as one
more hard rule after its own (`crates/policy/src/rules.rs`): the progress
question by `news`, the hello question by `hello`, and with `--gate` the
moment question by `news` for a skill's news and by both kinds for anything
else. Taking progress news:
- `quiet` holds always;
- `quiet_mornings` holds before noon at the lamp, by
  `--utc-offset-minutes` or `--zone` (the zone's offset at that moment, so
  noon moves with summer time); without either the local time is not
  known, and it holds all day, since a rule kept is obeyed whenever it may
  hold;
- of `news`: `never` holds always; `not_before_minute` holds before that
  minute and `not_from_minute` from it on, both all day with the local
  time not known; `not_while_talking` holds while the room (`room/state`)
  is not known to be quiet, as it is when the answer comes back, not as it
  was when the question was asked (the reviewer on #937). The question
  already waits while anyone talks
  to the lamp: this adds the talk that is not to it, a call or a visitor.
  A limit on `hello` holds no news.

While it holds the answer is `wait`, by the rule, with `guard`
`kept_rule`, and **no model is asked**. News held so is asked about again
once the rule no longer holds, as with the other hard rules. A yes that
comes back after a rule was kept is turned to `wait` the same way. The
newest `Rules` is obeyed whoever it names: who is at the lamp now is not
known to the policy, and a rule errs toward quiet. It is kept when the
voice goes, since a rule does not end with it. The diagnostics add
`progress_held_rule` and the start line `rules=true`. Without `--rules`
the topic is not listened to and everything is as it was. The hello on
arrival and the skills' asks obey it the same way
([below](#hello-on-arrival-say-hello-now-752)). Tests:
`crates/policy/src/rules.rs` (the hours; each limit for its own kind only,
by the hour and the room), `progress.rs`
(`a_kept_rule_holds_progress_news_and_no_model_is_asked`: `wait` with no
model asked, quiet mornings until noon, a rule forgotten holds nothing,
news not while talking held in a busy or unknown room and not in a quiet
one, a limit on the hello holding no news, an answer after a rule kept;
`a_yes_that_comes_back_into_a_room_now_busy_is_held_by_not_while_talking`:
asked in a quiet room, the answer coming back into a busy or unknown one
is a `wait`),
and `crates/policy/tests/it/progress.rs`
(`with_rules_a_kept_rule_holds_progress_news_and_no_model_is_asked`, on a
bus, with the typed `news.never`; without `--rules` nothing listens to the
topic). Not shown on a lamp.

Once a second it also says how it is itself, on `diagnostics`
(`lamp_msgs/Diagnostic`, under the name `policy`), with how many
decisions it has made and how many of them by the rule, and
`name_listening` `true` while something on the lamp listens for its name
(`name/heard` has a live publisher, looked at in that same second; left
out when nothing does). The tests wait for that instead of sleeping
through the second (#1060):

| Level | Message | When |
|---|---|---|
| `ok` | | A model answers and the log is written |
| `warn` | `the decision model is not answering: the rule decides until it does` | From the first failed asking to the next that is answered |
| `warn` | `its log cannot be written: decisions go on without it` | While a decision cannot be written down |
| `error` | `it can no longer ask the decision model: the rule decides` | The thread that asks has ended |

The room matters more than any one of the model's answers: in the
combination below a quiet room adds 1.771, where the strongest answer adds
at most 1.132. With the room not known, no set of answers reaches the 0.8
that `answer` asks for by more than a little.

## The decision: is this said to the lamp?

A lamp on a desk in a shared office hears much that is not meant for it.
Before it answers it has to judge whether what it heard was said to it.

**What is judged.** The voice publishes what it hears in pieces, as the
recogniser writes them. The pieces are put end to end into one utterance,
which is over when a piece is marked `finished`, when 300 ms have passed
with no further piece, or when a piece of another of the voice's turns
comes (each piece carries the number of the turn it was heard in, #908): a
decision is about one turn's words, and carries that turn's number, never
a later one's. Today the voice marks none, so the 300 ms end every
utterance. An utterance is judged once. One that reaches 2,000 bytes is
judged at that length and the rest begins another, so that talk which
never pauses is still judged, in parts.

**What is known.** With each utterance goes a context:

| Field | What it says |
|---|---|
| `heard` | The utterance |
| `room` | `quiet`, `busy` or `unknown` |
| `lamp_is_speaking` | Whether the lamp was speaking |
| `lamp_last_said` | The end of what the lamp had last said: at most its last 200 bytes, begun at a whole character |
| `seconds_since_lamp_spoke` | How long before the utterance the lamp had last spoken |
| `name_heard` | The highest score the lamp's name was heard with while it was said; not there when nothing listens for the name (below). Not shown to the model |
| `like_asker` | How alike its voice is to the voice of the person the lamp is in conversation with, from -1 to 1, as the voice said it for the request; not there when the voice said none ("How alike to the asker", below). Not shown to the model |

`lamp_is_speaking`, `lamp_last_said` and `seconds_since_lamp_spoke` are
taken when the utterance's first piece comes, not when the utterance
ends. By its end the lamp may have begun to answer it, and
whether to answer must not be judged from the answer. The log is what the
weights are fitted from, and a context that held the lamp's own reaction
would give away what is to be learned. "Last spoken" is the later of the
last piece of what the lamp said and the last moment its state was
`speaking`, because the text of an answer can come before its sound has
been played to the end.

The lamp is **in conversation** when it is speaking, or when it said
something with a letter or a digit in it within the last 20 seconds. An
answer that was only "..." is the lamp keeping quiet aloud and starts
nothing.

**What the model is shown.** The state, with its fields in this order:

    {"heard":"...","lamp":{"what_it_is":"A small desk lamp robot that can talk. It sits on a desk in a shared office.","name":"Lamp","people_call_it_with":["Lamp","Hey Lamp","Lamp ơi"],"the_recogniser_often_writes_its_name_as":["Milan","Weiland","Wei Lan","Weimar","Helen","Liam","Lam","Lan","lamb","plan","Man","Hey man"]},"room":"quiet: nobody else has been talking","lamp_is_speaking_now":false,"lamp_last_said":null,"seconds_since_lamp_spoke":null}

`room` is `"quiet: nobody else has been talking"`, `"busy: people are
talking to each other"` or `"unknown"`. `lamp_last_said` and
`seconds_since_lamp_spoke` are null unless the lamp is in conversation, and
the seconds are given to a tenth.

**What the model is asked.** Seven questions, each answered with the
probability of yes. Every one begins "state.heard is what a speech
recogniser wrote for something just said near the lamp; it can mis-write
names and words." and goes on:

| Name | The question |
|---|---|
| `calls_lamp` | Does state.heard call the lamp: its name, one of state.lamp.people_call_it_with, or one of the mis-writings in state.lamp.the_recogniser_often_writes_its_name_as, at the start or the end of what was said? |
| `names_someone_else` | Does state.heard address some other person or device by name (for example 'Tom,', 'Samantha,', 'Hey Siri'), a name that is not the lamp's and is not in state.lamp.the_recogniser_often_writes_its_name_as? |
| `others_business` | Is state.heard part of people's own work talk with each other: reports, invoices, clients, meetings, calls, printers, lunch plans, who did or sent or told what, or a reply to something another person said? |
| `lamp_could_answer` | Could a friendly talking desk lamp sensibly answer state.heard: a general-knowledge question, the time or the weather, a joke or a story, small talk about how it is, a greeting or a good night, or a request about its own light, voice or volume? |
| `follows_lamp` | state.lamp_last_said is what the lamp itself said state.seconds_since_lamp_spoke seconds ago, or null if it has said nothing lately. Is state.heard said back to the lamp about that: a reply, a follow-up question on the same subject, or an instruction such as stop, wait, go on or something else instead? If state.lamp_last_said is null the answer is no. |
| `listening_noise` | Is state.heard only a listening noise or a scrap: mm-hm, uh-huh, yeah, okay, ha, a cough, one meaningless word, with no question and no request in it? |
| `asks_to_stop` | Does state.heard ask whoever is speaking to stop, wait, hold on or be quiet? |

The model is not shown the names. The wording, the order of the questions
and the order of the state's fields are all part of what was measured: the
weights below belong to them, and a test holds each to the letter.

## How the answers are combined

The seven answers, the room and the conversation become one probability by
a weighted sum put through the logistic curve:

    sum = 0.263
        + 0.722 x calls_lamp
        - 0.249 x names_someone_else
        - 2.042 x others_business
        + 1.132 x lamp_could_answer
        - 0.136 x follows_lamp
        - 0.645 x listening_noise
        + 0.031 x asks_to_stop
        + 1.771 if the room is quiet
        - 0.206 if the lamp is in conversation

    probability = 1 / (1 + exp(-sum))

| Probability | Choice | What it means |
|---|---|---|
| 0.8 or more | `answer` | Answer it |
| 0.4 to under 0.8 | `look_up` | Turn to whoever spoke and wait, without speaking |
| under 0.4 | `stay_quiet` | Go on as it was |

**Where the numbers come from.** They were fitted on 2026-10-13 on 346
labelled examples from the lamp's own records. On the half held out, 164
examples, a rule alone was right 74% of the time, and the model's answers
with these weights 87%. Where the combined probability was 0.8 or more it
was right 99% of the time, which is why `answer` begins there. They are a
first fit, to be made again as the log grows. That measurement was made
before this program existed, and not with it.

What the model cannot do: it cannot tell that the lamp was called when the
recogniser dropped the name from the text. So the next input is a sign from
the device itself that it was called.

**With the name heard on the lamp.** Since 2026-10-14 a part of its own
listens for the lamp's name in what its microphone hears and says so on
`name/heard` ([name.md](name.md)). While that part is there, the sum is
another one, with the name as one more term:

    sum = -0.328
        + 0.540 x calls_lamp
        - 0.634 x names_someone_else
        - 1.704 x others_business
        + 0.439 x lamp_could_answer
        + 0.104 x follows_lamp
        - 0.588 x listening_noise
        + 0.038 x asks_to_stop
        + 1.683 if the room is quiet
        + 0.081 if the lamp is in conversation
        + 2.346 if the name was heard, with a score of 0.9 or more

A hearing belongs to what was said if it began between a second before
the voice started to listen to that person and the moment the text came.
The name is said at the beginning and the text arrives after the end, so
the start of listening is what joins them; if that is not known, the
eight seconds before the text count.

With nothing on the lamp listening for the name, the first sum is used
and the decision's context has no `name_heard`: not heard and not known
are kept apart. The model is not shown the sign either way, because its
answers were measured on the state without it.

These eleven numbers are a fit to a simulated sign and to no real
hearing. They were fitted on the same 182 examples as the first sum, with
the sign drawn at the rates `lamp-name` showed on lamp-4ace's own
microphone that night: heard for 85 in 100 calls, for 60 in 100 of the
lines the model scores as naming someone else, and for nothing else. The
numbers are the mean of 200 such fits. On the 164 examples held out, the
sum with the sign was right 150.8 times on average, against 142 without
it, and answered what was not for the lamp 7.5 times of 44 against 17; it
ignored what was for the lamp 5.7 times of 120 against 5. The log now
holds real hearings beside real decisions, which is what the next fit is
to be made from.

**How alike to the asker.** Since #4306 the voice can say, for each
request, how alike its voice is to the voice of the person the lamp is in
conversation with, the one who said the line that began it
(`attention/addressed`'s `like_asker`, [attention.md](attention.md)). The
Policy joins it to the utterance by the request's number, the newest the
voice said for that number in the last 60 s, and keeps it in the context
as `like_asker`. It is the cosine of the two lines' voice prints, from -1
to 1. Both sums have a term for it, added only when it is there:

        + w x (like_asker - 0.70)

0.70 is where the term is nothing: the voice's own bar for two prints
being one person's. `w` is 0 in both sums today, so every decision is the
one it was without the sign, to the bit, and a test holds that. It is to
be fitted once a lamp has run with the sign. On lamp-4ace's played rounds
of 30 Oct (1,166 lines), inside a conversation the seven answers told
follow-ups from desk talk badly: `follows_lamp` 0.61 against 0.42,
`others_business` 0.57 against 0.56. Whose voice it is should. The model
is not shown the sign. Without the voice's same-voice check, outside a
conversation, or when a print is not known, the context has no
`like_asker` and the term adds nothing.

**When no model answers.** The rule that was the baseline decides:
`answer` if the room is quiet, the lamp is in conversation or its name
was heard, otherwise `stay_quiet`. It gives the probability 0.74 for the first and 0.26 for the
second, which is how often it was right, and the decision says that the
rule made it.

**While the room is unknown.** A room nobody has judged is not taken for a
quiet one, so the 1.771 is never added. The most the sum can then reach is
2.148, a probability of 0.90. `answer` is then chosen only when
`lamp_could_answer` is near 1 and next to nothing counts against, or when
the lamp is plainly called as well: with `lamp_could_answer` at 0.9 and
every other answer at 0 the probability is 0.78, which is `look_up`. The
rule, for its part, then answers only in conversation. Decisions made now
will read as more reserved than the measurement, which had the room.

**A second judgement, in shadow: `--attention-next` (#1040).** The fit
counts a reply to the lamp against answering: `follows_lamp` −0.136, and
being in conversation −0.206. Yet on lamp-4ace on 2026-10-16 the answer
told such replies apart. Of the first builder's 79 hand-read `stay_quiet`
decisions in conversation, the 33 said to the lamp had a median
`follows_lamp` of 0.60, and the 46 not had 0.29. With the switch, each
decision is also judged by `attention::combine_next`: the same sum and, in
conversation only, 3 for each unit of `follows_lamp` above 0.55. The term
is one-sided, so the second judgement is never below today's, and out of
conversation it is the same. The 3 and the 0.55 are a logistic fit of all
79 labels on `follows_lamp` alone, with a ridge (L2) penalty of 0.5 on the
summed log loss and the bias not penalised: +3.02, centred at 0.55. Scored
with each row left out in turn, 14 of the 33 said to the lamp and 1 of the
46 not reach 0.6. A plain fit gives +5.02 centred at 0.51 (17 and 7): the
penalty is what keeps the term this cautious (the reviewer on #1106; the
script is on #1040). With 3 and 0.55, 13 of the 33 said to the lamp leave
`stay_quiet` (to `look_up`) and none of the 46 not. The other three answers
in the rows (`calls_lamp`, `lamp_could_answer`, `others_business`) add
nothing on the rows left out. The second judgement is written on the
decision's line as `next` (`probability`, `choice`, `from_rule`), left out
when the rule decided. The diagnostics count the decisions whose two
choices differ (`attention_next_differs`) and the start line says
`attention_next=true`. It is never published and nothing acts on it. A day
of it on lamp-4ace, with the lines where the choices differ read by hand,
says whether it goes live. The full refit still needs the day's answers
in conversation labelled as well (#873's `lamp-policy-replay --again …
--labels … --refit`). Test: `tests/it/policy.rs`,
`with_attention_next_a_second_judgement_is_written_beside_and_nothing_published_changes`,
and `attention::tests::the_next_judgement_raises_a_reply_to_the_lamp_in_conversation_only`.

**The rule alone for kinds it is certain of: `--rule-decides` (#705).**
A call the voice holds for the Policy's decision (`LAMP_POLICY_HOLDS_ACTIONS`)
waits for Jev's answer: 0.55 s median and 0.97 s p90 on lamp-4ace on
18 Oct, which left a held call waiting 0.18 s median and 0.47 s p90, and
the rule (`attention::fallback`) would have said the same on 45 of the 51
that waited. With `--rule-decides KIND[;KIND…]`, an attention question of
a kind named there is decided by the rule (`by: rule`, `took_ms` 0) and the
model is not asked; every other kind is asked as before. Questions keep
their order: a named kind that comes while the model is still being asked
about the one before it waits for that answer (up to Jev's ~0.55 s), and is
then decided at once, so "no wait" holds for a held call with nothing
asked ahead of it (the reviewer on #1524). The rule answers whenever the
room is quiet, the lamp is in conversation (speaking, or words of its own
within 20 s) or its name was heard at 0.9 or more, and stays quiet
otherwise, so a kind with `conversation true` is always `answer` (Sixth on
#1524). So never name a `busy true …` kind while the voice relies on
`LAMP_GOING_ON_ASKS` (#1538): that switch asks the decision for a line a
busy room's conversation lets out, and a `busy true` kind would answer
every one of them by the rule, undoing #1522's fix. `--agreement` keeps
such kinds out on its own (Jev disagreed with the rule on 39 of the day's
48), but the list is a person's to write (Fourth on #1524). A kind with the lamp speaking is refused at start: the rule would
answer talk said over the lamp's reply whatever it was, and a night's
record can't show the office's talk over the lamp that wasn't for it. A
kind is written as `lamp-policy-replay --agreement` prints it (room,
conversation, the lamp speaking, the name's score: `busy false false
>=0.99`), and is named only once a night's `--agreement` shows no
disagreement on it (Fourth's conditions on #705). Its line says `certain`
and no failure, leaves out `asked` and `state`, and the model's health line
is not moved by it. The cost: those questions' model answers are no longer
logged for fitting, and with `--images` they get no picture question
either. A rule
decision carries no `follows_lamp`, so the voice reads its `stay_quiet` as
the plain one; `--agreement` counts a following `stay_quiet` as a
disagreement, so no such kind is named. Only the single asker: with
`judges` the switch does nothing. The start line says `rule_decides=N`.
Off unless set. Tests: `tests/it/policy.rs`,
`a_kind_the_rule_decides_is_not_asked_and_others_are` and
`a_rule_decision_leaves_the_models_health_as_it_was`; `main.rs`,
`the_rule_decides_kinds_are_read_and_a_speaking_kind_is_refused`.

## Shadow mode

Every decision is published with `shadow: true` and nothing subscribes to
act on it, unless `--act` says otherwise for one question. The voice answers
or does not as it did before. What the policy would have done is in the
log, to be laid beside what the lamp did.

### Acting on the lamp's actions: `--act attention` (#800)

With `--act attention` the attention decisions are published, and written
down, with `shadow: false`; the start line says `shadow=false` and
`act=attention`, and so does the diagnostics' `shadow`. The voice, with
`LAMP_POLICY_HOLDS_ACTIONS=on`
([attention.md](attention.md#the-policy-holds-the-lamps-actions-800)), then
holds each call that changes the lamp (the volume, the light) or sets or
takes away a timer or reminder until its turn's decision, paired by the
turn's number (`request`): on `stay_quiet` with the lamp's name not heard
it is not run; otherwise, or after 2 s with no decision, it runs as before.
Speech is not held: what the lamp says stays the voice's rule. Without the
flag every attention decision stays shadow, and the voice's switch only
makes a held call wait 2 s and run: one in a turn where the lamp's name was
heard runs at once, as it does with the flag.

The case it is for: on lamp-4ace on 15 October at 14:49 the lamp heard
"Thailand" in a busy office and ran `volume_up`, which raised the software
volume from 75% to 95% for eleven hours; this decision, in the same second,
was `stay_quiet` (p = 0.235). Of lamp-4ace's 3,698 attention decisions from
13 to 16 October, 99.5% were made within 2 s (the first builder, #800).
Accepted on lamp-4ace on 16 October at 07:25 with the voice of #908
(`holds.sh`, `twopeople829.sh`). Test: `tests/it/program.rs`,
`with_act_attention_the_attention_decision_is_published_for_real`.

What is published on `policy/decisions`:

| Field | What it says |
|---|---|
| `about` | `attention` |
| `choice` | `answer`, `look_up` or `stay_quiet` |
| `probability` | What the choice was made from |
| `answers` | The model's answer to each question, by its name. Empty when the rule decided |
| `by` | The model as its service names it (`typesafe/jev-1.13-20260917`), or `rule` |
| `took_ms` | How long the asking took, answered or not |
| `shadow` | `true`, or `false` with `--act attention` |
| `request` | The number of the turn the voice heard the utterance's newest piece in, if it said: `voice/heard` carries it (#908). What the voice's actions are paired with when the Policy holds them (`docs/attention.md`) |

What was heard is not in the message.

## The log

One line of JSON for each decision, in `<log-dir>/decisions-YYYY-MM-DD.jsonl`,
the day by UTC. Its shape, with numbers made up:

    {"at_unix_ms":1791936000123,"about":"attention","shadow":true,"context":{"heard":"Hey Lamp, what time is it?","room":"unknown","lamp_is_speaking":false,"lamp_last_said":"It will rain this afternoon.","seconds_since_lamp_spoke":41.5},"in_conversation":false,"state":{...},"asked":"typesafe/jev-1.13","answers":{"asks_to_stop":0.09,"calls_lamp":0.07,...},"judgement":{"probability":0.6261,"choice":"look_up","from_rule":false},"by":"typesafe/jev-1.13-20260917","took_ms":655,"waited_ms":302,"usage":{"input_tokens":1066,"output_tokens":134,"cost":4.4772e-5},"pieces":3,"cut":false}

| Field | What it says |
|---|---|
| `at_unix_ms` | When the decision was made |
| `context` | Everything that was known, as above, whether or not the model was shown it |
| `in_conversation` | Whether the lamp was in conversation |
| `state` | What the model was shown. Its fields are here in the order of the alphabet; what was sent had them in the order above. Left out on a `certain` line (`--rule-decides`): nothing was shown |
| `asked` | The model that was asked for. Left out on a `certain` line: none was |
| `answers` | Its seven answers. Empty when the rule decided |
| `judgement` | The probability, the choice, and whether the rule made it |
| `by` | Who decided: the model as its service names it, or `rule` |
| `took_ms`, `waited_ms` | How long the asking took, and how long it was from the utterance's last piece to the asking. The pause that ends an utterance is part of the second |
| `connection` | What the asking was sent on: `new` (made for it, and part of `took_ms`), `ahead` (made while the person was speaking), `kept` (from an asking before) or `replaced` (a kept one had ended, and another was made within this asking). Not there when the model did not answer |
| `failed`, `failed_why` | Only when the model's answers were not used: `unreachable`, `timed_out`, `refused`, `unreadable` or `missing`, and one sentence |
| `usage` | What the asking cost, if the service said |
| `request` | The request's number, if known |
| `pieces`, `cut` | How many pieces the utterance came in, and whether it was ended by its length |

A decision is published first and written second, so that whoever comes to
act on one does not wait for a disk. Each line is one write. A line that a
failed write left unfinished is ended before the next one is added: a
reader passes over a line that is not JSON. Nothing removes old days.

## What the lamp then did

In shadow mode nothing acts on a decision, so what the lamp does next is
the voice's own doing. It is written beside the decision, one line of JSON
for each, in `<log-dir>/outcomes-YYYY-MM-DD.jsonl`
(`crates/policy/src/outcome.rs`). With numbers made up:

    {"at_unix_ms":1791936001900,"about":"attention","decision_at_unix_ms":1791936000123,"choice":"answer","lamp":"spoke","spoke_after_ms":412,"said_after_ms":380,"heard_to_sound_ms":1290,"request":17,"said":"It is ten past nine.","overtaken":false}

| Field | What it says |
|---|---|
| `decision_at_unix_ms` | The `at_unix_ms` of the decision this belongs to |
| `choice` | What the policy would have done |
| `lamp` | `spoke`: the voice's state turned to speaking. `silent`: it did not within 6 s of the last of what was heard |
| `spoke_after_ms` | From the decision being published to the lamp's first sound. **Less than nothing when the sound came first.** This is all the time anything acting on the decision would have had |
| `said_after_ms` | The same, to the first of the lamp's words in writing |
| `heard_to_sound_ms` | From the last piece of what was heard to the lamp's first sound |
| `request`, `said` | The request the lamp's answer belongs to, and its words as far as they had come a second after its first sound |
| `overtaken` | Another utterance was decided before this outcome was known |

It is two things. The other half of an example: what was judged, and what
then happened, which is what the weights and thresholds are to be fitted
again from once someone has said which outcomes were right. And a
measurement, on every turn the lamp takes, of whether a decision comes
early enough to be acted on.

**The times come from the messages' own stamps** (#1652). The lamp's first
sound, its words and what was heard are each timed by the stamp its
message was sent with: the publisher's `at_us`, on the same monotonic
clock. They are not timed by when the policy's loop took the message up.

- **Why:** each turn the loop takes what the lamp is doing before what is
  heard. A turn held up past the gap between the two took the lamp's sound
  first, and a real answer was written `silent` (#1569 row 25).
- **A sound stamped before the utterance began** is no answer to it, even
  if it is taken up after.
- **A message with no stamp, one stamped ahead of the clock, or one stamped
  more than 6 s (the outcome's window) before it was taken up,** is timed
  from when it was taken up, within 20 ms of its arriving (5 ms while a
  question is out). The last is a recorded session played again
  (`lamp-bus replay` sends each message with its recorded stamp): all of it
  then counts from its taking up, as before #1652.
- **What the policy decides** keeps the take-up times, and the decision's
  own time is when it was made.
- **How much earlier the times are now:** in the policy's own tests, the
  lamp's state was taken up 4.5 ms after its stamp (median; 21.8 ms at the
  90th percentile), and what was heard 0.2 ms after. So `spoke_after_ms`
  and `heard_to_sound_ms` are that much smaller than before #1652 on a
  loop that isn't held up.

Which answer belongs to which utterance is told by time alone (the turn's
number that what is heard carries is not used here): the lamp's first
sound after an utterance began is taken as its answer to it. With two utterances close
together that can be wrong, and the earlier one's outcome is then closed
as it stands and marked `overtaken`. Words that were written and never
played (an answer that was dropped) are `silent`, with `said_after_ms` and
no `spoke_after_ms`. An outcome that is still waited for when the program
is stopped is not written.

The journal gets a line of the same without the words:

    lamp-policy: outcome of decision 12 choice=answer lamp=spoke spoke_after_ms=412 heard_to_sound_ms=1290 overtaken=false logged=true

## Privacy

- **What is heard is sent to the decision model's service.** Every
  utterance goes, as text, to the service at `--endpoint`, with the end of
  what the lamp last said: unless told otherwise, to OpenRouter, for
  TypeSafe's model. On lamp-4ace the owner approved that for what is said
  in the office, on 2026-10-14. On any other lamp it is for played tests
  until its owner has said so.
- **The logs hold what was heard, and what the lamp said, in words.** That
  is deliberate: a judgement cannot be fitted again without what it was a
  judgement of. It is why the directory is the owner's alone. It is made with mode 0700 and
  its files with 0600, and a directory that its group or everyone can enter
  is refused at the start.
- **What the program prints never holds what was heard or said.** Its
  lines are counts, times, choices and kinds of failure. The loop cannot
  print at all: what it has to say leaves it as a value that can hold no
  text made while running, and a line printed from inside the library
  fails the check every change has to pass. A message on the bus that
  cannot be read is counted and nothing of it is repeated, because the
  reason it cannot be read may quote it.
- **The key** is read once from its file and sent in one header. It is in
  no error, no log line and no log file. A server whose certificate is not
  trusted is never sent it.
- The message on the bus holds no text that was heard.

## When something around it fails

None of these stops it. Each is counted, said once when it begins and once
when it ends, and the loop goes on.

| What | What it does meanwhile | What it prints |
|---|---|---|
| Nobody publishes a topic it listens to | Waits. A voice that has gone is taken not to be speaking | `voice/heard has no publisher (times=1)`, then `voice/heard has a publisher again` |
| The model cannot be reached, is late, refuses or answers something unusable | The rule decides | `the decision model is not answering (timed_out): the rule decides until it does`, then `the decision model answers again after failures=3` |
| The log cannot be written | Decides and publishes without it | `the decisions log cannot be written (no storage space): decisions go on without it`, then `the decisions log is written again; missing=7` |
| The bus cannot be published to | Decides and writes the log without it, and tries again each second | `decisions cannot be published on the bus (...)`, then `decisions are published again; unpublished=2` |
| A message is not of its topic's type | Passes it over | `a message on voice/heard is not of its type: such messages are passed over`, then `voice/heard is read again after passed_over=3` |
| Utterances end faster than they are judged | One question is out at a time and eight utterances may wait. When a ninth ends, the one that has waited longest is let go unjudged | `more than 8 utterances wait to be judged: the oldest are let go`, then `no utterance waits any more; dropped=2` |
| A topic's socket cannot be read | Goes on with the other topics | `voice/state cannot be received from (...)`, then `voice/state is received from again` |
| The thread that asks the model ends, which only a fault in a judge can cause | The rule decides from then on. It does not come back until the program is started again | `the thread that asks the decision model has ended: the rule decides from now on` |

For each decision it prints one line:

    lamp-policy: decision 12 about=attention choice=look_up probability=0.626 by=model took_ms=655 waited_ms=302 on=kept pieces=3 bytes=26

## Asking a decision model

`crates/policy/src/decide.rs` is the one way of asking, and knows nothing
of lamps. A judge is anything that takes a state, questions and a deadline,
and returns every answer or one failure:

| Failure | When |
|---|---|
| `unreachable` | The name was not found, the connection was refused or broke, or the certificate was not trusted |
| `timed_out` | The deadline passed first |
| `refused` | The service answered with a status other than 200. Its body is not read |
| `unreadable` | What came back is not a set of answers |
| `missing` | The answers leave out a question that was asked |

A question is yes-or-no (`noul`, answered with the probability of yes) or a
choice among named options (answered with a probability for each). The
state is handed over as the text that is sent, because the order of its
fields is part of what a model reads and a JSON value keeps none.

Jev is asked with one request at a time, on a connection that is kept
from one asking to the next:

    POST https://openrouter.ai/api/alpha/decisions
    Authorization: Bearer <key>
    Content-Type: application/json

    {"model":"typesafe/jev-1.13","state":{...},"questions":{"calls_lamp":{"type":"noul","instructions":"..."},...}}

and answers:

    {"model":"typesafe/jev-1.13-20260917","answers":{"calls_lamp":{"type":"noul","noul":0.88},...},"usage":{"input_tokens":1066,"output_tokens":134,"cost":4.4772e-05}}

What comes back is held strictly to what was asked. Every question has to
be answered, as its own type, with a probability from 0 to 1. A choice has
to be one of its options, and its probabilities have to add up to 1 within
0.02. Anything else fails the whole asking: the policy is never handed half
an answer. An answer is read by its stated length or in chunks, up to
256 KiB.

**The connection is kept.** Making one (the name, the connection, the
secure handshake) took 0.20 to 0.29 s of a decision that took 0.57 to
0.79 s from lamp-4ace, so it is made once and used again:

- After an answer the connection is kept for 150 s. A conversation's turns
  come closer together than that.
- A judge can be told to get ready (`Decider::ready`). The policy does so
  the moment the voice begins to listen to someone, and Jev then makes the
  connection while they talk, unless it has one with at least 6 s to go.
  One that has carried no request yet is kept for 12 s.
- A kept connection that the other end has closed is found out when it is
  used, before a byte of an answer has come. Another is made and the
  request sent again, once, inside the same deadline. Nothing is counted
  as a failure. A request is a question with no effect, so asking it twice
  costs at most one request's price.
- A connection is used again only when its answer ended cleanly and the
  service did not say it would close: by its stated length exactly, or by
  a last chunk that came with its ending line. Anything else is read as it
  was before and the connection is let go.

Those two times are inside what was measured against openrouter.ai from
the office's network on 2026-10-14, outside this program, one connection
for each figure: a connection that had answered a request answered another
after 20, 45, 90 and 180 s left alone; one that had carried no request yet
answered after 10, 12 and 14 s and was found closed after 16, 18, 20 and
45 s. The second request on a connection, one question asked, was
answered in 0.52 to 0.57 s six times of seven and in 1.03 s once.

The deadline is over everything together: looking the name up, connecting,
making the connection secure, sending and reading. The system's own lookup
of a name takes no deadline, so it runs on a thread of its own and is
waited for only as long as the asking has left. Measured from the office's
network, outside this program, the service answered in 0.66 s in the
middle and under 0.76 s nine times in ten, and now and then took seconds.

## How long a decision takes

From the last piece of an utterance to the decision on the bus:

    the wait  +  up to 20 ms to notice it  +  the asking  +  up to 5 ms to take the answer up

The wait is what makes sure an utterance is over. While the voice is
listening to someone, more of what they say may come, and it is the pause
of 300 ms. Once the voice has said it is no longer listening, the text
that arrives is of something already said, and it is 80 ms. A voice whose
state is not known is taken as still listening.

Measured on the development Mac, ten times each, with a stand-in model that
answers at once: 314 to 324 ms after the last piece, 319 in the middle, and
7 to 9 ms after a piece marked `finished`. With the real model the asking
is added: see "On the lamp".

### A second asking when the first is slow (#1522)

The asking is the remote judge's own time, and in a busy room a person
waits for it while the voice holds the reply (`LAMP_GOING_ON_ASKS`). On
lamp-4ace on 18 October, of 1,472 model decisions, the 1,427 on a kept
connection took 573 ms in the middle and 989 ms nine times in ten, and the
44 on one made `ahead` 1,031 ms in the middle (the first builder, #1522). Request 28 that day
waited 0.8 s for a decision that took 1,003 ms on a kept connection: the
service's own tail.

With `--hedge-after-ms N` (off unless set; the first builder's choice is
550, about the middle), a question not answered within N ms is asked again
on a second connection of its own (`hedge::Hedged`, two judges, each on a
thread of its own with its own kept connection), and whichever answers
first is taken. The other's answer is dropped when it comes, and its
connection kept for the next question. A failure is not asked again: a
first asking that fails before N ms is the answer, as without it; once both
are out, one that fails waits for the other. If the second connection is
still out on an earlier question, nothing is asked again. Each second
asking is said in the journal once it ends, `decision asked again after
550 ms; the second asking answered first, 612 ms after the question`, or
`neither answered` when both failed or the deadline came first, so a day's
extra requests are counted with `journalctl -u lamp-policy | grep -c 'asked
again'`. At
lamp-4ace's spread about half the questions would be asked twice. Not
measured on a lamp yet: the check is a day's p90 of the voice's `the
Policy's decision came after` with it and without, and the count of second
askings.

## On the lamp

On lamp-4ace since 01:15 on 2026-10-14, as `lamp-policy.service`, in
shadow mode. The owner approved, that night, sending the recognised text
of what is said in the office to the decision model's service. The key is
in `/etc/lampOS/policy/jev.key`, readable by root alone, and systemd hands
the service its own copy (`LoadCredential`); the log is in
`/var/lib/lampos-policy`, which only the service's user can enter.

**The hello on a home lamp** (the first builder, 30 Oct, on the launch
testers' findings and on #4625): the product unit passes `--zone system
--hello --act hello --gate --hello-by-rule`, so Welcome back's hello, which
lamp-skills asks on `skills/ask` even with `LAMP_SKILLS_ASK=skip`
([skills.md](skills.md)), is decided here by the hard rules alone (talking,
twenty minutes since the last hello, one an hour to each person, kept
rules): a pass is `say_now` at once, with no wait for the model. The judge
is still asked for each pass, in shadow, off the answer's path, and its
answer is only written down ([below](#by-the-hard-rules-alone-4625)). Every
other question stays off; `--gate` also makes the moment question, which
the product's skills do not ask under `skip`.

**On a home lamp** (#811) the product unit asks Jev through V1's route, the
campaign-api proxy (`--endpoint
https://campaign-api.autonomous.ai/api/v1/ai/v1/jev/decisions`), as V1's own
Jev client does, and `jev.key` is the user's key, taken by setup (#806, the
owner's 16:13 decision), or copied once by `lamp-update install` from V1's
config on a lamp that ran V1. Tried from lamp-75c9 on 2026-10-16
with one synthetic question in this request's form and that lamp's key: 200,
`typesafe/jev-1.13-20260917`, the answer in its usual shape, with the
`User-Agent` `lamp-policy` sends (Cloudflare refuses Python's default one).
Each of those two requests opened a new connection and took 6.7 s and 2.4 s;
the service keeps its connection, but how a first asking through the proxy
fits the 3 s deadline is not yet measured.

Its first run there, a synthetic voice through a loudspeaker with nobody
in the office, eight lines played and seven heard as separate utterances:

| What the voice heard | Room | The policy would | The lamp did |
|---|---|---|---|
| "What is 2 + 2?" (said as "Hey Lamp, ...") | quiet | answer (0.95) | answered |
| "So, did you send the report to Maria yesterday?" | quiet | look up (0.57) | said nothing |
| "I did, but she has not replied yet. ..." | quiet | look up (0.68) | said nothing |
| "What is the capital of Italy?", no name | busy | look up (0.73) | said nothing |
| "Milan. What color is the sky on a clear day?" (said as "Hey Lamp, ...") | busy | answer (0.83) | answered |
| "I'm done, son's finished." (said as "And at sunset?") | busy | stay quiet (0.40) | said nothing |
| "Good night, plan." (said as "Good night, Lamp.") | busy | look up (0.74) | answered |

It never chose to answer other people's talk, and chose to answer both
questions that began with the lamp's name. The follow-up question was
lost before it reached anything that decides: the recogniser wrote
something else. The room came from `room/state` (quiet, then busy after
three lines the lamp left alone) and the eyes' count from
`humans/faces/tracked` (no face: nobody was there).

A decision took 574 to 785 ms once its first had made the connection
(1,146 ms), each on a connection of its own, after the 300 ms the program
waits to be sure an utterance is over. So it was known about 0.9 to 1.1 s
after the voice had the text.

### With the shorter wait and the kept connection

The same night, 01:47 and 01:55, two runs of one scene of eight played
lines (builds `c68966a0d` and `08b77a80c`), nobody in the office, the Mac
that played them heavily loaded. Fourteen decisions; the lamp answered
twelve of the fourteen utterances and the policy chose `answer` for those
twelve and `look_up` for the two it left alone. One line of each run got
no decision: the service sent no text for it and no answer, so there was
nothing to judge.

| | Before | After |
|---|---|---|
| The wait before asking (`waited_ms`) | 301 to 322 ms | 80 to 95 ms |
| The asking (`took_ms`) | 574 to 785 ms, 672 in the middle, each making its own connection (six decisions, after a first of 1,146) | 505 to 562 ms nine times of fourteen, 645 once, and 1,015 to 1,152 four times; 536 in the middle |
| From the text to the decision | about 0.9 to 1.1 s | about 0.6 to 0.65 s, and 1.1 to 1.25 s the four slow times |

`took_ms` runs from the asking thread taking the question to its answers
being checked. Of the seven decisions of the second run, the first was
sent on a connection made while the person was speaking (`ahead`) and six
on the one kept from the asking before (`kept`); none made a connection of
its own. The four slow askings were slow at the service: one was `ahead`
and two `kept` (the fourth was before the kind was written down). The
same two speeds showed in the measurement from the Mac: of nineteen
requests, fourteen took 0.50 to 0.57 s, one 0.65 s and four 0.97 to
1.03 s.

**It is still not early enough to hold an answer back.** The outcomes log
gives the time from each decision to the lamp's first sound. Of the twelve
answers, the decision came before the sound three times (by 47, 75 and
147 ms) and after it nine times (by 40 to 616 ms). The lamp's first sound
came 468 to 812 ms after the text of what was heard: the service writes
down what it heard and begins its answer at nearly the same time, so there
is about half a second between the text and the sound, and through
OpenRouter the model alone takes that long at its quickest.

What follows from that:

- **A judge that decides before the lamp speaks has to be on the lamp, or
  much nearer.** The signs the lamp has by itself are there at once: how
  the room is, whether it is in conversation, and, once they exist,
  whether it was called by name and whether the speaker is facing it.
  With a called sign the measurement reached 95% without any model.
- **The model's place is the second look**: about half a second later, to
  stop an answer that has just begun, to choose `look_up`, and to be what
  the quick judge is fitted from.
- **Asked directly, the model may be quick enough.** TypeSafe's own
  service is said to answer in 0.10 to 0.14 s. That has not been measured
  here and needs a key this project does not have.

Its own journal that night: eight lines of counts and times, and no word
of what was heard.

### With the name heard: first live run, and what the text cannot carry

04:26 the same night, build `2f422ad1`, with `lamp-name` running: eight
lines played, four to the lamp and four to other people. Four decisions
were made.

| What was played | Text the voice got | Name heard | The policy would | The lamp did |
|---|---|---|---|---|
| "Hey Lamp, what is two plus two?" | none | | no decision | answered |
| "So, did you send the report to Maria yesterday?" | the same, 4.3 s after it was played | no | look up (0.43) | said nothing |
| "I did, but she has not replied yet." | none | | no decision | said nothing |
| "Hey Liam, are you coming to lunch?" | the same, **14.7 s after it was played**, during the next line | | | said nothing |
| "Hey Lamp, what color is the sky on a clear day?" | none | 0.92 | (look up, 0.76, decided on the line before) | answered |
| "The build finished about an hour ago." | "The bill finished about an hour ago." | no | stay quiet (0.25) | said nothing |
| "Hey Lamp, how many days are in a week?" | none | | no decision | answered |
| "Hey Lamp." | "Hey, Land." | 0.9998 | answer (0.90) | answered |

What worked: the two lines that had both a text and a moment of their own
went as they should. Talk with no name in a busy room was left alone at
0.25, and a call whose text was "Hey, Land." was answered at 0.90, which
without the name heard would not have reached `answer`.

What it showed of the policy as built:

- **Three of the four lines to the lamp never had a text**, and one of
  the four to other people. The service
  answered the three and wrote nothing down of what it heard. The policy
  judges utterances by their text, so for those turns it does not exist.
- **A text can come a turn late.** "Hey Liam, are you coming to lunch?"
  arrived after its own turn had ended unanswered and 4.3 s into the
  next line. The policy joined it to the name heard in that next line,
  which was a call to the lamp, and the outcome written beside it says
  the lamp spoke: it did, to the next line.

So the text is not a reliable carrier of what was just said, and nothing
that acts can be keyed to it. The name heard belongs to its moment: it
comes from the sound. What follows for the design:

- **The judge that acts decides for a turn of the voice**, from the voice
  beginning to listen to its deciding the person has stopped, on the
  signs that belong to that turn: whether the name was heard in it, how
  the room is, whether a conversation is going, who is facing the lamp.
  None of those waits for a text.
- **The model is asked when a text comes inside its turn**, and its
  answer then weighs in or is the second look. A text that comes late is
  for the log.
- **The outcomes log is only as good as that joining.** Outcomes written
  so far are joined by the time a text arrived, and some are of the turn
  after.

None of this is built. The policy on the lamp is still the one that
judges texts, in shadow mode.

## The local second judge (task 17)

**What exists and what we take.** The [addressee study](said-to-whom.md)
compares the alternatives and recommends a small local model of the signs
already available. This implementation reuses `Decider`, the exact seven
typed questions, Serde, the private record format and the existing bus. It
uses eight bounded numerical features and logistic heads; it needs no new
crate, downloaded model, inference process or provider. It preserves Jev's
byte-pinned state and its separately fitted combining coefficients. V1's
addressed-speech and follow-up safeguards, and the limits of retaining them
here, are recorded in [reuse decisions](reuse-decisions.md#local-policy-judge-task-17-2026-10-14).

### Run and inspect

`lamp-policy-judges` is a new executable in the same crate. **It refuses to
start without `--local-judge` and always publishes `shadow: true`.** It
subscribes and publishes as `policy`, so First must stop the existing Policy
owner before replacing it for a played-scene check, then restore that owner
afterward. Do not start two Policy owners on the same bus. This command
does not stop, restart or configure any other process.

Build with `cargo build --release -p lamp-policy`. With the existing bus,
the already provisioned decision-service key and authorization for the
played scene, this one foreground command shows both judges:

```sh
./target/release/lamp-policy-judges --local-judge --show-judges \
  --key-file /ABSOLUTE/EXISTING/DECISIONS_KEY \
  --log-dir /ABSOLUTE/PRIVATE/JUDGE_LOG \
  --bus-root /run/lampos-bus --deadline-ms 100 --request-ms 3000 --seconds 60
```

The uppercase paths are operator substitutions, not supplied credentials or
deployment paths. An existing directory must be private; a new one is made
0700 and each JSONL file is 0600. Real text goes to Jev in network mode and
into these private logs. `--show-judges` prints question probabilities,
choices, kinds and microsecond timings; it does not print heard text, keys
or service-supplied model names. Use `--offline` **instead of** the key
option for a local-only check: the slow judge then explicitly reports
unavailable and no service is contacted. Offline mode refuses key,
endpoint and model options.

| Option | Meaning | Default |
|---|---|---|
| `--local-judge` | Explicitly enable this path | required |
| `--key-file` / `--offline` | Existing service key, or deliberately unavailable slow judge | exactly one mode |
| `--weights` | Absolute path to a validated weights JSON | embedded authored, unfitted seed |
| `--deadline-ms` | Final-choice budget from release by the utterance gatherer, 1–1000 ms | 100 |
| `--request-ms` | Separate slow-request timeout, 1–60000 ms | 3000 |
| `--seconds` | Finite qualification duration, 1–600 seconds | 60 |
| `--show-judges` | Print numeric judge notices | off |
| `--log-dir`, `--bus-root`, `--endpoint`, `--model` | Same path/service meanings as above | private log required; existing bus/Jev defaults |

The library API is `policy::run_with_local` with `judges::Options`; the old
`policy::run` has no second judge. A normal finite stop allows up to 100 ms
for log completion. SIGTERM still kills the process immediately, as with
the old executable; it does not produce a graceful receipt.

### Inputs and weights

The single data file is [`local-weights.json`](../models/policy/local-weights.json),
with licence, SHA-256 and reproduction in its [SOURCE.md](../models/policy/SOURCE.md).
Each question has an intercept and eight feature coefficients; a separate
local choice head combines its seven probabilities after subtracting their
all-unknown priors. Every unknown feature contributes **exactly zero**.
Thus all-unknown input returns the head's prior, not an observed negative.
The initial numbers are authored heuristics, **not fitted or calibrated**.

| JSON cue | Known encoding; missing or `null` is zero contribution |
|---|---|
| `name_score` | Finite 0–1, mapped to −1–1; no listening source is unknown, not score zero |
| `conversation` | True +1, false −1; unknown until a Voice state was received before the first transcript piece |
| `room` | `quiet` +1, `busy` −1; otherwise null; the existing context's room at gather release |
| `facing`, `face_age_ms` | True/false × freshness `1-age/2000`; either missing or age ≥2000 ms contributes zero |
| `since_lamp_spoke_ms` | `1-min(ms,40000)/20000`, from the frozen first-piece context |
| `word_count` | `min(words,12)/6-1`; count of alphanumeric runs; null when the utterance was cut at its byte bound |
| `duration_ms` | `min(ms,6000)/3000-1`; **always null in the live path**, since transcript receipt intervals are not speech duration |
| `text_called` | +1 only when true; false/unknown zero; exact positive name predicate mirrored from Voice |

The face observation is the newest retained receipt no later than the last
transcript piece, at most two seconds old. Only valid near boxes (height
at least 0.15) count. A known facing face wins; observed-away requires a
nonempty set of known-away near faces. Missing pose, invalid geometry,
empty view and no near face are not treated as evidence of looking away.
Voice-known and face histories retain at most 32 observations apiece.
Source disappearance makes later cues unknown. These are **receipt-time
associations**, not microphone-clock alignment, gaze or speaker identity;
the face may belong to a listener rather than the speaker. They do not
use the turn's number `voice/heard` carries.

Local implements `Decider::ask` with the exact seven question definitions.
It parses at most 4 KiB of cues, performs bounded arithmetic and returns
seven finite typed probabilities without filesystem or network calls.
Weights are validated before the run, at most 64 KiB, with finite bounded
coefficients and fixed names/order. `names_someone_else` and `asks_to_stop`
have constant priors in the seed: these cues cannot establish their
semantics. This judge supplies no new stop-word or interrupt authority.

### One final choice and a late reply

The local answer is calculated immediately when an utterance becomes
eligible and recorded before waiting for the slow answer. One worker asks
Jev with the unchanged state. A slow answer contributes only if it is
complete **and observed by the loop strictly before the original choice
deadline**, and the choice is still open. The 3-second request timeout
allows a genuinely late result to be audited after the 100 ms choice.
The two durations are independent. The new path skips the old warm-up
request, which could occupy that worker before the first asking.

Each judge has its own calibrated-or-seeded choice calculation. With both
valid, the choice probability is `local_share * local_probability +
(1-local_share) * Jev_probability`; the shipped share is 0.5 and remains
unfitted. With only one valid, its probability stands unchanged. Jev's
fitted coefficients are **never applied to local question probabilities**.
With neither, the stand-in probability is 0.5. Thresholds remain 0.4 for
`look_up` and 0.8 for `answer`, with this explicit operational guard:

**If no timely slow answer and no sign of a call, choose `look_up`.** This
holds in quiet rooms too, even if the local probability would have crossed
the answer threshold or fallen below the look-up threshold. A call sign is
a known conversation, a positive transcript-name match, a known facing
face younger than 2000 ms, name confidence ≥0.9 when the room is not
known busy, or a valid name score ≥0.999 in any room. The last condition
follows First's sure-name change in `e07449e13`; a 0.993 meeting detection
still cannot open the busy-room guard. These are call evidence conditions,
not a claim that the seed's probability will exceed the answer threshold.
The probability is retained; `guard` records why its usual threshold
choice was overridden. `from_rule` means neither model supplied a valid
probability, not that this guard fired. These remain hypothetical choices:
`look_up` sends no motion command, and cannot prevent Voice speaking.

There is only one outstanding slow request. Later utterances use the local
judge while it is occupied. If a newer utterance overtakes an older open
choice, the older choice closes first using its local result; the newer
one follows in utterance order. The old slow reply is logged under the old
run/decision ID and cannot change a published choice, a later utterance or
its outcome watch. A failed or disconnected slow judge leaves local
decisions running. The existing waiting queue stays bounded at eight.
Stopping closes an open choice locally and names the unobserved slow
request; it does not invent that request's result. One misbehaving worker
can remain detached for that process run; no replacement is spawned.

The 100 ms target is an internal decision budget after gathering, not a
real-time guarantee. The loop revisits an open choice every at most 5 ms,
shortening its last wait to the remaining budget;
OS scheduling, bus work and caller-provided note handling can add delay.
The normal 300 ms gather pause (80 ms after Voice stops listening), ASR,
audio transport and speaker latency are outside it. At startup and in the
library API, weights/directory validation can perform I/O before listening.

### Audit and comparison

`models-YYYY-MM-DD.jsonl` records immutable weights, question names, run ID,
budget and requested slow model. `judges-YYYY-MM-DD.jsonl` contains:

- `local`: immediate seven answers, frozen cues and measured local time.
- `slow`: seven answers or a sanitized failure kind, worker completion and
  loop receipt times, `used`, `after_final` and any earlier close reason.
  `late_or_closed` also includes a reply after an early superseding choice;
  the times distinguish it from a wall-clock deadline miss.
- `choice`: exactly one final choice per decided utterance, both judge
  slots (each with a `status`: `answered`, `failed`, `pending`, `late`,
  `remote_busy`, `worker_gone` or `deadline_before_remote`), probabilities, guard,
  context, request if supplied, comparison snapshot and exact boundaries.
- `shutdown`: comparison totals, failures, any unobserved slow request and
  still-open outcome. These are not silently labelled completed/silent.

Join judge rows by `(run, number)`. The existing outcome records retain
their timestamp-based association and can be ambiguous; they are not proof
of a stable acoustic turn join. The bus `Decision.answers` is a descriptive
share-weighted blend when both judges contribute, or the one valid set.
The final probability comes from the separate choice calculations above,
not from reapplying a fitted head to that blended map. `Decision.by` is
`local`, `jev`, `local+jev` or `rule` in this mode.

A bounded writer has one worker, 64 queued records, at most 64 KiB per
record and one in-flight append. The decision loop serializes within the
bound and attempts the queue without waiting on disk. A full/failing log
does not stop decisions. Queue acceptance is not a completed write: in
this mode `Outcome.logged` and `Totals.unlogged` describe queue admission
(the latter counts rejected final-choice rows). Once a second diagnostics
report queued/written/failed/dropped/pending counts; the finite shutdown
receipt also distinguishes abandoned work. `written` is a successful append,
**not fsync or power-loss durability**. A stalled write may remain pending
after the 100 ms grace; the worker is cancelled without spawning another.
An abruptly killed process can lose queued evidence.
This bounds memory and pending work, not the daily archive's disk size.
Like the original Policy records, these dated files have no Records deletion
adapter; the finite command does not authorize removing an active writer's
files. Provision and retain the private qualification directory deliberately.

The mirrored strict busy-room rule is compared on each runtime snapshot,
with separate 2×3 tables for `play`/`hold` against the three Policy choices.
The source is now `e07449e13` (`first_output_busy` as well as `decide_busy`
and `called`), including the retained sure-name condition introduced after
the original task 17 host run. Model records and fitting reports identify
this revision.
Runtime rows are labelled `same_snapshot`: completed text, Policy's
20-second conversation and near-face receipt view, with `own_turn:false`
because this is an utterance being judged. `named_sure` is computed from
the supplied Policy name score at 0.999; no positive score means no
positive sure-name evidence in that proxy. This is a declared proxy, not
Voice's actual first-output gate. Recorded `voice_first_output` snapshots
need Voice's real prefix, 45-second state, own-turn flag, retained
`named_sure` bit and fresh any-face result. Voice can retain an earlier
qualifying hearing even after a weaker one arrives; do not recompute that
bit from the latest score. The [comparison contract](said-to-whom.md#task-17-local-judge-and-busy-rule-comparison)
keeps the tracks and missing-evidence denominators separate. Permission is
not an observed audible answer. No independently labelled lamp dataset was
available to Fourth; synthetic differences do not establish improvement.

### Fitting from First's labelled scenes

The offline executable takes one JSON object per line:

```sh
cargo run --release -p lamp-policy --bin lamp-policy-fit --offline -- \
  /ABSOLUTE/SCENES.jsonl /ABSOLUTE/NEW_WEIGHTS.json /ABSOLUTE/NEW_REPORT.json
```

An authored schema example, not a lamp recording:

```json
{"schema":1,"id":"scene-a-turn-1","session":"scene-a","split":"train","source":"authored schema example; no recording","rights":"Apache-2.0 example","cues":{"schema":1,"name_score":null,"conversation":false,"room":"busy","facing":null,"face_age_ms":null,"since_lamp_spoke_ms":null,"word_count":4,"duration_ms":null,"text_called":true},"labels":{"calls_lamp":true,"names_someone_else":null,"others_business":false,"lamp_could_answer":true,"follows_lamp":null,"listening_noise":false,"asks_to_stop":null},"directed":true,"baseline":{"kind":"same_snapshot","room":"busy","own_turn":false,"going_on":false,"named_sure":false,"text":"Lamp, what time now?","facing":null}}
```

`schema`, unique `id`, `session`, `split`, `source`, `rights`, `cues` and
`labels` are required. Split is `train`, `calibration` or `test`; supply at
least one session in each, with **no session in more than one split**.
IDs/sessions are nonempty, at most 128 bytes; source/rights declarations
at most 256 bytes each, with no control characters and at most 64 distinct
values of each. They declare provenance; the program cannot verify rights
or whether two names secretly refer to the same encounter. Keep related
people, sessions and days together when assigning groups.

Each of the seven semantic labels is an independent boolean or null;
omitting a nullable cue/label also means unknown. `directed` is the
independent intended-addressee label, required on calibration/test rows
and optional on training rows. Do not derive labels from what either judge
or the lamp did. `baseline` is optional; if present, every one of its
members is required, including explicitly nullable `facing` and boolean
`named_sure`. The latter has no default: old task 17 snapshots without it
are rejected, not silently reinterpreted under the newer rule. Preserve
those original records and their old source revision; omit the baseline
when this observation was not recorded. Missing baseline evidence is excluded from paired error counts. All object schemas
reject unknown fields. Cues use schema 1 (default when omitted), as above.
Live rows must leave duration null unless acoustic boundaries were actually
measured. The example alone is insufficient to fit a model.

The reader caps input at 16 MiB, 10,000 rows and 16 KiB per line including
its newline. Empty, malformed or oversized input fails without echoing it.
It fits question heads on **train** only. A head without both positive and
negative labels becomes a clearly reported prior-only constant with zero
feature weights. It then freezes those heads and fits the separate local
choice on **calibration** only; both directed classes are required there.
The deterministic full-batch optimizer uses 300 steps, learning rate 0.25,
L2 0.01 on coefficients, clipping each coefficient/intercept to ±16; the
library caps steps at 500. The 0.5 judge mixture remains unfitted.

The thresholds themselves are fixed, not optimized by the fitter. The
calibration split fits the separate choice head, not an answer threshold.
Only **test** rows contribute to the reported Brier/log loss, threshold
choices, per-question metrics and paired baseline errors. Those scores
describe the fitted local model without the runtime fallback guard or a
timely Jev contribution. They do not measure the fused runtime. Reports
include class counts, unlabelled heads, source/rights and snapshot coverage.
Unlabelled per-question metrics are null, not zero loss. Outputs use
create-new 0600 files and never overwrite an existing path or symlink.
The two output files are not one atomic transaction: a later write failure
can leave a partial/new first file, and the command exits unsuccessfully.
Accept a model only with a successful run and its matching report.

### The first lamp-record fit: abstention, not qualification

First supplied [89 labelled played-scene rows from lamp-4ace](verification/policy-scenes-lamp-4ace-20261014/README.md)
on 2026-10-14. Fourth reproduced the committed weights and report byte for
byte on the development Mac. The [fit audit](verification/local-judge-fit-20261014/README.md)
keeps the commands, input hashes, score ranges, class coverage and limits.
There are 10 directed training rows: three named calls and seven follow-ups.
Calibration has nine directed rows, test eight. The question heads learn
from train; the choice head learns from calibration; neither changes the
fixed 0.8 answer boundary. Every fitted score is below it, including train
and calibration, so the zero-answer test result follows from the model.
No fitting or threshold-mapping defect was found.

That result is consistent with this sparse, overlapping evidence; it is
not a usable model or proof that the probabilities are calibrated. The
calibration split contains no near-name negatives, while the held-out test
has six. Several test negatives score above every test positive. A lower
threshold selected from this test set would not establish discrimination.
Keep these rows and splits frozen; collect independent training/calibration
coverage and a new held-out qualification before choosing another policy.

An explicit-call override would be a separate policy decision, not a
correction to the fit. The supplied `text_called` cue is true on three
directed and four non-directed rows; raw name score at least 0.999 occurs
on three directed and one non-directed row. Neither is a provider-certified
addressee or Voice's retained name latch. This evidence does not justify
forcing an answer from either cue. The live Voice rule remains First's
decision, and the local judge remains opt-in and shadow only.

The 14 `same_snapshot` permission errors and First's reported 0/63 audible
intrusions concern different stages and cohorts. Policy evaluates completed
utterances; Voice's attention gate runs only when the provider proposes
output. Provider silence has no first-output gate verdict. The
[proposed first-output receipt](verification/local-judge-fit-20261014/voice-first-output.md)
specifies the existing seven-field baseline object, a separate provenance
sidecar, and the transcript identities needed for a defensible join. This
documentation does not add a publisher or reconstruct missing captures.

### Host verification and what First still must show

The reproducible commands, machine, first/repeated timings, synthetic
permission differences and full gate are retained in the
[original task 17 verification record](verification/local-judge-host-20261014/README.md).
The [post-rebase comparison record](verification/local-judge-floor-20261014/README.md)
audits First's new sure-name exception, passes 143 tests on each host, and
retains the new nine-case comparison and repeated measurements.
All Fourth measurements are development-host measurements. No provider was
used beyond local HTTP/TLS test stand-ins; no lamp or real key was accessed.

For First: run the foreground command during played scenes, with one Policy
owner. Start with a quiet-room scrap and no call, then a name inside busy
table talk with a score below 0.999: absent a timely slow answer both should
show `look_up`. Also compare a score of at least 0.999 without a facing
observation: the current baseline permits it, while the seed can still choose
`look_up` from its probability without a guard override. Compare
an explicit call, a nameless follow-up and known/unknown/away face poses.
Keep the separate Voice first-output snapshot and independently annotate
the addressee; inspect both seven-answer records and every late result.
Measure first use and repeated eligible-to-choice time on that lamp,
separately from last sound to final audible response. Stop normally to retain
the queue/write receipt. Do not infer absence of speech from shadow choices.

**Most likely wrong on the lamp:** the unfitted feature/choice weights,
conversation carryover, and receipt-time association of a face or acoustic
name with the utterance. No test here establishes intended-addressee
accuracy, semantic stop detection, privacy in a shared room, or improvement
over the current busy-room floor. Fitting and a held-out lamp comparison
remain necessary before any action wiring.
The retained verification also reproduces an existing name-association
fixture's timing sensitivity: a preceding detection can still lie inside
the next utterance's one-second pre-roll window. That existing behavior
and test were not changed by this task.

## Another model, asked the same questions again (#871)

`lamp-policy-replay` asks the attention decisions in the decisions log
again, of each model given, one after the other, so that a new version of
the decision model can be laid beside the one in use before the lamp is
switched to it. Each question is the one sent on the day: the state text is
put together again from the logged `context` by the same code
(`attention::state_text`), with the same seven questions, and each model's
answers are made a choice by the same weights. Those weights were fitted to
`typesafe/jev-1.13`'s answers on 2026-10-13: another model's answers go
through them as they are.

    lamp-policy-replay --key-file ABSOLUTE_PATH --log-dir /var/lib/lampos-policy \
        --days 2026-10-14,2026-10-15 --models typesafe/jev-1.13,typesafe/jev-1.14 \
        [--labels ABSOLUTE_FILE] [--only-labelled] [--endpoint URL] \
        [--deadline-ms 10000] [--pace-ms 500] [--most N] > replay.jsonl
    lamp-policy-replay --again replay.jsonl --labels ABSOLUTE_FILE [--refit [--cost-ratio N]]
    lamp-policy-replay --agreement --log-dir /var/lib/lampos-policy --days 2026-10-17,2026-10-18 \
        [--from 2026-10-18T11:00:00Z] [--until 2026-10-19T00:00:00Z]

- **`--agreement`** (#705, the Policy's wait on a held call) asks nothing.
  For each decision a model made, it says what the rule (`fallback`) would
  have said of the same context, and counts the two by kind: the room,
  whether the lamp was in conversation, whether it was speaking, and the
  name's score (`none`, `<0.5`, `0.5-0.9`, `0.9-0.99`, `>=0.99`). Taking the
  turn means `answer` or `look_up`, or a `stay_quiet` whose `follows_lamp`
  is 0.6 or more (the voice's `FOLLOWS_FROM`): the voice runs the lamp's own
  calls on it, where the rule's plain `stay_quiet` holds them (Fourth and
  the reviewer on #1510). It prints one line per kind (decisions, agreed,
  the rule took it and the model stayed quiet, the other way round, and how
  many of the model's were a following `stay_quiet`), then the kinds with at
  least 20 decisions and no disagreement: where the rule could decide
  without asking, once a night in shadow has borne it out (Fourth's
  conditions on #705). `--from` and `--until` (UTC,
  `YYYY-MM-DDTHH:MM:SSZ`) keep only the decisions made from and before
  those moments, so the night is counted apart from the days its kinds
  were picked on: they are picked before the night and only tested on it.
  A day past its month's end is refused, not rolled over. The run first
  says how many decisions it read from the days and how many fall in the
  window, so an empty table from a missing file reads apart from an empty
  night. 20 of 20 agreeing still leaves a true disagreement
  rate up to about 14 % likely at 95 %, so the list is for the shadow night,
  not proof. A line cut inside a character is counted unread here and when
  asking, and passed over by `--pictures` (which counts no unread lines),
  so no mode ends on one. Counts only, no words.
- **Where:** on the lamp, where the logs are. They hold what was heard, so
  they stay there. What is heard is sent to the model's service again, as on
  the day. The day of a file is UTC's.
- **What it prints:** one line for each decision and then a summary, and
  **no words**. Each line has the decision's place (`2026-10-15#139`, the
  file's day and the line's number) and moment, the room, whether the lamp
  was in conversation and whether its name was heard, the kinds of what was
  heard (`words`, `names_lamp`, `other_language`), what was chosen on the
  day, what should have happened if labelled, and each model's choice,
  probability, time (`took_ms`), name as its service gave it and seven
  answers (numbers, by the question's name, for fitting the weights to a
  model they were not fitted to), or why it failed.
- **The summary,** for each model: how many it answered and how fast
  (`took_ms` p50, p95, most), what it chose, how often its choice was the
  logged one, and how often it did what should have happened: over all, for
  requests that name the lamp, in a busy room and for another language. Then
  how often each two models chose alike.
- **Labels:** one JSON line each, `{"at_unix_ms": N, "should": "answer"}`
  or `"stay_quiet"`, the moment as the lines give it. `answer` is met by
  `answer` or `look_up`. A run can be labelled after it is seen:
  `--again` counts its printed lines again with the labels, asking nothing.
- **`--refit`,** with `--again`: whether a model loses only because the
  weights were fitted to another model's answers. For each model, the
  weights are fitted again to its own seven answers, the quiet room, the
  conversation and the name heard (a logistic regression, as the day's
  were, its weights pulled towards nothing as scikit-learn's default
  `C = 1` pulls them), on half the labelled lines, and counted on the other
  half; 200 times, on halves drawn from one seed, the same for every model
  and every run. Beside it, the run's own choices (the day's weights),
  counted on the same halves. It prints one more line, `refit_summary`:
  for both, the mean right over all, for requests that name the lamp, in a
  busy room and for another language, and how often each answered what was
  not for the lamp and ignored what was; and the weights fitted on every
  labelled line, to be read. It needs lines with answers (a run from
  `28a38cbb1` on). A model is not fitted, and its entry says why
  (`why_not`) and gives **no counts**, so none is taken for measured: with
  fewer than 20 labelled lines with answers, with those lines all of one
  label, or with no half that could be fitted. A half whose fitted part is
  all of one label is passed over; the counts are means over the halves
  fitted (`halves_fitted`, beside `halves_unfitted`).
- **`--cost-ratio N`** (1 to 20, default 1), with `--refit`: an answer that
  was not for the lamp costs N times an ignored request (#871, the first
  builder: the product is "speaks when spoken to"). The fit counts each
  example to be left alone N times, and each count says `cost`, N for each
  answer not for the lamp and 1 for each request ignored, for the day's
  weights and the refit alike; the summary says `cost_ratio`. The 1.13 and
  1.14 comparison asked for uses 3. The day's weights are two sums, with
  and without something listening for the name; this is one sum, the name
  heard one more term, not heard and not known both counted as not heard.
  It changes nothing the lamp does.
- **The kinds are rough:** `names_lamp` is one of the ways people call it or
  the recogniser writes its name, as a word; `other_language` is a letter
  outside English's, `¿` or `¡` (not the call "Lamp ơi" itself), or three
  words or more with no common English word. They count lines apart and
  decide nothing.

Tests: `crates/policy/src/replay/tests.rs` (the log read with each line's
place, kinds, each model asked the text sent on the day and choosing by the
same weights, no words in a line, the counts, labels, counting again),
`crates/policy/src/replay/refit/tests.rs` (a model whose answers are on
another scale done badly by the day's weights and as well as the other
once fitted to its own, on the same halves; a run without answers not
fitted; labels of one kind, on every line or on a model's answered ones,
not fitted and given no counts; the weights found from examples drawn from known ones; the same
halves every time) and
`crates/policy/tests/it/replay_program.rs` (the program against a service
played on this machine: two models asked in turn, the state sent equal to
the day's, and none of what was heard in its output).

### Timing each kind of connection: `--probe-connections` (#1522)

On lamp-4ace on 18 October the 44 askings on a connection made `ahead`
took 1,031 ms in the middle, and the 1,427 on a kept connection 573 ms, of
1,472 model decisions (the first builder on #1522). A connection made ahead is fully
ready before the question: `open()` finishes the handshake. But lamp-policy
makes one only when no kept connection has 6 s left of its 150 s, so in
practice it is always the first question after 2.5 quiet minutes: a new
connection and a quiet spell together. `--probe-connections` takes the two
apart:

    lamp-policy-replay --probe-connections --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR \
        --days YYYY-MM-DD[,...] --endpoint URL [--models NAME] [--deadline-ms 10000] [--rounds N]

It takes the newest logged attention question of those days that a model
decided, and asks it seven ways, on one judge kept for 600 s after an answer
(`replay::PROBES`), about 17 minutes and seven askings a round. Never one
the rule decided alone (`--rule-decides`), whose words stayed on the lamp
(`replay::newest_asked`), and only to the `--endpoint` given, which is
required: the one lamp-policy asks, not the built-in default (the reviewer
on #1630):

| Probe | Before it | Sent on |
|---|---|---|
| `fresh_after_quiet` | 300 s with nothing asked | a new connection |
| `kept_at_once` | nothing | the same one |
| `fresh_at_once` | nothing | a new connection |
| `ahead_at_once` | nothing | one made ahead, asked 2 s later |
| `kept_after_140s` | 140 s | the kept one |
| `kept_after_300s` | 300 s, past lamp-policy's 150 s | the kept one, or `replaced` if the other end closed it |
| `ahead_after_quiet` | 300 s | one made ahead, asked 2 s later |

One line each as it is made: the probe, what it was sent on (`new`, `kept`,
`ahead`, `replaced`) and `took_ms`, no words. Read it with what each
`took_ms` holds (the reviewer on #1630): a `fresh` probe's includes TCP and
the TLS handshake (0.20 to 0.29 s on lamp-4ace on 14 October), and
`fresh_after_quiet`'s a name lookup too, as addresses are kept 300 s; an
`ahead` or `kept` probe's holds neither. So:
- **A new connection's first request** is `ahead_at_once` against
  `kept_at_once`: a new connection, with no handshake in its time. If it is
  slower, holding the kept connection longer (or a second one warm) would
  help.
- **A path that goes cold in a quiet spell** is `ahead_after_quiet`,
  `kept_after_140s` and `kept_after_300s` against the at-once ones.
- **The handshake's own cost** is `fresh_at_once` against `ahead_at_once`.
- `kept_after_300s` also says whether the other end keeps a connection that
  long (`replaced` if not). `--rounds` (1 to 3)
repeats it. Run it with the room quiet: lamp-policy's own askings would
break the quiet spells. Tests: `the_connection_probe_asks_seven_ways_with_its_waits`
and `the_probe_asks_only_what_went_to_a_judge_on_the_day`.

## The lamp's picture with the question (#871, `--images`)

`lamp-policy --images MODEL [--sight-file ABSOLUTE_PATH]`, off unless set,
shadow. As the policy sends each attention question, it hands the same
utterance to a thread of its own, if that thread is free. **Only when the
eyes see someone** is anything asked: what they see now, their newest list
at most 2 s old, looked at when the asking begins and again at the moment
the picture would be taken, never only when the utterance ended (the
reviewer on #873: a question that waited in the queue while the person
left, or a list gone stale, sends nothing; the utterance's own `seen` is
still written in the line). on lamp-4ace on 2026-10-16 at 03:47 to 03:53, with nobody
at the desk, `picture_person` came back 0.26 to 0.51 thirteen times while
the eyes saw no face each time, so whether anyone is there is the eyes' to
say (the first builder on #871). Otherwise the line says `no_face`, or
`eyes_unknown` when the eyes said nothing fresh, and nothing is asked. The thread asks
`MODEL` (`typesafe/jev-1.14`) twice at once, on two connections:
- **without** the picture: the seven questions as sent on the day;
- **with** the picture: the state with `picture` last, the seven questions,
  and six about the picture, each its own simple question:
  - `picture_person`: is anyone in the picture at all (first: on an empty
    desk on lamp-4ace at 02:28 the others were answered near chance,
    "two people talking" 0.62 to 0.69);
  - `picture_faces_lamp`: is a person looking toward the camera;
  - `picture_absorbed`: typing, reading, on a call, in headphones;
  - `picture_talking_to_another`: two people facing each other;
  - `picture_asks_with_a_look`: a wave, or a look that asks;
  - `picture_away`: nobody at the desk, or asleep.

It writes one line for both to `pictures-YYYY-MM-DD.jsonl` in the log's
folder. Nothing acts on it, and the decision is never late for it: an
utterance that comes while the thread is still asking is passed over. Each
picture question is a sign for later, one among the name, the room, the
microphone's rise and the eyes' own count of faces turned to the lamp; none
decides alone.

**The picture, and when it may leave the lamp.** It is the eyes' newest
frame (`/run/lampos-eyes/sight.jpg`, written only with `lamp-eyes
--sight-file`), sent by the voice's rule ([eyes.md](eyes.md)):
- the eyes keep the file only while the voice permits the camera, which is
  exactly while the microphone may listen, so a muted lamp, or one whose
  privacy switch is on, has none;
- a frame older than 1 s, or larger than 64 KiB, is not taken, as the
  voice's `sight` takes none.

- the picture is taken at the moment the request is written, after any
  slow looking up or connecting, and again if it has to go on another
  connection: what is sent is the frame the eyes keep then, at most 1 s
  old, never one copied before (the reviewer on #873). If they keep none
  then (the camera was closed, so the microphone was), nothing is written
  and the line says `withdrawn`. The line's `age_ms` is the age of the
  frame actually sent.

TypeSafe takes 256 to 1,568 pixels a side and 78,400 in all, so the eyes'
320 by 240 frame is made 640 by 480 (each pixel repeated) and encoded
again. It goes as one of TypeSafe's `assets` with the
`TypeSafe-Danger-Zone-Images: true` header, which only that request carries.
The picture is written nowhere. The line says only whether one was sent,
its size, the eyes' size and its age, or why not (`no_face`,
`eyes_unknown`, `no_frame`, `stale`, `too_large`, `not_a_jpeg`,
`unreadable`, `withdrawn`, `not_written`).

**Sent** means some of a request with a frame in it left the lamp, in any
try, whatever came after (the reviewer on #873). A request written whole
on a kept connection that the service then closed with no answer was sent,
even if the next try, on a new connection, was withdrawn because the
camera had been closed meanwhile. So `picture` also has:
- `delivery`: `answered` if the service answered a request with the frame
  (it arrived), else `uncertain` (written, whole or in part, and whether it
  arrived is not known);
- `tries`: each try at writing it, `{"written": "whole", "answered": ...}`
  or `{"written": "part", "request_bytes_written": N, ...}`;
- `withdrawn`: true if the asking then ended because no frame might go.
The size and age are those of the last frame that went. `not_written`
means a frame was taken and not a byte of its request went.

**The line:**
- `at_unix_ms`: when the question was sent;
- `context`: what was heard, in words, as in the decisions log, in the same
  private folder;
- `seen`: the eyes' counts;
- `picture`;
- `without` and `with`: each has `by`, `took_ms`, its answers, and the
  `choice` and `probability` the same weights make of its seven answers,
  or why it failed.

**Counted, asking nothing:**

    lamp-policy-replay --pictures --log-dir /var/lib/lampos-policy --days 2026-10-16 [--labels ABSOLUTE_FILE]

The summary says how often a frame was sent, its `delivery` (answered or
uncertain), how often the asking was withdrawn after one went
(`withdrawn_after_sending`), and why none was sent. For each asking,
without the picture and with it, it gives how often it answered and
failed (by kind), `took_ms` p50/p95, and how often it did what should have
happened. Both are counted right on the same lines: a frame sent and both
chose. A line where either made no choice is in neither's right, and why
is in that asking's `failed`, so one is never counted on lines the other
is not. For each picture question, it says how
often its yes (0.5 or more) went with what should have happened, beside
the eyes' own `facing`; and again in `signs_given_a_person` for the four
that take a person for granted (faces the lamp, asks with a look, absorbed,
talking to another), only where `picture_person` was yes. `picture_person`
says how often the picture had a person in it, how often the eyes saw a
face, and how often the two agreed. Labels can be written by a scene's clock: each
label is given once, to the line nearest it within 3 s, whether or not a
frame was sent with that line, so a decision nobody labelled never counts as
labelled. A label equally near two lines is given to neither, and two labels
that disagree on one line are both dropped. Two labels at the very same
moment that disagree are refused when the file is read, whichever comes
first; the same label twice at one moment is one label. The summary's `labels` says how
many were given, matched, unmatched, ambiguous and conflicting.

**Not shown:**
- No picture was sent to the service from the Mac. Whether the decisions
  endpoint the policy uses (OpenRouter's) passes `assets` and the header on
  to TypeSafe is for the lamp's first run to show: a `refused` in every
  `with` would say no.
- On lamp-4ace the eyes' `--sight-file` has been off since 2026-10-13
  ([eyes.md](eyes.md)): without it every line says `no_frame`.
- The file is the eyes' user's and mode 0600: the policy has to run as
  that user to read it.

Tests:
- `crates/policy/src/pictures/tests.rs`: the voice's rule for a frame, the
  frame made as large as the service takes, the state and the thirteen
  questions, whether anyone is in the picture first; the picture told as sent if any try went; and the reviewer's
  case on #873, a request with the picture written whole on a kept
  connection the service closed unanswered, then withdrawn on a new one
  where nothing was written, told as sent, `uncertain`, withdrawn.
- `crates/policy/src/decide/tests.rs`,
  `what_became_of_each_request_with_pictures_is_told`: written whole on a
  closed kept connection and unanswered, then answered on a new one.
- `crates/policy/src/replay/tests.rs`,
  `a_picture_that_went_is_counted_and_both_askings_are_counted_on_the_same_lines`.
- `crates/policy/tests/it/pictures.rs`: the two askings against a played
  service, only one with the picture and the header, the line without the
  picture, nothing asked with no frame, and the thread passing over what
  comes while it asks.
- `crates/policy/tests/it/program.rs`,
  `with_images_the_question_is_asked_again_with_the_eyes_frame_beside_the_decision`:
  the program itself, the decision published as before and the picture's
  line beside it.

## Progress news: "say this progress now?" (#732)

Built by the second builder on 2026-10-15 (`second-P2`). **Off unless
`lamp-policy --progress` is given**, and shadow: it decides, publishes and
writes down, and nothing acts on it.

**What it is for.** The lamp hands computer work to Claude Code or Codex
and, like a good chief of staff, tells the person how it went
([tasks.md](tasks.md#the-lamp-as-chief-of-staff-watching-and-telling-2026-10-15)).
Finished work and questions are told by the voice's own rules. Progress
while the agent works is this question: the tasks part says on
`tasks/status` how each piece of work is going, as typed state without the
agent's words ([tasks.md](tasks.md#progress-on-the-bus-serve---progress-732));
the policy asks a decision model whether a change is worth telling now. The
owner's design: watching and deciding cost no tokens of the realtime model,
which is used only to say it. Jev is the first judge, asked through the same
`Decider` as the attention question, by a second `Jev` with its own
connection, on a thread of its own: a progress question never waits for an
attention one, nor the other way round.

| Option | What it is | If left out |
|---|---|---|
| `--progress` | Listen to `tasks/status` and ask the progress question | off |
| `--utc-offset-minutes` | The lamp's local time east of UTC, -720 to 840, for the hour of the day the model is shown | the hour is null |
| `--zone` | `system` or a zone's name, in place of the offset (#820) | the offset |
| `--act progress` | Its decisions act ([below](#acting---act-progress-752)) | shadow |
| `--rules` | It obeys the people's rules about speaking first ([above](#a-persons-rules-about-speaking-first-813)) | not listened to |

**When it asks.** For a task whose agent is `working` and whose progress
line has changed (`changes`) since it was last judged. What a task says the
first time the policy sees it is where judging begins, not news. Counts are
of one run of the tasks part (`run`): a restart keeps the store and the
task numbers and counts from nothing, so for a task already followed a new
run is judged from its own beginning, whatever count the last run reached
and at whatever count the new run is first seen (its first status may be
missed), and a yes about one run's change that comes back once another run
is there is `outdated`. While the tasks part is gone (its publisher's lease
ran out), nothing is asked and an answer that comes back is `outdated`; what
is kept of each task stays, so the quarter hour after a note holds across
the tasks part going and coming back as another run (the reviewer on #751,
17:05: `a_new_run_first_seen_at_a_lower_count_is_asked_about`,
`the_quarter_hour_after_a_note_outlives_the_tasks_part_going_and_coming_back`).
A task the tasks part lists as `unread` (watched, not read yet in this run)
keeps what is kept of it, and nothing is asked of it until a read of this
run comes; that first read is judged from the run's beginning at whatever
count it comes, even when an earlier read of the run was missed (the
reviewer on #793, 17:48); one in neither list has left the watch and is let go
(`a_task_unread_in_a_new_run_is_kept_and_not_asked_about_until_read`).
One progress question is out at a time.

**Two hard rules, around the decider and not left to it.** A question is
not asked while either holds, and is asked once it no longer does, about
the newest change:

- **Never while anyone is talking to the lamp:** the voice is listening,
  thinking or speaking, or something was heard or said by the lamp in the
  last 15 s (the voice's own news waits the same 15 s). The 15 s begin
  when the voice goes to `resting`, not at its last active `voice/state`
  before, which is said again once a second and can be most of a second
  old (the reviewer on #751, 15:26).
- **At most one note per task in 15 minutes:** a `say_now` for a task holds
  every later change of it for 15 minutes from that choice.

An answer that comes back once a rule holds (someone began to talk while
the model was asked) is turned to `wait`, written down with `guard`, and the
change is asked about again later. Talking holds it whatever became of the
task. Each hold is counted once for each change it holds.

**An answer about work no longer as it was asked about** is a `wait` with
`guard` `outdated`: another store, the task no longer watched or no longer
at work, or its line changed again (or was counted afresh) while the
question was out. It changes nothing kept for what is there now, so a task
of another store with the same number is neither judged nor held back for
15 minutes by it, and a newer change is asked about in its turn. Found by
the reviewer on #751 (15:23); tests
`an_answer_about_work_no_longer_as_asked_is_a_wait_and_changes_nothing_there_now`
and `the_quiet_begins_when_the_voice_stops` (`crates/policy/src/progress.rs`).

**What is known** (the log's `context`):

| Field | Where it comes from |
|---|---|
| `store_id`, `task`, `change`, `run` | `tasks/status`: whose work, which change of its line, and the run of the tasks part that counted it |
| `engine`, `state`, `working_s` | `tasks/status` |
| `idle_s` | The policy's wall clock less the agent's `last_active_unix_ms` |
| `changed_now`, `new_share` | `tasks/status` |
| `new_signs`, `told_signs` | Signs seen (`signs_seen`) less those of the notes already chosen for this task, and those |
| `notes_told`, `since_note_s` | Notes chosen for this task, and how long ago the last |
| `someone_here` | The eyes: a near face (as tall as 15% of the picture) in a list no older than 2 s; null without one |
| `quiet_s` | Seconds since anything was heard or said, or the voice listened, thought or spoke; null if nothing since the policy began |
| `room` | `room/state` |
| `spoke_up_last_hour` | Turns the lamp began itself in the last hour, and the notes this chose in it (below) |
| `hour` | The local hour of the decision's moment, by `--utc-offset-minutes` or `--zone` |

A turn the lamp began itself is told from `voice/state` alone: its first
state (`thinking` or `speaking`) came with nobody listened to in the 10 s
before. That is decided at the turn's beginning and kept to its end
(`resting`). The turn is counted once, at its first sound, however often
it speaks before it ends. An answer whose first sound is slow (the model
may take 30 s) is never counted (the reviewer on #883, 05:22, of the
hello's same count; test
`a_slow_answer_is_not_unasked_and_a_turn_is_counted_once`). Nothing on the
bus says so outright (task news and hellos are told as written turns
inside the voice), so this is a stand-in for that, and a late or missed
`listening` would count an answer as unasked. In shadow nothing is said, so the notes
this part chose are counted too. Acting (`--act progress`) they are not:
the voice says them, and they count once, as turns the lamp began itself.

**What the model is shown**, its fields in this order (made-up numbers):

    {"lamp":"A small desk lamp robot on the desk of the person it works for. It hands computer work to coding agents and tells the person how it is going, like a thoughtful chief of staff: only what they would be glad to hear, at a moment that suits them.","work":{"agent":"Claude Code","state":"working","minutes_at_it":12.6,"minutes_since_it_last_did_anything":0.2,"its_progress_line_just_changed":true,"share_of_the_line_that_is_new":0.67,"new_signs_of_a_step_forward":["the tests pass","a pull request was opened"],"signs_the_person_was_already_told_about":["something was found"],"progress_notes_told_for_this_work":1,"minutes_since_the_last_note_for_this_work":20.0},"room":{"someone_at_the_lamp":true,"seconds_since_anything_was_said":340,"room":"quiet: nobody else has been talking","times_the_lamp_spoke_up_unasked_in_the_last_hour":1,"hour_of_day":14}}

Not known is null. No word of the agent's or the person's is in it: the
policy never has them.

**What the model is asked.** Three questions, each answered with the
probability of yes. Every one begins "state.work is how a piece of
computer work that the lamp handed to a coding agent is going while the
agent is still at it, read from the agent's own progress line without its
words; state.room is how things are around the lamp now." and goes on:

| Name | The question |
|---|---|
| `worth_telling` | Is there a real step forward in state.work that the person who asked for the work would be glad to hear about, such as the tests now passing, a build that works, a bug found or fixed, or a pull request opened, rather than routine work going on? |
| `good_moment` | Is this a good moment for the lamp to speak up unasked with one short piece of news: someone is at the lamp, nobody has spoken for a little while, the room is not busy with other people's talk, the lamp has not spoken up unasked often in the last hour, and it is not late at night or early in the morning? |
| `too_much` | Would one more note about this work now be too much for the person: notes about it were told often or a short while ago, or its new signs are only ones they were already told about? |

**How they are combined.** As if the three were independent:
`probability = worth_telling × good_moment × (1 − too_much)`; `say_now`
from 0.5, `wait` below. Authored, not fitted: no progress decision has been
labelled. The log is what a fit is to be made from.

**When no model answers.** A rule: `say_now` only with a sign not told yet,
someone seen at the lamp, a minute of quiet or more (or nothing said since
the start), a room not known to be busy, fewer than two turns begun unasked
in the hour, and between eight in the morning and ten at night if the hour
is known; probability 0.6 for yes and 0.1 for no, authored and unmeasured.

**What is published** on `policy/decisions` (`lamp_msgs/Decision`):
`about` `progress`, `choice` `say_now` or `wait`, `probability`, the three
`answers` (empty when the rule decided), `by`, `took_ms`, `shadow: true`,
and `task`, `<store>:<task>:<change>@<run>`, which tells a later reader
which piece of progress it was about: the run of the tasks part is part of
its name, since counts begin again at each run ([msgs.md](msgs.md)). An attention decision
has no `task` and is written as before.

**What is written down**, one line in the same `decisions-YYYY-MM-DD.jsonl`
with `about` `progress`: `at_unix_ms`, `shadow`, `context` (above), `state`
(what the model was shown), `asked`, `answers`, `judgement` (probability,
choice, `from_rule`), `by`, `took_ms`, and when there is one `guard`
(`talking`, `told_recently` or `outdated`), `connection`, `failed`, `failed_why`,
`usage`. The journal gets one line with no words:

    lamp-policy: progress decision 4 task=3 change=7 choice=wait probability=0.214 by=model took_ms=561 guard=none

The diagnostics add `progress_decided`, `progress_by_rule`,
`progress_said`, `progress_held_talking` and `progress_held_recent`.

**Without the switch** `tasks/status` is not subscribed to (no socket is
made in its directory), no second judge or thread is made, and the
diagnostics carry `decided`, `by_rule` and `shadow` only, as before
(`without_the_switch_the_tasks_topic_is_not_listened_to_and_the_report_is_as_before`,
`crates/policy/tests/it/progress.rs`). An attention decision's bytes on the bus
are those it had (`crates/msgs/tests/it/tasks.rs`).

**Measured on the development Mac** (Apple M2 Max, macOS 26.6.2, release
build, other sessions running: load average 14 to 20), with
`cargo run --release -p lamp-policy --example progress_cost -- --samples 30`
and a stand-in that answers at once. The state and the arithmetic: 2.4 µs
in the middle of 1,000 (139 µs the first). A status published on a bus in
`/tmp` to its decision received there, through the loop, the asking thread
and the log: 52.7 ms in the middle of 30, 48.1 ms the first, 58.9 ms the
largest, most of it the loop's two 20 ms waits. A real Jev adds its own
0.5 to 1 s (above). **These are the Mac's, not the lamp's.**

**On the lamp** (the first builder): add `--progress --utc-offset-minutes
420` to `lamp-policy`'s `ExecStart` beside `lamp-tasks --progress`, and
after a day of real work handed over:

```sh
journalctl -u lamp-policy --since yesterday | grep 'progress decision'
sudo cat /var/lib/lampos-policy/decisions-*.jsonl | grep '"about":"progress"' > /var/tmp/progress-decisions.jsonl
grep -o '"took_ms":[0-9]*' /var/tmp/progress-decisions.jsonl | cut -d: -f2 | sort -n \
  | awk '{t[NR]=$1} END {print "decisions="NR, "median_ms="t[int((NR+1)/2)], "max_ms="t[NR]}'
grep -o '"choice":"[a-z_]*"' /var/tmp/progress-decisions.jsonl | sort | uniq -c
grep -o '"guard":"[a-z_]*"' /var/tmp/progress-decisions.jsonl | sort | uniq -c
```

For the ones a person would disagree with: each line's `state` beside the
progress line it was about (`get_progress` while the task runs, kept by
hand and not committed, since it is the person's work), marked `agree` or
`disagree` with a word why. The log itself holds no word of the agent's or
the person's.

**Most likely wrong.** That Jev, asked a question it was not built for, is
calibrated without examples, and that the product of three answers is a
fair probability. That `voice/state` tells an unasked turn from an answer.
That the 15 s of quiet and the hold of a `yes` that comes back while
someone talks are the person's sense of "talking to the lamp". That the
signs in the state are what made the step worth telling.

**Not shown.** No real model was asked: every test and measurement used a
stand-in. Nothing ran on a lamp, against real `tasks/status`, or for longer
than a few seconds. In shadow nothing acts on these decisions.

### Acting: `--act progress` (#752)

Built by the second builder on 2026-10-15. Since 06:58 that day lamp-4ace
runs the question above in shadow. The owner: build every component of the
brain first, then perfect each one; the policy is the part that decides
when the lamp speaks first, and until now it decided nothing. **Off unless
`lamp-policy --progress --act progress` is given.**

- **What acts.** Only the progress question. Its decisions go out on
  `policy/decisions` with `shadow: false`; "is this said to the lamp?" is
  published with `shadow: true` as before, in the same process. The log
  line of each progress decision says `"shadow":false`; the log is written
  for every question either way.
- **Who acts on it.** The voice's task news, with `LAMP_PROGRESS_NEWS=on`
  ([tasks.md](tasks.md#the-voices-side-of-progress-news-752)): a `say_now`
  at most 60 s old, of the store and about the newest change `tasks/status`
  shows, is fetched and told under the voice's own gates. A `wait` does
  nothing. Neither side acts without its own switch.
- **The hard rules** are unchanged and hold before the voice's gates: no
  question while anyone is talking to the lamp, at most one note per task in
  15 minutes. Acting, the notes chosen are no longer added to the turns the
  lamp began itself in the hour: the voice says them, and `voice/state`
  counts them once.
- **What it says of itself.** The start line ends `progress=true
  act=progress`, and the diagnostics add `progress_acts` `true`.

**Without `--act`** the decisions are `shadow: true`, their log lines
`"shadow":true`, and the diagnostics carry no `progress_acts`, as on the
branch before (`a_change_is_asked_about_once_decided_in_shadow_and_written_down_with_its_state`);
with it, `acting_a_say_now_goes_out_for_real_while_the_attention_question_stays_shadow`
(`crates/policy/tests/it/progress.rs`) shows `shadow: false` for progress and
`true` for attention. The chain from a change on `tasks/status` to one note
offered by the voice runs with this crate's own loop and a stand-in judge in
`crates/voice/tests/it/progress_news.rs`.

**On the lamp** (the first builder): the steps are in
[tasks.md](tasks.md#playing-progress-news-on-lamp-4ace).

**Most likely wrong.** That Jev's yes to the progress question is a yes a
person would give: with nothing labelled, the first real decisions are the
first measurement. That one note in 15 minutes per task is few enough, and
several tasks at once still more than a person wants.

## Hello on arrival: "say hello now?" (#752)

Built by the second builder on 2026-10-15. **Off unless `lamp-policy
--hello` is given**, and shadow unless `--act hello` is given too.

**What it is for.** The Welcome back skill says hello to someone who comes
back and sits down, and asks this question first (`--gate`, below). The
voice's own hello, which held a due hello up to 2 s and asked on
`voice/arrival` (`LAMP_HELLO_ASKS_POLICY=on`), was removed (#2825): nothing
publishes `voice/arrival` now, and its reading here is unfed
([speaking-first.md](speaking-first.md#asking-the-policy-752)).
It is asked by a judge of its own, a third `Jev` with its own connection, on
a thread of its own.

| Option | What it is | If left out |
|---|---|---|
| `--hello` | Listen to `voice/arrival` and ask the hello question | off |
| `--act hello` | Its decisions act: `say_now` says the hello, `wait` keeps it. Needs `--hello`; `--act` may be given once for each question | shadow |
| `--hello-by-rule` | The hard rules alone decide: a pass is `say_now` at once, the judge asked only in shadow ([below](#by-the-hard-rules-alone-4625)). Needs `--hello` | the judge decides a pass |
| `--hello-quiet to-the-lamp` | The 15 s quiet counts only talk to the lamp or by it ([below](#the-quiet-counts-talk-to-the-lamp-1155)). Needs `--hello` | all talk counts |

**Two hard rules, around the decider, and a third with `--rules`.** While
one holds the answer is `wait`, by the rule (`by` `rule`, a `guard` in the
log), and no model is asked: the voice is waiting, and a question left
unanswered would leave the hello to its rule.

- **Never while anyone is talking to the lamp:** the voice listening,
  thinking or speaking, or anything heard or said in the last 15 s. With
  `--hello-quiet to-the-lamp`, the 15 s count only talk to the lamp or by
  it ([below](#the-quiet-counts-talk-to-the-lamp-1155)).
- **No hello within twenty minutes of the last:** the voice's minutes since
  its last hello, or the last `say_now` this acted on.
- **No hello to the same person within an hour** (`ONCE_PER_PERSON`), for a
  skill's ask (`--gate`): by the ask's `person`, the memory's id of the
  known face at the lamp, which lamp-skills reads from the eyes' known file
  as it asks (never the name); every ask without one, a face not known,
  shares one key, `someone`. Counted from the last `say_now` this acted on
  for that person, as the twenty minutes are, so nothing in shadow starts
  it. Guard `greeted_person`, checked after the two above and before a kept
  rule, and again as an answer comes back. The first builder, 30 Oct, on
  the launch testers' findings: on stable 8899 Welcome back greeted the
  same person every time they came back after five minutes away, as soon
  as its 20-minute cooldown let it (15:13 and 15:41 on one lamp); the ruling
  is at most once an hour per person. Tests:
  `hello::tests::one_hello_an_hour_to_the_same_person_and_asks_without_one_share_a_key`,
  `hello::tests::a_hello_in_shadow_starts_no_persons_hour`.
- **With `--rules` (#813), not when a rule the people at the lamp kept
  holds it** ([above](#a-persons-rules-about-speaking-first-813)): `quiet`,
  `quiet_mornings`, and the `hello` limits (`never`, not before or from a
  minute of the day, not while the room is not known to be quiet). Guard
  `kept_rule`. A limit on `news` holds no hello.

<a id="by-the-hard-rules-alone-4625"></a>
**By the hard rules alone (#4625, `--hello-by-rule`).** The first builder,
30 Oct, on #4625: #4305 took Jev out of everything the lamp does, and the
rules as built are the policy, so on the product and on lamp-4ace the hello
is decided by the hard rules alone: someone talking, twenty minutes since
the last hello, one an hour to each person, and kept rules with `--rules`.
The product units pass it. lamp-4ace's unit does not yet: its pinned
lamp-policy predates the option, and lamp-policy exits on an unknown
option, so it comes with the change that pins a build with #4625 there.
With the switch:

- **A pass is `say_now` at once**, by the rule: `by` `rule`, probability
  `RULE_YES` (0.6), `took_ms` 0, no answers. There is no wait for the model
  on the answer's path. Acting (`--act hello`), it starts the twenty minutes
  and the person's hour as any `say_now` acted on does. The room, the hour
  and the lamp's own speaking up, which the rule weighs when no model
  answers without the switch (`fallback`), are not weighed: they are the
  judge's questions, and the judge does not decide.
- **The judge is asked in shadow** for each pass, beside it: the same
  state and questions, with at most 1.5 s, after the decision is published.
  The decision's log line says `judge` `asked_in_shadow`, or `not_asked`
  when the judge's thread was still busy with an earlier pass or gone. When
  its answer comes, a line of its own is written to the same log, `about`
  `hello_judge`, `shadow` true, never published: the context and state, the
  `decided` choice (`say_now`), the judge's `answers` and `judgement`, and
  `differs` when it would have decided otherwise; or `failed` when it gave
  no usable answer. The journal says it in one line, and the diagnostics
  add `hello_judged` and `hello_judge_differs`. It gives the next tuning
  round its data and never acts.
- **A hard rule holding** is `wait` by the rule as without the switch, and
  no model is asked, in shadow or otherwise.

A decision line by the rule (the context and state shortened):

    {"at_unix_ms":1000000,"about":"hello","shadow":false,"context":{...,"skill_ask":true},"state":{...},"asked":"jev","answers":{},"judgement":{"probability":0.6,"choice":"say_now","from_rule":true},"by":"rule","took_ms":0,"judge":"asked_in_shadow"}

and its journal lines:

    lamp-policy: hello decision 1 arrival=1 choice=say_now probability=0.600 by=rule took_ms=0 guard=none
    lamp-policy: hello judge in shadow arrival=1 decided=say_now judge=wait probability=0.020 took_ms=640

Tests: `hello::tests::a_rule_pass_says_hello_while_the_judge_says_wait`
(a pass is `say_now` on the first turn while the judge has not answered;
its answer, `wait`, is written as `hello_judge` with `differs` and
counted, and nothing more is decided),
`hello::tests::by_rule_the_persons_hour_still_holds_a_second_hello` (the
twenty minutes and the person's hour hold by the rule, with no judge asked),
and the option tests in `main.rs` (`--hello-by-rule` read, refused without
`--hello`).

<a id="the-quiet-counts-talk-to-the-lamp-1155"></a>
**The quiet counts talk to the lamp (#1155, `--hello-quiet to-the-lamp`).**
In an office the voice hears talk every few seconds that is not for the
lamp, so 15 s of quiet seldom come, and the hello is waited by the rule
nearly always. With this switch the quiet counts only turns said to the
lamp or by it:

- **What a turn is.** It begins when the voice starts listening after it
  was resting or speaking, and holds what is heard in it, its thinking, and
  the voice's resting after it. Its number is the newest request heard in
  it (`voice/heard`'s `request`): talk that goes on, the voice listening
  again before it rests, is judged by its newest request. The voice's state
  and its words come on two topics, so words of a new request heard before
  the voice is seen listening begin their turn, and the listening seen
  after them is the same turn.
- **The verdict.** The voice says each turn's verdict on `voice/turn`
  (`lamp_msgs/VoiceTurn`, [msgs.md](msgs.md)), as its journal says it:
  `answered`, or `held_back` when the answer was held as not said to the
  lamp. On the Live route that is the rule that judges each answer
  ([attention.md](attention.md)); on the second route the same rule, or a
  short sound with nothing to show it was said to the lamp. On both, the
  model's own `not_for_me` is `held_back` too (below). A turn nothing
  judged, or with nothing heard, is not told, and counts.
- **Held back.** The turn's talk is taken back from the quiet: it counts
  again from the talk before the turn began, and nothing more heard,
  listened to or thought about in that turn counts. The verdict may come
  while the voice thinks or once it rests, and before its words: the second
  route says it before the words it judged go out, so a verdict for a
  request not heard yet is kept for its words, and taken up when they come
  (the judged turn's words, not a new turn). One for an earlier request
  than the turn's is not this turn's, and a kept one is let go when words
  of a later request come instead. A new request heard in a turn held back,
  the voice listening again before it rests, counts until it is judged.
  Once the lamp has spoken in a turn, as when it asks again what was
  unclear, the turn is talk by it: it counts until the lamp stops, and no
  verdict takes it back.
- **What stays.** Never while the voice listens, thinks or speaks: a turn
  is counted from its first word until its verdict comes, so a hello never
  comes over someone talking. Only the hello's own hold changes: the model
  is still shown the room's talk (`room`, "busy: people are talking", and
  the seconds since anything was said, all talk counting), for it to judge
  a hello at the lull, and the moment question beside it (`--gate`) counts
  all talk as before.
- **A turn the model itself declines** as not for the lamp (`not_for_me`,
  called, or written as words on the second route) is told `held_back` at
  the decline, after an `answered` if the rule let it out first, and is
  taken back the same way (the first builder on #1817). One the lamp acted
  on first is the lamp's: a decline after an action ran (the Live route), or
  after any tool ran (the second route), is not told.

The policy reads `voice/turn` only with the switch on. Tested with a replay
of a busy office at the rig's scale (`tests/it/hello.rs`,
`in_a_busy_office_a_hello_comes_at_a_lull_and_never_over_talk`): a quiet of
400 ms, the room talking about every 350 ms; someone who comes back while
it talks is waited by the rule, and one who comes back at the lull, 300 ms
after the last turn's first word, is asked about and greeted. Without the
switch the same lull is still inside the quiet, and the rule waits.

A yes that comes back once someone has begun to talk, or once a rule was
kept, is turned to `wait`; the room it is judged by then is the room as it
is when the answer comes, as the moment and progress questions judge it.
Asking never takes longer than 1.5 s, whatever `--deadline-ms` says
(`hello::MOST_ASKING`): past it the rule answers, still inside the voice's
2 s. The 2 s count from when the arrival was received: one that waited
behind another is given only what is left of them, less 0.2 s for its
answer to go out (`hello::time_left`, `ANSWER_MARGIN`), and the rule answers
at once when nothing is left. An arrival not decided within those 2 s,
waiting or asked about, is let go (`hello_late`): an answer that comes after
them is never published and changes nothing kept (the reviewer on #958).

**What is known, and shown.** The arrival (minutes away, seconds there,
minutes since the last hello), and the room as the progress question has it
plus facing: someone near (the eyes' list, no older than 2 s), whether such
a face is turned to the lamp (null without a near face), the seconds since
anything was said (the voice's count), the room, the turns the lamp began
itself in the hour (with the hellos chosen in shadow, as for progress), and
the hour, by `--utc-offset-minutes` or `--zone` as for progress. Made-up
numbers:

    {"lamp":"A small desk lamp robot on the desk of the person it works for, like a friend at the next desk: when someone comes back and sits down it may greet them with a few warm words, and otherwise it keeps quiet.","arrival":{"minutes_away":42,"seconds_there":3,"minutes_since_the_lamp_last_said_hello":95},"room":{"someone_at_the_lamp":true,"facing_the_lamp":true,"seconds_since_anything_was_said":310,"room":"quiet: nobody else has been talking","times_the_lamp_spoke_up_unasked_in_the_last_hour":0,"hour_of_day":9}}

**What is asked.** Two questions, each beginning "state.arrival says
someone has just come back to the lamp's desk and sat down in front of it;
state.room is how things are around the lamp now.":

| Name | The question |
|---|---|
| `welcome` | Would a friend at the next desk greet them now with a few warm words, given how long they were away, whether they are turned to the lamp, and the hour? |
| `intrusive` | Would a hello now more likely interrupt or annoy them than please them: the room is busy with other people's talk, it is late at night or early in the morning, they were away only a short while, or the lamp has spoken up unasked often in the last hour? |

`probability = welcome × (1 − intrusive)`, `say_now` from 0.5. Authored,
not fitted. With no model's answer, the rule: a hello unless the room is
known to be busy, the lamp began two turns or more itself in the hour, or it
is before eight or after ten (when the hour is known); 0.6 or 0.1,
unmeasured.

**Counting "spoke up unasked" in the hour.** Acting, the hellos are said
and `voice/state` counts each as a turn the lamp began itself. In shadow,
the hellos chosen are counted as notes. But the hello may still be said
for the same arrival (the voice's own rule's, `LAMP_SPEAK_FIRST=on`, until
#2825 removed it), and that turn is counted too. So a turn the lamp begins itself (its first `thinking` or
`speaking` with nobody listened to lately) within 5 s after a hello chosen
in shadow is taken for that same greeting (`ONE_GREETING_WITHIN`; the
reviewer on #883), and followed to its end, however late its first sound
comes: the coordinator says `thinking` while it waits for the model, which
may take up to 30 s. Spoken, it is counted once, as a turn the lamp began,
however often it speaks before it ends (one owner may think and speak
again: the reviewer on #883, 04:55);
ended without a sound (cancelled), the hello chosen is counted once, as
chosen. A greeting said before the decision came needs nothing: the lamp
was talking, so the hard rule answered `wait` and no note was made. Whether
a turn was asked for is known at its beginning and kept to its end: only a
turn the lamp began itself is counted, at its first sound, and an answer
whose first sound is slow is never counted (the reviewer on #883, 05:22).
Tests: `one_greeting_is_counted_once_in_shadow_or_acting`,
`a_greeting_whose_first_sound_comes_late_is_still_counted_once` (first
sound 6 s and 30 s after the turn began, and a turn cancelled before it),
`a_turn_that_speaks_twice_is_one_turn_and_two_turns_are_two` and
`a_slow_answer_is_never_counted_as_speaking_up_unasked` (two answers with
their first sound 1 s and 30 s after the person was listened to: none
counted, and the rule says hello; two turns the lamp began itself: two).

**Asked by a skill: the gate (#939, `--gate`).** The Policy is the one
gate for everything the lamp starts itself (`docs/skills.md` on
`lamp-voice`): a skill asks on `skills/ask` (`lamp_msgs::SkillAsk`, an
event: the ask's number, the skill's name, the act, the person if known,
and for a hello its minutes away and seconds there), and is answered by a
`Decision` on `policy/decisions` that names the ask (`ask`), within 2 s
(as long as the voice's own hello waited, before #2825 removed it). With `--gate`, which needs `--hello` and
`--act hello`:
- `act` `hello` is put to the hello question as an arrival of that number
  is: the same hard rules (someone talking, a hello within 20 minutes),
  and one of its own, a hello to the same person within the hour, by the
  ask's `person` ([above](#hello-on-arrival-say-hello-now-752)), the same
  question and the same log. The quiet it is shown is the
  policy's own (since anything was last said, or since it started), and
  the last hello its own. The decision names the ask, not an arrival:
  whether it came from a skill is carried with it from waiting to decided
  (`Context::skill_ask`, written in the log only when true), so no other
  ask can take its name (the reviewer on #958, 12:19). Its 2 s count from
  its receipt, as an arrival's do.
  `--act hello` is needed because a skill told `say_now` says the hello
  itself: the decision must act, so that the 20 minutes are counted from
  it and the hello is counted once, as the lamp's own turn on
  `voice/state`, never also as a note chosen in shadow.
- `news` and `unprompted` are put to **the moment question**
  (`crates/policy/src/moment.rs`), "is this a good moment for the lamp to
  start this itself?", on a connection of its own:
  - one hard rule first, never while anyone is talking to the lamp (the
    voice listening, thinking or speaking, or anything heard or said in the
    last 15 s, as the hello question knows it): `wait` by the rule, no model
    asked; and again as an answer comes back, by how things are then;
  - then one question to the model, `good_moment` (the progress question's,
    for anything the lamp would start: someone at the lamp, nobody spoken
    for a little while, the room not busy, not spoken up unasked often in
    the hour, not late at night or early in the morning), shown the skill's
    name, what the lamp would do (news of work, or something small of its
    own) and the room: someone at the lamp, seconds since anything was
    said, the room, the times the lamp spoke up unasked in the hour (its own
    turns on `voice/state`, as the hello question counts them), the hour;
  - `say_now` from 0.5; with no answer within 1.5 s, the rule: yes unless
    nobody is there, the room is busy, the lamp spoke up unasked twice in
    the hour, or it is before eight or after ten;
  - published with `about` `moment`, `shadow` false and the `ask`, and
    written down with its state, as every decision is. An ask's 2 s count
    from its receipt: one taken up after another is given only what is
    left of them, less 0.2 s, and the rule answers at once when nothing is
    left. An ask not decided within its 2 s, waiting or asked about, is let
    go and counted `late`: an answer after them is never published (the
    reviewer on #958, 12:19).
  
  With `--rules` (#813) a second hard rule holds as the first does, before
  the model and again as an answer comes back: a rule the people at the
  lamp kept ([above](#a-persons-rules-about-speaking-first-813)), guard
  `kept_rule`. `news` is held by `quiet`, `quiet_mornings` and the `news`
  limits; anything else unprompted, which has no limits of its own, by
  those and the `hello` limits too (`rules::Kind::Unprompted`): someone
  who asked for no hello before nine, or no news while on a call, is not to
  be invited to stretch then either. Authored: a rule errs toward quiet.
  A `reminder` is held by none of them, only by someone talking (below; the
  reviewer on #1003): a rule about the lamp speaking first is not about
  what the person asked it to say. The rules are handed to the moment
  question only with `--rules`. As an answer comes back the hello question
  now judges by the room as it is then, as the moment and progress
  questions do, and a yes held then is counted with the holds before the
  model (`hello_held_talking`, `hello_held_recent`, `hello_held_rule`).
  Tests: `moment::tests::a_kept_rule_never_holds_a_reminder_and_still_holds_the_rest`,
  `moment::tests::a_rule_kept_while_the_question_is_out_holds_it_when_the_asker_goes`.
- `reminder`, a reminder the person set and now due (the Reminders skill,
  `[ask] policy = "reminder"`), is the moment question's **hard rule
  alone**: `wait` while anyone is talking to the lamp, else `say_now` by
  the rule at once, with no model asked and nothing recorded as failed. The
  hour, the room, whether anyone is at the lamp and how often it spoke up
  do not count, as the person asked for it; neither do the kept rules about
  speaking first. It is decided at once, even while a question is out or
  other asks wait before it, so it is never let go late behind them (the
  reviewer on #1099). Tests: `moment::tests::a_reminder_is_said_by_the_rule_alone_and_held_only_while_someone_talks`,
  `moment::tests::a_reminder_is_decided_at_once_behind_a_question_out_and_another_ask`.
- An ask that fails its check (a skill's name, a known act, a hello
  without its minutes and seconds) is passed over.
- **An add-on's say** (`--addons`, #2530 part 2): the add-ons' judge
  (`lamp-addons`) puts a say that passed its own rules to the policy as an
  `AddonAsk` on `addon/ask`: the proposal's number, the add-on's name, its
  `why` and how `sure` it is, never its words. It is put to the moment
  question as something `unprompted`, with the same hard rule, kept rules,
  question and fallback, shown the add-on's name and "say one short line an
  add-on on the lamp suggests, about: `why`". Its decision has `about`
  `addon`, names the `proposal` (no `ask`) and is `shadow` unless
  `--act addon`. The judge acts only on `say_now` that is not shadow.
  Counted as `addon_asks` in the diagnostics, with `--addons` only, and
  the start line says `addons=true`. Like `skills/ask`, any publisher's
  message is read; one not from the judge names no proposal it waits for,
  so its answer acts on nothing. Tests: `moment::tests::an_add_ons_say_is_decided_about_addon_naming_its_proposal`,
  `add_ons_are_answered_with_the_gate_and_act_when_asked`.

The diagnostics add `skill_asks` (asks taken), and `moment_decided`,
`moment_said`, `moment_held_talking` and `moment_late`, with `--gate` only
(and `moment_held_rule` with `--rules`), and the start line `gate=true`. Without `--gate` the topic is not listened to.
One hello only: the voice's own was removed (#2825), so the skill's
through the gate is the lamp's one. Test: `tests/it/hello.rs`,
`with_the_gate_a_skills_ask_is_decided_by_its_question_and_names_the_ask`
(a hello asked with 42 minutes away and 3 seconds there, `say_now` acting
with `ask` and no `arrival`; the next held by `greeted_recently` with no
model asked; news shown to the moment question with the skill and someone
at the lamp, a yes of 0.8 `say_now` acting with the ask; something
unprompted while the voice listens `wait` by the rule with no model asked;
`skill_asks` 4, `moment_decided` 2, `moment_said` 1, `moment_held_talking`
1), `crates/policy/src/moment.rs` (the state to the byte, the answer, the
rule's four reasons to keep quiet and that not known is not no), and
without the switch nothing listens to `skills/ask`.

**Kept rules, tested** (#813, `--rules`): `crates/policy/tests/it/hello.rs`,
`with_rules_a_kept_rule_holds_the_hello_and_the_skills_asks_and_no_model_is_asked`
(on a bus: with `quiet` kept, an arrival is `wait` by the rule with guard
`kept_rule` and no model asked, and a skill's `news` too; the rule
forgotten, the next arrival is asked about and said; `hello_held_rule` 1,
`moment_held_rule` 1); the gate test above (without `--rules` nothing
listens to `memory/rules` and the report has neither count);
`hello.rs`'s `with_rules_a_kept_rule_holds_the_hello_and_no_model_is_asked`
(also a yes coming back after a rule was kept is a `wait`, and without the
switch a rule holds nothing); `moment.rs`'s
`a_kept_rule_holds_news_by_its_own_limits_and_anything_else_by_either`;
`rules.rs`'s `anything_else_unprompted_is_held_by_the_limits_of_either_kind`.

**What is published and written.** On `policy/decisions`: `about` `hello`,
`say_now` or `wait`, the probability, the two answers, `by`, `took_ms`,
`shadow` (`false` only with `--act hello`) and `arrival`, the number
`voice/arrival` gave. The new optional `arrival` field is left out of every
other decision, whose bytes are as they were (`crates/msgs/tests/it/arrival.rs`).
The log gets a line in `decisions-YYYY-MM-DD.jsonl` with the context, the
state shown, the answers, the judgement and any `guard`, `failed` or
`usage`; the journal one line with no words:

    lamp-policy: hello decision 2 arrival=98563000000 choice=say_now probability=0.720 by=model took_ms=612 guard=none

The diagnostics add `hello_decided`, `hello_by_rule`, `hello_said`,
`hello_held_talking`, `hello_held_recent`, `hello_held_person` (the hour
to the same person), `hello_late`, `hello_held_rule` with `--rules`,
`hello_judged` and `hello_judge_differs` with `--hello-by-rule`, and `hello_acts` with `--act hello`; the start line `hello=true`, `hello_by_rule=true` with the switch, and `act=` names each
question that acts (`act=progress,hello`). A yes held as it comes back is
counted too, under its kind, with or without `--rules`: someone talking
by then counts in `hello_held_talking`, which the code before #1003 did
not count. The decisions are as they were; only that count is new.

**Without the switch** `voice/arrival` is not subscribed to, no judge or
thread is made, and the start line, decisions and diagnostics are as they
were (`without_the_switch_arrivals_are_not_listened_to_and_the_report_is_as_before`,
`crates/policy/tests/it/hello.rs`, which also covers shadow, acting and the
twenty minutes, talking, and a failed model).

**How long it took** the voice's own hello, before #2825 removed it
([verification](verification/hello-asks-host-20261015/README.md)): on the
development Mac, with a stand-in judge that answers at once, a held hello
had its answer 22 to 38 ms after it was said on the bus; on the lamp Jev's
own time (0.5 to 1 s, at most 1.5 s) comes on top.

**Most likely wrong.** That Jev answers a question it was not built for
the way a person would. That the hour, the room and facing are what a
person weighs before saying hello. That another turn the lamp begins
itself within 5 s of a hello chosen in shadow (news, say) is taken for that
greeting and counted once, not twice; and that the voice's own greeting
begins its turn within those 5 s (it holds the hello up to 2 s for the
decision, then begins).

## Not shown

The historical measurements below belong to First's original Jev-only
implementation. They are not measurements of task 17 or of Fourth's host.
First has since run the shadow judge on lamp-4ace and supplied script-labelled
played-scene records, as the fit audit above describes. Fourth reproduced
the fit on a Mac; Fourth did not run on a lamp or use a real provider.
Those selected rows do not establish performance on live human conversation,
an exact paired comparison against Voice's first-output gate, or a model
safe to wire to output. The entry point remains opt-in and shadow only;
leaving the switch unset changes no existing lamp behavior, deployment or
actuator boundary.

Host tests on one development Mac (x86_64); the same tests as a Linux
ARM64 build under emulation in a container, each time a build went to the
lamp; and the runs on a lamp above. The first of those runs is the first
in which this program asked the real service, from the lamp and from the
Mac alike (three decisions there the same night). What it shows of the
service: that it takes this program's request and that the built-in roots
trust its certificate.

What was run: 81 tests of the crate. As 60, they passed 30 times in a
row, and then in the three runs with `lamp-msgs` and `lamp-bus` that
every change here is held to. Before that, one test of the log failed once
in fifteen runs: it cleared the log's way before the program had finished
with the decision before, which was then written after all. The test now
waits. The program was not changed for it. As 78 and then 79, eleven
times in a row on the Mac while it was loaded, after one bound of a test
that held the loop to a time a loaded machine misses was widened.

- 29 of asking, against a server on 127.0.0.1 that plays a script: eight
  of them of the kept connection (kept by length and in chunks, not kept
  after an unclean end or past its time, replaced when the server had
  closed it, made ahead of a question). Four are over a secure
  connection, with a certificate made for the test and trusted in place
  of the built-in roots.
- 11 of the decision, 8 of putting utterances together, 5 of the log, 5 of
  the outcome, 2 of the options.
- 18 of the loop on a real bus in a directory of its own, with a stand-in
  for the model and the test as the voice.
- 3 of the built program, started, fed and killed.

And the 13 tests of `lamp-msgs`, one of them for the new type.

Not exercised:

- **The weights against real answers.** The four measured examples are
  checked as arithmetic. No labelled example has been put through this
  program and the real model, so the 87% is the measurement's and not this
  program's.
- **Whether this context is the measurement's.** When the lamp's side is
  taken (the first piece), what "last spoke" is measured from, and the
  seconds to a tenth are this program's choices. How the 346 examples had
  them is not known here. If they differ, the weights fit less well than
  measured.
- **The pause of 300 ms on a lamp.** It was given, not tuned. How far
  apart the voice's pieces come inside one utterance on a lamp has not
  been looked at: one utterance may be judged as two, or two as one.
- **A piece marked `finished`.** No publisher sends one today. It is taken
  as the last piece of its utterance, added to the others, and not as the
  whole of it again.
- **That an answer's text comes before its sound has ended.** It is the
  reason for the two moments "last spoke" is taken from. It was not
  measured here.
- **A kept connection that dies without a word.** One the other end
  closed is found out at once and replaced. One that a network box
  between has silently dropped would take the request and never answer:
  that asking would time out, and the next would make a new connection.
  It was not played and has not been seen.
- **How long the service really keeps a connection.** The figures are
  from one night, one connection each, from one network.
- **The system's own lookup of a name.** The tests connect by address, or
  with a lookup of their own in its place. An IPv6 address is read from an
  endpoint and has never been connected to.
- **A bus that cannot be used at all.** A topic that cannot be published
  was played with a file in its way. A root that cannot be subscribed to
  ends the program at the start, and that was not played.
- **A disk that fills.** The log failing was played with a file in the
  way, not a full disk.
- **SIGTERM.** The built program was killed with SIGKILL.
- **Hours of running.** The longest run was a few seconds. The lease of
  3 s and the once-a-second sign of life were not watched for longer.
- **A unit in the repository.** `lamp-policy.service` runs on lamp-4ace
  and is kept with that lamp's other deploy files, outside the
  repository. Nothing in `deploy/` installs the policy yet.
