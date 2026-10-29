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
- Keep existing wire field names and meanings, including `Option` fields.
  Renaming or repurposing one is not an additive change: a missing `Option`
  can decode as `None` and silently lose its old meaning. Add independently
  ignorable metadata without replacing existing fields.
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
lamp's name (`name/rising`), and `presence_topics()` whether anyone is at
the lamp (`room/presence`). Touch, Schedule, Ears and privacy have separate
additive catalogues. `logging_topics()` adds the optional operational log
stream, and `output_caps_topics()` the Actions-owned output restrictions.
`declaration_of()` recognizes every catalogue without changing `first_topics()`.

## The types

| Type | Topic | Kind | Period | What it says |
|---|---|---|---|---|
| `Audio` | `mic1/audio`, `mic2/audio` | event | 20 ms | A block's header: sample rate, channels, sample format, the block's count. The samples follow it in the payload as they are (`Audio::block`), under the encoding `json+pcm` |
| `ImageRef` | `camera/image` | latest | 500 ms | The absolute path of the newest frame's file, its width, height and encoding, and when it was captured |
| `JointState` | `body/joint_states` | latest | 100 ms | ROS's `sensor_msgs/JointState`: `name`, `position` (rad), `velocity` (rad/s), and `effort`, which here is the share of its full strength each servo reports (0 to 1) and not ROS's newton metres. And `commanded` (rad), which ROS's has not |
| `Faces` | `humans/faces/tracked` | latest | none | A list of faces: a passing id, a box in fractions of the picture as in ROS4HRI, confidence, yaw if it was estimated, whether it faces the lamp if that was judged, and with `LAMP_EYES_SPEAKING=on` whether its mouth is speaking (`speaking`: `state` yes/no/unknown, a `score`, and `from`, `corners` or `mesh` with `--mouth-model`, whose scale the score is on; `from` absent from eyes before #1470), absent otherwise |
| `HumanFamiliar` | `humans/familiar` | latest | 2 s, good for 5 s | How familiar the remembered person at the lamp is: `level` `new`, `regular` or `close`, or `null` for nobody near, someone not remembered or not recognised. No name (#1670, [eyes.md](eyes.md)) |
| `HumanSeems` | `humans/seems` | latest | 2 s, good for 5 s | How the near face seems: `kind` `positive`, `negative` or `unsure`, and a `confidence` from 0 to 1. `from`: `fer_plus` or `blendshapes`, the reader that said it; without it, `fer_plus` (the one reader before it). With both readers on, a consumer that wants one takes the stream, not `latest`, and splits by `from`. No name, print or picture. Mood reads its kind with `LAMP_MOOD_SEEMS=on`, whichever reader said it, so a lamp trying it runs one reader (mood.md) (#1671, [eyes.md](eyes.md)) |
| `Speech` | `voice/heard`, `voice/said` | event | none | Text, whether it is finished, and the locale and the request's number if known. After `hri_msgs/LiveSpeech` |
| `VoiceState` | `voice/state` | latest | 1 s | Resting, listening, thinking or speaking |
| `VoiceLink` | `voice/link` | latest | 1 s | Ready, or unavailable with one of seven kinds of reason |
| `Online` | `lamp/online` | latest | 1 s | `online` true, false or absent (nobody knows), `from` the part whose link said so, `down_for_s` while offline |
| `RoomState` | `room/state` | latest | none | Unknown, quiet or busy |
| `VoiceTurn` | `voice/turn` | event | none | A turn's request and its verdict, `answered` or `held_back` as not said to the lamp (#1155). Said by the voice for each turn its rule judged, and `held_back` for a line the model declines (`not_for_me`); read by the policy's hello with `--hello-quiet to-the-lamp` ([policy.md](policy.md#the-quiet-counts-talk-to-the-lamp-1155)). `turn_topics()` lists it |
| `VoiceRemarked` | `voice/remarked` | event | none | A line the voice held as a remark about the lamp in a busy room, its request's number alone, never its words (#4175). Said by the second route with `LAMP_REMARK_ABOUT_IT=on`, as `voice/turn` says `held_back` for it; read by Expression with `LAMP_REMARK_GLANCE=on` for one small glance ([expression.md](expression.md)) |
| `VoiceTone` | `voice/tone` | event | none | A line's request, its arousal (calm -1 to lively +1) and how sure, from its prosody against the person's baseline, and its words' valence (down -1 to glad +1) when a word says it. Said by the voice with `LAMP_VOICE_TONE=on` ([voice-tone.md](voice-tone.md)). `tone_topics()` lists it |
| `RoomPresence` | `room/presence` | latest | 1 s, each good for 3 s | Whether anyone is at the lamp (#1677, [presence.md](presence.md)): `someone`, a face seen or a voice heard within 20 s (`RoomPresence::SOMEONE_WITHIN_S`), and `nobody_s`, how long nobody has been there in whole seconds (0 while someone is), from the last face or voice or from when the eyes began looking; and `by`, while someone is, what last showed it (`face` or `voice`; absent otherwise). Said by the eyes with `LAMP_PRESENCE=on`, once a second while they look and at once when `someone` changes; nothing while they are not looking, so no fresh value is not known, never nobody. Read by Light out when you leave and by Messages (#1673). `presence_topics()` lists it |
| `RoomActivity` | `room/activity` | latest | 1 s, each good for 3 s | What the face at the desk shows, against the person's own sitting (#1678, [eyes.md](eyes.md#what-the-person-at-the-desk-is-doing-lamp_activityon-1678)): `state` (`away`, `at_desk`, `leaning_in`, `head_down`; `leaning_back` and `sitting_low` were dropped after lamp-75c9's sitting), `for_s`, how long it has held, `sitting_s`, the sitting's length (0 while away), and `baseline`, whether the sitting's first two minutes are in (without it only `away` or `at_desk`). `yawns`, this sitting's yawns, added 2026-10-27, optional and left out at 0 (eyes with `LAMP_EYES_YAWN=on`, #3709; 0 while away). Said by the eyes with `LAMP_ACTIVITY=on`; nothing while they are not looking. Read by Stretch break. `activity_topics()` lists it |
| `StoodUp` | `room/stood_up` | event | none | Someone at the desk stood up and left (#1678): `sitting_s`, the sitting they left, and `side` (`left` or `right`, the side of the picture they were last seen at; added 2026-10-20, optional, left out near the middle). It lasts 5 s (`StoodUp::LASTS`, `expires_ms`), so a reader that keeps the newest value lets it go and the next departure starts anew. Said once by the eyes with `LAMP_ACTIVITY=on`. For a reader that watches them go (#1669). `activity_topics()` lists it |
| `Light` | `light/command`, `light/state`, `light/test`, `light/drawn` | latest | 1 s | On or off, one of ten colours by name, a level from 1 to 5; `Light::TINTS` is each colour as the ring draws it, which `lamp-cloud` also tells the app ([cloud.md](cloud.md)) |
| `Diagnostic` | `diagnostics` | event | 1 s | ROS's `DiagnosticStatus`: ok, warn, error or stale, a name, a message, a few keys and values |
| `Health` | `lamp/health` | latest | 5 s, means nothing after 15 s | How the whole lamp is: the worst level, one sentence, every part that has reported with its level, message and age, worst first, and the parts that are expected and never reported |
| `BoardState` | `board/state` | latest | 5 s, means nothing after 15 s | The computer's numbers: each temperature by sensor (°C), load per processor, memory in use and disk free as fractions, uptime, and the Wi-Fi signal and whether the clock is set if they could be asked |
| `NameHeard` | `name/heard` | event | none | The lamp's name was heard in what its microphone hears: how sure the listener was, from 0 to 1, and how many milliseconds before this was sent the name began. A sign, never proof ([name.md](name.md)) |
| `NameHeard` | `name/rising` | event | none | The same hearing's first sign, as soon as the score reaches the listener's bar and while the name is still being said: the score so far, and how long ago it reached the bar. Its final word, with the highest score, is on `name/heard`; a first sign with none was a near miss (#782, [name.md](name.md)) |
| `NameHeard` | `name/near` | event | none | A hearing that reached `lamp-name --near`'s lower bar (`LAMP_NAME_NEAR`), when it is over: its highest score, and how long before this was sent it reached that bar. Read only beside a line's first word, never as the name alone ([name.md](name.md)) |
| `StopHeard` | `stop/heard` | event | none | "Stop", "wait" or "hold on" was heard in what the microphone hears, by `lamp-name`'s `stop.onnx` with `--stop-least` (off unless set): the score at the step it first reached the bar, and how many milliseconds before this was sent. Said at once; a sign, never proof (#828, [stop-word.md](stop-word.md)) |
| `ShutdownRequest` | `lamp/shutdown_request` | event | none, lasting 2 s | Turn the lamp off (#3697): `stamp` (the asker's boot clock), `source` (`button` from `controls`, the only one: the app asks through setup's change helper) and `reason`. Only `lamp-setup watch-reset` acts on it, with `LAMP_SHUTDOWN=on`, through `ShutdownRequest::accept`: from its source's own publisher, this boot's clock, under 2 s old ([shutdown.md](shutdown.md#the-request)). `shutdown_topics()` lists it |
| `Pause` | `lamp/pause` | latest | none | Whether the lamp is paused, as `lamp-keeper pause` or `resume` last said; kept until the next, read with `latest` by each part that honours it ([keeper.md](keeper.md), "Named states and the pause") |
| `StandAsked` | `body/stand_asked` | latest | none | The stand a person asked for by voice (#4135): `share`, 1 to 100 percent of the lamp's own stand (`StandAsked::check`); kept until the next or until the bus is made again at power-up. Read by lamp-body with `LAMP_BODY_POSE_ASK=on` in place of `--stand-share` ([body.md](body.md), "A stand asked by voice"). `body_topics()` lists it |
| `UpdateStatus` | `update/status` | latest | every 60 s, lasting 180 s | What lamp-update is doing and last did: the running build, `now` (`idle`, `checking`, `fetching`, `switching`, `watching`, `restoring`) and the last round's outcome by name, build and time, with no reason text; for a round that refused, failed, rolled back, was unreachable or found no quiet moment, a fixed name for where it stopped (`reason`, `UpdateResult::REASONS`: the phase it was in, or `models`), and when the round began (`started_at_unix_ms`), both absent for a round that ended before its first phase and for a value read back from the records at start; said at each phase by a round that acts, read by lamp-cloud for the app (#2194, [cloud.md](cloud.md)) |
| `UpdateCheck` | `update/check` | event | none | The app's Check for update (#4153, [cloud.md](cloud.md)): empty (`{}`). Said once by lamp-cloud for each `update.check` it takes, at most once a minute (`UpdateCheck::AGAIN_AFTER`); read by `lamp-update serve --apply on` between rounds, whose next round then starts at once with all its own rules. A check that came during a round is dropped, and `serve` too takes at most one a minute. Nothing is kept, so a lamp-update that starts later reads no old one; an older lamp-update never subscribes ([updates.md](updates.md)) |
| `Hand` | `body/hand` | latest | 1 s | Whether a hand is pushing the lamp now, as the body feels it (`pushed`), and when that push began on the body's own clock (`since_us`, 0 when not pushed); said with every step lamp-body passes on the bus, every 80 ms ([body.md](body.md), "A hand on the bus") |
| `BodyGesture` | `body/gesture` | event | none | What became of one of the body's gestures: its `name` (the clip's, the body's own `idle` and shifts included) and `state`, `began`, `over` (played out, let go, replaced, or the run stopped) or `dropped` (asked and dropped before it began, the anti-tip rule's refusal and #1239's `busy` and `held by a hand` included, or ended for a higher ask, so no `over` follows). Said by lamp-body with `LAMP_BUS=on` where its journal says the same ([body.md](body.md), "On the bus"). Under `lamp-body --arbitration on` (#940), a drop or an end also carries optional `source` (whose it was), `why` (the journal's reason) and `by` (who holds the body instead, a skill or the conversation that named itself), each left out when not known, so the message is unchanged without the switch. Read-only. `body_topics()` lists it |
| `VoiceMute` | `voice/mute` | latest | 3 s | Whether the voice's software mute is on (`muted`), and why: `reason: "sleep"` when a hand held on the head put the lamp to sleep, absent for the mute as it always was (#711). Only while muted, `muted_by` (#4332) says who or what muted it: `sleep`, `button` (the person's hold), `restart_guard` (`HandControls::restored_after_fault`) or `unreadable` (a record read as muted because the voice couldn't trust it); `lamp-cloud`'s `mic.get` passes it on. It's a field, not new `reason`s: a `reason` an older reader doesn't know fails its parse, a field it doesn't know is skipped, and a voice before it says none. Said by the voice with `LAMP_HEAD_TOUCH=on` and `--controls-mute`, when it changes and every second; Expression's `--head-touch` sleeps from it. `mute_topics()` lists it. Not the physical slide |
| `LightRequest` | `light/request` | event | none | A light asked for that lapses (#940, [skills.md](skills.md)): a `claim` (`source`, a part or `skill:<name>`; `priority`, one of `idle`, `skill`, `conversation`, `safety`, `person`, in the order they give way; the asker's `clock_id` and `until_us`, at most 30 s ahead, renewed by asking again) and the `light`, and an optional `pattern` (#940 item 3: `kind` `breathe`, `candle` or `pulse`; `period_ms`, 4–12 s for a breath, 1–4 s for a candle's wander, 3–6 s for a pulse's way around; `low`, a breath's floor from 1 to under the light's level, which is the top, unused by a candle, which dips at most a fifth under its level and only at level 3 and up, and by a pulse, which rises over it, at level 2 and up; none for a light that is off), and an optional `fade_ms` (#1677: 1 to 60 s, how slowly the ring rests into the light, as a light going out when nobody is there fades; none, its own quick ease). The ring shows the highest-priority request that holds, the newest claim of equal priority (a renewal keeps its place), and the person's lasting `light/command` once none does, with `LAMP_LIGHT_REQUESTS=on` ([ring-choreography.md](ring-choreography.md), "The lamp's own light"). `arbitration_topics()` lists it |
| `LightOutranked` | `light/outranked` | event | none | A light asked for on `light/request` that the ring holds but doesn't show: its `source` (the hidden request's `claim.source`) and `by`, the `claim.source` shown over it or `person` for the person's own light (#940). Said when it is taken hidden, when it loses its place and when what is over it changes, and again after each renewal while it stays hidden. The asker keeps its claim, which is shown by itself once what is over it lapses; nothing is said for what hides every request alike (the privacy switch, a turn's face, a self-test) ([ring-choreography.md](ring-choreography.md), "The lamp's own light"). `arbitration_topics()` lists it |
| `LightReached` | `light/reached` | event | none | A light asked for on `light/request` that the ring has shown on resting frames for 350 ms, said then and again after each renewal while it is shown: its `source` (the request's `claim.source`) and the `light` (#1146). Nothing is said inside a turn, whose face never shows the lamp's light, nor for a light never shown that long ([ring-choreography.md](ring-choreography.md), "The lamp's own light"). `arbitration_topics()` lists it |
| `Rules` | `memory/rules` | latest | none | What the people at the lamp asked it about speaking first, as typed facts for the policy (#813, [memory.md](memory.md#what-a-person-teaches-and-corrects-813)): `person` (the memory's id `p-<hex>` of the known person, or `everyone`), `quiet` (asked never to speak first, unprompted), `quiet_mornings` (asked not to speak first in the mornings, before noon local time, or never), `rules`, how many rules of every kind the room's and theirs hold, and `hello` / `news` (`Unprompted`: `never`, `not_before_minute`, `not_from_minute`, local minute of the day, `not_while_talking`; absent in an older message, nothing held), the greeting's and the unasked news's limits, every typed rule merged, the strictest winning; and `keep_light_on` (#1677), the room's rule to keep the light on, whoever asked (written only when true; absent, as in an older message: not asked). Said by the voice with `LAMP_MEMORY_RULES=on` at each session open and each rule kept or forgotten; kept until the next. Never a rule's words |
| `MemoryForgot` | `memory/forgot` | event | none | Memory has forgotten everything it kept under a person, asked by the app or by voice: the person's id only, or none for every person ([memory.md](memory.md#forgets-told-on-the-bus-memoryforgot)). Said by memory with `--bus-root`, after the asker's answer; read by the voice's tone baselines. `check()` takes memory's own ids only (`p-…`, its `r-…` twin, `everyone`), never another slug, which could be a name. `memory_topics()` lists it |
| `Decision` | `policy/decisions` | event | none | What the policy decided ([policy.md](policy.md)): what it was about, the choice, its probability, the decision model's answer to each question by the question's name, who decided, how long the asking took, whether anything acts on it, the request's number if known, for a `progress` decision the work it was about (`task`, `<store>:<task>:<change>@<run>`), and for a `hello` decision the arrival (`arrival`), both added 2026-10-15 as optional fields; for an answer to a skill's ask, the ask (`ask`, 2026-10-16, optional); for an answer to an add-on's say (`about` `addon`), its proposal (`proposal`, 2026-10-21, optional, #2530). Not what was heard |

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
`memory/rules` or `memory/forgot`: `memory_topics()` lists them; nor `name/rising` or `name/near`, which
`name_topics()` lists; nor `stop/heard`, which `stop_topics()` lists.
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
| `ExpressionRestRequest` | `expression/rest_request` | event | 1,000 / 500 ms | Skills' leased rest, wake or cancel intent, bound to the host clock, publisher, Expression session and episode |
| `ExpressionRestState` | `expression/rest_state` | latest | 200 / 500 ms | Expression's correlated planning receipt: idle, resting, waking, completed, cancelled or refused, a refusal with its optional `reason` (at most 160 bytes, absent otherwise) |
| `ExpressionSleepClaim` | `expression/sleep_claim` | latest | 60,000 / 180,000 ms | lamp-skills' live claim on the night's sleep (#940): the skill's name (`night-sleep`) and `on`; Expression treats one older than 180 s as none and sleeps by its own schedule, the fallback for one release |

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

The rest contracts are included in `expression_topics()`. Admission or lease
extension requires an original observation fresh within 500 ms, and a lease
is at most three seconds. A stale, unexpired same-episode Rest renewal may
be discarded while the prior lease and receipt stay unchanged; it adds no
time. A new receipt is not a renewal of its request. A completed
receipt reports planning completion, not physical movement. The rest
request, receipt, nested echoed request and clock stamps follow the common
additive-field rule above. Unknown `op` or `phase` values are refused;
required identities, clocks and lease bounds are still checked. See
[Bedtime and Morning](bedtime-morning.md) for the default-off switches and
owner cancellation rules.

## Optional output restrictions

`output_caps_topics()` adds one complete Actions snapshot:

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `OutputCaps` | `output/caps` | latest | 1,000 / 3,000 ms | Actions session, persisted revision and up to eight source-owned light/volume ceilings |

The snapshot, nested `OutputCap` entries and clock stamp accept additive
fields. Required fields, scalar bounds, identities, duplicate sources,
envelope identity and freshness remain checked. Snapshot freshness controls
admission, not a restriction's lifetime: a missing heartbeat cannot release
an accepted ceiling. Unknown wall time retains restrictions; a newer valid
complete snapshot may explicitly release them. A ceiling grants no output
authority and combines with other ceilings by minimum. Actions' private
`caps_put`, `caps_remove` and `caps_get` socket commands retain their separate
strict schemas, including rejection of unknown fields and operations. See
[Bedtime and Morning](bedtime-morning.md) for clock and consumer limits.

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
capture. With `LAMP_PRIVACY_CONTROLS=no-slide` Controls reads no slide and
says so instead: `no_privacy_slide: true`, left out when false, never beside
`privacy` (`check()` refuses both). [Privacy switch](privacy-switch.md) describes wiring, authority and
the coordinated opt-in across Controls, Voice and Ears.

## Optional microphone permission contract (#755)

`privacy_topics()` declares two new topics, separate from the original
seventeen. Only Voice's selected privacy worker publishes `voice/privacy`;
only the eyes publish `eyes/camera_closed`.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `MicrophonePrivacy` | `voice/privacy` | latest | 10 / 50 ms | Voice controller identity, microphone generation, original physical acquisition, expiry, capture permission and known/unknown physical mute |
| `CameraClosed` | `eyes/camera_closed` | latest | on ask | The eyes' receipt for a camera close the voice asked about (`{"close_receipt":N}`, #1802): the voice's token `close` and `released`, whether the last stop of the capture device returned without a fault; said only once the camera is shut |

Version 1 of `lamp_msgs/MicrophonePrivacy` carries a publication `ClockStamp`,
16-character hexadecimal Bus `session_id`, 32-character hexadecimal
`controller_boot`, positive `microphone_generation`, `observed_us`,
`expires_at_us`, `permitted`, optional `physical_muted`, and `no_slide`, left
out when false. Unknown physical state uses `observed_us = 0` and cannot permit
capture, except that a `no_slide` lease (a lamp with no slide) may permit with
`physical_muted` unknown: its `observed_us` is the Controls publication, held
to the same rules as a switch reading, and it never has a physical level. A known sample's expiry
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
100 ms declared period/300 ms lifespan), and `controls/pet` (`HeadPet` v1,
event, 500 ms lifespan; #3997). `declaration_of` recognizes all three;
`first_topics()` and the existing `controls_topics()` inventories are unchanged.
A pet is its own type on its own topic, not a `TouchGesture` value, because
older readers refuse an unknown gesture and would then refuse the whole
retained `controls/touch_state`. `HeadPet` ignores additional fields; its
check holds `began_us <= petted_us <= observed_us <= stamp.at_us`, at least 3
swaps, `swaps_per_second` from 0 to 25, and `ended` only with `resting`
([controls.md](controls.md), "A pet"). Only `lamp-controls` with
`LAMP_HEAD_PET=on` declares and publishes it.
Only the explicit `lamp-controls --touch` path publishes them: a change at
once, what only grows while a hand stays at most every 20 ms, events, and
every 100 ms otherwise ([controls.md](controls.md)). `TouchEvent`,
`TouchSourceState` and `TouchState` ignore additional JSON fields, including
fields in nested sources and retained events. The known fields keep their
required types and semantic checks; unknown source, gesture and intent values
are refused. An extra field such as `permit` supplies no authority and is not
written back. Physical privacy evidence keeps its separate strict contract.

This reader tolerance must ship before a later publisher adds fields, as the
[one-build-apart rule](updates.md#what-parts-say-to-each-other-one-build-apart)
requires. It cannot make older deployed readers accept an extended message.
The existing touch JSON output, topic declarations and old controls shapes
are unchanged; an added field must not change the meaning of an existing
action or replace a required check.

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

## Optional speaker observation and private audio receipts (#1685)

`crates/msgs/src/speaker_state.rs` defines the optional bus type below.
`speaker_state_topics()` and `declaration_of()` find it; `first_topics()` is
unchanged. Actions publishes it only with exact `LAMP_REQUESTED_AUDIO=on`.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `SpeakerState` | `speaker/state` | latest | 1 / 3 s | The mixer's owning Actions process reports host `stamp`, publisher `session_id`, monotonic `mute_generation`, optional `volume_raw` (0–100) and logical/observed `muted`; unavailable volume grants no output |

The bus envelope must identify `actions`, `lamp_msgs/SpeakerState`, JSON and
Latest with a three-second expiry. The payload's publisher session, sequence
and monotonic timestamp must match the envelope; consumers also check host
boot, freshness and generation/sequence continuity. Actions advances the
generation before handling an explicit mute's mixer I/O. Later unmute does
not restore an old generation. Additive fields are tolerated as for other
bus messages; required fields and known values remain checked. This is an
observation, not permission to change volume or proof of physical silence.

`crates/msgs/src/requested_audio.rs` instead defines strict private socket
receipts: `RequestedAudioContext`, `RequestedAudioOffer`,
`RequestedAudioOperation` and `RequestedAudioLease`. They have no bus topic.
They reject unknown fields and unknown operation/phase/outcome enum values.
Voice supplies the context outside model arguments; Skills binds its offer
to checked asset bytes, definition generation and original monotonic
deadlines. See [requested audio](requested-audio.md) for all fields, the
500 ms lease, final output checks and bounded revocation semantics.

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
| `VoiceArrival` | `voice/arrival` | event | none / none | A hello is due and held for the policy: published by the voice until #2825 removed its own hello, published by nobody now; the policy puts a skill's hello ask as one ([speaking-first.md](speaking-first.md#asking-the-policy-752)): the arrival's number, minutes away, seconds there, seconds of quiet, minutes since the last hello if any. No picture, no face, no identity |

Published by nobody since #2825 (by the voice with `LAMP_HELLO_ASKS_POLICY=on` until its own hello was removed); the policy still puts a skill's hello ask as one. `check()`
asks only for a positive number. `Decision` has the optional `arrival` the
policy's `hello` decision names it by; it is left out of every other
decision, so their bytes are as they were (a test).

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `AddonProposal` | `addon/proposal` | event | none / none | A partner's add-on suggests what the lamp might do ([addons.md](addons.md)): the proposal's number (`lamp-skills`' monotonic clock in µs), the add-on's name and version as `lamp-skills` loaded them, the act (`say`, `light`, `move`, `mood`, `note`), why (a fixed list), how sure (0 to 1), how long it holds (1 to 30 s), and the one part its act names: a light, a gesture, a mood nudge (±0.1 at most, 10 min at most), or a note of at most 300 bytes from the add-on's folder. Written by `lamp-skills` alone, with `LAMP_ADDONS=on`. Never an order |
| `AddonDecision` | `addon/decision` | event | none / none | The add-ons' judge on one proposal ([addons.md](addons.md), #2530): the proposal's number, the add-on, its act, `kept` or `dropped`, `by` `rule` (or `jev` for a say, later), and why in at most 200 bytes. No words of the proposal's note |
| `AddonAsk` | `addon/ask` | event | none / none | An add-on's say past the judge's rules, put to the policy (#2530 part 2, [addons.md](addons.md)): the proposal's number, the add-on, its `why` and how `sure` it said it was. No words: the note stays with the judge. Answered by a `Decision` with `about` `addon` naming the proposal. Only `lamp-addons` writes it |
| `MoodNudge` | `mood/nudge` | event | none / none | A kept add-on's mood for `lamp-mood` (#2530): a clock stamp, the add-on, energy within ±0.1, warmth from 0 to 0.1, and `until_us` at most 10 minutes after the stamp. Only `lamp-addons` writes it |
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
| `SoundClasses` | `ears/classes` | latest | 500 ms / 1,500 ms | Unavailable with no scores, or multi-label speech/music/laughter scores with original window times, completion, capture epoch, model id and own-voice evidence; Ears publishes it only with `LAMP_EARS_CLASSES=on` and `LAMP_PRIVACY_CONTROLS=on`, or `no-slide` with the person's stored yes, off by default (#1669, sound-events.md) |

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

## The texting contract

Added by the Fourth on 2026-10-19 (#1673). The types live in `src/chat.rs`
and are re-exported at the crate root; `chat_topics()` and
`declaration_of()` find their topics, and `first_topics()` is unchanged.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `ChatIn` | `chat/in` | event | none / 60 s | A text the person sent from the Autonomous app ([messages.md](messages.md)): the `run_id` `lamp-cloud` acked it with, the app's `session_id` if it gave one, the text, and `speak` (say the reply aloud too; off unless asked) |
| `ChatOut` | `chat/out` | event | none / none | A piece of the lamp's answer: the `run_id` it answers (none only for task news), the `session_id`, the `ChatPart` (`delta`, `final`, `error`, `task_news`) and its text |
| `MessageOut` | `messages/out` | event | none / 10 min | A short text the lamp sends unasked, for the person's phone: `from` (`skill:<name>`) and `text`, one line of at most 280 bytes; guard mode's news ([messages.md](messages.md), "Unasked texts"). Published by `lamp-skills`; nothing carries it off the lamp yet |

Published by `lamp-cloud` (`chat/in`) and the voice (`chat/out`). `check()`
holds a run or session name to 1 to 64 letters, digits, `-`, `_`, `.` or
`:`, and a text to 16,000 bytes; a `ChatIn` has words in it, and a `ChatOut`
carries a run unless it is task news, which never does. Photos and files
are not carried yet: a bus message is at most 60,000 bytes. A `ChatIn`
lasts `CHAT_ANSWER_WITHIN` (60 s), the minute `lamp-cloud` waits for its
answer before it ends the run, so one still queued after that is passed
over, never answered (the reviewer on #1713).

## The reports contract

Added by Jarvis on 2026-10-19 (#1683). The types live in `src/reports.rs`
and are re-exported at the crate root; `reports_topics()` and
`declaration_of()` find their topics, and `first_topics()` is unchanged.

| Type | Topic | Kind | Period / lifespan | Meaning |
|---|---|---|---|---|
| `ReportsRequest` | `reports/request` | event | none / none | A part asks for health and crash reports to stop: `share`, and `held` (left out when false) when the line's answer was held. `lamp-cloud` takes only `false` from the bus, which anything on the lamp can write ([cloud.md](cloud.md), "Health and crash reports") |
| `AccountState` | `account/state` | latest | 5 s / 15 s | Whether this lamp's login is no longer accepted (`signed_out`) and how long that has lasted (`for_s`), said by `lamp-cloud` with `LAMP_SIGNED_OUT=on` and read by the voice ([cloud.md](cloud.md), "Signed out"). Past its lifespan or absent means signed in |
| `ReportsState` | `reports/state` | latest | 5 s / 15 s | Whether reports are being sent (`share`: the switch on and the person's yes), how many wait (`queued`), and how the newest choice was made: `by` (`voice`, `app`, `setup` or null), `held`, and `at_s` (wall seconds) |

Published by the voice's `reports_stop` (`reports/request`) and
`lamp-cloud` (`reports/state`), only with `LAMP_REPORTS=on`; the voice's
`reports_state` reads `reports/state`.

## The owner's place (not a topic)

`lamp_msgs::owner_place` (#4333, [owner-place.md](owner-place.md)) is no
message on the bus: it is the one copy of the owner's place's rules, read by
`lamp-cloud`, by `lamp-setup change` and by the voice. `Place` is
`{name, lat, lon, set_at}`: `name` an area name (the neighbourhood, district
or city as the phone's geocoder gives it, which the app builds from
sub-locality and locality), never a street address, checked only for 1 to 64
characters on one line, digits allowed ("District 1"); `lat`/`lon` already
rounded to 2 decimals by the app. `fit` checks the app's `data` and rounds it
(`rounded`: latitude to 0.01°, longitude to a step at least 1 km wide at the
cell's edge nearest the pole); `read` reads `/etc/lampOS/person/place` and
rounds again; `utc_time` keeps `set_at` as ISO 8601 UTC to the second.
