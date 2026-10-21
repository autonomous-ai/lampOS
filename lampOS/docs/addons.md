# Add-ons

Proposed by blue as blue-P1 on 2026-10-21
([#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6309164481)),
shaped by the first builder's answer the same day
([#1722, 13:1x](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6309437399)).
**Built so far** (blue, 2026-10-21, off unless `LAMP_ADDONS=on`): the
`addon` kind and its load-time refusals, the read allowlist,
`lamp_msgs::AddonProposal` on `addon/proposal`, and `lamp-skills` writing it
within the rate limits below (`crates/msgs/src/addon.rs`,
`crates/skills/src/addon.rs`). The judge's part 1 (#2530): `lamp-addons`,
its own process (`crates/addons`, `deploy/product/lamp-addons.service`),
which reads `addon/proposal` and the lamp's state, keeps or drops each
proposal by the rules below, asks the ring for a kept light at `Idle`, and
says every verdict on `addon/decision` (below, "The judge as built").
**Not built yet:** carrying out a kept mood (a nudge input on `lamp-mood`,
waiting for the first builder's word, #2530), the say with its Jev question
and the say service's context line (#2530 part 2), and everything after
1.0.

## Why

Partners want to sell the lamp with their own logic (an emotion, a light, a
behaviour) without building a lamp from scratch. An add-on is that logic. It
reads a few summaries of what the lamp senses and suggests what the lamp
might do. The lamp stays one living thing with one voice: it judges every
suggestion, and nothing an add-on writes reaches a speaker, a light or a
motor unjudged.

## The rule in one line

An add-on **reads** the topics on one allowlist of summaries and **writes
one topic**, `addon/proposal`, which `lamp-skills` writes on its behalf. A
judge keeps or drops each proposal; a kept one is carried out by the parts
that own the outputs today, at `Skill` priority or below.

## On the lamp, an add-on is a skill

There is no second plug-in system. An add-on is a skill folder
([skills.md](skills.md)) of a new kind, `addon`, and takes everything a
skill has: the package and its signature, `install`, `remove` and `list`,
the Skills Store's lamp side, `[permissions]` checked at load and again at
every call, triggers, `[[while]]`, `[quiet]`, `[active]`, `most_a_day`,
`[budget]` and the bus recordings in `tests/`. What is new is small:

1. one typed write topic, `addon/proposal`;
2. a read allowlist for the `addon` kind;
3. one judge flow, with one new Jev question that acts;
4. a context line on the say service, so the voice can know a kept note
   without being told to speak it.

```text
add-on skill folder (data only: triggers, conditions, propose steps)
        │  lamp-skills loads it, checks [permissions] and the allowlist
        ▼
lamp-skills ── addon/proposal (stamped with the skill's name, rate-limited, leased)
        ▼
judge: rules (light, move, mood, note), then Jev for a say ("speak now?")
        │  policy/decisions, naming the proposal
        ▼ kept
lamp-skills carries it out through today's owners, each call checked again:
  light  → light/request       (ring arbitration, priority Skill or Idle)
  move   → the body's sockets  (body arbitration, priority Skill or Idle)
  mood   → lamp-mood's nudge input (new, bounded)
  say    → the say service, as a note the model phrases
  note   → the say service's context line → the voice's try_context
```

## The `addon` kind's manifest

`skill.toml` says `kind = "addon"` and a `version`; beside it, as in every
skill folder, a `SKILL.md` with its `name` and `description` (the examples
below show only `skill.toml`). An add-on is always
installable: it is never core, and `remove` takes it off like any
installable skill.

```toml
kind = "addon"
version = "1.0.0"

[permissions]
topics = ["ears/classes", "voice/state"]   # only from the allowlist below
propose = ["mood", "light"]                # what it may propose, and nothing else
```

- **`propose`** is the add-on's only output permission: a list of `say`,
  `light`, `move`, `mood` and `note`. An empty or missing list lets it
  propose nothing.
- **Refused for the `addon` kind:** `speak`, `light`, `body`, `look`,
  `messages`, `known_faces`, `output_limits` and `actions` in
  `[permissions]`; `say`, `sound`, `cue`, `light`, `body` and `look` steps;
  `[ask]` (the judge is asked about every proposal); `tool` (a person's
  call is the model's, not a partner's); `[history]`; and `run.py`, as for
  every skill. An add-on that names any of them is not loaded, and the
  journal says what it named.
- **Its steps** are `[[do]] propose = { ... }` only (below).
- **The read allowlist** is checked at load: a topic in `topics` that is
  not on it refuses the add-on.

## What an add-on may read

Summaries only, in phase 1:

| Topic | What it gives an add-on |
|---|---|
| `voice/state` | resting, listening, thinking, speaking |
| `room/presence` | `someone` at the desk or not, and for how long nobody was; never who (see below) |
| `mood/state` | awake, energy, warmth, asleep |
| `light/state` | the light shown: on, colour, level |
| `ears/classes` | the sound classes heard (music, laughter, ...) |
| `clock/local` | the local time of day, for the day's phase |

`room/presence` is read with `known_faces` refused, so an add-on is never
told whether the person is known, nor who.

**Never on the allowlist:** `voice/heard`, `voice/said`, `chat/in`,
`chat/out`, `humans/*`, `camera/*`, `mic1/audio`, `mic2/audio`,
`memory/rules`, `schedule/due`, `tasks/status`, `messages/out`, and the
lamp's memory. An add-on that wants more asks the person, like any
permission, through a later change to this list, not through its own
manifest.

## `addon/proposal`

A `lamp-msgs` type, `lamp_msgs/AddonProposal`, version 1, an event topic.
`lamp-skills` is its only publisher; an add-on never writes the bus.

| Field | What it is |
|---|---|
| `proposal` | This proposal's number: `lamp-skills`' monotonic clock in µs when it was made; grows, never 0. The decision names it |
| `addon` | The add-on's skill name, stamped by `lamp-skills` from the folder it loaded, never from the add-on's own words |
| `version` | The add-on's version, as loaded |
| `act` | `say`, `light`, `move`, `mood` or `note` |
| `why` | One of a fixed list of reasons, never free text: `music`, `laughter`, `evening`, `morning`, `arrived`, `left`, `quiet`, `long_sitting`, `mood` (`lamp_msgs::ADDON_WHYS`) |
| `sure` | How sure the add-on is, 0.0 to 1.0, as its manifest states it; the judge may weigh it, never be bound by it |
| `holds_ms` | How long the proposal stays worth doing: 1 s to 30 s, as a `Claim`'s lease (`msgs/src/arbitration.rs`) |
| `light` | For `light`: colour, level 1 to 5, and an optional pattern (`breathe`, `candle`, `pulse`), the same shape a skill's light takes |
| `gesture` | For `move`: one of the body's gestures, by name, played within the body's limits. A clip of the add-on's own `moves/` comes later |
| `mood` | For `mood`: a nudge of `energy` or `warmth`, at most ±0.1, and `for_ms`, how long, 1 s to 10 minutes (`for = "10m"` in the folder) |
| `note` | For `say` and `note`: what the add-on suggests the lamp might mention, at most 300 bytes, written in the add-on's folder, never made at run time. It reaches the model quoted and untrusted, never as an instruction (below, "The judge") |

Every text an add-on can send is in its folder, so it is read when the
package is signed and installed, and a running add-on cannot compose words.

**Rate and lease** (to be measured on a lamp): per add-on, at most one
proposal every 10 s and 20 an hour; at most 4 `say` proposals an hour
across all add-ons (built, `crates/skills/src/addon.rs`); at most 2
waiting for a decision (with the judge). A proposal over a limit is not
published, and the run's `skills/events` says why. A kept proposal holds
for its `holds_ms`, and its claim then lapses by itself.

## The judge as built (#2530 part 1)

`lamp-addons serve`, off unless `LAMP_ADDONS=on` (or `--enable`): without it
it opens nothing and publishes nothing. It reads `addon/proposal` (only from
`lamp-skills`, the source every proposal must have) and these topics, each
taken the cautious way when never read or no longer fresh, except where
nothing said is the lamp's ordinary state (the pause, and the sleep mute
on a lamp without the head's controls):

| Rule | Read from | Dropped when |
|---|---|---|
| Privacy | `voice/privacy` | the voice does not permit capture now, or nothing fresh (1 s) |
| Pause | `lamp/pause` | paused; never said is not paused, as `lamp-keeper` means it ([keeper.md](keeper.md)) |
| Asleep | `voice/mute`, `expression/rest_state` | muted for sleep, or the rest skill resting (its word fresh within 1 s), or a mute once said gone stale (3 s). The voice says its mute every second only with the head's controls on (`LAMP_HEAD_TOUCH`, `--controls-mute`); with none said 3 s after the judge starts, or after the voice restarts (a new publisher on `voice/state`, which may run without the controls), the rest skill alone tells sleep |
| Setting up | `onboarding/pending` | its last word holds an invitation: setup owns the light and the voice while it runs a step |
| Talking or speaking | `voice/state` | not `resting`, or nothing fresh (3 s) |
| A busy room | `room/state` | not `quiet` (`unknown` included), or nothing fresh (3 s) |
| Colder (a `mood` only) | the proposal | its warmth is below 0: an add-on may warm the lamp, never cool it |
| The person's light (a `light` only) | `light/command`, `light/state`, `mood/state` | the newest `light/command` this boot came from the actions part or the app, and `light/state` shows it (or one of the judge's own glows is over it); with none, the ring shows its own idle light. It stays the person's base for good. Once it **rests**, an add-on light is not dropped but asked for **5 s at most** (`BRIEF_GLOW_US`), and the ring then shows their light again by itself. A light they set **off** never rests: off stays dark. An on light rests when Mood has said `asleep` (on the judge's own boot clock) since they set it, or 12 h have passed (`PERSONS_LIGHT_RESTS_US`, the first builder on #2530, 20:1x, after the lamp-75c9 run). Their next choice is fresh again |
| Not yet | the act | `note` or `move` |
| Too soon (a `say` only) | `voice/state` | the voice has not rested for 15 s since it last spoke or was spoken to (`SAY_QUIET_US`), as progress news waits |
| Enough this hour (a `say` only) | the judge's own memory | add-ons together had 4 says kept within the hour (`SAYS_AN_HOUR`) |
| Asking (a `say` only) | the judge's own memory | another say is being put to the policy: one at a time |
| Late | the proposal's age | judged after its `holds_ms` |
| Rate (rule 8) | the judge's own memory | its add-on had one kept within 2 minutes, or 10 within the hour; a dropped one is not counted |

**A say (part 2, #2530).** A `say` past every rule is not kept by the rules alone: the judge puts it to the policy as one `AddonAsk` on `addon/ask` (the proposal's number, the add-on, `why`, `sure`; no words), one at a time, and waits 3 s (`ASK_WAIT_US`) for a `Decision` from `lamp-policy` with `about` `addon` naming the proposal. Only `say_now` that acts (`shadow` false), with the rules holding again as it comes, keeps it, `by` `jev`; `wait` or a shadow answer drops it, `by` `jev`, with why; no answer drops it, `by` `rule`. Only a kept say counts toward the rates. In this PR a kept say is said kept and not spoken: the policy's question and the voice's side come in part 2's next PRs. `--say-quiet-ms` shortens the 15 s for tests only.

A kept `light` becomes one `LightRequest` on `light/request` from
`addon:<name>` at `Priority::Idle`, holding its `holds_ms` (30 s at most) on
the boot's clock: every skill, the conversation, safety and a person
outrank it, and the ring stays the light's one writer. A kept `mood` is
said to `lamp-mood` as one `MoodNudge` on `mood/nudge` (the second PR of
part 1): energy both ways and warmth up only, 0.1 at most, inside Mood's
own clamps, held for its `for_ms` and fading over the last minute, the
newest replacing the last, none taken asleep. A mood that would lower
warmth is dropped. Every
verdict is a journal line and an `AddonDecision` on `addon/decision`
(`proposal`, `addon`, `act`, `choice` kept or dropped, `by` rule, `why`).
The policy's own `policy/decisions` stays `lamp-policy`'s alone (the first
builder, #2530). Not handled yet: a person's light restored from disk at
boot (`LAMP_LIGHT_KEEP=on`, not set on the product) was not set this boot,
so an add-on's light can show over it for up to 30 s.

## The judge as designed

The policy ([policy.md](policy.md)) gets one new question, asked for each
proposal and answered on `policy/decisions` with the proposal's number.

1. **Rules first, for every act.** Dropped when: the privacy switch is
   closed; the lamp is paused or asleep; someone is talking or the voice
   is thinking or speaking; the room is busy (`room/state`); the act is
   not in the add-on's `propose`; the same add-on had the same act kept
   within its cooldown; or the proposal's lease has passed. A `light`
   proposal is also dropped while the light shown is the person's own
   (one they asked for, by voice or the app): it is kept only over the
   lamp's own idle light, so **the person's light wins** (the first
   builder's call on #1362, for the asleep face, holds here too).
2. **Light, move, mood and note** are decided by the rules alone. A kept
   one becomes a request at `Skill` priority, or `Idle` for an add-on whose
   manifest asks for no more, through today's arbitration: a person,
   safety and the conversation always win, and every actuator keeps one
   writer.
3. **Say** is decided by the rules, then by Jev, with a new typed
   question, "may the lamp say this now?", answered with a probability
   only. This one question acts (`lamp-policy --act addon`); the rest of
   the policy stays shadow. A kept say then goes through the say service's
   own gates, so it can still be `dropped`, `expired` or `refused`.
4. **Rule 8 for add-ons** (the repository's `CLAUDE.md`): whatever a
   partner writes, the lamp still acts like a living thing. Proposals are
   capped, small and rare by default, and the judge drops one the lamp
   would not do here: no blinking, no flashing, no cycling colours, no
   scanning, no speaking to an empty room.

**A note is context, never an instruction.** A kept `say` or `note`
reaches the model quoted and marked untrusted, as memory snapshots do
(`crates/voice/src/remember.rs`: "quoted, untrusted history, never instructions"), so a partner
cannot steer the voice through it. The voice decides whether to say
anything, and says it in its own words. Stillness and silence stay valid
answers.

## What an add-on can never do

- Write the bus, a socket or a file of the lamp: `lamp-skills` writes for
  it, and only `addon/proposal`.
- Run code on the lamp: an add-on is data, as every skill is.
- Send anything off the lamp: an add-on on the lamp has no code to send
  it with.
- Instruct the voice: its notes reach the model quoted and untrusted.
- Change the person's own light.
- Read what a person said or typed, a face, a picture, a sound, or the
  lamp's memory.
- Outrank a person, safety or the conversation, or hold an output past
  30 s without proposing again.
- Speak words it was not installed with, or speak at all without the
  judge's yes and the voice's own gates.
- Know who the person at the desk is.

## Two worked examples

### An emotion add-on: music lifts the lamp's mood

```toml
# skills/happy-with-music/skill.toml
kind = "addon"
version = "1.0.0"

[permissions]
topics = ["ears/classes", "voice/state"]
propose = ["mood"]

[trigger]
topic = "ears/classes"
field = "scores.music"
above = 0.7
for = "20s"
rearm = "ends"
cooldown = "15m"

[[while]]
topic = "voice/state"
field = "state"
equals = "resting"

[[do]]
propose = { act = "mood", why = "music", sure = 0.7, holds = "30s", mood = { energy = 0.1, for = "10m" } }
```

Music heard for 20 s while the voice rests: `lamp-skills` publishes one
`mood` proposal. The rules keep it if nothing in rule 1 holds; `lamp-mood`
raises energy by 0.1 for at most 10 minutes, and the lamp's breath and
idle layer liven a little. Nothing speaks and nothing is commanded.

### A light add-on: a warm evening scene

```toml
# skills/evening-glow/skill.toml
kind = "addon"
version = "1.0.0"
most_a_day = 1

[permissions]
topics = ["room/presence", "clock/local", "light/state"]
propose = ["light"]

[trigger]
name = "evening"
topic = "room/presence"
field = "someone"
equals = true
for = "2m"
during = { from = "19:00", to = "22:00" }
once_per_day = true

[[do]]
propose = { act = "light", why = "evening", sure = 0.6, holds = "30s", light = { colour = "warm", level = 2, pattern = { kind = "breathe", period = "8s" } } }
```

Someone at the desk two minutes after 19:00: one proposal for a warm, slow
breath. It is kept only while the ring shows the lamp's own idle light; if
the person chose their light, by voice or the app, the proposal is
dropped and their light stays. If kept, the ring is asked at `Idle`
priority for 30 s, after which the claim lapses by itself; every skill, the
conversation and any word from the person outrank it.

## Phases

**1.0, on the lamp, off by default** (`LAMP_ADDONS=on`). The `addon` kind,
the allowlist, `addon/proposal`, the judge's rules, Jev's say question
acting, the mood nudge input and the say service's context line; a partner
guide with the two examples; the bus recordings in each add-on's `tests/`.
Without the switch, `lamp-skills` refuses `kind = "addon"`, publishes
nothing new, and the lamp is unchanged. The app's skills page (`lamp-cloud`) reads
the same switch from its own unit: without it an installed add-on is
listed as not loading, "add-ons are off on this lamp", never as working.
Turning add-ons on sets `LAMP_ADDONS=on` in both units.

**After 1.0, off the lamp, through Harness.** A partner's add-on runs on
the partner's computer as an agent or service and speaks the same
proposal contract over Harness, so a partner writes one thing. Here the
summaries leave the lamp, so every remote add-on needs its person's yes
for what it subscribes to, seen and revoked per add-on; memory and
context reads come here too, with the same yes. `AGENTS.md` keeps Harness
out of this release, and the transport is the Harness team's (below).

## Harness (after 1.0)

Harness today lets the lamp ask agents on a paired computer to do work
(`lamp-tasks`, [tasks.md](tasks.md); the device protocol over the paired,
end-to-end encrypted channel, `docs/harness.md` in the parent repository).
No agent can read lamp topics or write to the lamp. The remote half needs
new capabilities on that same channel, agreed with the Harness team on
`autonomous-ai/openharness`. The first builder opens that issue once the
on-lamp half has run on a lamp. Its draft:

> **Device add-on capabilities on the autonomous-device channel**
>
> lampOS will let partners' add-ons suggest what the lamp does: an add-on
> reads summaries of the lamp's state and writes typed proposals, which
> the lamp judges ([addons.md](https://github.com/autonomous-ai/lampOS/blob/lamp-voice/lampOS/docs/addons.md)).
> On the lamp an add-on is a skill. After 1.0 we want the same contract
> from an agent on the paired computer. We ask for, negotiated in
> `hello` and off unless both sides offer it:
>
> 1. `addon.subscribe`: the device pushes events from its allowlisted
>    topics (the same summaries as on the lamp) to an agent that asked,
>    with the existing event cursor, ring and resync. These summaries
>    leave the lamp: presence, voice state, sounds and the day's phase
>    say a lot about a person's routine. So each remote add-on needs its
>    person's yes for the summaries it subscribes to (not only for
>    memory), and the person can see and revoke that yes per add-on.
> 2. `addon.propose`: an agent sends one typed `AddonProposal`; the
>    device answers with a receipt (`queued`, `rejected`) and later the
>    judge's decision (kept or dropped). Idempotency keys and the existing
>    token bucket apply.
> 3. `addon.context`: a bounded, read-only snapshot of the lamp's
>    context (who is present as yes/no, room state, mood, recent topics as
>    labels), with the person's yes, never transcripts or pictures.
> 4. Per-add-on identity on the computer side: which agent or package
>    the frames come from, so the device can rate-limit and revoke one
>    add-on without unpairing the computer.
>
> No new pairing, transport or credential: these ride the existing
> encrypted device channel. Questions for the Harness team: where an
> add-on's identity comes from (a Store package, an API provider, a
> pane), and whether `addon.subscribe` fits the event ring's limits.

## Not decided

- The reasons list for `why` and the colours a `light` proposal may name.
- The numbers for rate, lease and the mood nudge, until a lamp shows them.
- Whether `move` is in 1.0 or waits for the body's arbitration to be on
  by default.
- The package signing key for partners' add-ons (the Skills Store's key
  is still the first builder's call, #1983).
