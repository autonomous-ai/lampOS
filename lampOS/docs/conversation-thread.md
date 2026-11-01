# Keeping the thread across a new session

`crates/voice/src/thread.rs`. Status on 2026-10-13: host tests, and two
renewals on lamp-4ace with the first wording of the note, which the lamp
spoke about (below). The present wording has not been through a renewal on a
lamp yet.

## Why

A session with the provider does not last. Gemini Live announces its own
close and a lost connection is replaced by a new one. On lamp-4ace the close
has been announced 3 minutes 20 seconds, 15 minutes and about 39 minutes
after a session opened, the two timed on 2026-10-13 with five minutes'
notice, so someone talking to the lamp for a quarter of an hour may well
meet one.
On lamp-4ace resumption is off (the relay does not forward it), so each new
session begins knowing nothing: a person in the middle of a conversation
finds the lamp has forgotten their name and what they were talking about.

## What it does

The provider worker keeps the last exchanges the lamp took part in, as the
provider's own transcripts gave them:

- what a person said, when the lamp began to answer within 20 seconds of it;
- what the lamp answered.

At most twelve exchanges (eight before #1693), 300 bytes of the person and 400 of the lamp in
each, with whitespace collapsed. Talk around the lamp that it did not answer
is not kept: it is dropped when the next thing is heard, or when an answer
begins more than 20 seconds later.

With the end sent early (`--early-end-ms`, [faster-answers.md](faster-answers.md),
way 2) an answer's words can arrive before the lamp is sure the person has
finished. If the request is then given up, because they went on or for any
other reason, its exchange is taken out again (`Thread::unsay`): the lamp
never said those words. What the person had said before it waits, as heard,
for the next answer. The worker knows such a request by its authority: its
end was sent while the lamp still said it was listening, which without that
option never happens.

`Recalling`, which replaces the plain dialer in `run_with_continuation`, opens
every session through the same `Dialer` as before. When the thread holds an
exchange from the last ten minutes it first adds this to the end of the
instruction:

```text
A moment ago
This is what you and the person at your desk have just said to each other,
for you to carry on from if they go on with it. It is background only: never
read it out and never answer any of it again.
Person: ...
You: ...
```

That is for a newest exchange no more than 45 seconds old
(`GOING_ON_FOR`). An older one is told as finished:

```text
Earlier
About 7 minutes ago you and a person at your desk said this to each other.
That conversation is finished and they have gone back to their work. It is
background only, for when someone comes back to it: never read it out and
never answer any of it again.
Person: ...
You: ...
```

Until the afternoon of 2026-10-13 there was one wording for both, "Earlier
in this conversation ... for you to carry on from the way anyone carries on
a conversation they are in". The lamp's character by then answered only
when called or when already talking with someone
([character.md](character.md)), and the note told every new session that it
was. In lamp-4ace's record of that afternoon in a talking office, from the
character change at 16:05 to 16:47: 157 utterances ended and 18 were
answered. Eight sessions were opened in that time. The first three answered
nothing they were not asked (one began with an empty thread, two with a
single exchange in it). After someone had come and talked to the lamp at
16:28, each of the next five (16:28, 16:37, 16:40, 16:41, 16:43) answered
the first or second thing it heard, which was colleagues' talk: "por" got
"The weather here is pretty warm and humid today", "la nueva tecnología"
got "It's moving so fast these days, isn't it?", and the next session, told
that exchange, took "la noche" as its continuation: "It is truly amazing
how quickly things are evolving." That last session then answered nine
times in two minutes, five of them "It's past five in the afternoon now."
Each such answer became the thread's newest exchange and was told to the
next session. The thread was cleared by hand and the service restarted at
16:47. The two wordings are in the host tests; what the new one does to a
session's first turn on a lamp is not yet known.

The note says nothing of sessions, connections or memory and does not tell
the lamp to keep quiet, and a test holds it to that. The first wording began
"Your connection was just renewed and you lost your memory of the
conversation you were in" and ended "Do not mention the renewal or this
note, do not repeat or answer any of it again, and if nobody speaks to you,
say nothing". On lamp-4ace on 2026-10-13, in a run of the conversation
checks with two renewals (one announced, one after a write to the provider
timed out), two things followed:

- The lamp opened the next answer after each renewal with "Sorry about
  that, my connection dropped!" and "My connection dropped for a second, but
  I'm all here!". Telling the model what not to mention told it what there
  was to mention.
- In the two runs of two people talking that came after, the lamp answered
  16 times with a spoken "...": it took its turn, showed that it was
  speaking, and said nothing. The question put to it by name and the one
  that follows it got "..." in both runs, where the same check had them
  answered an hour earlier. In the 527 scenarios recorded before those two
  there are three such replies. Without the note the model keeps quiet by
  not answering at all, which the service closes after four seconds as
  unanswered.

An answer with no letter or digit in it is also left out of what is told,
so a "..." cannot come back as something the lamp said.

With nothing said in the last ten minutes the thread adds nothing, so a
lamp left alone starts its next session as it always did. The instruction
may be 16,384 bytes in all (it was 8,192 until the character file grew past
seven thousand and left the thread room for three short exchanges). The
thread takes what the character file, the time of day and, for a session
without some of its functions, the line of what it can't do right now
([character.md](character.md), "What the runtime adds to it") leave of
that, and when it does not all fit, the latest exchanges that do are told
and the older ones are left out. Eight exchanges at their longest and the
note take 5,933–5,985 bytes, twelve about 8,800. With the file lamp-4ace runs
(10,180 bytes on 18 Oct, #1549) the room is 6,036 bytes with a plain time
line (168) and every function, so eight fit at the longest and the four
older are left out; with lamp-4ace's `LAMP_CLOCK=on` time line (222–228
bytes) it is 5,976–5,982, so the eighth may be left out too. Shorter
exchanges fit more of the twelve. The line of what it can't do takes 188 bytes for the light alone,
236 for the light and the volume, and 278 with the reminders too.

## A session renewed on purpose

The provider worker also asks for a new session itself
(`crates/voice/src/provider_worker.rs`, `RENEW_AFTER_UNANSWERED`): when eight
requests in a row have gone unanswered it asks the connection to end at its
next idle moment (`InputSender::renew_when_idle` in `lamp-gemini`, the same
clean boundary as an announced close: an answer in progress finishes
first), at most once in fifteen minutes. The thread above is what makes
this cheap: whatever the lamp and a person said in the last ten minutes
goes into the new session.

Why: a session can stop being of use without failing. On lamp-4ace on
2026-10-13 a session about thirteen minutes old, with people talking
Vietnamese around the lamp, was transcribing English said to it as Spanish
("Hey Lamp, what is two plus two?" as "¿Cuánto es dos más dos?"), and with
the character file of that hour it answered nothing for five minutes
([character.md](character.md)). A new session heard the same question as
it was said. Nothing on the lamp's side can tell a session in that state
from a room that is simply not talking to the lamp, so the rule does not
try to: eight unanswered requests in a row get a fresh session either way.
In the ordinary case that costs about two seconds in which what is said is
not heard and leaves a shorter history for the service to bill on every
turn.

It was at most once in three minutes at first. With the character that
answers only when called, a talking office is nothing but unanswered
requests, and lamp-4ace renewed every three minutes while the office
talked (16:22, 16:25, 16:28, 16:37, 16:40, 16:43). A new session is where the lamp was likeliest
to answer what was not for it (above), so each renewal was a chance to
speak out of turn. The new sessions also heard no better: the first things
three of them were given came back as "por", "pausa" and "la nueva
tecnología". Fifteen minutes leaves the rule as a last resort for a session
gone wrong, which is all the evidence supports.

Checked by a session test beside the one for an announced close (idle: it
ends at once; with an answer in progress: the answer completes first). On
lamp-4ace it has acted about a dozen times with no fault.

## Each person's own thread

Completed exchanges carry their stored owner through the memory handoff. A batch
can contain different people and room exchanges; each retains its own scope.
The person visible when the batch finishes does not replace those stored scopes.
This does not change the attribution or session-switching rules below.

With a memory and eyes that remember faces (#764), each exchange is kept
as whose it was: the room's, or the memory id (never the name) of the
known person the eyes name at the lamp as it is made, or, with no name
fresh, of the one the worker counts there (someone known at once when
nobody was or the one before was last seen 30 s ago, a different known
person once known 10 s with the one before unseen, nobody after 30 s
unseen and two minutes with no exchange; until then what is said is
still the known person's). The counted person decides whose session it
is: for up to those 10 s a newcomer is answered in the session of the one
before, but what they say is theirs, so it is not told to the one before
([memory.md](memory.md#a-session-for-each-person-764-follow-up)). Each keeps their own last twelve, four
threads at most with the room's (48 exchanges), the oldest going first past
that. A session is told only the exchanges of the person it was opened for,
and the worker opens a new session between turns when that person changes
([memory.md](memory.md#a-session-for-each-person-764-follow-up)), so past
those 10 s one person's conversation never reaches another's session, and someone who
comes back within ten minutes is told theirs again. The first session is
for whom the eyes name as the voice starts, read before it opens: opened
for the room, it was renewed for them the moment it was ready: 4
`server_go_away` drops 0 s after a start on lamp-52e6 on 15 October (#705;
`thread::tests::the_first_session_is_for_whom_the_eyes_name_at_start`).
The file keeps whose
each exchange is (`whose`, left out for the room's, so a thread with
nobody known is written as before); an entry whose `whose` is not a memory
id is not read back. Forgetting still drops every thread. With nobody ever
known everything is the room's and nothing here changes
(`thread::tests::with_nobody_ever_known_nothing_changes`).

With `LAMP_CONVERSATION_ROOM_LINES_TOLD=on` (#4339, the second route), a
known person's turn is also given the room's exchanges of the same
conversation, marked as said in the room (`(said in the room)` before the
words, or a system note when there are none): one conversation split line by
line into the person's thread and the room's, on whether the mouth was read
speaking, no longer leaves each answer half of it. The same conversation is
walked back from now over the person's exchanges and the room's, each within
45 s (`GOING_ON_FOR`) of the next; another person's exchange or a longer
quiet ends it, and only the room's from the person's first exchange in it
count. Within the chat's 12 exchanges and 600 s
(`thread::Thread::recent_with_room_lines`,
`thread::tests::room_lines_told_to_a_person_are_their_conversations_only`).
Never the reverse and never another person's; the thread text and the
running note stay the scope's own ([memory.md](memory.md)). Part of the
"said aloud" risk, accepted: a room's line narrowed because another voice
spoke it is told to the person's turn too, marked, since it was said aloud
beside them; it never enters their running note or saved memory.

## A line soon after the lamp's words (`LAMP_FOLLOWS_ON`)

Off unless `LAMP_FOLLOWS_ON=on` in the voice's environment (#829, the
first builder at 09:01 on 17 October 2026).

On lamp-4ace at 08:58, in a quiet room with one person, "Hey Lamp, what is
your favourite season?" was answered. Then "And what do you like about
it?", said 12 s after the answer, was heard right and sent to Gemini, which
left it unanswered ("got no answer in 4009 ms; closed as unanswered"). A
follow-up about 3 s after an answer had been answered.

- **Nothing in the code treated 3 s and 12 s apart.** The model got the
  line's sound and nothing beside it.
  - The name note needs the name heard.
  - The room and session facts are not told while a conversation is going
    on, which is 45 s from the answer's start (`GOING_ON_FOR`, above).
- **The character file leaves it to the model.** It gives a line to the
  lamp only when it is called, or when "you said something to them a moment
  ago, and this follows on from it", and "when you really cannot tell, stay
  silent". At 12 s the model kept quiet. Its reasons aren't in the journal,
  so this part is inferred.

With the switch, the coordinator marks the Start of a line whose speech
began within 20 s (`follows_on::WITHIN_US`) of the lamp's own words
finishing playing (`follows_on: true`, and `"follows_on": true` in the
session trace's `input_admitted`).
- **Not over the lamp:** a line that takes over from the lamp's sound, or
  from a line someone said that is still in hand, is never marked
  (`not_speech::takes_over`). One begun over a sound sent on for the model
  to judge, with the lamp silent, is.
- **A line whose first sound came a moment before the words finished,**
  admitted once the reply was over, counts as begun at once.
- **The time** is the reply's own end in the coordinator, as its last
  sound plays. It isn't when the worker handed the sound on, which runs
  ahead by the queued audio. Only a reply that played to its end starts the
  clock; one cut off never does.
- **After a Stop that cut the lamp's speech** (a hand, the head, or a stop
  word said over it with `LAMP_STOP_WORDS`) the clock is cleared, so
  nothing said next is marked from an earlier answer, with
  `LAMP_STOP_TOLD` or without. With it, the first line after the Stop is
  told the lamp's words are over (`STOPPED_NOTE`) instead.
- **A continuation after an early end** isn't marked again. Its first part
  was told beside its own sound, in the same session.
- **The provider worker** then gives the model `follows_on::NOTE` with the
  line's End, unless the End says it was a sound sent on for the model to
  judge, or the room is busy: "(For you to know, not to answer: you said
  something a moment ago, so what you hear now may follow on from it.)" At
  the Start it can't be known that a line will end as a sound, and a thump
  told it "may follow on" would push the model to answer it.
- **The note says only what the lamp did,** in the character file's own
  words. Whether the line follows on is still the model's to judge, so talk
  that doesn't follow on stays unanswered.
- **The second route** takes the Start and adds no note: this is the Live
  route's only.

Not yet played. The lamp check is the first builder's scene `s-0857` with
the switch on and off.

Tests:
- `follows_on::tests::a_line_begun_within_twenty_seconds_of_the_lamps_words_may_follow_on`:
  12 s, 19 s and 20 s do; 21 s doesn't. Nor does a line over the lamp's
  sound or over a line said, one with the lamp never having spoken, or any
  without the switch. A line over a sound sent on does.
- `coordinator::tests::early_end::with_the_switch_a_question_soon_after_the_answer_is_told_it_may_follow_on`:
  through the coordinator's own loop and the real provider worker, against
  the scripted service. A question about 12 s after the answer's last sound
  is marked and sent with the note; one about 22 s after isn't; without the
  switch neither is.
- `coordinator::tests::turn_taking::a_request_begun_soon_after_the_lamps_words_says_so_in_its_start`:
  the Start and the trace, and no mark after a Stop.
- `provider_worker::tests::a_request_begun_soon_after_the_lamp_spoke_is_told_it_may_follow_on`:
  through the worker's loop. Told with its End in a room not known to be
  busy; not in a busy room, nor without the Start's mark, nor when the End
  is a sound.

Not exercised through the loop: a line begun over a sound sent on (it's
`takes_over`'s case, pinned in the rule's own test), and the clock cleared
at a Stop (the early-end harness has no hand, head or stop word).

## A long conversation's running note (`LAMP_CONVERSATION_SUMMARY`)

A turn on the second route is told only its scope's last few exchanges:
the thread keeps 8 a scope, and the chat is given the last of them from the
last few minutes. On lamp-06c1 on 29 October, 11 exchanges after a quiz's
rules the lamp said there was no quiz, and a budget said 14 exchanges back
was gone. That is #449's defect 4 on V1: the opening turn with an
activity's rules fell out of what the model was shown.

With `LAMP_CONVERSATION_SUMMARY=on` (on in the product unit; the second
route only), `crates/voice/src/conversation_summary.rs` keeps a running
note of each scope's conversation:

- **When.** After a turn's reply and its save to the thread file, never on
  its path. The exchanges older than the scope's newest 4 that are not yet
  in its note are handed to the note's own thread once 3 of them wait (the
  thread's 8 less 4 less 1, so they go before the thread drops the oldest;
  7 with a thread of 12), or the oldest of them is three quarters of the
  thread's 10 minutes old (a slow talk). The numbers are read from
  `thread.rs`, so a wider thread moves them, and so do the tests; a thread
  of 4 or fewer keeps one less word for word, so its oldest is still given
  before it goes. An exchange answered in a held scope (a face's gap, or a
  line carried into a person's conversation, `Thread::held_for`) counts
  toward the batch but is never given: none of it reaches a note or a talk
  note (the reviewer-lead's P2 on #4479). The forget count is read before
  the exchanges are taken, so a forget between the two drops the job
  (Tests' P2 on #4479). The turn's end waits for nothing: the hand-off is a queue of
  2, and a full one drops the job and counts it.
- **How.** The chat's own DeepSeek model and key (as the observation pass
  uses them), 20 s at most, is given the note so far and the exchanges, as
  quoted data, the person's lines as "Person:" and the lamp's as "You:".
  It writes the whole new note: `Current activity:` first (a game, plan or
  task: its rules, where it stands, the score, what is covered, or "none"),
  then `Key facts:` as `- ` lines, oldest first; only what the person said
  or what was agreed, never the lamp's own guesses; no instruction in the
  history followed.
- **Its size.** At most 6,400 bytes, about 1,600 tokens or 1,200 words.
  Over that, the oldest `Key facts:` lines go first, then the oldest `- `
  line anywhere; the end is never cut.
- **Whose.** One note a scope, a person's memory id or the room's, in the
  worker's memory only: never the thread file, the disk or the store. A
  turn is told only its own scope's, the same key it is told the thread's
  lines by; a person's never reaches the room's turns or another person's.
  A line nobody placed is the room's, and is told the room's note, as it is
  told the room's thread lines.
- **How it is told.** In the system message, before the thread's lines,
  within the instruction's room, under the heading "Earlier in this
  conversation" and the line "Your own running note of what came before the
  lines below. It is background only, for keeping track: never read it out,
  recite it or answer any of it again." A withheld owner name is taken out
  of it as out of the thread.
- **How long.** Gone after the thread's 10 minutes with no exchange of its
  scope; a scope still talking keeps it fresh. Any `forget` or
  `forget_everything` asked, the thread's own forget, and a forget from the
  app told by memory on `memory/forgot` (a person's, the room's or every
  person's, read every half second off any turn's path,
  `conversation_summary::memory_forgot`) drop the thread and every note, and
  a note being made when one came is never kept.
- **When it fails.** The model failing or taking too long, or giving an
  empty note, is counted (`FAILED`, `EMPTY`) and journalled with sizes and
  times only; the previous note stays, and the next turn is told it.

It costs one DeepSeek call per batch, off the turn's path, and up to about
6.5 KB more in a turn's prompt once a conversation has a note.

### Talk notes: the note kept at a conversation's end

With the same switch, a conversation's end (the memory writer's, after 45 s
with no exchange or a change of scope) keeps its note in the lamp's memory
as that conversation's **talk note** (`talk_note`, memory-days.md):

- **What.** If the scope has a note, the exchanges of the conversation not
  yet in it are folded in once more on the note's own thread (the model
  failing keeps the note so far, counted), and the note leaves the worker's
  memory for the store, in the conversation's own scope, the room's
  included, linked to every exchange of the conversation and dated by the
  last. A conversation too short to have a note keeps none: its exchanges
  can be recalled themselves. Nothing is kept after a forget since the
  conversation began, and nothing for a conversation every exchange of
  which was forgotten, or one that overflowed the 64 sources or lost an
  exchange. With `LAMP_MEMORY_PERSONAL_NEEDS_FACE=on` a room conversation
  keeps none: one nobody known was seen in keeps nothing
  (`needs_face::observes`, reviewer-three's P1 on #4479).
- **How long.** Until forgotten: it never expires.
- **Never told unasked.** It is never in a session's note. A recall with
  the switch on also asks for talk notes of the days it asks about, or by
  its words, or the newest when it names no subject ("what did we talk
  about earlier?", or no words at all): the asker's own (up to 6,400 bytes) marked theirs and told as
  "your own summary of a talk with this person on that day, in your words,
  not theirs", then the room's (up to 3,200 bytes, only when the recall asks
  the room's scope at all, and not the day the asker forgot everything)
  told as "the room's, never told as the asker's own"; both "never read it
  out whole". An asker the lamp can't place asking about themself is still
  told nothing (#4196), and another person's are never asked for.
- **Forgetting.** "Forget everything about me" takes the person's. Any
  forget that removes one of its exchanges takes it, a kept fact's forget
  too, since its evidence goes, even after the exchanges expired (their ids
  are kept, `origins` in memory-days.md); forgetting the note itself takes
  its conversation; and room lines adopted by someone known take the room's
  note of them.
- **The observation pass.** It is given the note as `"summary"`, told it is
  the lamp's own words, to cite only the records and never to propose a fact
  only the summary supports. The voice also leaves out a proposal none of
  whose words is in what the person said in the records it cites while one
  is in the note, counted in the journal.

## What it keeps to itself

By itself the thread lives in the provider worker's memory and nowhere
else, and a restart of the service forgets it. The transcripts it is built
from are the ones the service already records in its session trace.

A service can also be given a file for it (`LAMP_THREAD_FILE` in its
environment, `--thread-file` to the provider worker). Then the same few
exchanges are written there within a second of an answer growing, whole and
readable by the service's user only, and read back when the service starts:
what is from the last ten minutes is told to the first session, and the
rest is ignored. On lamp-4ace the file is in the service's runtime
directory under `/run`, which is memory and not a disk and is gone at a
power cycle. This is for a service that ended and was started again a few
seconds later: on 2026-10-13 that happened twice from faults and about
twenty times from installing builds, and each time whoever was talking to
the lamp found it had forgotten them. A file that cannot be written or read
costs the thread at the next restart and nothing else.

What a person said goes into the next session's instruction as they said it.
That is no more than they could say to the lamp directly.

## Two voice routes, one file

The second voice route ([second-voice.md](second-voice.md)) writes the same
file as the Gemini worker, whole, after each of its turns. So that Gemini
does not erase that talk when it comes back, and is told it:

- At each connect (`Recalling::connect`, through `thread::current`) Gemini
  takes the file if it is newer than what it holds. The second route does
  the same before each turn's chat.
- Newer is by the file's modification time (`Thread::reload`), compared
  with the time of the file it last read or wrote itself (`Thread::wrote`).
  Its own file is not news.
- A forgetting (`Thread::forget`, through `clear_forgotten_thread`, applied
  before any reload) moves that mark to its own moment. A file written
  before a forgetting is never reloaded, so a reload cannot bring back what
  was forgotten. A file written after it is.
- A file that cannot be read changes nothing. A coarse filesystem clock or
  the wall clock stepped back can only miss a reload (`ponytail:` in
  `thread.rs`).
- **A file read but not one this build can read** (#1505: a later build's
  field, since `Kept` stays strict, or no thread file at all) is **set
  aside, not silently forgotten**: renamed beside itself as
  `thread.json.unreadable-<unix_ms>` (in `/run`, so a power cycle ends it),
  and said once with serde's reason and the file's size, never its words.
  The thread then starts empty, as before, and the reload counts as no
  thread taken, so nothing more is said. The line gives the error's kind and
  place, never its message, which could quote the file. A thread is ten
  minutes of context, so losing it once costs little; losing it without a
  word did not say why the lamp forgot. The set-aside file is never told to
  a session and `forget` does not reach it, but its words stay in `/run`
  until the next reboot, where before the next turn wrote over them; the
  reset removes it with the voice's whole `/run/lampos-voice`, not by name:
  a product reset that starts setup in place empties that folder
  (`reset::VOLATILE`, setup.md "The reset", #4500), and a reboot empties
  `/run`.

- Each exchange keeps its `whose` in the file, so one a person made on the
  second route comes back to the Live route as that person's. Who is at the
  lamp and whom the session was opened for are each process's own and are
  not taken from the file. The second route opens the thread for the person
  at the lamp before each turn (`Thread::at_lamp`, `Thread::opening`),
  counted from the eyes' known file as the Live worker counts it. With
  `LAMP_SECOND_SCOPED_CONTEXT=on` and a memory scope bound at admission, the turn's recall, history, kept things
  and answer are that request's scope alone, not the person opened for
  ([memory.md](memory.md#second-route-context-scope), #1675).

Shown by `a_thread_another_route_wrote_while_this_one_was_away_is_told_again`
and `an_exchange_on_the_second_route_comes_back_to_the_live_route_as_the_same_persons`.

## Verification

`thread::tests` (4 passed on macOS; the fourth: a thread saved by one
process is told again by another with a clock of its own, without what is
older than ten minutes, what claims to be from the future or what is not a
thread, the file is whole and private, and the new process's own requests do
not run into what it read): that what was asked and answered is
kept, in pieces as it arrives, and the talk around it is not, nor an answer
with no word in it; that an exchange older than ten minutes is not told,
something said long before an answer is not taken for its question, only
twelve exchanges of bounded length are held, and with less room the latest
that fit are told in order; that a new session's instruction is the written
one with the time and then the thread after it, with an empty thread and no
time is the written one unchanged, and with no room for the thread still
carries the time.

Not shown: that the model, told this in its present wording, does carry
on, does not bring the earlier conversation up by itself, and still stays
quiet for talk that is not for it. The character file is touchy (see [character.md](character.md)); a
renewal with a thread has to go through its checks before this is relied on.
