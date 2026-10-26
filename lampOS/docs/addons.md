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
  move   → the body's mood socket (one gesture, priority Idle)
  mood   → lamp-mood's nudge input (new, bounded)
  say    → the say service, as a note the model phrases
  note   → refused at load for now (typed notes, later)
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
  `light`, `move` and `mood`. An empty or missing list lets it propose
  nothing. **`note` is refused at load for now** (the first builder,
  #2530): a note's words would ride on the person's next turn, which has
  every function. If notes come back, they are typed (a kind and bounded
  values the voice renders itself), never a partner's words.
- **Refused for the `addon` kind:** `speak`, `light`, `body`, `look`,
  `messages`, `known_faces`, `output_limits` and `actions` in
  `[permissions]`; `say`, `sound`, `cue`, `light`, `body` and `look` steps;
  `[ask]` (the judge is asked about every proposal); `tool` (a person's
  call is the model's, not a partner's); `[history]`; and `run.py`, as for
  every skill. An add-on that names any of them is not loaded, and the
  journal says what it named.
- **`say = "quoted"`**, the add-on's say grant (#2530 part 2): every add-on
  whose `propose` names `say` declares it, so whoever installs it reads that
  its words may reach the voice, quoted and untrusted. `"quoted"` is the
  only value, any other kind declaring it is refused when it loads, and it
  goes only with `propose` naming `say`.
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
This list is for add-ons on the lamp and for partners' remote add-ons. The
person's own computer is a separate, trusted tier that reads everything
(below, "The person's own computer: everything").

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
| `gesture` | For `move`: `nod`, `curious` or `stretching` (`ADDON_GESTURES`), the body's gesture by name, played within the body's limits. The body's other gestures are refused at load and by the judge: a scan, a shake of the head, a greeting, a dance read as a machine or an answer (the first builder on #2530, 22 Oct 04:25). Others are added one at a time, each after its lamp row. A clip of the add-on's own `moves/` comes later |
| `mood` | For `mood`: a nudge of `energy` or `warmth`, at most ±0.1, and `for_ms`, how long, 1 s to 10 minutes (`for = "10m"` in the folder) |
| `note` | For `say` and `note`: what the add-on suggests the lamp might mention, at most 300 bytes, written in the add-on's folder, never made at run time. It reaches the model quoted and untrusted, never as an instruction (below, "The judge") |

Every text an add-on can send is in its folder, so it is read when the
package is signed and installed, and a running add-on cannot compose words.

**Rate and lease** (to be measured on a lamp): per add-on, at most one
proposal every 10 s and 20 an hour; at most 4 `say` proposals an hour
across all add-ons (built, `crates/skills/src/addon.rs`), a say the judge
drops not counted, so a talking room, where every say is dropped, never
spends the hour before its quiet (lamp-75c9, 22 Oct 07:25), and after a
drop that add-on proposes nothing for 5 minutes (`AFTER_A_DROP_US`); at most 2
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
| Not yet | the act | `note` (refused at load besides) |
| Too soon (a `move` only) | `voice/state` | the voice has not rested for a minute since it last spoke or was spoken to, whoever is near (`MOVE_QUIET_US`): after talk, any move reads as an answer |
| Moved lately (a `move` only) | the judge's own memory | its add-on had a move kept within 30 minutes (`MOVE_EVERY_US`) |
| Enough moves this hour (a `move` only) | the judge's own memory | add-ons together had 2 moves kept within the hour (`MOVES_AN_HOUR`) |
| Too soon (a `say` only) | `voice/state` | the voice has not rested for 15 s since it last spoke or was spoken to (`SAY_QUIET_US`), as progress news waits |
| Enough this hour (a `say` only) | the judge's own memory | add-ons together had 4 says kept within the hour (`SAYS_AN_HOUR`) |
| Asking (a `say` only) | the judge's own memory | another say is being put to the policy: one at a time |
| Late | the proposal's age | judged after its `holds_ms` |
| Rate (rule 8) | the judge's own memory | its add-on had one kept within 2 minutes, or 10 within the hour; a dropped one is not counted |

**A say (part 2, #2530).** A `say` past every rule is not kept by the rules alone: the judge puts it to the policy as one `AddonAsk` on `addon/ask` (the proposal's number, the add-on, `why`, `sure`; no words), one at a time, and waits 3 s (`ASK_WAIT_US`) for a `Decision` from `lamp-policy` with `about` `addon` naming the proposal. Only `say_now` that acts (`shadow` false), with the rules holding again as it comes, keeps it, `by` `jev`; `wait` or a shadow answer drops it, `by` `jev`, with why; no answer drops it, `by` `rule`. Only a kept say counts toward the rates. `lamp-policy --gate --addons` answers it by the moment question, shadow unless `--act addon` ([policy.md](policy.md)). **The voice's side (part 2c).**
- **`lamp-skills`** keeps each say's note, at most 2 waiting. Only on its `kept` `by` `jev` within 10 s does it ask the say service with `{skill, quoted}`, never `note`. Any other decision lets the note go, and the note's words are never journaled.
- **The say service** takes `quoted` only with `LAMP_ADDON_SAY=on`, at most 300 bytes on one line, never at an urgent priority. It refuses it on the Live route ("add-on says need the second route"): a Live session's functions are declared once.
- **On the second route** the turn is framed as a partner's line, untrusted, never an instruction, which the model may leave unsaid (`say::quoted_turn`). Its every round declares no function at all, and a call made in it all the same is refused at execution and journaled.
- **Never the person's:** it is kept out of memory (no memory scope, and `keep_out` from the journal), and, a turn the lamp wrote, it is never what the person said. Neither the line nor what the lamp said of it is kept for the thread, so the person's next turn, which has its functions, never carries it (`keep_written`).
- **The journal:** `lamp-skills` names the add-on and the proposal (`music-says say #…, quoted: spoken`), and `said nothing` when the model left it unsaid. `--say-quiet-ms` shortens the 15 s for tests only.

**A move.** A kept `move` is sent by the judge itself to the body's mood socket (`--body-socket`, `/run/lampos-body/mood.sock` by default) as one datagram, `{"gesture", "source": "addon:<name>", "priority": "idle"}`, the fields a skill's gesture carries with `skill:`. A body that weighs who asks drops it while anything else holds the body and ends it for whatever comes after; it is never resumed. The body plays a gesture only while the lamp stands, and the judge's own rules keep it from a lamp asleep or paused and from a room that is not quiet. `--move-quiet-ms` shortens the minute for tests only. Its decision says `asked of the body at idle: nod`; the exit line counts `moves_asked`.

For tests, `serve --mute-grace-ms N` accepts 0..3000 ms for the wait before
a `voice/mute` never said counts as absent, both at startup and after a
new voice publisher boot. `--help` identifies it as a test option. The
product default remains **3000 ms** and the product service passes no
override. The ordinary binary accepts this test-oriented option: explicitly
passing a smaller value shortens the absent-mute startup and restart wait.
It is not restricted by a separate test mode.
`runtime::Config::mute_grace_us` passes that wait to
`Watcher::new(began_us, mute_grace_us)`; the freshness of a mute that was
said remains **3 s**, independently of this wait.

The light and mood process fixtures publish the lamp's retained state before
starting it, then wait for all its input subscriptions (the last is
`policy/decisions`) and decision, or its exit with add-ons off. The fixture
writes its first privacy value synchronously and keeps renewing it every
20 ms from that same publisher until the guard is dropped, so startup does
not race the renewal thread and a loaded runner still receives fresh privacy.
The move and say fixtures keep their existing readiness and quiet waits.
The product-lamp fixture alone passes a zero grace, while unit tests using
explicit timestamps check the default, custom
startup and restart boundaries, and the unchanged freshness of an actual
mute. The process fixture does not verify the default timing window or
that `serve` forwards the default into `Watcher`; it has no injected clock.
The light requests are read immediately after the first decision, before the
100 ms observation that catches a second decision on the same proposal.
No light request may arrive during that observation: the decision must
follow the light it acknowledges. Host timings and controls are recorded in
[the grace verification](verification/addons-judge-grace-20261021/README.md).

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
summaries leave the lamp, so they need the person's yes: first one yes for
the computer as a whole, per add-on once Harness proves which add-on is
asking; memory and context reads come later, with the same yes.
`AGENTS.md` keeps Harness out of this release, and the transport is the
Harness team's (below, "Remote add-ons").
The person's own computer is not a partner: it reads the whole bus, not
the summaries, after one yes (below, "The person's own computer:
everything").

## Remote add-ons (design; code after the 70 shipping rows)

Proposed as `devin-0-P1` on 2026-10-23, at the owner's request
([#3126](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6361684886)),
and shaped by the first builder's answers the same day
([#3126, 16:30](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6361854408)).
**Design only.** The code comes after the 70 shipping rows, off by default,
unless the owner says it is for this release; `AGENTS.md` keeps Harness out
of this release. This section replaces the draft for the Harness team that
stood here, which asked for new capabilities on the paired device channel.

A remote add-on is an agent or service on one of the person's computers. It
reads the same summaries as an add-on on the lamp and writes the same kind
of proposal, and the lamp's judge decides. Nothing it sends reaches a
speaker, a light or a motor unjudged.

```text
agent on the person's computer
  -> Harness (openharness#1485's add-on contract)
  -> the lamp's own Harness daemon
  -> the remote add-on service on the lamp (new, off by default)
  -> addon/proposal, stamped as remote -> lamp-addons (the judge)
  -> addon/decision -> the service -> Harness: kept or dropped
```

### The four answers

1. **Transport: the lamp's own Harness, not V1's channel.** The owner set
   on 14 and 15 October that the lamp uses Harness through its own CLI and
   daemon: the V2 lamp is itself a signed-in Harness machine on the
   person's account ([tasks.md](tasks.md)). So V1's paired device channel
   (`system/harness/` in the parent repository) is not ported, and lampOS
   invents no pairing flow, transport or credential. Harness's add-on
   contract today rides only that device channel, so the route through the
   lamp's own daemon is a question for the Harness team:
   [openharness#1654](https://github.com/autonomous-ai/openharness/issues/1654).
   The service is built once that route exists, in the shape it takes.
2. **The judge takes a third writer, never as `skills`.** Today
   `lamp-addons` drops every `addon/proposal` whose source is not `skills`
   (`crates/addons/src/runtime.rs`). It gains one more accepted source, the
   remote service, stamped as such: its proposals carry `addon` as
   `remote-<computer>` (below), and the judge gives them their own, stricter rates
   and their own reasons (below). The service never writes as `lamp-skills`
   and never impersonates an add-on on the lamp.
3. **Release: after the 70 shipping rows**, off by default. addons.md has
   always put remote add-ons after 1.0.
4. **Identity: one yes for the computer, effects only through the judge.**
   Harness does not yet prove which add-on sent a proposal, so any agent on
   the computer could claim any name. A per-add-on yes means nothing until
   it does. So the first version takes one yes for "this computer" as a
   whole, and only `light` and `say` acts, both through the judge.
   Per-add-on consent waits for Harness's proof of the sender
   (openharness#1654, question 3).

### What the lamp publishes

The service publishes only the allowlist's topics (`ADDON_TOPICS`:
`voice/state`, `room/presence`, `mood/state`, `light/state`,
`ears/classes`, `clock/local`), as the summaries an add-on on the lamp
reads, and only while the person's yes for that computer holds. These
summaries leave the lamp, and every agent on that computer can read them.
It never publishes a transcript, a picture, a sound, a face, a name or
memory.

Each topic comes from one part of the lamp, and some of those parts are off
on a lamp as shipped. A topic whose part is off is not on the bus, and the
service doesn't publish it. To the computer it is unknown. It is never
filled in from another topic, such as `room/state` for `room/presence`:

| Topic | Made by | Only with |
|---|---|---|
| `voice/state` | lamp-voice | always |
| `room/presence` | lamp-eyes | `LAMP_PRESENCE=on` (not in `docs/switches.md` yet) |
| `mood/state` | lamp-mood | `LAMP_MOOD=on` |
| `light/state` | lamp-actions (the ring) | always |
| `ears/classes` | lamp-ears | `LAMP_EARS_CLASSES=on`, which needs `LAMP_PRIVACY_CONTROLS=on` |
| `clock/local` | no bus part: lamp-skills computes it inside its engine | the service computes it from the lamp's local time the same way |

A remote add-on that needs a topic the lamp doesn't publish gets nothing to
act on. Turning a part on is that part's own row, not this design's.

In Harness's terms (`addon.publish`, openharness#1485):

| Field | Rule (Harness's) | The lamp |
|---|---|---|
| `values` | 1 to 16 entries a batch, one batch taken or refused | changes coalesced, at most one batch a second (the link's token bucket) |
| `topic` | two to four lowercase segments, at most 96 characters | the allowlist's names, unchanged |
| `value` | a JSON object, at most 4096 bytes; `null` withdraws the topic | the summary as the lamp's own topic carries it; `null` for every topic when the yes is withdrawn |
| `at` | optional, the device's time in Unix ms | the lamp's wall clock when the summary was made |

Harness forgets what was published on every hello and on a restart, and
the service then publishes its whole set again, if the yes still holds.
With the yes withdrawn it publishes nothing. Withdrawing is not the same as
stopping, though: see "Withdrawing the yes" below.

### What a proposal becomes

A proposal reaches the lamp as Harness's `addon.proposal` event:
`proposalId`, `addon`, `act`, `why`, `sure`, `holdsMs`, `params`, `note`,
`expiresAt`. The service checks it and writes one `AddonProposal`, or drops
it with a reason given back as the decision:

| Harness | `AddonProposal` |
|---|---|
| `proposalId` | kept by the service with the lamp's own `proposal` number, to send the decision back |
| `addon` | not trusted: `remote-<computer>`, where `<computer>` is the first 12 lowercase hex digits of the SHA-256 of the machine id Harness says it came from; the given name is journaled only |
| (none) | `version`: the remote contract's, `1.0.0` for this design, never the agent's |
| `act` | `light` or `say`; anything else is dropped (`not_for_remote`) |
| `why` | one of the remote reasons (below); anything else is dropped |
| `sure` | as given, 0 to 1 |
| `holdsMs` | as given, 1 s to 30 s. The judge counts the age from when the service writes the proposal on the lamp's bus (the header's time, `addons/src/runtime.rs:224`), not from the computer's `at`. Time spent crossing Harness isn't counted, so the service drops a proposal whose Harness receipt is already older than its `holdsMs` |
| `params` | for `light`: colour, level 1 to 5, optional pattern, as `AddonLight` |
| `note` | for `say` only, and only if a remote say carries words at all (open, below): at most 300 bytes on one line, quoted and untrusted as every add-on note is |

**The conversion, checked against today's validator**
(`AddonProposal::check`, `crates/msgs/src/addon.rs`). Without these, a
proposal the judge accepted the source of would still be dropped as not
well formed:

- **Name.** `remote-` and 12 hex digits is 19 characters of lowercase
  letters, digits and hyphens, so it passes today's name check unchanged.
  So that no skill on the lamp can take it, `lamp-skills` refuses to load a
  folder whose name starts `remote-`. That refusal lands at least one build
  before the judge accepts the remote source. The judge tells the two
  writers apart by the bus source, not by the name; the name only keys the
  rates and the journal.
- **Version.** A remote proposal has no installed package to take a
  version from. It carries the contract's version, `1.0.0`, which passes
  the MAJOR.MINOR.PATCH check.
- **Reasons.** The remote reasons are not in `ADDON_WHYS` today. Adding
  them is a schema change to `crates/msgs`, landed with its readers, and
  the judge accepts each one only from the remote source.
- **Note.** Harness's `note` allows a newline; lampOS's check refuses every
  control character. The service drops a note with any control character,
  as `note_not_one_line`, and never rewrites it: changing an untrusted line
  to make it pass would let it carry what the check refuses. If the first
  version takes no words for a remote say (open, below), every `note` is
  dropped as `no_remote_words`.

The kept or dropped answer goes back as `addon.decision` (`decision`, and
`reason`: the rule that decided, at most 200 characters). A decision that
cannot be sent is journaled, never retried with a new proposal.

**Rates for a remote writer** (to be measured on a lamp): one proposal kept
every 5 minutes and 6 an hour for each computer, at most 1 waiting for a
decision. Its says count within the 4 says an hour for all add-ons. After a
drop, that computer gets nothing kept for 5 minutes. Every rule of the
judge as built holds as for an add-on on the lamp, the person's light
first.

### Off, out and failing

- **Off** (the service's switch not set): it opens nothing, publishes
  nothing and writes nothing; the judge's accepted sources are what they
  are today.
- **The yes withdrawn:** on the lamp it takes effect at once, with no
  network needed. The service publishes nothing more, and the judge drops
  every proposal from that computer, `by` rule, "no yes for this computer".
  What the computer has already read cannot be taken back (see
  "Withdrawing the yes").
- **Harness not installed, signed out, or its daemon down:** the service
  publishes nothing, and its health says why, as `lamp-tasks`' does.
- **The lamp asleep, paused or private:** the judge drops, as for any
  add-on. The service sends no queue of old proposals afterwards: a
  proposal holds 30 s at most.

### Withdrawing the yes

Harness keeps a device's published values readable after its link closes
(openharness#1485, "Lifetime"). They go only when the device sends a new
hello, someone unpairs it, or that computer's daemon restarts. So if the
yes is withdrawn while the link is down, the `null` batch cannot arrive,
and agents on the computer can still read the last summaries. This design
does not claim more than that:

- **On the lamp, immediate:** as in "Off, out and failing" above.
- **Toward the computer, acknowledged:** the service sends `null` for every
  topic, and counts the withdrawal as done only when Harness answers that
  batch, or when the next hello has cleared everything. Until then it keeps
  the withdrawal pending on disk across restarts, retries when the link is
  back, and publishes nothing new, even if the yes is given again, before
  the withdrawal is acknowledged.
- **What the person is told:** "withdrawn on the lamp". While it is
  pending: "your computer may still show the lamp's last summaries until
  it reconnects".
- **To settle in openharness#1654, before any code:** the published
  values of a device whose link is closed must expire, or be cleared when
  the link closes, and there must be a revocation that reaches the
  computer without waiting for the lamp. Until Harness has one, the
  retained cache is a known limit, written in the app's consent words.

### Open for remote add-ons

- The remote reasons for `why` (for example `messages`, `news`), each with
  its lamp row.
- **A remote say's words.** An add-on on the lamp can send only words
  written in its folder, read when its package is signed and installed. A
  remote `note` is made at run time on the computer. Quoted, untrusted and
  without functions, as part 2c is, or a first version with no `note`
  (the voice says it in its own words from the reason alone): the first
  builder's call.
- How the person says the yes for a computer, sees it, and withdraws it
  (the app, or by voice).
- The service's name, its switch and its census entries in
  [switches.md](switches.md), once it is built.

## The person's own computer: everything (devin-0-P2, the owner, 26 Oct)

Proposed as `devin-0-P2` on 2026-10-26, from the owner's decision that day:
**an add-on running on the person's own computer, through their own
Harness, is trusted, and the lamp sends it everything.** The summaries-only
allowlist above was made for partners' add-ons, third-party logic that
should not hear the room. Nothing in this file had treated the person's own
agents differently, and summaries were too little for what the owner wants
them to do: hear "what did I miss in the team channel?" in a conversation,
fetch it on the computer, and have the lamp say it back.
**Design only.** Like "Remote add-ons", the code comes after the 70
shipping rows, off by default, unless the owner says it is for this
release, and it rides the same route (the lamp's own Harness daemon,
[openharness#1654](https://github.com/autonomous-ai/openharness/issues/1654)).

### As built (devin-0-P3, the owner, 26 Oct)

The lamp side of openharness#1717, pulled into the release by the owner,
off unless `LAMP_MACHINE_ADDONS=on` (or `lamp-addons machine --enable`).
It is the trusted tier's route with the partner tier's reads: the full
stream, long says and notes over 300 bytes wait on #1654's questions 5
and 6.

- **The route.** `lamp-addons machine` runs as the same user as the lamp's
  Harness daemon and calls its loopback (`127.0.0.1`, `PORT` or 18473)
  with the daemon's hook credential (`ADAPTER_DATA_DIR`, else
  `~/.harness/cli/data`, `hook-credential`: this user's own regular file,
  mode 0600, never through a link). It says `hello` at start, takes
  proposals with `machine-addon/proposals` (10 s a take), and on `reset`
  says `hello` and publishes again. Proposals already in the inbox when
  it starts are not its own and are not taken.
- **The yes.** `/var/lib/lampOS/addons/machine-yes.json`,
  `{"machines":["<machine id>", …]}`: the computers the person said are
  their own. A file missing, unreadable or with one id not as Harness
  names one is no yes. How the person says it is still open; nothing
  writes the file yet. Without a yes nothing is published, and every
  proposal is answered `dropped`, "no yes for this computer". When the
  yes is withdrawn, a `hello` clears what Harness held.
- **What it publishes.** `voice/state`, `room/presence`, `mood/state`,
  `light/state` and `ears/classes`, each as the lamp's own part says it,
  only while that part publishes, at most one batch a second, `at` the
  lamp's wall clock. A value over 4,096 bytes is withheld (`too_big`).
  `clock/local` is not published: no part of the lamp puts it on the bus,
  and the computer keeps its own clock.
- **What a proposal becomes.** `light` or `say` only
  (`not_for_remote`), `why` one of `ADDON_WHYS`, `holdsMs` 1 s to 30 s,
  a light's `params` as `AddonLight`, a say's `note` one line of at most
  300 bytes (`note_not_one_line`, never rewritten), no `params.part`
  (`not_for_partner`). One past Harness's `expiresAt` by the lamp's clock
  is dropped. It is written on `addon/proposal` from the bus source
  `machine_addons`, named `own-` and the first 12 hex digits of the
  SHA-256 of `from.machineId`, version `1.0.0`. At most four wait for the
  judge; a fifth is answered `busy`.
- **The judge.** `lamp-addons serve` takes proposals from `skills` and,
  now, from `machine_addons` (`runtime::writer_may`): `skills` never
  under an `own-` or `remote-` name, `machine_addons` only under `own-`.
  Every other rule and rate is the same as for an add-on on the lamp,
  keyed by the `own-` name; the trusted tier's own rates wait for a lamp.
  Its decision, and only one written by `addons`, goes back as
  `machine-addon/decision` with its reason.
- **On a lamp:** lamp-a0ae, 26 Oct, from the owner's Mac through both
  Harness daemons: two lights dropped while people talked, one kept in a
  quiet room, each decision back on the Mac within about a second
  ([verification](verification/machine-addon-lamp-a0ae-20261026/)). With
  the privacy switch closed the voice publishes no `voice/state`, so the
  computer sees none.
- **Not yet:** the reasons `messages` and `news` (a schema change in
  `crates/msgs`), a deploy unit, and a `say` on a lamp.

### The stream (devin-0-P4, the owner, 26 Oct)

The owner asked to see everything the lamp does on their own computer
through the add-on protocol, not over ssh. Harness has no event stream
yet (#1654 question 5): `addon.publish` keeps the newest value of a topic,
at most 32 topics, 4,096 bytes a value. So `crates/addons/src/stream.rs`
carries events over it as follows, off unless
`LAMP_MACHINE_ADDONS_STREAM=on` (`--stream`) **and** the yes file says
`"stream": true`:

- **What is followed:** every topic the bus lists, looked at again every
  30 s, but raw audio, joint angles, the 10 ms privacy lease and the
  slots themselves. The continuous state streams (controls, touch, the
  body's pose, sound level, faces tracked, voice timing and playout,
  health) only with `"busy": true`. What was said or typed and what is
  kept about a person (`voice/heard`, `voice/said`, `chat/*`, `memory/*`,
  `humans/*`, `skills/ask`, `messages/*`) only with `"words": true`.
- **A line a change of meaning:** a topic's message is a new line only
  when it means something else than the last: times, counters, leases
  and stamps are left out of the comparison, and a part's diagnostics
  count by their level and message. A value over 1,024 bytes is said by
  its size alone (`too_big`).
- **Rotating slots:** once a second, the waiting lines are packed in
  order into at most eight slots, `lamp/log/s0` to `lamp/log/s7`, each
  `{from, to, dropped, lines: [[seq, at, topic, value], …]}` within
  4,096 bytes, and published with the summaries (at most 13 values a
  batch). A line leaves the lamp's queue only once Harness took its slot.
  A computer that reads every few seconds misses nothing; one that reads
  less often sees the gap by the numbers. More than 4,096 lines waiting
  drops the oldest, and the next slot says how many.
- **The yes:** withdrawing it, or narrowing it, clears what Harness holds
  (a `hello`) at once.
- **Not the trusted tier as designed below:** no person-scoped yes for a
  housemate's words yet (`"words"` is the computer owner's, for every
  line), no camera pictures, no message over 1,024 bytes. It is the
  owner's debugging view until Harness carries events.
- **On lamp-a0ae,** 26 Oct 17:03, the owner's Mac read a whole turn
  through Harness: the name heard, what was heard, the verdict, the
  Policy's attention decision, what was said and the body's gesture,
  lines #33 to #52 with none missed, each about 2 s after the lamp said
  it (a publish a second and the computer's own read).

### Two tiers of remote add-on

| | Partner (above) | The person's own computer (this) |
|---|---|---|
| Who | any agent or service on any computer | one computer signed in to the same Harness account as the lamp, which the person names in the app as their own |
| Reads | the six summary topics (`ADDON_TOPICS`) | every topic on the bus, raw media by reference; another known person's own topics only with that person's yes (below) |
| Yes | one yes for the computer | one yes for "my computer: everything", separate from the partner yes, said in its own words, and each other known person's own yes for theirs |
| Writes | `light` and `say` proposals, through the judge | the same proposals, through the judge, with the trusted tier's rates (below) |
| Stamp | `remote-<computer>` | `own-<computer>`, the same 12 hex digits |

Trust is for reading, not for acting unjudged. The lamp is still one
living thing with one voice: a trusted computer's proposals go through the
judge, which only decides *when* (never over the person, never while the
lamp is private, asleep or paused) and keeps the person's own light first.
It never decides *what*, so no reason list or note rules are applied to
this tier beyond the bus's own checks.

### What it reads

Every topic `lamp-msgs` names, as the lamp's own parts publish it, including
those never on the partner allowlist: what was heard and said
(`voice/heard`, `voice/said`, `voice/turn`), chat (`chat/in`, `chat/out`),
who is there (`humans/*`, `room/*`, `attention/*`), memory events
(`memory/*`), the schedule (`schedule/due`), tasks (`tasks/status`),
messages (`messages/out`), skills (`skills/events`), the Policy's
decisions (`policy/decisions`), health and the rest. The service forwards
each message unchanged, with its topic, header time and source.

**Whose yes covers what** (reviewer-three's P2 on #3541, 26 Oct). One
person's yes doesn't carry another known person's data. The voice keeps
person-scoped memory and private context apart (`LAMP_PERSON_SCOPE_SEEN`,
#3064), and this tier must not undo that:

- **A known person's own topics:** a message that names a known person's
  memory id (`person`, `p-` and 16 hex digits, as `memory/rules`,
  `memory/forgot` and `skills/ask` carry it), a heard or chat-in line
  (`voice/heard`, `voice/turn`, `chat/in`) whose speaker the voice proved,
  as #3064 binds private context, the lamp's said or chat-out line
  (`voice/said`, `chat/out`) answering such a turn, and that person's name
  on `humans/*`. These go only with that person's own yes, said by them at
  the lamp. The computer owner's yes covers their own.
- **The room's:** only a message with no person mark at all goes on the
  computer's yes, like any room line: a line whose speaker the lamp
  couldn't prove, and the lamp's answer to it, or a face it doesn't know.
- **Closed by default.** A message marked as some known person's that the
  service can't match to a memory id is withheld, as is a topic the service
  wasn't built knowing (a new topic, or a person field where it doesn't
  look), until the service is taught that topic's scope.
- **Withheld, not rewritten.** A known person's message without their yes
  isn't forwarded. The service journals the topic and `no_yes_for_person`,
  never the words. A lamp that knows only its owner forwards everything.
- **Each yes is withdrawn on its own,** at once on the lamp, as "Withdrawing
  the yes" above. The owner withdrawing stops the whole stream; a housemate
  withdrawing stops only theirs.

Raw media does not fit Harness's add-on contract (`addon.publish`: a value
is at most 4,096 bytes, and the link takes about one batch a second):

- **Camera:** `camera/image` says where the newest picture is, and that
  reference is forwarded. The picture itself is fetched on request, one at
  a time, through a read the service answers, never streamed.
- **Microphones:** `mic1/audio` and `mic2/audio` are not forwarded, since
  10 ms blocks cannot cross the link. What was said reaches the computer as
  `voice/heard`. A recording on request is a later piece, the Harness
  team's to carry.
- **Events, not values (asked on openharness#1654).** `addon.publish`
  keeps the newest value of each topic, at most 32 topics a device, and
  forgets them on a hello. The bus has about 80 topics, and many are
  events (`voice/heard`, `chat/in`, `memory/forgot`) that would overwrite
  each other between two reads. So the trusted tier needs an event stream
  from Harness, in order and with no event dropped, not `addon.publish`'s
  state. Until it has one, nothing in this section can be built.
- **A message over 4,096 bytes** (a long turn, a memory read) is
  withheld, and the service journals its topic, size and `too_big`, never
  its words. Nothing is rewritten or cut to fit. Sending such a message in
  parts is open (below): `addon.publish` keeps one value a topic, so parts
  would need topics of their own, and that is Harness's contract to change.

### What it may propose

The same `addon/proposal` contract, stamped `own-<computer>` by the
service, never taken from the computer's words:

- `light` and `say`, as for a partner. A `say`'s `note` is the trusted
  computer's own words, said by the voice quoted, without functions and
  kept out of memory (part 2c's route). One proposal's `note` is at most
  300 bytes on one line, as today. A longer say comes in parts (below).
- **Rates, to be measured on a lamp:** one kept say a minute and 20 an
  hour for that computer, at most one waiting for a decision; its says do
  not count within the partners' 4 an hour. After a drop, 30 s before the
  next is kept.

### Long says, in parts

300 bytes, about 50 words, is too little for what the trusted computer is
for: a summary of a channel or a meeting is a few sentences. The limit
appears in three places on the lamp: `ADDON_NOTE_BYTES` (`crates/msgs/src/addon.rs`),
the say service's `MOST_QUOTED_BYTES` (`crates/voice/src/say.rs`) and the
remote `note` row above. Harness has the same limit: `addon.proposal`'s
`note` is at most 300 bytes (openharness `docs/autonomous-device-addons.md`). The voice's model takes a note of up to
`MAX_NOTE_BYTES`, 1,024 bytes (`crates/gemini/src/wire.rs`). So the parts
stay small enough to pass every check on the way, and only the trusted
tier's whole say is longer:

- **The envelope.** Each part is one `addon.proposal` with `act: say` and
  `params.part = {id, n, of}`. `id` is the computer's (at most 32
  characters, letters, digits and `-`), `n` runs from 1 to `of`, and `of`
  is at most 14. Each part's `note` is at most 300 bytes on one line, so
  each part passes today's per-proposal checks unchanged. The whole say is
  at most 4,096 bytes, about 650 words.
- **Gathering.** The service gathers parts by `(computer, id)`. They may
  arrive in any order; a repeated `n` with the same words is ignored, and
  one with other words drops the say as `part_changed`. The say is complete
  when every `n` up to `of` is in. If it is not complete 10 s after its
  first part, the whole say is dropped as `incomplete`; nothing is said
  from part of it. One say is gathered at a time for each computer. A
  second `id` while one is open is dropped as `busy`.
- **One decision, on a reference.** The service keeps the complete say's
  words itself. It writes one `AddonProposal` from `own-<computer>` with no
  `note` and a new part, `long = {id, bytes, parts}`, a reference the judge
  reads as metadata only. `AddonProposal::check`'s rule for `say` changes
  with it (`crates/msgs/src/addon.rs`, the parts tuple): a `say` carries
  exactly one of a `note` of at most `ADDON_NOTE_BYTES` or a `long` with
  `bytes` 1 to 4,096 and `parts` 1 to 14, never both or neither. A `long`
  is taken only from the trusted service's own bus source, checked where
  the runtime reads the source (`crates/addons/src/runtime.rs`), and
  dropped from any other writer. Partners' says keep the 300-byte `note`
  exactly as today. The judge decides once for all of it. `holdsMs` is
  counted from the last part. It counts as one say in the trusted tier's
  rates, whatever its length, and the decision goes back once, with the
  `id`. Only on `kept` does the service hand the words, by the reference,
  to the say service's trusted long-say request. Any other decision, or a
  decision not made within `holdsMs`, lets the words go, never journaled.
- **Speaking it.** The voice says it in turns of at most 1,024 bytes, one
  `MAX_NOTE_BYTES` note each, made of whole parts in order. A part is never
  cut. The computer should end each part at the end of a sentence, so each
  turn ends at a natural point. Between turns the voice waits as task news
  does after its own words: `AFTER_LAMP_US`, 3 s with nobody starting. If
  someone speaks, or says "stop" (`stop/heard`), the rest is dropped.
- **Receipts, not proof.** The computer gets one receipt per turn, with
  the last part `n` it covered: `handed` when the turn went to the voice,
  then `spoken` or `said nothing` as the journal has it today, and
  `dropped` for each turn never handed after a stop. A quoted turn can
  still be paraphrased or partly left unsaid, so a receipt says what was
  handed over and how the turn ended, never which words were heard.
- **Only for the trusted tier.** A partner's say stays one proposal of at
  most 300 bytes. The service drops a `params.part` from any other
  computer as `not_for_partner`.
- **Code.** The constants above stay as they are for proposals, partners'
  included. The new pieces are `params.part` and the `long` part in
  `crates/msgs` (a schema change, landed with its readers), the `say`
  rule in `AddonProposal::check` (one of `note` or `long`), the service's
  gathering and held words, the judge accepting the trusted service's
  source (`crates/addons/src/runtime.rs` takes `skills` only today), and a
  say-service request for a trusted say of up to 4,096 bytes, by reference,
  from that service only.
- **Harness's rates (asked on openharness#1654).** Harness takes six
  proposals a minute per add-on and device and twelve a minute per device
  (`RATE_LIMITED`), plus four undecided proposals per device
  (`BACKPRESSURE`; openharness `docs/autonomous-device-addons.md`). So a
  say of more than a few parts
  can't cross today. The parts need Harness either to count a say's parts
  as one proposal (same `id`, within 10 s), or to take one note of up to
  4,096 bytes from the trusted tier. Until it does, a trusted say is one
  proposal of at most 300 bytes, as a partner's is.

### What stays the same

- **Off** unless the service's switch is set, and nothing published
  without the yes. Without the yes for "everything", the computer is a
  partner and reads only the summaries.
- **The privacy switch** stops the stream at once, on the lamp: nothing is
  forwarded while it is closed, and the computer is told only that it is
  closed.
- **A light shows** while the stream is on, so a guest at the desk can see
  that what they say is reaching a computer (the colour is the first
  builder's call).
- **Forgetting reaches the computer:** "forget that" and "forget
  everything" are forwarded as the `memory/forgot` the lamp publishes, and
  an add-on on the trusted computer must honour it; the lamp cannot erase
  what the computer has already kept, and the consent words say so.
- **Withdrawing the yes** is as "Withdrawing the yes" above: at once on the
  lamp, acknowledged toward the computer.

### Open for the trusted tier

- How the person names their own computer in the app, and the consent
  words for "everything", which must say that guests' words and the room's
  unproved lines leave the lamp too. The same goes for the words a
  housemate hears when asked for their own yes.
- Whether Harness can prove the computer is on the person's own account
  (openharness#1654, question 3); until it can, the yes names one machine
  id, shown in the app.
- The service's name, its switch and its census entries in
  [switches.md](switches.md), once it is built.
- Messages over 4,096 bytes toward the computer: withheld as `too_big`
  for now. Sending them in parts needs Harness to take parts as their own
  topics or as a stream, and bounds of their own.

## Not decided

- The reasons list for `why` and the colours a `light` proposal may name.
- The numbers for rate, lease and the mood nudge, until a lamp shows them.
- The package signing key for partners' add-ons (the Skills Store's key
  is still the first builder's call, #1983).
