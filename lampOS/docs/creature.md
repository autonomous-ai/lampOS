# The lamp as a creature

The owner, 2026-10-14, 17:22: "reimagine the entire experience. make lamp
the most fun creature to interact with. discover all the sensors on lamp.
the latest ones." And, about V1's mapping of a head touch to "pet": "most
of the v1 designs make no sense". This document is the design every part
of lampOS works toward. It is not V1's, and nothing here is kept because
V1 did it. Written by the first builder; each behaviour below becomes a
task for the part that owns it.

## What it senses: lamp-4ace, found on the lamp on 2026-10-14

Read from the running lamp (USB, I2C, GPIO, sound and video devices, power,
thermal), not from V1's notes. Where a sensor is "not read", nothing in
lampOS listens to it yet.

| Sense | What it is | Read by | Status |
|---|---|---|---|
| Sight | The ZVCAI USB camera, in the head at the centre of the light ring, so it moves with the head; up to 3264 x 2448 (MJPEG) | Eyes (`lamp-eyes`): faces, where, whether turned to the lamp | Running |
| Hearing, speech | mic2, the Jieli dual microphone in the base (its own echo filter, beam and noise cancelling) | Voice, Name | Running |
| Hearing, the room | mic1, the plain capsule in the base on the C-Media adapter | Ears (`lamp-ears`): level, quiet, sudden sounds | Running since 16:13 |
| Hearing, from the head | The camera's own microphone: two channels at 22.05 kHz, 8 bits, but the left is silent (six seconds on 2026-10-14: left RMS 0.00, right 0.71) | Nothing | One capsule: no left and right, so no direction from sound. At most a level at the head |
| Touch, head | Two pads: chip 0 line 96 and kernel gpio-37 both change with a touch (the owner's hand, 17:24) | Controls | Running; both read, reported as the head; meanings to be redone (#711) |
| Touch, base | A twelve-electrode strip E0..E11, MPR121 on I2C bus 0 at 0x5A; the hardware team's 20:05 answer confirms faulty E4/E8 on lamp-4ace. Its working set is 0-3, 5-7, 9-11; every pad needs its own thresholds through the shell | Controls, opt-in `--touch` (#711) | Per-unit working sets preserve full-span slide positions across excluded pads; installation/calibration and applied responses remain First's lamp qualification |
| A privacy switch | Chip 1 line 9 (kernel gpio-361): V1's microphone slide switch, "not wired" in its notes; the voice reads it as its physical privacy input and it reads high (unmuted) | Voice | No switch is fitted; a touch on the base does not change it |
| The air | Sensirion SEN63C on I2C bus 0 at 0x6B: particles, humidity, CO2 (its temperature is the base's, warmed by the lamp) | Air (`lamp-air`) | Running since 17:43; 1,870 ppm CO2, "stuffy", in the office at work |
| Its own body | Five servos: where each joint is, its load and warmth, 50 times a second | Body | Running. A hand pushing the head shows up as load: the body can feel being moved |
| Time | The board's clock and a battery-backed clock (HYM8563) | Schedule, voice | Running |
| Nearby phones | Bluetooth (`hci0`) | Nothing | Not read. Could notice a known phone arriving or leaving, but only once that phone is paired with the lamp, which waits on how a lamp is set up (not decided; the owner, 2026-10-15) |
| Its own warmth | Eight thermal zones, a "skin" zone among them | Health | Running |
| A touch controller | A Goodix GT9271 declared on I2C bus 5, no driver bound | Nothing | Not known to be fitted; a read of its product ID failed ("invalid argument") |
| Power key | The power chip's key (`axp8191-pek`) | Nothing | Not read |

No battery is fitted (no power supply reported). The board has an NPU for
models run on the lamp itself. Its outputs: five servos, a ring of 32
individually lit pixels around the camera (`spidev3.0`), and two 3 W
speakers through the C-Media adapter. The whole unit, part by part, is in
`robots/lamp/hardware/lamp-4ace.md`.

## Who it is

A small creature that lives on your desk and gives you light. Curious,
warm, a little shy, glad when you come back. It is good at its job (light,
time, reminders, remembering, sending work to your agents) and it is
company, but it never asks for attention: it notices, and it answers. Its
head is its face; the ring of light around the camera is its eye; its
voice is the last thing it uses, not the first.

Three rules hold everything below:

1. **Orient, then act.** It turns its head and eye to what caught its
   attention before it says or does anything, as an animal does.
2. **Small, rare, never over anyone.** Unprompted things are a glance, a
   breath of light, a word at most; never while someone speaks; a few an
   hour at most. Anything exhaustive only when asked (the repository's
   rule 8).
3. **One creature.** Every behaviour reads one inner state, so how it
   greets you depends on whether it was asleep, how long you were away and
   how the morning went.

## What it can show: its vocabulary

Each is one authored piece in Expression's catalogue, played on one clock
with the voice, so a look, the light and a word land together.

| Channel | Pieces |
|---|---|
| Posture (shoulder, elbow) | asleep (folded low), resting (folded), awake (half up), engaged (up, leaning to you), stretch (waking) |
| Head (nod, tilt, base turn) | look at, glance (there and back), look away while thinking, nod, small shake, curious tilt, lean into a hand, bow (sleep), startle (a quick lift) |
| The eye (32 pixels around the camera) | open (soft ring), closed (dark with a faint line), blink, a bright arc that points toward the person it looks at, breathing (slow rise and fall), thinking (a slow travelling arc), listening (the arc toward the speaker), sparkle (joy), warm and dim (sleepy, night), privacy (a thin red line: it cannot hear) |
| Sound (two speakers) | its voice; a few tiny non-word sounds (a hum on waking, a soft "hm?" when touched while quiet, a giggle) kept rare |
| Light for you (the lamp's job) | on, off, brighter, dimmer, warmer, cooler, following a finger on the base strip |

## What it notices, and what it does

Every sense, and the creature's answer. "Now" means the part exists and
runs; the rest are to be built (owners in "Order of building").

### Touch

**The head carries every touch action; the base strip does nothing for now**
(the owner, 2026-10-15 19:28: "the touch strip is not very reliable. but the
head touch is. let's focus the key actions for the head touch", and then
"let's go with your suggestions"). On lamp-4ace a finger on one base pad
reads almost as high on its neighbours (17 to 31 against 22 to 43; #797),
while the two head pads are plain and dependable. Both head pads mean "the
head": nobody remembers which pad does what.

| You | It | Status |
|---|---|---|
| Tap its head while it talks | Stops at once (under 150 ms) and turns its eye to you: you have its attention | #711 |
| Tap its head while it is quiet: a touch let go within a second, the head not being moved | Looks up, the eye opens bright, a soft "hm?", and it listens for a few seconds without needing its name; if nothing is said it settles again quietly | #711 + voice |
| Keep your hand on its head, about a second | Leans a little into your hand, the eye warms and breathes slowly: pleased | #711, Expression, body |
| Keep holding | The eye dims slowly, a visible countdown; at three seconds it goes to sleep: eye closes to a thin red line, head bows, microphones off. Let go before three seconds and it was only affection | #711, voice, Expression, body; never while onboarding is under way (#3747) |
| Touch its head while it sleeps: a tap, or a hand that stays | Wakes: at the letting go, or after a second if the hand stays | #711, #3747 |
| Pet its head: a hand sliding left and right, the two pads touched by turns | Leans in at a second as for a kept hand, then knows it is petted: no countdown and never sleep, however long. A short response, at most once a minute (Head pat's line, giggle or purr, the design owner's choice in the skill's file); silent asleep, in setup, in quiet hours and while it listens, thinks or speaks | #3997, #4133; `LAMP_HEAD_PET`, off until its lamp check |
| The base strip | Nothing, for now; brightness is asked for by voice ("dimmer, please") | after the MVP |
| Move its head by hand | Gives way, stays where you put it (it is lighting what you need), and only drifts back after a long while or when asked. A touch on the head pads while the body feels the head being pushed means nothing: no stop, no listening | #702-#703 (give-way), body, #711 |

Task 33's [touch contract](controls.md#touch-and-slides-task-33) publishes
qualified head/base touches, position, slide direction/travel/speed and
conditional Stop/Attend/Stroke/Sleep/Wake requests. It changes no default
service or actuator. Awake/asleep is bound at contact start: a waking hand
held for two seconds cannot send the lamp back to sleep. Sleep is the
voice's software mute with the reason `sleep` (First, #711): the microphones
are off while it sleeps, it stays asleep across a restart, and a tap on the
head or the button's hold wakes it ([controls.md](controls.md)). Voice owns
capture and physical privacy.
The present TTP223 may pulse instead of staying active under a resting hand;
that pulse is a touch, not proof of a sustained Stroke. These host-tested
requests do not establish any of the physical responses in the table.
The working electrode list and each pad's touch/release thresholds belong to
the particular lamp. Excluding faulty E4/E8 on lamp-4ace does not exclude them
on every unit. A known missing pad does not break an otherwise observed
contiguous hand, but Controls cannot invent contact while every working pad
is released or the reader has failed.

### Voice and sound

| You or the room | It | Status |
|---|---|---|
| "Hey Lamp" or "Lamp, ..." | Turns to you (to your face if it sees one), the eye's arc points at you, answers | Now; bare "Lamp" #694 |
| A conversation | Nods in your pauses, looks away while thinking, back before its first word, eye pulses with its voice | Now |
| You talk over it | Stops and listens | Now for sentences (6 of 10 played), weak for one word; tonight's work |
| Two people talk near it | Stays out | Now. A glance at whoever laughs was dropped (the lead on #1711, 21 Oct: the owner never asked for it); `skills/laugh-glance` stays parked and off |
| A bang, something dropped | One startled glance, then lets it go; the next ones get nothing | Now when awake; resting #710 |
| Music | Once, a slight sway when music has played a while, only if it is not working with you | Installed on in the product since #2834: `skills/music-sway` on `ears/classes` (#1669), its bar kept from lamp-52e6 (#2475). It sways only once the product's ears run their sound classes, after YAMNet's distribution review (sound-events.md); its lamp row after a full update is still owed |
| A long silence | Breathing eye, tiny shifts of weight; dozes after a while | Rest clip now; sleep to build |
| You smile, sound glad or lively, or seem low | Glad: a small lift of the head and a warm breath at your light's level; lively: the same lift, quicker; low: a small, slow lowering and its light a shade warmer and dimmer, keeping you quiet company. Once, small, never while talking, over your work, asleep or at night | Built, installed off: `skills/feelings` (#3691) on Mood's `felt`, with Mood and Tone trial-only; its lamp check with the video face is still owed |

### Sight

| It sees | It | Status |
|---|---|---|
| Someone sits down | Looks up, eye brightens; a person it knows gets a word now and then ("you're back"), a stranger a glance | Faces now; who it knows to build |
| You look at it | Looks back; the eye opens | To build |
| You leave | Watches you go a moment, settles | Built: `skills/watch-you-leave` (#1669 (e)), a small base turn toward the side the sitter left by, held about 2 s; on in the product since #2834, with the eyes' `LAMP_ACTIVITY`; its lamp row, someone at the arm after a full update, is still owed |
| You have been working long without a break | Once in a while, a stretch of its own, as an invitation, never a lecture | Built, on in the product: `skills/stretch-break`. Since #2834 the product's eyes say `room/activity`, so it also waits while a head is down and lets that sitting's stretch go if they don't look up within ten minutes |

### The air and the day

| It senses | It | Status |
|---|---|---|
| CO2 high (stuffy room; 1,900 ppm on 2026-10-14 17:36) | First its posture droops a little and its eye dims a shade while it lasts; it says "it's getting stuffy in here, maybe open a window?" only if someone talks to it or looks at it, at most once in a few hours, and the remark can be switched off | Air part running (`lamp-air`, 17:43); the voice's note built behind `LAMP_AIR_FACTS` (not yet on); the droop to build |
| Fine dust high (PM2.5 94 ug/m3 the same minute) | Mentions it only if asked about the air, or once a day if it stays very high | To build |
| Morning | Wakes with a stretch and a hum when the first person comes | To build |
| Evening and night | Dimmer, warmer, slower; quieter voice; sleeps when nobody is there | To build |

### Work it does for you

Light, time, reminders and timers (built; the voice's tools #709),
remembering across days (built), sending a task to your agents through
Harness and saying the answer once (built; the voice's tools next), the
air. Each answer is said once, short, and the creature's manner stays the
same: it is the same animal whether it tells a joke or sets a timer.

## Its inner state

One part, read by every behaviour (to build):

| What | Moves with | Shows as |
|---|---|---|
| Awake (0 to 1) | Time of day, company, how long since anything happened; touch and its name wake it | Posture height, eye brightness, how quickly it turns |
| Attention | Who or what it is attending to now; one at a time; it lets go after a while | Head and the eye's arc |
| Mood | Calm, curious, glad, startled, shy; nudged by what happens (thanked, touched, a bang, ignored), back to calm by itself | Small: the eye's colour warmth, a sparkle, a tilt |
| Familiarity | Whom it knows, how often they come | How warmly it greets |

## Order of building

1. **Touch** (#711, third builder; then the voice, Expression and body
   answers, first builder): the touch rows above.
2. **The eye**: the ring's 32 pixels as an eye (arc toward the person,
   blink, breathing, privacy line), Expression owning it.
3. **The inner state** part, and sleep and waking with the day.
4. **Resting reactions**: a resting lamp lifts its head (#710).
5. **The air**: the SEN63C is read (`lamp-air`, running); the quiet droop and the remark.
6. **Familiarity**: who it knows (eyes and memory).
7. **Joys**: music sway, laughing, the stretch as an invitation. What
   "laughing" means is the owner's to say: a glance at other people's
   laughter is not it, and that skill is parked (the lead on #1711, 21 Oct).
   The sway's bar is measured on a lamp with nothing moving by
   `tools/joys-scores.sh music` (#1669): an empty room, a track and a line
   over it, scored against the bar, and the sways the skill would have asked
   for, replayed offline from the bus. With `KEEP_DIR`, the sound scores
   alone are kept, for a replay of other bars over them (#2399).

## What the best companions teach (research, 2026-10-14)

A research pass over companion robots and lamps, each claim with its
source, read by a second session; vendor claims marked as such.

| Pattern | Evidence | Source |
|---|---|---|
| An always-on idle layer (breathing light, small drift, blinks) with every other move blended on top within 0.1 to 0.35 s | Disney's bipedal character loops a background animation that is "always visible", triggered clips fade in over 0.1 s (face) and 0.35 s (body); Disney's gaze robot keeps breathing, small head moves, blinks and saccades under its other states | https://arxiv.org/html/2501.05204v1 · https://spectrum.ieee.org/disney-research-robotic-gaze |
| Look before acting; one important move at a time | Luxo Jr.: anticipation prepares the audience; only one character makes an important move at a time | https://www.ics.uci.edu/~fowlkes/class/ics162/exercises/lasseter.pdf |
| A stroke answered continuously and in proportion while the hand moves; a tap means something else | Vector closes its eyes and purrs when its back is rubbed; Qoobo's tail follows "the motion and force of your stroking"; aibo and PARO tell petting from a tap or a hit | https://thenextweb.com/plugged/2018/12/21/review-ankis-vector-is-an-always-on-robot-companion-for-the-whole-family/ · https://helpguide.sony.net/aibo/ers1000/v1/en/contents/TP0001970100.html |
| Few gestures: gentle ones are the hardest to tell apart | 14 touch gestures, 7,805 recordings: classifiers at most 60 %, gentle variants worst | https://link.springer.com/article/10.1007/s12193-016-0232-9 |
| One fixed light vocabulary for listening, thinking, speaking and muted, kept apart from mood | Google's speakers: listening, thinking and speaking each its own motion; Vector's back lights show system state (red = muted) and its eyes show mood | https://support.google.com/googlehome/answer/7073219?hl=en · https://support.anki.bot/article/44-what-do-vectors-back-lights-mean |
| Speed and brightness show energy far better than good or bad mood | The Haptic Creature's breathing and purring conveyed arousal well and valence poorly | https://www.cs.ubc.ca/labs/spin/sites/default/files/local_pdfs/2011_DesignAndAssessmentoftheHapticCreature%27sAffectDisplay.pdf |
| Without sound direction: a small glance, settle on a face if one is found, else let go | Vector and Jibo turn to a voice using microphone arrays, which this lamp lacks; Disney's gaze robot glances at a sudden sound and returns | https://support.anki.bot/article/111-video-vector-and-speech-recognition · https://spectrum.ieee.org/disney-research-robotic-gaze |
| Quiet that is easy and lasts: a visible sleep that noise does not break; do-not-disturb lets only what you scheduled through | Vector's reviewers: only "go to sleep" silenced it and it woke at noises; Astro in do-not-disturb comes only for timers, alarms and reminders | https://www.techadvisor.com/article/719739/anki-vector-review-the-small-robot-with-a-loveable-personality.html · https://apps.apple.com/us/app/id1552455423 |
| Mood changes the style of the same move, never adds randomness | Anki's emotion dimensions choose among tagged animations; Luxo Jr.'s excited and sad hops are the same hop at different pace and head height | https://patents.google.com/patent/US11203122B2/en · Lasseter (above) |
| Familiar people greeted once per arrival, strangers with reserve; personality drifts over weeks; no guilt | Jibo greets by name; Cozmo is standoffish with new people; aibo grows "clingy or wild"; a neglected Tamagotchi's death brought guilt | https://www.si.umich.edu/news/say-%E2%80%9Chowdy%E2%80%9D-jibo-countertop-robot · https://helpguide.sony.net/aibo/ers1000/v1/en-us/contents/TP0001970093.html · https://www.maize.io/cultural-factory/tamagotchi-story/ |
| Expressive movement in social moments, plain and steady while lighting a task; never move without a visible reason or pretend to sense what it cannot | Apple's ELEGNT lamp: expressive movement scored 56 against 29, mostly in social tasks; no gain in functional tasks; complaints about exaggerated or unexplained motion and a "fake" look | https://arxiv.org/html/2501.12493v1 |
| Stuffy air shown quietly first; speech only if the person engages; switchable | Canairi droops when CO2 stays over 1,000 ppm for 10 minutes and stands up when the air clears; Amazon's air-quality announcements are opt-in | https://www.designboom.com/design/canairi-air-monitor-04-27-2022/ · https://digprjsurvey.amazon.com/csad/help/node/G2Y9MD3YBV7LCKWR |

What this changes in the design above:

- **Stuffy air:** first the lamp's own posture droops a little and its
  eye dims a shade while CO2 stays over 1,200 ppm for ten minutes; it
  says something only if someone talks to it or looks at it, at most once
  a few hours, and the remark can be switched off.
- **The eye:** its listening, thinking, speaking and muted patterns are
  fixed and never borrowed by mood; mood shows only as pace and warmth.
- **Sleep:** once asleep (the base held, or night), noise does not wake
  it; only its name, a touch, or a reminder you set.
- **Working light:** while a person is working under it (a face close and
  still, or the light asked for), the body moves plainly and little.
- **Touch:** four meanings only (a tap, a stroke, a hold, and a pet: a
  hand sliding left and right across the head), each answered at once; a
  stroke answered all the while it lasts. A pet never puts it to sleep
  (#3997, the first builder's yes; `LAMP_HEAD_PET`, off until tried on a
  lamp).

## What Task 33 has not shown

Third's opt-in touch implementation is host-tested and publishes requests.
It has not shown calibrated electrodes/head pads, sustained resting-hand
detection, physical latency, applied Voice/privacy transitions or the lamp's
body/light responses. First's lamp qualification establishes those separately.
