# Whom the lamp answers

Scope: `crates/voice/src/attention.rs` and its use in the provider worker
(`crates/voice/src/provider_worker.rs`). Part of step 7 of
[brain-build.md](brain-build.md), brought forward on what the lamp did.

## The rule

The model decides what to say. For the first things a new session hears,
whether an answer is played is decided on the lamp:

| When | The lamp plays an answer |
|---|---|
| The request is one of a session's first two, and the room is busy (three things in a row it said nothing to, inside a minute; [character.md](character.md)) | only if it said something to someone in the last 45 seconds, or began the turn itself (a hello, a look), or the text of what it heard shows it was called |
| Any other time: a session that has heard more, a quiet room, a room not yet known | always: the model's own judgement stands, as before |

This is the second form of the rule. The first held back every answer in
a busy room unless the name was in the text, and failed on the lamp for
the reason given below. This form keeps the hold to the few requests
where the model is known to be unreliable, and reads the text only as
evidence for playing, never for holding.

## Why it is the lamp's to apply

On lamp-4ace on 2026-10-13 a played test cut the link to the provider
twelve times and each time let the new session hear two people talking to
each other. Three runs on the evening's builds:

| The session was | New sessions | Spoke words to the two people |
|---|---|---|
| told nothing about the room | 11 | 4 |
| told how the room was | 12 | 3 |
| told how the room was, and given a function to decline with ([tools.md](tools.md)) | 12 | 2 |

"How was your weekend?" was answered in all three. Nothing the session is
told or given has taken this away, and a difference of one or two in
twelve is not one that can be told from chance.

What makes a rule on the lamp possible is timing (what makes this one
unusable is further down). The service writes down
what it heard as well as what it says. In those three runs the text of
what was heard had arrived before the first sound of every one of the 24
answers, by 0.32 s at the least and 0.44 s in the middle. When an answer's
first sound comes the lamp can already read whether it was called, so
holding an answer back costs no time.

## What happens to an answer that is held back

It is dropped in the provider worker, as one the model declined itself is:
none of its sound reaches the coordinator or the speaker, and its words
are not kept as something the lamp said. The request ends when the service
completes it. The service still believes it answered; whether that
changes what it does next has not been looked at.

