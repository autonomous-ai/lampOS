# Mood: one inner state

`lamp-mood` (`crates/mood`) is the small, deterministic inner state described
in [Creature](creature.md), task 35 / [#713](https://github.com/autonomous-ai/lampOS/issues/713).
It publishes metadata for its readers. It does not command the body,
ring, working light or speakers, change a microphone's state, or select a
voice reply. Its first readers are the lamp's one breath, the ring's at
rest and the head's in Expression's idle layer, which take its phase while
Mood runs (#1670,
[ring-choreography.md](ring-choreography.md#the-lamps-one-breath-1670)), and
the energy of that idle layer's moves, which its liveliness moves
(`mood_liveliness`, [alive.md](alive.md)); they shape only the resting face
and the idle head, and never decide whether the ring may show anything or
the body may move. Warmth has no reader yet: nothing the lamp shows has a
colour warmth Mood could move.

The component is off unless explicitly enabled. There is no model, transcript
classification, word storage, face history, familiarity learning (the eyes keep how often a
remembered person comes, #1670) or private
restart record. A restart starts from the current local time, with calm
energy and warmth and no attention. Privacy and input/output readiness remain
the existing owners' responsibility.

## Why these dimensions

The research supports a small state used consistently by separate expression
owners; it does not supply measured constants for this lamp. The review and
V1 keep/change decisions are in [reuse-decisions.md](reuse-decisions.md#mood-bounded-inner-state-task-35-2026-10-14).

| Primary source | What is supported | What is not established |
|---|---|---|
| [Anki, US11203122B2](https://patents.google.com/patent/US11203122B2/en) | A simulated state may have several dimensions and influence a separate animation selector. It need not describe human emotions. | A patent is a vendor design disclosure, not measured validation of these dimensions or decay rates. |
| [Lasseter, 1987](https://www.cs.cmu.edu/afs/cs/academic/class/15462-f13/www/lec_slides/Lesseter.pdf) | Staging makes one important action readable; character state changes the pace and posture of the same action. | Animation practice is not a robot user trial or permission for constant movement. |
| [ELEGNT, 2025](https://arxiv.org/html/2501.12493v1) | In a video study with 21 valid participants, average perception ratings on a 0–100 scale were 56.16 for expressive movement and 28.77 for functional movement alone. Benefits were strongest in social tasks; unexplained or exaggerated movement drew criticism. | This is not prolonged desk use. Selected functional-task metrics showed no significant difference; the paper does not say all functional expression is ineffective. |
| [Haptic Creature, 2011](https://doi.org/10.1145/1957656.1957820) | The touch-only study communicated arousal more successfully than valence. | It does not validate brightness, color warmth or inference about this lamp's user. |
| [Amazon Astro documentation](https://digprjsurvey.amazon.com/csad/help/node/G2HDFAZG923JG4W7) | The vendor documents scheduled-item exceptions to do-not-disturb and a summoned exit from Night Mode. | It is not measured proof of noise immunity. The strict wake allowlist below comes from Creature. |

The original UCI Lasseter link could not be fetched during this review;
university copies and indexed original text supplied the relevant passages.
No source is treated as biological calibration.

## State and authored constants

All scalar values are finite and bounded from 0 to 1. Awake describes the
lamp's authored wakefulness, energy its short-lived level of activation, and
warmth its friendly baseline. They are not estimates of a person's emotions.
Familiarity is always `unknown`, an explicitly unavailable slot kept for v1
readers. How familiar a remembered person at the lamp is comes from the eyes
(`familiar`, below, #1670): Mood holds the level only for its lease and keeps
no history of anyone.

| Quantity | Rule |
|---|---|
| Awake day target | 0.70 from 07:00 to 19:00 local time; 0.40 from 19:00 to 22:00; 0.15 otherwise. |
| Company | Fresh company evidence adds 0.15 to the awake target, without accumulating per frame, word or heartbeat. |
| Long quiet | No inactivity penalty for 15 minutes, then a rise to a maximum penalty of 0.55 at 60 minutes. Clamp the resulting target. |
| Awake settling | Approach the target with a two-minute half-life. Name and attending touch raise awake to at least 0.85. |
| Calm energy | Baseline 0.30. An accepted bang adds 0.25 to a transient component with a 15-second half-life and 120-second cooldown. Total energy is capped at 0.70, including the air offset. |
| Warmth | Baseline 0.60. Kindly touch adds 0.05 to an excess capped at 0.25; the excess has a 20-minute half-life and a 10-second cooldown. |
| Future explicit feedback | While awake, typed thanks adds 0.04 and laughter 0.03, within the same warmth bound and cooldown. These are not inferred from words or available as live semantic observations. |
| Smiled at (#1671, `LAMP_MOOD_SEEMS=on`) | Two `positive` reads in a row of the near face (about 4 s) add 0.10 energy, half an accepted bang's, with the same 15-second half-life, and 0.03 warmth, as laughter, within its bound and cooldown. At most once in 120 seconds; a smile held on doesn't add again. Seen only as the resting ring's breath a touch quicker and the idle head a touch livelier for some seconds: nothing turns to the person and nothing is said. |
| Someone low (#1671, `LAMP_MOOD_SEEMS=on`) | Three `negative` reads in a row (about 6 s) let go energy's transient and lower energy 0.10 for two minutes while a face is near: a slower breath and fewer idle moves, keeping them company quietly. A smile, or nobody near (said, or the face's lease lapsing), ends it, and it doesn't come back with the face. It doesn't renew while they stay low: after two minutes the lamp is its usual self beside them. A change of near face doesn't end it or a run of reads, since one person's passing id changes often; a second person who sits down beside someone low finds the lamp quiet for the rest of those two minutes. |
| How a line sounded (#1722, `LAMP_MOOD_TONE=on`) | From the voice's `voice/tone`, once a line, counted against bars (`tone_kind`). **Glad words** (valence 0.5 or over), unless the voice was surely flat (arousal under -0.3 with sure 0.6 or over), are a smile: 0.10 energy and 0.03 warmth, and they end a quiet. **Down words** (valence -0.5 or under) are someone low: energy's transient let go and 0.10 lower for two minutes while a face is near. With no face near, a down line does nothing, and isn't kept for a face that comes after. **A surely lively voice** (arousal 0.6 or over, sure 0.6 or over) with no glad or down words adds 0.05 energy and no warmth. Every brightening, a smile's included, shares the smile's 120-second cooldown, so a smile and a glad line together add once. A calm line alone is nothing: calm isn't sad. Asleep, a line changes nothing. Only the kind is kept, never the numbers, the request or the words. |
| Air | Fresh CO2 at or above 1,200 ppm must remain uninterrupted for ten minutes before an energy offset can grow toward -0.08, with a five-minute half-life. Freshness loss or clear air ends the sustained qualification. |
| What it felt (#3691) | Mood names the feeling behind the numbers in `mood/state`'s optional `felt`. A smile, a glad line or a kindly touch is `glad`, and a surely lively line is `lively`; each holds 90 seconds (`FELT_HOLD_US`) from when it was seen, so the `feelings` skill, which waits for 30 seconds with no lamp speech and a minute after a talk, still sees a glad line the lamp answered. Someone low with a face near, by the face's three reads or a down line, is `low` for the two-minute low spell, and it ends with the spell when nobody is near. A newer feeling replaces an older one, also inside the smile's cooldown, when the energy doesn't move again. While a low spell is on, only a feeling that ends the spell replaces `low`: a smile or a glad line ends it, and is `glad`. A lively line keeps `low`, since keeping someone low company wins over a lively voice, which may not be theirs. A kindly touch keeps `low` too, since a hand on the lamp while someone is low is comfort, not glad; its warmth still lifts. A bang and the air move energy only and are no feeling. Sleep ends any feeling. |

These values are authored starting values, not thresholds fitted to this
lamp, a health standard or a claim about human feelings. Quiet returns warmth
to 0.60; there is no resentment, attention debt or penalty for being ignored.
Air affects energy quietly and selects no attention target.
Quiet means time since qualified social activity: current company, active
Voice, nonblank speech, name, touch, reminder or awake explicit feedback.
A bang or air reading does not reset this inactivity clock.

## Sleep and wake

Sleep is an explicit latch, not merely an awake value near zero. While it is
latched, published awake is zero and attention is empty. Automatic sleep needs
no fresh company or active Voice lease, awake at or below 0.25, and at least
60 minutes of quiet by day or 15 minutes at night. There is no additional
120-second dwell. A fresh explicit sleep request in the core can latch sleep
without waiting for decay; no live producer supplies it yet. Startup at a
night target below the threshold starts asleep. Day startup is 0.70; evening
or unknown local time starts at 0.40. There is no saved mood or sleep file to
restore after restart.

Only three qualified causes can clear a running instance's sleep latch:

- A fresh name event.
- A fresh attending or waking touch.
- A user-set reminder explicitly permitted to wake.

The event must have been observed after the sleep boundary. An old queued
name or touch cannot undo a later sleep, and the gesture that requested sleep
cannot also wake it. Morning, company, generic speech, a bang, air,
thanks/laughter and a wall-clock correction do not clear sleep. A bang while
asleep does not add energy.

This follows Creature's final sleep rule: its earlier morning-company example
does not create a fourth wake cause. A name event is possible only if the
independent input owner permits its detection. Waking Mood cannot unmute a
microphone, override a physical privacy control or advertise listening
readiness. A Controls software-mute request is not a confirmed sleep outcome.

## One attention target

Attention is one of `None`, one current face scoped to its publisher and
passing track ID, touch, or sound with unknown direction. Mood keeps
no collection of past faces and invents no speaker-to-face association.
Name may select an already qualified current face, otherwise sound with
unknown direction. A reminder wakes and clears attention without an invented
target.

| Cue | Priority | Maximum lease |
|---|---:|---:|
| Name or touch | 90 | 5 seconds |
| Face arrival | 20 | 2 seconds |
| Accepted sudden sound | 10 | 1 second |

Higher or equal priority can replace the current target. Lower-priority cues
are dropped, not queued. Expiry clears attention instead of resurrecting a
preempted target. A face disappearing or its company lease expiring also
clears face attention, including a name-selected face before its five-second
maximum. A stream of frames for the same face renews company without renewing
its original attention lease. The sleep latch clears attention and suppresses
ambient attention changes.

These are state leases, not permission to rotate a joint. The single final
actuator owner still checks privacy, freshness, cancellation and safety.

## Published contract

`mood/state` is `lamp_msgs/MoodState` version 1, a `Latest` topic from source
`mood`, with a one-second promised period and a 2.5-second lifespan. It carries
`stamp` (clock identity, monotonic microseconds and positive sequence), the
publisher's `session_id`, `awake`, `energy`, `warmth`, `asleep`, optional
`attention`, `familiarity: unknown`, optional `familiar` (#1670): the level
(`new`, `regular`, `close`) of the remembered person the eyes recognise at the
lamp, from `humans/familiar`, held only for that message's five-second lease and
dropped when the eyes say nobody remembered (no name), and optional `breath` (#1670): the lamp's
one breath, its `phase` from 0 to 1 at the stamp and its `pace` from 0.7 to
1.25 (`lamp_msgs::mood_pace` of awake, energy and asleep). Mood begins it at
the plain breath's phase on the boot clock (5.2 s, `MOOD_BREATH_US`) and
moves it on at the pace whenever its time moves, so it never jumps; a reader
moves it on from the stamp to its own now at that pace
(`MoodState::breath_at`, `breath_in`). When Mood stops, or its state goes
past its 2.5 s, both readers go back to the plain breath together, so they
stay in step, but the breath moves once from Mood's phase to the plain one:
the ring eases there in about a fifth of a second (its 70 ms ease), and the
head at no more than twice its breath's speed. Starting never jumps: Mood
begins its phase where the plain breath is. Face attention carries only the
publisher's 16-character lowercase-hex boot token and a passing ID bounded
to 128 bytes; sound and touch add no invented geometry.

Optional `felt` (#3691) is what the lamp felt while it holds: `kind`
(`glad`, `lively` or `low`, as the table above says) and `until_us`, on the
stamp's monotonic clock. `check()` takes it only when `until_us` is after the
stamp and no more than two minutes after it (`MOOD_FELT_MOST_US`). A state
without it felt nothing to name. Mood publishes a state at once, within its
20 ms floor, when a feeling begins, changes or ends. An older reader ignores
the field, and Expression and the ring read the state as before. Its first
reader is to be the `feelings` skill.

The timestamp belongs to state computation, not a successful publication
receipt. A future consumer must check the payload clock, session, sequence
and age as well as the topic's lifespan, and apply its own output authority.
A newly written bus value containing an old payload does not make that state
fresh. A stopped or blocked publisher is optional; it must not keep a consumer
acting on expired state. The ring's reader takes only a checked state from
source `mood` whose stamp is no later than now and no older than the 2.5 s
lifespan, on the ring's own monotonic clock; anything else is the plain pace.

## Pure engine and clocks

The engine starts with `Engine::new(clock_id, now_us, local_minute)`, returning
an error for invalid input. Events carry `observed_us`, `valid_until_us` and a
typed kind. Kindly touch is explicit metadata distinct from ordinary touch.
Clock identity and timestamps belong to one monotonic clock generation.
No engine operation reads a file, bus, socket, model or local timezone.

`apply(now_us, event)` checks and applies one event; `advance(now_us,
local_minute)` settles elapsed state and supplies a current local-clock
observation. Both report whether the state changed meaningfully.
`snapshot(now_us, sequence, session_id)` returns the checked wire state only
at the engine's current time; it does not advance the clock. Event leases are
half-open (`observed_us <= now_us < valid_until_us`), positive and no longer
than 15 seconds. Each of the twelve event kinds has a strictly increasing
observation watermark. Future, expired, duplicate, out-of-order and pre-sleep
observations are rejected atomically.

Elapsed decay, expiry, inactivity and cooldowns use the injected monotonic
clock. Local civil time supplies only the day target. Midnight, daylight
saving changes and corrected wall time cannot manufacture elapsed time,
replay a morning event or deliver a reminder. Reminder scheduling remains
with Schedule. Decreasing monotonic time is rejected rather than moving a
lease or cooldown backward.

On `advance`, the previous local-minute observation governs the elapsed gap;
the supplied minute becomes effective at the current instant. The engine
does not invent skipped wall-clock transitions. A replay that needs the full
day shape must supply clock observations across those transitions.

Long gaps remain bounded work: settle decay and expire old leases without
replaying each missed tick. Unknown local time does not invent night. A new
process starts a new instance with no attention and neutral energy/warmth.
It does not claim continuity with a previous instance's sleep preference.
Durable privacy preferences remain
independent and authoritative across that restart.

## Live inputs and contract gaps

The adapter consumes existing typed metadata, with source identity, message
shape, age and ordering checks before the pure engine accepts an event.

| Existing topic/type | Expected source | Maximum age | Available meaning |
|---|---|---:|---|
| `voice/state` / `VoiceState` | `voice` | 3 s | Thinking/speaking is an active Voice lease; resting/listening is not proof of company or input readiness. |
| `voice/heard` / `Speech` | `voice` | 1.5 s | Nonblank speech activity renews a presence lease. Text is discarded after the whitespace/empty check, never semantically classified or retained. |
| `humans/faces/tracked` / `Faces` | `eyes` | 2 s | Current company and one temporary face candidate; no recognition or familiarity. |
| `name/heard` / `NameHeard` | `name` | 1.5 s | A detector score of at least 0.90, with onset age included; no words or speaker location. |
| `controls/state` / `ControlsState` | `controls` | 300 ms | Enabled/ready Controls session evidence required to qualify a gesture. |
| `controls/gesture` / `ControlGesture` | `controls` | 500 ms since publication | Fresh released Stop/Pet metadata becomes ordinary/kindly touch; software mute is not mapped to sleep. |
| `ears/sudden` / `SuddenSound` | `ears` | 1 s | A qualified sudden sound, with onset age included, not a known direction or a wake request. |
| `air/state` / `AirState` | `air` | 15 s | Fresh CO2 metadata for the sustained air offset. |
| `schedule/due` / `ScheduleDue` | `schedule` | 3 s | A user-created due-item token, not reminder text, delivery acknowledgement or speech permission. |
| `humans/familiar` / `HumanFamiliar` | `eyes` | 5 s | The level of the remembered person recognised at the lamp, or none; no name (#1670, `lamp-eyes --visits on`). Held for its lease, never kept. |
| `humans/seems` / `HumanSeems` | `eyes` | 5 s | How the near face seems, as its kind only (`unsure` is none, the confidence stays the reader's), with `LAMP_MOOD_SEEMS=on`; without it Mood doesn't subscribe to it, and the adapter drops any before it is read (#1671, `lamp-eyes --seems` or `--seems-blendshapes`). Only the count of reads in a row of one kind is kept, never a read or whose face. With both of the eyes' readers on (the measure's set-up), only Blendshape V2's reads are taken: a FER+ read (`from: fer_plus`, or no `from`) within 10 s of Blendshape's last (`BLENDSHAPES_HOLD_US`) is dropped, so FER+ never breaks the product reader's run. FER+ alone is taken as before (Ten's 75c9 run on #2602, 22 Oct 02:05: each FER+ smile was followed by Blendshape's `unsure`, and Mood never smiled). |
| `mood/nudge` / `MoodNudge` | `addons` | 3 s | A kept add-on's nudge (#2530, [addons.md](addons.md)), written only by the add-ons' judge with `LAMP_ADDONS=on`. Energy moves both ways and warmth up only, each 0.1 at most, added before Mood's own clamps, so neither passes them. Held until its `until_us` (10 minutes at most), fading over its last minute; the newest replaces the last; none is taken asleep, and sleep ends one. |
| `voice/tone` / `VoiceTone` | `voice` | 5 s | How a line sounded against the person's own way (voice-tone.md), as what it counts as only (glad, low, lively or nothing), with `LAMP_MOOD_TONE=on`; without it Mood doesn't subscribe to it, and the adapter drops any before it is read (#1722). Numbers outside their ranges are refused. |

Tone and add-on nudges have independent publisher and event ordering. Both
can be taken at the same observation time, in either order. A line changes
the transient mood without replacing a nudge or extending its deadline;
their combined result stays inside Mood's energy and warmth clamps. Turning
tone off leaves nudges unchanged.

**On the second voice route** (`LAMP_VOICE_ROUTE=second`, the owner's rule
of 19 Oct that a voice feature works there too). The three voice inputs
work on either route. `voice/state` is made
from the coordinator's ring phase each tick, and `voice/heard` from a
transcript with no request, which the second route's recogniser gives
(`second_provider/worker.rs`). Both are said by the same coordinator code
whichever route answers (`coordinator/second.rs`: every path written for
`rig.provider` runs unchanged). The optional `voice/tone` input comes from
the completed line's tone reading with `LAMP_VOICE_TONE=on` and `LAMP_BUS=on`
(see [voice-tone.md](voice-tone.md)). The remaining inputs come from the
eyes, Controls, the ears, air, name, Schedule and the add-ons' judge, which
the voice's route doesn't touch. The
voice's own readers of Mood work the same way: the ring's breath, and the
hello's warmer sentence for someone who comes by often (a written turn,
which the second route answers too). Tests: the second route alone gives
what it heard to the heard path and passes through listening, waiting and
speaking (`coordinator/tests/second_route.rs`,
`the_second_route_alone_gives_mood_what_was_heard_and_the_voices_phases`).
Its ring phases equal Gemini's (`the_second_route_alone_answers_a_spoken_request_and_no_gemini_worker_starts`).
No input is Live-only.

Legacy headers use a wall-clock timestamp without future grace. Typed
`ClockStamp` values must match the host boot/clock and the current monotonic
time. A supplied expiry must be positive and cannot extend the input's bound.
Per-topic publisher/sequence checks reject duplicate and older observations;
each retired-publisher set holds at most 16 entries and refuses further
turnover on exhaustion. A new publisher must have a strictly later wall
timestamp than the last publisher, so a same-millisecond restart is ignored
until a later qualifying frame. A stamped input's sequence must increase
and its monotonic timestamp cannot decrease. Schedule's store/item/delivery
tuple is accepted at most once within the process, with at most 128 remembered tuples and refusal
on exhaustion. This is not a persistent exactly-once guarantee across restart.
Mood never acknowledges or retrieves a reminder's words. Stop's release must
also be less than 500 ms old. The existing head Pet recognizer waits 200 ms
for settling and 1,200 ms for its decision after release, so Pet allows a
release age under two seconds while still requiring publication under 500 ms.
The accepted event retains the original release time and expires at the
earlier of those two bounds. A Pet released before sleep cannot wake it merely
because recognition finishes afterward. Legacy wall-clock
corrections can temporarily make source packets future or out of order; they
do not reverse the engine's monotonic clock or create a wake event.

Each payload is at most 16 KiB. A Faces packet has at most 16 faces, unique
IDs of at most 64 bytes and validated finite geometry. The adapter retains
only the current candidate: keep it if still present, otherwise choose the
lexicographically smallest ID. A changed publisher, expiry or invalid update
removes the adapter's candidate. An invalid frame does not inject a new core
event; any already accepted face lease expires within its original two-second
bound. Idle polling also expires the adapter's candidate. Selection does not
imply identity or a known speaker.
Only short sudden sounds qualify; duration is at most 300 ms and `short`
must agree with a duration under 200 ms. Air metadata keeps its own sensor
validity; a missing CO2 value is not good air.

The runtime requires the exact version-1 topic declarations. The wire header
has no per-frame schema version, so declaration checking does not prove an
individual frame's version. Host qualification is still pending.

First resolved the producer boundaries in
[#713](https://github.com/autonomous-ai/lampOS/issues/713#issuecomment-6167222890):
there is no thanks producer now, so thanks/laughter remain engine/replay-only.
Future #711 Sleep/Wake intents must be separate from mute. Current Stop/Pet
observations are advisory to Mood and do not confirm that the whole lamp
slept, woke or changed its privacy state.
The core can exercise explicit semantic events in authored replay without
pretending a live producer exists. The `voice/heard` subscription uses only
metadata and drops the text immediately; Mood does not subscribe to
`voice/said`, classify a transcript, or add a guessed feedback topic. A
successful state update acknowledges neither a reminder nor an audible
response.

## Run and measure

The process has this explicit opt-in interface:

```sh
lamp-mood serve --enable --zone Asia/Ho_Chi_Minh --bus-root /run/lampos-bus
```

`--zone` is required when enabled and resolves an IANA timezone through Jiff's
bundled rules, or `system`: the lamp's own zone (`/etc/localtime`), followed
as it changes ([local-time](../crates/local-time/src/lib.rs), as Schedule and
Skills take it). `--run-for-ms N` bounds an observational run from 1 to 86,400,000 ms.
`--clock-id ID` is a test-only clock identity override. Duplicate and unknown
options are rejected. `LAMP_MOOD=on` in the environment is `--enable`; any
other value is not. Without either, startup exits before clock lookup,
timezone resolution, bus access, signal setup or threads.

The product unit, `deploy/product/lamp-mood.service` (#1670), runs
`lamp-mood serve --zone system --bus-root /run/lampos-bus` as `lampos`, after
setup, and is off unless a drop-in sets `Environment=LAMP_MOOD=on`. Off, it
prints its disabled result, exits 0 and stays inactive, with nothing on the
bus touched. The unit sets `LAMP_MOOD_SEEMS=on` (#2799), so once Mood is on
it takes how the near face seems from the product eyes' Blendshape V2 reader
(FER+ is never in the product). A test runs the binary with each value of the switch, and on
Linux with `--zone system`.

The runtime separates one state owner from one bus worker. Both use a 5 ms
loop; the worker drains at most four messages per topic per poll and at most
128 mapped events per batch. Its event queue holds 64; overflow is counted
and dropped. Output has one replaceable latest-state slot, not an accumulating
queue. The worker drops a state already more than 250 ms old before beginning
publication. A bus-root lock prevents a competing Mood writer.

Meaningful changes are a changed sleep latch or attention target, or a change
of at least 0.01 in a scalar since the last offered state. They may publish
every 20 ms; unchanged state has a one-second heartbeat. No-op, ignored or
duplicate inputs do not force a publication. Diagnostics and the final process
report contain fixed metadata/counters, not sensor payloads or words.

Filesystem access and internal bus syscalls do not have hard deadlines.
Isolation keeps a blocked worker off the state path, but publication can
still stall and expire. After attachment, shutdown waits at most 200 ms for
the worker and reports an error if it has not stopped. Initial clock/bus
attachment performs I/O before that worker boundary and has no hard timeout.
The process must exit before a
replacement starts. The lock remains held through that worker's bus and socket cleanup.

The fixed bounds above describe Mood's own state and queues. The inherited
bus receiver can skip an arbitrarily long invalid datagram stream inside one
receive call, and its publisher scans/sorts subscriber filenames before applying
the link cap. An invalid flood can stall the single bus worker; arbitrary bus
directory contents are outside Mood's memory bound. The reducer still advances
and stale output loses authority under its payload clock and TTL. No bus library
behavior was changed here.

The final `published` counter counts Mood bus calls returning success, not
confirmed consumer delivery. Diagnostic failures have their own counter and do
not turn a successful Mood publication into a failed one. The bus may suppress
repeated filesystem errors, so this counter alone does not prove retention or
delivery; the latency fixture observes an actual matching state at a subscriber.

The host targets are **under 1 ms p95** from pure-engine event handling entry
to returned state, and **50 ms p95** from accepted-event receipt to an observed
state at a subscriber for a cue causing a meaningful change as defined above.
The latter includes queue and delivery delay after acceptance; a name/touch
qualifies unless it leaves exactly the same state. This refines the initial
publish-return target: the bus delivers datagrams before retained-file writes
and before `publish()` returns. Subscriber receipt cannot bound that return or
prove retained-file durability. The measured proxy begins earlier, immediately
before the authored cue publisher call, and ends at a validated matching state
received by the subscriber.
Neither target includes sensor detection, acoustic feedback or actuator
response. Initial attachment is measured separately: bus subscriber discovery
can take 200 ms. First use, repeated events, bursts and slow dependencies need
separate measurement.

The publication benchmark must use an external monotonic timer and match
output sequence/session to the injected cue. A payload timestamp is not a
receipt or evidence that publication completed. No hard latency guarantee is
made for a stalled filesystem.

The [host evidence](verification/mood-host-20261014/host-checks.md) records
155 passing scoped Mood/Msgs tests and two separately executed manual timing
tests on the x86_64 Mac. The final 101-cue bus proxy measured 29.580 ms p95;
first process state took 308.050 ms. The three reducer cases each used 2,000
samples and had p95 below 0.4 microseconds. These are host observations with
the exact boundaries above, not lamp latency or a publication-return bound.
Full Mac workspace coverage on the earlier base passed 3,837 tests, zero
failures, 29 ignored; the Body-fix rebase's strict workspace Clippy/build and
422 Body/Mood/Msgs tests also passed. After the Eyes repair, strict Linux
workspace Clippy, 155 focused tests and the ARM64 cross-build pass. The final
Mac check of the changed upstream crate closure plus Mood/Msgs passed 1,764
tests, zero failures, 11 ignored. The host evidence records each base and
the further checks for upstream changes.
Readiness requires a green native ARM64 run on the final task head, recorded
in Fourth's log and pull request.

Focused qualification needs these boundary cases:

- Exact sleep entry, an observation at that boundary, old queued wake cues,
  and every non-allowlisted event while asleep.
- Warmth and energy saturation, repeated bursts, half-life decay, and an air
  stream that becomes stale just before or after ten minutes.
- Direct attention preempting a face or sound, equal-priority replacement,
  disappearance/expiry, and no resurrection of the previous target.
- Source restart, duplicate sequence, malformed/nonfinite payload, maximum
  retained identities, unknown local time, wall-clock steps and long gaps.
- Disabled startup without I/O, first attachment, full queues, a blocked
  publisher, expired output and bounded shutdown.

## Mood on a lamp

The rule for turning Mood on on a lamp, lamp-4ace first (the first builder on
#1722, 21:36; the rule as posted on #1670, 21:5x):

- **Builds:** lamp-voice and lamp-expression from a build with #1728 and
  #1749, so the ring and Expression read Mood's breath, and `lamp-mood` from
  the same build. lamp-4ace's Expression is pinned by hash, so it moves too.
- **The switch:** `LAMP_MOOD=on` for `lamp-mood`, the product unit's drop-in
  above, or on lamp-4ace (which has no `lamp-mood` unit of its own) the same
  as `orangepi`. Nothing else: the ring and Expression read it whenever it
  runs.
- **Familiarity stays off** (`lamp-eyes --visits`): new data about a
  person, the owner's decision. With it off, `familiar` is absent and nothing
  else changes.

**The lamp minute,** `lampOS/tools/mood-minute.sh`, run as root with the
person away from the arm:

```sh
sudo MOOD_BIN=/opt/lampOS/mood/current/lamp-mood EVAL=/path/to/lamp-voice-eval \
  lampOS/tools/mood-minute.sh [--clap] [--stop-at S]
```

It taps `expression/body` for 60 s with Mood off, then starts `lamp-mood` as
a transient unit (`lamp-mood-check`, `LAMP_MOOD=on`, as `RUN_AS`, orangepi
by default) and taps `expression/body` and `mood/state` for 60 s more. Then
it stops Mood, with nothing installed. The shell only runs the minute: the
numbers are `lamp-voice-eval mood-minute DIR`'s (`crates/voice-eval/src/mood_minute.rs`),
reading the taps it wrote, with Mood's phase from `MoodState::breath_at` and
the plain one from `plain_breath_phase`, the readers' own. Counts and numbers
only:

- **off:** the head's period (`period_s`, the plain 5.2 s) and its fit to
  the plain breath (`fit_to_plain_breath`, R from 0 to 1);
- **on:** `mood/state`'s checks (one session, sequences rising, the pace
  within 0.7 to 1.25), the head's period, and its fit to Mood's breath. That
  fit is the pitch against the sine and cosine of Mood's phase moved on to
  each sample (`MoodState::breath_at`), and `in_step` means R ≥ 0.8;
- `--clap`: "clap now" half way through, and whether Mood's pace went over
  1.1 in the 10 s after;
- `--stop-at S`: Mood stopped S s in, and the head's fit to the plain breath
  from 3 s after;
- each part's CPU, voice, Expression and Mood, from its cgroup's `cpu.stat`.

The ring's frames aren't on the bus (`light/state` is its setting only), so
the ring breathing with the head, and no flicker as Mood starts or stops, is
watched by eye. Tests: `mood_minute`'s own (5: a tap line read, in step and
not, the marks, an old state and too few samples), and
`lampOS/tools/mood-minute-test.sh` (20), with
stand-ins for `systemctl`, `systemd-run` and `lamp-bus`. A head that follows
Mood's quicker breath is in step, with off's period 5.2 s and on's 4.34 s. A
head that keeps the plain breath while Mood's is quicker is not. Mood is
stopped after. It's refused, with nothing started, when a part is down, Mood
already runs, a binary is missing or an option is wrong. Judging on's head
against the plain breath instead of Mood's fails both cases.

**What each reader says it took** (the first builder on #1722, 20 Oct
03:54): while it takes a fresh state from Mood, the ring says
`lamp-voice ring: the ring breathes with Mood: pace 0.81` and Expression
`lamp-expression: Mood's liveliness -0.31`. Each is said first on the first
fresh state, then on a change of 0.05 or more, at most once a minute
(`lamp_msgs::MoodTaken`; `ring_mood.rs` `taken_line`, Expression's
`runtime::liveliness_line`). A number only. With Mood off there's no fresh
state, so a lamp's journal is as before. With Mood gone and back, its first
state is said again once the minute since the last line has passed, even at
the same value (`MoodTaken::gone`).

**The trial, nothing moving:** `lampOS/tools/trials.sh mood`, for the step
that turns Mood on for lamp-4ace. It runs Expression in shadow: its own command line without `--enable`, in a drop-in
of its own, so it plans each move on `expression/state` and asks the body
for nothing. It's refused, with nothing changed, while a Mood already runs
(`lamp-mood.service` or a trial's left behind). Then, after the trial's mark so the readers' first lines are
seen, it starts `lamp-mood` as a transient unit (`lamp-trial-mood`,
`LAMP_MOOD=on`, as Expression's user). It passes on all of these within
120 s:

- `mood/state`'s breath at a pace other than 1, its `pace` and `phase` in
  either order (`lamp-bus latest` prints a message's keys sorted);
- the ring's line and Expression's line above;
- `expression/state` with `enabled` false, and a drift asked for
  (`drifting`). The drift is a body move: on a lamp with no body, whose
  `expression/state` says `fresh measured joints` are missing (lamp-75c9,
  its poses not yet measured), it's said "not checked", never passed, once
  the rest has been seen (the row's `UNCHECKED`, #1670).

It fails if `mood/state` says a `familiar` level: nobody known.
Afterwards Mood is stopped, the shadow drop-in removed and Expression
restarted on its own line, and `light/state`'s value (not the bus's header,
which changes with every send) must be as it was before. Mood's pace is
exactly 1 with nothing happening from 07:00 to 22:00, so by day the readers'
first lines say 1.00 and 0.00, and the trial asks for one clap near the lamp
40 s after Mood starts. A clap lifts the pace for about 25 s and the
liveliness for about 40 s, and only one counts in 120 s, so it still shows
when a change may next be said, a minute after the first. Mood takes
`ears/sudden` only from the ears, so a message published by hand doesn't move
it.

## Replays and the future lamp trial

The authored 24-hour fixture covers sleep through dawn/company/noise,
name and touch wakes, warmth rising and decaying, quiet dozing, a reminder
wake, repeated bangs, sustained/cleared/stale air and target competition.
Publisher restart and clock corrections are separate engine/adapter tests;
they are not claimed as events in this day curve. Its event provenance must explicitly say
**synthetic**. Accelerating its clock does not make it a recorded day or a
real-time latency test.

The retained [trace and curve](verification/mood-host-20261014/README.md)
contain 1,441 minute samples and 29,364 authored inputs. Rebuilding and
repeating the generator reproduced the trace byte for byte. The SVG labels
its data synthetic and uses the same zero-to-one axes for all three scalars.

First accepted the authored 24-hour replay for handoff and reports a real
sanitized-day recording beginning at 18:23 ICT on 2026-10-14, to be offered
after the following day. No completed recording has been supplied for this
task. A future real-day replay needs observed event timestamps and source
provenance, without words, images or a face history. Do not label a scenario, a generated
trace or a repeated synthetic fixture as recorded lamp behavior.

First can start with a one-minute metadata-only trial of the provisioned
binary, leaving all physical consumers unchanged:

```sh
lamp-mood serve --enable --zone Asia/Ho_Chi_Minh --bus-root /run/lampos-bus --run-for-ms 60000
```

In another shell, observe the existing topic without retaining input words
or raw face history:

```sh
lamp-bus --root /run/lampos-bus tap mood/state --seconds 60
```

Expect the current host clock identity, one publisher session, advancing
sequences, bounded scalars and familiarity `unknown`; roughly one update per
second plus meaningful changes. Sleep has awake zero and no attention.
The process prints its fixed counters on normal exit; the retained state
loses authority after its 2.5-second TTL. Running `lamp-mood serve` without
`--enable` prints its disabled result and touches no bus files. Stop the
optional producer to roll it back; there are no deployment or consumer changes.

This command alone does not record the day. The qualified capture procedure
must specify the accepted metadata schema, privacy filtering, receipt times
and a bounded destination before calling its output recorded-day evidence.
Existing `lamp-bus record` stores message send timestamps, not receipt times,
so it cannot establish accepted-event-to-publication latency by itself. Do
not record audio, voice words or raw face topic history to fill this gap.

For a longer trial, increase `--run-for-ms` up to `86400000`. Check the three explicit wake causes
after sleep, repeat incidental noise without waking, observe night/day and
long-quiet settling, interrupt the air stream and restart optional publishers.
Measure accepted-event to observed `mood/state` separately from physical
touch/name detection, using the same clock and exact boundaries as the host
measurement. No output consumer is added just to make the trial visible.

Most likely wrong on the lamp: a busy office's repeated face/Voice leases may
hold awake too steadily, the 15/60-minute quiet thresholds may feel early or
late, and a 0.05 warmth nudge with 20-minute decay may look flat once an eye
consumer exists. Check current Controls heartbeat and delayed-Pet admission
first if touch fails; the metadata path does not include recognizer latency.
These are authored tuning hypotheses, not faults established by a lamp trial.

## Not shown

No real recorded day, device execution, deployment, sensor-to-state latency,
physical touch-to-feedback or acoustic timing is established. Host evidence
and code inspection do not establish that these constants feel natural to a
person beside the lamp. Thanks/laughter, a confirmed Controls sleep outcome
and familiarity remain unavailable. The only expression driven by Mood is the
lamp's one breath, the ring's and the head's at rest, tested on the host and
not yet seen on a lamp; the product unit has not run on one either. The state
does not prove that the lamp is listening, can accept input, has spoken,
has recognized a person, or has changed a physical output.
