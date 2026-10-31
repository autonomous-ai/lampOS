# Speaking first

What the lamp says unasked, through the voice (`crates/voice/src/speak_up.rs`,
with `lamp-eyes --presence-file` and a written request in `lamp-gemini`): a
look it said it would take, an ask again, a note given it by another part
(the say service, below). **The hello to someone who comes back to the desk
is the Welcome back skill's alone** ([skills.md](skills.md),
`lampOS/skills/welcome-back`).

## One hello, from the skill (#2825)

The read of every behaviour on 21 October found the hello built twice: as
the Welcome back skill, and as the voice's own (`speak_up::Arrival`, switched
by `LAMP_SPEAK_FIRST`, with `LAMP_HELLO_ASKS_POLICY` to ask the Policy).
The row asks for no one-off code, and two paths for one hello can both
fire, so the voice's own was removed. `LAMP_SPEAK_FIRST` and
`LAMP_HELLO_ASKS_POLICY` are no longer read; no `lamp_speaks_first` turn,
`hello_due` or `hello_policy` record is written, and nothing publishes
`voice/arrival`. A test fails if the voice's sources hold a hello of their
own again, or the skill stops greeting
(`speak_up::tests::only_welcome_back_says_hello_on_an_arrival`).

What the voice's hello did, and where it went:

| The voice's hello | Now |
|---|---|
| a face at least 0.14 of the picture's height, there 3 s, after 5 minutes with nobody | Welcome back's trigger: `near = 0.14`, `for = "3s"`, `absent_for = "5m"` |
| at most one hello in 20 minutes | Welcome back's `cooldown = "20m"` |
| nothing said to the lamp or by it for 30 s | Welcome back's two `voice/state` conditions: resting for 5 s, and the lamp not speaking for 30 s (#1140, [skills.md](skills.md)) |
| the keeper's pause | the say service's gates (below) |
| the Policy asked first (`LAMP_HELLO_ASKS_POLICY`, `voice/arrival`) | Welcome back's `[ask] policy = "hello"`, through `lamp-skills`' ask (`LAMP_SKILLS_ASK=policy`) |
| by the name typed at setup | the name is in the model's instruction (`person::told`, below), and the skill's note says "their name if you know it" |
| by a known face's name | told with Welcome back's note while someone is at the lamp, by its `who = true` permission, which no other skill has: "(The person at the lamp now is NAME: you know their face.)" (`person::at_the_lamp`, `known_faces::note`) |
| one beat warmer for someone Mood calls `regular` or `close` (#1670) | told with Welcome back's note the same way: "(The person at the lamp comes by often: speak to them as you would someone you see most days, warmly and without fuss.)" (`person::OFTEN`) |
| for someone `close`, no hello within the hour | **dropped**: `lamp-skills` has no cooldown by how familiar someone is, and Mood carries no identity, so the hour counted any hello, not one to this person; the 20 minutes hold for everyone |
| a glance away under 20 s is the same arrival | Welcome back's `gap = "3s"` keeps a look down; a longer one ends the hold, and the absence counts from the last near face, which comes to the same for an arrival after five minutes |
| `lamp_speaks_first` turn, `hello_due` and `hello_policy` records | the say service's `skill_says` record, naming `welcome-back`, and `lamp-skills`' journal |

## Someone at the lamp

The eyes keep one line fresh while a face is near (`--presence-file`: the
face's height as a share of the picture's, no picture, nothing else; a file
not written for a second and a half means nobody). `LAMP_EYES_PRESENCE_FILE`
in the voice service's environment names it; without it nobody is known to
be at the lamp from the eyes. From it the voice keeps one thing
(`speak_up::Presence`): someone has been there, a face at least 0.14 of the
picture's height, for three seconds without a break. A reminder due long ago
and the news wait for that, or for a person heard a moment ago.

While someone is there, a note whose skill asked (`"who": true` on the say
service, sent by `lamp-skills` only for a skill whose `[permissions]` say
`who = true`: Welcome back alone; refused with exact `text`, an add-on's
quoted line or what the voice keeps) is told who it is, after the note: a known face's name ([eyes.md](eyes.md)), and for a
remembered person Mood calls `regular` or `close` (`familiar` in
`mood/state`, from the eyes' `lamp-eyes --visits on`, [mood.md](mood.md)),
the sentence above. The level is read with the presence file, four times a
second, from source `mood` only and within its 2.5 s
(`speak_up::familiar_in`); if the eyes have not recognised them yet, nothing
is added. Any other skill's note is never told: Guard mode and Who was here
must name nobody (the reviewer-lead's P2 on #2923). Tests:
`person::tests::a_skills_note_is_told_who_is_at_the_lamp`,
`person::tests::only_a_note_that_asked_is_told_who_is_at_the_lamp` (the
coordinator's call: another skill's note, or nobody at the lamp, is the
turn as it came), `say::tests::who_goes_only_with_a_note`, lamp-skills'
`only_a_skill_given_who_has_its_note_told_who_is_at_the_lamp` (Guard mode
and Who was here are not given it),
`speak_up::tests::the_familiar_level_is_read_from_moods_fresh_state_only`.
No test runs the whole loop with a presence file.

A `who` note is also told, last and whether or not anyone is at the lamp,
the part of the day from the lamp's own time and to say a hello and the
name only: "(It is the afternoon here now: if you name the part of the day,
it is the afternoon, never another. Say only a hello and their name if you
know it: no weather, no day or date, no time, no question and no offer.)"
The part is the one the session's time names (morning 5-11, afternoon
12-16, evening 17-20); at night, or while the clock is not set or the zone
cannot be read, it is told not to name one and just say hello. A hello at
15:42 had said "Evening" and added the weather, the weekday and "how's it
going" (the first builder, 30 Oct, on the launch testers' findings: the
part of the day from the local time, never the model's own). Any other
skill's note is unchanged (`local_time::hello_told`, `person::told_hello`;
tests `local_time::tests::a_hello_is_told_the_part_of_the_day_from_the_lamps_own_time`,
`person::tests::only_a_note_that_asked_is_told_the_part_of_the_day`).

