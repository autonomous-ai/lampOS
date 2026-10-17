# The message types

`lamp-msgs` (`crates/msgs`) is what the first topics of [the bus](bus.md)
carry: one Rust structure for each, in one crate, so that a publisher and its
subscribers share a definition and not two copies of one. It is a library
and nothing else: no process, no socket, no clock.

## The rule

- One type per topic. Its publisher declares it, a subscriber names the one
  it expects, and the bus refuses a second publisher or a subscriber that
  names another ([bus.md](bus.md), "Declaring a topic"). The bus compares
  names. It does not look inside a payload.
- The types are here and nowhere else. A crate that publishes or subscribes
  depends on this one and does not write its own copy of a message.
  `Log` and `LogLevel` are the deliberate transport-metadata exception:
  their single canonical definition is in `lamp_bus::logging` and re-exported
  here, so `lamp-bus logs` uses the exact type without a `msgs -> bus -> msgs`
  dependency cycle.
- JSON. Each type is a small flat structure sent as one JSON object, with
  ROS's field names and units where ROS has the type: SI units, angles in
  radians, times in seconds.
- A field this version does not know is ignored, so that a newer publisher
  does not break an older subscriber. ROS refuses any difference in a
  definition; one team on one machine can do with less.
- A field that may be absent is an `Option`, left out when there is nothing
  to say. Every other field has to be there.
- A value of an enumerated field that this version does not know is an error
  for that message: a state is not guessed.
- Reading the JSON checks a message's shape. Its sense, a light's colour and
  level or the lengths of a joint state's lists, is checked by the type's
  `check()`, which the reader asks for.

Each type has `TYPE`, the name the bus compares (`lamp_msgs/JointState`),
`VERSION`, `TOPIC` or `TOPICS`, and `declaration()`, which its publisher
hands to `Bus::publisher_declared`. `first_topics()` keeps the original
seventeen topics; `expression_topics()` lists the optional Expression
contracts separately, and `controls_topics()` lists the optional Controls
contracts, `keeper_topics()` the keeper's pause, `memory_topics()` the
memory's typed rules, and `name_topics()` the first sign of a hearing of the
lamp's name (`name/rising`). Touch, Schedule, Ears and privacy have separate
additive catalogues. `logging_topics()` adds the optional operational log
stream. `declaration_of()` recognizes every catalogue without changing
`first_topics()`.

## The types

