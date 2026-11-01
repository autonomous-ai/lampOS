# Ears: the room microphone

`lamp-ears` (`crates/ears`) listens to the lamp's room microphone and says
two things on the bus: how loud the room is against its own quiet, and when
a sound rises suddenly out of that quiet. By default only levels leave it.
The explicit PCM export below can also publish mic1 blocks for an authorized
recording. It decides nothing. With it stopped the lamp talks on as before.

## Why a part of its own

The lamp has two microphones ([microphone-topology.md](microphone-topology.md)).
The voice uses the second (mic2, the Jieli pair), whose chip filters echo,
steers its beam and evens loudness out: good for speech, useless for how
loud anything is. The first (mic1, the plain capsule on the C-Media
adapter in the front of the base) has none of that. V1 used it for the
room (noise, rain, thunder); nothing in V2 read it until now. Two things
the owner asked for need it:

- **Noise.** A knock, a clap, a door, something dropped: a living thing
  hears it and glances up, then lets it go (the repository's rule 8, and
  V1's ladder for a loud noise, kept as cases in
  [v1-verdicts.md](v1-verdicts.md), sheet HAL-3.5).
- **Who is close.** mic1 rose only 2 to 8 dB over its resting -51.5 dBFS
  for colleagues talking in the office, and heard the owner at the desk
  plainly ([brain-build.md](brain-build.md), "Two things follow for the
  architecture"). If that holds, a voice that is loud on mic1 is a voice at
  the lamp: the sign the busy-room rule lacks when nobody faces the camera
  ([attention.md](attention.md)).

The voice keeps mic2 to itself; the two parts never share a device.

## What it says

| Topic | Type | Kind | What |
|---|---|---|---|
| `ears/level` | `RoomSound` | latest, every 100 ms | The last 100 ms in dBFS; the room's quiet (the tenth percentile of the last 30 s heard while the lamp was not speaking; none for the first two seconds); whether the lamp's own voice is in it |
| `ears/sudden` | `SuddenSound` | event | Its loudest 20 ms, the quiet it rose out of, how long it stayed 10 dB over the quiet in the 300 ms it was watched, whether that was under 200 ms ("short": a knock, a clap) or not (a voice, a chair, music), and how long ago it began |
| `diagnostics` | `Diagnostic` | every second | `ears`: ok, or an error with why the microphone does not open; the quiet now, how many sudden sounds, overruns. With `LAMP_PRIVACY_CONTROLS=on` (the product's), always ok, "microphone observed open" or "closed", with `privacy_controls`, `capture_open_observed`, `capture_observed_us`, `capture_clock_id` and `capture_failures` (`privacy_runtime.rs`) |
| `ears/classes` | `SoundClasses` | latest, every 500 ms | Only with `LAMP_EARS_CLASSES=on` and `LAMP_PRIVACY_CONTROLS=on`, or `no-slide` with the person's stored yes ([privacy-switch.md](privacy-switch.md)): speech, music, laughter, door and footsteps scores (#2562) of the last patch, or why there are none (below, and [sound-events.md](sound-events.md)) ; `own_voice` `present` for a window the lamp spoke into, else `unknown` ([sound-events.md](sound-events.md)) |

**When a sound is sudden.** A 20 ms block at least 30 dB over the quiet
(`--rise-db`, 10 to 60) and at least -40 dBFS, after 200 ms that averaged under 10 dB over the quiet.
It is watched for 300 ms and then said. After one, none for a second.
While the voice says it is speaking (`voice/state`, if no older than 3 s)
and for half a second after, nothing is sudden and the quiet is not
learned: the lamp's own voice reaches mic1 at -13 to -17 dBFS.

These numbers are first choices except the 30 dB: with 20 dB, the office at
work gave 2 to 7 sudden sounds a minute between 16:15 and 16:33 on
2026-10-14 (52 in 18 minutes, all but 7 short, peaks -28 to -33 dBFS:
keys, cups, the start of a word); a glance at those would be a twitch, not
a living thing. One of the 52 rose more than 30 dB (-18.1 dBFS out of
-52.7).

## On the lamp

    lamp-ears --device plug:device_micro1 --rise-db 30 --bus-root /run/lampos-bus

Unit `lamp-ears.service`. **The capture level is pinned** in the product's
unit (`deploy/product/lamp-ears.service`; lamp-4ace's own does not) before
ears starts:
`amixer -c device_cmedia sset Mic capture 4` (-8 dB), the level lamp-4ace's
numbers here were measured at (#694). A lamp left at another level hears
another room: lamp-75c9 at capture 20 (+8 dB) read its quiet at -37.6 dBFS
against -53.6 at 4, with the card's Auto Gain Control on in both, so a 30 dB
rise was out of reach there. A lamp without that card starts ears all the
same (the line may fail). `lamp-ears check FILE...` runs the same listening over
recordings (16 kHz, one channel, 16 bits) and prints each sudden sound.

## Optional microphone privacy boundary (#755)

`LAMP_PRIVACY_CONTROLS=on`, or `no-slide` for a lamp with no slide (whose
lease Voice grants from its software mute alone,
[privacy-switch.md](privacy-switch.md#a-lamp-with-no-slide-no-slide)), selects
the new capture boundary. An absent value or
`off` preserves the existing capture path, CLI and output; no privacy subscriber
or new worker is created. Any other value, an empty one included, does the same,
as in Controls and Voice, so a mistyped unit never stops Ears; a value other
than empty or `off` is named in one journal line, since the room microphone then
ignores the switch. `check FILE...` is unchanged.
The product's unit sets `LAMP_PRIVACY_CONTROLS=on`.

When enabled, Ears starts with its microphone closed and consumes the declared
`voice/privacy` topic (`lamp_msgs/MicrophonePrivacy`, version 1, latest, published
every 10 ms, lifespan 50 ms). Only Voice grants this short lease. Controls switch
observations and `voice/state` speaking metadata never grant microphone access.
Voice must retain both its own microphone permission and a fresh physical
observation; software mute or loss of Voice therefore closes Ears as well.

The receiver checks the exact typed envelope and source, publisher boot, kernel
boot clock, original switch acquisition, expiry, monotonic sequence/time and
controller/microphone generation. Missing, invalid, expired, future, duplicate,
reordered or gapped input closes capture. Publisher, controller or microphone
generation changes close the old handle before another can open. Once a publisher
session has reported the switch closed, Ears refuses any later permit from that
same session, as Voice ends such a worker; only a new session can reopen. Up to 32 retired
publisher sessions are remembered for 50 ms after their retirement, using the
monotonic receipt time of the replacement. An entry is removed only at or after
`retired_at_us + 50_000`: every lease published before retirement has then expired
because its original publication cannot be in the future and its immutable
lifetime is at most 50 ms. Forgotten IDs do not refresh old packets, and the
global publication timestamp high-water is retained. This assumes the Voice
supervisor has retired the previous live publisher before starting its replacement.
A transition that would exceed 32 recent retired IDs closes capture and is
rejected; a fresh transition can recover after entries expire. Clock regression
and counter or retirement-time overflow remain closed until process restart.
A valid contiguous heartbeat can recover after a gap. Neither recovery can reuse
the old partial block, quiet history or pending sudden-sound observation.

The microphone owner holds one ALSA handle and at most one 320-sample partial
block. Reads are nonblocking. It checks permission before open, after open, after
each read and on a 2 ms owner tick even while sound is absent. It discards driver
buffers and drops the actual handle on closure, including when discard reports
an error; it does not merely stop publishing levels. This closes software capture,
not microphone electrical power. Device setup and driver calls still need native
latency qualification; a late return is checked before any sound is used.

One separate Bus pump performs filesystem/socket I/O. Input and metadata-output
queues each hold at most 16 items, drained at most 16 per tick. PCM never enters
these queues; the optional PCM handoff below uses a separate single slot. Input loss, queue overflow, source restart and failed pump work
fence queued output. The original lease is checked again immediately before each
level/sudden publication; later heartbeats cannot extend queued metadata's lease.
A blocked Bus operation cannot prevent the independent capture owner expiring
and closing its handle. A publication already inside a Bus syscall cannot be
retracted and may arrive after the lease expires. Level/sudden metadata retains
its original observation timestamp; these existing topic types do not carry a
privacy lease or guarantee that capture remains open when a reader sees them.
Capture errors use a nonblocking two-second retry deadline and never retry while
permission is absent. Diagnostics carry `capture_open_observed`,
`capture_observed_us`, `capture_clock_id` and the failure count. Their header uses
the same original observation time, so a delayed report does not claim the handle
is still open. Neither output carries sound samples.

The intended boundaries are accepted denial to local discard/drop on the next
2 ms owner tick, and expiry to discard/drop on the first owner tick at expiry.
These are polling targets, not measured hardware maxima. Host fixtures cover
missing/denied startup, malformed and stale leases, slow open/read results,
discard failure, generation and session changes, duplicate/gap fences, held
audio and queued metadata, 128 ordinary publisher cycles, recent and expired
retired replays, and retirement-capacity recovery. Linux ALSA behavior, scheduler
delay, physical-switch latency and electrical privacy are not established by
those fixtures.

## Earlier room-microphone observations

These measurements predate the optional privacy boundary above.

**First look, lamp-4ace, 2026-10-14, 16:13 (first builder, build
`b81c2df7…` of `bb4ad51e4`).** It reads 50 blocks a second with no overrun,
at 1.3 % of a core and 6 MB. The office's quiet on mic1 was -53 to -54
dBFS; twenty seconds of the office at work just before gave -50.7 at the
tenth percentile, -44.3 at the median and -32.6 at the loudest 100 ms.
macOS's "Bottle", "Pop" and "Glass" played from the office iMac about a
metre away: 2 of 6 were said as sudden (peaks -32.4 and -32.8, both short);
those sounds peak about -37 to -39 dBFS over 100 ms at mic1, at the edge of
the first rule of 20 dB and under the present one. A "Hey Lamp" played from the same place reached about -37 to -45
dBFS: no louder at mic1 than the office's own talk.

**Who is close, with people (2026-10-14, 17:12 to 17:42).** With the owner
at lamp-4ace and colleagues around it, the room microphone's rise over its
quiet in the 4 s before each line the voice heard (19 lines, the lamp's own
voice left out): the owner speaking to the lamp at the desk, 10 lines, all
answered, **19.5 to 26.5 dB**; people farther off, 4 lines at 17:41, none
answered, **9.8 to 15.1 dB**; 4 lines at 17:29 to 17:30, not answered,
14.7 to 23.5 dB (likely someone at the desk talking to someone else); one
word at 17:21, 8.9 dB. So the level says someone is close, not that they
are speaking to the lamp: a sign to put beside the name, the words and a
face, not one to decide by. The camera reported no face during the whole
half hour. One session, one office; not a rate.

## What exists, and what is taken

From general knowledge, not a fresh survey:

- **Level and onset** (energy over a running floor, as here): what every
  sound-level meter and V1's environment sensing do. Cheap, explainable,
  and it cannot tell a knock from a word.
- **Sound event classifiers**: Google's YAMNet (521 classes from AudioSet,
  about 3.7 million weights) and PANNs; consumer products use the same idea
  for glass breaking and alarms (Alexa Guard, Nest's sound detection). They
  would name what was heard ("door", "dog", "speech"). A later step if a
  reaction needs to know what it was.
- **Direction**: needs two or more capsules with their raw sound. mic2's
  pair is behind its chip; mic1 is one capsule. No direction here.

Taken: level and onset now; a classifier only when a behaviour needs one.

## Not shown

- Whether mic1 tells a person at the lamp from colleagues across the room
  in general: one half hour with people (above) separated the owner at the
  desk from people farther off, and not from someone at the desk talking to
  someone else.
- Any reaction: Expression's startle (#2822, played from an installed
  skill's file) and Mood read `ears/sudden`.
- The thresholds: chosen, then to be set from what the lamp hears in the
  office.


## Optional mic1 PCM export (#1669)

`LAMP_BUS_AUDIO=on` in Ears explicitly opts into raw room audio export. It is
otherwise off. This requires `LAMP_PRIVACY_CONTROLS=on` or `no-slide`; requesting audio without
that boundary fails before the compatibility capture path can open the device.
No product unit or default is changed by this code. Voice's same variable still
controls its own microphone independently.

Ears remains the only room-microphone reader. It publishes `mic1/audio` as the
existing `lamp_msgs/Audio` Event, `json+pcm`, mono 16 kHz S16LE in 320-sample blocks,
source `ears`. These are Ears samples, not Voice's post-AEC mic2 samples.
The Ears-only declaration and message header expire after 50 ms; the existing
Bus receiver adds its 50 ms transport grace, so blocks older than that combined
window are skipped by `recv`. Voice's Audio declaration is unchanged. Expiry
limits delayed delivery, not authorization to act after privacy revocation;
consumers must still validate current privacy and their own retention policy.
`Audio.sequence` advances for completed admitted capture blocks including drops.
Closing an open capture handle or discarding a partial block also reserves a
sequence gap, so even a short mute/reopen cannot look like continuous audio.
A gap indicates missing coverage, not an exact duration to fill with silence.
The envelope uses the original read-completion monotonic timestamp, not a later
pump time. It is not a hardware ADC timestamp.

The capture owner exposes a complete block only after its post-read lease/fence
checks. The optional handoff uses `try_lock` and a single slot; contention or a
valid full slot drops the incoming block instead of waiting. A stale slot is
cleared before a new offer. The metadata queue never owns PCM. The pump performs
Bus maintenance, then drains privacy again before taking the slot. It releases
the slot lock before packing and uses the guarded Event send with the original
permit and fence checked again.
Later heartbeats cannot extend a held block's permission. Mute, source restart,
lease gaps and owner invalidation retire the old capture era.

Buffer clearing is best effort, not secure erasure of compiler/kernel/subscriber
copies. The final callback cannot retract a datagram already handed to the
kernel. The ordinary Audio payload does not carry a consumer retention grant:
recording must be separately authorized, and this is not approval for the
classifier's two-second retention proposal. See [the review boundaries](ears-pcm-proposal.md)
and [sound-events.md](sound-events.md). Privacy-owner review and native ARM64
validation are required before live qualification.

Host tests cover default-off, privacy-required admission, partial-read/mute and
original deadline behavior, slot contention, stale encoded packets and the
actual guarded Bus API. Target ALSA checks and hardware CPU/on-off measurements
remain separate; host tests do not establish their result.


Audio publisher creation, maintenance or send failures discard the pending
block and retry the optional exporter after two seconds, reporting the first
failure and recovery once. Such an exporter error does not deliberately restart
the metadata pump or discard healthy level/sudden state. A genuinely blocked
filesystem operation can still delay lease delivery, which makes the independent
capture owner close on expiry as before.

Privacy denial stops PCM events; there is no separate "export stopped" event.
While healthy, the exporter can continue refreshing its source entry during
mute. A live source therefore does not prove an open microphone. Consumers use
privacy authority and block freshness, not source presence, for that decision.

The standard deployed bus uses a 2770 root:lampos directory and 0660 sockets;
any process in the lampos group can subscribe. There is no per-subscriber
retention enforcement. `lamp-bus record` can persist raw PCM using the caller's
umask and its serialized-file size cap (64 MiB by default). Operator permission,
private storage and retention limits are required policy, not a property the
publisher can enforce after handing a block to a subscriber.


## Optional room sound classes (#1669)

`LAMP_EARS_CLASSES=on` (exactly `on`) scores speech, music, laughter, a door
and footsteps (#2562) with a pinned YAMNet and publishes them on
`ears/classes`. It is off unless set, and runs only with
`LAMP_PRIVACY_CONTROLS=on`, a lamp with a slide, or with `no-slide`
([privacy-switch.md](privacy-switch.md#a-lamp-with-no-slide-no-slide)) and
the person's stored yes (`/var/lib/lampOS/consent/room-sounds.json`,
`classes::start`); otherwise one journal line says no classifier runs. `LAMP_EARS_CLASSES_MODEL` is the absolute path to
`yamnet.onnx`, with `yamnet.data` beside it; both must match their pinned
SHA256 or the topic says `invalid_model`. No unit sets either.

Capture offers each admitted block, from the same post-read callback as the
PCM export, to a bounded queue of 500 ms; it never waits, and a full queue
drops the block and purges. A separate thread holds at most one patch of
one uninterrupted capture era (under 1.5 s, capped at 2 s) in memory and
writes nothing; any gap, mute, epoch or fence change, overflow or age past
2 s purges it. The Bus pump publishes a score only while that era still
holds authority and the result is under 2 s old. The retention, timing and
measurements are in [sound-events.md](sound-events.md), "The publisher
behind LAMP_EARS_CLASSES". Guard mode's watch reads the topic
(`skills/src/watch.rs`, arrivals by sound only with `LAMP_GUARD_SOUNDS=on`,
which no unit sets), and so does the control door's watch
(`control/src/watch.rs`).
