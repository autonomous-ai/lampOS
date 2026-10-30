# Microphone privacy switch (#755)

This route is selected by `LAMP_PRIVACY_CONTROLS=on` in **Controls, Voice and
Ears together**. It is initially off for First's comparison minute. Once First
accepts it on lamp-4ace, the default promotion belongs to First under
[joining.md](joining.md#the-ready-line). Unset/off preserves the existing
direct-GPIO Voice path and original Ears loop; that older Ears loop does **not**
close MIC1 in response to the physical switch. Updating only Voice therefore
does not implement privacy for both microphones. Only the exact value `on`
selects the route, in all three, or `no-slide` for a lamp with no slide (see
[A lamp with no slide](#a-lamp-with-no-slide-no-slide)); any other value, an
empty one included, is off. Without `on`, a pins file that has a `privacy`
section is refused, as it was before the section existed.

**The product selects it.** Its lamps have the switch on chip 1, line 9 (the
lead on #1711, 20 Oct 09:33; on lamp-52e6 Fifth saw the line follow it, with
the camera and the microphone closing). So `deploy/product`'s pins file has
the `privacy` section, and its Controls, Voice and Ears units set
`LAMP_PRIVACY_CONTROLS=on`. With the route off, the switch closes only the
voice's microphone, and lamp-ears keeps reading the room microphone for
`ears/levels` and `ears/sudden`. With it on, Ears captures only under the
voice's lease, which a closed switch withdraws. The first builder turns it on
by default once lamp-52e6 has run it for an hour. A lamp's own pins file
(the factory's head pads, a calibration), which an update keeps, gains the
build's `privacy` section when it has none, with everything else in it kept
(`install::own_pins_follow_build`; the reviewer's P1 on #2143). Without it the
units would select the route with no switch evidence, and the lamp would
never hear. New lamps keep the section through the factory's steps, which
change only the head pads or calibrate through the typed profile. The voice
starts after Controls and so, at a shutdown or reboot, stops before it
(`After=lamp-controls.service` and `Wants=` in the voice's unit; #3697:
with the old order, Controls after the voice, every reboot stopped Controls
first, the voice saw the switch go unreadable, ended with 1, and the lamp
came back muted by the restart guard). An update's round still restarts
Controls before the voice, in startup order, so the old voice closes on the
unreadable switch just before its own stop; that close, then the stop, no
longer leaves the guard (lamp-06c1, 29 Oct 08:33; "A switch unreadable,
then the stop" in controls.md). A voice still holding the line on
its direct route is no obstacle: Controls asks for the line again each
second (below), so the microphones stay closed meanwhile.

Controls takes the single GPIO request for `/dev/gpiochip1`, line 9, pull-up,
electrical LOW = muted. A line it cannot take (another process still holds
it, as the voice's direct path does on a lamp moving to this route, or this
user may not open it) is asked for again each second, with the reason said
once in its log; until then every acquisition is unknown with a fault. Its
start line says whether the route is on (`privacy route on: /dev/gpiochip1
line 9`), or that the switch is on and the pins file has no privacy section. Header pin 11 is the task's hardware-team mapping,
not a measurement made by Third. The explicit pins-profile addition is:

```json
"privacy": {"chip":"/dev/gpiochip1","line":9,"active_low":true,"pull":"up"}
```

It is independent of button/head GPIO and the optional I²C strip. Selecting
the route without valid switch evidence leaves Voice and Ears closed. Shadow
gesture mode does not hide electrical privacy evidence. Controls reads and
publishes evidence; it cannot open microphones or clear a software mute. It
reads the switch in the same loop as the hand, so it runs at real-time priority
30, above the voice. Both the touch and privacy-only loops rest for at least
their thread CPU work, without sleeping again for time spent blocked, and
their health counts elapsed steps and gaps (#2602, [controls.md](controls.md)).

The selected Voice privacy child reads `controls/state` instead of requesting
GPIO. Each accepted observation retains the actual acquisition clock, time,
sequence and reader identity. Hand faults do not become microphone permission.
The selected publishers and subscribers rescan Bus registrations and retained
state every 5 ms; the ordinary 200 ms discovery interval exceeds this route's
50 ms freshness limit. Unselected services retain their existing cadence.
The state's header lifespan (300 ms) and its refusal of a stamp from ahead
are judged by the clock the header names: Controls says its `clock`, the
kernel boot id, whenever its stamp clock is this boot's, and then the bus
and the worker judge both by `at_us` (CLOCK_MONOTONIC), so chrony stepping
the wall clock neither expires a state in flight, nor makes a silence, nor
keeps an old state live longer. A header of another clock has expired. A
state without `clock`, from a Controls build before it, is still read by
the wall clock and is said once in the journal (`lamp-voice: privacy: the
Controls state of publisher … says no clock`), not refused, since an update
round can mix builds (queue #3997, C; [bus.md](bus.md)).
Invalid types, publisher or reader changes, future/stale evidence, reordered
messages, skipped Bus publications and receive overflow end this worker,
with one allowance: until Controls' publisher finds the worker's new socket
(the 5 ms rescan), the worker reads the topic's kept value, and a publication
in between reaches only that file. So each message up to and including the
worker's first datagram, within the worker's first second, resyncs the order
rather than continuing it; the publisher, its clock and the order going
forward are still checked. From the second datagram on, or after that first
second, datagrams or not, a skipped publication ends the worker (#1569 row
45). One that hears no datagram at all ends sooner: after twice
`controls/state`'s declared 100 ms the bus says Controls is late.
It ends cleanly, telling the voice the switch is unreadable (a fault beginning
`privacy switch unreadable: `), and the voice keeps the microphones closed and
reads again with a fresh worker each second instead of exiting (#795). So
it does when the worker ends cleanly before the voice has read its start, as
it does at once while Controls restarts (an update round that switches the
voice before Controls, #1396), and the health says the microphones are off
until the switch can be read. So it does too while Controls' publisher is
not yet up, as at a cold boot where the voice starts first (#3697): the
worker closes its socket as it ends, so the voice's Stop to it is refused
before its exit can be read. That refusal is no cleanup failure by itself:
the worker's clean exit within the wait's 300 ms grace is the verification,
and only a worker still running then is forced and ends the service. Before
the fix lamp-52e6's cold boot of 27 Oct logged `privacy switch unreadable:
privacy publisher unavailable; verified cleanup failed: privacy.c.p:
Connection refused (os error 111)`, exited 1, and came back muted by the
restart guard. The unit's order (above) only orders the starts, not
Controls' publisher being up, so this wait is what holds. Likewise in a
conversation: a privacy worker that closed its socket and has not yet
exited (a refused authority send, a receive on its closed socket at the
run's shutdown) is the switch unreadable, never a fault by itself, its exit
still verified within the shutdown's 300 ms (lamp-52e6's reboot of 27 Oct
16:37: `privacy.c.p: Connection refused; verified cleanup failed: ...
Transport endpoint is not connected`). With a stop already asked for, the
voice then ends with 0. A worker that ends so fast that its sockets are
gone before the voice connects to them (Controls stopped under a running
voice: lamp-52e6, 27 Oct 17:16:43, `No such file or directory (os error
2)`, exit 1) is told by its exit, read within 100 ms, not by that error:
a clean end is the switch unreadable, read again. Any other
failure of the worker, a crash at its start included, is still an abnormal
exit that ends the service.
Source acquisition sequence can advance several times between publications;
it must advance, but need not be consecutive at the consumer.

With `LAMP_PRIVACY_GAP_RIDE=on`, the conversation's worker has one bounded
exception after it published an allowed capture lease: its own missing-acquisition
expiry becomes `PrivacyUnavailable`, and it keeps reading while publishing
only denied/unknown privacy leases. The deadline is one second from the last
accepted acquisition's 50 ms expiry, including the fresh-open gate's 60 ms
stabilization. A late-publication notice may be ridden inside that deadline;
publisher loss/replacement, invalid evidence and sequence discontinuity still
end the worker. The muted wait and switch-off behavior are unchanged. See
[service-privacy.md](service-privacy.md) for the conversation's cut, held input,
recovery and the capture authority's existing lease tail.

The existing physical gate still requires observations younger than 50 ms and
60 ms of continuously fresh open-switch evidence before reopening. Voice's
durable software mute remains a separate inhibit. A closed switch cancels the
active serve cycle; verified cleanup precedes the next observation-only wait
and a fresh full runtime. Opening the switch never restores old PCM, replies
or capture permissions. The worker processes every LOW before later HIGH
values, and a worker which has observed LOW cannot issue another Ears capture
grant. A new worker and current owner authority are required.

**While the voice waits, it says so (#1371).** With the switch closed, or a
mute set, the voice runs no session and opens no audio; until #1371 nothing
said how it was, so `lamp-health` took it for a voice gone quiet (lamp-06c1
read `stale` for 2 h 25 min on 17 Oct). Every look of the wait now says its
health on `diagnostics` (with `LAMP_BUS=on`), a little more often than once
a second and at once when why changes (`bus_out::WaitHealth`), the worst
reason first:
- `ok` "the microphones are off (the privacy switch)": closed, or open but
  not yet steady (a flap stays this one line, not "off" and "well" by turns),
  whatever the mute says;
- `warn` "(muted by the restart guard after a fault, not by choice)" (#960);
- `warn` "the microphones are off until the privacy switch can be read",
  only once it has gone unread for 1 s (`WAIT_UNREADABLE_AFTER_US`, the
  wait's own grace), even beside a person's mute; before the worker's first
  reading it is `ok` "(reading the privacy switch)", so a wait never opens
  on a fault that is not there (the reviewer and Jarvis on #1376);
- `ok` "(asleep: a hand held on the head)", then "(muted)";
- the switch read open and nothing holding it: an empty `ok`, `link`
  `connecting`, said once more as the wait ends, until the session's own
  report.

While a due reminder waits for the microphones to open, each of these
adds "; a reminder is owed, to be said when they open" and the fact
`reminder` `owed`, at the same level; the reading and opening reports never
do, since every start of the voice passes through them. It is said at once
when one comes due or is cancelled (#1545, schedule.md).

`lamp-health show` says it on its first line, `The lamp is well; the
microphones are off (the privacy switch).` A muted voice is no longer stale,
so `lamp-keeper --act` (error or stale for 30 s) no longer restarts a voice
that was only muted.

## The second microphone

Ears owns MIC1; Voice normally owns MIC2. `voice/privacy`
(`lamp_msgs/MicrophonePrivacy`, version 1) is Voice's short permission lease
for Ears, with this structure:

| Field | Meaning |
|---|---|
| `stamp` | Kernel boot clock, publication time and nonzero publication sequence |
| `session_id` | Sixteen-hex-character Bus publisher boot; matches the envelope |
| `controller_boot` | 32-hex-character private IPC owner boot |
| `microphone_generation` | Owner's nonzero microphone generation |
| `observed_us` | Original physical acquisition (with `no_slide`, the Controls publication); zero only for unknown state |
| `expires_at_us` | Monotonic deadline bounded by both acquisition and owner |
| `permitted` | Fresh physical open (or fresh `no_slide` publication) and current Voice permission both allow capture |
| `physical_muted` | Observed LOW/HIGH, or unknown; never an inferred user action |
| `no_slide` | Only with `no-slide`, and left out otherwise: the lamp has no slide and `physical_muted` is unknown |

The topic is Latest, renewed on changes and at a 10 ms scheduling interval,
with a 50 ms declared lifetime. A permit expires no later than 50 ms after its
original acquisition or the current controller deadline, whichever comes first.
The controller's existing maximum authority lease is 250 ms: if the main owner
stalls but this child and Controls continue, that controller deadline still
limits renewal. A blocked/dead privacy child cannot renew the shorter Bus lease.

Ears also holds Voice to its latch: once a `voice/privacy` session has reported
the switch closed, Ears refuses any later permit from that session, and only a
new worker's session can reopen MIC1.

Ears has one capture owner and one bounded Bus pump. Its selected capture reads
are nonblocking; it checks permission before opening, before and after reading,
and before handing on derived sound metadata. Revocation, expiry, generation
change and queue loss discard held samples and drop the actual PCM handle.
Driver discard failure still drops that handle. No PCM is queued to the Bus
pump. Slow Bus I/O cannot keep that handle alive; native ALSA open/configure,
read/discard and process scheduling still require device timing measurements.
Derived metadata already inside a Bus send cannot be retracted; its original
timestamp is retained. See [Ears](ears.md) for that boundary.

## A lamp with no slide (`no-slide`)

On a board with no slide fitted, the pins file has no `privacy` section, so
with `on` the voice fails closed for good ("Controls has no selected privacy
source") and the microphones never open. Every lamp built to the hardware
contract has the slide (chip 1 line 9), and an update gives its own pins file
the section (#2143), so this is for a board without one. `LAMP_PRIVACY_CONTROLS=no-slide`, set in Controls,
Voice and Ears together, runs the same route on a lamp with no slide.

**What grants.** Controls reads no line, and its start line says `privacy
route on, no slide: the voice grants from its mute alone`. It refuses to
start if the pins file has a `privacy` section (a slide is wired: that lamp
is `on`); its replay under `no-slide` is as off. Each
`controls/state` it publishes carries `no_privacy_slide: true` and no
`privacy`, at the route's cadence: every 20 ms, with the selected bus's 5 ms
rescan and 1 ms step, as with a slide reader. The voice's privacy worker,
told by its `--no-slide` argument (its environment is cleared), takes each
fresh, ordered publication from the same Controls session as one observation,
read as not muted, with every envelope, session, order and 50 ms freshness
check a slide's state has. Its gate needs 60 ms of such steady observations,
as for a slide read open. The lease then needs Voice's controller to allow
the microphone. It says `no_slide: true`, `physical_muted` unknown,
`observed_us` the publication's time, and expires no later than 50 ms after
it. Ears captures only under that lease, with the same lineage, generation
(`microphone_generation`), epoch and fence as with a slide; its observation
regression check now applies to any lease with a nonzero `observed_us`.

**What closes it.** The software mute: a long press, or a hand held on the
head for sleep, sets Voice's controller to Denied, which revokes the lease and
ends the cycle exactly as with a slide. Without a slide **the software mute
is the only gate**. [controls/src/mute.rs](../crates/controls/src/mute.rs)
says its record "never grants permission to open a worker", and this page
calls the mute "a separate inhibit" beside the slide. Both stay true (a grant
still needs the fresh Controls marker and the controller's own permission),
but on a no-slide lamp nothing else can close the microphones. So the voice
refuses to start with `no-slide` and no `--controls-mute` (the persisted
record): a restart would forget a mute. Losing Controls, a
stale, skipped (past the first datagram, as above) or reordered publication, a publisher restart, or a state with
slide evidence or without the marker ends the worker as unreadable, and the
microphones stay closed until a fresh worker reads fresh markers again.

**Both must agree, or it fails closed.** A missing `privacy` section is never
read as "no slide": only `no-slide` in the voice together with the marker from
Controls grants. A voice with `no-slide` refuses a state with `privacy` or
without the marker; a voice with `on` refuses a state with the marker. A state
with both is invalid (`ControlsState::check`). `physical_muted` is never
inferred and no `PrivacySwitch` is ever made up: on a no-slide lamp
`physical_muted` stays unknown, and `PrivacySwitch` stays an electrical
observation only.

**The ring.** Only a verified physical LOW paints the red breath, and a
no-slide lamp has none, so its ring is as with the route off: the asleep face
(`LAMP_RING_ASLEEP=on`), the tap face and the reminder pulse are allowed, and
the wait never starts the muted indicator worker (`worker ring --privacy`), so
the ring keeps one writer. With a slide, a hand's sleep still shows the
asleep face while the slide is open, and gives the ring to the red breath
when the slide closes ([ring-choreography.md](ring-choreography.md), "A
hand's sleep", #1362). Controls never opens microphones or clears a
software mute.

**Expression's head touch.** `lamp-expression serve --head-touch` reads
`voice/privacy` and counts a valid lease as the switch open when it says
`physical_muted: false`, or `no_slide` (`runtime::privacy_muted_in`): a lamp
with no slide has no switch to close, so a head tap looks up as on a lamp
without the route, muted by software or not. The unknown lease, with the
Controls word stale, still counts as closed.

**Classes stay off without a yes.** lamp-ears' room sound classes (#1923,
`LAMP_EARS_CLASSES=on`) start on `on`, a lamp with a slide, as built. With
`no-slide` they start only with the person's stored yes from setup (the
lead's decision, #1711, 07:33 on #1722): `classes::start` reads
`/var/lib/lampOS/consent/room-sounds.json` (Seventh's #2110, `root:lampos`
0640) as lamp-ears starts, and only a JSON object whose `room_sounds` is
exactly `"yes"` counts. Missing, unreadable, over 4096 bytes or anything else
is no; then no classifier runs, whatever the switch says, and the journal
says so once. While it runs, the classifier reads the yes again every 2 s
(`YES_EVERY_US`): once a forget or a redo of setup has removed it, the
window is dropped, `ears/classes` says `privacy`, the journal says so once
and the classifier stays stopped until lamp-ears starts again.

**Face reads stay off without a yes.** lamp-eyes' read of how the near
face seems (`--seems-blendshapes` in the product's unit, `--seems` for our
own lamps' measure; `humans/seems`, eyes.md) is gated the same way, on every
lamp, slide or not (the reviewer-lead's P2 on #2872): when a read is due,
`seems::due` reads `/var/lib/lampOS/consent/face-expressions.json`, beside
the room sounds' yes and kept the same way (`root:lampos` 0640 in the 0750
folder lamp-eyes, in group `lampos`, can traverse), and only a JSON object
whose `face_expressions` is exactly `"yes"` counts. Missing, unreadable,
over 4096 bytes or anything else is no: no face is read and nothing goes on
`humans/seems` or into `--seems-log`. The yes is read again every 2 s while
a face is near, so a removed yes stops the reads within that and the one read
already running. The journal says once each time the yes comes or goes. The
eyes still look: the camera, the gaze, presence, who is there and the
speaking read are unchanged. The voice's
onboarding writes it: with `LAMP_FACE_EXPRESSIONS_ASK=on` it asks once
whether the lamp may notice whether people look happy or upset, to
answer more warmly (never said aloud, no picture of a face kept), and keeps
only a yes: a yes for this lamp, covering everyone it sees, not the yes of
the one who answered; a no or silence leaves no file, as never asked does. `redo_setup`, a
forget of everything and the reset plan remove it, as `room-sounds.json`
([onboarding.md](onboarding.md), "Face expressions"). Those are the only
ways to take it back: a forget of everything, a redo of the step, or the
reset. There is no separate "stop noticing my face" yet. Without the ask (the
product's unit doesn't set it yet), a test lamp's yes is written by hand, as
root:
`printf '{"face_expressions":"yes"}' > /var/lib/lampOS/consent/face-expressions.json`,
`chown root:lampos` and `chmod 0640` it, and remove it after.

**Only for a board with no slide.** Controls refuses `no-slide` only when the
pins file has a `privacy` section, and no pins file has one today. A slide
that is fitted but not declared (the product's hardware contract gives it
one on chip 1, line 9, `hardware-contract.md`) can't be detected here, and
`no-slide` on such a lamp ignores it. Set it only on a lamp with none fitted,
as lamp-4ace (`creature.md`).

**Ears doesn't check its own route.** An Ears set to `on` takes a `no_slide`
lease. That's safe: the voice makes one only on Controls' marker, which
needs `no-slide` in Controls and no `privacy` section.

**Rollback.** Builds before `no-slide` read it as off, in all three services.
Rolling back the binaries with the units unchanged is today's behaviour
exactly: the direct GPIO privacy worker, the original Ears loop (which does
not close MIC1 for privacy) and Controls with no privacy route. An older Ears
reading a no-slide lease finds it invalid (a permit with unknown physical
state, or an acquisition with no level) and stays closed.

Tests: `crates/controls/tests/it/no_slide.rs`, the no-slide tests in
`crates/voice/tests/privacy_controls.rs` and `privacy_pipeline.rs`,
`crates/msgs/tests/it/privacy.rs` and `controls.rs`, and Ears'
`a_no_slide_lease_opens_capture_and_expires_at_its_observation` and
`a_no_slide_observation_regression_is_refused`. The real worker's
`the_real_worker_with_no_slide_grants_from_its_controller_alone`
(`privacy_worker.rs`) runs on Linux only. No lamp was used.

## The switch in the lamp's health

With the switch attached, Controls sends a second item on `diagnostics`
beside its own report, once a second: `privacy switch`, from the switch's
own fresh reading (`privacy::Source::health`, the first builder on #2143).
Its message is `open` or `closed`. Until the switch has changed since
Controls started, it adds `, not moved since Controls started`, and
`changes` counts each change of the level read, so a lamp whose switch
never moves is noticed on `lamp/health` and in `lamp-health show`. Either
is well (`ok`): a still switch is not a fault, and a warning would mark the
whole lamp all day. A reading that can't be had is `warn`, `unknown:` with
its fault; Controls' own item already says the route is closed then.

## Muted appearance and timing

During the quiet physical-mute wait a separate ring worker may show the V1
dark-red indication: a four-second breath with red 2–10 and green/blue zero.
It requires fresh physical LOW and matching denied owner authority. Software
mute alone, missing evidence and an open switch do not authorize that light.
It neither paints a listening cue nor grants capture. It shows only a verified
LOW: while the switch cannot be read (#795), as when Controls is lost, the
voice stays up with its microphones closed and the ring is dark, not red, as
the ring crate requires of unknown evidence. That is the decision: the first
builder's 13:50 on #795 took "stays up, closed" as the answer to their 10:08
ask, and the red breath means only a switch read closed. The existing active
ring
must have stopped first; a shutdown black-write receipt **and** process exit
are required before its replacement starts. SPI stays outside the privacy and
audio owner loops. See [service privacy](service-privacy.md).

The indicator never reads the bus itself (#795 review). One receive from the
bus passes over any number of messages it refuses, so a flood of them on
`voice/privacy` must not stand between the writer's Stop and expiry checks.
- A thread of its own, `LeaseWatch` (`privacy_bus.rs`), reads the lease every
  2 ms.
- The writer takes the last lease found, if still fresh at that moment, or
  the reader's failure. Its own Stop, Authority and expiry checks come first,
  as before.
- A refused message ends the reader once its receive returns: the receive
  itself passes over any number of refused messages before it does, so this
  is not at once, and that is why it runs on the watch's own thread. The
  writer then blanks and exits on that failure.
- Tests: `the_lease_watch_answers_at_once_through_refused_input_and_lets_a_lease_expire`
  (each answer under 5 ms during the flood, no lease past its freshness, the
  failure kept) and
  `stop_during_refused_input_on_the_lease_topic_is_served_at_once` (the
  writer's real step, Stop served in under 10 ms, nothing red written).
- **At its start the reader resyncs as the worker's fence does** (#1569 row
  45, Third's note on #2244). Until the voice's publisher finds its new
  socket, it reads the topic's kept lease, and a lease published in between
  reaches only that file. So before its first datagram, in its first second
  (`resyncs`), a lease may follow a gap and skip sequence numbers. Its
  session, controller, order, time and generation are checked as ever, and
  after either bound a gap ends it as before. Before this, the first
  datagram's gap ended it, so the writer blanked and the red was lost at the
  start. Tests:
  `the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`
  and, once a lease has come as a datagram, a gap ending it inside its first
  second: `after_a_datagram_a_gap_ends_the_lease_reader_inside_its_first_second`.

Design targets, not measured lamp results: actual line read to Controls
publication within 50 ms; accepted mute to both capture gates closed within
100 ms. Controls schedules line reads every 5 ms, publication at most every
20 ms or immediately on a level/fault change, and its selected owner loop at
1 ms. Ears and Voice privacy workers poll at 2 ms. Ordinary mute indication
also targets 100 ms, but follows verified active-runtime cleanup and process
startup. A stalled writer can exhaust the shared 300 ms wait-cleanup grace;
incomplete indicator startup retains the existing three-second startup bound.
Forced exit or an unverified black write is a failure, never permission to
resume. These bounds can exceed the normal indication target. Kernel I/O and
kill/wait are not hard realtime deadlines.

## First's lamp handoff

Third does not access or operate a lamp. The qualification PR and its log must
name the exact native ARM64 run, commit and Controls/Voice/Ears binary hashes.
Use those three artifacts together; Ring and Msgs are linked libraries. This
document does not itself authorize a deployment or claim a gate passed.

1. Run the new binaries for the off minute with the feature unset and the
   existing pins profile. Compare the ordinary voice, button, Ears topics and
   ring behavior with the installed baseline.
2. Stop Voice and Ears before enabling the Controls privacy reader, so Voice's
   old exclusive GPIO request is released. Add the explicit privacy section,
   select the switch in all three services, start Controls, then Voice and
   Ears. Keep First's rollback binaries/profile/unit copies.
3. At startup with LOW, observe `controls/state.privacy.muted=true`,
   `voice/privacy.permitted=false`, no MIC1/MIC2 capture handles and a red
   muted indication. HIGH requires new stable evidence, then a fresh runtime;
   neither software mute nor old held audio may be restored as permission.
4. While listening, thinking and speaking, toggle LOW. Record the physical
   switch edge, original acquisition, Voice receipt, MIC1 and MIC2 discard/drop,
   last newly forwarded capture sample, speaker silence and indicator write
   separately. Repeat cold, warm and rapid flips. Driver return/handle release
   is not an acoustic or optical measurement.
5. Stop Controls, withdraw its switch evidence, restart its publisher and
   interrupt the lease path. Both capture paths must close. Faults remain
   faults; recovery uses a fresh supervised runtime, not a cached HIGH.
   Confirm physical opening cannot clear a restored software mute.
6. Exercise missing/failed SPI and interrupted shutdown. No second ring writer
   may overlap the first; no listening readiness may appear from red/black
   presentation alone. Check that later normal capture starts with no old
   queued samples. Retain failures as well as successful repetitions.

The most likely lamp gaps are actual line polarity/wiring, the selected ALSA
device ownership, cold-process scheduling against the 50 ms freshness budget,
and indicator startup/cleanup latency. Pull-up means a disconnected switch
wire may resemble HIGH; this software does not establish electrical
fail-closed wiring or microphone power removal. First must establish those
physical facts separately. No lamp or acoustic result is asserted here.
