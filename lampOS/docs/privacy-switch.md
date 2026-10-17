# Microphone privacy switch (#755)

This route is selected by `LAMP_PRIVACY_CONTROLS=on` in **Controls, Voice and
Ears together**. It is initially off for First's comparison minute. Once First
accepts it on lamp-4ace, the default promotion belongs to First under
[joining.md](joining.md#the-ready-line). Unset/off preserves the existing
direct-GPIO Voice path and original Ears loop; that older Ears loop does **not**
close MIC1 in response to the physical switch. Updating only Voice therefore
does not implement privacy for both microphones. Only the exact value `on`
selects the route, in all three; any other value, an empty one included, is off.
Without it, a pins file that has a `privacy` section is refused, as it was before
the section existed.

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
publishes evidence; it cannot open microphones or clear a software mute.

The selected Voice privacy child reads `controls/state` instead of requesting
GPIO. Each accepted observation retains the actual acquisition clock, time,
sequence and reader identity. Hand faults do not become microphone permission.
The selected publishers and subscribers rescan Bus registrations and retained
state every 5 ms; the ordinary 200 ms discovery interval exceeds this route's
50 ms freshness limit. Unselected services retain their existing cadence.
Invalid types, publisher or reader changes, future/stale evidence, reordered
messages, skipped Bus publications and receive overflow end this worker. It
ends cleanly, telling the voice the switch is unreadable (a fault beginning
`privacy switch unreadable: `), and the voice keeps the microphones closed and
reads again with a fresh worker each second instead of exiting (#795). Any
other failure of the worker is still an abnormal exit that ends the service.
Source acquisition sequence can advance several times between publications;
it must advance, but need not be consecutive at the consumer.

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
| `observed_us` | Original physical acquisition; zero only for unknown state |
| `expires_at_us` | Monotonic deadline bounded by both acquisition and owner |
| `permitted` | Fresh physical open and current Voice permission both allow capture |
| `physical_muted` | Observed LOW/HIGH, or unknown; never an inferred user action |

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
