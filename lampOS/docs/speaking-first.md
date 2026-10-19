# Speaking first

`crates/voice/src/speak_up.rs`, with `lamp-eyes --presence-file` and a written
request in `lamp-gemini`. Status on 2026-10-13: built, host tests, and the
provider's part tried on lamp-4ace without a sound being played. The lamp
has not yet said hello to anyone, nor taken a look on being asked. On
lamp-4ace the hello runs in its watching mode: the moments it would have
spoken are recorded and nothing is said.

## What it is for

Everything else the lamp says is an answer. A person at the next desk also
looks up and says a word when a colleague comes back. This is that, and
only that: one short hello to someone who comes and sits down. It is not an
announcement, it is not for people walking past, and it never goes into a
conversation.

## When

The eyes keep one line fresh while a face is near (`--presence-file`: the
face's height as a share of the picture's, no picture, nothing else; a file
not written for a second and a half means nobody). From that the voice
service decides (`Arrival`). A hello is due when all of these hold:

| | |
|---|---|
| someone is there | a face at least 0.14 of the picture's height, for three seconds without a break |
| they were away | nobody for five minutes or more before that. A face lost for under twenty seconds is someone who looked away |
| the lamp is at rest | nothing said to it or by it for half a minute, nothing being heard, a provider ready |
| it has not just done this | no hello in the last twenty minutes; for someone `close` (#1670, below), no hello in the hour before they arrived, so an arrival that began within the hour is not greeted at all |
| it is not paused | the keeper has not said `{"paused": true}` on `lamp/pause` ([keeper.md](keeper.md), "Named states and the pause") |

Someone already there when the service starts was never away and gets none.
A hello that is due but cannot be begun (the lamp is in a turn, or paused)
stays due until it can be. The pause holds every turn the lamp begins
itself the same way: the look it said it would take, the second look, a
reminder and news.

`LAMP_EYES_PRESENCE_FILE` in the voice service's environment names the
eyes' file; without it none of this runs. `LAMP_SPEAK_FIRST=on` lets the
lamp say the hello. Without that the moment is recorded (`hello_due` in the
session record, a line in the journal) and nothing is said.

On lamp-4ace it was only recorded from 2026-10-13 to 15, and in 680 voice
sessions it was never once due. Fifteen hours of what the eyes published
(a bus recording from 2026-10-14 18:23 to 10-15 08:46) say why: no face
reached the 0.14 of the picture's height that counts as someone at the
lamp (the largest were 0.11 to 0.14). At rest lamp-4ace's camera looks
across the side of the desk, and the people at the computer were at the
very edge of its view. Turning the lamp to face the seat is the fix; the first five minutes
([onboarding.md](onboarding.md)) has the lamp ask its person to do that. A
false "face" that the eyes report all night in one spot is #763. It is on since 2026-10-15, 07:34. Its voice side was tried at 07:59
the same day by writing a near face's size into the presence file by hand
for eight seconds, 21 minutes after the last person was seen: the lamp
said, unasked, "Oh, hey! Back again. It's good to have some company." The
eyes writing the file for a real person sitting down is still to be seen.

## How it is said

The lamp cannot be handed words to speak: its voice is the model's. So the
turn is a request like any other, with a written note in place of speech
(`ProviderInput::Written`, `InputSender::try_written`): one `clientContent`
message, complete, outside any activity, and whatever the model answers is
played like any answer. The note is:

```text
(Nobody has spoken. Someone has just come and sat down in front of you
after being away for a while. Say hello the way a friend at the next desk
would: a few warm words and no question they have to answer.)
```

By name when the lamp has one: the known face's at the lamp now
(`known_faces::hello`, [eyes.md](eyes.md)), else the name typed at setup
(`LAMP_PERSON_NAME`, `person::hello`: "… most likely “Example Name”, whose
lamp you are. Say hello … by name …", #806; quoted, so it reads as a name).
A known face's name wins, since it is the one at the lamp now; with neither,
the note above, with no name.

**Someone who comes by often** (#1670). Where Mood says how familiar the
remembered person at the lamp is (`familiar` in `mood/state`, from the eyes'
`lamp-eyes --visits on`, [mood.md](mood.md), [eyes.md](eyes.md)), the voice reads
it with the eyes' presence file, four times a second, from source `mood` only
and within its 2.5 s (`speak_up::familiar_in`). For `regular` and `close`, the
hello's note gains one sentence, the same hello one beat warmer:
"They come by often: greet them as you would someone you see most days,
warmly and without fuss." (`person::OFTEN`). For `close`, an arrival that
began within an hour of the last hello gets none: a friend wouldn't greet you
twice (the first builder on #1670, 12:19). It is judged from when they
arrived, so someone close is never greeted late while they sit there; a
glance away under twenty seconds is the same arrival (Tests on #1775).
Without a fresh level, or for `new`, the hello is as before. The level is
read when the hello comes due, three seconds after they arrive: if the eyes
have not recognised them by then, they are greeted as anyone. The hour counts
any hello by the lamp, not one to this person, since Mood carries no identity:
someone close who comes half an hour after a regular was greeted gets none. Tests: the
level read from Mood only; `close` greeted at 0 and 78 minutes, not at 26 or
52, where anyone else is greeted at each; someone close who glances away
once the hour is up is not greeted mid-sitting (Tests' test); and the warmer sentence only for
`regular` and `close`. The one line in `coordinator.rs` that passes the
level into the hello's words has no test of its own: nothing tests the
coordinator's hello. The typed name is also told to the model after
its character, by the provider and second workers that build it (the voice
passes them `LAMP_PERSON_NAME`), only as the name the person gave, never as
who is at the lamp: "… gave their name as “Example Name”. Whoever is at the
lamp may be someone else …" (`person::told`). So a skill's "by name if you
know it" (Welcome back) can use it. It is one line of at most 64 characters,
setup's own limit, with no control character, line separator or direction
mark; anything else, or no file (before setup, after a reset), is no name,
and the voice says why without saying the name. Each time it is read (when a
worker builds the instruction, and at each hello), the journal says so
without the name: `the person's name from /etc/lampOS/person/name is used
(12 characters)`. The first five minutes (#841) run in the same session, so
their notes come after the same instruction.

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
  answer) is the lamp asking for its eyes: `look_asked` in the record.
  Only in a turn someone spoke: in one the lamp wrote itself (its own look,
  a hello, a "Sorry?", a reminder) the words ask nothing, so looks never
  chain with nobody speaking (`coordinator::own_turn`, the worker's
  `heard` rule; cleared with each service cycle, whose turns number from 1
  again).
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
  second route has taken over or the calling session has been renewed;
  once dropped, the second route's own "Let me take a look." on that turn
  asks for a look of its own. Gemini Live only: the second route sends
  no `LookAsked`.
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
is told what became of it (`crates/voice/src/say.rs`). The hello, task
news and reminders are to move onto it.

**The socket** is `LAMP_SAY_SOCKET`, `/run/lampos-say/say.sock` if unset,
in a directory of its own: the voice runs as root and its own runtime
directory is closed to others. With `LAMP_SAY_GROUP` (a group's name, as in
`/etc/group`) the directory is 0750 and the socket 0660, both that
group's; without it both are the voice's user's alone (0700, 0600). A
socket left by a voice that has gone is taken over; one that still answers
is not. One JSON line in:

    {"skill": "reminders", "note": "Remind them, in one short line, to call the bank.", "priority": 8, "expires_in_ms": 120000}

- `skill`: 1 to 64 of `a-z`, `0-9`, `-` and `_`. Logged and traced only.
- Exactly one of `note`, an instruction the model phrases (as the hello's
  and the news'), `text`, words to say as they are (the model is told
  "Say exactly these words, in your own voice, and nothing else:" before
  them), at most 900 bytes with no control characters, `from`, words
  the voice keeps itself (below), and `sound`, a skill's own short sound
  (below).
- `priority`, 0 to 9, 5 if left out: higher first, the oldest first among
  equals.
- `expires_in_ms`, 1 ms to an hour, from when its line was read: not begun
  by then, it is `expired`. Its time is looked at again as it is begun, so
  that none begins after it, however long it waited for the voice's loop
  (the reviewer on #954). A line is at most 4,096 bytes with its newline.

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
dBFS; the head tap's "hm?" is -35) and no sample past -12 dBFS, so that a
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

**When it is said.** After a due reminder and before the hello and the
news, through a reminder's gates: no reply owed or queued, the speaker
free, nobody's utterance pending, the microphone's capture fresh and a
second since the last speech, the playback mask free, a ready session and
the keeper's pause off; and a quiet since the last turn of 15 s (as the
news), or 1 s from priority 8 up (as a reminder). It is a written turn like
the others (`skill_says` in the record, with the skill and the priority;
never the words), the lamp's own turn to the attention rule and the
Policy's hold. The Policy is not asked here: whoever asks the voice to
speak has asked the Policy's gate first ([skills.md](skills.md)).

**What the voice keeps: `from` (#939, (c), the first builder's 12:32).**
`{"skill":"reminders","from":"schedule","priority":8,"expires_in_ms":30000}`
asks for the reminder Schedule has due; `"from":"tasks"` for the news of
work handed over. The skill says when, and moves and lights with the
words; the voice keeps the words and their delivery, as it always did: a
reminder's durable acknowledgement, news told once. `lamp-skills` sends it
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
  the say service's own says, the hello and the news wait for it: a due
  reminder comes before anything else the lamp says unasked, as it did
  without the switch (Fourth on #1038).
- Tests: `say::tests::an_ask_for_what_the_voice_keeps_waits_for_its_own_source`,
  `coordinator::tests::say::a_due_reminder_or_news_waits_for_a_skill_at_most_its_time`,
  `coordinator::tests::reminders::a_reminder_a_skill_asked_for_is_answered_on_the_acknowledgements_own_proof`,
  `reminders::tests::on_and_skill_turn_schedule_speech_on_and_nothing_else_does`,
  `coordinator::tests::reminders::a_reminder_held_for_its_skill_keeps_other_unasked_speech_waiting_while_it_is_offered`.
  The last is of the condition only: the three places it holds (the say
  service's turn, the hello and the news, in the coordinator's loop) are
  driven by no test, as no harness runs the loop with all three (Thirteen
  on #1038). On a lamp: with `LAMP_SCHEDULE_SPEAK=skill` and `lamp-skills`
  stopped, a say sent in the 10 s a due reminder is held is said after the
  reminder, never before (`no skill asked for the reminder due within
  10 s` comes before `saying what the skill … asked` in the journal).

**Most likely wrong:** a say of priority 8 or more in the second after a
conversation's last turn: a reminder takes that second today, and a skill
may not need it.

## Asking the policy (#752)

Built by the second builder on 2026-10-15. **Off unless the voice starts
with `LAMP_HELLO_ASKS_POLICY=on`**; it needs the presence file as before.
`LAMP_SPEAK_FIRST=on` stays the switch for the lamp to say anything first.

The owner: the policy is the part that decides when the lamp speaks first.
With the switch, a hello that is due and could be begun (the rule above,
nothing owed, a provider ready) is not said at once:

1. The voice says so on `voice/arrival` (`lamp_msgs/VoiceArrival`): the
   arrival's number (the voice's clock in microseconds), minutes away,
   seconds there, seconds of quiet, and minutes since its last hello if it
   has said one since it started. No picture, no face, nobody's identity.
2. It holds the hello up to 2 s (`speak_up::ASK_FOR_US`) for the policy's
   decision about that number on `policy/decisions` (`about` `hello`,
   `lamp-policy --hello`, [policy.md](policy.md#hello-on-arrival-say-hello-now-752)):
   - `say_now`, acting: said, as before (with `LAMP_SPEAK_FIRST=on`), by
     name for someone the lamp knows ([eyes.md](eyes.md)); the policy is
     never told the name;
   - `wait`, acting: not said, and recorded (`hello_due` with
     `"policy":"wait"`, and "the policy said not now" in the journal);
   - a decision in shadow: the rule above, at once;
   - none within 2 s: the rule above.
3. Either way the hello counts as made for that arrival: no second asking.
   An answer the lamp could not act on within 10 s (it was in a turn) is
   old, and the policy is asked again. Someone new arriving drops what was
   asked for the one before.

The session record gets one line for each answer acted on:
`{"kind":"hello_policy","at_us":…,"arrival":…,"answer":"say_now|wait|shadow|none","waited_ms":…}`,
and the journal "a hello is due; asking the policy (arrival N)" and "the
policy's answer to the hello: say_now after N ms". What the policy decided
and why is in its own log.

**Without the switch** nothing here touches the bus (no `voice/arrival`
publisher, no subscription), the hello is the rule's on the very tick it
is due and ready, and the session record and journal are as they were:
`without_the_switch_the_hello_is_as_it_was_and_nothing_is_said_on_the_bus`
and `speak_up::tests::without_asking_a_ready_hello_is_the_rules_at_once`.

**How long a hello now waits**, measured on the development Mac (Apple M2
Max, macOS 26.6.2, release build), with `cargo run --release -p lamp-voice
--example hello_wait -- --samples 30`: from the first call that says the
arrival to the call that takes the policy's `say_now`, the voice calling
every 2 ms as its loop does, through a bus in `/tmp`, the policy's own loop
and a stand-in judge that answers at once: **31.6 ms in the middle, 38.0 ms
at most** (33.2 ms the first) in one run of 30, and 22.4 ms in the middle,
36.3 ms at most in a second. Without the switch it waits nothing (0.0 ms,
the same call). On the lamp Jev's own time comes on top: 0.5 to 1 s as
measured from lamp-4ace for the attention question, and never more than
1.5 s, since the hello question's asking is cut there and the policy's
rule answers in its place; past 2 s the voice's rule does. So a hello comes
about 0.5 to 1 s later than before. These are the Mac's numbers, not the
lamp's ([verification](verification/hello-asks-host-20261015/README.md)).

**On the lamp** (the first builder; the smallest step first):

1. Install the ARM64 run's `lamp-voice` and `lamp-policy` with the switches
   as they are, and see the minute out.
2. Shadow first: add `--hello` to `lamp-policy`'s `ExecStart` and
   `Environment=LAMP_HELLO_ASKS_POLICY=on` to `lamp-voice.service`,
   `sudo systemctl daemon-reload`, restart both. `journalctl -u lamp-policy
   -n 5` says `hello=true`; `lamp-bus info voice/arrival` lists the voice as
   publisher and `policy.` as subscriber.
3. Make one hello due, as at 07:59 on 2026-10-15: with nobody seen for
   five minutes and no hello for twenty, write a near face into the
   presence file for eight seconds:

   ```sh
   for i in $(seq 16); do echo 0.30 | sudo -u orangepi tee /run/lampos-eyes/present >/dev/null; sleep 0.5; done
   journalctl -u lamp-voice --since -1min | grep -E 'hello|policy'
   journalctl -u lamp-policy --since -1min | grep 'hello decision'
   ```

   Expected: "asking the policy", "the policy's answer to the hello: shadow
   after N ms" (N about Jev's time), the lamp's hello as before, and one
   `hello decision` line.
4. Then act: add `--act hello` beside `--hello`, restart the policy, and
   repeat step 3 after twenty minutes. "say_now after N ms" and the hello,
   or "wait" and none; a second try within twenty minutes is a `wait` with
   `guard=greeted_recently`.
5. Hand back N for each, the lamp's words, and the policy's decision lines.

**Most likely wrong on the lamp.** That the half second to a second of
Jev's time reads as the lamp hesitating, not as it noticing; that `wait`
from Jev, asked a question it was not built for, keeps a hello a person
would have liked; and the eyes: no real person has yet made a hello due on
lamp-4ace ([why](#when)), so this has been tried only by the written file.

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

`speak_up::tests`: one hello to someone back after five minutes and none
while they stay an hour, looking away now and then; none for someone there
at the start, away two minutes, walking past, or arriving into a room in
conversation until it has been quiet for half a minute; none again within
twenty minutes; a hello that could not be begun stays due; the eyes'
file counts only fresh and only for a near face; only the lamp saying
it will look asks for a look; and, asking the policy, a ready hello is the
rule's at once without the switch, is held for its own arrival's answer
(another arrival's is not taken), takes `say_now`, `wait` or a decision in
shadow as above, keeps an answer that comes while the lamp is busy, and is
the rule's after 2 s without one. `tests/hello_asks.rs` runs the policy's
own loop with a stand-in judge: an arrival, a `say_now`, one hello, and a
`wait` for the next within twenty minutes; a `wait` kept; someone talking,
a `wait`; a policy in shadow or silent, the rule. `session::tests` in
`lamp-gemini`: a written request is one message with no activity opened,
what comes back is its own, an ordinary request follows it, and a look
is the same one message with the picture before the note; a fact is one
incomplete turn, is no request, and is not written while an answer is
owed. `session_fact::tests`: only `on` turns it on, and the fact asks for
no quiet. The eyes' line has a test beside the kept frame's. The worker
telling a new session has no host test: it is seen in the service's
journal ("told the new session").

Not shown: the lamp saying it. What a hello sounds like in a room, whether
five minutes and twenty are the right numbers, and whether a face of 0.14
is a person at the desk and not one walking behind it, are to be seen with
people; the watching mode is there to count the last of these first.
