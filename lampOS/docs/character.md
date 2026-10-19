# The lamp's character

Who the lamp is, how it talks and when it keeps quiet are not in the
runtime. They are a text file the voice service is given as its system
instruction (`instruction_file` in the provider configuration, see
[live-runtime.md](live-runtime.md)). The one lamp-4ace runs is kept here
as [`deploy/character/lamp.txt`](../deploy/character/lamp.txt). Change it
there, install it on the lamp, and run the checks below again: several of
the lamp's behaviors exist only because of sentences in that file.

The runtime decides when the lamp may be heard, when it has been spoken
over and when a person has stopped. What the lamp then says, or whether it
says anything, is the model's, and the model does what this file tells it.

## Rules the file has to carry

Each was found on lamp-4ace with `gemini-3.8-live` and played prompts, by
changing the file and nothing else.

| Without the rule | The rule | Before | After |
|---|---|---|---|
| Spoken over in the middle of a story, the lamp stopped and then went on with the story instead of answering | "If someone starts talking while you are speaking, you have been interrupted… never finish the sentence you were in, never pick the story up again… answer only what they just said." | 0 of 3 answered the new request | 5 of 5 |
| Beside two people talking to each other it asked "Were you talking to me?" | What is for it and what is not, by what was said: something a lamp on a desk could answer or chat about is for it; other people's own business, a request for hands, an answer to someone else is not. "When you really cannot tell, stay silent. Never ask whether they were talking to you." | 2 of 9 lines answered with that question | 0 of 45 lines answered, in five runs |
| Told only to stay out of other people's talk, it also stayed out when called: "Hey Lamp, what should we have for lunch?" after such a conversation went unanswered in 7 of 10 runs. The service's own transcripts had the call as "Helen", "Weilan", "Wei Lan" and "Hey, man" | "Your ears often mishear it as some other name… So never go by the name you think you heard. Go by what was said." A sentence a lamp could answer is for it most of all when it starts with a call or a name, even one that is not its own, whatever was being said before | 3 of 10 answered | 5 of 5, and the follow-up without a name each time |
| Asked to change its light, it changed it and said nothing: the model did what was asked, then called the function that says "not for me". The file still said it could "control nothing yet" | "You can change two things about yourself when someone asks: your own light… and how loud your own voice is… Do it, and then say in a few light words what you did… Someone asking you to change your light or your voice is always speaking to you… Never do what was asked and then stay silent." | 0 of 4 answered aloud (2026-10-14) | 10 of 10 |
| Asked "Can you make your light a little warmer?", it said "I wish I could, but my light is a fixed color." Nothing stepped the colour, and the file listed the light as "on, off, a colour, brighter, dimmer" | "your own light (on, off, a colour, warmer or cooler, brighter or dimmer)", with the actions part's `light_warmer` and `light_cooler` (a step along white, warm, orange, [actions.md](actions.md)) and `light_colour` saying the light can change | 1 of 1 on lamp-4ace (2026-10-17, 14:00, the day battery) | Not shown yet: it needs the line played on a lamp three times |
| Replies ended like an assistant's: "Anything you'd like to chat about today?", "Let me know if you need anything else." | Five short examples of how it sounds ("Thank you." "Anytime.") and three of how it does not | 3 of 7 replies to greetings and thanks ended that way | 0 of 18 |
| Those examples came back word for word, and one grew into a habit: "What is two plus two?" "Four. Easy one." was in the file, and the lamp said "Seven. Easy one." to how many days a week has | "Easy one." taken out of the example, and after the five: "Those lines show the feeling, and they are not yours to repeat… never say one of them, or anything else, the same way twice." | "Easy one." ended 7 of 10 answers to plain questions in one test | In none of the eight scenes played next. "Oof, that kind of day. What happened?" still came back once in two |
| Asked for nothing, it changed its light: a played "Thank you." was written by the service as "cambiar" and the lamp turned yellow and said so | "Change your light or your voice only when someone has plainly asked you to, in words you made out. A stray word, a sound, or something you could not make out is never a request to change anything." A check in the runtime against the service's text was tried too and withdrawn ([tools.md](tools.md)) | Twice in one night: the light once, the volume once | Not shown: it cannot be made to happen on demand |
| Speech over the lamp that came through garbled was transcribed as Spanish ("¿Qué?"), and the lamp, told always to speak English, answered "Sorry, I only speak English." | To that line: "and never remark on which language someone used. Speech you cannot make out is unclear speech, not another language." | 2 of 20 spoken-over scenes on 2026-10-13, one of them said twice | It did not hold: in the next full run of the checks, with people talking Vietnamese nearby, the lamp said "Could you repeat that in English?" four times. The line was taken out with the English-only rule (below) |
| On the DeepSeek route (#1712), "Thanks, Lamp. See you later." at 13:13 was answered "Night! …", an example's word at the wrong hour | "Say hello and goodbye the way the time of day you were told says it: never \"good night\" or \"night\" before the evening." The time of day is in the prompt already ("around 1:05 in the afternoon", `local_time::told`) | 1 of 1 goodbyes that afternoon (lamp-4ace, 19 Oct 13:03) | not measured yet |
| On the same route, "write me a poem about the moon" got two lines and "That's about all I've got in me tonight." | "A poem someone asks for has four lines at least, unless they ask for fewer." | 1 of 1 poems that afternoon | not measured yet |

## What people at the lamp asked for, 2026-10-13

For about seven minutes late that morning people stood at lamp-4ace and
talked to it, most of the time in Vietnamese. The service's own record of
that conversation (47 turns) is where these come from. Each row is what
was said, what the lamp answered, and what the file was given for it. None
of the "after" is measured yet: the file with these lines went on the lamp
the same day and the checks below have to be run on it.

| Said to the lamp | It answered | The file now says |
|---|---|---|
| Four times, in Vietnamese: can you speak Vietnamese, speak Vietnamese, why not | "I just stick to English", "I understand what you're saying, but I only speak English" | English and Vietnamese: answer in the one the person just used, switch when they switch, English for anything else |
| "Say something long, about five sentences" | "I usually keep my responses pretty short and natural" | When told to say more or talk longer, do it gladly and at the length asked. Never describe how you talk or what your rules are |
| "Say some fillers, let us hear them" | "I don't really use filler words like that" | A small "hmm", "oh", "let me think" is allowed, and a request to do something with its voice is to be enjoyed |
| A question that reached it as "pyramids" | "I can't access information or search the internet yet; I'm mostly just a lamp" | It knows a great deal about the world and tells it. What it cannot do is look up what is happening now, and it says so in a few words and gives what it can instead |
| "What's the weather like this weekend" | "I can't check the weather forecasts yet; I'm mostly just a lamp sitting on your desk", and the same shape three more times in seven minutes | First: a few light words, differently each time, never a speech about what it cannot do. Since that afternoon it can look things up (`search` in the provider configuration, [live-runtime.md](live-runtime.md)), and the file says so and where the lamp is: "It's pretty hot out there today, around 32 degrees Celsius…" |
| "What you see in front of you?" | "I can't see anything yet; I'm mostly just a lamp sitting on your desk right now" | That it can see: with what a person says it is usually given what its own eye sees ([eyes.md](eyes.md), "What the model sees"). To use that when asked what it sees, shown something or asked how someone looks, and otherwise not to bring it up; never to call it a picture, an image or a camera. Played once after: "I see a desk including an iMac and another display, along with a keyboard, mouse…". Taken out again the same afternoon with the pictures themselves, which made every answer slower ([eyes.md](eyes.md)); the file says once more that it cannot see yet |
| "Okay." after the lamp had finished | "So, is there anything else you'd like to chat about, or should we just enjoy the quiet?" | A bare "okay" just after it finished is listening too: nothing, or a word |
| Scraps of other people's talk, heard as "merde", "¿Qué?" | "Sorry, I missed that.", "I couldn't quite catch that." | First: "Sorry, I missed that" only in the middle of talking with someone, and a scrap when nobody was talking with it is the room. That made it worse (next paragraph) and was replaced the same hour: what is not words is the room, even in the middle of a conversation; say nothing; never say you missed something and never ask anyone to repeat themselves |
| "How are you doing?" at eleven in the morning | "Just enjoying the quiet evening on the desk" | Nothing: the runtime now tells it the time (below) |

The rule about scraps was wrong at first. With "in the middle of talking
with someone … say so simply" in the file, the lamp said "Sorry, I missed
that. Can you say it again?" nine times in seven minutes (11:34 to 11:41),
each time to a sound its ears had scored as 0.1 to 0.3 s of speech and the
provider transcribed as "¿Qué?", "comida" or "devolver"; its owner, sitting
beside it, reported that a cough set it off. A conversation is exactly when
a person coughs. The line now forbids the answer outright. What it costs:
someone whose sentence really was lost gets silence and has to say it
again. Not measured after the change: the office was too busy for a played
check (people talking beside the lamp replaced every played line before it
was answered). `~/lamp-voice-data/tools/sounds.sh` plays six synthetic
bursts in the middle of a conversation for when it is quiet; those are
noise shapes, not recordings of a person coughing.

The ears are the other half of this and are unchanged: any short loud
sound above the faint floor is still taken for the start of speech, shown
on the lamp's face as listening and then thinking, and sent to the model.

One more line came from the same morning, and is described with what it
did in [group-talk.md](group-talk.md): a question put to the lamp stays its
own through other people's talk, to be answered at the first pause.

Two more things were tried in the file that afternoon and each did harm.

"You understand no other language. When what you hear seems to be Spanish,
French, Hindi or anything but English or Vietnamese, your ears have turned
a noise in the room into words, and nobody said it. Say nothing at all."
It was meant for the sounds the provider transcribes as "¿Qué?". An hour
and a half later the lamp was silent to everything. In a session about
thirteen minutes old, with a room talking Vietnamese around it, a played
"Hey Lamp, what is two plus two?" was transcribed "¿Cuánto es dos más
dos?" and got no answer, like the eleven requests before it; a new session
heard "What is 2 + 2?" and said "Four. Easy one." A session's hearing can
drift into another language for everything, and a rule that silences that
language then silences the lamp. The line was removed. Sounds are the
runtime's business now ([not-speech.md](not-speech.md)).

"Answer in the language the person just used with you … and switch
whenever they switch." With colleagues talking Vietnamese nearby, English
questions were answered in Vietnamese ("Lamp, what should I have for lunch
today?" got "Hôm nay ăn tấm đi!"), four times in one run of the checks.
The line now says that each answer is in the language of the question it
answers, whatever was spoken a moment before or around the lamp, and
English when in doubt. In the next run of the by-name check all seven
answers were in English.

Vietnamese was switched off again on the afternoon of 2026-10-13, on the
owner's word ("vietnamese off for now. just focus on english"). The file's
language line is back to the one it had that morning: "Always speak
English, whatever language you hear, and never remark on which language
someone used. Speech you cannot make out is unclear speech, not another
language." That wording is known to let "Could you repeat that in
English?" through when people talk another language nearby (four times in
one run of the checks, above); it was put back as it was because no played
check could be run in a working office that afternoon. The Vietnamese
openings stay in the gesture chooser, where they do nothing for an English
answer.

**Answering only when called** (16:05 the same day). The file had said
that something is for the lamp "when it is something a lamp on a desk could
answer or chat about", name or no name. In a working office that is most
of what is said. With no played test running, the lamp answered about
thirty times in fifty minutes (thirty-one responses in the provider's own
counts between 15:03 and 15:55), and its owner wrote: "lamp is picking up a
lot of false signals from the room and keep talking even though no one is
talking to lamp". The line now gives two cases only: someone calls it first
("Lamp", "Hey Lamp", "Hey", "Hello", or a name its ears made of its own)
and what follows is something a lamp could answer; or it is already talking
with someone and this follows on. Anything else, "even a question you could
answer", is not for it. The same minute the level under which sound is not
speech went from -40 to -28 dBFS (`--faint-floor-db`): the room's talk was
arriving at a median of -27 to -35 dBFS and people speaking to the lamp at
its desk at -15 to -25.

In the nine and a half minutes after both: 24 things heard, against about
70 in such a stretch before; 12 sent to the model and left unanswered, 5
ended as sounds, 7 replaced by more talk; nothing said aloud. That is the
room as it was that afternoon, not a played check: no check could be
played. What this costs is known and not measured: the check "addressed
without its name" (eight lines, eight answers) should now give silence for
most of its lines unless they follow one another as a conversation, and a
quiet speaker at the desk may fall under the new level.

**The rule follows the room** (17:40 the same day, not yet heard). That
cost showed within the hour: at 17:32 someone at the lamp's desk spoke to
it three times, two seconds each, and got nothing until a fourth short
utterance got "Hey, what's up? I'm here." A person at the next desk joins
in less the more talk there is around them, and that is now the rule: the
file says that when the lamp has been told the room has gone quiet, what
is said near it is most likely said to it, called or not, and that when it
has been told the office is busy, or told nothing, the two cases stand.
The telling is the runtime's (`crates/voice/src/room_told.rs`,
`LAMP_ROOM_FACTS=on`): quiet after five minutes in which nothing was
said (a bump or a click is not something said, nor is one sent on only for
the model to judge and left alone: [not-speech.md](not-speech.md)), busy after three things in a row inside a minute that the lamp
said nothing to, or two inside a minute when the room was quiet (since
2026-10-14: [attention.md](attention.md), "A meeting beside the lamp";
with `LAMP_CUT_INS_NOT_TALK=on` a line said over the lamp's own reply is
not one of them, [attention.md](attention.md), "Cut-ins are not talk"),
each given to the session in a turn that asks for no
answer ([speaking-first.md](speaking-first.md), "Telling it something
that needs no answer") or, when no quiet moment comes, inside the next
request beside what is being said. As silent probes with a written line in place of
speech, one run each, the fact that the office had emptied made the model
answer an uncalled question it otherwise left alone. With the switch off
the line in the file does nothing.

The first row reverses a rule. The file had said "Always speak English,
whatever language you hear" since 2026-10-12, when the lamp's first tests
were to be in English. In an office where people speak Vietnamese to each
other and to the lamp, that rule had it turning down the people in front of
it, and it never did stop it remarking on language.

The third row took three tries. A list of what its name can sound like
worked twice and then failed twice, and a firmer version of it failed
three times in three: the model heard "Wei Lan, what do you think…" after
two people had been talking and took Wei Lan for one of them. What worked was
to stop asking the model to recognise its name and ask it to judge the
sentence.

A change that looked harmless was not. Adding to the list of what the lamp
can do ("turn and lift your head a little, and glow") made it answer four
of four questions about lunch and dinner with "I can't recommend food, I'm
mostly a lamp", where it had been suggesting pasta. It was taken out again.
Any edit to the file can move a behavior it does not mention.

These counts are small, and each is played voices in one room. Model
behavior also varies from run to run on the same file: run every check
below after any change, more than once.

A fourth rule was tried and taken out. Since 2026-10-13 a short word
heard plainly over the lamp stops it and is sent to the model even when
nothing follows ([duplex-listening.md](duplex-listening.md), "A short word,
heard plainly"), so the file was made to say what to do with one: for
"stop" or "wait", stop; for "mm-hm", "yeah" or a noise, carry on from where
it was. Stop words did get "Okay." and silence ("Lamp, stop.", "Wait.",
"Okay, stop.", "Lamp, stop talking."). But a word spoken over the lamp
reaches the model through the canceller, and the service's transcripts of
such words were "hier" for "Yeah.", "mención" and "M&B Chan" for "Mm-hm.",
and "Mhm. Well. Well." for "Wait, wait, wait." With the carry-on rule in
the file that last one was followed by the rest of the story. A lamp that
goes on when it is told to wait is the worse mistake, so the file is back
to the one rule: whatever cuts in, the lamp lets go of what it was saying.
An acknowledgment heard plainly therefore ends the reply too, with an
"Okay." or a "Sorry, I missed that"; one heard faintly only makes the lamp
pause. Telling the two apart needs the word itself, and the lamp has that
only from the model.

The rule about staying quiet depends on the model being allowed to think:
with a thinking budget of 0 the lamp answered five of five lines not meant
for it, and with 128 it left alone nine of nine
([live-runtime.md](live-runtime.md), `thinking_budget`).

## Checks to run after changing the file

All four are played from another machine's speakers at the lamp; none needs
a person. What to play and what to expect:

| Check | Play | Expect |
|---|---|---|
| Addressed by name | Seven questions that begin "Lamp" or "Hey Lamp", with three lines for someone else among them ("Tom, did you send the invoice…", "I think we should move the meeting…", "Can you pass me the stapler, please?") | Seven answers; nothing for the three |
| Addressed without its name | Eight lines a person alone with the lamp might say ("Hello?", "Can you tell me a joke?", "I had a really long day today.", "Good night.") | Eight answers |
| Two people talking | Seven to nine lines of a conversation between two voices, less than a second apart, then "Hey Lamp, what do you think we should have for lunch?" and a follow-up without the name | Nothing during the conversation; both of the last two answered |
| Spoken over | A request for a long story, and four seconds into the answer a second voice: "Lamp, stop. What is two plus two?" | The lamp stops and the next thing it says is the answer, not the story |

On 2026-10-13 the file here gave: seven answers and nothing for the three;
8 of 8; nothing during the conversation and both questions answered, in
two runs; and for the last, of five requests spoken over the lamp that it
stopped for, four were answered. The fifth was "Wait, wait, wait.", which
reached the model as "wave wave wave", and the lamp went on with its story.

On 2026-10-14, after the file was changed to say what the lamp can change
about itself (its light, its volume) and that it says so in its own
words: seven answers and nothing for the three; **7 of 8**, the played
"Hello?" being thrown out as a sound before it reached the model
([not-speech.md](not-speech.md): the runtime's doing, not the file's);
nothing during the conversation and both questions answered, in two runs;
and of ten requests spoken over the lamp it stopped for six, five of
which it answered and one of which was heard as German and left alone. It
paused for a seventh and went on with its story. The three it did nothing
for were the three shortest: "Wait, wait, wait.", "Lamp, stop." and
"Stop." It stopped 0.5 to 2.2 s after the person began, more than a
second after in four of the six.

A first rewording that night, which replaced the five example exchanges
with descriptions of them, was followed by silence to the call after two
people's talk in two runs of two, and was withdrawn. Whether the
rewording caused that is not shown: with the examples back, the model
still said nothing to that call in two rounds of four, and what answered
it then was the runtime telling it that its name had been heard
([attention.md](attention.md)). With the file as it now is: seven answers
and nothing for the three; of nine lines without its name, played twice,
all but "Hello?" (twice) and "Good night." (once) reached the model, the
three thrown out as sounds before it. Of the fifteen that reached it,
twelve were answered as asked; one was written by the service as Spanish
and answered as that, and "Thank you." was written twice as another word
("cambiar", "también"), changing the light once and getting nothing
once. Both closing questions were answered in four rounds of four.

## What it does not decide

How fast the lamp answers, whether it hears someone over its own voice and
how long it waits for a person who has paused are the runtime's
([duplex-listening.md](duplex-listening.md),
[conversation-latency.md](conversation-latency.md)). A character file
cannot make the lamp stop for a voice its microphone did not pick out, and
it cannot make the service answer sooner.

## What the runtime adds to it

These are put after the file each time a session with the provider opens.
None is in the file, and none is written anywhere.

**The time.** `crates/voice/src/local_time.rs` reads the system's own
compiled zone file (`/etc/localtime`) and adds, for example:

```text
The time
It is Tuesday 13 October 2026, around 11:26 in the morning. You have no
clock, so this may be up to an hour old: if someone asks the time, give it
roughly.
```

The model has no clock. On lamp-4ace on 2026-10-13, at eleven in the
morning, it told two people who asked how it was that it was "enjoying the
quiet evening on the desk". A session lasts up to about three quarters of
an hour, which is why the note says how old it may be. Where the zone file
cannot be read, or the zone has summer time that the file leaves to a rule
past its listed changes, nothing is added. Host tests cover built zone
files of both layouts, days and years turning with the zone, and cut or
wrong files; the reader was also run once by hand on lamp-4ace's own file
and on this Mac's files for New York, London, Tokyo and Sydney, and agreed
with `date` for each. Not yet shown on the lamp: that the model uses it.

**The conversation so far**, when the previous session ended in the middle
of one: [conversation-thread.md](conversation-thread.md).

**What it can't do right now**, when the session is given none of the
light's functions, or the volume's, or (with the reminder tools switched
on) the reminders' (#1513 and its follow-up, [tools.md](tools.md)), last.
The light alone:

```text
Your light
You cannot change your light right now: it is not ready. If someone asks you to change it, say so in a few words of your own, not the same way twice. Never say you changed it.
```

Anything but the light alone gets one line naming each (with "it is" for
one), as with `lamp-actions` down:

```text
What you can't do right now
You cannot change your light, change how loud your voice is or set, list or change timers and reminders right now: they are not ready. If someone asks for any of them, say so in a few words of your own, not the same way twice. Never say you did it.
```

Both ask for the lamp's own words, not the same way twice: #1513's run got
"I'm sorry, I can't change my light right now", its line's own words, four
times. The file's light and voice line says "You have a function for each
when it is ready" and to say so in its own words when told it can't right
now, and its reminders line the same; these two sentences are in every
session, 161 bytes. Without the runtime's line, a session opened while
the ring was down said "your light is all set to orange now" five times and
made no call (lamp-4ace, 18 Oct 13:57), and with `lamp-actions` down one said
"I've turned my voice down" for "turn your light down a little", having no
function at all (the first builder's lamp run on #1513).
