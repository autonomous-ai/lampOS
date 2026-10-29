# Always alive: the idle layer, sleep and waking

Task 36 (#714), the first piece of [creature.md](creature.md)'s order of
building that the research ranks first: an idle layer that never stops,
with everything else laid over it, a sleep that noise does not break, and
a small stretch on waking. It is part of Expression
([expression.md](expression.md)) and off unless a catalogue has an
`alive` part: [`crates/expression/alive-behaviours.json`](../crates/expression/alive-behaviours.json)
is lamp-4ace's catalogue with one. Everything here was built and measured
on a laptop (Apple M2 Max, macOS); nothing was run on a lamp, and no
number here is a lamp's.

## What exists and what we take

**The research** ([creature.md](creature.md), "What the best companions
teach"): an always-on idle layer of breathing, a little drift and blinks,
with other moves blended in over 0.1 to 0.35 s (Disney); a visible sleep
that noise does not break, only the name, a touch or a reminder (Vector,
Astro); mood as the style of the same move, never as added randomness
(Anki, Luxo Jr.); and a body that moves plainly and little while someone
works under the light (Apple's ELEGNT).

- Taken: all four, as written below.

**The ring breathes and blinks already.** At rest the ring shows "a slow,
dim, warm breath (5.2 s)" and, with `LAMP_RING_EYE=on`, blinks every 7 to
16 s ([ring-choreography.md](ring-choreography.md)). Its breath is a pure
function of the boot clock: `0.5 - 0.5 cos(2 pi s / 5.2)` with `s` the
seconds of `CLOCK_MONOTONIC` within the hour (`crates/voice/src/ring_face.rs`).

- Taken: the body's breath is the same function of the same clock, so the
  head lifts as the ring brightens, with no message between them. The
  blink stays the ring's.

**What the body does with Expression's head offsets** (read in
`crates/body/src/body.rs`; the body is the fifth builder's and was not
changed):

- At rest, only with the body's `--rest-glance on`: head pitch and roll,
  cut to 50 and 25 counts, then to each joint's reach. lamp-4ace runs
  `--reach 150,15,1,250`: a resting head may lift 50 counts but dip only
  15 (1.3 degrees), and its roll moves 1 count. So at rest the idle layer
  can show itself only by lifting the head.
- The planner asks every 20 ms. Against simulated servos (the body's own
  test harness, with the planner's stream of zero offsets before and after
  as on the lamp, in a scratch test not kept), a resting head given slow
  lifts follows them a count a step with no jumps, and a zero stream writes
  nothing. Its "glance from rest" lead never engages under that stream
  ("Found on the way", below).
- In the other moods: pitch, roll and the base's turn, cut to 60, 45 and
  70 counts, and the body's 20-count rest applies.
- The body has its own idle (`--idle on`, on lamp-4ace): a standing lamp at
  rest shifts by one of a few small moves (the old runtime's 10 s `idle`
  clip, a look up, a dip; a tilt either way only on a head that tilts
  standing, which lamp-4ace's does not) after a random wait of 40 s to 3 min
  (`docs/body.md`, since 2026-10-16). While a body clip plays, Expression's
  offsets are not added, except under a shift at rest: that is one layer
  with the breath and the drift, starting and ending at their offsets, and
  neither a shift nor a drift begins within 30 s of the other.

- Taken: the breath and the drift lift the head only, within what lamp-4ace's
  body takes at rest. Which idle author a lamp keeps is the first builder's
  choice ("What to run on the lamp").

## What it does

| | When | What the body is asked |
|---|---|---|
| Breath | The voice resting, the body free (no hold, no concentration, fresh joints), and nobody working under the light | The head lifts and settles with the ring's breath: 0 to 0.03 rad (about 20 counts, 1.7 degrees) over 5.2 s; while Mood runs, at the lamp's one breath Mood keeps, the ring's too (#1670, [ring-choreography.md](ring-choreography.md#the-lamps-one-breath-1670)) |
| Drift | As the breath | Every 30 to 90 s (not on a beat), the head eases over 3 s to a new small lift (0 to 0.035 rad, about 23 counts) and tilt (up to 0.02 rad either way), and stays there until the next |
| Coming in and going out | When the breath may begin or must stop | Over 0.35 s (the catalogue's `blend_us`); over the 4 s of settling when it falls asleep |
| Still for work | A face of at least 0.04 of the picture (about a fifth of it each way) whose middle has stayed within 0.05 of the picture for 5 s; held 30 s after (a head bent over work is often no face) | Nothing from this layer: the body moves plainly and little |
| Falling asleep | 23:00 to 06:00 local time, the voice resting, the body free, and the eyes reporting nobody for 15 minutes | Over 4 s, everything else gives way to the sleeping pose: the head bowed 0.023 rad (15 counts, all lamp-4ace's down reach allows) |
| Asleep | Until woken | Only a slow breath: the head lifts 0.015 rad (10 counts) over 9 s. No clip begins: a bang, talk in the room, the voice listening are not answered by Expression |
| Stirring | With `LAMP_REST_THROUGH_SOUNDS=on`, asleep for the night: its counted name (#4285, see below) | The head rises 0.04 rad (about 26 counts, 2.3 degrees) from its bow over 0.4 s, eased, from the next tick; held while its answer is awaited, 5 s from the last counted name, then back down over 2 s. Its answer begins a full wake from where the stir holds the head |
| Waking | Its name (with `LAMP_REST_THROUGH_SOUNDS=on`, asleep for the night, a stir instead); a tap, double tap or stroke (an enabled `controls/gesture` that is not a long press); a reminder newly due (`schedule/due`, once each); the voice thinking or speaking (with `LAMP_REST_THROUGH_SOUNDS=on` in `lamp-expression`, asleep for the night, speaking only, #1672: see below); outside the night, the first face seen | A stretch of 2.7 s: the head rises 0.065 rad by 0.9 s, tilts 0.04 and turns 0.03 by 1.5 s (the turn only where the body takes it, outside rest), and settles, while the bow and the rest of the plan cross back over |
| Energy | Always | One number, 0 to 1 (0.5 in the catalogue): the same moves 0.7 to 1.3 times as large and 1.25 to 0.75 times as long. Where Mood runs, its liveliness (`lamp_msgs::mood_liveliness`, -0.5 to 0.5: its breath's pace laid on that range, 0 when calm and awake) is added to the catalogue's, within 0 to 1: livelier with energy, quieter dozing and asleep, exactly where the breath is quicker or slower (#1670). The breath's period is the lamp's one breath, so it stays with the ring |

Woken, it falls asleep again only after nobody again for the whole 15
minutes.

**Its own answer, not its thinking (#1672).** A line's end puts the voice
in thinking while its words are written down, with no words heard too, and
with words not said to the lamp, whose answer is held back. On lamp-06c1
on 28 to 29 Oct that woke the night's sleep three times (00:43:38 and
01:51:37 on lines of no words, 03:35:52 on three words not said to it),
though talk in the room should not wake it. With
`LAMP_REST_THROUGH_SOUNDS=on` in `lamp-expression` (off unless set, the
same switch as `lamp-skills'`, [switches.md](switches.md)), asleep for the
night, the built-in sleep or the rest of the skill that claims it, the
voice thinking does not wake it: the voice speaking, its own answer, does,
as it begins. A touch, a tap on the head and a reminder wake it as
before; manual sleep, another skill's rest and an awake lamp are
unchanged. Off, thinking wakes it, as before.

**Stir at the name, wake on the answer (#4285, the first builder's call,
30 Oct 03:5x).** With the same switch, asleep for the night, its counted
name (heard with the person's speech where `name_requires_speech` asks
it; an `ignored sleeping name` does nothing) no longer wakes it: it
stirs. The head rises 0.04 rad from its bow over 0.4 s, eased, from the
next tick, so it has risen within half a second of the name, as a sleeper
lifts its head at a voice. The stir is held 5 s from the last counted
name (`STIR_HOLD_US`); its answer in that time (the voice speaking)
begins the full wake from where the stir holds the head. With no answer
(a name held back, nobody going on, a TV) it settles back to its bow over
2 s and the night's sleep is unbroken: still asleep, the skill's rest
still held, no wake. A false name from a TV costs a stir, never a wake.
`expression/state` names it `stirring`. The ring is not lifted: the
voice's ring worker owns the ring at night and reads nothing of
Expression's (`expression/light` has no reader), so a lift there would
show nowhere; a ring half is the ring worker's, after a lamp look at the
head's. Tests in `night_sleep.rs`:
`with_the_switch_a_sound_of_no_words_ending_in_thinking_does_not_wake_the_lamp`,
`with_the_switch_a_line_not_said_to_the_lamp_does_not_wake_it`,
`with_the_switch_its_own_answer_wakes_it`,
`with_the_switch_its_name_stirs_it_and_only_the_answer_wakes_it` and
`with_the_switch_a_tap_or_a_reminder_wakes_it`; with thinking waking it,
the first two fail, and with a counted name waking it, the fourth. In
`alive.rs`:
`with_the_switch_a_counted_name_stirs_within_half_a_second_and_does_not_wake`,
`with_the_switch_a_name_with_no_answer_settles_back_and_the_sleep_is_unbroken`,
`with_the_switch_the_answer_after_a_stir_wakes_it_from_the_stir` and
`with_the_switch_an_ignored_sleeping_name_does_not_stir`, each red when
its part is taken out. A long hold of the base is the mute today and will be Sleep when
#711 makes it so; it does not wake the lamp. A face seen at night does not
wake it either: only its name (with the switch, a stir), a touch, a
reminder or its own answer.

With `alive.sleep.name_requires_speech: true` (#749), a sleeping name needs
accepted `voice/activity` person speech within two seconds before or after
the name's source onset. The switch defaults to false; existing catalogues
and wake paths stay unchanged. The onset is publication time minus `ago_ms`,
not receipt time, and must pass the existing context freshness limit (5 s in
the supplied catalogue). Plain Listening, transcript text, room noise and
the lamp's own audio are not speech evidence. Voice supplies an observation
after its echo/attention policy, with an 80 ms heartbeat and 120 ms speech
hold; this is not a raw microphone acquisition timestamp.

The first matching pair requests the existing stretch on the next planner
tick (20 ms target), without a deliberate wait. A name arriving first waits
only until onset + 2 s + the existing Activity delivery limit (300 ms here).
That delivery allowance does not enlarge the two-second source window.
There are at most 16 pending names and 128 recent speech observations;
overflow fails closed, and repeated names do not extend earlier deadlines.
Voice cancellation, owner restart, lost retained input or stale Voice clears
the evidence. A normal Resting-to-Listening turn preserves a pending name.
Uncorroborated names produce a one-shot journal line beginning
`lamp-expression: ignored sleeping name`, with a reason and count (bursts
are coalesced between ticks). This does not change microphone privacy or the
independent touch, reminder, morning-face and Thinking/Speaking wake paths.

## Night sleep is a skill

Since #940 the *when* of the night's sleep is the core skill
[`night-sleep`](../skills/night-sleep/skill.toml), on in every product lamp,
and the *how* stays here: the bow, the breath, the 4 s settle, the bounds,
and every reflex that wakes it. The skill holds today's trigger exactly:
nobody seen for 15 minutes (`present = false`, `for = "15m"`), the voice
resting all those 15 minutes (`[[while]]` on `voice/state`, `for = "15m"`:
the built-in clock began again whenever the voice was not resting, so not
Bedtime's 30 s; with `LAMP_REST_THROUGH_SOUNDS=on`, #1672, the voice
listening or thinking is carried through, `through`, so a sound heard as no
words is slept through, a baby or an alarm with no words too, and only
words heard, its name heard (written down or not), a new push (one
held begins them once, at its start), a touch or
the lamp speaking, `broken_by` and `speaking`, begin the 15 minutes again: on a real lamp small
room sounds at 23:26 to 23:41 kept it awake), awake all those 15 minutes (`[[while]]` on
`expression/state`'s `asleep`, `for = "15m"`: woken, it sleeps again only
after nobody again for as long), and 23:00 to 06:00 as its `[active]`
hours, so someone gone since 22:30 is slept on at 23:00 as before. Its one
step is `do.rest`, renewed every second through the night, to 22:59 at
most. It has no light, sound or words, and its runs ask nobody (the core
grant `sleep`, [skills.md](skills.md)).

Expression holds the claimed skill's rest as it holds its own sleep: the
voice listening (a night sound), a new voice turn, a privacy mute or the
body held do not end it; its name with speech, a touch, a tap on the head, a
reminder, a conversation (with `LAMP_REST_THROUGH_SOUNDS=on`, the lamp's
own answer, not the voice thinking, and its name only a stir), manual sleep or Expression off do. The first face out of the night wakes it, as it wakes the built-in
sleep. It begins on the built-in sleep's own conditions (a free body and a
resting voice; a privacy mute does not keep it awake). Another skill's rest
keeps the generic gate ([expression.md](expression.md#leased-skill-rest-requests)).

**Handed over, never lifted.** The night's sleep passes from one rest to
the next with the head down (#940, the first builder's P2 on #2710). A
newer episode, or a new skills session's first rest, of the skill that
claims the night takes the active rest over as it is: no release, no
`Releasing`, no settle again. Its lease lapsed (lamp-skills restarting, as
an update round does), the sleep is held down for 10 s
(`rest::SLEEP_HOLD_US`) for that taker, and a reflex still wakes it. With
no taker by then, Expression lets the skill's rest go, and the built-in
sleep keeps the night where it is while its own conditions hold (inside
the night, no face in the last look): no lift, and the first face of the
morning wakes it. Outside them the rest is released, the head rising over
the settle. A claim gone stale passes a held rest to the built-in sleep
the same way. Tests in `night_sleep.rs`:
`a_skills_restart_mid_night_hands_the_sleep_over_with_the_head_down`,
`a_newer_episode_takes_the_night_over_with_no_lift`,
`a_dead_claimer_passes_the_night_to_the_fallback_with_no_lift`; with the
handover made a release, the first two fail.

**The claim.** With `LAMP_SKILL_ROUTINES=on`, lamp-skills says
`expression/sleep_claim` (`{"skill":"night-sleep","on":true}`) at each
reload and every 60 s while the folder loads. Turned off by a person (the
`off` file) or disabled, it says `on: false`: a lamp that does not sleep at
night is that person's choice, so Expression does not sleep by itself
either. With `LAMP_REST_REQUESTS=on`, Expression reads it once a second: a
claim from `skills` said under 180 s ago gives the night to the skill.
None, or one older, is the fallback: Expression sleeps by its own schedule
as before, so a crashed lamp-skills or an old skills folder never leaves a
lamp that does not sleep. Nor does a skill that cannot rest (the first
builder's P2 on #2744): lamp-skills says the claim only while its rest
client has a fresh receipt from Expression and night sleep's last rest ask
was not refused for a reason that repeats (`routine::REPEATS`: Expression's
`rest observation is future, stale or expired`, no trusted local clock, no
fresh Expression, no answer in time, a spent bound). A refusal that passes
with the moment (the voice in a turn, a hand, a lean) or an admitted rest
clears it. Otherwise it says nothing, the last claim goes stale within
180 s, and Expression's own sleep takes the night; a person's `on: false`
is still said. lamp-skills' journal says each change once (`night sleep
cannot rest (...)`, `night sleep can rest again`); the test is
`a_rest_always_refused_gives_the_night_to_the_fallback`, which fails with
the claim read from the file alone. The journal says each change once:
`lamp-expression: no skill claims the night's sleep: Expression sleeps by
its own schedule, the fallback kept for one release (#940)`, and
`lamp-expression: night-sleep claims the night's sleep (on): ...`. A
claim that comes back while the fallback sleeps leaves that sleep be; the
first face of the morning or a reflex wakes it. The fallback is marked
`FALLBACK (#940` in `engine.rs` and `runtime.rs`, and goes in the release
after. Both switches are on in the product units, since the rest handover
(above) takes a skills restart or the window's edge with no lift.

**The same sleep.** `crates/skills/tests/it/night_sleep.rs` runs one fixture
night (22:30 to 06:05: a sound, a touch, a reminder, a conversation, its
name, the first face at 06:01) through both routes with today's product
catalogue. Each of the five settles is within one look of the eyes
(600 ms) of the built-in one (four to the tick; after the name, 0.6 s
earlier, as the skill's voice clock begins at the voice's word and the
built-in one at the next look), and the same five wakes come at the same
tick. Nothing a person sees or hears changes: the same bow, breath and
settle, no light and no sound. Known differences, none in the fixture: a
face below Expression's 0.6 confidence counts as someone for the skill; a
rest refused by Expression (the body held) is tried again a minute later;
and a lamp-skills restarted mid-night counts its 15 minutes from its start.
A lamp nobody saw for a whole day keeps its head down through 22:59 and
23:00, as the built-in sleep does: the rest's longest window (23:00 to
22:59) carries a rest still held in its last minute into the next night,
the same episode renewed with no cancel
(`a_lamp_nobody_saw_all_day_keeps_its_head_down_into_the_next_night`).
The night's hours are written twice, the skill's `[active]` and the
catalogue's `alive.sleep.night` (which still times the morning face):
`the_skill_and_the_product_catalogue_name_the_same_night` holds the product
ones equal, and the 15 minutes with them.

Nothing is random. A drift's wait and size come from a fixed hash of how
many drifts came before, so a replay gives the same plan every time. All
of it runs on the planner's injected clock and the inputs it was given.

## The catalogue

The `alive` part of `alive-behaviours.json`, every field checked when the
catalogue is read (`crates/expression/src/alive.rs`):

| Field | lamp-4ace's | Allowed |
|---|---|---|
| `states` | `["resting"]` | 1 to 3 voice states, never speaking: the lamp keeps its head still while it talks |
| `blend_us` | 350,000 | 100,000 to 350,000 |
| `energy` | 0.5 | 0 to 1 |
| `breath.period_us`, `breath.inhale` | 5,200,000; head pitch -0.03 | 2 to 20 s; the head only, at most 0.06 rad, 0.06 rad/s and 0.12 rad/s² at the largest energy |
| `drift.every_us`, `drift.move_us` | 30 to 90 s; 3 s | waits of 5 to 600 s, moves of 1 to 10 s |
| `drift.low`, `drift.high` | pitch -0.035 to 0, roll -0.02 to 0.02 | the head only, zero between them, within 0.06 rad and the same speed and acceleration as the breath at the quickest energy |
| `work.face_area`, `still`, `still_us`, `hold_us` | 0.04, 0.05, 5 s, 30 s | (0, 1], (0, 0.5], 1 to 60 s, at most 600 s |
| `sleep.night` | `[1380, 360]`: 23:00 to 06:00 | minutes of the local day, past midnight allowed |
| `sleep.name_requires_speech` | omitted (`false`) | `true` requires corroborating person speech for a sleeping name; First enables it after the compatibility minute |
| `sleep.nobody_us`, `settle_us` | 15 minutes; 4 s | 1 minute to 2 hours; 1 to 10 s |
| `sleep.pose`, `sleep.breath` | head pitch +0.023; 9 s, -0.015 | the head only, within 0.06 rad, reached slowly |
| `wake.keyframes` | five, over 2.7 s | 1 to 5 s, from and back to nothing, the head and the base only, no light, within the catalogue's limits, at most 0.35 rad/s and 2 rad/s² |

lamp-4ace's own "settle at rest" clip is left out of this catalogue: the
drift does what it did, and two authors of rest would be more movement,
not more life.

## What it reads

| Input | From | For |
|---|---|---|
| `voice/progress` (or `voice/timing`) | The voice | Resting or not; thinking or speaking wakes it |
| `body/joint_states` | The body | Fresh joints, as every request needs |
| `attention/concentration` | Its owner | Explicit concentration stops it all, as before |
| `humans/faces/tracked` | The eyes | Nobody there, someone working close and still, the first face of the morning |
| `name/heard` | The name listener | Waking |
| `voice/activity` | The voice's validated person-speech detector | Corroborates a sleeping name only with `sleep.name_requires_speech: true` |
| `controls/gesture` | Controls | Waking, by an enabled tap or stroke. New to Expression |
| `schedule/due` | Schedule | Waking, once for each notice that comes due. New to Expression |
| `controls/touch`, `controls/touch_state` | `--touch` Controls, only with `serve --head-touch` | The head (#711, below) |
| `voice/mute` | The voice, with `LAMP_HEAD_TOUCH=on` and `--controls-mute`; read only with `serve --head-touch` | Sleep from the head, the voice's mute with the reason `sleep` (#711, below) |
| The local time of day | The live service itself, from the wall clock and `--zone system` (the lamp's own time zone, written at setup) or `--utc-offset-minutes` (420 in Vietnam), at start and every minute | Night. Not a bus topic. `serve` refuses a catalogue with sleep without one. With `--kernel-clock` it is given only while the kernel says the clock is set (`lamp_ipc::clock_synced`, #830): a lamp has no battery clock, and one not set knows no night, so it neither falls asleep nor wakes by a wrong hour; one line says each change. With `--offline-face`, listening is also drawn as not ready (the `unavailable` face) while `lamp/online` says the internet cannot be reached, read once a second (#830) |

`--zone system` (or `--zone America/New_York`) takes the zone's offset at
each moment, so night stays at the same clock hours across a daylight-saving
change (#820); `--utc-offset-minutes` is the same all year, as before. The two
together are refused; an unknown zone ends the start (never a silent UTC).
`--zone system` follows `/etc/localtime`: when setup points it at the home's
zone while Expression runs, the next minute's reading takes the new zone, with
no restart. A change that cannot be read keeps the last zone (`lamp-local-time`,
shared with Memory and the policy).
`lamp-expression import --zone NAME` or `--utc-offset-minutes N` adds the same
time of day to an imported recording, from the recording's own wall clock, so
that a replay can see night.

### The head (#711)

Off unless `lamp-expression serve --head-touch`, and Controls runs with
`--touch`. The owner's design is in [creature.md](creature.md#touch):
- **A hand kept on the head a second** (Controls' head Stroke) is leaned
  into: the head lifts a hair and tilts toward it (`LEAN`: pitch -0.015,
  roll 0.02 rad, authored, to be looked at on a lamp), easing in over 0.5 s.
  It lasts while the head's state says that contact is still there, and eases
  out over 0.6 s once it is gone, or once the state has said nothing for
  0.5 s, so a stopped Controls never leaves it leaning. Only awake, and only
  with the body free.
- **A hand kept three seconds** (Hold) puts it to sleep at once, by day too,
  as the night's own sleep does: the same bow and breath. Never while
  onboarding is under way (#3747): the voice declines that hold, and its
  word that it is awake keeps Expression from folding.
- **A tap**, or a hand kept a second, wakes it when asleep (#3747). Awake, a tap is "are you there?" (First,
  #711, 05:05). It looks up: the head lifts a little (`LOOK_UP`, pitch
  -0.035 rad), and toward a face the eyes saw in the last second, more for
  one high in the picture and less for one low (`LOOK_UP_TOWARD`, 0.025 rad
  at the edge).
  - **Timing:** in over 150 ms, the moment of the voice's "hm?" and the ring's
    listening face; held 2 s, as long as a call waits; held on while someone
    is speaking (the voice says Listening); out over 0.5 s. It follows the
    tap itself. The ring's face follows the hum, so it is shown only when
    the hum plays (17 Oct).
  - **Only:** awake, with the body free, and not while the lamp thinks or
    speaks.
  - **Never while muted for privacy** (First on #711, 17 Oct 10:18). Looking
    up says "I'm listening", which a muted lamp must not seem to do. Such a
    tap neither looks up nor wakes a lamp asleep. A mute that begins while
    it looks up ends the look, eased as usual. Muted means either of these:
    - **The voice's own mute:** `voice/mute` muted, not `sleep`. The
      voice's sleep is not this: a tap still wakes it.
    - **The privacy switch closed:** `voice/privacy` with `physical_muted`.
      Only the Controls privacy route (`LAMP_PRIVACY_CONTROLS=on`, or
      `no-slide`, whose leases say `no_slide` and are never closed) says
      `voice/privacy`, and `voice/mute` leaves the switch out. So with the
      route off, on every lamp today, a closed switch still gets a look-up
      (the reviewer on #1282).
    - **Read without subscribing:** the lease is said every 10 ms, so it is
      read once at each request from the head and at each step of a look.
    - **The last word holds**, however late, until a word replaces it. So a
      sender that stops, or a voice ending or restarting while muted, never
      reads as open. The switch is open only when the last lease said is
      valid and says it is: unknown (the lamp can't listen then either), a
      lease that fails its check or doesn't parse all count as closed. Only
      never said counts as open.
    - **The tap that clears the voice's mute** (the restart guard's,
      `LAMP_RESTORED_MUTE`, `controls.md`) gets its look-up, or wakes the
      lamp, once the voice says it is open, within `TAP_OPENS_US` (1.5 s)
      of the tap (the seventh on #1282). The voice's once-a-second "still
      muted" in between uses nothing up.
    - **Tests:** `alive_engine`'s `muted_for_privacy_a_tap_gets_no_look_up`,
      `muted_for_privacy_a_tap_does_not_wake_it`,
      `the_voices_mute_holds_until_it_says_open` and
      `the_tap_that_clears_a_mute_looks_up_once_it_is_open`. Through the
      bus, `runtime`'s `a_tap_while_the_privacy_switch_is_closed_gets_no_look_up`
      and `the_switch_closed_mid_look_ends_it`, and its reader,
      `a_closed_switch_reads_from_the_last_privacy_lease_said`.
  - **Only the head:** never a turn of the body, so it sends no gaze, which
    would turn the base.
- Nothing else: a request not enabled, or not from the head, does nothing.
  Asleep, only a tap does anything.

**Where the voice says its mute** (`voice/mute`, said every second and kept
three), sleep from the head is the voice's, because it is the voice's
software mute with the reason `sleep`: microphones off (First, #711).
- While the voice says it was put to sleep, the lamp sleeps, by day too, and
  nothing else wakes it: not a tap the voice did not take, its name, a
  touch, a reminder or a face. After a restart of either part it is asleep
  again from the voice's first word.
- The body sleeps with it (#1362): lamp-body, with `LAMP_HEAD_TOUCH=on`,
  reads the same word and folds now, over its stand time, even with
  somebody in front of it ([body.md](body.md), "Asleep"). The sleeping pose
  here is the head's only, 0.023 rad, too small to see; the fold is what a
  person sees. Woken by a tap, the body stands up at once.
- When the voice opens again (a tap on the head, or the button's hold), it
  wakes.
- A hold the voice did not take as sleep (the voice open, or muted for a
  person's privacy) is not a sleep here either.
- When the voice has said nothing for three seconds, the head acts as above.

The eye's countdown between one and three seconds is the voice's ring's.

## Measured

On the M2 Max, Rust 1.96.0.

**Tests through the engine**, `crates/expression/tests/it/alive_engine.rs`
(16) and `alive_replay.rs` (6), with the catalogue above: the breath
equals the ring's function to 1e-9 and only lifts; it is in within 0.35 s
and out within 0.35 s, leaving exactly the listening posture; over 15
minutes, nine or more drifts, each 33 to 93 s from the last, with no tick
asking more than 0.75 milliradians of pitch more than the tick before
(0.4 of roll), also across the hour where the ring's phase jumps; a face
close and still stops the layer, and a moving one does not; asleep after
15 minutes of nobody at 23:30, into the pose in exactly 4 s, under a
milliradian a tick; not asleep by day, with the eyes silent, without the
time of day, or with someone passing every ten minutes; a bang and talk
do not wake it; its name, a tap, a stroke, a new reminder and its own
answer do, a long press and a shadow touch do not, the same reminder only
once, and the first face after 06:00; waking takes 2.7 s with no tick over
10 milliradians and ends at exactly the listening posture; energy 0 and 1
give 0.7 and 1.3 times the breath and waits 1.25 and 0.75 times as long;
and nineteen kinds of catalogue that could twitch or reach too far are
refused. A replay longer than ten minutes is refused without `--long`; an
import takes touch and reminders and is given the time of day; the live
service tells the planner the time every minute.

**A written hour through the built `lamp-expression replay --long`**
(debug build; the voice resting, joints and the room every 100 ms, the
eyes reporting nobody every 600 ms; inputs written, not recorded):

| The hour | Inputs | Planned | Largest change in one tick | Farthest |
|---|---|---|---|---|
| From 10:00 | 114,065, none refused | Breathing in all but the first of 180,011 ticks; 53 drifts (8,003 ticks) | Pitch 0.0022 rad, the layer coming in; roll 0.00036 | Pitch 0.065, roll 0.0195 |
| From 22:45 | 114,065, none refused | Breathing for 15 minutes and 14 drifts; falling asleep at 23:00 (200 ticks), the breath fading over the same 4 s and no drift beginning; asleep for the remaining 45 minutes | Pitch 0.0022 rad, the layer coming in; falling asleep less | Pitch 0.054, roll 0.0195 |

An hour took about 100 s to replay in the debug build.

None of this says how it looks, whether the head moves at all for such
small asks, or how it sounds.

## What to run on the lamp

Smallest first. Nothing below moves the lamp until step 3.

1. **A quiet hour, recorded and replayed** (nothing moves). On lamp-4ace,
   with nobody talking to it:

   ```sh
   lamp-bus record /tmp/quiet-hour voice/progress voice/activity voice/sound \
     body/joint_states humans/faces/tracked room/state light/command \
     attention/concentration name/heard ears/sudden controls/gesture schedule/due \
     --seconds 3600 --max-mb 128
   ```

   (An hour of `body/joint_states` at 12.5 a second is about 45,000
   messages; `--max-mb 128` keeps any one topic's file from stopping at the
   default 64 MiB.)

   then, on a laptop or the lamp:

   ```sh
   lamp-expression import --clock-id "$(cat /proc/sys/kernel/random/boot_id)" \
     --long --utc-offset-minutes 420 /tmp/quiet-hour/*.jsonl > hour.jsonl
   lamp-expression replay --long --enable --clock-id "$(cat /proc/sys/kernel/random/boot_id)" \
     --catalogue alive-behaviours.json --input hour.jsonl > /dev/null
   ```

   The summary (on standard error) says in how many ticks each behaviour
   was planned and how often it began, the largest change in one tick and
   the farthest the body was asked. Expected: no input refused, breathing
   in nearly every tick of the hour, about a drift a minute, the largest
   step under 0.003 rad. Run at night with nobody seen for 15 minutes, it
   should fall asleep. (The import needs the boot of the recording: run it
   before the lamp restarts, or give that boot's id.)
2. **Install the catalogue** (copied to
   `/opt/lampOS/expression/catalogues/alive-lamp-4ace.json`) and add to
   the `lamp-expression serve` command lamp-4ace runs (the unit made from
   `deploy/units/lamp-expression.service`, with `--enable`):
   `--catalogue /opt/lampOS/expression/catalogues/alive-lamp-4ace.json --utc-offset-minutes 420`.
   The body keeps `LAMP_EXPRESSION_BODY=on` and `--rest-glance on` (without
   it a resting lamp takes nothing from Expression). For one idle author,
   the body's `--idle on` should go off.
3. **Watch it at rest**, standing and folded, ten minutes each: does the
   head lift and settle with the ring, or not move, or move in steps? Does
   a drift read as a creature shifting, or as a servo hunting? Is the servo
   heard? Then once at night (or with `sleep.night` set to include now and
   `nobody_us` at one minute): it bows and breathes slowly; say "Lamp" and
   it stretches.

## On lamp-4ace (first builder, 2026-10-14)

Merged at 23:03 (pull request #729). Its CI build of `lamp-expression`
(`321e44e3…`, run 38581248942) ran on lamp-4ace from 23:02 with lamp-4ace's
catalogue as it was, without an `alive` part: a minute, no restart,
Expression `ok`. From 23:07 it runs `deploy/lamp-4ace/expression-alive-lamp-4ace.json`
(this catalogue, copied unchanged) with `--utc-offset-minutes 420`. Against
the catalogue it replaces, the only difference besides `alive` is that
"settle at rest" is gone. The body runs with `--rest-glance on`, so a resting
head takes the breath and the drift. What it looks like, and the first night
asleep, are not yet seen: at 23:08 the lamp was standing for the played
checks of the overnight soak.

## Where it is most likely to be wrong on the lamp

1. **Too small to show, or shown in steps.** The breath asks about 20
   counts of the head at 12 counts a second at most. On lamp-52e6 a
   25-count ask of the head reached 12 to 14 ([body.md](body.md), "How the
   head follows"), and a servo with a proportional gain stops short of a
   small goal and pushes there. If nothing moves, `breath.inhale` can go to
   -0.045 (the rest clip's size); if it moves in steps that read as a servo
   hunting, the breath goes to 0 and the ring breathes alone.
2. **The servo heard.** A head always moving a little at rest is heard in a
   quiet room, and the name listener and the voice listen at rest. Ten
   minutes of the microphone at rest, with and without the layer, through
   `lamp-name check`, would say whether it matters.
3. **Sleep broken by a voice.** Expression holds its own plan, but the
   body's listening pose answers any speech the voice takes as a turn: a
   lamp asleep still looks up when two people talk near it, until the
   voice's busy-room rule gives up. A sleep that holds needs the voice or
   the body to know it is asleep; the inner state (#713) is where that
   belongs.
4. **A bow nobody sees.** 15 counts, all lamp-4ace's down reach allows.
5. **Working light guessed.** A face of 0.04 of the picture and 5 s still
   are guesses; the eyes see little of a head bent over a desk.
6. **The time of day.** From the board's clock and the battery-backed one,
   at UTC+7; a clock not yet set at boot would make a wrong night.

## Not shown

- Anything on a lamp: how any of it looks or sounds, the servo's own
  response to small slow asks, and the ring and the head together.
- A real quiet hour of lamp-4ace: the hours above are written.
- The inner state (#713): energy is a constant here, and the lamp's
  "asleep" is Expression's alone; nothing else on the lamp knows it.
- The full-body stretch: the body's `stretching` clip moves the arm, which
  Expression cannot; waking with it would be the voice's or the body's.
- The ring's look while asleep (the eye, #712, has none yet). Its breath
  should take this catalogue's sleep period when it does.

## Found on the way, not mine

The body's glance from rest (task 32, `--rest-glance on`) is laid "from
where it rested", with a head lifting from rest led 40 counts ahead. Both
need the body to have counted 50 steps at rest, and every request from
Expression resets that count. Expression asks every 20 ms, zero offsets
at rest included, so on a lamp neither ever engages: a glance is followed
as in any other mood. Against simulated servos, with zero requests before
and after the glance as on the lamp, the glance lifted the goal 50 counts,
against 80 to 90 in the body's own test that asks only during the glance;
lamp-4ace lifted 49 at 18:40, the same. lamp-52e6's head needs the lead to
start from below. Said on #714 for the fifth builder.