A call for something to be done in a held request is answered and not run
(row 6 below), with one exception: a call that withdraws the person's
consent is done though its line is held (the first builder on #1722,
16:44; #1683). The list is `DONE_THOUGH_HELD` in `attention.rs`, and today
it holds only `reports_stop`. Such a call only reduces what leaves the lamp,
never adds to it. Done for a colleague's line, it costs all that a stop
does ([cloud.md](cloud.md), "The person's choice"):

- every report waiting is deleted;
- one `stopped` is sent;
- sharing stays off until the person turns it back on in the app;
- since the answer is held, nobody is told at the time: before #1880's
  follow-up only the journal knew.

If it were not done, a person who asks to stop sharing and is held would
have to ask twice. The first builder ruled at 16:44 on the cost of one
report not sent. Asked again at the real cost (Third's read of #1880,
18:39), the first builder kept it at 18:44 and made it visible: the stop is
kept by `lamp-cloud` as the voice's, held, and when; the app's reports row
shows it; and the voice says it only when asked about reports
(`reports_state`, [tools.md](tools.md)), never unasked. Turning reports
back on is the app's ([cloud.md](cloud.md), "The person's choice").

The answer stays held: the model is told the call's result, and none of
what it says after it is played. The room is told nothing of it, as of any
held line: the call does not count as the lamp answering. The journal says
`request N: reports_stop done though the answer was held (consent
withdrawn)`. Every other call, the light, the volume, the schedule, memory,
skills, stays held as before. A call that adds to what leaves the lamp is
never put on the list.

On the second route, with `LAMP_REPORTS=on`, a held line is asked one
round for a `reports_stop` (`attention::DONE_THOUGH_HELD`,
[second-voice.md](second-voice.md)) only when its heard words name
reports, sharing or sending: one of `report`, `reports`, `sharing`,
`sending` or `data` as a whole word, in any case
(`attention::HELD_ASKED_WORDS`, #1925). That is checked on the heard words
before anything leaves the lamp, and the model decides on the line asked.
Every other held line is sent nowhere, as without the switch: a held line
is people in the room talking to each other (37 in 44 minutes on the first
arm of 19 October), and asking each one would send it all out and pay a
chat round for each. The journal says `a held line names reports: asked
once`, with no words.

## Where it gives way

The rule fails open. The answer is played when no text of what was heard
has arrived, when the room is not known to be busy, and in any turn the
lamp began itself. The name is the word "Lamp", or "Lamb", which is how
the service writes it when it mishears; nothing else is taken for it.

## What it does not know yet

Whether the person is looking at the lamp. Someone who walks up to it in a
busy office and speaks without its name is held back, until the room has
been quiet for five minutes or they say its name once. The eyes' report of
who faces the lamp ([eyes.md](eyes.md), `facing`) is to be added.

## Played on the lamp, and why it is off

On lamp-4ace on 2026-10-13, with the rule on:

| Played | Result |
|---|---|
| Twelve new sessions, each hearing two people talk to each other | 0 of 11 judged sessions answered them. The rule held back the 4 answers the model began |
| Seven questions with its name, straight after, a few seconds apart | 7 of 7 answered |
| Room test: four questions without its name in a quiet room | 4 of 4 answered |
| Room test: six lines of two people talking | Nothing said |
| Room test, room busy: one question without its name | Left alone |
| Room test, room busy: two questions with its name ("Hey Lamp, ...", "Lamp, ...") | **0 of 2 answered. Both were held back** |

The last row is the rule failing at the thing it exists for. The service's
text of what it heard does not contain the name. For "Hey Lamp, what is
the capital of Italy?" it wrote "Milan, what is the capital of Italy?";
for "Lamp, what colour is the sky on a clear day?" it wrote "What color is
the sky on a clear day?". In 310 lines of what the lamp heard that day,
many of them played with the name, the word "Lamp" was written not once:
"Lam" and "Lan" three times each, and otherwise "Milan", "Man", "plan" or
nothing. The seven answered above were answered because each came within
45 seconds of the last, or before any text had arrived, not because the
name was read.

So the text of what was heard cannot say whether the lamp was called, and
the rule was switched off the same evening (21:26). What the runs do
show: the timing holds (the text arrives before the first sound), and
holding an answer back at the worker works. What is missing is a sign of
being called that does not come from that text. Candidates, none tried:
the model saying so itself through a silent function, the lamp spotting
its own name in the sound it captured, and the eyes' report that someone
is facing it.

## The second form

The rule as it now is (top of this page) differs in two ways. It applies
only to a session's first two requests, because that is where the answers
to other people's talk were: 9 of 35 new sessions within their first two
requests, against 2 of 87 things heard on a session that had been running
that afternoon. And the text is read only as evidence that the lamp was
called: its name, or one of the words the service has written in its
place at the start of a called request in the lamp's own records
("Milan,", "Man,", "Lam", "Lan", "plan"). A text with none of them proves
nothing, so outside those two requests nothing is held.

What it still costs: someone who calls the lamp in a busy room within its
first two requests after a new session, in words the text gives no sign
of, is not answered until they say it again. A session is renewed at most
every fifteen minutes in a busy room.

Since 2026-10-14 there is a second thing that can show the lamp was
called, and it does not depend on the text: the lamp's name heard in the
room's sound, by the part that listens for nothing else
([name.md](name.md)). The provider worker takes its hearings from
`name/heard`. A hearing counts for a request if the name began no more
than 1.5 s before the request did, or at any time until the request's
sound ended: the name is the first thing said, a little before the voice
is sure someone is speaking, or it is said in the middle or last. A name
that begins to be heard after the person has finished was not said in
what they said, and is not counted for it (it was, until the review of
2026-10-14 showed a line to "Liam" a second later letting out an answer to
other people's talk). Once a hearing has counted for a request, a later
one does not take that away. Like the text, it can speak for playing an answer
and never against: a held request is played, and the log says
`its answer is played: the lamp's name was heard in the room's sound`.
With no such part running, or no bus, the rule goes by the text alone.

**Every verdict is in the journal** (the first builder, #829, 17:21: on
lamp-4ace an answer to colleagues' Vietnamese at the desk played with no
verdict line at all). Each answer let out says why, as each held one does:
`its answer is played: <why>` from the Live worker, and the same from the
second route. Output that comes for a request other than the one in hand,
once another has begun, is never judged, and the journal says so once:
`request N: its output came with request M in hand, and is not judged`.

That narrows the cost above to the calls that part misses, which were
three in twenty on its first measurement, and adds one: a line that begins
with a name like the lamp's ("Hey Lan") now passes this rule in a busy
room on a new session, and is left to the model's own judgement as
everything was before the rule.

The cost it narrows had not been measured, and it was most of the calls.
The earlier count of new sessions called by name (14 of 16 below) was
made by starting the voice afresh, and a voice just started does not yet
know that its room is busy, so the rule never applied. On 2026-10-14 the
case itself was played (`namecall.sh`, nobody in the office): eleven
lines of two people talking to each other, so that the room is busy and
45 s have passed since the lamp spoke; the link to the service cut, which
gives a new session in the same process; then one question by name, in
one of three synthetic voices. Twelve rounds:

| | Rounds of 12 |
|---|---|
| The lamp said nothing through the two people's talk | 12 |
| The name was heard in the sound | 11 |
| The answer was let out because the name was heard, the text having shown nothing in time ("What is 2 + 2?") | 9 |
| The answer was let out by the text ("Milan, what comes after Tuesday?") | 2 |
| The answer was held back: the name was not heard and the text began "Helen," | 1 |
| The lamp was heard to answer | 9 |

So as the rule stood the day before, ten of these twelve answers would
have been held back from someone who had called the lamp by its name.
With the name heard in the sound it was one, and "Helen" is now among the
words the text may show a call by (with "Mom", "Amen" and "Weiland", each
written for the lamp's name at least once in its records).

**On the second route only** (#1973, the first builder at 22:38), more
words the service writes for the name ("Lynn", "Len", "Lin", "Lump",
"Land", "Line", "Palin", "Palan", "Louvre", "Lampy", `attention::STAND_ALONE_INS`) are never
taken for someone else's name, and call the lamp only standing alone, set
off as a name is. Its worker's rule counts them
(`Attention::count_stand_alone_ins`, `called_by_name_on`). Deepgram writes
"Lamp" so; Gemini's transcript doesn't, and Linh and Lin are common names.
So on Live they are people's names as before, and "Lin, pass me that."
between people stays held (`the_names_stand_ins_call_the_lamp_on_the_second_route_only`).
On the second route: "Hey, Lynn. How are you today?", "Lynn, what time is
it?", "Thank you, Lynn." are its calls, "Line up the meetings" is not
(`called_by_name`). On lamp-4ace on 19 Oct, on the second route, "Hey Lamp,
how are you today?" and "Lamp, what time is it?" came back with "Lynn" and
were held as said to someone it calls by name (#1712). The voice keeps no
list of the known faces' names yet, so a known person called Lynn is not
told from the lamp.

"Louvre" joined them on 25 Oct. It is the word written for "Lamp," most
often: 27 of the played lines on lamp-4ace from 22 to 25 October, each said
as "Lamp" or "Hey Lamp". 15 of those were held, 11 as said to someone it
calls by name ("Louvre, what did I just ask you?" five times of five).
lamp-name heard none of those five, so the near rule
([second-voice.md](second-voice.md), a first word like the name heard near)
could not take it. "We went to the
Louvre in May" is no call.

"Lampy" joined them on 26 Oct: on build 6999 on lamp-4ace (14:1x, a played
line in a loud office), "Lamp, what did I just ask you?" came back as "Lampy,
…" and was held as said to someone it calls by name.

Of the three rounds with no answer heard, the rule held one. In one the
service sent the words of an answer ("Four. Easy one.") and no sound, the
only time in the 800 and more answers in the lamp's records. In one it
wrote the question as "Milan, 1 vez que eu passo tu" and the model said
nothing. Where the lamp answered, its first sound came 0.98 to 1.24 s
after the end of speech was decided.

Not shown: a person, and a real office. The talk was two synthetic voices
from one loudspeaker, and the name part hears a synthetic "Hey Lamp"
better than it may hear a colleague's.

### A second look when the lamp heard its name and the model did not

The rule above only lets an answer out. It cannot make one. In the check
of two people talking ([character.md](character.md)), on 2026-10-14, the
closing "Hey Lamp, what do you think we should have for lunch?" got
nothing in two runs of two: the service wrote it as "Hey, what do you
think we should have for lunch?", it followed a conversation about lunch,
and the model took it for more of that. The part that listens for the
name had heard it both times, with a score of 1.000.

So the lamp now tells the model what its own ears heard
(`crates/voice/src/second_look.rs`, switched on with
`LAMP_SECOND_LOOK=on` in the voice unit, with the bus on). When a request
that was heard ends with nothing said, and the name began to be heard in
the sound no more than 1.5 s before that request began or at any time
until its sound ended (with a score of 0.98 or more at first; any hearing
the name part reports, since 07:45 the same day: see the end of the next
section), the voice begins one turn of its own
with a written note in place of speech, as it does for a hello:

> (Your own ears caught your name, Lamp, at the start of what was just
> said to you, so you were being called. If it is something a lamp on a
> desk could answer or chat about, answer it now in your usual way, as if
> you had not waited, and do not mention the wait or your name. If it is
> other people's own business, say nothing.)

What the model then says is spoken like any answer. The trace has
`second_look_due` with the score, and the turn's `input_admitted` has
`"admission_basis":"lamp_looks_again"`.

- **Once.** The turn the lamp begins is not a heard request, so whatever
  comes of it, nothing more is owed. Nor does it count as one of the two
  first requests the rule above covers on a new session: the session
  heard nothing in it. (It did count, until the review of 2026-10-14: a
  line to "Liam", its second look, and the next line of other people's
  talk was already the session's third request and no longer held.)
- **The plainest hearing counts.** Of the hearings in the 1.5 s before a
  request, and of those in it, the one with the highest score is the
  request's, not the newest.
- **Only what was said in the request.** A name that begins to be heard
  after the person has stopped is the next request's, if one begins
  within 1.5 s, and nobody's otherwise.
- **Only to the session that heard it.** The note speaks of "what was
  just said to you". If the link was renewed in between, the session
  that is there was never told, and nothing is asked: the journal has
  `the session that heard it has gone` and the trace `second_look_dropped`.
- **A wordless answer is nothing said.** An answer with no word in it
  ("...") ends the turn as `nothing_said`, and is owed a second look like
  a silent one; its `second_look_due` has `"after":"nothing_said"`.
- **Someone who speaks first is listened to.** A request that begins
  before the second look has drops it, and one that could not begin
  within three seconds of the silence is dropped too.
- **It is late.** That nothing was said is only known when the service's
  wait for an answer has run out, four seconds after the person stopped
  (`no_reply_after_ms`, lamp-4ace's provider file). The answer comes a
  second or so after that. On a product lamp the wait is the 8 s that
  `LAMP_PROVIDER_PATIENCE` fills in, so the second look comes about 9 s
  after the person stopped; before it, a silent request there ended at 30 s
  with its connection and the second look was dropped (the reviewer on
  #1394).
- **A name like the lamp's.** Lines to "Lan" or "Liam" set the name part
  off in five of ten at the score it reports from (0.9) and in two of ten
  at 0.999 ([name.md](name.md)); 0.98 is between, and what such a line
  gets is then the note's last sentence and the model's judgement.

Played on lamp-4ace on 2026-10-14 from 06:04, nobody in the office:

| What was played | What should happen | What happened |
|---|---|---|
| Two people talking for nine lines, then the call and a follow-up without the name (`twopeople.sh`), four rounds | Nothing said through the talk; the call and the follow-up answered | Nothing said through the talk, and the call answered, in all four. In two the model answered by itself, its first words about 2 s after the caller stopped. In two it said nothing, the second look was taken, and its first words came about 5.3 s after the caller stopped. The follow-up was answered in all four, about 1.5 s after |
| Ten lines to someone with a name like the lamp's ("Hey Lan, did you see this?", "Hey Liam, the meeting moved.", `near.sh`), ten seconds apart | Nothing said | Nothing said to any of the ten. The name part reported four hearings (1.000, 0.903, 0.998, 1.000) and a second look was taken three times; told its name had been heard, the model still said nothing to other people's business |

Not shown: a person, a real office, and a line with a name like the
lamp's that a lamp could answer ("Hey Lan, what time is it?"), which the
note would let through.

### The same sign, given while the person is still speaking

A second look is late: five seconds after the caller stopped. The same
sign can be given in time. When the name is heard plainly (0.98 or more)
no more than 1.5 s before a request's sound began to be sent, or at any
time while it is being sent, the provider worker sends the service one
written note inside that request, beside the sound
(`crates/voice/src/called.rs`; `try_note` in `crates/gemini`, which the
service receives as `realtimeInput.text` within the request's activity):

> (For you to know, not to answer: your own ears caught your name, Lamp,
> just now. If what is being said is something a lamp on a desk could
> answer or chat about, it is said to you. Other people's own business
> still is not yours.)

It opens as the lines about the room do, and the character file says of
such a line that nobody said it aloud and that it is never to be answered
or mentioned. It is offered once for a request, and only while that
request's sound is still going to the service: at any other moment it
would end the link, so the link's own wrapper refuses it then. The link
keeps it and writes it as the last thing before the end of the request's
sound (`commit` in `crates/gemini/src/session.rs`), so that it shares the
request's fate: a request that is given up, thrown out as a sound, or
taken over by a newer one takes its note with it. As first built the note
was written the moment it was offered, and one whose request was then
given up stayed with the service and was read with the next thing heard,
whoever said it and whenever: the review of 2026-10-14 showed that with a
failing test, and the note was off on the lamp from 08:15 to 08:51, when
the mend went on it.

A note in that place was new to the service, which had closed the link on
a note with no sound beside it ([speaking-first.md](speaking-first.md)).
So before it went back on it was tried without a sound, on lamp-4ace
through its relay at 08:50 (`lamp-voice provider-check CONFIG
--note-in-speech NOTE SOUND`: a recorded question sent as if heard, the
note offered half a second in, nothing captured or played). Three
questions in a synthetic voice, one session each:

| Sent as if heard | What the service wrote | Link after | Answer, and its first sound after the end was sent |
|---|---|---|---|
| "Hey Lamp, what do you think we should have for lunch?" | The same | Up | "Hmm, maybe something spicy like some good pho? What are you craving?", 1.06 s |
| "What is two plus two?" | "What is 2 + 2?" | Up | "Four.", 0.87 s |
| "How was your weekend?" | The same | Up | "It was so peaceful here with the office quiet. I just enjoyed the stillness. Did you do anything fun?", 1.41 s |

What that shows: the service takes a note written just before the end of
a request's sound, three times of three, and answers. Not shown: the same
through the lamp's microphones (nothing is played while people are in),
and a request given up on the lamp with a note in it, which the scripted
tests cover and nothing on the lamp has yet. It is
switched on with `LAMP_NAME_NOTE=on` in the voice unit, which the voice
hands to the worker as `--name-note` (the worker is started with an empty
environment; the first build read the switch there and never sent a
note). The log has `the model is told the lamp's name was heard`.

Played on lamp-4ace on 2026-10-14 from 07:28, nobody in the office:

| What was played | What happened |
|---|---|
| Two people talking for nine lines, then the call and a follow-up, four rounds | Nothing said through the talk. The call answered in all four, its first words 2.2 to 2.5 s after the caller stopped. The model was told in three (the name heard at 1.000, 0.996 and 1.000); in the fourth the name part reported nothing and the model answered by itself. No second look was needed |
| Ten lines to someone with a name like the lamp's, ten seconds apart | Nothing said to any of the ten. The name part reported five hearings (0.977, 0.991, 0.966, 0.946, 1.000); the model was told twice and said nothing both times |

**What this shows is that the note arrives and does no harm.** It does
not show that the note is why the calls were answered: in the four rounds
played just before, with the note not reaching the service through the
build's fault, the model answered all four by itself, and in the four
before those it left two unanswered. Whether it raises the rate needs
many more rounds than twelve.

One asking is enough. At first, when the model had been told inside a
request and still said nothing, the second look asked it again: twice
with the lines to a similar name, and nothing was said either time. Since
2026-10-14, 08:20, with the note switched on, the second look is taken
only for a request whose name came too late for the note: heard after the
person had stopped, as with a lone "Hey Lamp." that is over before the
name part has said so. (A note the link itself dropped, which it does
while an earlier answer is still owed, is not known of here, and such a
request now gets neither. Not seen yet.)

The rule has five tests of its own, and the note inside a request three
against a scripted service: it is written just before the end of the
request's sound and the link goes on; one whose request is given up is
never written; and one offered to a request that then ends is not there
for the next. The worker's wiring is tested for the rule (a name heard
as a request began, and one heard after its sound ended); for the note
it was exercised on the lamp.

**The score asked for was lowered the same morning, from 0.98 to the 0.9
the name part reports from.** A played "Hey Lamp, tell me two interesting
things about the ocean." was heard at 0.967; the service wrote "Hey mom,
tell me…"; no note was sent and no second look taken; and the lamp said
nothing. About a quarter of that morning's hearings of played calls
scored between 0.91 and 0.98 (five of the twenty in the logs read), while
lines to a name like the lamp's scored 1.000 as often as real calls did. So the higher bar kept the sign from real calls and
kept nothing out. What keeps the lamp out of a line to "Lan" is the note's
last sentence and the model's judgement, which held in each of the five
such lines it has been tried on. Not shown: the lower bar on the lamp;
people were arriving and nothing more was played.

Played on lamp-4ace on the evening of 2026-10-13, with two synthetic
voices and nobody in the office:

| What was played | What should happen | What happened |
|---|---|---|
| The link cut twelve times, each new session then hearing two people talk to each other | Nothing said | 11 sessions could be judged; none answered within its first two things heard. The model kept quiet by itself in most; the rule held back two answers. Before the rule: 4 of 11, 3 of 12 and 2 of 12 |
| Seven questions put to the lamp by name, on a running session | Each answered | 6 answered, first sound 1.10 to 1.52 s after the end of speech was decided. The seventh, "Good night, Lamp.", never reached the model: the local gate judged it not speech, as it did in 5 of the 8 earlier runs of this test ([not-speech.md](not-speech.md)). Not the rule's doing |
| Quiet room, four questions without its name | Each answered | 4 of 4 |
| Two people talking to each other, six sentences, on a running session | Nothing said | Nothing said |
| Busy office, a question without its name ("What is the capital of Italy?") | Left alone | **Answered.** The session had been open a while, so the rule did not apply, and the model took a plain question as its own |
| Busy office, two questions by name | Each answered | Both answered; nothing was held back |

So the second form does what the first could not: it stops a new session
answering other people's talk without withholding an answer from someone
who called. It does not make a running session leave alone a clear
question that nobody put to it: that needs a sign the lamp does not have
yet, of whom a sentence was said to. And one of the called answers shows
what the missing name costs beyond this rule: "Hey Lamp, what is the
capital of Italy?" was heard as beginning "Milan", and the lamp answered
about Milan.

Later the same night, with the memory's three functions also declared
and the same rule, three more things were counted:

- **New sessions and other people's talk, twice more:** 2 of 12 sessions
  and then 1 of 12 answered within their first two things heard. Each
  time one stray answer started it: the lamp answered a third thing on a
  session (a scrap heard as one word; a remark about the weather), which
  the rule does not cover, and because it had then "said something to
  someone in the last 45 seconds" the rule let the next new session's
  first answer through. An answer nobody asked for excuses the next one.
- **New sessions and a question put to the lamp by name:** 14 of 16
  answered their first one, and both that did not had heard "Hey Lamp" as
  "Milan" and said nothing. With "Milan" and "Weiland" added to the names
  the character file says its ears make of "Lamp": 7 of 8, two of them
  heard as "Milan" and answered; the eighth was not heard at all.
- **Short phrases with the name at the end:** of five "Good night, Lamp."
  that reached the model, three were heard as "Good night, love" or
  "Good night, man" and left unanswered.

All three point the same way as the missing name in the text: the lamp
needs to know it was called from something other than the provider's
text. Measured on 346 examples from these records (`docs/policy.md`), a
sign from the device that the lamp was called takes "is this said to the
lamp" from about three in four right to about nineteen in twenty. That
sign, and the face turned to the lamp that the eyes now publish, are what
the part that decides whom the lamp answers is being built to use.

One more thing was tried and is not used. The model was asked to say when
it heard its name, through a function of the silent kind
(`LAMP_NAME_SIGNAL`). It did hear it: on 6 of 7 called questions on a
running session, and on none of the talk between two people. But having
made that sign it said nothing else: 1 of those 7 questions was answered.
A function of the ordinary kind would let it answer, about 0.7 s later.
Neither is acceptable, and the switch is off. The second route offers the
same function with the switch, and there it is of the ordinary kind: its
call costs the answer a chat round ([second-voice.md](second-voice.md)).

## Hearing its name in its own sound: first measurement

Not built into the lamp. A measurement of whether it can be.

A study on 2026-10-14, kept outside the repository, ran three kinds of
detector for "Hey Lamp" and "Lamp ơi" over 14,344 generated utterances
and found none that catches 85 calls in 100 at 2 false alarms in 100. It
recommended a small classifier of our own on an open speech front end
(openWakeWord's mel and embedding models), and as a fallback a ready-made
open keyword spotter that had caught the most "Hey Lamp" on generated
files. Both were written in Rust on the library the lamp already uses for
its sentence-end model.

The same night both were run on what the lamp's own microphone heard.
Six finite sessions of the voice with its recording switched on
(`lamp-voice directed ... --diagnostics`, the only way the voice writes
sound down), 86 s each, nobody in the office, 44 lines played from a
loudspeaker in eleven synthetic voices: twenty "Hey Lamp" calls, ten
lines to a similar name ("Hey Lan", "Hey Liam", "Hey Lance", "Hey Mom"),
ten of office talk, and four in Vietnamese. Each line was cut from the
recording by the time it was played and given to each detector whole.
Fired on, of the lines of each kind, on the sound before the echo
canceller:

| Detector | "Hey Lamp" (20) | A similar name (10) | Office talk (10) | "Lamp ơi" (2) |
|---|---|---|---|---|
| Our classifier, score 0.9 or more | 17 | 5 | 0 | 2 |
| Our classifier, 0.999 or more | 14 | 2 | 0 | 1 |
| The same with similar names in its training, 0.9 | 14 | 3 | 1 | 1 |
| The same, 0.999 | 10 | 2 | 0 | 1 |
| The ready-made keyword spotter | 2 | 1 | 0 | 0 |

After the echo canceller the counts differ by one or two and not in one
direction.

What it shows, and it is small:

- **The ready-made spotter does not survive the lamp's microphone.** On
  generated files it caught 70 to 82 of 100 calls; here 2 of 20. Louder
  copies of an earlier set of fourteen recordings changed nothing there
  either. It is not a fallback.
- **Our classifier, which has heard nothing but generated speech and
  never this room or this microphone, catches 14 to 17 of 20** and was
  set off by none of the office talk at either threshold. What sets it
  off is a name like its own.
- **Training it on similar names did not help here.** At 0.999 it caught
  10 calls where the first caught 14, and was set off by the same two
  names. At 0.9 it caught 14 against 17, was set off by three names
  against five, and by one line of office talk. That version was trained
  on more generated speech of the same kind; it says the next gain is
  not there.

So "called by name" can be one sign, right about seven times in ten and
quiet through ordinary talk, and cannot be a gate: three calls in ten are
missed, and a colleague called Lan or Lâm would set it off.

Not shown: a person. Every voice here was synthetic and played through a
loudspeaker near the lamp, loud enough that one recording clipped. 44 lines is too few to tell 14 from 17. The Vietnamese lines
are four. Nothing ran on the lamp's processor: the detectors ran on a
Mac over the recordings. What is needed next is real voices through the
lamp's microphone, which the same recording procedure gives, and the
microphone's sound on the bus so that a part of its own can listen.

## A working morning in the office (lamp-4ace, 2026-10-14, 07:45 to 10:59)

The first hours with people at work around the lamp and nobody playing
anything to it. Read from the lamp's own records afterwards
(`morning.sh`): counts and the service's text, no sound. Nobody was asked
to speak to it and, as far as the records show, nobody did.

| | |
|---|---|
| Sounds that reached the voice as possible speech | 156 |
| Thrown out as sounds, not sent to the service | 71 |
| Taken as said and sent | 85, of which the service wrote no text for 72 |
| The lamp's name heard in one | 1, at 0.963, in half a second of sound with no text; the model was told and said nothing |
| Second looks taken | 0 |
| Answered | 1 |
| Times it stopped or paused for someone | 0 |

**The one answer.** At 10:04:33 someone near the lamp said one short,
loud word (0.31 s of speech) that the service wrote as a one-word "hey"
in another language, and 2.3 s later the lamp said "Hey, I'm here." (What
people around the lamp say is not copied into these pages, even one
word.) Nothing in the lamp's rule stood in
the way, and by the rule as decided it was not meant to: the room was
judged quiet at that moment, and in a quiet room the lamp answers what is
said near it. Its name was not heard. The eyes saw no face. The session
had been running since 09:43, so it was not one of a new session's first
two requests. What it came down to was the model taking a short word like
"hey" (in Vietnamese, the word people call each other with, and the
lamp, with: "ơi") for a call.

**What the watching policy made of the same morning**
([policy.md](policy.md)). It judged the 13 of these that had text. For
12 the decision model answered, in 0.6 to 2.5 s, and chose to look up
six times and to stay quiet six times, never to answer. For the
thirteenth, which was this one, the model did not answer within its 3 s
and the policy's fallback rule said to answer (0.74). So the judge that
would probably have kept the lamp quiet was late for the one case that
mattered, and the rule that stood in for it agreed with the mistake.

**What it says about the design.** One answer to someone else's word in
three and a quarter hours is the best the lamp has done in an office,
and it is still a lamp speaking to nobody. The signs that were there and
unused: nobody was facing it, and it was one word. A person at the next
desk who hears "hey" with nobody looking at them looks up; they do not
answer. That is the policy's middle choice, and the body can already do
it. It needs the eyes' evidence where the decision is made in time,
which is what the local judge is for (task 17 of the running list), and
a fallback rule that looks up, and does not speak, when the judge is
late.

Not shown: someone who did call the lamp that morning and was ignored.
Nobody is known to have tried.

## A meeting beside the lamp (lamp-4ace, 2026-10-14, 11:08 to 11:21)

A meeting began at the table beside the lamp at about 11:08. In the next
twenty minutes the voice took 75 things as said, the part that listens
for the name heard it four times (0.928, 0.930, 0.953 and 0.993, all in
people's talk), and the lamp answered three times: at 11:10:37 "Four." (the model
had been told its name was heard), at 11:15:04 a long answer about a car
part, and at 11:17:29 the time, wrongly ("roughly around ten forty-five").
None of the three is known to have been said to it. From its own records:

- **The room was still taken for quiet.** It had been quiet since 10:50.
  The meeting's first things left unanswered were 54 s apart (11:09:08 and
  11:10:02), the third more than three minutes later, so the run of three
  inside a minute that makes a room busy did not come until 11:14:50. The
  answer at 11:10:37 was given in a room the model had been told was
  quiet, where what is said near the lamp is most likely said to it.
- **Busy was known and not told.** From 11:14:50 the room was busy, but the
  fact waits for six seconds with nothing heard or said, and for no
  conversation going on; a meeting gives neither. The session was told at
  11:16:30. The answer at 11:15:04 came in between, after a name note at
  11:14:47.
- **The name was heard in people's talk.** 0.953 and 0.928 at 11:10,
  0.930 at 11:14:47, 0.993 at 11:20: what this part hears in Vietnamese
  talk scores as high as a real call ([name.md](name.md)), so the score
  cannot be the test.
- **The time.** A session is told the time when it opens; this one opened
  at 10:48 and was told to give the time "roughly".

What was changed, each behind a switch, 1 to 5 on lamp-4ace since 11:52 the same day (`0ca828c67`) and 6 since 12:10, none yet played to:

| | Change | Switch |
|---|---|---|
| 1 | A quiet room becomes busy at two things left unanswered inside a minute, not three | `LAMP_ROOM_FACTS=on`, as before |
| 2 | When the room changes and no quiet moment comes, it is told inside the request being heard, beside what is said, as the name is; a request's name and room go as one note | `LAMP_ROOM_FACTS=on`, as before |
| 3 | In a busy room the name note is given only when the eyes' newest look, under two seconds old, has a face turned to the lamp (`humans/faces/tracked`, [eyes.md](eyes.md)); otherwise it is held and the journal says why | `LAMP_NAME_NOTE_BUSY=face`, beside `LAMP_NAME_NOTE=on` |
| 4 | The model asks the lamp's clock for the time ([tools.md](tools.md), "`the_time`") | `LAMP_CLOCK=on` |
| 6 | The rule is decided at a call for something to be done, too, and a held request's call is answered and not run, except a call that withdraws consent, today only `reports_stop`, which is done though its answer stays held ("What happens to an answer that is held back", since `90c66e1bf`): at 11:57, with the busy form on, the model asked for the light during the table's talk and the light was switched on while the answer's sound was held. On the lamp since 12:10 (`46403ffbd`) | `LAMP_ATTENTION=on` or `busy` |
| 5 | The rule's busy form: in a busy room every answer is held, not only a new session's first two, unless a conversation with the lamp is going on, the text shows it was called, or the eyes' newest fresh look has someone turned to the lamp; the name heard in the sound counts only with someone turned to it | `LAMP_ATTENTION=busy` in place of `on` |

Against this meeting, by its records: with 1 the room would have been busy
from 11:10:02, before the first answer, and with 2 told inside the next
request; with 3 the notes at 11:10:37 and 11:14:47 would have been held
unless someone was turned to the lamp (the eyes' newest look at 11:29 had
nobody). The third answer, the time at 11:17:29, came in a room the model
had been told was busy, with no note: 1 to 3 would not have stopped it,
and with 4 it would at least have been right. 5 is for that one.

Why 5 and not only 1 to 3: the same afternoon, as silent probes (a
recorded sentence and a written note sent to the service, nothing played,
one run each, a new session each time), with the note that the office was
busy beside it, "Did you push the fix for the login page yesterday?" was
left alone and "Hey Lamp, what do you think we should have for lunch?" was
answered, but "What is 2 + 2?" was answered "Four." and "How was your
weekend?" was answered as if asked of the lamp. Told the room is busy, the
model still takes a plain question for its own. A person at the next desk
in a busy office answers a question put to them by name or with a look.

**Played, 2026-10-14, 13:52 (four rounds of `twopeople.sh`):** the lamp
spoke into none of the 36 lines of two people talking to each other, and
answered the call that followed in one round of four. In the other three
its name was heard (1.000, 1.000, 0.922) and nobody's face was seen, a
loudspeaker having none, and the answer was held. Today's record of the
name heard in the office with nobody calling, 07:45 to 13:02: 17 hearings,
0.903 to 0.997. So since 14:04 (`e07449e13`) a name heard at 0.999 or
more counts as a call in a busy room with or without a face
(`attention::SURE_SCORE`), and stands for a face in the name note too;
on those numbers three of the four calls are answered and none of the
seventeen. A second played check at 14:05 does not count: colleagues were
talking beside the lamp and the service wrote their words.

**A name heard close to the lamp (built 2026-10-14 evening, switch
`LAMP_ATTENTION=busy-near`, off unless set; on lamp-4ace since 18:46 that
day, build `41d09542f`).** The calls held in the
afternoon's played rounds had their name heard at 0.97 to 0.99 with no face
seen. With the owner at lamp-4ace the same evening, the room microphone
(the ears, `docs/ears.md`) rose 19.5 to 26.5 dB over the room's quiet for
the ten lines said to the lamp and 9.8 to 15.1 dB for people farther off.
So in its busy form, a name heard at 0.95 or more (`attention::NEAR_SCORE`)
while the room microphone rose 18 dB or more over its quiet during the
request (`NEAR_RISE_DB`; read from `ears/level` every 100 ms, the lamp's
own voice left out) counts as a call. What it costs: the morning's record
had the name heard at up to 0.997 in people's talk; if that talk is at the
desk, close, it is answered. Played scenes cannot show it: the office's
loudspeaker is a metre away and reaches the room microphone at about 8 to
16 dB. Not yet run on the lamp.

Its rises are in the journal, so the bars can be set from the lamp's own
requests (#829, the first builder at 15:23 and 17:09; on lamp-4ace on 16
October request 169 passed the near rule from colleagues talking about eye
drops at the next desk, and nothing logged its rise). With `busy-near`,
each verdict's line ends with the rise the rule judged at the request's
first output: "its answer is held back: … (the room microphone N dB over
its quiet)", and "its answer is played: its name was heard close to the
lamp (the room microphone N dB over its quiet)". So the true calls held
are counted beside the answers let out. The name note's line "the lamp's
name was heard in a busy room and nobody is turned to the lamp" gives the
rise over the request's sound so far, when the name is heard, or "no rise
of the room microphone read". Nothing the lamp does changes.

**This home's bar, measured at setup (#753, `lamp-near-call`, nothing
reads it yet).** 18 dB is one evening's guess at one desk. Setup asks the
person to say the lamp's name from their own chair, and each answer is
one try (`near_try`):
- the name heard once on `name/heard` within 6 s of the prompt's end, at
  0.95 or more;
- mic1's largest rise over its quiet from the hearing's start to 1 s
  after, the lamp's own voice left out.

A try doesn't count if the lamp spoke from 0.5 s before the hearing to the
end of that second. Its echo would be in the number, and speech anywhere
else in the window is not. Three tries are needed, from at most five
prompts, and setup stops asking once five can't reach three (`asks_again`).
The bar is the quietest try less 3 dB, held to 16–24 dB (`near_rise`). It
is never under 16 because people farther off reached 15.1 dB, and never
over 24 so a loud talker isn't left with a raised voice as the only way in.
No bar is set, and the rule keeps 18, with fewer than three tries or when
they spread over more than 10 dB.

Only numbers are kept, never sound: `/var/lib/lampOS/voice/near-call.json`
(0600, written whole), `{"near_rise_db", "tries": [{"rise_db", "score"}],
"at_unix_s"}`. A file whose bar its own tries don't make is not used, nor
one with a try no step could have accepted (a score outside 0.95 to 1, or
not a number).
Setup's `how_you_sound` step (#1803, [onboarding.md](onboarding.md)) takes
the tries, with the prompt's end read from the Unix clock when its playback is
whole, and writes the file on onboarding's storage worker.

**The rule reads it with `LAMP_ATTENTION_NEAR_CALIBRATED=on`** (off unless
set), on both routes: the Live worker's busy-near rule and `--addressed`
judgement, the second route's heeding, and the coordinator's room floor. Each
takes the bar from `attention::near_rise_db()`, which looks at the file at
most every 10 s and reads it only when it has changed, so a new measurement is
used without a restart. A missing or unfit file keeps 18 dB. Each process's
journal says the bar in use when it changes: "lamp-voice: the near rule's
bar is 19.4 dB over the room's quiet (this home's, measured at setup)", or
"(the default: no measurement that fits)". The switch goes to the two
routes' workers alone (`process.rs`). The reset wipes the file
(`reset-plan-product.json`, and lamp-06c1's plan). `NEAR_SCORE` and
`NEAR_RISE_DB` are `lamp-near-call`'s own, so setup and the rule share one
bar.

What each costs. 5: in a busy room a question put to the lamp without
its name in the service's text and without a face turned to it is not
answered, nor one from someone whose face the camera cannot see or with the
camera closed; played voices from a loudspeaker have no face, so played
busy-room scenes measure the text alone with it. 1: a person alone who says two things inside a minute
that the lamp leaves alone, then asks it something without its name, is
not answered until they use the name. 2: a note that is not about what is
being said goes to the service in the middle of someone's sentence; the
name note has gone there since 2026-10-14 and three of three called
questions were answered with it, but a room note has not been tried. 3: a
real call in a busy room from someone not facing the lamp, or with the
camera closed, goes without the note, as it did before notes existed; and
whether the eyes judge "turned to the lamp" rightly has not been checked
with a person in front of this lamp. The second look (in the coordinator)
is not gated by the room or the eyes: it comes only when the model said
nothing and the name was heard, and in this meeting it never came.

## Said to the lamp from close to it (#753)

On both voice routes: the second route judges it as the Live worker does
([second-voice.md](second-voice.md)). Off unless `LAMP_ADDRESSED=on`, and only with `LAMP_ATTENTION` set, since
the rule is what follows each request, and `LAMP_BUS=on`, since the ears,
the eyes and the topic are reached through the bus's root, which the
coordinator gives the worker only then (`--bus-root`, `crates/voice/src/bus_out.rs`).
Without the bus every sign is unknown, nothing is addressed and nothing is
published (`crates/voice/src/addressed.rs`,
built by the fourth builder on 15 October 2026). For each request the
provider worker judges two signs it already reads, over the request's own
sound, from when it began until its sound ended:

- **Near**: the room microphone's highest rise over the room's quiet while
  the voice microphone heard the request (`ears/level`, the same reading
  and the same 18 dB line as `busy-near` above). So the two microphones are
  used together: the voice microphone says when the person spoke, the room
  microphone how loud that was over the room.
- **A close face turned to the lamp**: of the eyes' looks taken while the
  person spoke or in the 1.5 s before, at least 2, the share that saw a
  face judged turned to the lamp and at least 0.15 of the picture's height
  is 0.4 or more. These are V1's gaze rule's lines, measured on its lamps
  (`hal/config.py`, `GAZE_*`): a share and not one look, because one look's
  judgement of a head's turn is noisy; from just before the speech, because
  people turn before they speak; 0.15 because on 2026-09-30 colleagues'
  faces were 8.3% to 13.6% of the picture and the person at the desk 19.2%
  to 46%; 0.4 because a still person centred in view measured 50% against
  V1's first line of 60%.

Said to the lamp is both. Neither alone is enough: someone at the desk may
be talking to someone else, and in the one robot study with numbers,
looking at the robot went with speaking to it 65% of the time
([said-to-whom.md](said-to-whom.md)). It needs no name.

What it does:

1. **On the bus.** Once for each request, `attention/addressed`
   (`lamp_msgs::Addressed`, an event): the session and request, when its
   sound began and ended, the rise, whether near, how many looks, the
   tallest face turned to the lamp, the share of looks with a close one,
   whether that is facing, the name's best score, and `addressed`. A
   sign the ears or the eyes said nothing fresh about is `null`, not
   `false`. It is published when the request's sound ends, or when its
   answer begins if that comes first, for the Policy and anything else.
   A turn the lamp wrote itself (a hello, a look) is not told, nor a
   request cancelled before either.
2. **In the rule.** In a busy room, an answer the rule would hold is let
   out for a request said to the lamp, with the reason "someone close to
   the lamp and turned to it spoke". It never changes an answer the rule
   lets out already, or its reason. The rule's own sign, `turned`, is the
   eyes' newest look when the answer begins, which may come after the
   person has looked away, and any face in view; this one is the looks
   while they spoke, and only a close face.
3. **In the record.** With `LAMP_FIRST_OUTPUT` set, the judgement is the
   line's `addressed` field beside the decision. Without the switch the
   line is as before.

The voice microphone's own level is not used: its processing evens out its
level, the two microphones' streams have clocks of their own
([microphone-topology.md](microphone-topology.md)), and `mic2/audio` is on
the bus only with `LAMP_BUS_AUDIO`.

The cost: at most 300 rises and 200 looks are kept (the last 30 s); with
the switch on, the eyes' newest look is read every 100 ms beside the ears.
Each request is judged once, in the worker's loop, before its answer's
first sound is let out. On the fourth builder's Intel Mac (load average 10
to 14, release build, 2,000 times each) judging a request with the 30 s
full took 1.5 µs (at most 7.2 µs), and reading the eyes' newest look from
the bus 21 µs (at most 77 µs). Not measured on a lamp.

Not shown: any of it on a lamp. Its lines were measured for other rules:
18 dB for `busy-near` on lamp-4ace, and the face's lines for V1's gaze on
its own lamps (green-lamp's frames for the size); recordings at the lamp and from 3 m are
asked for on #753 to check them. The host tests use synthetic
levels and looks, and a real bus in a temporary directory.

## A cut keeps its name (#782)

Off unless `LAMP_CUT_KEEPS_NAME=on` in the voice's environment (built by the
fourth builder on 15 October 2026). The fast early end (`--endpoint
smart-turn-fast`) ends a turn at a pause the person then goes on after, and
the rest is admitted as a new request. It continues the first in two ways:

- On lamp-4ace's path, with no end sent early: the turn was ended, by a
  confident verdict at the commit (400 ms, `--endpoint-commit-ms`) or by the
  600 ms window, and speech begins again within a second of the block that
  ended it (`CONTINUES_WITHIN_US`, from the block that ended it to the block
  that set the new speech off, by the capture clock), before anything of the
  answer was heard, on the same connection and microphone
  (`EndedTurn::goes_on_in`, checked as `commit_admitted_input` admits it).
  The first form of the switch covered only the next way, and on this path
  did nothing: found by the reviewer on #809 and mended the same evening.
- With `--early-end-ms`: speech begins again after the end was sent early
  and before the lamp was sure (`continue_after_early_end`).

With the switch, the coordinator says so in that request's Start
(`ProviderInput::Start::continues`, sent only then) and in its
`input_admitted` record (`"continues":true`, only then), and the provider
worker begins the request with what the rule knew of the
first part: the name, heard surely or not and how surely, the text heard so
far, whether it was close, whether it was said to the lamp from close to it
(`LAMP_ADDRESSED`, above), and the window from where the first part began
(`Attention::continued_at`). A request on another session keeps nothing.
Without the switch the Start is the line it always was, and every request
begins afresh, as before.

Why: on lamp-4ace on 15 October, 9 of the 84 fast ends in the played scenes
fell at a comma inside a line, and the 600 ms window ended two more just
after "Hey Lamp," ([endpoint-pilot.md](endpoint-pilot.md), "Cut at a
comma"). The name was then said 1.5 s or more before the new request began,
so it did not count for it, and in a busy room the rest was held as not said
to the lamp, as on 14 October at 13:32, four times. A test through the
worker's own loop with the start kept shows it: in a busy room, "Hey Lamp,
… about lamps," then "just in its reply …" is held without the switch and
answered with it.

What it does not change: when the turn ends, what the provider hears, or a
request that begins afresh. Speech over an answer that has begun to be heard
is cutting in, as it always was, and so is speech more than a second after
the end. A continuation is speech the coordinator heard begin soon after it
ended the turn, or, with an end sent early, before its silence window ended.
It cannot tell a second voice that begins within that pause from the first
going on: such a voice would keep the first part's name too. That is the
case to watch for when it is tried in a busy room.

What it cannot reach: once the last three things the lamp heard went
unanswered (`body_link::Attention::busy`), a new sound within 1.8 s of an
end, while the answer has not begun, takes no turn (`HOLD_FLOOR_US`, the
`floor_held` record). The rest of a cut line is dropped there before it is
admitted, with or without the switch. In lamp-4ace's records of 15 October
(10 sessions, 932 ends, `~/lamp-voice-data/sessions/turn-times-20261015`,
read on the Mac the fourth builder shares with the first), 401 holds came
within a second of the end of the turn they held, and 46 next turns were
admitted that soon. Which of the 401 were the same person going on, and
which the played room's other voices, was not measured. Tests:
`a_turn_ended_at_a_comma_goes_on_in_the_next_request_only_with_the_switch`
and `a_new_request_long_after_the_end_does_not_go_on`
(`coordinator/tests/endpoint_session.rs`, the model's end at the commit),
and `a_question_after_the_window_ended_its_name_goes_on_and_cutting_in_does_not`
(`coordinator/tests/early_end.rs`, the real provider worker, the window's
end, and speech over a played answer).

## A tap on its head is a call (#711)

Off unless `LAMP_HEAD_TOUCH=on` in the voice's environment, and only with
`LAMP_BUS=on`, as the name heard: the provider worker reads the head's taps
from `--touch` Controls on the bus (`controls/touch`, `bus_out::TapsIn`).
A tap is a touch on the head let go between 0.12 and 0.5 s
([controls.md](controls.md)): the owner's "tap its head while it is quiet:
you can talk without its name" ([creature.md](creature.md#touch)). A request
that begins within 2 s of the tap's release (`TAPPED_BEFORE_US`, the same
window the ring's listening face and Expression's look-up show; it was 6 s
until the #849 review found it let a request out after both had settled), or
one whose
sound is still going on when the tap comes, is let out in every room and
every form of the rule, even the strict busy one with nobody seen turned to
the lamp: a hand on the lamp's head is surer than any hearing of its name.
A short sound is the exception: the check for short sounds (#800) judges it
first, so a cough after a tap is not taken for a question.
A tap read more than 2 s after its release, or the same tap again, counts for
nothing. Later requests are judged as before.

**So does the lamp's name said alone** (#829, the first builder at 16:32).
On lamp-4ace the owner said "Lamp," and paused. Its call's wait ran out
(#891), and the model was told only the name had come. Then "Check my
work." came as a request of its own, and was held in a busy room by day
with nobody seen turned: "its name was heard in the sound, and nobody is
turned to the lamp". Now a request that begins within 3 s
(`AFTER_CALL_US`) of the end of one that was only the name
(`ProviderInput::Called`, then its End) is let out as the tap's is, in
every room and every form of the rule, on the Live route. The second
route drops `ProviderInput::Called`, so there it is judged as before.

It counts only if the call itself showed the lamp was called, as the busy
room judges any request (`Attention::call_shown`):
- its name heard at 0.999 (`SURE_SCORE`), or at 0.95 close to the lamp
  (`NEAR_SCORE`, the room microphone 18 dB up), but by day with
  `LAMP_SPOKEN_TO=day-and-night` close is not enough on its own, as for a
  request (the 17:09 sign below; the reviewer on #1158). The eyes are not
  read for a bare call, so then only its words or a sure hearing show it;
- or its text a call by name (`called_by_name`: "Lamp?", "Hey Lamp.");
- and not a call to someone else (`addresses_someone_else`).

A bare call is made by the name's first sign (`name/rising`), which comes
at the name part's own 0.9 and is for timing only ([name.md](name.md)). In
one morning that part heard the name 17 times in people's talk, at 0.903
to 0.997 (above). So on that alone, a similar name heard at 0.92 in a
pause let out whatever began in the next 3 s in a busy room (the reviewer
on #1108). The call is judged as the next request begins, on all it
heard: its text, its hearings of the name, and how close it was. How
close is known once its own answer began, as for any request, so a call
the model did not answer counts on its score and its text only. Two
requests within the 3 s after a call that counts are both its. The bare
call's own request isn't called by itself. A short sound is still judged
as one first. The Policy's hold for the lamp's actions (#1054) is
Second's side of it.

The 3 s is not yet measured. The owner's two cases at 16:30:25 and
16:31:21 are what it has to cover: the gap from the bare call's End to
the next request's start, which the session's journal holds.

Tests: `attention::tests::a_request_just_after_the_lamps_name_said_alone_is_called`
and `a_bare_call_counts_only_when_its_own_hearing_shows_the_lamp_was_called`
(the rule), and, through the worker's own loop from the call's
`ProviderInput::Called` and End,
`provider_worker::tests::spoken_to::a_request_after_a_bare_call_is_answered_only_when_the_call_showed_the_lamp`:
after "Lamp." the next request's answer plays, after "Hey Clamp?" it is
held.

**The "hm?"** (`crates/voice/src/head_cue.rs`, First's design on #711): a tap
on a quiet lamp's head plays a soft hum of its own, two short tones rising
like a question (330 and 415 Hz with two soft overtones, 110 and 160 ms with
30 ms between, each gliding up 4%, faded in and out over 25 ms; 7,200
samples at 24 kHz). It is its own turn, a sound turn
(`coordinator/speaker_tone.rs`, `commit_sound_turn`): admitted with
`admission_basis: head_tap_cue`, nothing sent to the provider, played whole
through the voice's own speaker path so the echo canceller has it, and none
for the face, the head, the room or the voice's state. Each tap writes a
`head_tap_cue` row with `plays` and, when it does not, `why`:
- `microphone_not_allowed`;
- `was_speaking`: the lamp was heard speaking after the hand arrived (that
  touch stopped it, and stopping is the whole answer);
- `not_quiet`: a turn, input, reply, playback or the keeper's pause;
- `listening_not_ready`, `reply_owed` (a look it was asked for);
- `hummed_recently`: at most one in 30 s; the tap is still a call.

Its default level is -20 dBFS RMS while it sounds, raised from -35 after
Thirteen's tap-hum comparison on lamp-c0a4 at product speaker level:
-30 dBFS was ear-only, -25 faint, and -20 clear. First approved -20 at
11:40; the [#3275 carry instruction](https://github.com/autonomous-ai/lampOS/pull/3275#issuecomment-6368135260)
records that choice.
`LAMP_HEAD_TAP_CUE_DBFS` (from -60 to -20) sets it for tuning by ear; a value
outside that refuses the voice's start. The shared default also sets the
wake hum, the hand-sleep breath, and the unclear and misheard hums in the
coordinator when this override is absent. `say`'s `Cue::Hum` uses the same
default directly, without reading the override. The c0a4 level comparison
covered the tap hum only: it does not qualify those other sound paths by
ear, or measure physical hand-to-first-speaker latency. The trace's
`head_tap_cue_attempted`, `head_tap_cue_first_write` (ALSA accepted) and
`head_tap_cue_final_sample_retired` follow it to the speaker.

How fast: Controls raises a tap 150 ms (`head_gap_ms`) after the hand lifts,
so a pulse in the pad is not a new contact. On Third's Mac the hum's decision
to its first chunk sent to the speaker took 2.3 ms median and 2.7 ms p95
(`head_tap_cue_decision_to_speaker_dispatch_latency`, ignored by default);
the bus to the voice's poll took 1.8–2.1 ms median before. Controls' tap to
the speaker's write on a lamp is not measured yet (First's target: under
150 ms).

When the hum plays, the ring shows the listening face at rest for 2 s,
settling back over half a second ([ring-choreography.md](ring-choreography.md)),
and Expression gives its small look-up ([alive.md](alive.md)): one gesture
from one tap event, as First asked on #711 at 05:05. The hum's own turn is
drawn as that listening face in every frame (`RingFrame::listening`), from
the pass that admits it, so the face comes with the hum. The ring keeps it
at rest for the rest of the window. A tap the voice doesn't answer gets no
hum and no face (First on #705, 17 Oct 07:26). The tap is still a call for
the attention rule.

**A hand's sleep: a sigh, and the "hm?" as it wakes** (#1362, the first
builder on #1362, 21 Oct 09:21 and 10:35). With `LAMP_HEAD_TOUCH=on` and
`--controls-mute`, a hand held on the head puts the lamp to sleep at 3 s.
Two quiet sounds of the lamp's own let the person hear that it worked; no
chime, never a beep:
- **The breath, in the hold's last second.** When Controls says the hand
  has been on the head 2 s (`controls/touch_state`, the head's `held_us`,
  as the ring's countdown reads it), the voice plays a falling sigh
  (`head_cue::breath`): 0.9 s of breathy noise that darkens as it fades
  (brightest at 1.4 kHz falling to 350 Hz), with a faint hum under it
  falling from 247 to 185 Hz. It comes in over 120 ms and fades out over the
  rest, with its loudest tenth of a second at the hum's level
  (`LAMP_HEAD_TAP_CUE_DBFS`). It is its own sound turn
  (`admission_basis: head_sleep_breath`), once a hold (one contact), and only
  while the lamp is quiet, allowed and listening ready (`head_cue::may_sigh`;
  not held to the tap's 30 s). It begins only between 2.0 and 2.2 s of the
  hand (`BREATH_UNTIL_US`), while all but its last, already fading tenth of
  a second fits before the sleep: a hand first seen later (a busy loop, or a
  conversation opened late in the hold) gets none, never one cut to a tick
  (the reviewer on #2487). The trace row is `head_sleep_breath`, with
  `held_us`, `plays` and `why`. The ring shows nothing for it: the hold's own
  dimming goes on. **It never moves the sleep:** the speaker plays only
  while the microphones may hear (it drops all sound when they are denied,
  `PrivacyClosed`), so the breath can't follow the mute. It comes before
  it instead. Controls' sleep at 3 s ends the conversation on the pass that
  sees it, as before, with the breath cut there if it still plays, so the
  microphones and the camera close not one sample later for it. A hand
  lifted between 2 and 3 s gets a sigh and no sleep: a creature that starts
  to settle and doesn't.
- **The "hm?", as the person wakes it.** A tap on the head (or the button)
  that opens a hand's sleep marks the wake in the controls
  (`HandControls::take_woken`, fresh for 2 s). The conversation the wake
  opens says this same rising "hm?" first, as its own turn
  (`admission_basis: head_wake_cue`), once it is quiet and listening is
  ready, strictly before 1.5 s from that conversation's original start or
  not at all. This is a conversation-start limit, not the tap's deadline:
  the wake marker may already be up to 2 s old when consumed. The existing
  bounds therefore permit a decision to play almost 3.5 s after the hand; the
  acquisition-to-first-write target remains 500 ms, to be measured on a lamp.
  Startup's microphone permission may still be stabilizing for 60 ms;
  this refusal waits alongside quiet and listening readiness, without
  restarting the window or permitting any sound while denied. At or after
  the deadline even a newly ready loop drops it (`wake_window_expired`).
  A physical mute, a new hand sleep or an error cancels it with the
  conversation, recording one refused `head_wake_cue` with
  `why: conversation_ended` if no decision has yet been made.
  If no hum is configured, the wake marker is consumed without scheduling
  or recording a cue, including when a button clears a saved hand sleep
  after head touch has been disabled.
  It is not held to the tap's 30 s, since the person asked for it. The ring
  shows the listening face with it. The trace row is `head_wake_cue`, with `acquired_at_us` (the
  waking input), `plays` and `why`. No sound comes for a mute the restart
  guard left, or for the person's own mute: only for a sleep a hand chose.
  Expression's night sleep is not the voice's, so neither sound plays for it.

Not measured on a lamp yet: the breath's first sample against the 2 s mark,
and the tap to the "hm?". Both follow `head_sleep_breath_first_write` and
`head_wake_cue_first_write` in the trace. The targets are the breath within
50 ms of 2 s, and the "hm?" within 500 ms of the tap.

**The same "hm?" after an empty "Sorry?"** (`LAMP_UNCLEAR_HUM=on`, off unless
set; #1417). When the lamp can't make out a turn, it asks again with a
written turn of its own that tells the model to say only "Sorry?" (#926). On
lamp-4ace on 17 Oct (16:26) "make your light a bit cooler" was heard as Hindi,
that turn came back with no audio (`no_audio_answer`), and the person got
silence. With the switch, that turn's silent end owes the "hm?". Silent is no
audio at its settled end, or an answer cut as `nothing_said` ("...", or a
function's name said aloud), as the second look counts it.
- It plays as its own sound turn with the ring's listening face, as the tap's
  does, under the tap's rule (`may_hum` above): the microphone allowed, the
  lamp quiet and listening-ready, no look owed.
- The tap's 30 s rarity doesn't apply. That keeps a sound nobody asked for
  rare; this one answers the person's own words each time they aren't made
  out, and each needs words of theirs first. A second empty "Sorry?" hums
  again (the Seventh on #1421). It still sets the tap's 30 s.
- It plays within 2 s of the silent end (`UNCLEAR_HUM_WITHIN_US`), and is
  dropped if any turn begins first or the person's Stop comes (`cancelled`).
- Only that turn: a turn that ended silent because it was the room's talk
  stays silent, and a "Sorry?" the model said owes nothing.
- Expression gives no look-up here: that comes from a tap on the head, and
  the hum's own turn is no turn for the head (`speaker_tone`).

The trace says `unclear_hum_owed` at the silent end (with `"after":
"nothing_said"` for a cut answer), then `unclear_hum` with `plays` and, when it
doesn't, `why` (`another_turn_began`, `late`, `cancelled`, or `may_hum`'s word)
and `held_by` (what held it back last, if anything). Its speaker rows are its
own, not the tap's: `unclear_hum_attempted`, `unclear_hum_first_write` and
`unclear_hum_final_sample_retired` (`SpeechOrigin::UnclearHum`), so a count of
tap hums doesn't take it in.

How fast: the hum is decided in the loop pass that sees the silent end, and
its first chunk goes to the speaker as the tap's does (2.3 ms median from the
decision on Third's Mac, above). The boundary to report is the end-of-speech
decision to `unclear_hum_first_write`. Most of it is the provider's: on
17 Oct's transcript the end of speech was decided at 3.63 s, the "Sorry?" was
asked at 4.69 s, and its empty end came at 8.70 s, so the hum would start
about 5.1 s after the person stopped. The target for the lamp's own part is
the silent end to `unclear_hum_first_write` under 100 ms. The light battery
reports both. Not measured on a lamp.

Tests:
- `coordinator::tests::reminders::an_empty_sorry_owes_one_hm_and_a_said_one_or_another_silent_turn_none`:
  the state, with this transcript's two turn ends, a "Sorry?" the model
  said, another silent turn, someone speaking first, too late, the held
  reason and a Stop.
- `coordinator::tests::second_route::an_empty_sorry_in_the_loop_hums_with_the_switch_and_only_then`:
  the loop itself, through a scripted provider. An unclear turn, the written
  "Sorry?", and its empty end hum to the speaker with the switch and not
  without; "..." hums too, and a said "Sorry?" doesn't. Said again after the
  first hum and unclear again, the second empty "Sorry?" hums too. With the
  asking's mark taken out, with "..." owing nothing, or with the tap's 30 s
  put back, it fails.

## The record of whom it answered

Off unless set: `LAMP_FIRST_OUTPUT=/absolute/private/directory` in the
voice's unit, passed to the provider worker as `--first-output-log DIR`
(`crates/voice/src/first_output.rs`, built 2026-10-14 evening for #700,
after the fourth builder's proposed contract in
[voice-first-output.md](verification/local-judge-fit-20261014/voice-first-output.md)).
With it, each time the rule judges the first output of a request, one JSON
line is written: what the rule was given, as the Policy's fitting rows take
it (`baseline`: room, own turn, conversation going on, name heard surely,
the text heard so far, facing), whether it played or held and why, the
request's session and number, what came first (sound, words or a call),
when the inputs were read and when it decided, and the machine, boot,
worker run and the program's SHA-256. Each worker run writes its own file,
open to its owner only, at most 8 MB; lines wait in a queue of 64 and a
thread of its own writes them, so the voice never waits on the disk. A full
queue or a full file is counted, and the run's last line (`capture_end`)
says how many lines were admitted and how many dropped. A file without that
line is a run that did not end cleanly.

Since #799 (2026-10-15) each function call is a line of its own too
(`voice_tool_call`): the request's session and number, the function's name,
when the call reached the worker (`called_us`), when our answer was handed to
the connection (`sent_us`), whether whoever was asked answered and whether
the connection took it. No arguments and no answer. The run's last line then
also counts them (`tool_calls`); a run without calls ends as before.
`lamp-voice turn-times` reads them beside the session records
([tools.md](tools.md#where-a-turn-with-a-function-call-spends-its-time-799)).

Its first build (`8ae7b8868`) failed on lamp-4ace on 2026-10-14 at 20:42:
opening the record read and hashed the whole program on the provider's own
thread, which held the worker so long that its messages reached the voice
late, and the voice ended the worker at every start ("stale, reordered or
wrong-boot runtime message"). Seven restarts and about 45 seconds without a
voice, then the build before it was put back. The hash is now taken on the
record's own thread and written as the file's first line (`capture_start`),
with the machine, the boot and the run; a test checks that opening the
record is quick. That build (`bf01f7e6d`) is on lamp-4ace since 20:48 with
`LAMP_FIRST_OUTPUT=/var/lib/lampOS/voice/first-output`: a minute with no
restart, then two played lines at 20:50. "Hey Lamp, what is the capital of
Italy?" gave one line (played; "the room is not known to be busy", as the
voice had just started; the seven fields as heard; 11 µs from reading the
inputs to the decision), and "Tom, did you finish the slides for
tomorrow?", which the model did not answer, gave none, as it should.

With `LAMP_ADDRESSED=on` each line also has `addressed`: the request's
`attention/addressed` judgement (above), if it was judged.

Each line also has `lamp_asked`: whether the lamp's own last line before the
request ended in a question, as the rule read it (the short-sound case below
turns on it). It is beside the decision, not in the seven `baseline` fields,
which the Policy's fitting reads as they are.

What was heard is in these files, and so are bystanders' words in an office:
they stay on the lamp and are not published. Not recorded yet: whether any
sound of the answer was played (`request_outcome`), when a request began, and
which transcript, name and eyes messages the rule read.

## A short sound let through in a quiet room (#800)

With `LAMP_SHORT_SOUNDS=quiet-room-checked` ([not-speech.md](not-speech.md#a-room-sound-let-through-and-answered-800))
the rule has one more case, before the busy-room ones: a request whose end
came marked as a short sound let through is played only when what was heard
is a short word said to the lamp, it was said close to the lamp (the room
microphone, as for `--attention-near`), someone is turned to the lamp, the
lamp began the turn itself, or it answers the lamp's own question (below);
otherwise it is held. Off unless set; the rule is otherwise as above.

**An answer to the lamp's own question (17 October).** On lamp-4ace on
2026-10-17 at 16:11, the lamp asked "Shall I start it?" and the person said
"Yes. Please start it.", with 700 ms between the two. The end came after
"Yes." (`silence_fallback`), which was heard as "bien", so the request ended
as a short sound. The model's first output was `confirm_agent_task`, and the
check held it: not a short word, not close, nobody turned to the lamp. Held,
the call was answered and not run, and nothing was said. A short sound is
now played, with the reason "a short sound with a word in it, right after
the lamp's own question, in a conversation going on", when all three hold:
- what was heard has a letter or a digit in it;
- a conversation is going on. With `LAMP_SPOKEN_TO=on` it must also not be
  someone else's (#829: a reply to someone just called by name, a line
  calling someone else). The product doesn't set that switch, so on a
  product lamp a colleague's or a TV's short line in the window ("Sí,
  bien") is played too, and the model judges it (the reviewer on #1355);
- the lamp's own last line before the request ended in a question
  (`Thread::lamp_asked`, as #926 reads it).

Then the model judges it. "Right after" is as wide as the conversation:
the lamp's newest line with a word in it ended in `?`, and some line of the
lamp's began within the last 45 s (`GOING_ON_FOR`). Nothing short that ends
within 4 s of the lamp's own words is marked a sound at all
(`AFTER_OWN_WORDS_US`), so the case sees sounds from 4 s after the lamp
stopped until 45 s after its question line began. It isn't once: a held
sound adds no line of the lamp's, so every sound with a word in it in that
window plays, until the lamp says something else (Twelve and the reviewer on
#1355). The thread times an exchange from when the lamp's line began, not
when it ended, so a tighter bound would cut the question at the end of a long
answer. Only an ASCII `?` counts as a question (`Thread::lamp_asked`, #926's
own): a line ending in a full-width `？` would not, which doesn't matter for
the English the lamp speaks. It reads only the line's last mark: an answer
that ends in a question of form ("Anything else?", or a story's "Who knew?")
opens the window too, and a question with words after it ("Shall I start
it? It takes a minute.") doesn't. The first-output record's `lamp_asked`
shows how often it opens. A room sound with no question before it is held
as before: of the 17 requests held this way on lamp-4ace from 16 to 17
October, only 3 came in a conversation (the first builder's count on #828).
What it lets through is a sound heard as words in that one moment, "¿Qué?"
as well as "bien". It is part of `quiet-room-checked`, the mode the product
runs (#1332), not a switch of its own. The second voice does the same
from the same thread: its worker tells the engine whether the lamp has just
asked (`worker::lamp_just_asked`), and the rule then judges the line as any
other ([not-speech.md](not-speech.md#a-room-sound-let-through-and-answered-800)).
Tests: `a_short_sound_with_a_word_right_after_the_lamps_question_is_its_answer`
(`attention.rs`: "bien" played in every room; held with no question, with no
conversation, with no word; a short word keeps its own reason; unchecked or
not a sound, the question changes nothing) and
`a_sound_with_a_word_right_after_the_lamps_question_is_answered` (the
worker's own loop, with the conversation as the worker keeps it: "bien"
after "Shall I start it?" answered, after a line that asks nothing held);
on the second voice,
`a_checked_sound_with_a_word_right_after_the_lamps_question_is_answered`
(`second_provider/tests.rs`: "bien" answered when the lamp has just asked,
held when it hasn't, "¿?" held) and
`the_lamp_just_asked_only_with_its_question_in_a_conversation`
(`second_provider/worker.rs`: the question within the conversation's 45 s,
not after it, not a line that asks nothing).

## The lamp spoken of is not a call (#829)

Off unless `LAMP_NAME_IN_PASSING=ignored` in the voice's environment (built
by the fourth builder on 15 October 2026; the coordinator passes the worker
`--name-in-passing-ignored`, right after `--attention-near`).

The rule counts the text as showing a call when the word "lamp" is anywhere
in it ([`called`](../crates/voice/src/attention.rs)). So in a busy room
"I left the lamp on in the kitchen", said by one person to another, lets an
answer out, and so does "the lamp said it would rain". On lamp-4ace it was
played twice (14 and 15 October). Both times the model declined it of its
own accord, so the rule did not decide what happened.

With the switch, the name counts only when it is said to the lamp: opening
a sentence, after any greeting ("Lamp, ...", "Hey Lamp, ...", "Okay.
Lamp, ..."), or right after a comma ("Can you explain that, Lamp?",
"Can you, lamp, ..."). After any other word it is a lamp spoken of: "I
left the lamp on", "a new lamp", "her desk lamp" (`called_by_name`; the
reviewer's cases on #864 are its tests). What the service writes in the
name's place is read as before.

In the played scenes of 14 and 15 October, the service's text held "lamp"
14 times. Three spoke of a lamp, and two of those were inside a request
that is still a call ("Hey man, ... a story about a lamp ...", the service's
word for "Hey Lamp"). Without the switch nothing changes.

Tests:
- `the_name_said_to_the_lamp_is_a_call_and_the_lamp_spoken_of_is_not_with_the_switch`
  (every call the service wrote on 13 October, and the lamp spoken of);
- `the_lamp_spoken_of_in_a_busy_room_is_held_only_with_the_switch` (the
  rule);
- `the_lamp_spoken_of_is_held_in_a_busy_room_with_the_switch`, and the same
  without it played (the worker's own loop);
- the flag's place among the worker's flags (`addressed.rs`).

What it does not do: tell who said a call. A person who says "Lamp" to the
lamp while facing someone else is still calling it.

## The Policy holds the lamp's actions (#800)

Off unless `LAMP_POLICY_HOLDS_ACTIONS=on` in the voice's environment (the
coordinator passes the worker `--policy-holds-actions`, right after
`--sounds-checked`), and nothing without `LAMP_BUS=on`.

On lamp-4ace on 15 October at 14:49 the lamp heard "Thailand" in a busy
office, ran `volume_up` and stayed 7 dB louder for eleven hours. The
Policy's decision in the same second was `stay_quiet` (p = 0.235). Of
3,698 attention decisions from 13 to 16 October, the Policy chose
`stay_quiet` for 2,345, and the lamp spoke anyway on 437 of them (the first
builder, #800). That is too many to hold all speech on the Policy yet, but
an action changes the lamp and outlasts the moment.

With the switch, a call that changes the lamp, any of the executive's
functions (the volume and the light), or one of the schedule's three that
set or take away something to come (`set_timer`, `set_reminder`,
`cancel_reminder`: on lamp-4ace on 16 October "Yes, give me five minutes",
said to someone else, set a five-minute timer; the first builder on #908),
waits for the Policy's attention decision about its turn. The schedule's
other three, `list_reminders`, `repeat_reminder` and `snooze_reminder`, are
not held (the first builder, 05:57), nor is any other call:
- **A reply to the lamp keeps its schedule** (#1040). A `stay_quiet` whose
  `follows_lamp` answer is 0.6 or more (`policy_holds::FOLLOWS_FROM`) still
  holds the volume, the light and the executive's functions, but not the
  schedule's two that set something to come, `set_timer` and
  `set_reminder` (`policy_holds::SETS`). On lamp-4ace on 2026-10-16 at 11:20 "Set a timer for 1
  minute", said to the lamp in a conversation with it, was held and no
  timer was set: the fitted weights count a follow-up to the lamp against
  answering, though `follows_lamp` told the lamp's lines apart (median 0.60
  on 33 said to it, 0.29 on 46 not, the first builder's reading; at 0.6 or
  more, 17 of the 33 and 6 of the 46, against 19 and 14 at 0.5). A timer
  nobody asked for is a smaller harm than one asked for and not set, and
  it can be cancelled by asking. That 11:20 turn itself scored
  `follows_lamp` 0.35, so it is still held at 0.6: this lets through 17 of
  the 33 turns said to the lamp, not that one, and catching it on
  `follows_lamp` alone would take 0.35, which lets through 28 of the 33 but
  also 17 of the 46 not said to it (the reviewer on #1054). Only in
  conversation: the lamp's sound must have gone out within 20 s before the
  turn (`policy_holds::IN_CONVERSATION_US`, the Policy's own "in
  conversation"); all 79 rows were in conversation, and out of it a high
  `follows_lamp` is not measured (the reviewer on #1054). A cancel is not let through:
  `cancel_reminder` stays held as in any `stay_quiet`, since a wrong one
  is silent, nothing puts the reminder back, and the person finds out only
  when it does not come. At 0.6 about one turn in eight not said to the
  lamp is let through this way (Fourth on #1054: "No, cancel it, they
  moved it to Monday", said to a colleague just after the lamp set the
  reminder). The
  journal says why it ran: `the turn follows the lamp's last line, and what
  it sets on the schedule is not held from it`. Until the weights are fitted again
  (#1040). Only a turn that follows the lamp's own words is let through
  this way: one after a line with words that someone said and the lamp
  left unanswered, within 10 s (`Attention::follows_an_unanswered_line`),
  follows that line, and is held as any `stay_quiet`, whatever
  `follows_lamp` says. A short sound let through and ended as one is
  nobody's line, even when it was heard as a word ("¿Qué?"), as the room
  counts it (Fourth on #1054). In the first builder's two-person round on #829
  (15:39), "Yes, give me five minutes." after "Are you coming to the
  meeting at two?" set a timer as a reply to the lamp's story; #1090 holds
  that scene when the question names Daniel or the room is busy, and this
  the quiet room's unnamed question. It is known only with the voice's own
  rule for whom it answers (`--attention`), which follows each line.
  Tests: `policy_holds::tests::a_stay_quiet_that_follows_the_lamp_is_told_apart_by_its_follows_lamp`,
  `policy_holds::tests::only_the_schedule_s_two_that_set_run_on_a_reply_to_the_lamp`,
  `provider_worker::tests::policy_holds::a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet`
  (and a cancel in such a turn held),
  `provider_worker::tests::policy_holds::a_timer_in_a_turn_after_a_line_the_lamp_left_unanswered_is_held`,
  `attention::tests::a_request_after_a_line_the_lamp_left_unanswered_follows_that_line`.
- **With `LAMP_FOLLOWS_RUN=on`, a reply to the lamp runs its light and its
  volume** (#1465), off unless set, and only with
  `LAMP_POLICY_HOLDS_ACTIONS`. The same reply as above (`follows_lamp` 0.6
  or more, in conversation, not after a line the lamp left unanswered)
  runs the light's tools (`light_*`) and `volume_up`, `volume_down` and
  `volume_unmute` (`policy_holds::REPLY_VOLUME`), as well as the schedule's
  `set_timer` and `set_reminder` (`policy_holds::runs_on_a_reply`). Still
  held on a reply: every skill's function, since `check_my_work`'s look
  sends a still of the desk to the model and `dance` and `show_verdict`
  move the body; `volume_mute`, since a lamp muted wrongly cannot say what
  it did; and `cancel_reminder` (the first builder on #1474, after the
  reviewer's finding). Named, or with a face turned, all of them run as
  before. On lamp-4ace on 2026-10-18 at 10:57 "Thanks. Now turn yourself
  back up a little, please.", 13 s after the lamp had turned itself down
  when asked by name, scored `follows_lamp` 0.89 at p 0.31, `stay_quiet`
  (the Policy's room `unknown` after a restart): its `volume_up` was held,
  and the model asked again 72 times in 49 s. What it costs: a line
  scored that high that was not said to the lamp (6 of the 46 on
  2026-10-16) can change the light or the volume, which the person sees
  or hears and can ask back. The journal says why it ran: `the turn
  follows the lamp's last line, and its light and its volume are not held
  from it`. Tests: `policy_holds::tests::with_follows_run_a_reply_runs_the_light_and_the_volume_only`,
  `provider_worker::tests::policy_holds::with_follows_run_a_reply_to_the_lamp_turns_the_volume_back_up`
  (with it off held; a cancel, the mute, `check_my_work` and `dance` held;
  the volume held when the lamp said nothing lately or `follows_lamp` is
  0.5), `provider_worker::tests::policy_holds::follows_run_is_read_right_after_policy_holds`.
- **The turn's decision** is an attention decision the Policy publishes
  for real (`policy/decisions`, `about` `attention`, `shadow` false:
  `lamp-policy --act attention`) about the turn **by its number**
  (`Decision.request`). The voice tells each piece of what it heard on
  `voice/heard` with the number of the turn whose input began last before
  the piece came (`crates/voice/src/bus_out.rs`), the same turn the
  voice's own rule takes it for; the Policy's decision carries the number
  of the pieces it judged, and an utterance never holds pieces of two
  turns: a piece of another turn ends the one open (the reviewer on #908,
  06:40). So a decision is its own turn's however late it comes, and
  never another's: a late decision about the turn
  before neither releases nor holds this turn's action (the reviewer on
  #908). A decision without a number is nobody's; of two about one turn
  (its words judged in two pieces), the newer counts. Where the number can
  still be wrong: the service does not say which turn its text of what was
  heard is of, so a piece of a turn's words it writes only after the next
  turn's input began is the next turn's, here as in the voice's own rule.
- **`stay_quiet`, and the lamp's name not heard** in the turn (a hearing of
  0.9 or more on `name/heard`, as the Policy counts it; known here only with
  `LAMP_ATTENTION`, which follows the turn, and without it the Policy's
  decision alone, which weighs the name itself): the action is not run. The model is told `{"done": false, "next": "Not done: it was not
  clear that was said to you. Do not call it again for this request. Say
  nothing at all."}` (`policy_holds::HELD`), and none of the turn's sound
  is let out after it.
- **Asked for again** (#1465): on lamp-4ace on 18 October at 10:57 a
  follow-up's `volume_up` was held and the model asked for it 72 times in
  49 s, each held, while the lamp said nothing for most of a minute. A held
  call of the same turn made after the model was told is answered `{"done":
  false, "next": "Not done, and it will not be done for this request."}`
  (`policy_holds::HELD_AGAIN`), and the turn is ended there, once: handed on
  as interrupted, so the coordinator lets it go (its playback with it) and
  listens for the next line, and the session's cut stops the service's
  generation. Calls made together with the first, in one message, are not
  asking again: each is answered `HELD`. A call that comes once the
  coordinator has retired the turn is answered as any call of a retired
  turn (below, or by the session itself), never run. The journal line is
  `request N asked again for an action held (volume_up); the turn is ended`.
- **`answer` or `look_up`, or the name heard:** it runs.
- **No decision within 2 s** of the call: it runs, as without the switch.
  While it waits nothing else does: the call is kept aside and looked at on
  each tick.
- **Withdrawn while it waits:** a call the service cancels
  (`toolCallCancellation`), or one whose turn is retired (its owner
  cancelled, or a newer turn) or whose session is no longer the live one,
  is let go, never run. A withdrawn call is not answered; one of a retired
  turn is answered `{"done": false}`, which the service takes only if it is
  still waiting, and the service settles the stopped turn as it does any
  other. All three are looked at again right before it would run (the
  reviewer on #908). The cancellation is asked of the session itself
  (`call_withdrawn`), which notes it as it reads it off the connection:
  while the output is full no provider event is taken, and its event may
  wait behind the sound before it (the reviewer, 05:11).
- Speech stays as it is, and so does every other call, the schedule's
  other three among them. A request the
  voice's own rule holds ([#829](#the-lamp-spoken-of-is-not-a-call-829),
  the busy room) has its calls answered and not run before this is reached.
  A turn the lamp wrote itself is not held. On the second route the same
  gate holds the same calls ([below](#on-the-second-route-1712)).

The worker logs, with no words of what was heard: `request N asked for
volume_up; waiting for the Policy's decision`, then `run after M ms:
the Policy took the turn as the lamp's` (or `its name was heard`, `no
decision came in time`), or `not done: the Policy judged the turn not for the
lamp, and its name was not heard (M ms)`, or that it was withdrawn or its
turn ended while it waited, and not run.

Tests: `crates/voice/src/policy_holds.rs` (the gate; the switch; which
calls are held; each decision its own turn's by number, also before the action is asked for,
shadow and progress decisions and ones without a number passed over; a late
decision of the turn before neither releasing nor holding this turn's
action, in both orders; one still owed to an earlier turn never settling a
later one), `crates/voice/src/bus_out.rs` (what was heard told with the
number of the turn whose input began last), `crates/policy/tests/it/policy.rs`
(`a_piece_marked_finished_is_judged_without_the_pause_and_carries_its_request`:
the decision carries the number of what it judged;
`pieces_of_two_turns_are_judged_apart_each_with_its_own_number`: turn 1's
words and turn 2's 50 ms later, neither finished, two decisions with
numbers 1 and 2, through the real Policy loop and bus),
`crates/policy/src/gather.rs` (`a_piece_of_another_turn_begins_another_utterance`),
`crates/gemini/src/session/tests/tools.rs` (a withdrawn call
known before its event is taken) and `crates/voice/src/provider_worker/tests/policy_holds.rs`
(the worker's own loop against the scripted service, an executive and a bus
of the test's own: `stay_quiet` not done, nothing asked of the executive and
none of the turn's sound out; `answer` and `look_up` run and the sound after
comes out; no decision, run after the wait; without the switch, run at
once; another turn's `stay_quiet` holding nothing; listing reminders not
held; the flag's place; a
kept call whose turn is cancelled, one the service withdrew, and one
withdrawn while the output is full, never run; a timer on `stay_quiet` not
set; a held call asked for again and again, told it will not be done and
its turn ended once, and two held calls made together both told `HELD`,
#1465). The worker tests run on a logical clock of their own (#1128): the
scenario clock for the worker's leases, heartbeat bound and the 2 s wait,
and a paused Tokio clock for the session and the scripted service, moved
together 2 ms at a time by the rig's `tick`. The ten take about 1.7 s, not
15 s, with the same assertions, the 2 s wait included.

### On the second route (#1712)

With the same switches the second route (`LAMP_VOICE_ROUTE=second`,
[second-voice.md](second-voice.md)) holds the same calls of a spoken turn
by the same gate (`policy_holds::gate`, `for_turn`, `runs_on_a_reply`),
in `crates/voice/src/second_provider/policy.rs`. The journal lines are
Live's word for word, `lamp-voice second:` for `lamp-voice provider:`, so
the battery's counters read both routes alike.
- **Its turns are the Policy's by number.** The engine's `Started` for a
  turn comes before the turn's heard words, and the coordinator tells
  those words on `voice/heard` under that number
  (`bus_out::input_started`), as for Live. A turn begun before the last
  one's words were heard ends that one, so its words never come under the
  newer number.
- **One reader of the Policy's decisions.** The rule keeps the route's
  one subscriber on `policy/decisions` (`Heeding::reading_the_policy`).
  The gate reads each decision's kind from it (`Heeding::policy_verdict`),
  and a line going on in a busy room (`LAMP_GOING_ON_ASKS`, #1969) reads
  the same decisions for its own verdict (`Heeding::policy_now`). So the
  route has one way to read the Policy's decision by request.
- **The turn's facts come from the route's own rule** (`LAMP_ATTENTION`),
  as the line is judged (`Heeding::policy_turn`):
  - the name heard at 0.9 or more;
  - a line someone said and the lamp left unanswered just before;
  - the lamp's words heard to their end within 20 s before the line.

  Without the rule, or without the bus, the route holds nothing, and says
  so once at its start. Live's `busy_talk` comes with going-on asks,
  which this route has not. Its rule already holds what a line let out
  only as the conversation going on would set on the schedule
  (`Heeding::holds_call`), before the gate is asked.
- **The wait is in the turn's own task.** The call looks for its decision
  every 20 ms (`policy::LOOK_EVERY`) for at most 2 s, and nothing else
  waits. A round's calls are put to the gate together, so the round waits
  2 s at most however many it holds, as Live parks a round's calls at once.
  Before this, three actions with the Policy down took 6.04 s, one wait
  after another (the peer read on #1984,
  `a_rounds_held_calls_wait_for_the_policy_together`). A newer turn ends the task, and with it the call, which is not
  run (`its turn ended while it waited for the Policy's decision; not
  run`). Calls the route answers itself (the web search's, `more_functions`)
  and the lamp's own turns are never held.
- **Held:** the model is told `HELD`, the round's other calls run, and the
  turn ends there in silence, as `not_for_me` ends it. The model is not
  asked again, so it is never told `HELD_AGAIN`, and the #1465 loop cannot
  happen here.
- **What was said before a held call may be out already.** The cascade
  streams the model's words as they come, and Live lets none of a held
  turn's sound out. The first builder took that trade on #1712: keeping
  every turn's words back until its calls are known would make every
  turn later. The journal counts them: `request N: W words were let out
  before a held call; the turn ends here, and nothing more is said`. If
  the battery shows such words on held turns, the fix is to hold the
  words back only once a call the Policy holds is in the round.
- **What the wait costs.** It is measured from the model's call to its run
  on the route's harness (`second_provider::tests::policy::measure_the_wait_from_the_call_to_its_run`,
  run with `--ignored`; Mac, debug build, the decision on a real bus):
  - with the turn's decision already there, 40 calls: median 0.1 ms, p90
    0.1 ms, most 0.4 ms;
  - with none, the cap, 10 calls: median 2,012.5 ms, p90 2,019.9 ms,
    most 2,022.7 ms.

  Without the switch nothing waits, and the 0 ms line stays the
  battery's. A decision that comes during the wait is taken at the next
  look, at most 20 ms later. Speech is never delayed: only the held
  call's round waits. Not measured on a lamp: how often the decision is
  there before the model's call.
- **Tests:**
  - `second_provider::policy::tests`: the gate on a bus of the test's
    own; `stay_quiet` held, `answer` and `look_up` run, no decision run
    after 2 s, another turn's never counted, the name run at once, a reply
    to the lamp with and without `LAMP_FOLLOWS_RUN`, and the route's own
    calls never held;
  - `second_provider::tests::policy`: through the engine. It covers the
    turn's numbers, a held call ending the turn in silence with its
    round's other call run, a call let run, and the lamp's own turn never
    put to it. As `serve` builds it, a held `light_on` never reaches the
    executive, and a taken one does;
  - `second_provider::worker::tests::the_policy_is_told_the_name_a_line_left_unanswered_and_the_lamp_speaking_lately`;
  - `the_policy_holds_this_routes_actions_only_with_the_bus_and_the_rule`;
  - `the_lamps_actions_and_a_line_going_on_read_one_reader_of_the_policy`;
  - `policy_holds::tests::why_a_call_ran_and_a_reply_to_the_lamp_are_one_for_both_routes`.

**Needs the Policy's side:** `lamp-policy --act attention`, which publishes
its attention decisions with `shadow: false` ([policy.md](policy.md#acting-on-the-lamps-actions---act-attention-800)).
Without it every call the switch holds waits 2 s and runs (one in a turn
where the lamp's name was heard runs at once, as always), so the switch is
on only with it. On lamp-4ace both are on since 16 October 07:25 (the first builder,
`holds.sh` and `twopeople829.sh`).

## The lamp speaks when spoken to (#829, step 2)

Off unless `LAMP_SPOKEN_TO=on` in the voice's environment (built by the
fourth builder on 16 October 2026, from the first builder's judgement on
#829 at 22:13 on 15 October; the coordinator passes the worker
`--spoken-to`, right after `--name-in-passing-ignored`). `day-and-night`
is the switch with the day's own bars as well (below; the worker's
`--spoken-to-by-day`, right after `--spoken-to`).

**On in the product since 17 October (#828, #1378).** With
`LAMP_ATTENTION=busy-near` and no face seen (the camera saw the floor),
the battery that evening on `460a1a1aa` answered 6 of 12 day lines without
the switch and 11 of 12 with it. Without it a busy room holds every line
nobody calls the lamp in, its name heard too, and a held line gets no
answer, so no conversation begins again. "How is that task going?", 46.8 s
after the lamp's own task news began (35.2 s after it ended), just past the
45 s a conversation lasts, was held, and so was everything after it. With
the switch it asks about the lamp's work (`ABOUT_ITS_WORK`), its answer
makes the next lines a conversation going on, and the name counts with no
face. In a busy room a reply to the news is taken only so, or as any other
line is: taken on timing alone (`AFTER_NEWS`), two colleagues talking 45 to
180 s after the news were answered every line (the Second on #1378). Its
cost: a reply that says nothing of the work, past the news's own 45 s in a
busy room, is held, as without the switch. Outside a busy room the first
line after the news is still a reply to it. Tests:
`with_the_switch_the_battery_after_the_lamps_news_does_not_go_deaf` (the
battery's four lines at their times: answered with the switch, held without
it), `with_the_switch_five_follow_ups_after_the_news_are_answered_and_talk_is_held`
(five follow-ups 17 s apart, the first asking about the task 100 s after
the news, all answered with the switch and held without it; a line calling
someone else by name inside the conversation and one a minute after it
held; within the news's own 45 s the five answered either way) and
`two_people_talking_soon_after_the_news_in_a_busy_room_are_held` (the
Second's colleagues 120 s after the news, held with the switch and without;
within 45 s a conversation going on either way; a reply about nothing held
in a busy room and taken in a quiet one).

Without it the rule holds answers only in a busy room. In a room not known
to be busy, and in a session it trusts, every answer is played, whatever
was said and to whom. On lamp-4ace at 02:16 on 16 October, "Did you get my
message about the budget?", said by one person to another with no name
and nobody at the lamp, was answered 11 times in 12. At 14:49 on 15
October, a word overheard in the office's talk turned the lamp's volume up
for 11 hours.

With the switch, in every room, an answer is played only when something
shows the lamp was spoken to:
- its name: in the text (`called`, or `called_by_name` with
  `LAMP_NAME_IN_PASSING=ignored`), or heard by `lamp-name` in the request's
  sound;
- a face turned to the lamp (the eyes are read for every answer, not only
  in a busy room);
- a conversation with the lamp going on (`going_on`, as above);
- someone close to the lamp and turned to it spoke (`LAMP_ADDRESSED=on`);
- the lamp began the turn itself.

Otherwise it is held, for the reason "nobody spoke to the lamp: no name,
nobody turned to it, no conversation with it going on", and so is every
lamp action its answer calls for: the call is answered and never reaches
the executive, as in a busy room. A short sound let through in a quiet
room (#800, above) keeps its own rule.

The first builder's day scene (#829, 02:39 on 16 October) adds three
things, under the same switch. Played straight after the two-person
scenes, one person alone was answered while each line came within 45 s
of the lamp's last words. "How is that task going?" came 45.3 s after the
lamp's news that its task was done, and was held. From then on nothing
was answered: each held line counted as more talk, so the room stayed
busy.
- **Only talk between people makes a room busy** (`room_told.rs`). A
  request whose answer the rule held is not talk, as a sound is not. Busy
  ends 3 minutes after the last thing left unanswered that counts,
  whatever one person says meanwhile, instead of only after 5 minutes with
  nothing heard. A room not yet known, as after the voice starts, is quiet
  after 5 minutes with no talk between people (the first builder at 05:46:
  with one person talking to the lamp from the start, `room/state` said
  "unknown" all night). Whether two voices are speaking is not known: that
  needs the same-voice part (#865) on the lamp.
- **The lamp's own news is replied to.** The first thing anyone says
  within 3 minutes of a turn the lamp began itself (a task's news, a
  reminder) is played, once, in a room not known to be busy. In a busy
  room only a reply that asks about its work (next) is (#1378). Its
  answers keep their 45 s.
  The 3 minutes start when the turn's first sound is handed on to be
  played, owner-checked and with output credit. An own turn that ends
  silent, is cancelled before it speaks, or whose sound a cancellation
  drops while it waits for credit, opens none (the reviewer on #918 at
  05:11 and 06:17).
- **A question about the lamp's own work is for it.** "task", "reminder",
  "timer" or "alarm" in what was heard, within 10 minutes of the lamp's
  news or of it starting a task, a timer or a reminder, is played in
  every room.

One more, from 03:28 that night (#829, #908): in a busy room, "Hey Lamp,
turn your volume down a little." was held, because the name was heard in
the sound and nobody was turned to the lamp. With the switch, a request
with the name heard in its sound is played in a busy room too, without a
face: nobody faces the lamp at night. With `on` that holds by day too.

**The day's own bars, `LAMP_SPOKEN_TO=day-and-night`:** the switch, and
by day the name needs more (the next two paragraphs). It is its own value
because no lamp has played it by day, and by day it lets out fewer desk
calls than `on`, or than `busy-near` without the switch (the reviewer on
#1158). Its lamp check is a weekday in the busy office with the owner's
"Lamp, …" calls. Night is 21:00 to 4:59 where the lamp stands, as it
tells the time (`local_time::night_here`), read once a request as its
sound ends, off the way to its first output, and only with
`day-and-night`. A zone
that can't be read is day, and so is any time while the clock is known not
to be set (`LAMP_KERNEL_CLOCK=on`, no time client yet): the lamp has no
battery clock and shows the last time it saved, so one switched off at
22:00 and on at 09:00 where NTP is blocked would read night in the
morning (the reviewer on #1083; Expression does the same, `offline.md`).
The voice passes `LAMP_KERNEL_CLOCK` on to its provider and second
workers, which start with a cleared environment; it never reached them
before (Ten on #1158). The hours are the same for every lamp: they fit
an office, and a home desk at 22:00 can be someone working late, so they
are to be looked at again with home use.

By day that cost too much (the first builder at 15:23 on 16 October). In
the busy office the name heard alone let the lamp answer office talk three
times in 25 minutes: 0.932 (talk in Vietnamese), 0.933 and 0.997 (both
heard as Spanish). `lamp-name` had heard its name 227 times since 07:00,
at a median of 0.998. So by day, in a busy room with nobody turned to the
lamp, the name alone counts with `day-and-night` only heard surely
(`SURE_SCORE`, 0.999), close to the lamp or not (the next paragraph).
With a face turned, in a room not known to be busy, or at night, a
hearing at `lamp-name`'s 0.9 floor still counts. With `on`, all seven of
that day's answers are let out, the three to office talk too: no score
separates them (0.996 for a call, 0.997 for talk).

Close is not enough by day either (the first builder at 17:09 on 16
October). Request 169 on lamp-4ace at 17:08 played on the near rule
(`NAMED_NEAR`: 0.95 or more and the room microphone 18 dB up), and the
lamp answered colleagues talking about eye drops beside it. So with
`day-and-night`, by day, a name heard close to the lamp also needs a face
turned to it or its own words calling it by name (`called_by_name`); both
of those let an answer out anyway, so by day the near rule no longer
counts on its own. The voice journals no transcripts, so "its words" is
read as the lamp's name said to it in them. At night, with `on`, or
without the switch, the near rule stands as it was. By day with
`day-and-night` all four of that day's played calls are held as they
were let out: their words did not carry the name, nobody was turned to
the lamp, and none was heard at 0.999 (the reviewer on #1158). Each plays
with a face turned or the name in its words.

**With the switch, the 18 dB bar decides no named answer.** With `on` the
name at the 0.9 floor plays anyway. With `day-and-night`, by day close
needs what plays on its own, and at night the 0.9 floor plays (the
reviewer's probe on #1158, 720 cases: close or not never changed a
verdict). The bar still matters without the switch, for short sounds, and
for a bare call (above; not by day with `day-and-night`). So a weekday of
rises sets it for those only. It isn't set from the lamp's own rises yet:
the journal held two busy-room rises from 16 October 00:00 to 17 October
07:10 (22.2 and 21.9 dB, requests 13 and 14, both "nobody is turned"), and
nothing says whose they were. Both sit above 18 dB. The bar stands as it
was, from the room microphone's rises in [ears.md](ears.md).

And one from 04:58 (#829, 05:02): in the two-person scene, Samantha asked
"Daniel, are you coming to the meeting at two?" and Daniel answered her
"Yes, give me five minutes." He was in a conversation with the lamp, so his
reply counted as a follow-up, and the lamp set a five-minute timer. With
the switch, a line that follows, within 10 s (`ANOTHER_REPLY_US`), one that
called someone other than the lamp by name is that person's reply. That
means a capitalised name set off by a comma, opening the line ("Daniel,
...", "Hey Samantha, ...") or closing it ("..., Daniel?"), that isn't the
lamp's name, a word the service writes for it, or a word like "Okay," or
"Well," (`addresses_someone_else`). For that line only the lamp's name
counts: not the conversation, the lamp's news, a question about its work,
or a face, since the person replying is often facing the lamp's way. A
short sound let through and checked (#800) is held the same way: a short
word, said close, or with a face turned to the lamp, is still the other
person's reply (the reviewer on #918 at 06:06).

The same scene at 15:39 on lamp-4ace (the first builder at 15:40, #829):
after Samantha's "Daniel, are you coming to the meeting at two?", Daniel's
"Yes, give me five minutes." set a timer again, and the lamp, which had
just told him a story, said "Got it. Five minutes, starting now." Two more
cases are now that person's, with the switch:
- **A line that itself calls someone else by name with a question to
  them** (`addresses_someone_else` on its own text, and a "?" in it,
  `TO_ANOTHER`), in every room. The call and its reply can come as one
  request, and the line before then calls no one. A request to the lamp
  that opens with a word and a comma ("Tomorrow, remind me to call the
  bank.", "Seriously, what time is it?") is the lamp's: times and sentence
  words like these are among the words that are no one's name (`NOT_NAMES`,
  Second on #1090), and a line with no question puts none to anyone.
- **In a busy room, a line that follows a question the lamp left
  unanswered** (`AFTER_QUESTION`): within the same 10 s of a line heard as
  a question (ending "?") whose answer was held or never came. Named or
  not, the question was for someone else, and the reply is to them. A
  question the lamp answered leaves the next line alone, and so does a
  quiet room.
The lamp's name still lets the answer out in both. Any timer or reminder
it would set is held with it, before the Policy's hold (#1054) is asked.

And one from 06:07 (#918, the first builder at 06:15): in the two-person
scene, Daniel asked "Hey Lamp, how much is one hundred dollars in euros?"
and was answered. Samantha and he then said a line each to each other,
which the lamp left alone, and the next scene opened with his "Did you get
my message about the budget?", to her. The lamp answered it ("I don't
recall that message") in 3 of 4 scenes, as the conversation going on: it
came inside 45 s of the lamp's answer. It was the same voice, so a
same-voice check would not have held it. With the switch, once two things
said since the lamp last spoke were left unanswered as talk
(`TALK_ENDS_CONVERSATION`, `RoomTold::talked_past_the_lamp`), no
conversation with the lamp is going on, however lately it spoke: the
people have gone on talking to each other. A sound, or a request the
rule held, is not such talk, and a request is counted once. One line left
alone is not enough: a person says "okay" or thinks aloud between their
questions. The lamp speaking again, an answer or its news, begins the
count afresh, and so does something done for a line, a light or a timer,
with no word said (the sixth on #1029). That is a call that is run: a
lookup, the time or a memory, is not, and nor is an action the Policy
held. The name, a face turned to
the lamp, or close and turned to it, still let an answer out. Not yet
played.

The first line of that talk, Samantha's "Anyway, …", came before any line
was left alone, and rode on Daniel's conversation. With a voice model
(`LAMP_SAME_VOICE_MODEL`, [same-voice.md](same-voice.md), "In the voice"),
a line of 1.5 s or more, said with the lamp quiet, in a voice plainly not
the conversation's, is no part of it. A line held as another voice ends
the conversation: whoever speaks next needs the name or a face, until the
lamp speaks again (the battery's scene 3, 17 October; same-voice.md). The
voice is judged at the first output, so a line the model leaves alone is
never judged, and counts as one line left alone, as before.

Beyond the reply to the lamp's news, the question about its work and the
name without a face (with `day-and-night`, at night only), the switch only
holds: what the rule holds without it stays held.

What it would have cost (lamp-4ace's first-output records of 15 October,
13:01 to 16:07, 69 decisions; [the evidence](verification/spoken-to-20261016/README.md)):
4 of the 25 answers that played had none of those signs. Two were one-word
sounds in a room not known to be busy. Two were the rest of a call the
lamp had cut at its comma (#782); with `LAMP_CUT_KEEPS_NAME=on` the rest
keeps the call's text and is played.

**Its cost: "Lamp, ..." from someone not facing the lamp.** The service
seldom writes the name. Of the calls played to the lamps from 12 to 16
October, the text of what was heard showed the call for 84 of 769
"Lamp, ..." and 242 of 1,415 "Hey Lamp, ..." (most often it begins with
the words after the name). `lamp-name` hears "Hey Lamp, ..." (12 of 12
with the lamp quiet, #828), but not "Lamp," alone, which it was not
trained on. So with the switch, "Lamp, ..." said with nobody turned to the
lamp, and no conversation going on, is held about nine times in ten.
Facing the lamp, or "Hey Lamp", is answered.

Tests:
- `with_the_switch_only_what_was_said_to_the_lamp_is_played_in_every_room`
  and `with_the_switch_the_name_needs_no_face_and_the_lamps_own_turn_plays`
  (the rule);
- the day's own bars: `with_the_days_bars_in_a_busy_room_by_day_the_name_alone_counts_only_heard_surely`
  (the seven of 15:23, with `on` too),
  `with_the_days_bars_a_name_heard_close_by_day_also_needs_a_face_or_the_words_calling_it`
  (request 169's shape), `with_the_days_bars_a_bare_call_close_by_day_needs_its_words_or_a_sure_hearing`,
  and `local_time`'s `night_is_from_nine_in_the_evening_to_five_in_the_morning_where_the_lamp_stands`;
- `what_nobody_said_to_the_lamp_is_held_with_the_switch_and_played_without`,
  `the_lamp_called_by_name_or_in_a_conversation_is_answered_with_the_switch`,
  `a_turn_nobody_said_to_the_lamp_never_changes_the_lamp` and
  `the_same_turn_with_the_lamps_name_changes_the_lamp` (the worker's own
  loop: volume, mute and light);
- `with_the_switch_a_reply_to_someone_called_by_name_is_not_the_lamps` and
  `someone_called_by_name_is_told_from_the_lamp_and_from_words_that_open_a_sentence`
  (the reply of 04:58);
- `an_own_turn_that_said_nothing_is_no_news_to_reply_to`, and through the
  worker's own loop `news_the_lamp_spoke_is_replied_to`,
  `an_own_turn_completed_silent_is_no_news`,
  `an_own_turn_cancelled_before_it_spoke_is_no_news`,
  `news_whose_sound_waited_for_credit_and_then_went_is_replied_to` and
  `news_whose_sound_a_cancellation_dropped_before_it_went_is_no_news`;
- `with_the_switch_the_lamps_news_is_replied_to_and_its_work_asked_about`
  and `with_the_switch_a_held_request_is_not_talk_and_busy_ends_after_the_last_talk`
  (the day scene, in the rule and in the room);
- `with_the_switch_two_lines_left_alone_after_the_lamp_spoke_end_its_conversation`
  (the scene of 06:07, line by line, in the room);
- the flag's place among the worker's flags (`addressed.rs`, and
  `both_workers_take_the_spoken_to_flag_in_its_place`).

The second voice route takes the flag and, having no attention rule, does
not apply it, nor its night and day. Reading the eyes for every answer adds the read of the
newest `humans/faces/tracked` message to each decision; its time is in the
record's `sampled_start_us` and `sampled_end_us`, and the record's
`facing` is now filled in every room.

## Cut-ins are not talk (#828)

Off unless `LAMP_CUT_INS_NOT_TALK=on` in the voice's environment (the
worker's `--cut-ins-not-talk`, right after `--room-facts`; #828, the first
builder at 09:27 and 10:14 on 17 October 2026).

On lamp-4ace at 09:21 the first builder played ten cut-ins over the lamp's
stories, one person and the lamp's own voice in the room.
- Requests 21 to 23 were held as "a busy room".
- "Hold on, I have a question. What day comes after Monday?", heard whole,
  got no answer.
- **The busy room came from the room's own count** (`room_told`): three
  things left unanswered inside a minute. Each cut-in the lamp left without
  a word counted: a stop it obeyed, or one that took no yield.
- **The room microphone's rise played no part:** the "21.9 dB over its
  quiet" at the end of those lines is only logged.

With the switch:
- **A line that began while the lamp's own answer could be heard** is
  marked: its reply had sound or was playing (the Start's
  `over_the_lamps_voice`). A reply only in hand, the lamp silent while it
  waits for an answer, doesn't count, so two people talking near a silent
  lamp count as before (the reviewer on #1291).
- **Left unanswered, a marked line isn't counted** (`RoomTold::began_over_the_lamp`,
  `left_unanswered`, which says what each line was). The journal says so:
  "request N, said over the lamp's reply, got no answer in M ms; … not
  counted toward a busy room".
- **A cut-in heard as two or three requests is kept out whole.** Each part
  keeps its own mark until it's closed, whichever way it closes.
- **A marked line still keeps a busy room busy**, so people talking over
  the lamp's answers in a busy office don't turn it quiet. It can't make a
  room busy.
- **A line said after the lamp finished still counts,** and so does
  everything else that made a room busy before.
- **What it costs:** in an office, people talking over the lamp's answer
  don't start a busy room any more. That room is known as busy later, or
  not at all, if nothing else is left unanswered.
- **The lamp check:** the first builder's ten cut-ins with the switch on
  and off, and a busy office where people talk over the lamp.

Tests: `room_told::tests::with_the_switch_cut_ins_left_unanswered_do_not_make_the_room_busy`
(three silent cut-ins in a minute, then the "Hold on…" question, with and
without the switch), `with_the_switch_a_split_cut_in_is_kept_out_whole_and_keeps_a_busy_room_busy`,
`the_cut_ins_switch_is_on_only_and_its_flag_follows_the_room_facts_one`,
`coordinator::tests::turn_taking::a_request_over_a_reply_with_sound_says_so_in_its_start`
(the Start's bit through `commit_admitted_input`),
and `provider_worker::tests::a_request_begun_over_the_lamps_voice_is_kept_out_of_the_busy_count_with_the_switch`
(the Start's mark reaching the room, a reply only in hand not marking, and
the flag's place for both workers).

## The room after a restart (#1465)

Off unless `LAMP_ROOM_FILE` names a file (the worker's `--room-file PATH`,
right after `--cut-ins-not-talk`), as the thread's `LAMP_THREAD_FILE` does.

**Why:** the room is judged in the provider worker's process
(`room_told.rs`) and starts unknown in each new one. Quiet is known only
after five minutes with nothing heard, and busy only after a run of talk.
On lamp-4ace on 18 October at 10:57, a minute after the voice had been
restarted for an install, "Thanks. Now turn yourself back up a little,
please.", with no name, came 13 s after a named ask the lamp had answered.
The Policy read the room as `unknown` and held its `volume_up` (#1465).

**What it does:**
- The room and its clocks go to the file within a second of changing, and
  every 30 s besides: the room, when quiet began to be watched, when
  watching began, the last talk and the last sound, on the shared
  monotonic clock, with the machine's boot
  (`/proc/sys/kernel/random/boot_id`, as `lamp-schedule` reads it). Never
  the voice's session id, which is drawn afresh for each session: keyed on
  that, no worker would ever take back the room of the one before (the
  reviewer on #1487). Where the boot can't be read, no room is kept, with
  one line. Whole or not at all, owner-only, as the thread's file. Nothing
  about requests goes: those end with the worker that heard them.
- At the worker's start, a file from this boot written within the last
  five minutes is taken back. A quiet room is then quiet at once, and a
  busy one ends three minutes after its last talk, as it would have. The
  time no voice was listening counts as nothing heard, except for a room
  not yet known: of that gap only the first 30 s count toward its five
  quiet minutes, in case a meeting began in a longer one (the reviewer on
  #1487). An older file, or
  one from another boot, is not taken: the room may have changed in ways
  no voice heard.
- The journal says what was taken ("the room as it was 41 s ago: quiet"),
  or why not ("the room's file is not taken: it is 412 s old"), or that
  the file couldn't be written, once.
- `room/state` publishes the room taken back on the worker's first tick,
  and a new session is told it as before.

**What it costs:** a room taken back may be wrong if it changed while the
voice was down. People who began to talk in those seconds are counted as
before from the next thing left unanswered. Only a restart under five
minutes is taken back.

**Not shown:** on a lamp. The first builder's check is a deliberate voice
restart in a quiet room, then the 10:57 follow-up: its `context.room`
should read `quiet` (with Eighth's #1474 for the `volume_up` itself).

Tests: `room_told::tests::a_room_kept_by_the_voice_before_is_taken_back_if_from_this_boot_and_under_five_minutes_old`,
`the_room_goes_to_its_file_and_comes_back_from_it`,
`two_workers_in_one_boot_share_the_room_under_the_machines_boot`,
`a_room_not_yet_known_counts_only_the_first_of_a_long_gap` and
`the_room_files_flag_takes_a_short_absolute_path_after_the_switches`.

## A remark to the room is not a reply (#829, scene 3)

Off unless `LAMP_REMARKS=on` in the voice's environment (the worker's
`--remarks`, right after `--provider-patience`; the first builder and
Fourth on #705, 18 October 2026, 13:0x). Only with `LAMP_ATTENTION`, and
on the Live route only: the second voice's rule has no conversation rule
to narrow.

In a busy room the rule lets out whatever is said while a conversation
with the lamp is going on, whoever says it. In the battery's scene 3 on
lamp-4ace on the night of 17 October, "I should bring an umbrella
then.", said by one person to the other seconds after the lamp's weather
answer, was answered 10 times in 10, and "Okay, I will book the car for
eight." once. A person at the next desk does not answer a remark said
across the room.

With the switch, a line is held as `a remark to the room after the lamp's
answer, in a busy room, asking nothing of the lamp` when all of these hold
(`remarks.rs`, `Attention::first_output_busy`):

- the room is busy, and it was let out only as `a conversation with the
  lamp is going on`: not its name heard (or heard surely), not the text
  calling it, nobody turned to the lamp, nobody close to it, not said to
  it from close;
- it does not answer the lamp's own question (`lamp_asked`);
- **it looks like a remark** (`remarks::remark`): past "okay", "yes, and",
  "so" and the like it begins with a person who does something ("I",
  "we", "she", "they", "I'll", "we're"…), it has four words or more, and
  it neither wants nor needs anything: a word that begins "need", "want",
  "lik", "wonder", "wish", "hop", "lov" or "prefer" ("she needs", "I was
  wondering", "I'd love"), or "too", "rather", "use", "wanna" or "missed";
  nor says the person could not take something in ("I didn't catch that",
  "I can't read in this light", "I'm not sure I follow": "can't", "didn't",
  "don't", "not"… within three words before "hear", "see", "catch",
  "understand", "read", "follow" or "get");
- and it asks nothing (`remarks::asks`): no `?`; no sentence that begins
  with a question word or a helping verb, contractions included ("what's",
  "whats", "how's": Gemini's transcript often drops the `?`), or with a
  word that tells someone to do something ("remember", "tell", "turn",
  "let's", "don't"…); no "you", "your", "lamp", "please" or "thanks";
- every letter in it is plain ASCII (`remarks::english_letters`). A line in
  another language has none of these cues, Vietnamese among them for its
  marks, so it is never held here (Fourth, 13:01).

A line is held for what it is, never for what it lacks (the reviewer on
#1499): "Warmer.", "And tomorrow", "Lights off", "What's the weather
tomorrow", "I need more light", "It's too dark in here" and "I'd like a
poem about the sea" all play, and so does a bare answer ("Yes go ahead",
"No, not now.", "Sure"), whatever the lamp asked.

**Two remarks held end the conversation (#1522).** A line held as a
remark is talk between people, not a line the lamp's own rule held: left
unanswered it keeps a busy room busy, as talk does (busy ends three minutes
after the last), and counts toward the two that end a conversation with
`LAMP_SPOKEN_TO` (`RoomTold::judged`, `talked_past_the_lamp`). On lamp-4ace
on 18 October at 14:44, on `3f4f3c5c3`, the lamp answered every line of
the two people's talk in scene 3, "Okay, I will book the car for eight."
with the time, and then spoke their words back in scene 4's opening, 6 s
on: each line was let out as `a conversation with the lamp is going on`,
re-armed by the lamp's own reply 2 to 7 s before. With the switch, the
call and "And what about tomorrow?" are answered, the umbrella and "Yes,
and we should leave a bit early." are held as remarks and end the
conversation, and the car line and scene 4's lines ("Did you see the
report from the client?", "I know.", "We need to cut something before
Friday.") are held as a busy room's talk, until the name, a face or the
lamp's own question shows the next line is for it. It needs
`LAMP_SPOKEN_TO`: the product's unit sets it; lamp-4ace's repo unit
does not, and lamp-4ace has it on the lamp itself (the first builder on
#829, 18 October, 11:0x), so a battery with `LAMP_REMARKS` needs it checked
there first. Test:
`attention::tests::with_remarks_two_remarks_held_end_a_conversation_its_own_replies_kept_going`,
with the switch off as on the lamp (all nine answered).

**The lamp's question, known by its end.** With the switch, the lamp
asked also when the end of its newest line before the request, as it was
said, has a `?` anywhere, or its last sentence asks without one: a
helping verb and a person ("Shall I start it", "Would you like…", "Is
there anything else"), a question word with a helping verb within four
words after it ("Which one would you like", "What time should I set it
for"; "is" or "are" within two, so not "How nice it is to see you"),
"how about", "or would", "Want me to…", or "let me know"
(`Thread::lamp_asked_with`, `remarks::lamp_asked`). Not "Will do." or
"Should be sunny tomorrow." Known misses: "is" or "are" said third or
later as the only helping verb ("Which of these is better", "What kind of
tea is it"), the price of keeping "How nice it is to see you" out; they
matter only when the transcript drops the `?`. The
thread keeps only the first 400 bytes of what the lamp said, so a
read-back "I can ask Claude Code to … Shall I start it?" longer than that
lost its `?`; and Gemini's transcript of the lamp's words often has none.
On the night of 17 October "No, not now." and "No thanks, not now.",
answers to "Shall I start it?", were judged with `lamp_asked` unset six
times. Each exchange now keeps the last 160 bytes of the lamp's words
beside the first 400; not in the thread's file, which an older build
reads with `deny_unknown_fields` and would refuse after a rollback (#1505),
so after a voice restart a read-back longer than 400 bytes is known to have
asked only once the lamp speaks again. This reaches the rule's other uses of `lamp_asked`
too: a short sound that answers the lamp (`SOUND_ANSWERS`) and the unclear
check (#926). Without the switch `lamp_asked` is as before.

**What it costs.** A remark meant for the lamp that begins with a person
("I really liked that story.") goes unanswered in a busy room unless the
name is said or the person turns to it. And one person who thinks aloud in
two remark-shaped lines ends their own conversation with the lamp: in a
busy room, after "I think I will go for a run later." and "I should bring
some water with me.", both held, their next ask without the name ("What's
the weather tomorrow?", "brighter") is held as a busy room's talk, where
without the two-remarks rule it would play (the reviewer on #1533). One
remark before it ends nothing. A remark in another language is
still answered there. Of the night's 32 lines the first builder's first
filter would have held (on lamp-4ace's first-output records, 17 October
19:00 to 18 October 07:00), the umbrella and the car (11) are held and
the 13 of ours said to the lamp play: "Remember that my favourite colour
is green." (6), the answers to "Shall I start it?" (6), and "write me a
poem about the moon" heard without its name (1). The 8 lines heard in the
office that are none of ours are not judged here.

Tests: `remarks::tests::only_the_remarks_to_the_room_are_remarks` (with
the reviewer's probe on #1499), `remarks::tests::a_bare_answer_is_never_a_remark`,
`remarks::tests::a_line_in_another_language_is_never_a_remark`,
`remarks::tests::the_lamps_question_is_known_by_how_it_ends`,
`attention::tests::with_remarks_a_remark_to_the_room_is_held_and_what_is_said_to_the_lamp_plays`,
`attention::tests::with_remarks_a_remark_outside_a_busy_room_or_named_plays`,
`thread::tests::the_lamps_question_is_known_by_its_end_past_what_is_kept`,
`provider_worker::tests::policy_holds::remarks_is_read_after_provider_patience`.

## A line going on in a busy room waits for the Policy (#1522)

Off unless `LAMP_GOING_ON_ASKS=on` in the voice's environment (the
worker's `--going-on-asks`, right after `--remarks`; the first builder's
decision on #1522, 18 October 2026, 15:14). Only with `LAMP_ATTENTION` and
`LAMP_BUS=on` (without the bus it says so and changes nothing), and on the
Live route only (`going_on_asks.rs`). **It holds anything only with the
Policy deciding for real (`lamp-policy --act attention`).** With the Policy
in shadow, nothing it decides is acted on: a line's shadow decision ends its
wait as it comes, and the line is played as without the switch (`a
conversation with the lamp is going on, and the Policy decided it only in
shadow`; the journal says `the Policy decided it only in shadow, after N
ms`). Before, each such line waited the full 2 s (the reviewer on #1538,
16:21).

On lamp-4ace on 18 October at 14:44, on `3f4f3c5c3`, after Daniel's call
in the battery's scene 3, every line of the two people's talk was let out
as `a conversation with the lamp is going on`, each 2.4 to 6.8 s after the
lamp's reply to the line before. "Okay, I will book the car for eight."
was answered with the time. Then, across the 6 s gap to scene 4, the lamp
spoke the two people's own words back. Each reply re-armed the
conversation, so it never ended. The Policy had the room as busy, and its
own decisions on those seven lines were `stay_quiet` 5, `look_up` 1 and a
`stay_quiet` with `follows_lamp` 0.66 (Samantha's "And what about
tomorrow?"). Over the day (17 October 17:00 to 18 October 08:10 UTC, both
builds), 48 lines were let out as going on, all in a busy room. The Policy
said `look_up` 20, `stay_quiet` 19 and a following `stay_quiet` 9: it would
have held 39 (Fourth's `going_on_fourth.py`, run by the first builder on
#1522 at 15:14).

With the switch (`Attention::waits_for_policy`, `Attention::first_output_busy`):

- **Which lines:** in a busy room, a line whose only reason to be played is
  the conversation going on: only a line whose verdict would come to the
  Policy's arm waits. Not one with its name heard (plainly or close), the
  text calling it, a face turned to the lamp, a line said to it from close
  or close to it (the room microphone's rise, read before the wait), a head
  tap or a bare call just before. Nor a reply to the lamp's own question
  (`lamp_asked`, as the remark arm reads it, also read before the wait):
  "Yes, go ahead." to "Shall I start it?" is the person's answer, and held
  it would leave a hand-over never run (Eighth on #1538). Nor someone
  else's line (a reply to a person called by name, a question put to one,
  a reply to a question the lamp left alone), nor a question about the
  lamp's own work, which the rule holds or plays without the Policy. Those
  play or are held as before, at once. In a room not known to be busy
  nothing changes. With `LAMP_REMARKS` too, a remark to the room (above)
  is held without waiting (the reviewer and Jarvis on #1538: each of these
  had waited first).
- **The Policy's word:** its attention decision on the line, by the line's
  number, made for real (`policy_holds::Verdicts`, the same reader the
  lamp's held actions use). Played on `answer` (`a conversation with the
  lamp is going on, and the Policy took the line as the lamp's`) and on a
  `stay_quiet` whose `follows_lamp` is 0.6 or more (`… and the Policy found
  the line said back to the lamp`), and on `look_up` (`… and the Policy
  turned to whoever spoke`). Held on any other `stay_quiet` (`a busy room: a
  conversation with the lamp is going on, but the Policy judged the line
  not for the lamp`), its calls with it. **The Policy's choice decides**
  (the first builder's rulings on #1538, 16:51, and on #1522, 19:04):
  - **A `stay_quiet` with `follows_lamp` under 0.6 is held**, from 0.18 to
    0.57 as Jarvis's probe had them. The Policy chose silence, and
    `follows_lamp` under `FOLLOWS_FROM` says the line doesn't answer the
    lamp. That is the fault #1522 fixes. The cost is a real follow-up
    scored low: on lamp-4ace on 16 October `follows_lamp` was a median 0.60
    on the 33 lines said to the lamp and 0.29 on the 46 not (`policy_holds`).
    At 0.6, 17 of the 33 said to the lamp get through; at 0.35, 28 of the
    33 do, and so do 17 of the 46 not said to it. Tonight's battery sets
    the bar: if its wanted rows are held, `FOLLOWS_FROM` moves down from
    its own numbers. Until then it stays at 0.6.
  - **A `look_up` plays**, its words and its call (#1522, 19:04, replacing
    16:51's "its speech is held"). Turning without speaking is right for the
    lamp named in passing, not for a question put to it mid-conversation:
    lamp-4ace's 18:44 battery held "What do you see right now?" for one.
  - **No timer from talk.** In a busy room, a reply that follows the lamp
    (a `stay_quiet` at `follows_lamp` 0.6 or more, which the gate for
    actions lets set a timer or a reminder, #1040) sets nothing on the
    schedule unless its words name a timer, a reminder or the lamp
    (`going_on_asks::names_the_schedule`). On lamp-4ace at 18:4x Daniel's
    "Yes, give me five minutes.", to Samantha, set a five-minute timer that
    rang mid-day. Not when the line answers the lamp's own question: "Ten
    minutes." to "For how long should I set it?" sets the timer asked for
    (the reviewer on #1594). Only with the gate for actions on
    (`LAMP_POLICY_HOLDS_ACTIONS`), which holds the call. Known misses, each
    refused only on a following `stay_quiet` in a busy room: asks that name
    no timer, reminder or the lamp ("Wake me at seven.", "Count down five
    minutes.", "Set one for ten.").
  - **Of two decisions on one turn** (its words judged in two pieces), the
    newer one has heard more, so it wins, even where the older was more
    certain.
- **A busy room ends sooner once the lamp is answering someone** (the
  first builder on #1522, 19:08; Eighth on #1594). Once the lamp has
  answered a line since the last talk between people, the busy room ends a
  minute after the later of that talk and the lamp's first answer since it,
  not three minutes after the talk (`RoomTold::end_busy_once_answered`).
  While people go on talking it stays busy, so a call answered amid their
  talk ends nothing, and scene 3 is held as before. A scene that starts
  more than a minute after the talk is still busy at its call, and for a
  minute from its answer: a minute from the talk alone would have ended it
  at a call made 70 s after the talk, and the talk after the call would
  have been answered. On lamp-4ace at 18:51 a day line said to
  the lamp 108 s after two people's talk was still in a busy room, held by
  the Policy. It's not ended on every line the lamp plays: scene 3's named
  call is played first, and the umbrella line would then be answered in a
  room not known to be busy.
  - **Talk the Policy holds is still talk** for the busy room, its run and
    its clock (the reviewer on #1594, 20:53; the first builder's ruling,
    20:58; `RoomTold::judged`): two people talking on
    after a call keep the room busy, each line held as not the lamp's.
    Booked as nothing, their talk let the room end a minute after the call,
    and every line from then on was answered. It ends no conversation,
    though: their next question to the lamp is still asked of the Policy,
    and answered on a following `stay_quiet`.
  - **Never later than without the switch:** a first answer 170 s after
    the talk ends busy at the three minutes, not a minute past them (Jarvis
    on #1594).
  - **A reminder the lamp rings answers no one** (`RoomTold::spoke_unasked`,
    Jarvis): rung into two people's pause, it had ended their busy room a
    minute later.
  - **A restart forgets the first answer:** the room's file keeps the busy
    room and its talk, not the answer, so a voice started again ends busy
    three minutes after the talk, as without the switch, unless it answers
    again (the first builder, 20:58, accepted it).
- **A busy room ends a minute after its talk** (`LAMP_BUSY_ENDS_SOONER=on`,
  the second route only; the first builder on Queue #1722, 04:00). "A room
  is busy while people are talking, not for three minutes after: someone
  who walks up a minute after a chat and says 'Hey Lamp, good morning!' is
  talking to the lamp." With the switch, when nothing since the last talk
  between people was talk or answered (only lines held, `Left::Held`,
  sounds or silence), busy ends a minute after that talk, not three
  (`RoomTold::end_busy_sooner`): two people talk to 36 s, and lines held at
  120, 137 and 159 s are in a quiet room from 96 s, where without it they
  are in a busy one until 216 s. More talk, a line left alone, keeps it busy
  a minute from then. **Their talk held in the busy room is still talk:**
  with the switch a line held only because the room is busy and it wasn't
  called (`BUSY_UNCALLED`, `BUSY_NAMED_UNTURNED`) counts for the busy
  room's clock and run, as a Policy hold as not the lamp's does
  (`RoomTold::judged`; the reviewer's P2 on #2372, the first builder at
  04:36). Without it, two people talking on in the room they made busy
  were held line after line, and busy ended a minute after the run that
  made it, while they talked. Now it stays busy while they talk, and the
  minute runs from their last line. Once the lamp answers, it ends a minute after its
  first answer since the talk, as above: the second route turns on
  `end_busy_once_answered` with the switch, as Live does with
  `LAMP_GOING_ON_ASKS`. The Live route is unchanged. Tests only
  (`room_told`, `second_provider::worker`).
- **When it waits:** the Policy decides as the line's words end, and
  Gemini's first output usually comes later, so the decision is often
  there already and nothing waits. If it is not, the line's first output,
  and everything after it, is kept until the decision comes, for at most
  2 s (`policy_holds::WAIT`). Meanwhile the worker takes nothing more from
  the service, as when its output is full, and still serves its control
  (stop, privacy, retirement) on every tick. With no decision in 2 s the
  line plays as without the switch (`… and no decision of the Policy's came
  in time`): a Policy that is down must not make the lamp deaf in a
  conversation. The journal says each wait (`its answer waits for the
  Policy's decision`) and its end (`the Policy's decision came after N ms`,
  `no decision of the Policy's came in N ms`, or `its turn ended while it
  waited`).
- **What it costs:** none of the day's 48 lines got `answer`, so what it
  lets through, it lets through by `follows_lamp`. A follow-up said to the
  lamp that the Policy doesn't read as said back to it is held (17 of the
  33 lines said to the lamp on 16 October scored 0.6 or more; `policy_holds`).
  A line that
  waits is that much later. **Expected, from lamp-4ace's numbers of 18
  October** (Ten's read on #1538): the Policy's decision lands about 1.25 s
  after the end of the person's speech (median; 1.66 s p90), and a busy
  room's first audio 1.22 s after it (median; 2.49 s p90). So most lines
  wait little or nothing, a slow Jev against a fast reply waits up to about
  0.45 s, and only a Policy that doesn't answer makes a line wait the full
  2 s. The journal's `the Policy's decision came after N ms` lines are held
  against these.
- **Not shown:** none of it has run on a lamp. Its battery is scene 3, the
  scene-4 gap, and the first speaker's own follow-ups (Fourth's A/B scenes
  2 and 5, #705 at 14:58), counted for wanted lines held. Also count the
  journal's `the Policy's decision came after N ms` lines against the
  ~0.45 s tail above, and expect no `no decision of the Policy's came in
  2000 ms` line, which would mean the Policy is down, nor any `only in
  shadow` line, which would mean it runs without `--act attention`. Not
  tested through the worker's loop: the room microphone's rise read before
  the wait (the rigs have no ears); `waits_for_policy` with a line said
  close is.

Tests: `going_on_asks::tests::a_line_going_on_is_played_on_answer_or_following_and_held_otherwise`,
`policy_holds::tests::what_a_decision_says_of_a_line_going_on`,
`attention::tests::with_the_switch_a_line_going_on_in_a_busy_room_is_the_lamps_only_as_the_policy_says`,
`attention::tests::on_scene_three_the_two_halves_of_the_fix_hold_the_same_lines`
(scene 3 and scene 4's opening with `LAMP_REMARKS` alone, this switch alone
with the Policy's decisions of 14:44, and both: the same two lines answered
and seven held each way), and through the worker's own loop with the Policy's decisions on a bus:
`provider_worker::tests::going_on_asks::with_the_switch_a_line_going_on_in_a_busy_room_is_played_only_as_the_policy_says`
(the 14:44 lines with their decisions then: the car line and scene 4's
opener held, "And what about tomorrow?" played at once, a `look_up` played,
the car line played without the switch),
`…::with_the_switch_a_line_going_on_waits_for_a_late_decision_and_is_played_with_none`
(a decision 300 ms after the first output: played only once `answer`
comes, held on `stay_quiet`; none: played after 2 s),
`…::with_the_switch_a_line_that_calls_the_lamp_does_not_wait`,
`…::with_the_switch_a_reply_to_the_lamps_question_does_not_wait_and_is_not_held`
("Yes, go ahead." to "Shall I start it?" played at once on a `stay_quiet`
at `follows_lamp` 0.4, and with no decision at all; after a line that
asked nothing, held),
`…::with_the_switch_a_look_up_plays_and_a_stay_quiet_holds_its_call_too`
("And turn yourself up a little." with `volume_up` first, the gate for
actions off: on `look_up` the call runs and the words after it play; on
`stay_quiet` at 0.4, neither),
`…::with_the_switch_talk_in_a_busy_room_sets_no_timer_unless_it_names_one`
("Yes, give me five minutes." on a following `stay_quiet`: no timer; "And
set a timer for five minutes.": set),
`room_told::tests::with_the_switch_a_busy_room_ends_a_minute_after_the_lamps_first_answer_since_its_talk`,
`attention::tests::with_both_switches_scene_three_after_a_pause_is_still_judged_in_a_busy_room`
(scene 3 after a pause of 20 s and of 70 s, each line in the room
`RoomTold` gives: the call and the follow-up answered, the rest held),
`attention::tests::with_the_switch_two_people_talking_on_after_a_call_keep_their_room_busy`
(the reviewer's probe: two people talk on for nearly two minutes after a
call, each line held by the Policy; every line in a busy room, held, and
both questions to the lamp answered, with `LAMP_REMARKS` or without),
`room_told::tests::with_the_switch_held_talk_keeps_a_room_busy_and_a_reminder_or_a_restart_ends_nothing`,
`provider_worker::tests::reminders::a_reminder_rung_into_a_busy_room_is_no_answer_to_end_it_by`
(through the worker's loop: a reminder's audio in a busy room is no answer,
so the room is busy two minutes after its talk; Jarvis on #1613),
`going_on_asks::tests::only_a_line_naming_a_timer_a_reminder_or_the_lamp_names_the_schedule`,
`…::with_the_switch_a_shadow_decision_ends_the_wait_and_holds_nothing`
(a `stay_quiet` in shadow before the line's answer: played at once; 300 ms
after it: played then, not after 2 s),
`…::with_the_switch_a_remark_held_as_one_does_not_wait_for_the_policy`
(with `LAMP_REMARKS`, the umbrella line held with its turn's end at once
and no decision; without it, played after the 2 s),
`…::with_the_switch_a_line_whose_turn_ends_while_it_waits_is_let_go_at_once`
(cancelled while it waits: let go on the next tick, none of its sound out,
and the next call answered at once) and
`…::going_on_asks_is_read_after_remarks`.

## At a desk, the name or a follow-up (`LAMP_NAME_OR_FOLLOW_UP`)

At the owner's desk on lamp-4ace on 22 October, the owner talked to a
colleague and the lamp kept answering: of its 18 answers, the journal gave 8
to "someone is turned to the lamp", 6 to "a conversation with the lamp is
going on" and 4 to its name. A person at a desk faces the lamp's way while
talking to someone else, and whether a turned face means a line is for the
lamp was never checked with real people (above). The room was never judged
busy either, since the lamp answered everything, so the weak reasons (the
room not known to be busy, a trusted session) went out with the face as
their sign, and each answer's window let the next lines of the talk in. With
`LAMP_NAME_OR_FOLLOW_UP=on` (the worker's `--name-or-follow-up`, right after
`--live-scoped-session`, on both routes), in a busy room a face turned to the
lamp is never a reason on its own and only confirms the name heard in the
sound, and a line is in the conversation going on when it began while the
lamp's own words played or within 8 s of their end
(`name_or_follow_up::FOLLOW_UP_FOR`; `LAMP_FOLLOW_UP_FOR_MS` sets it, from 1
to 15 s, the lead's decision 4 on 23 Oct), however long it then goes on, and
nobody has talked past the lamp since; the thread's 45 s and a busy room's
15 s, read as the line is judged, don't decide there (on lamp-52e6 the same
day, follow-ups begun as the answers played were answered 0 of 3). Later, a
line needs the name, a tap or its words calling the lamp. In a room not known to be busy the rule is as without the
switch, a face a reason and the windows as they were: on lamp-4ace from 17:06
with the busy-room rule everywhere, the owner said at 18:0x that a person
alone with the lamp does not say its name a thousand times a day. So the room
turns busy from the talk the lamp holds: there a line held as said to someone
else by name, or as said to nobody with someone in view and turned away, is
talk even with `LAMP_SPOKEN_TO`, never a lone voice's held lines with nobody
in view (that evening one unseen person's misheard call and the lines after it
turned the room busy, and the scene was lost), and two faces in the
eyes' newest look, or a line in another voice, make it busy at once
(`room_told::RoomTold::count_held_talk`, `two_people`). What the model is told
of the conversation is not changed. Off until a real desk-talk set passes.
Its costs: in a busy room an answer to the lamp's own question begun more than
the follow-up window after its words (8 s unless `LAMP_FOLLOW_UP_FOR_MS` sets
it) needs the name; in a room not known to be busy a person
alone facing the lamp on a phone call is answered line after line, as before
the switch: the lamp holds none of it, and one face makes no busy room. So is
a colleague off camera, with no voice model, while the person faces the lamp,
until a line is held. And a face on a screen or in a photo keeps the room
busy, so a person alone with it needs the name.

## A line said to a person is no call by its start (`LAMP_NAME_SAID_TO_A_PERSON`, #3945)

On c0a4 a real "Ma'am, could you pass me the salt please?", said to a
person, was heard by `lamp-name` at 0.95 to 0.99 at the line's start, and
the lamp answered it: with the product's second sign of the name asked a
hearing at the line's start is its own second sign (second-voice.md). With
`LAMP_NAME_SAID_TO_A_PERSON=on` as well, on the second route, it is not
when the line's first word, set off as a name is (past a greeting, followed
by a comma or a stop, or the whole line), is one said to a person
(`attention::SAID_TO_A_PERSON`): ma'am, madam, sir, mum, dad, honey,
sweetie, babe, dude, bro, buddy, guys, compared lower case with the
apostrophe kept, a curly one read as a straight one. Every other second
sign still lets it count: a face turned to the lamp, said close to it or
to it from close, its words calling it, and, with this switch only and only
for a start it held, a conversation with the lamp going on
(`Signs::going_on`, passed to `Attention::weigh_the_name`); for every other
line a conversation going on is no second sign, as #1722 has it. Held, the line is judged as one with no
name, and the journal says `its first word, set off, is said to a person
("ma'am"), and nothing else shows it was called: not counted
(LAMP_NAME_SAID_TO_A_PERSON)`, with the word from the list and where the
hearing began, never the rest of the line. The lamp's name, "lamb", a word
written for it (`STAND_INS`, `STAND_ALONE_INS`, `HEARD_FOR_THE_NAME`), a
person's name of its shape (`NAMES_OF_ITS_SHAPE`) and a word that isn't
alphabetic are never on the list, so "Lamp, …", "Blake, …" and
"1. How are you today?" (#3923's lines) are answered as before. Its known
costs: "man" and "mom" stay stand-ins, so "Man, …" is still read as a
call by its words before the hearing matters; and a real "Lamp, …"
written by the speech to text as "Ma'am, …" (#3923, name.md) is held
unless a face, a conversation going on, or closeness shows the call. Lamp
check on c0a4 (28 Oct, a person speaking, turned away): the one "Ma'am, …"
lamp-name heard as the name (1.00, begun 0.9 s in) was held, the other
"Ma'am, …" lines had no hearing and were left alone; "Lamp, what time is it in
London?" and "Hey Lamp, are you there?" said to the lamp were answered. Off
unless set.

## Switch

`LAMP_ATTENTION=on` in the voice unit (the worker's `--attention`). Off by
default. On on lamp-4ace since 22:15 on 2026-10-13.

`LAMP_ATTENTION=busy` (the worker's `--attention --attention-busy`): the
rule's busy form, 5 above. On lamp-4ace since 11:52 on 2026-10-14.

`LAMP_ATTENTION=busy-near` (the worker's `--attention --attention-busy
--attention-near`): the busy form, and a name heard at 0.95 or more close
to the lamp counts as a call (above). Needs the ears running. Off by
default; not yet on lamp-4ace.

`LAMP_ADDRESSED=on` (the worker's `--addressed`, right after the attention
flags; nothing without `LAMP_ATTENTION`, and nothing heard, seen or
published without `LAMP_BUS=on`, which lamp-4ace has): each request is judged said to
the lamp from close to it, published on `attention/addressed`, and in a
busy room lets out an answer the rule would hold (above). Off by default;
not on any lamp.

`LAMP_CUT_KEEPS_NAME=on` (in the coordinator's environment; no worker
flag): a request that goes on within a second of the turn the lamp just
ended, or after an end sent early, keeps what the rule knew of it (above). Off by default; not on any lamp.

`LAMP_NAME_NOTE_BUSY=face` (the worker's `--name-note-busy-face`, right
after `--name-note`; nothing without `LAMP_NAME_NOTE=on`): in a busy room
the name note waits for a face turned to the lamp. Off by default. On
lamp-4ace since 11:52 on 2026-10-14.

`LAMP_NAME_NOTE_HEARD_AS=on` (the worker's `--name-note-heard-as`, right
after the name note's flags; nothing without `LAMP_NAME_NOTE=on`): the name
note says the name may have sounded like another word ("even if it sounded
to you like another word, such as "Mom", "man" or "ma'am"",
`called::NOTE_HEARD_AS`). On lamp-4ace on 19 October, Samantha's "Hey Lamp,
tell me a story about a brave little robot…" came back in ten takes as "Hey
man" 4 times, "Hey Mom" twice, "Hey ma'am" once, and with no greeting 3
times. lamp-name heard the name each time, and four "Hey Mom" requests in
one arm went unanswered (#829, the first builder at 04:00 and 05:22): the
model's own ears heard someone else's name. Off by default; not on any
lamp. Tests:
`provider_worker::tests::review_2026_10_14::review_sound_with_the_heard_as_line_the_name_note_says_it_may_sound_like_another_word`
`called::tests::the_heard_as_line_is_passed_only_with_the_note_and_only_for_on`
and `provider_worker::tests::going_on_asks::the_heard_as_line_is_read_after_the_name_note`.

`LAMP_NAME_IN_PASSING=ignored` (the worker's `--name-in-passing-ignored`,
right after `--attention-near`; nothing without `LAMP_ATTENTION`): "the
lamp" spoken of in what was heard is no call (above). Off by default; not
on any lamp.

`LAMP_POLICY_HOLDS_ACTIONS=on` (the worker's `--policy-holds-actions`,
right after `--sounds-checked`; nothing without `LAMP_BUS=on`): the lamp's
actions wait up to 2 s for the Policy's attention decision and are not run
on a `stay_quiet` without the name (above). Off by default; on lamp-4ace
since 16 October; needs `lamp-policy --act attention`.
`LAMP_SPOKEN_TO=on` (the worker's `--spoken-to`, right after
`--name-in-passing-ignored`; nothing without `LAMP_ATTENTION`, and no eyes
or name from the sound without `LAMP_BUS=on`): in every room, an answer is
played only when the lamp was spoken to (above). `day-and-night` adds the
day's own bars (the worker's `--spoken-to-by-day`, right after
`--spoken-to`). Off by default; on in the product unit since 17 October
(#1378), and on lamp-4ace by its drop-in.

`LAMP_GOING_ON_ASKS=on` (the worker's `--going-on-asks`, right after
`--remarks`; nothing without `LAMP_ATTENTION` and `LAMP_BUS=on`, and it
holds nothing while the Policy is in shadow: run it with `lamp-policy
--act attention`): in a busy room, a line let out only as a conversation
going on waits up to 2 s for the Policy's decision and plays on `answer`
or a `stay_quiet` said back to the lamp (above). Off by default; not on any
lamp.

`LAMP_CUT_INS_NOT_TALK=on` (the worker's `--cut-ins-not-talk`, right
after `--room-facts`): a line said while the lamp's answer could be heard,
and left unanswered, is not counted toward a busy room (above). Off by default; not
on any lamp.

`LAMP_ROOM_FILE=/run/lampos-voice/room.json` (the worker's `--room-file
PATH`, right after `--cut-ins-not-talk`): the room is kept across a
restart of under five minutes (above). Off by default; not on any lamp.

`LAMP_VAD_INPUT=linear-gated` (passed to the capture worker only): the
speech detector reads the echo canceller's linear output, which keeps a
"Stop." said over the lamp that the suppressor takes out, but only where
speech would plainly be a person, the room clear or at most 20 % explained
by the render. Elsewhere, the lamp's own voice included, it reads the
processed block, so the lamp alone gives the processed block's false yields
and is not scored as speech for whom the lamp answers, a busy room or a
turn's start, as it was with `linear`. Replayed on the #828 tables it yielded
to 10 of 19 lines said over the lamp, as processed, against 14 with `linear`
(false yields: 2 with this, 1 processed, 12 with `linear`); with
`LAMP_LISTEN_GAPS=on` a gap of the lamp's silence stays on the processed
block ([serve-runtime.md](serve-runtime.md)). Off by default; not on any lamp.

`LAMP_HEAD_TOUCH=on` (the worker's `--head-touch`, right after the name
note's flags; nothing without `LAMP_BUS=on` and Controls run with
`--touch`): a head tap is a call for 2 s (above). Off by default; not on any
lamp.

## Not shown

Host tests: the rule and its bookkeeping (five, with the service's real
renderings of called requests as cases), and the worker with the scripted
provider in a room it is told is busy (one). The second form was played
once each way (the table above), with synthetic voices through a
loudspeaker and nobody in the room: not with people, and not on a day
when the office is full. The host tests give
the rule text with the name in it, which the real service does not
write: they did not catch the fault above, and the played room test
did.