| Type | Topic | Kind | Period | What it says |
|---|---|---|---|---|
| `Audio` | `mic1/audio`, `mic2/audio` | event | 20 ms | A block's header: sample rate, channels, sample format, the block's count. The samples follow it in the payload as they are (`Audio::block`), under the encoding `json+pcm` |
| `ImageRef` | `camera/image` | latest | 500 ms | The absolute path of the newest frame's file, its width, height and encoding, and when it was captured |
| `JointState` | `body/joint_states` | latest | 100 ms | ROS's `sensor_msgs/JointState`: `name`, `position` (rad), `velocity` (rad/s), and `effort`, which here is the share of its full strength each servo reports (0 to 1) and not ROS's newton metres. And `commanded` (rad), which ROS's has not |
| `Faces` | `humans/faces/tracked` | latest | none | A list of faces: a passing id, a box in fractions of the picture as in ROS4HRI, confidence, yaw if it was estimated, whether it faces the lamp if that was judged, and with `LAMP_EYES_SPEAKING=on` whether its mouth is speaking (`speaking`: `state` yes/no/unknown and a `score`), absent otherwise |
| `Speech` | `voice/heard`, `voice/said` | event | none | Text, whether it is finished, and the locale and the request's number if known. After `hri_msgs/LiveSpeech` |
| `VoiceState` | `voice/state` | latest | 1 s | Resting, listening, thinking or speaking |
| `VoiceLink` | `voice/link` | latest | 1 s | Ready, or unavailable with one of seven kinds of reason |
| `Online` | `lamp/online` | latest | 1 s | `online` true, false or absent (nobody knows), `from` the part whose link said so, `down_for_s` while offline |
| `RoomState` | `room/state` | latest | none | Unknown, quiet or busy |
| `Light` | `light/command`, `light/state`, `light/test`, `light/drawn` | latest | 1 s | On or off, one of ten colours by name, a level from 1 to 5 |
| `Diagnostic` | `diagnostics` | event | 1 s | ROS's `DiagnosticStatus`: ok, warn, error or stale, a name, a message, a few keys and values |
| `Health` | `lamp/health` | latest | 5 s, means nothing after 15 s | How the whole lamp is: the worst level, one sentence, every part that has reported with its level, message and age, worst first, and the parts that are expected and never reported |
| `BoardState` | `board/state` | latest | 5 s, means nothing after 15 s | The computer's numbers: each temperature by sensor (°C), load per processor, memory in use and disk free as fractions, uptime, and the Wi-Fi signal and whether the clock is set if they could be asked |
| `NameHeard` | `name/heard` | event | none | The lamp's name was heard in what its microphone hears: how sure the listener was, from 0 to 1, and how many milliseconds before this was sent the name began. A sign, never proof ([name.md](name.md)) |
| `NameHeard` | `name/rising` | event | none | The same hearing's first sign, as soon as the score reaches the listener's bar and while the name is still being said: the score so far, and how long ago it reached the bar. Its final word, with the highest score, is on `name/heard`; a first sign with none was a near miss (#782, [name.md](name.md)) |
| `Pause` | `lamp/pause` | latest | none | Whether the lamp is paused, as `lamp-keeper pause` or `resume` last said; kept until the next, read with `latest` by each part that honours it ([keeper.md](keeper.md), "Named states and the pause") |
| `Hand` | `body/hand` | latest | 1 s | Whether a hand is pushing the lamp now, as the body feels it (`pushed`), and when that push began on the body's own clock (`since_us`, 0 when not pushed); said with every step lamp-body passes on the bus, every 80 ms ([body.md](body.md), "A hand on the bus") |
| `BodyGesture` | `body/gesture` | event | none | What became of one of the body's gestures: its `name` (the clip's, the body's own `idle` and shifts included) and `state`, `began`, `over` (played out, let go, replaced, or the run stopped) or `dropped` (asked and dropped before it began, the anti-tip rule's refusal and #1239's `busy` and `held by a hand` included, or ended for a higher ask, so no `over` follows). Said by lamp-body with `LAMP_BUS=on` where its journal says the same ([body.md](body.md), "On the bus"). Read-only. `body_topics()` lists it |
| `VoiceMute` | `voice/mute` | latest | 3 s | Whether the voice's software mute is on (`muted`), and why: `reason: "sleep"` when a hand held on the head put the lamp to sleep, absent for the mute as it always was (#711). Said by the voice with `LAMP_HEAD_TOUCH=on` and `--controls-mute`, when it changes and every second; Expression's `--head-touch` sleeps from it. `mute_topics()` lists it. Not the physical slide |
| `LightRequest` | `light/request` | event | none | A light asked for that lapses (#940, [skills.md](skills.md)): a `claim` (`source`, a part or `skill:<name>`; `priority`, one of `idle`, `skill`, `conversation`, `safety`, `person`, in the order they give way; the asker's `clock_id` and `until_us`, at most 30 s ahead, renewed by asking again) and the `light`. The ring shows the highest-priority request that holds, the newest claim of equal priority (a renewal keeps its place), and the person's lasting `light/command` once none does, with `LAMP_LIGHT_REQUESTS=on` ([ring-choreography.md](ring-choreography.md), "The lamp's own light"). `arbitration_topics()` lists it |
| `LightReached` | `light/reached` | event | none | A light asked for on `light/request` that the ring has shown on resting frames for 350 ms, said then and again after each renewal while it is shown: its `source` (the request's `claim.source`) and the `light` (#1146). Nothing is said inside a turn, whose face never shows the lamp's light, nor for a light never shown that long ([ring-choreography.md](ring-choreography.md), "The lamp's own light"). `arbitration_topics()` lists it |
| `Rules` | `memory/rules` | latest | none | What the people at the lamp asked it about speaking first, as typed facts for the policy (#813, [memory.md](memory.md#what-a-person-teaches-and-corrects-813)): `person` (the memory's id `p-<hex>` of the known person, or `everyone`), `quiet` (asked never to speak first, unprompted), `quiet_mornings` (asked not to speak first in the mornings, before noon local time, or never), `rules`, how many rules of every kind the room's and theirs hold, and `hello` / `news` (`Unprompted`: `never`, `not_before_minute`, `not_from_minute`, local minute of the day, `not_while_talking`; absent in an older message, nothing held), the greeting's and the unasked news's limits, every typed rule merged, the strictest winning. Said by the voice with `LAMP_MEMORY_RULES=on` at each session open and each rule kept or forgotten; kept until the next. Never a rule's words |
| `Decision` | `policy/decisions` | event | none | What the policy decided ([policy.md](policy.md)): what it was about, the choice, its probability, the decision model's answer to each question by the question's name, who decided, how long the asking took, whether anything acts on it, the request's number if known, for a `progress` decision the work it was about (`task`, `<store>:<task>:<change>@<run>`), and for a `hello` decision the arrival (`arrival`), both added 2026-10-15 as optional fields; for an answer to a skill's ask, the ask (`ask`, 2026-10-16, optional). Not what was heard |

The kind and the period are the table of first topics in [bus.md](bus.md)
read as declarations. A stream is `event` and a latched topic `latest`. "How
often" is the period, and where that table gives two rates the slower is
declared, because a period is a promise. "With each look", "with each thing
said" and "when it changes" promise none. No lifespan is declared: the table
has none. A topic that promises no period and can be silent for longer than
the bus's lease needs its publisher to call `alive()`.

`policy/decisions`, `lamp/health`, `board/state` and `name/heard` were
added after that table was written and are not in it: this table is where
they are listed. `lamp/pause` is not one of the first topics either:
`keeper_topics()` lists it, and `declaration_of()` recognizes it. Nor is
`memory/rules`: `memory_topics()` lists it; nor `name/rising`, which
`name_topics()` lists.
`first_topics()` has them with the others. The last two are the only
topics with a lifespan: a kept report of how things are must not outlive
whoever wrote it.

`Level::rank()` orders the four levels for a reader: ok, stale, warn,
error. A part never says of itself that it is stale; whoever writes
`lamp/health` says it of a part not heard from.

## Optional operational logs

`Log` and `LogLevel` are re-exported from their single definition in
`lamp_bus::logging`. `logging_topics()` registers `logs` as
`lamp_msgs/Log`, version 1, event, with no period or lifespan. The existing
seventeen-topic `first_topics()` catalogue is unchanged.

`Log` carries `part`, `level`, `code`, `message`, `attempt` and
`created_unix_ms`. The part follows the bus source-name rule; code is
`[a-z][a-z0-9_.-]{0,63}`; message is at most 512 UTF-8 bytes. Levels are
`debug`, `info`, `warn`, `error`, `fatal`, in increasing order. These are
operational severities, not the `Level` used by `Diagnostic`.
`attempt` is a positive admission-attempt count belonging to that logger
instance, including attempts rejected before publication. `created_unix_ms`
is admission wall time, not the bus's publication wall time. Unknown fields
are ignored; required fields and known enum values remain mandatory.

`Log::check()` checks the payload. `Log::from_message()` additionally requires
the exact topic/type, event kind, JSON encoding, payload at most 4 KiB, and a
part matching the header source. The logger is opt-in and the message contains
no automatic runtime instrumentation. See [logging](logging.md) for use,
bounded admission, terminal rendering, privacy and persistence limitations.

## Optional expression contracts

Expression's additive types live in `src/expression.rs` and are re-exported
at the crate root. Their new topics do not change any existing type:

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `VoiceTiming` | `voice/timing` | latest | 100 / 500 ms | Explicit voice session/turn/readiness and scheduled/terminal edges |
| `PersonActivity` | `voice/activity` | latest | 100 / 300 ms | Actual admitted human speech or pause, with session and turn |
| `VoicePlayout` | `voice/playout` | event | 20 / 200 ms | Committed speaker interval and its level, with cancellation identity |
| `VoiceProgress` | `voice/progress` | latest | 100 / 500 ms | Optional complete timing heartbeat with a positive audio generation and explicit resumable hold |
| `VoiceSound` | `voice/sound` | event | 20 / 200 ms | Optional committed playout interval belonging to one matching audio generation |
| `Concentration` | `attention/concentration` | latest | none | Explicit concentration, initially false until supplied; never inferred from a quiet room |
| `ExpressionBody` | `expression/body` | latest | 20 / 200 ms | Relative radian offsets, hold or a normalized calibrated-owner gaze request |
| `ExpressionLight` | `expression/light` | latest | 20 / 200 ms | Face and brightness multiplier preserving the lasting `Light` setting |
| `ExpressionState` | `expression/state` | latest | 200 / 500 ms | Planned behaviors, shadow/enabled status, missing facts and warnings |

`ClockStamp` identifies the host boot and shared `CLOCK_MONOTONIC` time in
microseconds. It is distinct from the bus header's private `at_us`.
`ExpressionLease` identifies both the planner and Voice sessions, request,
turn, increasing sequence and an apply/expiry interval of at most 200 ms.
`valid_at()` checks the stateless clock/expiry part of the consumer gate;
the actuator owner must also fence prior sessions/turns/sequences and enforce
privacy and safety. The optional `resumable_voice` catalogue setting chooses
the complete `VoiceProgress`/`VoiceSound` pair instead of
`VoiceTiming`/`VoicePlayout`; mixing the two cannot refresh a held turn or
restore its old sound. `PersonActivity` remains shared. The wrappers flatten
the existing timing/playout fields into their JSON object, adding required
`audio_generation` and, for progress, `held`. A hold advances the generation
after the owner discards old sound and unpublished observations; resumption
keeps that generation. Terminal retirement and freshness still take priority.
New generation messages require First's Voice publisher integration; this
task changes no hardware owner. See
[Expression](expression.md) for exact integration and evidence limits.

## Optional controls contracts

Controls' additive types live in `src/controls.rs` and are re-exported at the
crate root. They do not change the original topic catalogue:

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `ControlGesture` | `controls/gesture` | event | none / 500 ms | Original release time, duration, source and gesture, with an explicit Stop, SoftwareMute or Pet intent and shadow/enabled status |
| `ControlsState` | `controls/state` | latest | 100 / 300 ms | Input freshness/fault, button holds, head/base status and independently retained latest Stop and SoftwareMute requests |

Both use `ClockStamp` for shared host monotonic time and boot identity, plus a
Controls process `session_id`. A gesture's sequence identifies the same request
on both topics; renewing state preserves that request's original stamp instead
of refreshing it. The retained fields cannot hide a Stop behind later petting.
`ready` reports fresh input observations, not Voice readiness or applied mute.

`ControlGesture::fresh_at()` checks shape, enabled status, clock and the 500 ms
publication-age limit; Stop also expires 500 ms after its original release.
The owner must additionally fence publisher/session changes, deduplicate
requests, and apply its own privacy and safety rules. `SoftwareMute` requests
a lasting software inhibit; neither Stop nor any gesture permits reopening.
An owner that accepts it retains the inhibit independently of message expiry
or Controls restart. Physical capture permission stays with Voice. These are typed
handoff contracts; see [Controls](controls.md) for the consumer obligations and
[service privacy](service-privacy.md) for the existing physical authority.

### Selected physical privacy observation (#755)

With `LAMP_PRIVACY_CONTROLS=on`, `ControlsState.privacy` contains a
`PrivacySwitch`: its original acquisition `ClockStamp`, GPIO reader session,
optional `muted` level and optional fault. A known level has no fault; an
unknown level has a fault. Its independent freshness limit is 50 ms, even
though the containing state has a 300 ms transport lifespan. Publication of
another heartbeat does not change the acquisition stamp. Hand-input `ready`
and `enabled` do not grant or suppress this physical observation. Selected
Controls normally publishes every 20 ms and immediately on a level or fault
transition; the existing 100 ms topic declaration is unchanged.

Absent/off omits the field from serialized state. Older messages without it
still deserialize; a selected Voice treats absence as unknown and denies
capture. [Privacy switch](privacy-switch.md) describes wiring, authority and
the coordinated opt-in across Controls, Voice and Ears.

## Optional microphone permission contract (#755)

`privacy_topics()` declares one new topic, separate from the original
seventeen. Only Voice's selected privacy worker publishes it.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `MicrophonePrivacy` | `voice/privacy` | latest | 10 / 50 ms | Voice controller identity, microphone generation, original physical acquisition, expiry, capture permission and known/unknown physical mute |

Version 1 of `lamp_msgs/MicrophonePrivacy` carries a publication `ClockStamp`,
16-character hexadecimal Bus `session_id`, 32-character hexadecimal
`controller_boot`, positive `microphone_generation`, `observed_us`,
`expires_at_us`, `permitted`, and optional `physical_muted`. Unknown physical
state uses `observed_us = 0` and cannot permit capture. A known sample's expiry
cannot exceed acquisition plus 50 ms. Permission also expires with the
controller snapshot; a fresh publication cannot extend either source.

`check()` validates shape and timestamp relationships. `from_message()` also
checks the exact topic/type/kind/encoding/source, Bus boot and sequence,
publication stamp, wall-clock envelope age, monotonic clock and current expiry.
Consumers additionally fence ordering, loss, retired sessions and generation
changes. Ears discards buffered PCM and destroys its handle before crossing a
capture era. The muted ring requires denied microphone and camera authority
plus fresh physical LOW; a software mute alone is not a physical indication.
These are software permission contracts, not evidence of electrical power
removal or measured handle-close/visible-light latency.

### Opt-in touch contracts (Task 33)

`touch_topics()` declares `controls/touch` (`TouchEvent`, event, 500 ms
transport lifespan) and `controls/touch_state` (`TouchState`, latest,
100 ms declared period/300 ms lifespan). `declaration_of` recognizes both;
`first_topics()` and the existing `controls_topics()` inventories are unchanged.
Only the explicit `lamp-controls --touch` path publishes them, normally every
20 ms and on events. All fields are strict; old controls shapes are unchanged.

Source/session/contact identity, original acquisition and contact times,
raw working-source masks, filtered event geometry, normalized strip position/travel/
speed and conditional speaking/quiet/asleep intents are carried separately.
Sleep and Wake are explicit alternatives, never a toggle. The owner binds
lifecycle state at contact begin and checks original/current source age under
100 ms before application. A 500 ms bus envelope does not renew that limit.
Primary Stop/SoftwareMute and reset-only FactoryReset do not depend on being
awake. No payload grants capture or confirms physical output.

For the strip, state masks contain only the configured working electrodes;
raw acquisitions/recordings retain all twelve hardware bits. Sparse ordered
sets preserve the full twelve-slot coordinates while skipping known excluded
pads for contact topology. An excluded-only sample never prolongs a hold.

The receiver's restart/fault/contact and per-source consumed-sequence fences,
the legacy primary sidecar's single-consumer requirement, provenance and
on-lamp qualification are in [Controls](controls.md#touch-and-slides-task-33).
Physical application, privacy closure, noise margins and response latency have
not been shown on a lamp by this builder.

## Optional Schedule contract

Schedule's additive types live in `crates/msgs/src/schedule.rs` and are
re-exported at the crate root. `schedule_topics()` and `declaration_of()`
discover the topic; `first_topics()` remains the original seventeen topics.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `ScheduleDue` | `schedule/due` | latest | 1 / 3 s | Counts and, only while enabled and ready, the oldest pending occurrence's identity and timing; no reminder text |

`ScheduleDue` is version 1 of `lamp_msgs/ScheduleDue`. It carries a
`ClockStamp`, a persistent `store_id` of exactly 32 lowercase hexadecimal
characters, a `revision`, `enabled`, `ready`, `pending`,
`waiting_for_clock`, and optional `oldest`. The two counts are each from 0
through 128 and together cannot exceed 128. `oldest` is present exactly when
`pending > 0`, `enabled` and `ready` are all true. An unavailable or shadow
publisher may keep its counts visible but cannot advertise a notice.
`ready` describes Schedule's ability to serve due work, not Voice readiness.

A `ScheduleNotice` contains positive `id` and `delivery` numbers,
`due_unix_ms` from 0 through 253402300799999 (the last millisecond of
9999-12-31 UTC), a `late` flag, and optional `late_by_ms`. The latter is an
unsigned millisecond count when lateness is currently known; its absence
does not mean zero. No text or instructions are carried on this topic.
Unknown additive fields are ignored; required fields, numeric types and
the checks above are enforced. `check()` validates structure, including
the existing `ClockStamp` identifier contract. It does not read a clock,
establish current boot/freshness, or deduplicate deliveries.

Schedule owns the persistent queue. It advertises the same delivery token
until an explicit durable socket acknowledgement, cancellation or snooze
changes that occurrence; another heartbeat does not mean another delivery.
The bus's one latest file is neither the durable queue nor permission to
speak. A consumer must validate current host boot, stamp, publisher identity
and ordering, refuse an expired or superseded offer, then retrieve the exact
`store_id`/`id`/`delivery` from Schedule's private socket before speech.
Only confirmed completion may be acknowledged. Voice still owns admission,
privacy, interruption and the final speaker check; First wires that path.
See [the bus contract](bus.md#optional-schedule-topic-third-2026-10-14).

## Adding one

1. Write the structure in `crates/msgs/src/lib.rs` with `Serialize` and
   `Deserialize`: flat, no generics, each field's unit in its comment, and
   without `deny_unknown_fields`.
2. Give it `TYPE` (`lamp_msgs/` and its name), `VERSION` 1, `TOPIC` and
   `declaration()`, and add it to the appropriate topic catalogue and
   `declaration_of()` lookup.
3. Test that it is written as the JSON intended, reads back, ignores an
   unknown field and refuses a missing one. Add its row to the test of
   declarations, to the table above and to the first topics in bus.md.

Changing one: an optional field can be added with nothing else. A new
required field, a new enumerated value, a changed unit or meaning is a new
`VERSION`, which the bus shows and does not check. Under a new name it is
another type, and a topic's declaration file has to be removed before the
topic can be published as that.

## Not shown

The original type tests run on a development Mac (x86_64): 15 of the types
and one that declares the seventeen topics on a real bus and sends six of
the types through it. The example in the crate's documentation is
compiled, not run. On lamp-4ace five of the types travel: the voice
publishes `Speech`, `VoiceState` and `RoomState`
(`crates/voice/src/bus_out.rs`), the eyes `Faces`
(`crates/eyes/src/bus_out.rs`), and the policy reads all four and
publishes `Decision` ([policy.md](policy.md)). The body publishes
`JointState` and its own `Diagnostic` (`crates/body/src/bus_out.rs`). The
actions program asks for a `Light` and the voice's ring worker says the
one it shows (`crates/voice/src/ring_light.rs`). The health program
publishes `Health`, `BoardState` and the board's own `Diagnostic`
([health.md](health.md)). Nothing publishes the others yet: `Audio`,
`ImageRef` and `VoiceLink`.
`VoiceLink`'s seven reasons group the voice program's
fault categories and have not been agreed with its owner. Which way a face's
yaw is positive is written down and no detector has been held to it.

Eight additional Expression contract tests passed on Third's x86_64 Mac:
declarations and compatibility, required identities, clock/schedule bounds,
terminal states, finite offsets, normalized gaze, light settings and lease
expiry. Those initial host tests did not qualify hardware; First's later
legacy-contract integration is recorded in [Expression](expression.md).
The added resumable progress/sound contracts have no lamp qualification.
Their six new contract tests passed on the same development Mac, together
with all 44 message tests including the doctest and the 2,353-test dependent
gate. The source-copy run also passed all 44. Commands and skipped inherited
cases are in the [task 18 evidence](verification/expression-rest-host-20261014/results.json).

The Controls rows describe the source contract, not a measured deployment or a
test result. No Controls publisher/consumer integration, GPIO/I2C behavior or
physical stop/mute latency has been established by this documentation review.

## The tasks contract

Added by the second builder on 2026-10-15 (#732). The type lives in
`src/tasks.rs` and is re-exported at the crate root; `tasks_topics()` and
`declaration_of()` find its topic, and `first_topics()` is unchanged.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `TaskStatus` | `tasks/status` | latest | none / none | How each piece of work handed over is going ([tasks.md](tasks.md#progress-on-the-bus-serve---progress-732)): the store, the run of the tasks part that counted the changes (counts begin again at each run), and for each task its engine, `AgentState`, seconds at it, when last active, when read, how many times its progress line changed, whether it changed at this read, the share of it that is new, and the `Sign`s new in it and seen in it; and the tasks watched whose progress has not been read yet (`unread`, added 2026-10-15, left out when none); and the tasks let go because their agent settled, for two minutes after, with that state (`settled`, `TaskSettled`, added 2026-10-16 for Task news, #1021, left out when none). Never the agent's words |

Published by `lamp-tasks serve --progress` only: at start, after each
read, when a task joins or leaves the watch, and when one joins or leaves
`settled`, which at the end of its two minutes is published with nothing
else happening; its
publisher says once a second that it is alive. `check()` holds the store to
32 lowercase hexadecimal characters, the run to a positive number, at most
16 tasks listed once each in
order of their numbers, a positive task number, an engine of 1 to 32
lowercase letters, digits or underscores, times within years 1970 to 9999,
`new_share` from 0 to 1, signs listed once each in order with every new
sign also seen, and no `changed` without a change counted. `AgentState`
and `Sign` are enumerations: a value this version does not know refuses
the message. A state Harness adds later is written `unknown` by its
publisher, and a new sign is a new `VERSION`.

## The arrival contract

Added by the second builder on 2026-10-15 (#752), in `src/arrival.rs`;
`arrival_topics()` and `declaration_of()` find its topic, and
`first_topics()` is unchanged.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `VoiceArrival` | `voice/arrival` | event | none / none | A hello is due and the voice holds it for the policy ([speaking-first.md](speaking-first.md#asking-the-policy-752)): the arrival's number, minutes away, seconds there, seconds of quiet, minutes since the last hello if any. No picture, no face, no identity |

Published by the voice only with `LAMP_HELLO_ASKS_POLICY=on`. `check()`
asks only for a positive number. `Decision` has the optional `arrival` the
policy's `hello` decision names it by; it is left out of every other
decision, so their bytes are as they were (a test).

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `SkillAsk` | `skills/ask` | event | none / none | A skill asks the policy whether the lamp may do something it would start itself (#939, [policy.md](policy.md)): the ask's number (the asker's monotonic clock in µs, positive), the skill's name, the act (`hello`, `news`, `reminder` (added 2026-10-16), `unprompted`), the person if known, and for a hello its minutes away and seconds there. No words, no picture, no face |

`check()` asks for a positive number, a skill's name (1 to 64 lowercase
letters, digits, hyphens), a known act, a person as `p-` and 16 hex digits
if given, and a hello's minutes and seconds. `Decision` has the optional
`ask` the policy's answer names it by, left out of every other decision.
Added by the second builder on 2026-10-16 (#939), in `src/skill_ask.rs`;
`skill_ask_topics()` and `declaration_of()` find its topic.

## The ears' contracts

The ears' types live in `src/ears.rs` and are re-exported at the crate root
([ears.md](ears.md)). Like the controls', they do not change the original
topic catalogue. Neither carries any sound.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `RoomSound` | `ears/level` | latest | 100 ms / none | The room microphone's last 100 ms in dBFS, the room's quiet (none for the first two seconds), whether the lamp's own voice is in it |
| `SuddenSound` | `ears/sudden` | event | none / none | A sound that rose suddenly out of the quiet: its loudest 20 ms, the quiet, how long it stayed loud in 300 ms, whether that was short, how long ago it began |

Levels are dBFS from -120 (digital silence) to 0; `check()` refuses
anything else, and a peak under its quiet.

## Whether it was said to the lamp

`Addressed` lives in `src/attention.rs` and is re-exported at the crate
root ([attention.md](attention.md), "Said to the lamp from close to it").
The voice publishes it with `LAMP_ADDRESSED=on`. It carries no sound, no
words and no picture.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `Addressed` | `attention/addressed` | event | none / none | One request: its session and number, when its sound began and ended (the lamp's monotonic clock), the room microphone's highest rise over the quiet and whether that is near, how many looks of the eyes, the tallest face turned to the lamp, the share of looks with a close face turned to it and whether that is facing, the name's best score, and whether it was said to the lamp from close to it |

`check()` refuses an end before the beginning, a rise outside ±120 dB, a
face height, share or score outside 0 to 1, a face height without looks
or looks without one, a share of fewer than two looks, `near` or
`facing_close` said with no reading under it, and an `addressed` that is
not exactly near and facing. A sign the ears or
the eyes said nothing fresh about is `null`.

The Schedule contract tests establish message shape and declaration rules
only. They do not establish durable delivery, Voice integration, clock
accuracy, acoustic completion or reminder latency on a lamp.

The touch contract tests likewise establish shape, sampled recognition and
receiver fences. They do not establish real electrode/head calibration,
physical contact latency, capture closure or applied Sleep/Wake/Stop on a lamp.

## The skills contract

Added by the seventh builder on 2026-10-16 (#938). The type lives in
`src/skills.rs` and is re-exported at the crate root; `skills_topics()` and
`declaration_of()` find its topic, and `first_topics()` is unchanged.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `SkillEvent` | `skills/events` | event | none / none | One stage of a run of a physical skill ([skills.md](skills.md)): the skill's name, the `SkillStage` (`triggered`, `asked`, `answered`, `done`), why in words, and, with `answered` only, whether the Policy `allowed` it |

Published by `lamp-skills` only. `check()` holds the name to 1 to 64
lowercase letters, digits and hyphens, `why` to 2,000 bytes, and `allowed`
to `answered`. `SkillStage` is an enumeration: a stage this version does
not know refuses the message.
