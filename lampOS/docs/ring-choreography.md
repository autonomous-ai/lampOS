# Conversation and ring ownership

The directed Rust runtime now has an optional, separately supervised ring
writer. The interaction Controller owns the conversation; a small local policy
derives light cues from that authority. This is the first connected expressive
output, not a finished character performance. Head/body motion, visual social
understanding and expressive variation remain implementation and physical
qualification work.

## What triggers a cue

| Observed state | What the ring shows |
|---|---|
| Startup, no admitted turn or completed turn, in a bounded `directed` run | Black. Worker readiness alone does not imply listening readiness. |
| The same in `serve`, while the microphone is permitted and input is available | The resting face: a slow, dim, warm breath (5.2 s) with a faint brighter patch that wanders around the ring. Black as soon as privacy closes, input is unavailable or the authority goes stale. |
| Fresh physical mute during the quiescent `serve` wait, with `LAMP_PRIVACY_CONTROLS=on` | A separate worker shows a slow dark-red breath, only after the active writer has closed. Unknown or stale switch state is black. The ordinary resting face remains denied. |
| Fresh retained input admitted to the current turn | The listening face: cool, at once, brighter with the person's voice. |
| The same while the internet cannot be reached (`LAMP_OFFLINE_VOICE=on`, the link down 5 s or more, #830) | The offline breath: one slow, dim breath in warm amber, about 2.5 s a breath, never more than a third of the listening face's least, no red, no flashing, the same all round. Only listening changes; idle, thinking and speaking are as always, so an empty room sees nothing different. |
| Local input ended; the turn is unfinished without active playback | The thinking face: an amber point circling the ring once every 1.15 s with a fading tail. No spoken waiting filler. |
| Matched speaker acceptance followed by `playback_started` | The speaking face: warm, as bright as the lamp's own voice is loud. Provider audio arrival alone does not start it. |
| A duplex yield in force (`--turn-taking duplex`: the lamp has stopped handing reply audio to the speaker for someone speaking over it) | The listening face, as bright as that person's voice is loud, on frames that are still speaking frames in phase, owner and permit (`RingFrame.attending`). It ends with the yield: the speaking face again if the reply resumes, the ordinary listening phase if the interruption is taken. |
| Playback's actual final sample retired | End the speaking phase; completion clears the owner and cue, and in `serve` the resting face eases back in. |
| New admitted question, privacy closure, expired authority or stop | Retire the old cue under its original identity. A successor can obtain its own cue. |

### The face

`crates/voice/src/ring_face.rs` holds what the ring shows and nothing else: a
mood (resting, listening, thinking, speaking), a voice level and a time go in,
32 colours come out, and no channel exceeds the configured ceiling. It has no
I/O, no clock and no authority. The ring worker keeps one face and renders
every frame it is allowed to write from it, so a change of mood eases over
about 70 ms instead of jumping, and the first frame of a cue is already
visible. Every pattern but the eye's (below) is symmetric around the ring or
travels around it, so none depends on which pixel is at the top of the head.

The voice level is one byte in each `RingFrame`: the root mean square of the
last 10 ms block, 0 at -48 dBFS and 255 at -14 dBFS, of the person's captured
audio while they speak and of the lamp's own speech as it is handed to the
speaker. It shapes a frame and grants nothing: when a frame is requested, and
whether it may be written, are decided exactly as before. In the face the
level rises within 20 ms and falls over about 170 ms.

One sequence has a look of its own. When thinking ends in rest with nothing
said in between (the service chose not to answer, or the speech was not for
the lamp), the face closes for a moment: it goes dark over the first fifth of
0.9 s and opens again over the rest, as eyes close on "not for me". A turn
that was answered never does this.

A level between two brightness steps is shown by rounding each pixel at its
own fixed point, so some pixels sit one step higher. That keeps a dim breath
smooth where a ceiling of 24 leaves the resting face two or three steps to
work with. Nothing is dithered in time.

### A hand kept on the head: the countdown (#711)

Off unless set: `LAMP_HEAD_TOUCH=on` in the voice's unit, which the voice
passes to its all-day ring worker as `--head-touch`
(`crates/voice/src/ring_head.rs`); it needs `LAMP_BUS=on` and Controls run
with `--touch`. The ring worker reads the head's state (`controls/touch_state`,
the head's `held_us`) every 50 ms. From one second of a hand kept on the
head (Controls' stroke) to three (its sleep), the whole face dims to 15 % of
itself, eased like every other change: a countdown the person can see, and
can stop by letting go, when it brightens again. Under a second nothing
changes; a hand a push from the body spoiled has no time and dims nothing.
It never makes the ring dark: what is shown still says the lamp hears. The
sleep that follows at three seconds is not drawn here yet (#711): the ring
worker is gone while the voice is muted.

### A tap on the head at rest: the listening face (#711)

Same switch and same worker. **The face follows the voice's "hm?", not the
tap** (First on #705, 17 Oct 07:26: no face when the hum doesn't play). The
ring reads no taps. The first frame of the hum's own turn
(`RingFrame::listening`, stamped by the coordinator for a `HeadCue` turn
from the pass that admits it) starts the tap's window (`Face::hum_frame`).
It is keyed by the hum's turn, so the hum's later frames don't stretch the
window, and each hum gets its own. A tap the voice doesn't answer gets no
face. Those are the reasons `head_cue::may_hum` gives in `head_tap_cue`'s
`why`: `microphone_not_allowed`; `was_speaking` (a touch that stopped
speech); `not_quiet` (a reply, provider audio or a speaker chunk under way,
a stream owned or a candidate open, a submission draining, paused, or the
speaker not yet free); `listening_not_ready`; `reply_owed`; and
`hummed_recently` (within 30 s of the last hum). Until 17 Oct the
ring read taps from the bus itself, and showed the face on every tap at
rest.

The same hum after the lamp's own empty "Sorry?" (`LAMP_UNCLEAR_HUM=on`,
#1417) is drawn the same way. Its turn (`SpeechOrigin::UnclearHum`) is
stamped listening from the pass that admits it, so it opens the same 2 s
window (`TAP_LISTENS_US`) and settles the same. It isn't held to
`hummed_recently` ([attention.md](attention.md)), so a second empty "Sorry?"
gets its face too.

The head's hand time (above) is read from the bus by a thread of the head's
own (`ring_head.rs`), never by the ring's loop, which only reads what it
last found, after each tick's Stop, Authority and expiry (#849 review,
twice):
- The loop does no bus work at all, so no receive can stand between it and
  its Stop, Authority and expiry.
- The test `a_flood_of_head_requests_never_holds_back_revocation_or_expiry`
  floods `controls/touch` from four senders. While it goes on, a 50 ms
  permit must blank within 150 ms, and a Stop sent then must end the worker
  within 50 ms. Since 17 Oct nothing in the ring receives `controls/touch`;
  the test stays as a guard.
- It failed on the first head, which read the bus before the controls. On
  the second head (at most four receives a tick, after the controls) this
  fixture passes on Third's Mac. The reviewer's own fixture, a different
  procedure on a Mac too, made that second head miss Stop by 485 ms. Why the
  two procedures differ is not established. What this head guarantees is
  structural: the ring's loop does no bus work, so no receive can stand
  between it and its Stop, Authority and expiry.
- The head's thread keeps the hand's time with its expiry, the state's stamp
  plus Controls' 300 ms. The ring's loop checks that expiry itself, so if
  the thread stalls, the hand lapses on time and stops dimming the face
  (#849 review).
- **Shown:** at rest, the resting face becomes the listening face, the one a
  turn shows while it listens, for `TAP_LISTENS_US` (2 s, as long as a call
  waits for the rest on lamp-4ace, #891), from the hum's first frame.
- **How fast:** the face opens with the hum's first frame, drawn in the
  pass that decides the hum. From Controls' tap that is the bus to the
  voice's poll (about 2 ms on a host, [attention.md](attention.md)), that
  pass, and one 2 ms ring tick. It waits on the coordinator's loop, which
  #1178 saw held 50 ms or more. Before 17 Oct the ring read the tap itself
  and drew it on its next resting frame, within about 27 ms of the bus.
  First's target is under 100 ms. On a lamp: the hum turn's first
  `ring_feedback` `Presented.write_finished_at_us` minus
  `head_tap_cue.acquired_at_us`. Not measured yet.
- **Speech in the window** is a turn, whose own listening face goes on from
  there.
- **Afterwards:** if nothing comes, the face settles back to its own light,
  most of the way in about half a second (`SETTLE_EASE_US` 170 ms, 600 ms in
  all), slower than the ring's usual easing, so it does not snap.
- **The hum's own turn** owns the lamp for its 0.3 s, when no resting frame
  may be shown. Each of its frames is the listening face instead
  (`RingFrame::listening`, stamped by the coordinator for a `HeadCue` turn,
  from the pass that admits it), so the face does not wait for the hum to end.
- **Not changed:** other turns' faces, and the ring's refusal of a resting
  frame when nobody may listen.
- **Tests:**
  - The coordinator's `only_the_hums_own_ring_frames_are_marked_listening`,
    the loop with a ring: a tap that hums has its first requested frame
    marked `listening`, and a tap refused while the lamp thinks has none.
  - `ring_face`'s `each_hum_opens_its_own_window_at_rest_until_its_settle`
    and `a_hummed_tap_listens_at_rest_for_its_window_and_settles_back_slowly`:
    the window, its settle, and a hum's later frames not stretching it.
  - `ring_worker`'s `only_a_frame_of_the_hum_opens_the_taps_window_at_rest`:
    a drawn hum frame opens it, and another turn's frame doesn't.

With the hum, it is one gesture with the voice's "hm?" ([attention.md](attention.md#a-tap-on-its-head-is-a-call-711))
and Expression's look-up ([alive.md](alive.md)), all from the one tap event
(First, #711, 05:05). A tap with no hum gets no face, but Expression still
looks up, unless the lamp is muted for privacy: then it doesn't look up
either (First on #711, 17 Oct 10:18; [alive.md](alive.md)). The privacy
switch counts there only on the Controls privacy route, which alone says it
to Expression.

### The eye (task 34, #712)

Off unless set: `LAMP_RING_EYE=on` (or `strong`, below) in the voice's unit, which the voice
passes to its all-day ring worker as `--eye` (`crates/voice/src/ring_sight.rs`).
Without it every frame is what it was (a test renders every mood with and
without the switch, with a face seen, and compares them pixel for pixel). With
it the face is the lamp's eye in two ways:

- **It looks at the person while it listens.** The ring worker reads the
  eyes' newest look (`humans/faces/tracked`) ten times a second, takes the
  largest face no older than 0.7 s, and the face eases its look toward it
  over about a quarter of a second. Looking to one side, the half of the ring
  away from the person dims, by up to three quarters, and the half toward them
  keeps its light; a person within 6 % of the middle of the camera's picture,
  or no face at all, has the whole ring, as before. So when the body turns the
  head to a face, the look should lead and fade as the face comes to the
  middle of the picture (not yet seen on a lamp). Only listening looks;
  thinking and speaking keep their own faces.
- **It blinks at rest,** now and then: every 7 to 16 s, not on a beat, the
  resting face dims to a quarter for 150 ms, eased in and out.

**The strong eye** (`LAMP_RING_EYE=strong`, passed as `--eye --eye-strong`;
first builder, 2026-10-16). On lamp-4ace on 15 Oct the owner found the eye too
subtle: the body turns the head to a face, so the face sits near the middle of
the picture, inside the 6 %, and the eye rarely leant. The strong eye is the
eye with two changes, and `on` is as before:

- **It leans sooner and further:** from 3 % off the middle, fully at 14 %
  (6 % and 22 % for the plain eye), and its far side dims by up to nine
  tenths (three quarters).
- **It notices a person who arrives:** when a face is seen after none for
  3 s, the whole face brightens by a third at once and settles back over
  about a second (0.3 s to a third of the way), listening or at rest, as eyes
  widen at someone who comes in. A face lost for a moment and found again is
  not noticed again.

Tests: the strong eye leans toward a person near the middle whom the plain eye
looks straight at; it notices an arrival once, against a plain eye run frame
for frame beside it, and again only after 3 s without a face; with the eye off
it changes nothing. Not yet seen on a lamp: it is for the owner's face and the
iMac's camera on 16 Oct.

### The lamp's one breath (#1670)

The ring's resting breath and the head's (Expression's idle layer,
[alive.md](alive.md)) are one breath: on the boot clock over 5.2 s, as
always, and where Mood runs (`LAMP_MOOD=on`, [mood.md](mood.md)), at the
phase Mood keeps in `mood/state`'s `breath`. There is no switch of the
ring's own: without Mood, or with no fresh state, every frame is what it
was. The ring worker reads `mood/state` four times a second
(`crates/voice/src/ring_mood.rs`), Expression's service its own copy as
often, and both take the phase through the one function
(`lamp_msgs::MoodState::breath_in`): from `mood` only, checked, no older
than its 2.5 s, moved on from its stamp to now at its pace. So the head and
the ring breathe together at any pace, as [creature.md](creature.md) has it
("mood shows only as pace and warmth"). Listening, thinking, speaking and
muted keep their own faces.

| Mood says | Pace | One breath | Blinks (with the eye) |
|---|---:|---:|---:|
| Calm (energy 0.30), awake 0.40 or more: the day and the evening | 1 | 5.2 s | every 7 to 16 s |
| Energy up to 0.70 (a bang's) | up to 1.25 | down to 4.2 s | every 5.6 to 12.8 s |
| Awake under 0.40, dozing | down to 0.7 at 0 | up to 7.4 s | up to every 10 to 23 s |
| Asleep | 0.7 | 7.4 s | every 10 to 23 s |

Energy and dozing add; the pace stays from 0.7 to 1.25 (`lamp_msgs::mood_pace`).
Mood begins the breath where the plain one is and moves it on at the pace
whenever its time does, so a change of pace changes the tempo from where the
breath is: it never jumps. When Mood starts or stops, both the ring and the
head go from one clock to the other at once (the head eases, as it always
has at the hour's turn). A blink itself stays 150 ms. Expression's sleeping
breath (9 s) is its own and keeps the boot clock, as before. These are
authored starting values, small on purpose (CLAUDE.md rule 8), not fitted to
a lamp.

Tests: Mood's breath begins at the plain phase, goes on at the pace, keeps
its place at a bang, and a reader moving the last state on lands where Mood
does (`crates/mood/tests/it/engine.rs`); with no breath the face is as before,
frame for frame; the ring breathes at the phase given, 11 to 12 breaths a
minute at the plain pace, 14 to 15 at the quickest and 8 to 9 at the
slowest; blinks follow the pace; the ring's reader takes the breath from
Mood's fresh, valid state on a real bus and nothing else; Expression's head
holds the phase it is given and goes back to the plain breath without a
leap (`alive_engine.rs`); and the ring's and the head's breath are the same
at every phase and with none (`crates/voice-eval/tests/it/one_breath.rs`). Not
yet seen on a lamp.

Which way the ring counts was measured on lamp-4ace on 2026-10-14 at 19:54:
with the voice stopped, `crates/ring/examples/pixel_map.rs` lit pixel 0 red,
1 and 2 orange and yellow, 8 green, 16 blue and 24 white, and a photograph
from in front showed them at the top, just after it, and at three, six and
nine o'clock. So pixel 0 is at the top and they count clockwise as a person
facing the lamp sees them. The camera sits in the middle of the ring looking
out, so a person on the left of its picture is on the lamp's left, which that
person sees on their right; the look goes there.

On lamp-4ace since 2026-10-14, 20:15 (build `d3e9cc3d2`, the ring worker
started with `--eye`). Played a request at 20:16 with no face in view, the
lamp stood and listened with the whole ring lit, as before. The look toward
a face is not yet seen on the lamp: a face photograph on the screen in front
of the lamp, which the eyes found in every look on 2026-10-13, was found in
none that evening, because the screen was locked and showed only its clock
(the eyes' own frame showed it), so the photograph never appeared. It needs
the screen unlocked, or a person at the lamp. The blink at rest has not been watched on the lamp either.

Not done, from the task: the mood's pace and warmth (from the inner state,
#713, when it publishes). The muted look is the asleep face, below.
With the Controls privacy route on (`LAMP_PRIVACY_CONTROLS=on`), the muted
look is the physical-mute indication below instead, and the asleep face stays
off whatever `LAMP_RING_ASLEEP` says: the ring has one writer at a time (the
first builder on #795, 2026-10-16 13:50). So with the route on, the red breath
is the hardware switch and a software mute leaves the ring dark; the warm
asleep face is the route-off look, and, with head touch, a hand's sleep's
while the slide is open (#1362, "A hand's sleep" below). A lamp with no slide
(`LAMP_PRIVACY_CONTROLS=no-slide`) has no red breath to show, so its ring is
as with the route off: the asleep face, the tap face and the pulse, and no
indicator worker ([privacy-switch.md](privacy-switch.md#a-lamp-with-no-slide-no-slide)).

### Physical mute indication (#755)

`LAMP_PRIVACY_CONTROLS=on` selects the Controls-backed privacy path and enables
an optional ring worker during the quiescent privacy wait. Unset leaves the
existing GPIO and ring behavior unchanged. The supervisor passes
`ring --privacy --bus-root PATH` explicitly because worker environments are
cleared. It verifies the active ring's destruction before this writer starts,
and verifies this writer's shutdown black receipt and successful process exit
before starting the next active ring. The SPI advisory lock is retained until
the old worker drops its device; a Stop request alone is not a handoff.

This worker reads only the typed Voice-owned `voice/privacy` lease. Its shared
reader validates the source, message type, publisher session, sequence and
current kernel clock, with a bounded drain of 32 messages. The ring also checks
the matching IPC controller boot and microphone generation, fresh denied
microphone and camera snapshots, no turn owner, `permitted:false`, and
`physical_muted:Some(true)`. Original switch evidence expires within 50 ms.
Software mute alone, unknown state and normal Allowed authority cannot paint
this indication or grant capture/readiness.

`GuardedRing::show_privacy_muted` takes no pixels, ceiling or conversational
permit. It renders a fixed four-second cosine breath on all 32 pixels, red
2–10 and green/blue zero, at most 40 frames per second. The red ceiling reuses
V1's dark-red mute preset; this is not a claim of an identical optical waveform.
The worker checks every 2 ms, drains priority control before and after bus I/O,
and blanks on missing, stale, open, wrong-boot or invalid evidence. Controller
loss or reader failure blanks and ends that worker. An observation that expires
during a write is checked again on return and blanked. The existing 10 ms
observed write budget (with the late-write rule below, #1620) and bounded
cleanup attempts remain in force; Linux SPI syscalls have no hard userspace
deadline.

SPI and bus work stay outside the physical sampler and Voice owner loop.
Indicator failure cannot reopen a microphone, start audio or bypass privacy
cleanup. Its optionality does not permit two writers: failed or unverified
cleanup prevents a successor. A start that fails before any writer process
exists leaves nothing to clean up; the muted wait goes on without the light. Fake-sink tests establish protocol and write
behavior only. Lamp checks must measure switch acquisition, permission/handle
closure, speaker silence and visible indication separately, including rapid
flips, stale input, worker failure and repeated writer handoffs.

### The resting face and its authority

A resting frame belongs to no turn and claims nothing about one. The ring
worker paints it itself, forty times a second, and only in `serve` (the
coordinator starts the worker with `--resting CEILING`, the same ceiling as
its cues). `GuardedRing::show_resting` writes it only when the installed
authority is fresh, the microphone is permitted, input is available and no
turn owns the lamp, and never over a displayed cue. `tick` blanks it when the
authority expires, privacy closes or input stops being available. An owner
appearing does not blank it: the last resting frame stays until the first cue
frame is painted over it, so a turn begins with no dark frame. The coordinator
receives no receipt for a resting frame; a blank that ends one is reported
like any other.

So the resting face means one thing: the lamp is on and able to hear. It is
dark when the privacy switch is closed.

### The asleep face (#712)

A lamp that cannot hear, because it is muted or its privacy switch is
closed, has no ring worker, so without this its ring is dark, which reads as
off or broken. With `LAMP_RING_ASLEEP=on` in the voice's unit, the service
shows the asleep face while it waits for the lamp to hear again: a faint warm
glow on the five pixels at the bottom of the ring (at most 12, 5, 1 of 255,
and never over the resting ceiling), breathing once every 6 s between three
tenths and all of that. It fades in over 1.5 s, starting 1.5 s into the wait,
so the short wait at the start of every cycle never shows it. It is not red
and not a line: a sleeping thing closes its eyes and keeps breathing.

It is painted by `ring_asleep` from a thread of the voice, through
`lamp_ring::asleep::AsleepRing`, the one writer that needs no permit because
the one picture it can show (and black) says only that the lamp cannot hear.
It opens the device after the cycle's ring worker has gone, and it is black
with the device closed before the wait returns, so the next cycle's ring
worker is again the only writer. If the device cannot be opened it says so
once in the journal and the ring stays dark, as before.

**A reminder owed while it waits (#1545, `LAMP_REMINDER_PULSE=on`).** While
a reminder is owed and the wait is the privacy switch's or the person's mute
(`Waiting::pulses`: not the restart guard or a switch that can't be read;
Thirteen on #1545), and the lamp is not asleep (the sleep a hand held on the
head chose, whose face this is, even with the switch closed too; the
reviewer on #1581; `pulse_shown`), one soft arc of the asleep face's own
warmth goes once around the ring over it, in 4 s (#1494's pulse's look): the
first at the due time, on the wait's first look that finds it owed, so
someone who glances up then sees it, and then one on each minute from then
(`ring_asleep::Pulses::around`). It is a sixth of the ring wide and never
above the face's peak (`lamp_ring::asleep::owed_pixels`), so the writer
still shows nothing but this face, its pulse and black. It stops when
nothing is owed (said when the microphones open, or cancelled), and none
starts half an hour after it was owed, so a mute that lasts the night does
not pulse all night; the journal and the health still say it is owed. The
minute and the half-hour count from the due time whatever holds the pulse
back meanwhile (asleep, a switch that can't be read, a lapse of the offer):
one missed is not made up, and none of these starts the half-hour again
(Thirteen and the reviewer on #1581). A new muted cycle starts its own. The
wait tells the painter on each look (`Asleep::owed`: since when, while it
may show); it needs `LAMP_RING_ASLEEP=on`, without which the ring is dark
while muted and there is nothing to pulse over. Switched on without the face
(`LAMP_RING_ASLEEP` off, no ring, or the Controls privacy route), the journal
says so once as the service starts, with the reason, rather than pulse
nothing (`pulse_without_face`; the first builder on #1545).

**A tap on the head while muted (#711, First on #711, 17 Oct 10:18).** A
tap still gets an answer, but nothing moves and nothing is heard: the ring
shows the asleep face for about a second (`TAP_SHOWS`, 1 s). It comes at
once, up over a quarter of a second, held at the top of a breath, and down
over the last quarter (`TAP_FADE`). Then it's black with the device
closed, by itself. One at a time. None for the tap that lets the lamp hear
again, since its ring worker is about to start. The trace says
`head_tap_muted_face`. Muted here is the software mute or the privacy
slide closed: the wait reads the slide itself and keeps it apart from the
software mute, so its last state (`PrivacyState::Muted`) counts too, and a
closed slide with the mute open still gets the face (`may_hear`, the
reviewer on #1288). The face is drawn only where it is the muted look:
- head touch on (`LAMP_HEAD_TOUCH=on`, which brings the taps);
- not the Controls privacy route with a slide (`on`), whose ring is the
  switch's own indication and has one writer (#755): there a software mute
  stays dark, rather than show a look that claims the switch (with
  `no-slide` the face shows, as with the route off);
- not `LAMP_RING_ASLEEP=on`, which shows the face the whole time already;
- not while a hand's sleep's face shows (below).

**A hand's sleep (#1362, the first builder on #1362, 21 Oct 09:21).** A
person who holds the head to put the lamp to sleep must see that it worked,
and a dark ring reads as idle. So with head touch on, while the wait is the
sleep a hand chose (`Observation::asleep`, the voice's mute with the reason
`sleep`), the ring shows the asleep face as above: the same faint warm glow
at the bottom and 6 s breath, starting 1.5 s into the wait and fading in
over 1.5 s, as the body folds (#2445, body.md's "Asleep" once that is
in). It is the trace's `head_sleep_face`, once per sleep, and it stops when
the sleep ends (a plain mute, the person's or the app's, gets none); the tap
that wakes the lamp ends the wait, and the cycle's ring worker takes over
(`ring_asleep::SleepFace`; `sleep_face_allowed` says whether it is drawn
and whether it yields to the slide). It is drawn:
- with head touch (`LAMP_HEAD_TOUCH=on`), where the face is not shown the
  whole time already by `LAMP_RING_ASLEEP=on`;
- on the Controls privacy route with a slide too, while the slide is open.
  The slide's own dark-red whole-ring breath is unlike it, so the two never
  look alike. When the wait reads the slide closed, the face stops there
  and then, black with the device closed, before the wait starts the
  slide's indicator worker, so the ring keeps one writer (#795). That
  worker keeps the device until the wait ends, so the face doesn't come
  back in that wait, even once the slide opens: then the ring is dark until
  the lamp wakes.

**With the person's light on, the light wins** (the first builder on #1362,
21 Oct 15:1x). On this lamp the ring is the room light, and a creature that
dozes off doesn't switch off someone's reading light (rule 8).
- **Why the wait draws it:** the conversation's ring worker writes black as
  the mute stops it (`GuardedRing::shutdown`, pinned in `lamp_ring`'s tests),
  so a light left alone would go dark at every hand's sleep.
- **What it draws:** when the sleep's look begins, the voice reads the last
  light the ring's light said (`light/state` from `ring`, however old, as
  the ring stopped saying it at the mute; `ring_asleep::persons_light`). If
  it was on, the sleep draws that light back at once, from the black, as the
  person's ring drew it at rest (`ring_face::Face`, the same colour, level
  and slow breath), with one slow dip of a step as it settles: up to a level
  lower over 0.6 s, held to 2 s, back up to their light by 4 s
  (`ring_asleep::LIGHT_DIP`, `dip_share`; one level's glow down, or half the
  lowest, `Lit::step_down_share`). Then it is their light, and nothing
  more: the fold and the breath say asleep.
- **With the light off,** never said, or said by another part, the sleep
  shows the asleep face as above.
- **The rest is the same:** the writer is the asleep face's
  (`AsleepRing::show_light`, under the same ceiling), and the slide's
  hand-over is the same either way: a closing slide blacks it at once.
- **The wake keeps the light up:** a wake that opens the lamp (the tap, or
  the wait ending on it) leaves the person's light on the ring and closes
  the device without black (`Asleep::release`, `AsleepRing::leave`), so it
  doesn't blink off while the conversation's ring starts, after its provider,
  capture and speaker; that ring then draws it again (the reviewer-lead on
  #2549). A sleep that ends any other way, and the asleep face, end black.
- **Not drawn here:** a light changed from the app during the sleep (with
  `LAMP_LIGHT_WHILE_MUTED=on` it is said and taken) is not drawn until the
  lamp wakes.

It is stopped, black and closed, before the wait returns, as the long face
is. Expression gives no look-up for a tap while the software mute holds
(#1282). It sees the slide only on the Controls route, so on this route a
tap with the slide closed still gets Expression's look-up beside this face
([alive.md](alive.md)).
Tests: `ring_asleep`'s
`a_taps_face_shows_at_once_fades_in_and_out_and_closes_by_itself` and
`a_tap_face_comes_one_at_a_time_and_never_for_a_tap_that_wakes_it`, and the
coordinator's `a_tap_while_muted_gets_the_asleep_face_once` (the muted
wait's call, `answer_muted_tap`, the slide closed with the mute open, and
where the face is allowed) and
`the_muted_waits_own_closures_count_the_slide_for_a_taps_face`. That one
runs the muted wait's own closures (`muted_wait`, whose privacy wait is a
parameter) with a waiter that says the slide closed or open and then
looks; it fails if the slide's state isn't kept or isn't given to the face
(the reviewer on #1288). Not shown on a lamp.

### The night's sleep (#3748)

A dozing creature looks the same whichever way it fell asleep (CLAUDE.md
rule 8). A hand's sleep mutes the voice, and its wait paints the asleep face
(above). The night's sleep, Expression's own or the `night-sleep` skill's
([alive.md](alive.md)), mutes nothing: the ring worker keeps the ring, so
without this it shows its resting face at Mood's Asleep pace. With
`LAMP_NIGHT_ASLEEP_FACE=on` in the voice's unit (passed to its ring worker
only), the ring worker reads `expression/state` every 100 ms, twice as often as
Expression says it, on a thread of its own (`ring_night::RingNight`). While
it says `asleep` (falling asleep or asleep), from `expression`, enabled,
valid and said within 500 ms, the resting face eases into the asleep face:
the same five warm pixels at the bottom, at most 12, 5, 1 of 255, breathing
every 6 s (`lamp_ring::asleep::asleep_pixels`), starting 1.5 s into the
sleep and over 1.5 s, as the body folds (`ring_asleep::AFTER`, `FADE_IN`).
Awake again, or no fresh state, it eases back out as fast, from wherever it
was, to the resting face (`ring_night::Fade`, `over`).
- **One writer:** it shapes only the resting frame, so a turn's cue paints
  over it as over any resting frame, and the privacy switch and the
  resting authority blank it as before. A hand's sleep stops the ring worker
  with the mute, so the two faces never draw at once; the 1.5 s before it
  shows covers the last `asleep` Expression said as the voice wakes.
- **With the person's light on, the light wins,** as at a hand's sleep: the
  face shows only over the light off (its ember), and the light switched on
  in the night fades it out; switched off, it fades in again after 1.5 s.
  The lamp's first light, before anyone asked for one, is on, so a lamp
  whose light was never switched off keeps it.
- **Not drawn:** over the unplug picture or the shutdown's warning, which
  come first.

Tests: `ring_night`'s
`only_a_fresh_enabled_expression_saying_asleep_is_asleep`,
`it_fades_in_after_a_moment_asleep_and_out_on_waking`,
`all_of_it_is_the_asleep_face_and_none_is_the_face_as_it_was` and
`the_worker_rests_on_the_asleep_face_only_while_expression_sleeps` (the
actual worker loop and guarded sink: the asleep face exactly while
Expression says asleep with the light off; never awake, unsaid or over the
light on); Expression's
`the_nights_sleep_says_asleep_for_the_rings_face_until_it_wakes`
(`alive_engine.rs`). Not shown on a lamp.

### The setup light (#806)

While a lamp is not set up, no voice runs, so the ring belongs to
`lamp-setup`, which breathes soft white while it waits for its person, soft
blue while it joins the home's Wi-Fi and amber after a join that failed
(docs/setup.md, "The setup light"). It writes through
`lamp_ring::setup::SetupRing`, the second writer that needs no permit: it can
show only those colours over the whole ring, no brighter than 24, and black.
It sends black and closes the device just before the marker that starts the
lamp, so the voice's ring worker is again the only writer.

### The waking face (#1681)

From power-on until the voice takes the ring, nothing else of a set-up lamp
lights it, often for a quarter of a minute, and a dark lamp reads as a
broken one. With `LAMP_WAKING_FACE=on`, `lamp-setup wake`
(`lamp-waking.service`, early at boot, only once setup's marker exists)
lights it as a creature waking: the asleep face's warm glow at the bottom
opens slowly up the ring, as an eye opens, over 20 s, and breathes every
6 s, never dark at the low of a breath; the bottom stays the brightest,
and no channel is above 24. It is lit from its first frame and fades the
rest of the way in over 1.5 s. If no voice has asked for the ring within
5 minutes, it turns pale and grey over the whole ring over 3 s and
breathes every 10 s, and stays so, still ready to be asked: a voice that
never started. No spinner and no blink.

It comes with every start of the voice, not only at power-on (#2848):
`lamp-voice.service` wants it and starts after it, so the app's Restart, an
update round's start of the voice and a crash restart each open the ring
like an eye again before the voice asks for it. A face still up is not
started twice. It is outside `lampos.target` and never restarted, so a stop
of the voice leaves it alone. A voice that crash-loops shows the face at
each try, which reads as a lamp trying to wake. Checked in a systemd 252
container on 29 Oct (a crash restart twice, `systemctl restart
lampos.target`, `systemctl restart lamp-voice.service`); not yet on a lamp.

It writes through `lamp_ring::waking::WakingRing`, the third writer that
needs no permit: it can show only that face and black. The ring opens with
a lock (`try_lock`), and a ring the voice cannot lock stays off all its
session, so the hand-over is asked, never raced: `lamp_ring::linux::open`
(the ring worker) and `open_asleep` (the asleep face) first ask on
`/run/lampos-waking/release.sock`; the waking face fades out over 600 ms,
sends black, closes the device and only then answers. `open_privacy` (the
physical-mute indication) asks for it at once: black and closed with no
fade, within 100 ms of asking. It waits no longer than 100 ms, and a ring
still locked then is opened without the lock (`lamp_ring::waking::take`):
the privacy switch's light never depends on the waking face. A face that
is alive but stuck may land the frame it was stuck on, and nothing more:
asked at once, it sends its black only within 50 ms of reading the ask
and otherwise closes with no frame, and the indication paints again every
25 ms. The voice's own asks
are given up after 2 s and the open goes on; that is only for a waking
face that doesn't answer. With no socket
(the switch off, or the face already gone) the open goes on at once, as
before. The setup light's opener and the waking face's own never ask.

Targets, not yet measured on a lamp: first light within 300 ms of the
unit's start, and the ring dark for at most 1 s at the hand-over. Not
shown: power-on to the unit's start, and whether `/dev/spidev3.0` is there
by then (it is tried every 200 ms until it is).

Host tests: `crates/ring/src/waking.rs` (capped and the same on both sides
of the bottom, closed it is the asleep glow, it only opens, a breath never
dark, unwell pale over the whole ring; the ask's 2 s bound, the at-once ask's
100 ms, a missing socket, and the privacy open locking when it can and
taking the ring when it cannot); `crates/setup/src/wake.rs` (the switch, lit from the first frame,
the fade-in and the opening, 6 s then 10 s breaths, pale after 5 minutes,
the fade-out); `crates/setup/tests/wake.rs` (with a real `try_lock` on a
file: the voice's lock fails while the face shows and succeeds the moment
it is answered, the privacy ring has it within 100 ms, a face that never
answers cannot keep the privacy ring dark past 100 ms and writes nothing
after but the frame it was stuck in, a device that isn't
there yet is tried again, and an ask before any ring opened is answered at
once).

### The lamp's own light

The resting face is also the lamp's light: a colour and a level that
someone can ask for ("make your light blue", "a bit brighter") and that
stays until it is asked for again. The faces of listening, thinking and
speaking are the lamp's expression and do not change with it; when a turn
is over the ring eases back into its light.

| | |
|---|---|
| Colours | Ten, by name: yellow, red, green, blue, cyan, purple, orange, pink, white, warm. `warm` is the amber the lamp has always rested in |
| Levels | 1 to 5: about 7, 24, 44, 68 and 90 hundredths of the ceiling, each with a slow breath of 6 to 10 more on top. The fifth reaches the ceiling at the top of a breath |
| Before anyone asks | On, warm, level 1: exactly the resting face described above |
| Off | Not dark. An ember: one pixel in four at three steps of red and one of green, the faintest amber the ring can show, and the others dark. About a quarter of the lowest light in all |

The ember is a few lit pixels and not the whole ring glowing fainter,
because the ring cannot show that: under the ceiling of 24 that lamp-4ace
runs with, the lowest light is already two or three steps of red and one
or two of green, and less than one step of each can only be shown as some
pixels red and others green. That was the first version, and on the lamp
it was a patchwork of pink and green.

Off is an ember because of what the resting face means. A ring that went
dark on "turn your light off" would look like a lamp that cannot hear,
while it can. What is answered says so: "My light is off. The faint glow
that is left shows I am still listening." The privacy switch still makes
the ring dark, whatever light is set.

The ring worker is the only writer of the ring, so it is also the one
that takes the light (`src/ring_light.rs`). Started with
`--bus-root DIR` (the coordinator adds it when the voice's unit has
`LAMP_BUS=on`), it:

- begins with the light last asked for, which the bus keeps as the newest
  value of `light/command`, or with the first light if there is none;
- looks for a light asked for on `light/command` (`lamp_msgs/Light`)
  on about every pass of its 2 ms loop (every 50 ms before #1806, which
  the person heard on each light call), and eases the ring into it. A request that is not a
  light there is (a colour that does not exist, a level outside 1 to 5,
  something that is not a `Light`) is passed over;
- says what it shows on `light/state` when that changes and once a
  second, each good for three seconds, so that a ring that has stopped is
  not a light that can be asked for.
- takes a self-test's colour on `light/test` (`lamp_msgs/Light`, asked
  less than a second ago) and shows it for 2 s at most, then the light
  asked for on `light/command` again. A `light/command` ends a test at
  once, even one for the light already shown. A test never touches
  `light/command`, and `light/state` goes on saying the person's light, so
  a test cannot overwrite what the person asked for (#812);
- says the light it drew on `light/drawn` (`lamp_msgs/Light`), from the
  ring's own write receipt for each resting frame, timed by when the write
  finished, when that changes and once a second, each good for three
  seconds. A frame the ring refused (the privacy switch closed, a turn
  owning the lamp) says nothing, so `light/drawn` is what was on the ring,
  where `light/state` is what was taken;
- with `--light-requests` (the coordinator adds it when the voice's unit
  has `LAMP_LIGHT_REQUESTS=on`, #940) takes lights asked for that lapse on
  `light/request` (`lamp_msgs/LightRequest`, `src/ring_requests.rs`): a
  skill's glow, or anything else that must not leave the person's light
  changed. Each asker (`claim.source`) has one request, its newest. The
  ring shows the highest priority that holds on the host's boot clock
  (`/proc/sys/kernel/random/boot_id`), the newest claim of equal priority
  (a renewal keeps its claim's place, so two skills renewing in turn never
  swap the light), and the person's light again once none does, with no
  one giving it back. An
  asker gives the light back early by asking with a time already passed.
  A claim holds 30 s at most, so a skill renews what it keeps. A
  `light/command` read gives way every request below `person` whose claim
  began before it, and it stays given way while its asker renews it
  without a gap; the asker's next claim, after that one lapsed or was
  given back, is heard again. A light read is the person asking whatever
  its stamp: wall-clock stamps only order a request and a person's light
  read in the same look, so a clock set back neither hides the person's
  light nor holds a later request off. At most 16 askers are kept, a
  request given way keeping its asker's place, so whatever is held can
  always be given way; a request refused for want of a place is not held,
  and its asker's next one is a new claim. 8 requests are read a look. A self-test's colour still comes
  first. `light/state` goes on saying the person's light; `light/drawn`
  says the request's while it is drawn. Once resting frames have shown a
  request's light for 350 ms running (five of the face's 70 ms fade times;
  frames more than 100 ms apart, or a tap's listening face or the face
  closing after a turn over it, begin the count again), the ring says so on
  `light/reached` (`lamp_msgs/LightReached`: the request's `source` and the
  `light`), so a skill can wait for its light. It is said again at the next
  frame after its asker renews it, so an asker that asks twice hears it
  twice; a new light, even from the same asker, or the same light back
  after a self-test over it, is reached afresh. A turn's face has colours
  of its own and never shows the lamp's light, so nothing is reached inside
  a turn (the reviewer on #1166), nor for a light the privacy switch keeps
  off the ring, the person's light ends, or that lapses first. A waiter
  subscribes before it asks and bounds its wait. A skill waits for it before
  the step that speaks, never after: its own say is a turn, so its light is
  reached only once that turn has ended (El on #1166). A request the ring
  holds but doesn't show, because another request or the person's light is
  over it, is said on `light/outranked` (`lamp_msgs/LightOutranked`: the
  request's `source`, and `by`, the `claim.source` shown over it or
  `person` for the person's light), so its asker is told it lost (#940).
  It is said when the request is taken hidden, when a request shown loses
  its place, when what is over it changes, and again after each renewal
  while it stays hidden. A turn's face, a self-test and the privacy switch
  hide every request alike and are no loss, so nothing is said for them;
  a request outranked meanwhile is still told. The loser keeps its claim and is
  shown by itself once what is over it lapses or is given back. A request
  may carry a `pattern` (#940 item 3, `lamp_msgs/LightPattern`), and the
  face moves the lamp's own light at rest in it, from the first resting
  frame that draws that light (a renewal keeps it, a self-test never
  starts it): `breathe`, the whole ring on a sine between the pattern's
  `low` and the light's level, starting low, 4–12 s a breath (6 s when a
  skill names none; under 5 s reads as anxious); `candle`, each pixel
  wandering at most a fifth under the level, as V1's candle did, on two
  slow waves of two and three lobes so neighbours stay close, 1–4 s a
  wander, only at level 3 and up, where the ring keeps the colour's
  channels at the product's ceiling (the reviewer on #1494: a swing from
  the lowest level broke dim pixels' colour about five times a second);
  `pulse`, at the light's level, one soft arc a sixth of the ring wide and
  up to a third brighter going once around in 3–6 s (4 s by default, far
  slower than the thinking face's point), then the light as it rests with
  no pattern, its own slow breath and all, only at level 2 and up, where
  the arc can be seen. The ring
  plays one pulse a minute from an asker, counted from what it drew: a
  second within it is drawn as already played and said in the voice's log
  (the first builder on #940: "something came" twice is a nag), and a pulse
  no frame drew (a turn over it, outranked) is not counted. V1's blink, notification flash
  and rainbow are not taken: they read as a machine (CLAUDE.md's rule 8).
  A light show a person asks for (Light show, #1679) is a skill's light
  asked again for each of its frames, a colour held 2 to 20 s with one of
  these patterns, under a minute in all: the ring sees nothing new, only
  requests; the show never runs on its own. Its colours do change, which
  the rule above says a lamp never does: that is the exception for a show a
  person asked for, as a person would dim the lights for a party, never a
  light the lamp chooses. A scene (Light scenes) is not
  a request at all: it is the person's own light, set through the actions
  part as they asked, and so kept with `LAMP_LIGHT_KEEP=on`.
  A turn's faces, a tap's, an ember and a self-test never move with it.
  A ring without patterns shows the light steady: the field is optional.
  A request may also carry a `fade_ms` (#1677, 1 to 60 s; past either end
  it is not taken): while it is the light shown, resting frames ease into
  it with a time of a quarter of the fade instead of the face's 70 ms, so
  the light falls as a share of itself, 98% of the way by the fade's end,
  never in a step (`ring_face.rs`, `FADE_EASES`). That is a skill's light
  going out when nobody is there ([presence.md](presence.md)). A turn's
  face over it, a tap's and the face closing after a turn ease as ever, a
  self-test is shown at once, and once it is let go the face settles back
  into the person's light over about half a second, as after a tap,
  waking rather than snapping on. `light/reached` is still said 350 ms into
  it. A ring without fades eases at its own 70 ms. A boot
  clock that cannot be read is said once and the ring takes the person's
  light alone.
- With `LAMP_SKILL_LIGHT_SPEAKING=on` as well (El on #1722, the first
  builder's 14:43; off unless `on`, and nothing without
  `LAMP_LIGHT_REQUESTS=on`), the request shown also tints the **Speaking**
  face, so a skill's light held through its words (Welcome back's warm
  glow, held exactly as long as its hello) is seen with them rather than
  covered by the turn for its whole life. The face keeps its own motion,
  following the lamp's voice with its slow two-lobed turn, and takes 60% of
  its colour from the light and 40% from its own warm (`ring_face.rs`,
  `SKILL_TINT_SHARE`); its least glow, with the voice quiet, becomes level
  3's (0.44 of the ceiling, `SKILL_SPEAKING_LEVEL`) whatever the light's
  level. Under it a level 1 or 2 is under Speaking's own least, and the
  ring can't tell a level the skill named from one `lamp-skills` filled in
  from the light shown; over it the voice would have only the gap to the
  ceiling, 0.90 to 1.0 at level 5, the idle turn's own swing, and the face
  would read as a lit disc rather than a mouth (Seventh on #2253). At 0.44
  the colour shows over plain Speaking's 0.26 and the voice still moves it
  0.44 to 1.0, three quarters of plain Speaking's 0.26 to 1.0.
  Only Speaking: listening, thinking (the amber point), offline, a tap's
  face and rest are as before, and a gesture's accent still rides over the
  tinted face. A self-test over the request, the person's own light, or a
  light that is out (#1677's ember) tints nothing. Given back or lapsed,
  the next frame is plain Speaking, eased at the face's 70 ms. The tinted
  face is a turn's frame like any other, written only under the turn's
  permit, so the privacy switch closing refuses it as it refuses every turn
  frame (`ring_face.rs`,
  `a_tinted_speaking_frame_is_refused_once_the_privacy_switch_closes`).
  Nothing is said `reached` for it: that is still for resting frames
  only. Not seen on a lamp.

Whoever asks never writes the ring: the actions program publishes the
light wanted and reads the state back ([actions.md](actions.md)). A bus
that cannot be used is said once in the voice's log and the ring runs as
it did, in its first light. The light is kept by the bus, which lives in
memory: after the board restarts the lamp is back in its first light,
unless `LAMP_LIGHT_KEEP=on` (#1468). Then the ring also keeps the person's
light in `/var/lib/lampOS/light/person.json`, written when it changes
(whole, beside it and renamed; at most once a second, the last change of
a burst written a second after the one before), and begins with it after
a restart when the bus has none: a light asked for since always comes
first, and a file that is not a light the ring has is passed over for
the first light. Only the person's light is kept, never a test's colour,
a request or a turn's face. That holds with `LAMP_LIGHT_REQUESTS=on` in
both the voice's unit and `lamp-skills`', as the product has them:
without it a skill's light goes through the actions part on
`light/command` ([skills.md](skills.md)), and a light a skill dims and
would give back is kept as the person's if the board restarts before it
is given back. A write that fails is said once and tried again each
second. The write runs in the ring's loop, at most once a second and
only after a change; its time on the lamp's disk is not yet measured
(the lamp check reads the ring's stall lines). The reset empties
`/var/lib/lampOS/light`, a file left beside it by a power cut included.

**While the microphones are off** (#1816, `LAMP_LIGHT_WHILE_MUTED=on`,
off unless set). The ring's light lives in the voice's ring worker, which
runs only inside a session. While the voice waits out a mute or a closed
privacy switch, no session runs, so nothing said `light/state` or took
`light/command`, and the app's light row found "the lamp's light is not
running" (mobile on lamp-0c89, #1396).
- **With the switch on:** on a lamp with a ring and the bus on, the
  muted wait opens the same light, with `LAMP_LIGHT_KEEP`'s file if that's
  on. It ticks it at each look of the wait, about every 2 ms, and the
  light looks for commands at each of those ticks (#1806; every 50 ms
  before).
- **What it says and takes:** the person's light is said once a second and
  at each change, and a light asked for is taken, so the app can read and
  set it. A command reaches `light/state` at the light's next look, about
  2 ms (50 ms before #1806).
- **Nothing is drawn:** the ring shows what the wait shows today, the
  privacy face, the asleep face or dark, and nothing goes on
  `light/drawn`. Requests (`light/request`) are not taken while muted. A
  self-test colour is read, as the light reads one, but never drawn, and
  `light/state` goes on saying the person's light (the reviewer on
  #1842).
- **When the switch opens:** the wait lets the light go before the next
  session's ring worker opens. That worker begins with the bus's newest
  command, so a light set from the app while muted is the one shown.

Tests: `coordinator::tests::service_mute::the_persons_light_is_said_and_taken_while_the_switch_is_closed`
(a wait that never ticks the light fails it) and
`ring_light::tests::the_light_while_muted_is_off_unless_on_with_a_ring_and_a_bus`.

**On lamp-4ace, 2026-10-14, about 03:00, nobody in the office.** Called
directly on the actions program's socket, each change was answered in 15
to 108 ms with the state read back, and a colour that does not exist was
refused. Stills through the office Mac's camera showed the ring blue at
levels 1, 3 and 5, red at 5, and pink, white, purple and green at level
1, each readable as its colour; and the ember, in its second form, as
eight amber dots. Asked aloud by a played voice, ten requests: nine did
what was asked and said so ("There, blue." "Red it
is!" "Better? That should be a bit easier on the eyes." "Back to warm,
just how I like it."), one of them a follow-up with no name in it. The
tenth, "make your light blue", reached the model as something else and it
made the light brighter, and said that it had.

Before that, the first four requests found a fault. The light changed
each time and the lamp said nothing: the model did what was asked and
then called its function for "that was not said to me", and the
character file still told it that it could control nothing. One of the
four, misheard, turned the volume up. The character now says what it can
change and that it is to say so, and a decline is no longer taken in a
request the lamp has acted on ([tools.md](tools.md)).

Not shown: a person asking. How the levels look to someone in the room,
and whether level 5 is a useful light under a ceiling of 24, a camera
cannot say: it saturates on the ring. The light is not restored after the
board restarts, unless `LAMP_LIGHT_KEEP=on` (above; not yet seen on a
lamp).

Seen through the office iMac's camera on lamp-4ace on 2026-10-12 with a
ceiling of 40: across eight stills of the resting face the dark centre of
the head, which reflects the ring, varied between 45 and 59 of 255 while a
patch of floor stayed at 167 to 170 in seven of them; during one exchange the
centre read blue while a question was played, warm while the lamp answered,
and fell back to the resting level afterwards. The ring itself saturates that
camera in every state, so these stills say nothing about brightness as a
person sees it, and the circling point was not caught in a still.

The cue signals accepted conversation state, not proven
social understanding: the directed voice harness still has false interruptions
from playback and does not know whether ambient speech addresses Lamp.
ALSA acceptance/retirement are software/driver boundaries, not optical or
acoustic measurements. Stillness, silence and an unlit ring are valid behavior.

## Explicit qualification option

Append `--ring-channel-ceiling 0..120` to `directed`, `directed-fixture` or `serve` to
include the ring worker. A value of zero still starts the guarded writer but
caps its output at black. Without the option no ring device is opened. Repeated,
missing, negative or out-of-range values fail argument validation. The option
is independent of audio diagnostics and software noise suppression.

```sh
target/release/lamp-voice directed /absolute/private/provider.json 45 /new/private/run --diagnostics --ring-channel-ceiling 24
```

This flag isolates qualification experiments; light remains part of the full
release scope. The command requires exclusive hardware ownership and does not
stop legacy services itself. It opens no motor bus.

## Ordering and deadlines

The coordinator publishes input-end, matched playback-start/end and turn
completion immediately. New input, interruption and privacy already have an
authority path. A cue cannot authorize itself: the parent publishes its
snapshot on priority control before sending its frame on the separate data
socket. The worker rereads control after receiving data. All phase publication
also runs in an audio-only session; that mode avoids SPI but is not identical
to the earlier control-message timing.

The policy retains one pending immutable frame. It creates a new phase-bound
Light permit, normally renews the static cue every 20 ms, and waits at most
100 ms for its matching receipt. This is lease maintenance, not an animation.
The original permit is clipped by actual input/authority leases. A heartbeat
cannot extend an already displayed frame's permit. A phase change can replace
the last cue after the outstanding receipt retires; it never relabels old work.

The writer's cooperative tick is 2 ms. Each tick shares a budget of 16 priority
control packets, retains at most one data frame and uses its original request
time. Data expires after 100 ms. An older heartbeat cannot suppress a newer
privacy revocation. A newer expired authority packet fails closed; it is not
silently ignored in favor of older allowed state: the ring is blanked at once
(`GuardedRing::lapse`, reported as authority lost, expired), and no frame or
rest is painted under the older lease until an authority newer than the late
one installs. Malformed/unknown control fields, mismatched lineage and
feedback backpressure are faults.

A lapse is not the end of the ring (#1178, lamp-4ace at 00:00:06 on 17 Oct:
the voice's loop held up past the lease, 250 ms at rest and 100 ms during a
turn, while the lamp's midnight log jobs ran; the ring worker ended on it. A
ring fault only disables the ring, so that alone did not end the voice: another
part's expired authority did, and the ring's end then made its cleanup fail,
first builder and reviewer on #1178). An installed authority that
runs out with no renewal leaves the ring's own guard to blank the cue and
refuse every frame and rest under it, and the worker waits, dark, for a fresh
one; a newer one that came past its time does the same, as above, with one
blank however many late ones follow. Each is one
line in the journal when it begins (`dark until a fresh one`) and one when it
ends (`it goes on`, with how long). Only no authority at all within 250 ms of
the start still ends the worker (`ring authority never came`).

The software qualification target is at most 50 ms from policy request to
write completion, with no microphone/speaker work waiting for SPI. A late
write or a receipt not back within 100 ms (a `TimedOut` fault) comes of a loop
or a ring held up a moment (#1178), so the choreography starts afresh rather
than leave the lamp listening with no light for the rest of the serve cycle.
A frame whose receipt has not come is held: no other frame is asked for until
that frame's late receipt comes back, which is taken once with every check
but the 50 ms target, so one hold costs one resync however long it lasts;
then the next frame goes out and the light comes back. A late write whose
receipt is already in holds nothing. `ring_resynced` in the session trace and
one journal line say so. At most three holds are forgiven in ten minutes
(`RESYNCS_PER_WINDOW`, `RESYNC_WINDOW_US`); past that the ring is slow, not
held up, and fails qualification. A frame held 2 s with no receipt at all
(`HOLD_MOST_US`) is a hung ring, which no resync forgives. Either way further
ring presentation is disabled, as for any other ring fault, and healthy voice
continues. This boundary excludes phase recognition
before the request and physical light onset afterward. The release's visible
transition target therefore still needs a real-device measurement.

Finite sessions give the ring worker a 630-second total lifetime cap, covering
the maximum 600-second directed run plus startup/shutdown reserve. In `serve`,
the coordinator selects its private continuous-worker lifetime while keeping
the parent, privacy and frame-expiry watchdogs unchanged. Authority expiry is
independent and much shorter: Controller snapshots are bounded to 250 ms, and
active input leases are 100 ms; an installed authority that runs out holds
the ring dark until a fresh one comes, and no longer ends the worker (#1178). Audio diagnostics retain their
separate 600-second wall/sample cap; diagnostic runs must leave startup room.

## Bounded renewal evidence

The summaries below describe finite directed sessions. Continuous `serve` emits
append-only ring observations into its bounded best-effort metadata writer; it
keeps no mutable index into an evicted log batch. Choreography and cleanup state
remain independent of logging loss. See [continuous chat core](serve-runtime.md).

Lease renewal still sends a real authority packet and frame every 20 ms when
eligible, and validates every returned receipt. Trace retention is separate:
logging two events per renewal previously filled the 20,000-event cap after
about 200 seconds with a continuously active cue. The cap is unchanged.

The first request and receipt for each owner/phase/ceiling remain individual
`ring_requested` and `ring_feedback` events, as do new playback occurrences
within the same turn. A cycle back to the same phase also stays individual
when its presentation lineage changed, even if no intermediate cue was sent.
During an unchanged cue, the first pair of each second
also remains individual. Intervening renewals share one `ring_renewals` record
and one raw `ring_requested` tail sample for that interval. This reduces a
stable cue from about 100 to four retained events per second without changing
any worker/control action or deadline.

Each summary retains the original owner and first/last full frames, request
count, validated presentation/rejection counts, unacknowledged count, exact
first/last presentation receipts and request-to-write latency minimum, maximum
and total. Its `at_us` is the first included request time;
`last_updated_at_us` is the latest actual request/receipt observation. Neither
replaces the worker's original write timestamps. A submitted frame without a
validated receipt remains unacknowledged, including on timeout or shutdown.
Receipts drained only during shutdown remain separate shutdown events.

The coordinator updates those two fixed slots in its append-only in-memory
trace. The tail sample always carries the latest actual request timestamp and
full original frame, including before its receipt arrives. It is marked
`trace_role: renewal_tail`; normal requests use `cue_or_periodic_head`. To count
commands, add head requests to summary `requested` counts; do not count the
tail samples again. Updating a tail can move its timestamp past interleaved
voice events, but never moves indices or changes those events. Consumers must
use original timestamps rather than infer time from array position. It keeps
no renewal history outside the bounded trace and one pending request. Final shutdown needs no summary flush to preserve the last partial
interval. Rejections, black writes, invalid receipts and service faults remain
individual events with the original pending-frame identity where available.
An unvalidated worker claim never increments successful presentation counts.

The voice evaluator currently checks individual `ring_requested` phase/voice
consistency and does not score `ring_feedback` or renewal summaries. Keeping
all transitions plus exact first/last request samples preserves the interval
boundaries used by those checks, including a stale tail exposed by a later
retirement receipt with an earlier driver timestamp. Summary
counts are additional operational evidence, not proof of physical light or
per-renewal acoustic synchronization. Individual renewal percentiles cannot be
reconstructed from min/max/total; do not describe the retained samples as all
writes. A different high-rate event source or exceptionally frequent real
transitions can still exhaust the unchanged global cap and fail the run.

Host regression coverage uses the real coordinator service, Controller,
choreographer and private Unix sockets with a virtual clock and synthetic
worker receipts. It includes more than 10,000 renewal pairs, a 600-second
schedule at 20 ms with dense voice events, phase/playback changes, interruption,
and fault injection. This proves bounded software retention for that schedule,
not physical SPI timing or a ten-minute acoustic trial.

## Failure and cancellation

Light receipts are evidence; they cannot advance the conversation or revive
an old owner. The policy checks request identity, write ordering and the
original permit. An expected supersession rejects old work without erasing a
new valid cue. A bare delayed `off()` callback is never queued for a turn.

Revocation and expiry reach `GuardedRing` even with no new frame. Stop is sent
to every worker before waiting for any one worker, within the existing shared
300 ms cooperative grace (750 ms with diagnostics). A clean ring shutdown
requires an explicit black-write receipt whose timestamps follow that stop
request, plus successful process exit. A zero exit alone is insufficient.

Ring startup and post-handshake failure are optional-capability failures. The coordinator latches the ring unavailable
before one nonblocking Stop attempt and stops sending authority or frames to
it. This does not change the current input, speech owner, playback occurrence,
or audio readiness. Mandatory audio/privacy failures remain terminal. A late
ring event cannot create listening readiness or revive its presentation policy.

The live loop observes at most 16 cleanup messages and one nonblocking child
status per pass. After 300 ms without observed exit, it requests termination
once, without waiting. The child remains owned until reaped; no replacement
writer starts before its exit is observed. The latency target is zero added wait from
observed ring fault to the next audio/control work, within the existing 2 ms
cooperative tick target. Kernel signal/status calls have no hard userspace
execution deadline. Host channel tests establish ordering and bounded work,
not loaded-device timing.

### Slow SPI writes and the ring's restart (#1620)

On lamp-52e6 the ring worker ended mid-session on `ring output fault: SPI
write returned after 13180 us; budget is 10000 us` (the last write of a
late run; the message did not say how long the run had lasted). The voice went
on without the ring, and every later stop of that service failed verified
cleanup, since a ring that faulted never writes the stop's black. Now:

- **Late writes are not a fault until they have lasted 2 s** (#911, kept).
  A write over the 10 ms budget (`WRITE_BUDGET_US`) is a late frame: the
  frame did go out. The writer faults only when a run of late writes has
  lasted 2 s (`LATE_RUN_US`); no single write, however slow, is a fault on
  its own beyond that. An on-time write, or more than 100 ms with no write
  (`LATE_GAP_US`), ends a run (`GuardedRing::transfer`,
  `crates/ring/src/lib.rs`). lamp-4ace's SD/MMC host re-tunes about once an
  hour and holds the bus for 0.3 s, about fifteen late writes in a row at
  50 Hz or one write blocked for most of it: kept, never a fault. The fault
  says how long the run lasted: `…; budget is 10000 us, late for 2010 ms` (2 s or more).
  Each late write kept is said in the journal at most once a minute, the
  newest, with how long its run has lasted and how many went unsaid since
  (`ring_worker::LateSaid`):
  `lamp-voice ring: SPI write 13180 us, over 10000 us, late for 340 ms (2 more since the last line)`.
- **A ring that faults comes back.** Once the faulted worker's exit is
  observed, and no sooner than 2 s after the fault (`RESTART_SPACING_US`), the
  voice starts it again with the same startup
  (`Worker::spawn_optional_ring_with_tick`, the same arguments), on a thread of
  its own: the coordinator's loop only looks at that thread each pass
  (`OptionalRing::restart_tick`) and never waits on it. The new worker writes
  its black before it says Ready, gets the current authority with the next
  20 ms publish (a closed privacy switch keeps it dark, and nothing stale is
  shown), takes the person's light from the bus as at the start, and gets a
  fresh choreography. The old worker's unconfirmed cleanup was recorded; the
  restarted ring's own stop is what a later stop checks, so that stop writes
  its black and is clean. A stop while a restart is under way waits for that
  bounded startup (`settle_restart`, at most the 3 s worker startup), then
  stops the ring it started like any other.
- **At most three a session** (`RING_RESTARTS`). A fourth fault leaves the
  ring off for the rest of the voice's run, said once in the journal and in
  the trace (`ring_off`), and the voice's report on `diagnostics`, which
  `lamp/health` reads, is a warning with the fact `ring` = `off`: `the light
  ring faulted and stays off until the voice starts again`. A ring that never
  started is not restarted, and is reported off the same way.

Journal lines: `lamp-voice: the ring faulted (…); starting it again, 1 of 3`,
`lamp-voice: the ring is back (1 of 3 restarts)`, and past the third
`lamp-voice: the ring faulted again after 3 restarts (…); it stays off this
session`. Trace kinds: `ring_restart` (attempt, of, after_fault_ms, reason),
`ring_restarted` (attempt, worker_boot), `ring_off` (restarts, reason). A
restart that fails to start is a degraded ring like any other, tried again
after the spacing while restarts remain.

Tests (host, fake SPI): `crates/ring/src/tests.rs`
`one_late_write_is_a_late_frame_not_a_fault`,
`an_sd_mmc_retune_of_late_writes_is_kept_not_a_fault`,
`late_writes_lasting_the_late_run_are_an_observed_fault_not_a_claimed_timeout`,
`an_on_time_write_ends_a_run_of_late_ones`;
`crates/voice/tests/ring_worker.rs`
`an_sd_mmc_retune_of_late_writes_is_kept_and_the_ring_goes_on`,
`a_late_run_past_two_seconds_faults_and_a_ring_started_again_writes_black_then_the_light`
(the fake sink's write time is injected through `FrameSink::simulated_elapsed_us`,
a test seam real sinks leave at `None`);
`ring_worker::late_said_tests`; `optional_ring_tests::a_faulted_ring_starts_again_off_the_loop_and_a_stop_after_it_is_clean`
(each restart tick under 50 ms, the 2 s spacing, the current authority, a
clean stop with the restarted ring's black-write receipt);
`optional_ring_tests::a_stop_during_a_restart_waits_for_it_and_ends_clean`
(a stop while the restart's startup is still running: Ok, with the
restarted ring's receipt);
`optional_ring::tests::a_ring_past_its_restarts_stays_off_and_says_so_once`;
`bus_out::tests::a_ring_off_for_good_is_a_warning_with_its_fact_and_the_rest_after`.
Not yet seen on a lamp; see
[verification/ring-slow-write-host-20261019](verification/ring-slow-write-host-20261019/README.md).

`ring_degraded` records unavailability and an initially unconfirmed cleanup.
`ring_degraded_blank` retains validated SPI completion, and
`ring_degraded_exit` separately reports actual process exit. Prior presentation
receipts become historical `ring_degraded_event` records. A validated early
shutdown black write keeps its original stop timestamp through final shared
shutdown; a later Stop does not invalidate that evidence. Failed stop delivery,
malformed cleanup, forced termination, abnormal exit, or missing black-write
proof still fails final cleanup. A clean degraded shutdown may complete the
voice run; its trace still records that ring qualification failed.

The final shared shutdown looks at every child's exit once more, the disabled
ring's included. A ring that had already ended mid-session (its
`ring_degraded_exit` recorded then) is seen ended again in the stop's first
pass, a millisecond or so after `service_stop_requested`; that
`worker_exit_failed` carries `exited_before_stop: true`, so it is not read as
the stop ending the ring (lamp-52e6, 19 Oct 2026, #1620). The stop still fails
on the missing black-write receipt: a ring that ended on its own never said
the strip went black. The ring's shutdown path is the same with or without the
eye (`LAMP_RING_EYE=on` or `strong`), host-tested in
`every_eye_mode_writes_and_says_the_shutdown_black_at_a_service_stop`.

SPI I/O remains in the child. Linux cannot promise a userspace deadline for a
stalled kernel SPI write; the existing 10 ms write budget is checked on return.
A crashed/killed writer cannot itself blank a latched physical ring. Software
unavailability therefore does not claim darkness or another visible state.
A faulted ring is replaced only as above, after its exit, at most three times
a session; a ring that never started is not. An unavailable executable or failed startup
handshake disables only the ring; an already-created unready child remains owned
through the same bounded cleanup path. Required-worker and termination errors
during that wait stay terminal. Startup has a separate 16-event optional evidence
allowance and explicit overflow count. A fresh private ring socket namespace is
removed only with exit and inode checks; unsafe replacements remain visible
cleanup failures. Startup cleanup is not an optical-darkness receipt. See
[private socket startup](worker-startup.md) for the boundary and host tests.

## Verification without Lamp

The policy tests exercise real Controller transitions, stale and duplicate
feedback, phase changes, privacy, original leases and time bounds. Worker tests
use local sockets and fake sinks to cover cross-channel ordering, delayed
authority, cancellation, expiry, malformed controls, bounded service and both
primary/cleanup failures. Test fixture children acknowledge shell exec through a
bounded one-byte pipe before a closed-peer test can drop its socket. Under the
local ARM/QEMU setup, returning from child spawn alone left inherited peer
socket descriptors briefly alive; a successful send then meant kernel enqueueing,
not a ring-policy failure. The barrier changes test setup only: production uses
its existing worker handshake, and authority-send errors remain strict. Missing
or malformed fixture markers fail setup and kill/reap only that owned child.
The process probe additionally starts a separate child with a memory-only sink:

```sh
cargo run --locked -p lamp-voice --example ring_process_probe -- --samples 120
```

It uses synthetic conversation transitions, including simulated accepted
playback. Its JSON retains ordered samples, first/warm timing, startup and
shutdown evidence. It opens no microphone, speaker, SPI, camera, motor or cloud
connection. Host test and timing results do not establish Linux/ARM64 behavior,
physical darkness, color quality, speech/light synchronization or an end-to-end
voice improvement. Keep those results separate in qualification reports.

See [reuse decisions](reuse-decisions.md) for the V1 behavior retained and
changed, and [the ring driver](../crates/ring/README.md) for protocol and safety
boundaries.

### Host qualification, 2026-10-10

The source-only copy outside the parent repository passed **535 workspace
tests**, with zero failures or ignored tests, strict workspace/all-target
Clippy, formatting and a host release build of `lamp-voice`. The Rust source
manifest SHA-256 is
`50cb160bc188b910b35117617448337265b2ab104f85c133c2b2b91772c91d97`.
Exact commands, logs, ordered timing samples and file hashes are retained
privately in `artifacts/ring-live-20261010-50cb160b/`.

| macOS process probe, development profile | Result |
|---|---|
| Samples | 120, synthetic phases and memory-only sink |
| Worker startup | 7.049 ms |
| First request to write completion | 2.342 ms |
| All-sample p50 / p95 / maximum | 1.758 / 3.177 / 3.391 ms |
| Warm p50 / p95 / maximum, 119 samples | 1.758 / 3.189 / 3.391 ms |
| Shutdown | Matching post-stop black-write receipt and successful child exit |

The local Linux VM did not remain running, so this slice has **no new Linux or
ARM64 qualification**. It has not been deployed. The earlier 518-test native
audio checkpoint does not cover this source. Real SPI duration, physical light
onset/off, startup/crash recovery, brightness and concurrency with actual audio
remain unmeasured. No before/after voice speedup follows from this probe.

### Renewal evidence qualification, 2026-10-11

Before the correction, the production service with a virtual clock and private
Unix sockets failed at zero-based renewal 9,999, retaining 19,999 events
(199.98 simulated seconds). The same 10,050-pair workload now completes with
804 retained events. A separate 600-second simulated schedule completes
30,000 socket request/receipt pairs and retains 19,735 events, including
16,800 voice events, 256 startup/shutdown reserve events, 19 cancellations and
a visible terminal worker fault. These are synthetic workload results, not
a physical ten-minute conversation or a claim that every event rate fits.

The exact integrated standalone source passed **768 host tests, zero
failed and zero ignored**, formatting, strict Clippy and both release builds:

```sh
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=1
cargo build --locked --offline --release -p lamp-voice -p lamp-voice-eval
```

Rust/Cargo/toolchain manifest: `962c876dae67917da4c831afb29978bb4d011f0b55b04b3257e4ea176033b374` (167 files;
documentation excluded). Host: macOS x86-64. Sources were unchanged throughout
the sequential gates; local socket tests ran outside the network sandbox.
Independent review checked integration with playback occurrence identity,
whole-turn completion, lifecycle cues and the evaluator's original timestamps.
The original cap failure, hidden presentation-lineage and stale-tail red
regressions, scoped receipts, combined gate logs and source archive are retained
in `artifacts/ring-renewals-20261011/` with a separate durable backup.

This checkpoint does not qualify ARM64/Linux, Gemini, physical SPI timing,
light visibility, real interruption behavior or end-to-end voice latency.
No actor command, lease, timeout, brightness limit or trace cap was relaxed.

### Renewal bookkeeping cost on the host

A separate release probe compared baseline `e8b961c79` with the same ring patch
as the integrated checkpoint. It ran four fresh processes in before/after/
after/before order, each with 2,000 complete request/receipt pairs over real
private Unix sockets. The serialized request payloads matched across all four
runs, including three cancellations and twelve phase changes per process.
Both versions used the same virtual interaction clock and synthetic receipts.

| Repeated service call | Before p50 / p95 / max, µs | After p50 / p95 / max, µs |
|---|---:|---:|
| Request: authority checks, control/data sends and trace mutation | 9.146 / 15.379 / 72.499 | 37.916 / 54.210 / 159.709 |
| Feedback: receive, decoding, validation and trace mutation | 9.724 / 16.054 / 55.640 | 38.804 / 54.336 / 146.246 |
| Idle: no due renewal or queued receipt | 0.939 / 1.600 / 19.441 | 1.220 / 1.812 / 17.649 |

Each cell uses 3,998 samples, excluding only the first call per process.
First requests were 83.921 and 103.477 µs before, versus 93.366 and 134.917 µs
after. First feedback calls were 34.274 and 35.676 µs before, versus 38.021 and
58.929 µs after. No outliers were removed. Each run retained 4,000 events before
and 160 after; event count is not a byte-memory measurement.

The richer summary costs about 29 µs more per typical request or feedback call
on this Mac. Every measured call stayed under 2 ms, including first use. This
does not establish a deadline for a loaded coordinator or ARM hardware.
Timers exclude fixture construction, planned Controller transitions, mock-peer
actions, SPI, worker scheduling and file serialization. First use starts after
fixture construction, not at cold OS startup. CPU affinity, thermal state and
real-time scheduling were uncontrolled; compilation had finished before timing.

Raw nanosecond samples, first-use results, category breakdowns, outliers,
instrumented probe sources and source/build/binary hash receipts are retained
with the checkpoint evidence. This comparison measures bookkeeping cost;
speech response time and physical light timing remain separate open checks.
