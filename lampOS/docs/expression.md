# Expression

`lamp-expression` (`crates/expression`) plans posture, gaze, small gestures
and the light on one injected clock. It opens no hardware. It starts in
**shadow mode**; only `serve --enable` publishes actuator requests. Body has
an opt-in consumer, `LAMP_EXPRESSION_BODY=on`, used by First on lamp-4ace
since 2026-10-14, 10:05. Voice's ring integration and all physical
qualification remain First's work.

Since 2026-10-14 the voice publishes the three inputs it owes
(`voice/timing`, `voice/activity`, `voice/playout`) when its unit has
`LAMP_EXPRESSION_OUT=on`: see "What the voice says, as built" below.
Task 18 adds the separate `voice/progress` and `voice/sound` contract and
the optional [`rest-behaviours.json`](../crates/expression/rest-behaviours.json)
catalogue. The current Voice publisher still uses the original contract;
First must wire the new pair before selecting that catalogue on the lamp.
The separate [`playout-behaviours.json`](../crates/expression/playout-behaviours.json)
catalogue adds bounded recent-level sampling to today's original Voice
protocol. Selecting it does not enable the finishing nod, rest or new protocol.

## What exists and what we take

The [reuse decisions](reuse-decisions.md#expression-authored-timing-and-bounded-requests-2026-10-14)
record the V1 and current Rust paths inspected for the original planner. The
[task 18 addition](reuse-decisions.md#expression-held-turns-and-sparse-rest-2026-10-14)
records the pause, cancellation and idle safeguards reviewed for this change.
V1's movement bounds, playback cancellation and separation of a persistent
light setting from a transient face remain the owners' responsibilities.
Today's body already has calibrated gaze, joint protection and smoothing; Expression must
not replace those with a guessed camera-to-servo conversion.

The external evidence is published research or source inspection, not a
measurement of this lamp:

| Source | What is taken |
|---|---|
| [BML 1.0](https://projects.cs.ru.is/projects/behavior-markup-language/wiki/Wiki) | Preparation, stroke and relaxation on explicit time anchors; a missed gesture does not delay speech; report requested behavior separately from execution |
| [Reachy Mini movement API](https://huggingface.co/docs/reachy_mini/API/reachymini) | Sample an authored move at a fixed cadence, compose small offsets and make cancellation explicit |
| [ROS twist_mux source](https://raw.githubusercontent.com/ros-teleop/twist_mux/rolling/include/twist_mux/topic_handle.hpp) | Actuator-side priority and expiring command authority; an expression lease is not a privacy/safety lock |
| [Apple ELEGNT paper](https://arxiv.org/html/2501.12493v1) | Social expression and functional task motion serve different needs; keep stillness available during work |
| [Murray et al., CoRL](https://proceedings.mlr.press/v164/murray22a.html) | Listener feedback depends on timing. Use observed pauses and sparse gestures; our rule is not their learned policy |

The previous design mentioned Disney timing, doubled conversational stumbles
and specific turn-gap comparisons without sources available in this repo.
Those are not acceptance evidence here. The 100 ms light response and 350 ms
body transition below are engineering targets to measure.

## Ownership and layers

The existing voice coordinates capture, playback and the ring. The body owns
the servo bus, calibrated posture, gaze corrections, clamps, speed, load and
thermal limits. Actions owns deliberate requests and lasting settings.
Expression supplies an optional plan, with deliberate Actions taking priority
for any actuator they need. No model is asked to calculate joint positions.

The planner combines a base posture, authored offset clips, a normalized gaze
request for the body's calibrated solver, and a face/brightness envelope.
Offsets are in radians relative to the owner's calibrated safe posture and
attended heading. They are **not** absolute `JointState` positions, encoder
counts or shares of the body's configured reach. Every sum is clamped before
it leaves the planner; the body still applies its stricter physical bounds.
No autonomous room search or random idle motion is authored. The optional
idle layer, sleep and waking ([alive.md](alive.md)) are small, slow and
not random: a catalogue without an `alive` part plans exactly as before.

`behaviours.json` is the unchanged built-in catalogue: the original 320 ms
nod and no rest clip. `--catalogue FILE` replaces it at startup after
validation. Postures, offset bounds, face levels, freshness, clip weights,
triggers, states, delays, cooldowns and keyframes are data.
A clip using an existing trigger can be added without changing the engine.
A new evidence source requires a new typed input and engine rule. There are
at most 24 behaviors and 64 keyframes per behavior; catalogues are at most
128 KiB. A repeating clip is allowed only on playout evidence: a committed
interval, or the explicitly enabled bounded recent level described below.

A catalogue reads one release forward (#1680): a field this build does not
know, at any depth, is read past, and `lamp-expression` names it once at
start ("fields this build does not read, ignored: speaking.sway_pitch"), so a
roll back after a newer build wrote the catalogue never stops it. A known
field with a bad value, an unknown value of a known field (a new trigger)
or a `version` other than 1 is still refused. A missing field the build
writes only when set (an empty list, `false`, none) is not named. Tests:
`crates/expression/tests/it/forward.rs`.

| Initial behavior | Built-in plan |
|---|---|
| Look away and back | A small thinking glance develops over 250 ms. A committed first-audio schedule causes return over up to 250 ms, reaching the planned speaking posture 40 ms before that schedule. A measured body lag can be supplied; the default is unknown |
| Nod in a pause | Only after true human activity followed by a pause in the same turn. Begin after 120 ms quiet, authored stroke at 150 ms into the 320 ms clip, no more than once per 4 s. Resume speech cancels the nod |
| Follow played sound | Sway and light brightness follow valid playout intervals and their 0..1 level, not transcript timing or cloud packet arrival. A supplied measured body lag advances only the body's sample into the committed schedule; light samples the current interval. No valid interval means no audio modulation |
| Yield when interrupted | Retire the old turn's queued sound. Change the requested face on the next tick and blend the body from its last requested offsets to listening over 350 ms. Listening is shown only if capture can retain input |
| Stay still for work | Explicit concentration holds the body and suppresses expressive movement. A quiet room alone is not evidence that someone is concentrating |

A requested trajectory is not observed motion. The built-in amplitudes and
signs have not been tuned on the lamp. Small offsets can fall below the
body's 20-count movement deadband; a synthetic nod can therefore be invisible.
First should inspect that before increasing any amplitude. Motor speed,
clearance, calibration and acoustic onset remain the hardware owners' checks.

## Optional finishing nod and quiet rest

Select `--catalogue crates/expression/rest-behaviours.json` explicitly for
the task 18 plan. It also selects the new Voice protocol described below;
`serve --enable` remains a separate requirement for actuator requests. No
installed lamp setting or hardware owner is changed by this catalogue.

**A nod as speech ends.** The new clip still needs admitted true human
activity followed by a false edge in the same turn, fresh Listening readiness,
fresh joints and a quiet room. It starts 240 ms into that pause, rises to a
`head_pitch` offset of +0.08 radians at 500 ms into the clip, holds that
offset through 650 ms, and returns to zero at 1.2 seconds. The start-to-start
cooldown is four seconds. The shape changes only head pitch and has no light
modulation.

`Behaviour.continue_in` defaults to `[]`. Its only permitted extension is
`["thinking"]` on a Pause clip whose starting state is Listening. A clip
already begun may finish from its original start time after that transition;
Thinking cannot start a new nod. The old Listening activity heartbeat may
expire while it finishes. Fresh same-turn speech evidence cancels a carried
nod even if Thinking is no longer admitting input; that denial cannot start
a new nod or claim readiness. A held or retired turn, Speaking, concentration,
a busy/unknown room or stale joints/Voice also cancels it. A committed first
audio time attenuates the carried nod with the Thinking return envelope, to
reach the requested speaking posture before that time; a corrected earlier
schedule shortens the return. The planner never delays audio for the nod.
Physical arrival remains unverified. A stalled loop skips a missed stroke
instead of delivering a late full nod.

The +0.08 radians is one additive clip, about 52 requested encoder counts
using Body's conversion. It is not the final requested dip: the Listening or
Thinking base posture, the thinking glance and the planner's sum clamp also
apply. Body then applies its own reach, speed and deadband. First's five
silent trials below inform the longer envelope; they did not test this
combined catalogue or establish a general body lag. `body_lag_us` remains
`null`.

**Small, slow, rare rest.** One four-second head-pitch clip rises to
+0.045 radians over two seconds and returns to zero over two seconds. It
starts only after 45 seconds continuously eligible: fresh Resting Voice with
`accepting_input=true`, fresh joints, a fresh Quiet room, no held turn and no
explicit concentration. Input inhibition cancels it and requires a new dwell.
It then waits at least 90 seconds after completion before another clip.
Normal heartbeats preserve the dwell; lost eligibility cancels the clip and
requires a new continuous dwell. An interrupted rest clip also starts its
cooldown. A late tick starts at most one clip, with no catch-up queue.

Catalogue validation permits at most one Rest clip: only Resting, mandatory
quiet/concentration gates, two to six seconds long, zero offsets at both
ends, at least 20 seconds dwell and 60 seconds cooldown, at most 0.06 radians
on either head axis, and no yaw, arm, light, repeat or held final pose. The
shipped clip uses only pitch. These are bounds on requested offsets, not a
motor-safety or naturalness qualification.

Each smoothstep keyframe segment is also checked analytically against
0.06 radians/second and 0.12 radians/second² on either head axis. A long clip
cannot hide an abrupt short segment. The shipped two-second rise requests
at most 0.03375 radians/second and 0.0675 radians/second² before composition
with other layers; these are calculated trajectory bounds, not measurements.

**Current Body boundary.** Body deliberately ignores Expression offsets
when settled at rest. It also has a separate optional idle author. This task
therefore supplies a rest plan, not resting motion on the lamp. First must
choose one author, decide how an eligible rest request reaches the final
writer, and preserve Body's local safety and privacy rules. See
[What Expression may add](body.md#what-expression-may-add).

## The startle: a glance at a sudden sound

Added by the first builder on 2026-10-14 for the ears ([ears.md](ears.md)).
Since #2822 its move is the Startle skill's own file
([`skills/startle/skill.toml`](../skills/startle/skill.toml), `[startle]`),
not the catalogue's: `lamp-expression serve` reads
`/var/lib/lampOS/skills/startle/skill.toml` once when it starts (or the file
`--startle` names; `replay` reads one only with `--startle`), adds it to the
catalogue as the clip "glance at a sound" (`src/startle.rs`), and the journal
says `the startle from PATH`. Without the file, or with one that fails the
check below, there is no startle until a restart, and the journal says why.
A catalogue's own `sudden` clip (one written before #2822 and kept on a
lamp) is never played: it is dropped as the catalogue is read and named once
in the journal with the fields this build does not read. Expression still
plays the skill's move as its reflex, within its own tick, because a startle
answers within 200 ms or not at all; lamp-skills only checks the file (the
`startle` core grant, [skills.md](skills.md)). A
`SuddenSound` marked short (a knock, a clap, something dropped) starts the
clip once, on the tick it is received, if that tick is inside the clip's
stroke; a second sound inside the clip's cooldown gets nothing. A sound
that went on (a voice, a chair) starts nothing. The catalogue refuses a
`sudden` clip unless it moves the head only, begins and ends at rest, lasts
at most 2 s, is never allowed while speaking (resting, listening or
thinking, since a person thinking still looks up at a bang), does not repeat or
hold, and has a cooldown of at least 5 s. Like the pause nod it needs fresh
joints and an unheld turn. The sound itself is not one of the voice's
transients: it outlives a turn ending, and a glance under way finishes when
the voice's turn changes or someone begins to speak; it stops only if the
lamp begins to speak or is asked to stay still for work. Replaying
lamp-4ace's own inputs from 2026-10-14 16:44 (a sound published by hand
while the lamp was thinking in a busy office) gives the whole glance, from
19 ms after the sound to 1.62 s.

The skill's move is the one lamp-4ace's and the product's catalogues had
until #2822, frame for frame: the head up 0.07 rad in 250 ms, a slight tilt
(0.03 rad) while it holds, back at rest by 1.6 s, then nothing for 30 s,
so a run of knocks gets one glance, as a person glances at the first and
lets the rest go (rule 8). Tests: `tests/it/sudden_engine.rs` (the skill's
file equals the old clip, the product with it glances exactly as before, a
catalogue's own sudden clip is never played, and a file without a bounded
`[startle]` is refused).

## Optional idle layer, sleep and waking

Task 36 (#714), in [alive.md](alive.md): with an `alive` part in the
catalogue ([`alive-behaviours.json`](../crates/expression/alive-behaviours.json),
lamp-4ace's catalogue with one), a resting lamp's head lifts and settles
with the ring's own breath and now and then drifts a little, none of it
while a face stays close and still under the light; late at night with
nobody seen for 15 minutes it bows into a slow breath that no sound or talk
breaks; its name, a touch, a reminder coming due or its own answer wake it
with a 2.7 s stretch. Asleep, no clip begins and the voice's posture gives
way to the sleeping pose. It reads two more topics, `controls/gesture` and
`schedule/due`, and a local time of day the service makes itself from
`--zone system` or `--utc-offset-minutes`, one of which `serve` requires of
a catalogue with sleep (`docs/alive.md`; #820).
With `serve --head-touch` it also reads the head's `controls/touch` and
`controls/touch_state` from `--touch` Controls: a hand kept on the head is
leaned into, a three-second hold puts it to sleep, a tap wakes it
([alive.md](alive.md), "The head"; #711). With it, where the voice says
`voice/mute`, sleep follows the voice's mute with the reason `sleep`.
No drift begins within 30 s of the body seen moving (`BODY_QUIET_US`): a
shift at rest of the body's own (`docs/body.md`) and a drift are one layer
and never land back to back. Any move counts, not only the body's shift: a
gesture, or the look up at a tap falling back, holds a drift off too, so a
drift that waits after a tap is meant. It is seen on `body/joint_states`, which the
service already reads: a joint's goal going 0.04 rad/s or faster for 0.3 s
(the breath and the drift go under 0.02, a shift 0.06 to 0.09; a goal that
leaps once is not a move). The body keeps the same 30 s after a drift.
The breath is the ring's: the boot clock over 5.2 s, and where Mood runs, the
phase of the lamp's one breath in `mood/state`, which the service reads
four times a second from the bus it already has; no switch of its own, and
with no fresh mood nothing changes (#1670,
[ring-choreography.md](ring-choreography.md#the-lamps-one-breath-1670)).

## Optional speaking: the lamp's own speech in its head (#933)

Seen on lamp-4ace on 2026-10-16 at 06:21 and 07:44 (#933): while the lamp
told a story its head and body held one pose, and only the ring changed. The
only movement while speaking was the `follow played sound` clip: a fixed
1.2 s back-and-forth of at most 0.022 rad of head pitch and 0.009 of roll,
scaled by the sound's level, and only in a quiet room. Usually well under a
degree.

A catalogue with a `speaking` section (`crates/expression/src/speaking.rs`;
in `alive-behaviours.json` since #933) adds three things from the lamp's own
sound, as the body meets it: the playout said ahead, sampled at
`body_lag_us`, or, when the voice has not said that far, the farthest piece
it has said that is still to end. The voice says its sound 200 ms ahead
(`FORETELL_US`) and the catalogues' lag is 300 ms, so until 20 Oct no piece
was ever at the lag: the speaking motion took level 0, and a story on
lamp-4ace nodded and swayed never and only settled (#1759; the coordinator
test, which set the lag to 0, hid it). Now a move lands at most 100 ms early,
under one nod's 360 ms. Tests: `speaking_engine.rs`'s
`an_answer_said_200_ms_ahead_nods_and_sways_at_the_catalogues_own_lag` (0
nods without it), and the second route's coordinator test at the
catalogue's own lag. The old playout clip still takes the level at the lag
only. The nods move the head's pitch and the sway turns the base,
within the catalogue's limits, in any room
(it is the lamp's own speech). None runs while the body is held, while
concentrating, asleep, or once someone has cut in, and nothing is owed after.
- **A nod on a stressed word.** A dip of `nod` (0.075 rad of head pitch, about
  4°) over `nod_us` (0.45 s), in and out smoothly. Until 24 Oct it was 0.035
  over 0.36 s. With the body's pose at `--speed 120`, lamp-4ace's head showed
  only its posture moving through an answer; at `--speed 300` (#3385) the nods
  showed as head swings of 3 to 7.6° an answer. The speaking posture now holds
  the head 0.04 up, so the dip comes down from there. Body takes at most 60
  counts (about 5°) of Expression's head pitch before adding mood and gaze, and
  folded the head reaches 15 counts below home, more as it stands. It comes when the level is
  at least `stress_level` (0.35) and `stress_rise` (1.5) times the recent
  level, which follows the sound with a 0.25 s time constant, and at least
  0.15 (about 5 dB) over its own sentence's mean level so far
  (`STRESS_STEP`), past the sentence's first 0.3 s (its onset, not a
  stress). **At most one nod a sentence, and never two within 4 s**
  (`NOD_SPACING_US`, or `nod_gap_us` if longer), each between 70% and 100%
  of `nod`, so no two are alike. People nod on a few stressed words and are
  still in between: lamp-4ace's 150 s story in the second route's
  ElevenLabs voice, loud all the time (a mean of 0.73), asked for 75 nods,
  one every 2 s, before this (the first builder on #2030, 20 Oct 03:22).
  `speaking_engine.rs`'s
  `a_loud_story_nods_a_handful_of_times_not_every_two_seconds` draws a story
  from that voice's measured spread: 12 nods in 150 s, 156 without the
  rule. Each is a catalogue key, so the arm hour can compare two rates from one
  build (the first builder on #2030, 03:32): `sentence_nods` (1 without
  it; 0 is no limit, up to 10), `stress_step` (0.15; 0 to 0.5) and
  `nod_spacing_us` (4 s; up to 10 s). All three at 0 nod as before the rule
  (`the_catalogue_can_turn_the_nod_limits_off_for_a_dense_arm`: 156 nods
  against 12).
- **A sway between sentences.** After `pause_us` (0.35 s) of the lamp silent
  (level under 0.06), as the next sentence begins (level back over 0.12),
  now and then (about two sentences in five) the base turns out from centre
  to one side and back: out over `sway_us` (0.8 s) and back over another,
  eased at both ends, by `sway_yaw` (0.08 rad, about 52 counts, inside the
  body's 70-count cap for Expression), 75% to 100% of it each time. Whether
  a sentence sways, which side and how far are picked afresh each time
  from a count, so a replay is the same; a sentence that begins while a
  turn is still under way does not sway, so the base is at centre between
  sways. An answer that ends mid-sway eases back to centre from there over
  `sway_us`, never further out. (#1067's review: a sway on every sentence,
  always to the other side and first to the same one, read as a
  metronome.) A section still written with #955's `sway_roll` loads, says
  once on the journal that it is ignored, and does not sway. Until #955's review
  it rolled the head 0.025 rad: filmed from the front on lamp-4ace, head roll
  that small did not show, and on lamp-52e6 roll has a reach of 1 count. The
  base turning does. The body adds it to wherever its own gaze has turned
  the base; see `body.md`.
- **A settle as it finishes.** One soft nod (`settle`, 0.03 rad) over
  `settle_us` (0.6 s) when an answer of at least `settle_after_us` (1.5 s)
  ends, then rest.

The levels above (0.35, 0.06 and 0.12) are for a voice whose pieces with
sound average 0.34, as Gemini Live's do in the engine test. Each turn keeps
the voice's own running level: the mean of its first pieces with sound
(level 0.01 or more), then a 2 s follow, never pulled down by a pause. The
three bars scale by that level over 0.34, between 0.25 and 3 times, and
never fall under 0.015, 0.03 and 0.06. The stress bar never rises over
0.9 (or the catalogue's own `stress_level`, if higher): the voice's meter
stops at 1.0, so a voice that fills it would otherwise never nod, where the
fixed bars nodded as each sentence began (Seventh on #1722, 22:25). So a quieter voice, such as the
second route's ElevenLabs (#1712), nods and sways as Live's does, and a
hiss is still no word. `stress_rise` was relative already.

What moved the head with one answer ends at the fences that end it (#955
review): a new session, turn or audio generation, a hold or resume, a
cancellation, an interruption or a fault forgets every nod, sway and settle
owed. Only an answer that ends as it should (Finished, or its turn's own
Speaking-to-rest edge) keeps its settle across that fence. A new turn at once,
someone speaking, cuts even that short.

The behaviour names are `nodding on a stressed word`, `swaying between
sentences` and `settling as it finishes`. The old clip and the speaking pose
are unchanged, and a catalogue without `speaking` plans exactly as before.
The thinking glance (`look away and back`, 0.065 rad of yaw, out in 0.25 s and
held until just before the first word) was already there. It is too short to
show in stills 4.5 s apart.

Tests: `crates/expression/src/speaking.rs` (a stressed word nods and a steady
voice does not; nods keep their gap and differ; over ten sentences, for
ten counts, not every pause sways, both sides and both first sides occur,
each sway is 75% to 100% of `sway_yaw`, eased, without rolling the head,
and the base is at centre before each pause ends and after the answer; an
answer that ends mid-sway eases back to centre, never further out; a
short breath does not sway; a long answer settles and a short one does not; held or cut in on, nothing moves;
poses outside the limits are refused). `crates/expression/tests/it/speaking_engine.rs`
runs an eight-second answer with stressed words every 1.1 s and a half-second
pause every 3 s through the engine, in a quiet and a busy room. It expects
2 or 3 nods (one a sentence), the base turned out by more than 0.055 rad, over 0.02 rad
of pitch range, the settle, and what speaking adds never stepping more than
0.007 rad of pitch or of yaw in a 20 ms tick. Without `speaking`, the
same answer plans none of it and never turns the base, and #955's section
with `sway_roll` loads and plans no sway (`tests/it/program.rs`: the service
starts with it and says so once). It also checks the fences: a new turn, or a
cancelled answer, carries none of the old answer's motion (the offsets equal
a catalogue without speaking on every tick), and a finished answer is
settled after.
The same answer from a voice a third as loud (0.33 times), and a fifth
louder (1.2 times), gives 2 or 3 nods, a sway and the settle alike. On the
old fixed bars the quiet one failed. At 0.01 times, all hiss, it nods and
sways never. A voice that fills the meter (1.0 through every sentence) has no
word standing out of its sentence: it nods at most once a sentence and still
sways.

**Measuring it on a lamp** (`lampOS/tools/`, run from the computer beside
the lamp; each needs sudo on the lamp, its password taken from
`LAMP_SUDO_PASS` on stdin, never on a command line):

- `speaking-ab.sh` says one story request (the Mac's `say`) with Expression's
  catalogue without its speaking section, then with it (`--arms off,on`, or
  one of them), each by a runtime drop-in that is removed on any exit. It
  prints lamp-body's speaking steps, each speaking behaviour's starts (nod,
  sway, settle), the voice's levels against the scaled bars, Expression's
  offsets, and how the voice's turns ended (`ends=`, so a missing settle is
  told from a cut turn). With `--levels-only` Expression runs in shadow
  (its `--enable` left out), so it plans and names each move but nothing
  moves, and lamp-body is not touched: for a night with nobody at the arm.
  Its files stay in `/var/lib/thirteen-ab` on the lamp until removed by hand.
- `sound-shape.sh FILE`, on a `voice/sound` tap the run left there: the
  levels' spread, how many pieces each rise rule would take, the pauses
  under each quiet line, and how far ahead each piece was said (against
  `body_lag_us`: how #1759 found that the head never nodded on a lamp).

Tests: `tools/speaking-ab-test.sh`, `tools/sound-shape-test.sh`.

Not shown: how it looks on a lamp, and the body's own smoothing and speed
limits on these offsets.

## Optional recent playout level

First's task 18 addendum reports 565 cumulative refusals on lamp-4ace since
09:43, nearly all playout. Its separate four-answer recording at 12:08 had
353 messages: roughly 20 ms pieces, median publication 1.7 ms after interval
start, and at most 18 ms left when published. Those are First's measurements
in [the task list](work-split.md), not Third's replay or a 565-of-353 rate.
Third has not received that recording. A valid piece can end before receipt,
or between receipt and Expression's next 20 ms tick.

`Timing.recent_playout_us` defaults to **0**, preserving strict current
interval sampling in both existing catalogues. It accepts 0 or
20,000–200,000 microseconds. Select
`--catalogue crates/expression/playout-behaviours.json` to use **100,000**
with the original Voice protocol and original clips. This is an initial
tuning choice, not a measured optimum. To combine it with the resumable
protocol and longer nod/rest, copy `rest-behaviours.json` and explicitly add
`"recent_playout_us": 100000` to `timing`; deploy that combination only after
the new Voice publisher is wired. `serve --enable` is still separate.

With this option, a report whose interval has just ended is valid while
`now - end_us < recent_playout_us`. It remains subject to the same payload
and bus-header freshness, identity, ordering, non-overlap and generation
checks. Typed observation age must stay below 200 ms; the bus's 200 ms
lifespan and existing 50 ms wall-clock expiry grace are unchanged. The
planner retains one latest ended piece beside its queue of at most 128 committed
pieces. An admitted queued piece also becomes recent when its interval
ends. There is no queue of historical gestures and no catch-up after a
stalled tick.

Only fresh Voice already in Speaking may use that recent level. Its gain is
`level * (1 - (now - end_us) / recent_playout_us)`, sampled at the current
tick, with zero authority at the exact expiry. The immutable interval end
sets the deadline; new receipt times, ticks and Voice heartbeats cannot
renew it. A current interval wins, including a reported zero level. Future
sound cannot illuminate early or renew the recent observation. Recent
sound cannot change Thinking into Speaking, establish first-audio timing,
claim input readiness or revive a held/retired turn. Voice phase changes,
hold/resume, new identities/generations, terminal events, Voice expiry and
clock faults clear the recent evidence.

A measured body lag still selects a committed future interval for body
anticipation. If none covers that sample, the optional reactive fallback
uses the current/recent level at real `now`; it does not advance historical
sound by the lag. Light always samples at real `now`. Quiet-room,
concentration, joint freshness and actuator-local rules continue to apply.
Recent level is normal evidence rather than a refusal or missing-level
warning; after expiry a Speaking owner again reports missing
`fresh current or recent playout level`.

The publisher's normalized level is a ring envelope (the maximum of
constituent levels in a piece), not a microphone or independently measured
acoustic RMS. The recent fade extends visual/motion modulation; it does not
extend reported sound duration. The target remains a plan on the next
20 ms tick and measured lamp light response within 100 ms of receipt.
The most likely mismatch is a visible tail that feels late; compare current
and recent intervals separately before changing the window. See the
[source review](reuse-decisions.md#expression-recent-playout-evidence-2026-10-14).

## Clock and input contract

All new payload times use `lamp_ipc::monotonic_us()` (`CLOCK_MONOTONIC`) in
microseconds, shared by processes on one host. `clock_id` names that host's
boot, normally `/proc/sys/kernel/random/boot_id` on Linux. It is separate
from `session_id`, which changes every time Voice starts, and from increasing
`turn` and `request` identities. Sequence is increasing within a publisher
session and topic. Backward clocks, expired input, mismatched identities and
retired audio must not acquire authority.

The existing bus header's `at_us` is a publisher's **private** clock and must
not be compared across processes. Live adaptation of legacy inputs uses
receipt time minus the header's Unix-millisecond age. It rejects stale,
expired, untyped and more-than-50-ms-future wall timestamps. Lasting light
and concentration settings are exempt from observation-age limits; they
remain in effect until explicitly changed. Loading an old light setting
stamps its receipt, not an imaginary new physical observation. This estimates
legacy observation age; it does not convert transcripts into audio timing.

| Topic / type | Publisher and required meaning |
|---|---|
| `voice/timing` / `VoiceTiming` | Voice: heartbeat every 100 ms; current state, input readiness, voice session, turn and request. `Scheduled` includes the first committed sample time within 2 s; `Interrupted`, `Cancelled` and `Finished` retire that turn's audio |
| `voice/activity` / `PersonActivity` | Voice: actual admitted human activity edges and heartbeats at least every 100 ms, under the same session/turn/request. A pause requires a preceding true activity observation |
| `voice/playout` / `VoicePlayout` | Voice: non-overlapping committed speaker intervals, at most 200 ms each and at most 2 s ahead, with a normalized level. Revisions/cancellation must retire old scheduled intervals |
| `voice/progress` / `VoiceProgress` | Opt-in replacement for `voice/timing`: the same timing fields, plus positive `audio_generation` and durable `held` state; Latest, 100 ms heartbeat, 500 ms lifespan |
| `voice/sound` / `VoiceSound` | Opt-in replacement for `voice/playout`: the same interval fields plus matching positive `audio_generation`; Event, 20 ms declaration, 200 ms lifespan |
| `attention/concentration` / `Concentration` | An explicit user/attention owner: concentration on or off, default false until received. Room state or missing speech is never substituted |
| `voice/state`, `voice/heard`, `voice/said` | Existing Voice context; none can establish playback timing or safe listening readiness |
| `humans/faces/tracked` | Eyes: largest fresh confident face can supply normalized gaze; the body still owns camera calibration and its stillness/observation filter |
| `body/joint_states` | Body: fresh readback is required for body requests; absolute servo-middle angles are not reused as relative offsets |
| `room/state` | Voice: fresh quiet evidence permits optional nods, sway, unsolicited gaze and rest; unknown, busy or expired evidence suppresses them. A held turn's reactive face target is the gaze exception |
| `name/heard` | Name listener: contextual evidence only, never proof of admission or readiness |
| `ears/sudden` / `SuddenSound` | Ears ([ears.md](ears.md)): a short sound that rose suddenly out of the room's quiet starts the optional `sudden` clip; a sound that went on starts nothing |
| `controls/gesture` / `ControlGesture` | Controls: with an `alive` part, an enabled tap or stroke wakes a sleeping lamp ([alive.md](alive.md)); a long press does not |
| `schedule/due` / `ScheduleDue` | Schedule: with an `alive` part, a notice newly due wakes a sleeping lamp, once for each delivery |
| The local time of day (not a topic) | `serve --zone system` or `--utc-offset-minutes N` itself, at start and every minute; `import` with either, from a recording's wall clock. Only sleep reads it |
| `controls/touch`, `controls/touch_state` | Only with `serve --head-touch`: the head's Stroke, Hold and Tap, and whether that hand is still there (#711) |
| `voice/mute` / `VoiceMute` | Only with `serve --head-touch`: asleep while the voice's mute says `sleep`, awake when it opens; muted (not `sleep`), a tap gets no look up, until it says open, however late; kept 3 s for sleep ([alive.md](alive.md), "The head"; #711) |
| `voice/privacy` / `MicrophonePrivacy` | With `serve --head-touch` or rest requests, read at intake and final planning, never subscribed: the last lease said is the switch open only when it is valid and says `physical_muted: false`, or `no_slide` (a lamp with no slide has no switch to close); anything else, however late, is the privacy switch closed, and a tap gets no look up. A successful read with nothing ever said is open; an envelope or I/O read error is closed, including at the final planning boundary. Said only on the Controls privacy route (`LAMP_PRIVACY_CONTROLS=on` or `no-slide`) ([alive.md](alive.md); #711; [privacy-switch.md](privacy-switch.md)) |
| `light/command` | Actions: the user's lasting on/color/level setting. No light request is emitted before a valid setting is known |

The existing `voice/state` does not carry the private BodyLink's pre-audio
speaking state or duplex-yield mapping, so it cannot substitute for the
three Voice publications. The useful speaker boundary is
`WorkerEvent::Playback` in `voice/src/wire.rs`: owner, cursors, observed time,
accepted frames and queue observation. The coordinator labels the first write
“ALSA accepted; acoustic onset unmeasured”, and the playout the voice
publishes keeps that distinction: it is dated from the device's own count of
what is queued, never from a microphone.

### Held turns: the protocol First must publish

`Catalogue.resumable_voice` defaults to `false`: read `voice/timing` and
`voice/playout`. When true, read the complete `voice/progress` and
`voice/sound` pair instead, with the same `voice/activity` input. The live
service subscribes only to the selected pair; the engine refuses the other
pair, including in replay. There is no legacy fallback that could overwrite
a held state or admit untagged sound.

In JSON, `VoiceProgress` flattens `VoiceTiming` and adds `audio_generation`
and `held`; `VoiceSound` flattens `VoicePlayout` and adds `audio_generation`.
There is no nested `timing` or `playout` object. Both keep the original
boot-clock, publisher-session, turn, request and per-topic sequence checks.
Generation is at least one and never decreases within the same turn.

The producer and planner must agree on these boundaries:

1. On a temporary hold, discard the turn's queued audio and its unpublished
   playout accumulator (`Piece`) before publishing. Advance its audio
   generation, then publish a `VoiceProgress` with `held=true`,
   `state="listening"`, `event="state"`. Preserve the same session, turn
   and request. Continue publishing that held state and generation in every
   heartbeat; a one-shot hold could be lost on a Latest topic.
2. On accepting that hold, Expression clears its queued sound, schedule and
   prior sway, and begins one 350 ms blend from its last requested offsets.
   Repeated held heartbeats do not restart it. The next planner tick requests
   a Listening face only when `accepting_input=true`; otherwise it requests
   Unavailable. The 20 ms planner cadence supports a requested-face target
   within 100 ms, but does not measure bus or optical latency. Fresh face
   evidence can supply a normalized gaze request even in a busy room because
   the hold is a response to someone. Missing/stale face evidence gives no
   guessed target. Body currently ignores this request and keeps its own
   face tracking.
3. To resume, publish active progress with `held=false` in the same generation,
   and only then publish newly committed matching `VoiceSound`. The interval
   and its creation stamp must not predate the accepted generation/resume
   edge. Sound received before matching fresh progress, while held, from an
   older generation or from before resume is refused, not buffered for later.
   Never relabel discarded audio as new sound. Resume also advances the
   activity observation boundary, so delayed speech/pause evidence from the
   hold cannot reconstruct a nod after it.
4. A higher generation clears the previous queued sound even when Expression
   missed the intermediate hold. Another hold after resume requires another
   generation; re-holding an active generation is refused. Sequence numbers
   on different topics are not comparable, and cross-topic publication order
   is not a delivery guarantee.
5. Give-up still uses `Interrupted`, `Cancelled` or `Finished`, with
   `held=false`. These retire the whole turn. A later resume, including one
   with a higher generation, cannot revive it. Voice freshness expiry also
   retires its sound. Keep terminal events in Latest heartbeats until a new
   turn replaces them. An interruption following a hold keeps the blend
   already underway instead of starting it again.

**Implemented by First on 2026-10-14** (`1c52cc822`): the voice says the
second pair beside the first, built from it by `expression_out::Resumable`
(a turn's sound in generations from 1; holding back for someone drops the
unsaid piece, starts the next generation and says Listening, held, in
every heartbeat; after the resume a not-held heartbeat comes before the
first sound and nothing from before the resume is said; the turn's end is
said not held). Through the same authored scene as the first pair and the
rest catalogue, Expression's own engine took 156 progress messages (five
held) and 78 sound pieces and refused none, and planned the nod as speech
ends, the glance, the played sound and the yield, the yield at the moment
of the hold (9.4 s) rather than of the next turn (9.7 s)
(`the_held_turn_pair_is_taken_whole_by_the_rest_catalogue`). A test of the
publisher on a real bus (`while_speaking_the_held_sound_reaches_both_sound_topics`)
sees the held sound on both sound topics.

**On lamp-4ace since 14:27 the same day, acting:** `lamp-expression serve
--enable --catalogue /opt/lampOS/expression/catalogues/rest-behaviours.json`,
the body taking its requests (`LAMP_EXPRESSION_BODY=on`, within the
body's reach and speed). In the first three played questions, in a busy
office, it planned the glance while each answer was awaited, 0.13 s after
the end of speech was decided, and the body took 3,734 requests; the nod
as speech ends, the following of played sound and the rest are quiet-room
behaviours by the catalogue and did not come in a busy room, as written.
Its refusals since its start: four, one stale at start and three of
person activity while no listening input was kept. Not yet seen: any of
the quiet-room behaviours on the lamp, the light taking its requests (the
ring does not read `expression/light` yet), and how it looks.

Since 14:36 it reads lamp-4ace's own copy of the rest catalogue,
`deploy/lamp-4ace/expression-rest-lamp-4ace.json`, the same in every
behaviour with `body_lag_us: 300000`: in the trial at 10:12 that morning
the head was at its farthest about 0.3 s after Expression asked
([body.md](body.md), "What Expression may add"). Acting without a body lag,
Expression reported "waiting for measured body lag" as a warning, and the
lamp with it. The fifth builder's task 25 is to measure it properly.

Before that, use the built-in catalogue and
legacy pair on the lamp. Selecting the new catalogue against the current
publisher leaves required progress missing; it does not manufacture a hold
from `voice/state` or a transcript.

Concentration now starts false without a missing-input warning. Once an
explicit true setting is accepted it remains true until a newer accepted
false setting, including while the setting publisher is absent. Room Quiet
does not mean a person is free from work. Other missing-input reports remain,
including the unmeasured body lag; removing the concentration warning is not
a claim that the whole planner is ready without its required inputs.

### What the voice says, as built

`crates/voice/src/expression_out.rs`, switched on with
`LAMP_EXPRESSION_OUT=on` in the voice unit (with the bus on). The rules are
in `Timing`, which has no bus and takes its clock from the caller; the
coordinator looks at itself once each pass of its loop and hands that over.
`clock_id` is the kernel's boot identity, `session_id` the service's
`INVOCATION_ID`, and `request` is the turn's own number.
This is First's existing legacy publisher. It does not yet publish
`VoiceProgress` or `VoiceSound`.

| Said | From what | When |
|---|---|---|
| `voice/timing`, state | The turn in hand and the phase the ring is shown (listening, waiting, speaking); with no turn, resting | On each change and every 80 ms |
| `voice/timing`, `Scheduled` | While an answer has arrived and none of it has played: when its start gate will open if nothing more arrives (`StartGate::opens_by`), plus how far ahead of the speaker's queue a write sounded last time (30 ms until one has been measured in this run) | When first known, and again when it moves by 20 ms or more. More sound arriving only brings the start forward, so this is the latest it can be |
| `voice/timing`, how a turn ended | `Interrupted` if it spoke and was holding its sound back for someone when it ended; otherwise `Finished`, whether or not it spoke. The voice never says `Cancelled` (below) | Once, then in every repeat until another turn begins. After an interruption that no turn follows, listening for two seconds and then resting |
| `voice/activity` | Whether the detector called a block speech within the last 120 ms | Only while the turn is being heard by a voice that can keep it; on each change and every 80 ms |
| `voice/playout` | Since 2026-10-14, 13:00: the sound the voice still holds before the speaker, which plays straight after the speaker's queue, said ahead in whole pieces of 20 ms with each piece's own level (`Timing::foretell`). Before that, and still for a stretch nothing foretold, each write the speaker took: it ends when the queue as counted after the write has played, and is as long as its frames (`sounds`). The level is the ring's own (0 at -48 dBFS, 1 at -14 dBFS) | Each piece once, up to 200 ms ahead of now, while the turn is said to be speaking and the voice is not holding back for someone. Never overlapping, never for a turn not said to be speaking, and never one with less than 25 ms left to sound |

**On the lamp, recorded from 14:32 to 15:04 that day** (`lamp-bus record
voice/playout voice/sound` while the lamp answered people): 520 pieces on
each topic, each said with 37 to 220 ms of it still to sound (median
219 ms), where before every piece had at most 18 ms left.

Why the sound is said ahead (lamp-4ace, 2026-10-14): the speaker's queue
there holds about 20 ms, so a piece said as the speaker took it began
1.7 ms (median) before it was said and had at most 18 ms left (a recording
of the topic over four answers, 353 messages). The planner, reading it a
little later, had refused 565 since 09:43 as already over. The voice holds
some 350 ms of an answer before the speaker; said from there, in a test
of the planner reading everything 20 ms late with a 20 ms queue
(`said_ahead_the_lamps_sound_reaches_the_planner_in_time`), 78 pieces are
taken and none refused. What it costs: when the lamp stops for someone, up
to 200 ms said ahead never sounds; the planner clears it on `Interrupted`.

The integration findings below are First's records, preserved separately
from the task 18 host implementation:

- **In the legacy protocol, a turn's state never goes back.** The planner
  takes a step from thinking or speaking back to listening as the end of
  that turn's sound and then refuses the turn's later sound. So while the
  lamp holds its sound back for someone speaking over it (a duplex yield, which may yet
  resume), the voice goes on saying "speaking", and says `Interrupted`
  only when the turn is given up. The voice's own ring and body show
  listening from the first moment of a yield, as they did before; a
  consumer of `expression/light` and `expression/body` inside the voice or
  the body must let that local knowledge win. The new progress/sound pair
  above supplies the missing "paused, may resume" contract; First still has
  to connect it to this publisher.
- **The turn that cuts the lamp off begins in the same moment.** On the lamp
  the person who interrupts is heard at once as a turn of their own, so
  `Interrupted` and the next turn's first state arrive together. As first
  built the planner cleared the interruption when a new turn began, and the
  yield it was written for never showed in a test that passed the voice's
  messages through it. It now keeps the transition into the next turn of
  the same voice session (`accept_voice` in `engine.rs`, with a test).
- **A turn nobody answered is `Finished`, not `Cancelled`.** In an office
  most turns end that way: something was said that was not for the lamp,
  or a noise was taken for speech and thrown out. The planner takes
  `Cancelled` for something gone wrong: it asks nothing of the body or the
  light until the next turn and reports a warning. On lamp-4ace, in the
  first ten minutes with people in the office (eight things taken as said,
  none answered), that made the lamp's own health report read "The lamp
  has a warning: expression: voice turn cancelled". An ordinary end is now
  said as one, and the planner goes on planning a lamp at rest.
- **A note about one turn is not a warning.** The planner's report for
  the lamp's health (`standing` in `runtime.rs`) now warns only for a
  fault of the planner's own, a failing bus, a lost record, or, once it is
  switched on, an input it lacks. What it notes about one turn, such as an
  answer that came at short notice so that the look back was shortened, is
  said in the report's message and in `expression/state`, and the lamp is
  well. It also says in its journal which input it refused and why, for
  the first five and then ever more seldom.
- **The body can take its requests** (`crates/body/src/expression_in.rs`,
  `LAMP_EXPRESSION_BODY=on`, off by default; [body.md](body.md), "What
  Expression may add"). It takes the head's pitch and roll and the base's
  turn, adds them to the pose its own mood wants, and holds them to its
  own reach and speed. The sign it reads: a positive `head_pitch` dips the
  head. On lamp-4ace since 2026-10-14, 10:05. Asked there for dips of 40
  and 55 counts over about a second, the head dipped 17 to 38 counts (1.5
  to 3.3 degrees), farthest about 0.3 s after the request was, with the
  base still. The head follows a pose at 120 counts a second and leaves a
  move under 20 counts unmade, so the built-in nod (23 counts at its
  highest, over 0.15 s) cannot be made there, and a nod of a second does
  not fit inside a pause when 0.6 s of quiet ends the turn: this lamp can
  nod as someone finishes, if the planner carries a move begun while
  listening on into thinking. Task 18 implements that optional continuation;
  the new combined plan has not been tried in these lamp measurements.
- **The room is said again every second** (`RoomOut::tell`), so that the
  planner's five-second limit on that evidence is met by a room that is
  judged all the time and seldom changes.

First tested on a development Mac: `Timing` by itself (eight tests: a turn from
listening to its end with its first sound foretold, the repeats, the three
endings, the listening after a cut-off, nothing a reader would refuse, the
person's speech and pauses, the stretches of sound, the dating of a write),
and one test that passes twelve made-up seconds of the voice's loop through
`Timing` into `lamp_expression::Engine` as the bus would
(`crates/voice/tests/expression_out.rs`): 155 timing, 39 activity and 70
playout messages, none refused and no warning, and the planner planned
the nod in the pause (120 ms into it), the glance while the answer was
awaited, the sway while it played, the yield when it was cut off, and a
resting face after a turn nobody answered.

First recorded on lamp-4ace in its first ten minutes (2026-10-14, 09:26 to
09:36, people in the office): the planner took 16,971 inputs and refused 1,
ran 30,600 steps with the longest 2.4 ms, and no bus error. Which input was
refused is not known; the planner now says so in its journal when it refuses one.

`voice/timing` and `voice/progress` are Latest topics, so a terminal edge must
remain in their heartbeats until a new turn replaces it. A one-shot event
followed immediately by an ordinary state can be missed by a subscriber.
Cancellation is sticky within a turn; stale voice input also retires its
audio, and refreshing the same turn cannot revive it. Session retirement is bounded:
Expression remembers the newest 32 retired Voice sessions and forgets the
oldest when another retires. A forgotten session's late message is still
refused: the voice stream's order mark refuses a message stamped before one
already taken (a new session starts the mark's count again, never its time),
and one older than
`voice_stale_us` (500 ms) is stale. Only a voice process that many sessions old
and still running could come back, and the voice's one unit does not allow
that. Until
17 October the fence instead stopped Expression after 32 restarts until it was
restarted itself; lamp-4ace passed 32 in a day, and a home lamp, which
restarts its voice at every update, would have within weeks.

Room evidence expires after five seconds by default. The room publisher
sent on change only; since 2026-10-14 it says the room again every second.
Merely starting Expression does not make an old quiet-room observation
fresh. Lasting light and explicit concentration settings are the persistent
exceptions.

## Output contract and First's integration

These are new topics. `light/command` remains the existing `Light` setting
channel and does not change type or ownership.

| Topic / type | Subscriber and action |
|---|---|
| `expression/body` / `ExpressionBody` | Body: bounded relative offsets or a stillness request. The current consumer takes head pitch/roll and base yaw, ignores arms and the optional normalized gaze, and does not stir settled rest |
| `expression/light` / `ExpressionLight` | Voice's ring owner: draw the face and brightness multiplier using the included persistent `Light` setting; preserve off and explicit settings |
| `expression/state` / `ExpressionState` | Operator/model/recorder: enabled/shadow, planned behaviors, missing inputs and warnings; not proof of execution |
| `diagnostics` / `Diagnostic` | Health: readiness, accepted/refused input, transport losses/errors and measured host service-loop time |

Both actuator requests carry the boot clock, Expression process session,
Voice session/turn/request, sequence, issue/apply time and expiry. A lease is
at most 200 ms and is renewed on a 20 ms planner step. At the final output
boundary, Body and Voice must validate the message, boot identity, producer
session, sequence, turn, freshness and privacy. Deliberate Actions and local
safety take priority. Do not replay a queued old command after a newer one.

The consumer integration needs its own option, off by default. When no valid
lease remains, return to today's voice-state behavior. Kill or stall
Expression and verify that fallback on the lamp. A privacy or input-readiness
failure must never be masked by an expressive listening face. A hold is an
expression request, not permission to override a stop or safety inhibit.

While the keeper has paused the lamp (`lamp/pause`, read as the body reads
it, at most once a second; [keeper.md](keeper.md), "Named states and the
pause"), the service publishes neither `expression/body` nor
`expression/light`, and the returned and recorded output has neither.
The planner still takes its inputs and says its state, and what it sent
before runs out with its lease. Nothing kept on the topic, or a bus it
cannot reach, is not paused.

The planner publishes light before body, but the bus promises no cross-topic
ordering. Compare requested apply times and actual ring/joint observations;
publish order alone cannot establish that a person saw the face change first.
The body cannot promise return before sound until its lag and achievable
trajectory are measured. Never hold speech to make an animation finish.

## Commands and replay

Run from the standalone `lampOS/` directory:

```sh
cargo run -p lamp-expression -- catalogue
cargo run -p lamp-expression -- clock --clock-id development
cargo run -p lamp-expression -- serve --clock-id development --bus-root /tmp/expression-bus --seconds 5
cargo run -p lamp-expression -- replay --input crates/expression/fixtures/five-behaviours.jsonl --clock-id written-scenario --enable --until-us 2000000 --output /tmp/expression-plan.jsonl
cargo run --offline -p lamp-expression -- replay --input crates/expression/fixtures/held-resumed.jsonl --clock-id expression-rest-fixture --catalogue crates/expression/rest-behaviours.json --enable
cargo run --offline -p lamp-expression -- replay --input crates/expression/fixtures/recent-playout.jsonl --clock-id recent-playout-fixture --catalogue crates/expression/playout-behaviours.json --enable --until-us 400000
```

The server above is shadow-only. `serve --enable` enables planner output,
not hardware ownership. Linux uses the boot identity automatically when
`--clock-id` is omitted. Its one-owner lock prevents two Expression processes
on one bus. No deployment unit is installed by this task.

Replay input is one `{ "at_us": ARRIVAL_US, "input": INPUT }` per line.
The payload's observation stamp remains separate from injected arrival time,
so a late event can be tested honestly. Replay never sleeps or publishes to
a bus. Equal-time inputs are applied before that tick; inputs between ticks
are applied before the next 20 ms tick. Output is `{ "at_us": TICK_US,
"output": OUTPUT }` per tick, with a summary on standard error.

`lamp-expression import --clock-id ID FILE...` reads `lamp-bus record` files
and writes replay input to standard output. It needs a new typed timestamp
as a wall/shared-clock anchor. Legacy header `at_us` is ignored. Imported
arrival times are estimates: recordings lack receipt times; wall-clock steps,
transport delay and send delay cannot be reconstructed. Use explicit replay
arrival timestamps for any latency assertion. Import warnings preserve that
limit and sequence gaps/restarts.

A latched setting may predate the recorded interaction by hours. Import does
not silently move that record to the start of the clip; it can consequently
exceed the ten-minute replay bound. In that case, supply a written replay
record with the setting's known load/receipt time, or record a fresh setting
publication. Its time is configuration loading, not a physical observation.

A replay is bounded to 32 MiB, 100,000 lines, 50,000 inputs and ten minutes of
simulated time, with a 128 KiB line limit. Import accepts at most 32 files.
With `--long`, replay and import take an hour and five minutes: 384 MiB,
1,000,000 lines and 600,000 inputs, for a recorded quiet hour
([alive.md](alive.md)). The replay's summary also says in how many ticks
each behaviour was planned and how often it began, the largest change in
one tick and the farthest the body was asked.
Malformed records fail with a location; rejected engine inputs are reported.
Live input drain is bounded per topic. `--record FILE` records requested
output through a bounded queue; a slow/full disk drops records and never
blocks the planner. Files are created new, mode 0600, capped at 64 MiB.
Pending records can be lost at process shutdown. Bus failures do not grant
new authority; commands already sent expire.

The task 18 fixtures all use `expression-rest-fixture` and the explicit
`rest-behaviours.json` catalogue. Substitute their path in the last command:

| Authored fixture | Case to inspect |
|---|---|
| [`held-resumed.jsonl`](../crates/expression/fixtures/held-resumed.jsonl) | Listening on hold, then new sound on resume; four deliberately refused old, held or pre-resume intervals |
| [`held-cancelled.jsonl`](../crates/expression/fixtures/held-cancelled.jsonl) | A held turn ends with `Interrupted`; three deliberately refused revival attempts. The separate test also checks `Cancelled` |
| [`finish-nod.jsonl`](../crates/expression/fixtures/finish-nod.jsonl) | One qualified nod continues from Listening into Thinking without new activity heartbeats |
| [`no-concentration.jsonl`](../crates/expression/fixtures/no-concentration.jsonl) | No concentration publisher and no concentration warning |
| [`rest-quiet.jsonl`](../crates/expression/fixtures/rest-quiet.jsonl) | 140 authored seconds: first rest at 45 seconds, completion at 49, next start at 139 |
| [`rest-busy.jsonl`](../crates/expression/fixtures/rest-busy.jsonl) | 50 authored seconds with no rest in Busy |
| [`rest-concentrating.jsonl`](../crates/expression/fixtures/rest-concentrating.jsonl) | 50 authored seconds with no rest during explicit concentration |
| [`rest-expired.jsonl`](../crates/expression/fixtures/rest-expired.jsonl) | Quiet-room evidence expires at 47 seconds, cancelling the active rest clip |

These are written inputs, not lamp recordings. The
[`rest_replay` tests](../crates/expression/tests/it/rest_replay.rs) also exercise
loss, expiry, shadow mode, real local bus adaptation and the built CLI.
Their isolated healthy-status case supplies an authored zero body lag;
the shipped catalogue leaves lag unknown, so the CLI still reports that
missing measurement. A refused interval in the two adversarial fixtures is
the intended result, not permission to ignore refusals in a lamp recording.

The recent-level addendum uses the separate `recent-playout-fixture` clock:

- [`recent-playout.jsonl`](../crates/expression/fixtures/recent-playout.jsonl)
  supplies authored 20 ms pieces with publication 1.7 ms after onset. Some
  arrive then and end before the next tick; others have an explicitly
  authored transport delay and arrive after their end. Use
  `playout-behaviours.json` for this legacy-protocol fixture.
- [`recent-playout-generation.jsonl`](../crates/expression/fixtures/recent-playout-generation.jsonl)
  exercises recent levels across hold/resume, delayed stale-generation
  sound and a terminal event. It requires a catalogue with both
  `resumable_voice: true` and `timing.recent_playout_us: 100000`.

Neither fixture is First's recording. Payload publication time, authored
receipt time and planner tick are distinct. Tests compare the requested
output with the default option disabled; no inferred delay is labelled an
acoustic, optical or motor measurement.

The first three reports in the legacy fixture each reach their next planner
tick 18,300 us after authored receipt. With the option enabled, the requested
brightness follows the admitted level at that tick. With strict sampling,
the first has ended by that tick and the two delayed reports are refused.
The zero-level report then takes precedence; future sound begins at its
committed time, and the last recent level expires at 320,000 us even though
Voice heartbeats continue. These are exact injected-clock assertions.

`cargo run --offline --release --locked -p lamp-expression --example recent_playout_cost`
runs one first iteration and 100 warm iterations per mode on the same
15-record, 21-tick authored trace. It compares strict mode with the optional
100 ms window in this build, in that order. Timing includes catalogue clone,
Engine construction, JSONL parsing, injected planning and serialization to a
sink; it excludes catalogue parsing, process startup, transport, disk output
and hardware. Warm median is the average of the two middle sorted samples.
The example embeds its catalogue and fixture; First can build it for the lamp
and run the resulting `recent_playout_cost` binary without a source checkout.

## Verification and lamp checklist

Host gate results and source identity are recorded in
[Third's log](work-log-third.md). Tests use injected clocks for behavior,
real temporary Unix sockets for transport, and separate processes for CLI
and ownership checks. A replay establishes planned timing only.

The fresh-output/session and offline-face runtime fixtures finish service
construction and option setup before publishing their fresh inputs. The
engine uses the captured wall clock, but bus admission still checks the
real wall clock against the unchanged 500 ms VoiceTiming lifespan and 50 ms
transport grace. Setup must not consume that freshness window. Input loss,
stale retained inputs after restart, new session ownership and both offline
face outcomes retain their original assertions. A stall after publication
can still expire an input; the fixtures neither refresh it nor retry a step.

The sleeping-name journal fixture first admits a fresh empty face observation,
then advances its injected clock across the catalogue's 60-second absence
threshold while publishing fresh inputs again. It waits for the service's
observed `asleep` output before sending the name, avoiding a minute represented
by hundreds of durable bus-publication steps. The default fixture step remains
200 ms; transport lifetimes, freshness and service cache periods are unchanged.
The name's two-second source window and 300 ms delivery grace, all 25 subsequent
checks, the exact single journal line, child status check and the 15-second host
watchdog starting after child spawn retain their original bounds and assertions.

The held-Mood runtime fixture advances its injected clock by 250 ms per step,
matching the service's Mood read period; other sleeping-service cases keep
200 ms steps. It stamps Mood at the service's first clock sample, after the
other inputs' publication-span adjustment. Later receive and tick samples
include Mood's own publication work. Paired with the original wall sample,
these clock reads preserve the other inputs' full wall ages while the fixed
Mood phase is sampled at its source stamp. The plain-clock control still
breathes. The original amplitude and oscillation assertions, Mood pace,
service cache period and transport lifespans are unchanged.

The following counts and cost measurements are the **original task 4
gate**, not a new task 18 result. On Third's Intel i7-10700K x86_64 Mac
(64 GiB, Darwin 25.6.0, Rust 1.96.0), the full dependent gate passed 1,701
tests with five existing ignored tests.
Expression's 67 tests and eight new message-contract tests have no ignored
cases. The standalone source copy built the workspace and passed 92
Expression/message checks, including a doctest. The initial parallel run's
Voice failures and the successful serial rerun are retained in the
[host evidence](verification/expression-host-20261014/results.json).

`cargo run --offline --release -p lamp-expression --example replay_cost -- 100`
measured construction, parsing, 101 planned ticks over two simulated seconds
and serialization to a sink for the 42-record written fixture. In the final
100-run series, the first iteration took 0.284349 ms, the median 0.192103 ms
and the worst 0.296518 ms; no inputs were rejected. Catalogue parsing precedes
timing. This is host computation, excluding process startup, bus transport,
disk recording, scheduler jitter and physical response. Both 100-run series
are retained in the evidence; no lamp or before/after improvement is claimed.

For the new catalogue, run
`cargo run --release --locked -p lamp-expression --example rest_cost -- 100`.
It measures one first run and 100 warm runs of each held/resumed and finishing
nod fixture: Engine construction, JSONL parsing, injected-clock planning and
JSON serialization to a sink. Catalogue parsing, I/O, hardware and physical
timing are outside that boundary. The reported warm median is the upper
middle sorted sample (`warm_runs / 2`); the first iteration is not a measure
of process startup. Task 18's measured results and final gate
belong in [Third's log](work-log-third.md).

**Task 18 host results, 2026-10-14.** On that same development Mac, with
Rust/Cargo 1.96.0, formatting, strict all-target Clippy for all 14 touched or
dependent packages, the workspace build and the serial dependent test gate
passed: **2,353 tests, zero failures, eight inherited ignored cases**.
Expression passed 125 tests; messages passed 44 including the doctest.
The 62 added tests have no skips. A source-only copy outside the parent
repository built all 27 workspace members, resolved all 28 local packages
inside the copy, and passed the same 169 Expression/message tests.
Runtime source hashes matched after the gates and timing run. Commands,
outputs, skipped names and the earlier failures are retained in the
[task 18 evidence](verification/expression-rest-host-20261014/results.json)
and [failure ledger](verification/expression-rest-host-20261014/failures.md).

One release-profile invocation ran a first iteration and 100 warm iterations
of each authored scenario, with no other Third-owned build or test running;
unrelated host load was not controlled:

| Scenario | Inputs / deliberately refused / ticks per run | First iteration | Warm upper median | Warm maximum |
|---|---|---|---|---|
| Held and resumed, 300 ms of authored time | 18 / 4 / 16 | 0.172494 ms | 0.059406 ms | 0.124608 ms |
| Finishing nod, 1.8 s of authored time | 37 / 0 / 91 | 0.195416 ms | 0.185896 ms | 0.353597 ms |

The timed boundary is the computation described above. Separately, the
injected-clock replay requests Listening at 60,000 us after hold receipt at
41,001 us, and Speaking at 140,000 us after resume receipt at 120,001 us:
18,999 and 19,999 us respectively. Neither those authored intervals nor the
host cost establishes transport, optical, acoustic or motor timing. A direct
built-CLI replay also produced the expected 16 ticks, 14 accepted inputs and
four intentional refusals; its exact requested output is retained with the
evidence. No Linux/ARM64 or Rust 1.91 MSRV run was made by Third.

First can run the written fixture with the built `lamp-expression` binary
and compare the JSONL to the host's replay. Then record the lamp's real bus,
including whichever complete Voice protocol is selected, and import/replay
it. Before enabling a changed plan on the lamp, inspect requested offsets
and expired commands with offline `replay --enable`. Live shadow mode records
state, behaviors and missing inputs, but deliberately omits actuator payloads. To
inspect live actuator requests, explicitly enable the planner while both
actuator-consumer switches remain off. Once both consumers are enabled,
check: look back by first audible word, nod only in a person's pause,
immediate cancellation when speech resumes, light leading the interruption
transition, stillness during work, and fallback after killing Expression.

For task 18, First must additionally:

- Implement the progress/sound producer fence, including clearing a pending
  `Piece`, then record hold→resume and hold→give-up with delayed old sound.
  Check the face request within 100 ms of hold receipt separately from actual
  ring response and acoustic silence. Preserve local privacy/readiness.
- Film the carried nod and ring together, with joint feedback and the
  committed audio time. Measure request→peak and request→return separately;
  interrupt the nod with renewed speech and an earlier audio schedule. The
  roughly 0.3-second peak delay in First's earlier silent trials is not an
  acoustic offset or universal motion-onset lag.
- Choose the rest author before changing Body's settled-rest rule. Compare
  the optional rest with disabled rest during real desk work, with focus,
  busy room, input inhibition and expired evidence. Confirm no competing
  idle writer, arm motion, room scan or misleading listening face. The new
  rest shape's appearance and deadband losses are the likeliest mismatch.

**Recent-level addendum, 2026-10-14.** On Third's same development Mac,
formatting, strict all-target Clippy, serial tests for Expression and all
three dependents, and the workspace build passed: **1,461 tests, zero
failures, seven inherited ignored cases**. All 46 added tests passed.
A source-only copy built all 27 workspace members and passed all 171
Expression tests, without failures or skips. The final source hashes,
commands, results and failures are retained in the
[addendum evidence](verification/expression-playout-host-20261014/results.json)
and [failure ledger](verification/expression-playout-host-20261014/failures.md).

One release invocation of `recent_playout_cost` ran the same authored trace
first in strict mode, then with the option, with no other Third-owned build
or test running; unrelated host load was uncontrolled. Strict mode's first
iteration was 0.151605 ms, warm median 0.0514965 ms and maximum 0.057421 ms.
Optional mode's first iteration was 0.056098 ms, warm median 0.0510775 ms and
maximum 0.055327 ms. These are the computation boundaries described above,
not process startup or a statistically established speed improvement. All
202 samples are retained. The direct built-CLI replay accepted all 15 inputs
and produced 21 requested frames through 400,000 us. First should compare
the lamp's real short reports and visible tail before changing the window;
no new Voice, Body, Audio or deployment code is part of this addendum.

## Not shown

First's legacy publisher, shadow planner and Body consumer measurements are
recorded above and in [Body](body.md#what-expression-may-add) and
[work-split.md](work-split.md). Third has not accessed the lamp. Those earlier
measurements do not qualify the new hold/resume contract, the combined
finishing nod or rest. Voice does not yet publish the new pair, and Body
still suppresses settled-rest offsets.

Host tests and authored replays do not establish physical motion, camera
calibration, optical latency, acoustic onset/cancellation, safe motor
trajectories or acceptable expressiveness. Published robot studies are not
lampOS measurements. Body lag remains unknown; the new fixtures are written,
not captured from the lamp. Transport age estimates cannot repair clock
steps or recover receipt timing. First owns the remaining lamp-4ace wiring
and observation; Fifth now owns Body and its separate lamp qualification.
The recent-level addendum does not establish how the 100 ms tail looks, or
validate First's planned earlier Voice publication. Its normalized levels
remain speaker-owner reports.

## Privacy read failures at presentation boundaries

Head taps and ordinary skill rests keep failed retained-envelope or I/O
reads distinct from an absent privacy publisher. A failed read counts as
closed at intake and at the final planning boundary. An active tap look
uses the existing cancellation/ease-out path; an active ordinary rest is
cancelled. A subsequent valid open lease permits a new input, but opening
alone does not resume the cancelled work. Valid retained closed leases,
malformed payloads and the no-slide rule retain their existing meaning.

The correction adds no read, polling interval, sleep or retry. The target
is refusal at the same planning boundary that observes the error; physical
motion still follows the existing bounded ease-out and Body guards. Real-bus
controls exercise closed, unreadable, valid-open recovery and in-flight
cancellation, plus no automatic revival. These are injected-time planning
checks, not physical latency or microphone-readiness measurements.

## Leased skill rest requests

`LAMP_REST_REQUESTS=on` enables generic skill-owned rest intake. It is off
for every other value. In this mode, while lamp-skills claims the night
(`expression/sleep_claim`, fresh for 180 s), Skills chooses routine timing
and Expression stops its automatic night/absence sleep decision; with no
fresh claim it sleeps by its own schedule, the fallback for one release
(#940, [alive.md](alive.md#night-sleep-is-a-skill)); it is on in the
product's `lamp-expression.service`. The claimed skill's
rest (`night-sleep`) is held as the built-in sleep is, through the voice
listening, a new voice turn, a privacy mute or a held body, and the first
face out of the night wakes it; every other skill's rest keeps the gate
below. The existing renderer, name/touch/reminder/conversation reflexes,
manual sleep and privacy behavior remain. `--enable` still independently
controls actuator requests; the rest switch adds no writer or raw-joint
interface. See [Bedtime and Morning](bedtime-morning.md) for the disabled
core skill and its pending lamp comparison.

`expression/rest_request` is a typed `Event`
(`lamp_msgs/ExpressionRestRequest`, version 1), sent by one publisher named
`skills`. The payload and nested stamp tolerate additive fields. Unknown
`op` values are refused; required identities, clocks and lease bounds remain
checked.

| Field | Binding |
|---|---|
| `stamp` | Original host-monotonic `clock_id`, `at_us`, and global publisher `sequence`; never replaced by receive time. |
| `skill` | Generic bounded skill identity. |
| `skill_session_id` | Request header's publisher boot. |
| `expression_session_id` | Current `expression/state` header's publisher boot, not the host boot or the receipt publisher boot. |
| `episode` | Positive increasing episode within that skill and publisher session. |
| `op` | Exactly `rest`, `wake`, or `cancel`. |
| `valid_until_us` | After issue time and at most three seconds later. |

Intake requires the expected topic, type, Event kind, source, publisher boot
and matching header/payload sequence. Messages are bounded to 4 KiB. Admission
and lease extension require a source age strictly less than 500 ms, with no
future stamp; transport freshness never rejuvenates the payload. Skills renews at one-second
intervals. Rest renewals preserve settling, wake renewals preserve the
single running stretch, and cancellation releases only routine-owned rest
without starting a wake clip.

A delayed same-episode `rest` renewal that is at least 500 ms old, but not
future or expired, is discarded while its original accepted Rest lease
remains valid. Its source order is consumed, but the exact prior request,
expiry and `resting` receipt remain unchanged. It grants no extra time;
a later fresh renewal must arrive before the original lease expires. New
episodes, `wake`, `cancel`, request-stream gaps and owner or privacy changes
retain their normal checks and cancellation behavior.

Admission needs fresh resting Voice and measured joints, free of pause,
concentration, privacy or manual sleep. Idle Voice need not already be in
a retained Listening turn. The current Voice session/turn/request is pinned
for the episode. A stronger reflex, ownership change, loss of fresh inputs,
lease expiry or pause cancels it. A request-stream gap detected when the next
valid request arrives also cancels it; silence or malformed traffic is bounded
by the existing lease rather than a separate stream-gap notification. A failed retained privacy read also closes this gate: it refuses a new
ordinary rest and cancels one in flight. A later valid open read allows a
new episode; reopening alone does not revive the cancelled one. The claimed
night-sleep exception remains unchanged. A successful read with no privacy
publisher retains the compatibility behavior of a lamp without this route.
A routine cannot release hand
or privacy sleep. Existing Body fold, pause, range and motor guards remain
authoritative; Expression does not infer unfolding from joint angles.

When Expression ends an active rest itself, its journal says why once:
`lamp-expression: ended a skill's rest: <why>`, the first shut gate (the
voice listening or in a turn, privacy, a pause, a held body, a stale voice),
a changed Voice turn, a stronger reflex, rest no longer allowed, or the
lease running out. The night's sleep (#940) is the exception to a lease
running out: it is held down for 10 s past it (`SLEEP_HOLD_US`), and a
newer episode or a new session's first rest of the claimed skill takes it
over with no release; with no taker, the built-in sleep keeps it in place
while its own conditions hold ([alive.md](alive.md#night-sleep-is-a-skill)).
A `cancel` the skill then sends for that already ended
episode is a no-op: it is accepted with a `cancelled` receipt (unless
another skill's rest is active, whose receipt is kept) and resumes nothing.
Before #1672 it was refused as "rest episode was already spent". A cancel
that arrives with lost input is still refused.

Each accepted or refused episode leaves a tombstone. Cancelled or completed
episodes cannot be renewed into activity. New publisher boots retire old
Skills sessions across all skills. The planner remembers at most 32
skill/session identities without evicting old fences; exhaustion fails
quiet until a new Expression session. With one routine this permits at most
32 distinct Skills publisher sessions before Expression must restart. A
definition reload fences old run threads and cancels pending transitions,
but retains an already-admitted rest if its complete enabled owning definition
is unchanged. An edited, disabled or removed owner loses its rest.

The optional `expression/rest_state` Latest topic
(`lamp_msgs/ExpressionRestState`, version 1) publishes every 200 ms with a
500 ms transport lifetime. It contains `stamp`, `expression_session_id`,
`enabled`, the correlated `request` or null, and `phase`: `idle`, `resting`,
`waking`, `completed`, `cancelled`, or `refused`. A `refused` receipt also
carries `reason`, a short line of why (lost input, a stale, future or expired
observation, the owner unavailable or privacy-muted, or the transition
unavailable), absent from the JSON otherwise, so an older reader is
unchanged; the journal says `lamp-expression: refused a rest (N so far):
<why>` once for each new reason, not at every renewal. It is a planning receipt,
not proof of applied motion or unfolding. Cancelled and completed receipts
can retain their last accepted operation, so consumers correlate terminals
by skill, publisher session, target session and episode rather than requiring a
newer renewal's sequence or operation. Morning waits for `completed` before
requesting sound; Say would otherwise change Voice ownership mid-stretch.

The deterministic integration surface is `Engine::with_rest_requests`,
`ingest_rest`, `set_rest_paused`, and `rest_state`; live intake uses
`runtime::Service::with_rest_requests`. The switch leaves existing
`ExpressionState` messages unchanged. Host tests cover leases, source age,
strict transport, replay, publisher and Voice changes, privacy/manual sleep,
reflex precedence, stream loss, and the real bus receipt path. Physical
behavior and end-to-end lamp latency remain unmeasured for this addition.

## The unplug presentation (#1674)

With `LAMP_UNPLUG=on`, a matching fresh body Ready produces the resting face
and `asleep` behaviour. Expression acknowledges its publication, or, with no
light of its own to send (a cancelled voice turn, no fresh voice timing), that
it presents asleep and asks nothing of the body; the active
voice ring worker separately acknowledges an actual permitted asleep-frame
write. The skill requires both, plus Body's live proof. This opt-in path does
not make ordinary `expression/light` a general ring input or change privacy.
See [preparing to unplug](unplug.md).