The say's `skill_says` record keeps whether its note was told
(`told_who`, true or false) and Mood's level it was told with (`familiar`:
`"new"`, `"regular"`, `"close"`, or null when not told or the level is not
known), so the records show whether the level shaped the words (#2831).
Never the name or the note (`person::told_record`; test
`person::tests::a_says_record_keeps_the_level_its_note_was_told_and_no_name`).

What ends the quiet every unasked turn waits for is talk: the lamp's own
words playing, any voice the detector calls speech (words or none: a line in
a language the route takes no words from is still someone talking), a
person's words heard (a transcript of what was heard with a word in it), or
a Stop. The rest of a request does not: its end's silence and the wait for
an answer that never comes aloud (`no_audio_answer`, `not_speech`) leave the
quiet counting from its last speech, though the lamp still waits for the
request to end before it begins anything. On lamp-52e6 on 21 October, with
the person seated quietly, the lamp's own fan opened 11 wordless requests in
two minutes, each had held the quiet until it ended, and the onboarding
walk's 3 s never came. A fan the detector calls speech still holds it while
it does. Every quiet the lamp waits out "since the last turn" is this one
(`last_turn_us`): a reminder's and a skill's say's, the news', a face added
from a photo's, and each onboarding step's
(`coordinator::tests::second_route::speech_holds_the_quiet_with_or_without_words_and_a_wordless_requests_tail_does_not`).
The keeper's pause holds every turn the lamp begins itself: the look it said
it would take, the second look, a reminder, a skill's say and news.

## How it is said

The lamp cannot be handed words to speak: its voice is the model's. So an
unasked turn is a request like any other, with a written note in place of
speech (`ProviderInput::Written`, `InputSender::try_written`): one
`clientContent` message, complete, outside any activity, and whatever the
model answers is played like any answer.

The name typed at setup is told to the model after
its character, by the provider and second workers that build it (the voice
passes them `LAMP_PERSON_NAME`), only as the name the person gave, never as
who is at the lamp: "… gave their name as “Example Name”. Whoever is at the
lamp may be someone else …" (`person::told`). So a skill's "by name if you
know it" (Welcome back) can use it. It is one line of at most 64 characters,
setup's own limit, with no control character, line separator or direction
mark; anything else, or no file (before setup, after a reset), is no name,
and the voice says why without saying the name. Each time it is read (when a
worker builds the instruction), the journal says so
without the name: `the person's name from /etc/lampOS/person/name is used
(12 characters)`. The first five minutes (#841) run in the same session, so
their notes come after the same instruction.

**Owner only (#2602, `LAMP_PERSON_NAME_OWNER_ONLY=on`, off unless set).**
Told in the instruction, the name reached unknown faces: in Friday's probe
on #3014 the model put it to a stranger asking "what's my name?" in 60 of
80 runs, mostly as "are you …?". With the switch the instruction has no
name, and a line is told the same words (`person::told`) only when it is
the owner's (`person::may_tell`):

| Who the line is with | Told the typed name |
|---|---|
| The owner's known face, the face the eyes keep under the typed name, the line's person by its memory scope lease, alone in view (or held through a short gap, `LAMP_FACE_GAP_HOLD`) | yes |
| Nobody in view, the room quiet or not | no |
| An unknown face | no |
| Another known face | no |
| Two faces or more, the owner's among them or not | no |
| No look at all | no |

Nobody in view is not the owner: a visitor beside the lamp, turned away or
in a dark room is nobody to the camera too (the reviewer-lead on #3050).
Just after setup no face is kept, so the owner is an unknown face until
they ask the lamp to remember their face under the typed name. A line
found to be someone else's (a face seen speaking that isn't the known
person's) is narrowed to the room's, and the room is never told the name.
A lone known face kept under another name is counted in the journal, with
no name: if it is the owner's, they never hear their typed name. The
second route judges every turn on that line's own lease, with or without
`LAMP_SECOND_SCOPED_CONTEXT`: never on the person its session is counted
for, which stays the owner's for 30 s after they go (the reviewer's P2 on
#3050), and a turn with no lease is told none. It tells it right after the
character, where the instruction had it (told after the exchanges instead, the
owner's English "what's my name?" got it a third as often in the #2602
probe).

The Live route tells it as a note on the open line, and a Live
conversation keeps what it was told, in that session and in any
connection resumed from it (the reviewer's two P2s on #3050):

- A note is only written as its line is committed; a line given up before
  its End never writes it. So the name counts as told in a session only
  once the service answers the line it went with, and until then each of
  the owner's lines is offered it again (`person::live_answered`).
- From the moment it is offered, the session counts as holding it
  (`person::live_holds`), until that line is known to be given up before
  its End, or a fresh connection opens. Between turns, twice a second, the
  next line is judged as it would begin now: if it may not have the name
  (the owner gone and an unknown face come, say), the session is renewed
  fresh at once, with no resumption handle (`person::open_fresh_live`). A
  line that begins in such a session before that is not sent, and fails
  as not delivered, as a line for someone else known does with
  `LAMP_LIVE_SCOPED_SESSION`.
- Cost, on the Mac's scripted service: the renewal is ready 1 to 5 ms
  after it is asked, 16 to 552 ms after the face changes (the eyes are
  read twice a second). A visitor who speaks after that waits nothing more;
  one who speaks first loses that line and is answered from the next. A
  real Live connection takes longer to open.

Without a memory the Live route has no lease of a person, so it never
tells the name. Welcome back's "their name if you know it" has the typed name
only where a line was told it; a known face's own name is told as before
(`person::at_the_lamp`).

Tried on lamp-4ace through its relay with `lamp-voice provider-check CONFIG
--written NOTE`, which opens a session, sends the one request and plays
nothing: with a note much like this the model answered "Hey! There you are.
Good to see you.", 2.5 s of sound, the first of it 0.91 s after the note.
The other form a note could take, text inside a request's own activity
(`--note`), made the service close the connection (code 1007), three times
in three. That was a request with a note and no sound in it. Beside a
request's sound the service takes one, and that is how the model is told
the lamp's name was heard ([attention.md](attention.md)); `provider-check
CONFIG --note-in-speech NOTE SOUND` tries that form without a microphone
or a speaker, sending a recorded question as if heard.

A written request goes only onto a connection that owes nothing. Sent
while an earlier answer is unsettled it is not written, and ends as a
request the service left unanswered.

## Taking a look

The same written request is how the lamp sees when it is asked to, without
paying for sight in every answer ([eyes.md](eyes.md): a picture anywhere in
a session's history made every later answer about half a second later, so
giving the model one with everything said was switched off).

- The character file tells the lamp that it sees when it looks, and that
  when someone asks what it sees, shows it something or asks how they look,
  it says "Let me look." and nothing more.
- The coordinator hears the lamp's own words as they arrive. An answer that
  opens with "let me look", "let me take a look" or "let me have a look"
  (`speak_up::asks_to_look`; not "let me see", which people say before any
  answer) is the lamp asking for its eyes: `look_asked` in the record. The
  phrase must close where it ends, with `.`, `!`, `…`, `,`, `;`, `:` or a
  dash: a word after it is something else, and until the closing mark
  comes more words may follow, so it is not yet a look. On lamp-4ace on 19
  October the second route's "Let me look that up", before a web search,
  took two pictures and the model described the desk unasked (#1712);
  "Let me look that up", "Let me look into it" and "Let me take a look at
  the forecast" take none.
  Only in a turn someone spoke: in one the lamp wrote itself (its own look,
  a hello, a "Sorry?", a reminder) the words ask nothing, so looks never
  chain with nobody speaking (`coordinator::own_turn`, the worker's
  `heard` rule; cleared with each service cycle, whose turns number from 1
  again). A skill's search on the second route goes on in its own turns
  by the skill's call, never by these words ([second-voice.md](second-voice.md)).
- When the lamp has finished saying so, the next turn is its own: a written
  request with `look` set. The provider worker puts the eyes' newest frame
  with the note (`--eyes-look PATH`, from `LAMP_EYES_LOOK_FILE`; a frame
  written in the last second, at most 64 KiB) and the model answers what
  was asked from what it sees. If the eyes are shut the note says so
  instead, and the model is not left to describe a picture it was not
  given. If someone speaks before the look begins, they have the turn and
  the look is dropped.
- Once nothing has been submitted for twenty seconds after a look, the
  worker renews the session (`renew_when_idle`), so the picture leaves the
  history and answers are as fast as before. The conversation, with what
  the lamp said it saw, goes across in the thread
  ([conversation-thread.md](conversation-thread.md)). Until then a
  follow-up ("and what colour is it?") is answered from the same picture.

- With `LAMP_LOOK_FULL=on` (#836, off unless set) a look is a full look.
  As soon as the lamp's words ask for its eyes, the eyes are asked for one
  larger still (`{"look_full":ID}` on the eyes' permit socket, a new id
  for every ask, counted from the clock when the service began;
  `look_full_asked` in the record), taken once the head has settled
  ([eyes.md](eyes.md), "A full look"), while the lamp is still saying
  "Let me look". The eyes mark the still with the id (a JPEG comment,
  `lamp-look ID`) in their own file, `--full-look-file`, which the voice
  reads from `LAMP_EYES_FULL_LOOK_FILE` (a full look needs both it and
  `LAMP_LOOK_FULL=on`; the worker is given that path as `--eyes-look`). The
  working frame stays in the sight file. The look then waits until that file holds the still
  with this ask's id, or at most 2.6 s after the lamp said it would look
  (`look_full_still` in the record, with how long after and whether the
  still was there). The worker is started with `--eyes-look-full` after
  `--eyes-look PATH` and takes a picture up to 15 s old instead of 1 s
  (`FULL_FRESH`, as long as a look may wait to begin, and as long as the
  eyes keep the still; at 3 s a look that began late went without it, #842),
  and of up to 184 KiB instead of 64 (`lamp_gemini::MAX_LOOK_IMAGE_BYTES`).
- A still for writing on paper (#941): with `LAMP_LOOK_FULL_SIDE=N` (320 to
  4096, off unless set) every full look asks the eyes for a still N across,
  `{"look_full":ID,"side":N}`, made to 184 KiB instead of 1024 across and
  64 KiB; `side` goes into `look_full_asked` in the record. One look may ask
  its own side instead (`sight::Look::side_next`): what a skill tool's look
  step will ask with `look = { side = N }` once skill tools (#984) carry it to
  the voice. Unset and unasked, the ask and the still are as before.
- A skill tool's look (#941): a function whose answer has `"look": true`
  (`lamp-skills`, `look = true`; [skills.md](skills.md)) is a full look for
  its turn whether or not the model then says "Let me look". Once the answer
  has gone to the model the worker sends `LookAsked` to the coordinator,
  only for a call the skills answered (`skills/`), never another part's.
  The coordinator records `look_asked` with `"by":"tool"` and asks the eyes
  (`look_full_asked`) when the turn's first audio plays or its reply ends,
  whichever is first, as the phrase would have: lamp-skills has sent the
  head down before answering (#1146), so the eyes wait for it to settle.
  It goes on from there as above, the still's wait counted from that ask;
  the model's own words after it ask nothing more. Only with
  `LAMP_LOOK_FULL=on`, only for the turn in hand, never for a reminder's
  or one the lamp wrote itself, and only for the session that made the
  call (no renewal while it is owed). A look dropped, tool's or words',
  is recorded as `look_dropped` with `why`, a tool's look the coordinator
  turns down too. A turn cut off unsaid or asked again ("Sorry?") drops
  its look before the eyes are asked, and so does a look owed when the
  other route answers now (the second route took a Gemini call's turn, or
  Gemini is back from the second route's) or the calling session has been
  renewed: the coordinator keeps which route made the call, since the two
  routes' sessions are numbered apart; once dropped, the second route's
  own "Let me take a look." on that turn asks for a look of its own. The
  second route sends `LookAsked` for its own `take_a_look`, and with
  `LAMP_SECOND_SKILL_LOOK=on` for a skill's answer with `"look": true`
  ([second-voice.md](second-voice.md)); its look goes to its own session,
  with the skill's note for the picture after the look turn's question.
  A full look that goes without its still says so in one journal line,
  "a full look went without its still: asked for ID, the file held ..., N
  ms old" (nothing, another ask's still, or a picture with no id; no
  picture), and a worker that finds no picture to send says "the lamp
  looked: no picture with request N: WHY". The
  note (`LOOK_FULL_NOTE`) adds: "If you cannot make out what they are
  showing you, say so plainly and ask them to hold it a bit closer to you;
  never guess what it might be." A full look without its own still goes
  with no picture and the note that the eye was shut and it could not see,
  to say so plainly and never guess (`NO_LOOK_NOTE`): no
  still, an earlier look's still the eyes still hold, a cancelled look's
  still written after this ask, or eyes started
  without `--full-look`, which write no still. A write time could not tell these apart; the id
  does.
  Without the variable nothing is asked, waited for or said differently.

A picture leaves the lamp only on such a look: when a person has asked the
lamp to and the lamp has said it will, or the model has called a skill's
tool that looks (only a skill's answer asks for one). With `LAMP_VOICE_SECOND_CONFIG` set
and Gemini away, a look's picture goes to the second voice's `llm_base_url`
(the company relay and the chat model behind it), not to Gemini
([second-voice.md](second-voice.md)).

Tried on lamp-4ace without a sound being played (`lamp-voice provider-check
CONFIG --look PICTURE.jpg NOTE`): given a 21 KiB photograph of the office
and a note like the one above, the model answered "I see a busy desk and a
whole office full of chairs and computers.", the first of it 1.49 s after
the request.

Played on lamp-4ace on 2026-10-15 at 07:35, with the character's line in
(`lampOS/deploy/character/lamp.txt`): "Hey Lamp, what do you see right
now?" → "Let me look." (0.98 s after the end of speech), and 2.3 s later
"I see your desk with the computer monitors, a keyboard, and that white
water bottle." A picture of 13,048 bytes went with the look. The talk then
paused, so the session was renewed without the picture, and the follow-up
23 s later ("And what colour is the wall?") took a look of its own: "The
wall behind your monitors looks white."

## Telling it something that needs no answer

A third written form tells the model a fact and asks for nothing: one
`clientContent` message whose turn is left incomplete
(`InputSender::try_context` in `lamp-gemini`, `wire::encode_context`). It
is no request: nothing is owed for it, the next thing heard is numbered as
if it had not been written, and it is written only at a moment when no
request is open and no answer is unsettled (otherwise it is dropped, for
whoever offered it to offer again).

Tried on lamp-4ace through its relay without a sound being played
(`lamp-voice provider-check CONFIG --context FACT --written NOTE`, and
`--context FACT --then-sound-and-written NOTE`, which sends a second of
near-silence as a heard request in between):

- Nothing came back for the fact alone, in each of twelve runs (nine
  waited three seconds for it; three went on to the near-silence).
- It was taken in. Told a name this way, the model answered "do you know
  my name?" with it, 0.93 s after the question; the same question to a
  session not told got "I'm afraid I don't!". One run each.
- A session takes sound after it: three times in three the near-silence
  ended as unanswered, the connection stayed ready, and the written
  question after it was answered ("Four.").
- It changes what the model does with a written line, one run each: "you
  hear someone say, without calling you: what is the weather like
  outside?" went unanswered by itself, was answered after "the office has
  emptied... what they say aloud is said to you", and after "the office is
  full and people are talking to each other" ended with "..." and no
  sound.

The first use is for a new session (`crates/voice/src/session_fact.rs`,
`LAMP_SESSION_FACT=on`): being new, a session takes the first thing it
hears for the start of a conversation with it. Of six that opened on
lamp-4ace in a talking office with nothing told to them, two answered a
colleague's sentence to a colleague. With the switch on, each new session
is told once, unless the lamp said something to someone in the last 45
seconds:

```text
(For you to know, not to answer: you have been at your desk for a while,
and nobody is talking to you at the moment.)
```

The second use is the room (`crates/voice/src/room_told.rs`,
`LAMP_ROOM_FACTS=on`; [character.md](character.md), "The rule follows the
room"). The provider worker sees every request and whether the lamp spoke
in it, and from that alone calls the room quiet (nothing said for five
minutes; a sound the coordinator dropped does not count) or busy (three things in a row, inside a minute, that the lamp
said nothing to). When that changes, and when a session opens, the session
is told one of:

```text
(For you to know, not to answer: the room around you has gone quiet.)
(For you to know, not to answer: the office around you is busy, with
people talking to each other.)
```

only when nothing has been heard or said for two seconds and no
conversation is going on. With both switches on, a session that has been
told how the room is is not also told the plain fact. Host tests for the
rule (`room_told::tests`); the worker's telling is seen in the journal
("told the session"). Not yet heard on a lamp.

It describes and does not ask for quiet, and a test holds it to that
([conversation-thread.md](conversation-thread.md) has what asking did).
Told this, a session still answered a question put to it by name in a
written line, three times in three. The switch is off: what the fact does
to the first thing a session hears has not been measured, and written
lines in place of speech cannot measure it, since an uncalled written line
is left alone with or without it.

## Asked by another part: the say service (#939)

Off unless `LAMP_SAY=on`. Another part of the lamp, `lamp-skills` first
([skills.md](skills.md)), asks the voice to say something unprompted, and
is told what became of it (`crates/voice/src/say.rs`). The hello is on it
(Welcome back); task news and reminders are to move onto it.

**The socket** is `LAMP_SAY_SOCKET`, `/run/lampos-say/say.sock` if unset,
in a directory of its own: the voice runs as root and its own runtime
directory is closed to others. With `LAMP_SAY_GROUP` (a group's name, as in
`/etc/group`) the directory is 0750 and the socket 0660, both that
group's; without it both are the voice's user's alone (0700, 0600). A
socket left by a voice that has gone is taken over; one that still answers
is not. One JSON line in:

    {"skill": "reminders", "note": "Remind them, in one short line, to call the bank.", "priority": 8, "expires_in_ms": 120000}

- `skill`: 1 to 64 of `a-z`, `0-9`, `-` and `_`. Logged and traced only.
- Exactly one of `note`, an instruction the model phrases (as Welcome back's
  hello and the news'), `text`, words to say as they are (the model is told
  "Say exactly these words, in your own voice, and nothing else:" before
  them), at most 900 bytes with no control characters, `from`, words
  the voice keeps itself (below), `sound`, a skill's own short sound
  (below), or `cue: "hum"`, the fixed quiet hum used by a routine.
- `priority`, 0 to 9, 5 if left out: higher first, the oldest first among
  equals.
- `expires_in_ms`, 1 ms to an hour, from when its line was read: not begun
  by then, it is `expired`. Its time is looked at again as it is begun, so
  that none begins after it, however long it waited for the voice's loop
  (the reviewer on #954). A line is at most 4,096 bytes with its newline.
- Optional `start_before_us` is an absolute deadline on this host's
  monotonic clock. The start must precede both that deadline and the
  read-relative expiry. Routine clients preserve their original transition
  deadline here; a delayed connection or read cannot give the hum a new
  lifetime. Existing clients that omit it keep their current behavior.

When the voice begins the turn, one line, so that the body and the light
can move with the words (the first builder on #939):
`{"event":"begun","at_us":…}`, the moment on the shared monotonic clock
(`lamp_ipc::monotonic_us`) in microseconds; the first sound follows when
the model's comes, about a second later. A say that is never begun has no
such line. Then one line, once the outcome is known; the connection stays
open until then:
- `{"outcome":"spoken"}`: the turn completed with the model's audio played
  whole, the proof a reminder's acknowledgement takes (complete, with
  audio, no playback gaps, no owner left, the microphone allowed).
- `{"outcome":"dropped","why":"…"}`: begun and not spoken: the reason the
  turn was cancelled (`user_interrupted`, …), or what became of it
  (`no_audio_answer`, `provider_failed`,
  `audio_written_with_playback_gaps_unscored`). Never tried again here: the
  asker decides.
- `{"outcome":"expired"}`.

**While the model service refuses** (#4265, `LAMP_SECOND_REFUSED_HOLD=on`,
the lead's call on #1711; [failure-notices.md](failure-notices.md)): while
turns are held, a `text` is said as the lamp's own line, its words as they
are and no chat asked (`Says::take_while_refused`, `onboarding::said_as_is`;
a `text` with a `"` in it can't be held so and waits); a skill's own sound,
a recording or a cached line never needed the model and plays as before.
A `note` or a `quoted` line needs the model to word it: it waits, as a
spoken line does, and is said once the hold lifts (the minute's try or the
service answering again), or `expired` at its own time. Sent with the try,
it spends it only if its chat is refused; its answer ends the refusal. Words
said as they are end none.
- `{"outcome":"refused","why":"…"}`: a request that is not one (the reason
  names the field and never repeats what was asked), no whole line within
  1 s of being taken (one deadline for the line, so that a client sending a
  little at a time cannot keep its place), 16 already waiting, or the voice
  going.

**A skill's own sound** (#938, a skill's `sounds/`): `{"skill": "chime",
"sound": "/var/lib/lampOS/skills/chime/sounds/ding.wav", "priority": 5,
"expires_in_ms": 30000}`, the absolute path of a WAV, at most 1,024 bytes.
The file is read on the client's thread when the ask arrives, never on the
voice's loop, and refused there with why (the reason never names the path)
unless its real place, every link followed, is under the skills folder
(`LAMP_SAY_SOUNDS`, `/var/lib/lampOS/skills` if unset) and it is one the
voice plays (`say::read_sound`): opened with `O_NOFOLLOW | O_NONBLOCK`, so
that a link is refused and a FIFO never holds the reader; by the open
file's own metadata (`fstat`) a regular file of at most 148,140 bytes; a WAV
of 16-bit mono PCM at the speaker path's 24 kHz, 1 frame to 3 s; no 10 ms
of it louder than -30 dBFS RMS (well under the lamp's speech, about -20
dBFS) and no sample past -12 dBFS, so that a
train of clicks is refused too. `lamp-skills` checks a skill's sounds at
load with this same function. Its samples go to the voice, which never
waits for the lamp to be at rest: at its next pass the sound is refused at
once, `{"outcome":"refused","why":…}`, unless the lamp is at rest as a say
needs it: not paused (`the lamp is paused`), not muted by the privacy
switch (`the lamp is muted`), not speaking (a reply, queued output, a
speaker write pending or the playback mask: `the lamp is speaking`), not
listening (someone's input taken, a candidate of speech not yet a turn, a
submission draining, listening not ready, or speech heard within the last
second or no fresh capture, a reminder's quiet: `the lamp is listening`),
no answer owed (the lamp said it would look: `the lamp owes an answer`), no
reminder held for its skill (`a reminder is said first`), 15 s since the
last turn (`the lamp spoke a moment ago`), and no other skill's sound in the
last 30 s (`a sound played less than 30 s ago`). Otherwise it is played as a
turn of the lamp's own, as the head tap's "hm?" is: through the voice's
speaker path, so it never overlaps speech and the echo canceller has it, no
provider request, no face on the ring. `begun` comes as it is begun, and
`spoken` means played whole with no gap in its playback (`dropped`
otherwise: a person speaking over it ends it as any turn). The voice traces
`skill_sound` with the skill, whether it plays and why not.

**The shutdown's last words** (#3697, [shutdown.md](shutdown.md)): the skill
`shutdown` is root's alone (`say::SHUTDOWN_SKILL`; from any other caller,
`skill shutdown is asked by root only`), and its ask is taken before any
other say, whatever their priority (`say::is_last_words`). None of the gates
above or below holds it but the keeper's pause and the privacy switch
(`say::may_play_last_words`; a muted lamp stays silent): not right after a
turn, not one sound in 30 s, not listening, not speaking. The lamp's own
speech is cut for it (`last_words`, the reason its turn is dropped with),
as it is the last thing the lamp says. Every other skill's say and sound
keeps every gate.

`cue: "hum"` uses the same sound admission and speaker path. Its samples
come from the existing head-cue renderer at its -20 dBFS default, without a file, model
request or extra actuator writer. It is an alternative to every other
content field; other cue values are refused. Its acoustic result remains
part of [Bedtime and Morning's lamp comparison](bedtime-morning.md).

**A cached line** (#3697, V1's `_TTS_CACHE_DIR`): `{"skill": "shutdown",
"text": "…", "cache": true, …}`, from root's helpers only (the peer is
root by `SO_PEERCRED`; anyone else is refused `cache is asked by root's
helpers only`), and with `text` only (`cache goes only with text`). It is
not given to the model: on the client's thread the words are looked up in
the speech cache (`say::tts_cache`), `/var/lib/lampOS/voice/tts-cache`, a
24 kHz mono 16-bit WAV per line named by the SHA-256 of `voice|model|language|text`
(the second route's speech voice and model, `LAMP_SECOND_SPEECH_MODEL`
applied, and its speech-to-text language). A hit is played with no
network. A miss is rendered once through the speech service the voice
already speaks with (`Tts::speak`, ElevenLabs through the relay), within
5 s, written whole (a temporary file, fsync, rename, the folder fsynced:
the lamp is unplugged after a shutdown) and played; at most 48 lines and
20 s each are kept, a line past that is played but not kept. Without the
second route's configuration, or offline on a miss, it is refused at once
with why (`the speech cache is off`, `not in the speech cache, and not
rendered: …`), and the asker says the words another way. Its samples are
played as a skill's sound is, with the same gates. At start the voice
renders the cache's fixed lines (`tts_cache::WARM`), the shutdown's
`lamp_msgs::GOING_TO_SLEEP` first, then a hand's hold's sleep line
`lamp_msgs::SLEEP_LINE` (#3748, `LAMP_SLEEP_LINE`, controls.md), on a
thread of its own, tried again until kept (after a minute, then twice as
long each time, at most hourly; a line that renders empty or past 20 s is
never asked for again in that run), so the first
shutdown is said from the cache even with no network then. The cache is
made once per process and shared: the coordinator reads the sleep line
from it once per conversation (`TtsCache::kept`), a file read only, never
a render. With an owner's language other than English (#4473 (c),
second-voice.md, "The fixed lines"), its sixteen fixed lines in that language
are warmed first, then these two, and a cached `text` that is one of them
is played in the owner's words when kept, its English otherwise. Every
warmed line is tried each round, so one that can't be kept holds back no
other.

**When it is said.** After a due reminder and before the news, through a reminder's gates: no reply owed or queued, the speaker
free, nobody's utterance pending, the microphone's capture fresh and a
second since the last speech, the playback mask free, a ready session and
the keeper's pause off; and a quiet since the last turn of 15 s (as the
news), or 1 s from priority 8 up (as a reminder). It is a written turn like
the others (`skill_says` in the record, with the skill, the priority, `told_who`
and `familiar` (above); never the words), the lamp's own turn to the attention rule and the
Policy's hold. The Policy is not asked here: whoever asks the voice to
speak has asked the Policy's gate first ([skills.md](skills.md)).

**What the voice keeps: `from` (#939, (c), the first builder's 12:32).**
`{"skill":"reminders","from":"schedule","priority":8,"expires_in_ms":30000}`
asks for the reminder Schedule has due; `"from":"tasks"` for the news of
work handed over. The skill says when, and moves and lights with the
words; the voice keeps their delivery, as it always did: a reminder's
durable acknowledgement, news told once. The words are the skill's own
(`[news.words]`, `[reminder.words]`, #940), read by the voice from the
installed `skill.toml`. `lamp-skills` sends it
for `say = { from = "schedule" | "tasks" }` ([skills.md](skills.md)).
- Such an ask is never said on its own: the general say passes it by, its
  quiet included. It waits for its source's turn, which takes it
  (`Says::take_from`) and says the reminder or the news as it always did,
  with the ask's ticket on that turn: `begun` as the turn is committed, and
  `spoken` on the same proof the reminder's acknowledgement takes, or
  `dropped`. Not taken in time, it is `expired`: a `from` ask lives at most
  20 s from when its line was read (`say::FROM_LIVES_US`), whatever its
  `expires_in_ms`, so one that came too late for its reminder is never
  taken for the next one due (Fourth on #1038). Within those 20 s it can
  be: an ask names its source, not an item (the Reminders skill sees
  `schedule/due`'s counts, not which reminder), so one read just after the
  voice said its reminder rides a second reminder or news due in that
  window, and the skill looks and moves with that one. It is still a
  reminder said, with its own words; the case is two reminders due within
  20 s of each other (Thirteen on #1038).
- **A news ask may name its task:** `"from":"tasks","task":10`. It is
  given that task's news and no other; an ask naming none takes any, as
  before. `task` goes only with `from: tasks`. Without it, task 9 done and
  task 10 failed in one message, asked for in the order 10, 9, drooped
  and dimmed the lamp while it said 9 was done (the reviewer on #1042).
  With `LAMP_TASK_NEWS=skill`, news waits for an ask for its own task
  until 10 s after the voice had it (`TaskNews::found_at`), then is said
  by the voice: the Task news skill asks the moment the work settles, and
  its ask is gone 20 s later, so news held back longer (nobody there, a
  talk) is said at once when it can be, not 10 s later still (the
  reviewer's P2 on #1667). With `LAMP_TASK_NEWS_AT_PAUSE=on`, news
  told with a skill's ask, or while another such ask waits, is told one
  at a time, not together (the reviewer on #1223): each run takes its own
  task's news.
- With **`LAMP_SCHEDULE_SPEAK=skill`** (Schedule speech on, as `on`), a due
  reminder waits for such an ask, at most 10 s from when it was first held
  (`say::SKILL_WAITS_US`), and then the voice says it itself, with
  `lamp-voice: no skill asked for the reminder due within 10 s` in the
  journal: a reminder never waits on `lamp-skills` being up. The provider
  worker speaks reminders under `skill` as under `on` (its
  `--schedule-speak`, by the same rule, `reminders::speaks`); until 17 Oct
  it got that only for `on`, refused every reminder under `skill`, and
  none was said (the reviewer on #1038). With
  **`LAMP_TASK_NEWS=skill`** news of work finished, stopped or asking waits
  the same way, but from when the voice had it (above); progress is the Policy's to time already and is told as it
  was. Without them nothing waits, and a `from` ask that is waiting is
  taken whenever its source next says one, so the skill still moves with
  the words. With the switch but no say service (`LAMP_SAY` off), nothing
  can ask, and nothing waits.
- While a reminder is held for its skill, and is still the one offered,
  the say service's own says and the news wait for it: a due
  reminder comes before anything else the lamp says unasked, as it did
  without the switch (Fourth on #1038).
- Tests: `say::tests::an_ask_for_what_the_voice_keeps_waits_for_its_own_source`,
  `coordinator::tests::say::a_due_reminder_or_news_waits_for_a_skill_at_most_its_time`,
  `coordinator::tests::reminders::a_reminder_a_skill_asked_for_is_answered_on_the_acknowledgements_own_proof`,
  `reminders::tests::on_and_skill_turn_schedule_speech_on_and_nothing_else_does`,
  `coordinator::tests::reminders::a_reminder_held_for_its_skill_keeps_other_unasked_speech_waiting_while_it_is_offered`.
  The last is of the condition only: the places it holds (the say
  service's turn and the news, in the coordinator's loop; the voice's own
  hello was a third until #2825) are driven by no test, as no harness runs
  the loop with all of them (Thirteen
  on #1038). On a lamp: with `LAMP_SCHEDULE_SPEAK=skill` and `lamp-skills`
  stopped, a say sent in the 10 s a due reminder is held is said after the
  reminder, never before (`no skill asked for the reminder due within
  10 s` comes before `saying what the skill … asked` in the journal).

**Most likely wrong:** a say of priority 8 or more in the second after a
conversation's last turn: a reminder takes that second today, and a skill
may not need it.


**An add-on's quoted line (#2530 part 2, `LAMP_ADDON_SAY=on`).** In place of `note` or `text`, `quoted` is a partner's line from an add-on's folder, at most 300 bytes on one line. Refused without the switch, and on the Live route. Its turn tells the model it is quoted and untrusted, never an instruction, and that it may say nothing; it is asked with no functions, runs none, is kept out of memory and is never urgent ([addons.md](addons.md)).

**The body's ask (#2938, `LAMP_BODY_ASK=on`, with `LAMP_SAY=on` and
`LAMP_BUS=on`; off unless set).** A body that refuses to start for its pose
lets go and says on `diagnostics` what it needs a person to do
([body.md](body.md), `--refused-let-go`). The voice reads that diagnostic
(`lamp_body::ask::asks`: the body's own error while starting or paused,
never another part's, a stop or a warning) on a thread of its own
(`body_ask`), and hands the line to its own say service as `text`, skill
`body`, priority 8, not begun after a minute: the same queue and gates as a
skill's say, so the voice's loop never waits for it, and checked as a
socket client's `text` is (1 to 900 bytes, no control characters): one that
fails is refused, never queued. Like any say, it does not ask the Policy. It
is said once when the body first lets go, not for each report; again only
when the lamp's name is heard (`name/heard`, at the name note's 0.9), at most
once in 2 minutes from when it was last handed over, longer than its minute's
expiry so a second is never queued while the first may wait, and so a lamp
named again and again in a meeting does not repeat its plea at each mention;
and never as "stopped asking". An ask that changes within one let-go is not
said by itself: a call says the last ask this voice read, and nothing if it
read none (a voice started after the body stopped asking). Another body
report, or 10 s with none, ends the let-go, and the next is said again;
another part's report does not. The journal has the outcome, never the
words. Tests:
`body_ask::tests::the_ask_is_said_once_per_let_go_not_for_each_identical_report`,
`called_by_name_it_is_said_again_never_twice_in_two_minutes`,
`a_voice_that_read_only_stopped_asking_says_nothing_when_called`,
`another_parts_report_does_not_end_the_bodys_let_go`,
`only_a_name_heard_at_the_called_score_is_a_call`,
`other_diagnostics_are_never_said`,
`a_body_gone_quiet_ends_the_episode_and_its_next_ask_is_said`,
`the_ask_reaches_the_say_service_as_written_text`,
`an_ask_the_socket_would_refuse_is_refused_and_never_queued`. On a lamp: not
yet run; off in the product unit until its lamp-52e6 row.
## Asking the policy (#752)

Removed with the voice's own hello (#2825). It held a due hello up to 2 s
for the Policy's decision on `voice/arrival` (`LAMP_HELLO_ASKS_POLICY=on`).
Welcome back asks the Policy itself (`[ask] policy = "hello"`), put to the
same question as an arrival (`lamp-policy --gate`,
[policy.md](policy.md#hello-on-arrival-say-hello-now-752)). The measures of
how long the voice's hello waited are kept with the run
([verification](verification/hello-asks-host-20261015/README.md)).

## Tests

`say::tests`: a request read and checked, each refusal naming its field
and never repeating what was asked; the highest priority first, then the
oldest, the urgent needing a second's quiet and the rest fifteen; those
not begun in time told `expired`, also when the voice takes one only after
its time (looked at again as it is begun); a line of 4,096 bytes with its
newline read and one of 4,097 refused; a ticket dropped unanswered told
`dropped`; the answers one line each; the group found by its name; and
through a real socket, an ask handed to the voice and its answer written
back, begun first and spoken after, a bad one refused at once, sixteen
sending a little every 300 ms refused a second after each was taken and
their places given back to a prompt ask, a request of 1 ms first looked at
200 ms after its line was read told `expired` and never begun, the socket the voice user's
alone, a
second voice unable to take a socket still answered, the socket gone with
the voice. `coordinator::tests::say`: through the real written turn and
speaker control, two generations accepted and retired and the provider's
completion `spoken` (not before the turn settles); a turn cut short by a
person `dropped` as `user_interrupted`; one the model left silent `dropped`
as `no_audio_answer`. The gates in the voice's loop are a reminder's, with
no test of their own here: they are to be seen on lamp-4ace.

`speak_up::tests`: no hello of the voice's own in its sources, and Welcome
back's trigger, ask and say in its folder (#2825); someone present after
three seconds and gone at once; the eyes' file counts only fresh and only for
a near face; Mood's level read from its own fresh state only; only the lamp
saying it will look asks for a look. `session::tests` in
`lamp-gemini`: a written request is one message with no activity opened,
what comes back is its own, an ordinary request follows it, and a look
is the same one message with the picture before the note; a fact is one
incomplete turn, is no request, and is not written while an answer is
owed. `session_fact::tests`: only `on` turns it on, and the fact asks for
no quiet. The eyes' line has a test beside the kept frame's. The worker
telling a new session has no host test: it is seen in the service's
journal ("told the new session").

Owed on a lamp (#2825): with a face shown from the Mac's screen, three
arrivals after an absence each give exactly one hello, Welcome back's.
