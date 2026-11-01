# Bedtime and Morning (#1672)

**Since #940 (the first builder on #2602, 22 Oct):** the lamp's sleep is
the core skill [Night sleep](alive.md#night-sleep-is-a-skill), on in every
product lamp, and the first face of the morning wakes it as Expression's
reflex. Bedtime and Morning dropped its own `rest` and `wake` steps and its
bedtime trigger: it keeps the evening glow, the morning hum and the night's
ceilings, installed and off, a person's add-on. Its morning hum no longer
waits for a night it rested or for a wake it owns, and it plays no stretch
of its own. What this page says below of its bedtime rest, settling again
after a night sound and the morning's owned wake describes the routine
mechanism as Bedtime used it before #940; no shipped skill uses `resettle`
or `op = "wake"` now. The hazard where rest requests on with no skill meant
a lamp that never slept is closed: with no fresh claim on the night,
Expression sleeps by its own schedule (the fallback, for one release).

The product installer includes the [core skill](../skills/bedtime-morning/skill.toml)
at `/var/lib/lampOS/skills/bedtime-morning/`, disabled with `enabled = false`.
Turning it on means editing that installed `skill.toml` to `enabled = true`.
It stays on through the next install or update round: the round keeps it in
`/var/lib/lampos-skills-state/on` and writes the build's file turned on
([skills.md](skills.md), #2585). A multi-night lamp run survives an
overnight round.
It copies every file of each product skill's folder but its `tests/`
(`install::PRODUCT_SKILLS`). Those in `install::PRODUCT_SKILLS_OFF` are
installed disabled by install itself, and Bedtime and Morning by its own
`enabled = false`; the rest are installed enabled (Not feeling well came with
the product after its lamp check, #1681). Desk tutor, Desk tutor marks and the
other installable skills remain outside the product install
(`install::NOT_PRODUCT_SKILLS`); [skills.md](skills.md), "Core and
installable skills", has each one's row. Bedtime's
hours, movements, sound and ceilings are authored defaults awaiting a lamp
comparison. Host validation does not authorize an installation or establish
physical, optical or acoustic behavior.

## Default-off controls and ownership

Three independent process switches accept exactly `on`; every other value
leaves that feature off.

| Switch | Owner and effect |
|---|---|
| `LAMP_SKILL_ROUTINES` | Skills admits routine definitions and runs their rest manager. Disabled manifests still contribute no active triggers or tools. |
| `LAMP_REST_REQUESTS` | Expression accepts leased rest intents. While lamp-skills claims the night (`expression/sleep_claim`, renewed every 60 s, gone after 180 s), its automatic night/absence decision is the skill's; with no fresh claim it keeps its own schedule, the fallback for one release (#940). Its existing `--enable` remains necessary for actuator requests. |
| `LAMP_OUTPUT_CAPS` | Actions serves durable private cap operations; Voice applies their snapshots in the ordinary ring worker. Skills starts its limits client only when this switch and `LAMP_SKILL_ROUTINES` are both on. |

These controls and the manifest's `enabled` field must be coordinated during
a separately approved rollout. Turning on Expression's request intake alone
removes the old autonomous schedule without creating a skill run. Timed
Skills behavior also needs a configured local zone or offset, a synchronized
clock, and a durable daily file (`LAMP_SKILLS_DAILY_FILE` or
`--daily-state`). Actions requires an explicit persistent caps directory
(`LAMP_OUTPUT_CAPS_DIR` or `--caps-dir`; the option wins when both are given)
and `--bus-root` when caps are enabled; these are absolute paths without dot
components. Its caps directory is separate from the volatile runtime
`--dir` and from other stores.

The [product units](install.md) supply
`LAMP_SKILLS_DAILY_FILE=/var/lib/lampos-skills-state/daily.json` to Skills and
`LAMP_OUTPUT_CAPS_DIR=/var/lib/lampos-output-caps` to Actions, as
`Environment=` lines: an older binary from the channel ignores them, where it
would exit 2 on an option it doesn't know (#2157). Their respective
`StateDirectory=lampos-skills-state` and `StateDirectory=lampos-output-caps`
declarations use mode 0700 and the service user `lampos`. Systemd creates
these durable directories when the services start. The paths do not enable
the new features. Since #940 the product units turn on
`LAMP_SKILL_ROUTINES` and `LAMP_REST_REQUESTS` for Night sleep, with
Expression's built-in sleep as the fallback for one release;
`LAMP_OUTPUT_CAPS` stays unset, so this skill's ceilings stay off, and its
manifest stays disabled with no active trigger or advertised tool.

Skills chooses when a routine is eligible. Expression owns its rest pose
and transitions; Body retains the existing fold, pause, range and motor
checks. Voice owns sound admission, privacy, interruption and the speaker.
Actions owns mixer restrictions, and the existing ring owner applies light
ceilings. The skill adds no actuator writer, raw-joint interface or
microphone permission. It declares no model tool.

The manifest explicitly declares `kind = "core"`, `version = "1.0.0"`, and
its grants: tracked faces and Voice state, body, light, speech and
`output_limits`. The fixed hum requires speech permission; every rest, wake
or cancel requires body permission. Both are checked before a routine
request is sent, including the hum that follows a wake. Private caps require
their own default-denied grant, independently of presentation. The grant is
not restricted to core skills: an enabled installable skill with
`output_limits = true` can declare the same bounded restrictions. Removing
that grant can still release this service's already-owned exact tokens.
Accepted cap definitions carry a generation: a reconciliation overtaken by reload
cannot send a put or removal based on the old definitions. The final socket
write checks that generation without holding its lock over connection,
readiness waits or replies. A complete request sent before reload may still
finish in Actions; the new generation reconciles any owned token rather
than claiming that in-flight work was rolled back. Disabled core folders
remain listed and protected from removal or replacement, while contributing
no active triggers, read subscriptions or catalogue functions.

## Authored routine

The core manifest uses local time, with each window including its start and
excluding its end.

| Part | Eligibility | Authored result |
|---|---|---|
| Night ceilings | 21:00–06:00, independently of presentation or presence | Maximum ordinary ring channel value 24 and raw mixer volume 35. These are device values, not measured brightness or loudness. |
| Evening | 21:00–23:00; a nearby person for 30 seconds | One step warmer and one step dimmer than the person's own light as the ring shows it (`light/state`): the light breathes from their level down one step and back, 10 s a breath, for 20 s, then is given back. Always a level under their light, never over, in their colour when it is already warm. At level 1, off or not known, nothing is shown and the journal says why. |
| Morning | 06:00–11:00; a nearby person for three seconds and nothing heard on `voice/heard` since 06:00 | Ask Say for a hum. The lamp's wake is Expression's, at the first face (Night sleep, #940). |

Evening and morning each have one daily chance and require Voice to
have reported resting for 30 seconds. They retain the process's existing
`LAMP_SKILLS_ASK` mode. The product unit currently sets `skip`, so it does not
ask Policy. With `policy`, their `nudge` declaration becomes an `unprompted`
request: Policy may refuse it, including silent rest, because its existing
question judges speaking up and its fallback refuses known absence and
hours outside 08:00–22:00. There is no reminder exception or relaxed Policy
rule for this skill. With neither mode, no automatic presentation starts.
Record the ask mode in the lamp comparison; the policy-mode night path is
not qualified by the host tests.

Evening and morning may wait up to one minute for admission, and bedtime
up to 15 minutes (`wait = "15m"`, #1672): on lamp-06c1 the empty desk
matured two seconds before the voice had rested its 30 seconds after a
sound, and without the wait the night's chance was spent with no line.
A chance dropped because its conditions did not hold within its wait now
says so in the journal (`bedtime chance dropped: ...`).
A once-a-day trigger's watch reset after it had begun holding says so
once, with how long it held and why (#1672, row 7: bedtime never came in
23 minutes and the journal could not say why):

- `bedtime watch reset after 412 s: stopped holding: humans/faces/tracked present no longer false`
  (a face came, past the trigger's `gap`)
- `bedtime watch reset after 412 s: went silent: nothing held it on humans/faces/tracked for 10 s, past its 2 s`
  (the topic said nothing past its lifespan or `gap`)
- `bedtime watch reset after 412 s: left its window 23:00–06:00`
- `bedtime watch reset after 412 s: no trusted local moment` (the clock lost its trust)

A watch that never began holding, or a trigger that is not once a day,
says nothing.

### The morning greeting

The first builder's ruling: the greeting comes once a day, on the first face
seen 06:00–11:00, if the lamp rested that night, even when a sound ended
the rest early. Before, a morning after a night stir was refused (`this
skill owns no rest to wake`) and the hum was skipped too.

- **The night it rested.** Skills' rest manager keeps the local day the
  last night this skill's rest began (its rest window's start day: 02:00
  belongs to the night that began the evening before), set when Expression
  first said `resting` for it. With `--state-dir` (the product's) it is
  kept in `DIR/rested`, one line a skill (`bedtime-morning 20453`), and read
  back at the start, so a restart between the night and the morning still
  greets (#2757). It is kept through the morning's wake, for Welcome back's
  `yields_to`, and cleared when the skill's next night's rest begins or the
  skill changes. Without `--state-dir` a restart forgets it.
- **Already up.** A morning wake with no rest left to own, on the day after
  that night, is answered `Woke::Already`, once, and nothing is sent to
  Expression. The run then sends the body its `stretching` clip (6.5 s) and
  asks Say for the hum only once `body/gesture` says the clip is over, so
  the hum never cuts the stretch short. A stretch the body dropped, a push
  or a pause, and no hum follows. Both stay within the wake's 12-second
  start deadline. A wake of a still-owned rest is as before: Expression's
  waking stretch, then the hum, with no second stretch.
- **Never a sleeping lamp.** No `Already` answer, and so no stretch and no
  hum, while the lamp is put to sleep by hand (the newest `voice/mute` says
  muted with reason `sleep`) or kept private (the newest `voice/mute` says
  muted with no reason, or the newest `voice/privacy` is not a valid open
  lease: the slide closed). These are read as Expression reads them, and
  read again just before the stretch. The run is refused (`the lamp was put
  to sleep by hand, so it is not woken`).
- **Already spoken to.** The morning trigger has `unless_heard = true`:
  once anything was heard on `voice/heard` since 06:00 that day, the face
  is not greeted, the day's chance is claimed, and the journal says so once
  (`morning chance let go: spoken to since 06:00`). Anything the voice
  took in as heard counts, not only the person: a radio or a TV it heard
  as speech uses up the morning's greeting too (chosen: a missed greeting
  is quieter than one over someone already up). Mid-conversation the
  trigger's `[[while]]` (the voice resting 30 seconds) holds it back as
  before, and a skill turned off has no chance at all.
- **No rest, no greeting.** A night it did not rest, or the night before
  last, answers the wake with `this skill owns no rest to wake`: the run
  ends before the hum. A failed,
interrupted or refused chance is consumed; there is no later catch-up that
day. Missing, malformed or stale faces never count as an empty desk.

### One greeting after a rested night

On lamp-06c1 on 22 Oct, after a night it rested, Welcome back said its
hello at 06:01:14 (`it appeared on humans/faces/tracked after 7451 s
without`) and the morning chance was dropped at 06:01:34: with both
greeting, the first face of a morning could be greeted twice. The first
builder's ruling (06:08 on #1672): after a rested night, the morning
greeting is the welcome back.

- **It waits.** Welcome back's trigger has `yields_to = { skill =
  "bedtime-morning", trigger = "morning" }` ([skills.md](skills.md)). Its
  chance for the first face is kept, not spent, until the morning trigger
  is judged, and the journal says once `welcome-back waits: the morning
  greeting comes first`. It waits only while the morning trigger holds or
  is in its minute's `wait`, unbroken since Welcome back began waiting (the
  reviewer's P2 on #2757): a face the morning never holds for, such as one
  looking down at the keyboard every other second (Welcome back's 3 s
  `gap` keeps it, the morning has none), is greeted by Welcome back at once,
  not at 11:00. Both share `near = 0.14`, so a face too far for one is too
  far for the other.
- **The morning fires.** Its run is the face's greeting, and Welcome back's
  chance for that face is done: `welcome-back done: bedtime-morning's
  morning greeting was this face's hello`. With the Policy asked, a no to
  the hum is still the morning's chance taken.
- **The morning is dropped or let go.** Its conditions did not hold within
  its minute's `wait`, or someone spoke to the lamp since 06:00: Welcome
  back greets as before, at its trigger's next message while the face is
  there, after its own conditions.
- **One morning moment a day** (the first builder on #2757). Welcome back's
  hello in that window, after a rested night and before the morning was
  claimed, claims the morning's day in `daily.json`, as the morning's own
  greeting would, and the journal says once `bedtime-morning: morning
  chance let go: welcome-back already greeted this morning`. A face later
  that morning gets Welcome back's ordinary hello, if its five minutes away
  and its cooldown allow, never the morning's ritual.
- **A restart.** The morning's day is claimed on disk and the night rested
  is read back from `rested`. After the hum, a face there at the restart is
  no news to Welcome back's `appears`, and the morning does not fire again:
  nobody is greeted twice. Before the hum, the morning still greets the
  first face and Welcome back yields.
- **When it applies.** Only the morning after a night a routine rested
  (the rest manager's `rested_day`, kept in `rested` across a restart), 06:00
  to 11:00, on a day the morning has not yet been claimed in the daily
  journal, with this skill enabled and `LAMP_SKILL_ROUTINES` on, and able
  to act (an ask mode, a journal). Otherwise Welcome back behaves exactly as
  before. A later face that morning, after Welcome back's five minutes
  away, is greeted by it as always.

The ruling covers every face of the morning: once Welcome back has greeted
someone, the morning's day is claimed, and a face back later is welcomed,
never hummed at.

### The evening glow, relative to the person's light

The evening glow was fixed numbers, warm level 2 breathing to 1. On
lamp-06c1 the person's light was already warm at level 1 and nobody saw the
glow (#1672). It is now `light = { relative = { warmer = 1, dimmer = 1 },
for = "20s", pattern = { kind = "breathe", period = "10s" } }`, made as the
run begins against `light/state`:

- **A step warmer** is one along white, then the amber `warm`, the ring's
  own warmth as `light_warmer` steps it (`glow::EVENING_WARMTH`). Warm is
  as warm as evening light goes; orange or any other colour is one the
  person chose, kept as it is.
- **A step dimmer** is one of the ring's five levels. With the breath, the
  light stays at the person's level at the top and breathes down one step
  and back. At level 1 there is no step under it, and nothing is shown
  (the first builder, 21:18 on #2655: "down, never up"; someone at their
  dimmest light is already resting, and a lamp brightening on them at
  22:00 reads as a timer). The journal says `the person's light is at its
  dimmest: no evening glow`.
- **Always one visible level** (the first builder: "one visible level,
  never the colour"). On lamp-06c1 at 17:41 the person's light was warm 3:
  warmer goes no further than warm, so the glow is warm 3 breathing to 2.
  A warmth that is clamped is never made up for with another colour, and a
  `dimmer` of 0 still moves one level. Without a breath it is steady a
  level down.
- A light that is off or not known: nothing is asked of the ring, and the
  journal says why (`the light is off`, `the light shown is not known`).

The glow is asked of the ring as any skill's light is, at skill priority,
and given back after its 20 seconds.

### Settling again after a night sound

A lamp gone to bed that hears one sound stirs and settles back, as a pet
does, rather than staying up all night (the first builder's ruling, rule 8).
Bedtime's rest step says so:
`resettle = { most = 3, after = "30s", during = { from = "23:00", to = "06:00" } }`.

- **What wakes it.** Expression ends a routine rest itself when its gate
  shuts. Its journal says why, once (`lamp-expression: ended a skill's
  rest: the voice is no longer resting: it is listening`). The skill's
  cancel of that already ended episode is then a no-op, not a refusal (row
  7's "rest episode was already spent").
- **What re-settles.** Only a night sound: the voice listening that never
  became a turn. A turn (the voice thinking or speaking, so a name call or
  anything the person asked for) means someone is up and ends the night's
  rest, whether Expression's cancellation arrives during the turn or the
  listening becomes one while the lamp waits to settle (the reviewer-lead's
  P2 and the first builder's ruling on #2512). A hand, privacy, a pause, a
  reflex such as a touch, or a lapsed lease end the night's rest as before,
  and Skills says so.
- **Only at an empty desk.** Each re-settle reads `humans/faces/tracked`
  again at that moment, as bedtime's `present = false` trigger does: a
  valid, empty face list within its lifespan (the header's, else the eyes'
  two seconds). A face in view means someone is up and ends the night's
  rest (`a face is in view; someone is up, so the night's rest ends`).
  Missing, malformed or stale faces are not an empty desk: it waits, and
  settles only on a fresh empty look.
- **How.** Skills' rest manager, not a run, starts a new episode once the
  voice has said `resting` for 30 seconds again and no face is in view,
  inside 23:00–06:00 and the rest's own window. It uses the same `rest`
  operation: Expression has no bow clip, so this is the settle into the
  sleeping pose and the asleep face
  (the fold and the dim), and the release before it eased the pose away
  without the wake stretch. No new operation was needed.
- **How often.** At most three a night. Each says in Skills' journal why it
  woke and that it settled again (`woke at a night sound; the voice has
  rested 30 s again and no face is in view, so it settles again (1 of 3)`).
  A fourth sound leaves it awake, and leaving the window ends the waiting.
- **The daily chance.** A re-settle makes no run and no daily claim: the
  bedtime trigger stays spent for the night, so it cannot fire twice. The
  skill's own next operation (a morning wake, a cancel), a reload that
  changes the skill and a restart all end the waiting.

A successful morning request waits for a correlated Expression `completed`
receipt, for at most 12 seconds. `waking` only means the plan is running.
Starting Say earlier would open another Voice turn and cancel the owned
stretch. A timeout, cancellation, reload or changed Expression owner ends
the run before the hum. The original deadline and definition generation
are checked again after receiving the completion and immediately before
presentation, so an already queued answer cannot grant a late start.
The private Say request retains that same absolute `start_before_us`;
Voice expires it against the earlier of that deadline and its usual
read-relative limit. The routine client uses a bounded connection and
checks its generation again before sending. Say still applies its current privacy, quiet,
readiness and interruption rules. No routine operation can wake or release
manual or privacy sleep.

### The second voice route

With `LAMP_VOICE_ROUTE=second`, the authored morning still asks the local
Say service for `cue = "hum"`. Say makes the fixed quiet PCM and sends it
through the ordinary guarded speaker path; it opens no speech-to-text,
chat or speech-synthesis request. The evening and bedtime defaults contain
no spoken line. This skill needs `LAMP_SAY=on` for its presentation, in
addition to the routine controls above.

An authored replacement such as `say = { text = "Good morning.",
priority = 8, expires = "1h" }` uses Say's written-turn path after the same
owned wake completion. On the second route its instruction goes to the
text model and the answer to speech synthesis, without speech to text.
The model is asked to say the exact words; this does not establish that a
live model or a physical speaker produced them exactly.

`LAMP_SCHEDULE_SPEAK` controls due reminders from Schedule, including
`say = { from = "schedule" }`. It does not gate this skill's local hum or
its own `note`/`text` presentation. Neither feature relaxes Say's existing
quiet, privacy, readiness, expiry or complete-playback checks. A `spoken`
receipt still requires the entire admitted sound or answer to finish.

`lamp-skills list` validates the on-disk definitions independently of the
running service's switches. It can list an enabled routine that the service
excludes while `LAMP_SKILL_ROUTINES` is off; the list alone is not evidence
that the routine is active.

## Local windows and daily chances

For an overnight window, early hours belong to the date on which that
window began. A spring clock change can skip a window; an autumn repeated
hour does not grant a second daily chance. Time outside the window does not
count toward its required duration.

Live daily and windowed triggers, routine admission, and Skills' cap scheduling
require both a wall time from 2020-01-01 UTC through the end of 2099 and
positive synchronization evidence from `lamp_ipc::clock_synced()`. On Linux,
this reads the kernel's synchronization status and error bound without
changing time. A plausible date alone is insufficient. Missing, negative or
unavailable synchronization evidence makes scheduled time unknown; losing
trust invalidates the last trusted sample and pending windowed approvals.
There is no implicit non-Linux bypass. `--trust-clock` is an explicit
development/test override, false by default, and still requires the sane
date range. Product units do not set it. Legacy `[quiet]` hours continue
using their existing raw-clock behavior; ordinary and disabled skills add
no kernel-clock queries.

When enabled schedules first lack a configured, trusted local clock, Skills
logs `lamp-skills: scheduled skills are waiting for a configured, trusted local clock`.
It logs once per such interval, not on every tick. A trusted sample or disabling
all schedules resets that diagnostic. Ordinary and disabled schedules stay
silent. An already-admitted Rest has the bounded monotonic continuation
described below; missing trust never starts a new or pending Rest/Wake.

`clock/local` is sampled inside the existing Skills loop. Its value carries
`known` and, when trusted and recent, `day` and `minute`; Skills does not
subscribe to an external publisher of that name. The engine refuses a
scheduled sample more than two minutes old. Clock triggers use the same
conditions and Policy gate as sensor triggers.

Named daily chances use `LAMP_SKILLS_DAILY_FILE` (or `--daily-state PATH`) in a dedicated private directory
(mode 0700). The ledger is mode 0600, bounded to 128 skill/trigger pairs and
1 MiB, and locked to one process. Schema 2 keeps exact claimed dates in a
fixed bitmap covering the supported clock range, including local-zone and
overnight boundaries: 2019-12-30 through 2100-01-01, 29,223 possible dates
per pair. A mistaken future claim no longer consumes the intervening dates:
a corrected, unclaimed date can run, while every already-claimed date stays
consumed across reload, restart and repeated clock changes. No key or date
is evicted to make room. The full supported calendar for all 128 pairs fits
within the file bound.

The claim is durable before a Policy request or output leaves the engine;
file and directory sync precede success. Refusal, interruption or cancellation
does not refund it. Opening a schema-1 ledger preserves its original bytes.
On a later successful new claim it migrates to schema 2, conservatively
keeping every supported date through each old highest date consumed. Schema 1
discarded the earlier individual history, so a mistaken future high-water
date in an old file cannot be repaired automatically without risking a
replay. New schema-2 claims do not extend that legacy consumed prefix.

Malformed or newer-schema state is left intact and daily starts fail quiet.
Added stored fields are tolerated; known fields, bitmap length/content,
date bounds and unique keys remain validated. A daily storage/claim fault
is latched: another tick, date change or manifest reload does not retry it.
Repair storage and restart Skills so it reopens and validates the same
journal; an embedding engine can explicitly install a successfully reopened
journal. Ordinary skills continue if the daily ledger cannot be opened.

An accepted definition reload invalidates pending scheduled runs, routine
admission requests, presentation permits and any wake in progress. Run
threads carry the definition generation captured before they started, so
a late old thread cannot request rest or presentation after reload. The
manager retains only an already-admitted Rest whose complete enabled owning
skill definition is unchanged. Installing an unrelated skill therefore does
not end that rest; changing, disabling or removing its owner does. Retention
does not renew its lease or preserve an old wake/presentation permission.

## Leased rest and stronger reflexes

The [generic Expression contract](expression.md#leased-skill-rest-requests)
uses one `skills` request publisher with a global sequence. Each payload
identifies the skill, publisher session, target Expression session and an
increasing episode. Skills renews every second; each lease lasts at most
three seconds. Admission and renewal require an original observation less
than 500 ms old. Expression discards a stale, unexpired same-episode Rest
renewal while retaining the exact accepted lease and receipt; that discard
never extends the lease. A renewal does not restart settling or the stretch.

A state or receipt at least 500 ms old cannot admit, complete or renew a
request. A late or missing sample alone does not immediately cancel an
already-admitted rest: Skills withholds renewal while its existing lease
runs out, at most three seconds after the last sent request. Fresh matching
evidence before that expiry can permit renewal again. This grace adds no
time to the original lease. A changed owner, terminal receipt, explicit
cancellation or closed window still ends the episode.

A refused request's receipt says why in its `reason` (lost input, a stale
observation, the owner unavailable or privacy-muted, or the transition
unavailable), and Expression's journal says it once for each new reason, so
a refusal seen on a lamp can be traced to its cause.

If wall-clock trust disappears after Rest admission, Skills may continue
that Rest against a same-host monotonic deadline derived from the last
trusted sample's remaining local-window time and original monotonic sample
time. Delayed delivery cannot move that deadline later. It still requires fresh,
matching Expression evidence for every renewal. This cached deadline does
not predict daylight-saving, offset or timezone changes during the outage;
a new trusted sample updates
the window only while the prior deadline remains live. An expired deadline
cannot be revived by clock correction. Pending Rest, Wake and new transitions
still require trusted time. The 24-hour episode cap remains authoritative.
Each published non-cancel lease is bounded by the window and episode
deadlines known at publication, so it expires independently if Skills stops
ticking. A later clock correction can shorten the cached deadline. Fresh
matching evidence permits immediate publication of that shorter lease;
without it, the previously issued lease retains its original expiry until
cancellation or a valid shorter publication. A correction cannot
retroactively shorten a lease already held by Expression.

Expression binds an admitted episode to its current fresh Voice owner and
measured joints. A new owner, lost evidence, pause, privacy closure, manual
sleep, stronger reflex, request-stream gap or expiry ends that ownership.
Accepted, refused and terminal episodes cannot be replayed. A newer Skills
publisher retires the previous publisher's identities across all skills.
History is bounded to 32 skill/session entries; it is never evicted to
admit a replay. A full history refuses new identities until Expression
restarts with a new target session.

The existing name, touch, reminder and conversation reflexes remain local.
They can supersede routine ownership; cancelling that routine cannot undo
their wake or manual sleep. Routine cancellation releases only its own
pose smoothly, without playing another wake clip. Expression reports
planning state, not proof that Body moved or unfolded.

## Durable output ceilings

The night volume cap is on the software volume Actions already owns:
`Speaker` on `allwinnerhdmi`, the mixer `volume_up` and the app slider
change. The C-Media card stays where the hardware team set it. Two caps in
series, one on each card, would multiply and leave the night voice
inaudible (the first builder's ruling on #1672), so the routine adds no
second mixer.

Limits are restrictions, independent of light request priorities. All
active restrictions combine by minimum with the product ceiling. Volume
entry lowers an already louder mixer, preserves a quieter choice, and
never unmutes. All volume setters, including the app slider and unmute,
honor the effective ceiling. Any remembered pre-mute level is clamped too.
Relaxing or removing a restriction never writes back an old setting. If
entry reduced the mixer volume, that reduction remains after release until
a later explicit volume action changes it; release does not restore a saved
user preference.

Actions stores at most eight stable source identities, including their
terminal records, in a bounded 16 KiB state. A cap contains `source`,
`token`, `issued_unix_ms`, `valid_until_unix_ms`, and at least one of
`light_channel_max` (12–120) or `volume_raw_max` (1–100). New puts require a
known initialized wall clock, an issue time no more than five seconds old,
and a deadline at most 24 hours after issue. Exact retries observe the
existing result without extending the deadline. Changed retries, older
replacements and token-mismatched removal are refused.

Active restrictions survive an Actions restart or unknown clock. Unknown
time cannot expire them; already expired or cancelled records cannot be
revived by rollback. Explicit removal of the current matching token works
even with unknown time. Skills recovers the exact accepted tokens after a
restart and removes its own restriction when the window ends or the
definition is disabled or deleted. Skills requires the synchronized clock
described above for clock-driven creation or window-end removal. With a
definition still present, unknown, unsynchronized or regressed time does not
make Skills remove its restriction; explicit disable still releases only
its exact token. Window endpoints are resolved in the configured zone,
including skipped and repeated hours. This trust check governs Skills'
scheduling; Actions and Ring retain their initialized-time expiry rules.

The existing private Actions socket accepts these strict operations:

| Operation | Request fields | Meaning |
|---|---|---|
| `caps_put` | `cap` containing the fields above | Durably accept or observe an exact retry. |
| `caps_remove` | `source`, `token` | Remove only that source's current token. |
| `caps_get` | No additional fields | Read the current durable snapshot and effective minima. |

These operations are not model tools. Unknown fields or operation values
are refused. A successful mutation distinguishes durable `accepted`,
mixer `volume.confirmed`, and `ring.published`; `ring.confirmed` remains
false. Publication is not optical confirmation. Hardware failure leaves
the software restriction active and reports unconfirmed application.
Storage uncertainty requires reopening the owner before further success.
A concurrent stop cannot turn a recorded storage failure into a successful
exit; shutdown checks that state without additional storage or output work.

Actions clears its `ready` marker before startup recovery. With caps
enabled, readiness requires the exact current snapshot to be kept on the
bus and any required volume clamp to be confirmed. Failed ordinary volume
operations invalidate that confirmation. Snapshot publication, expiry and
failed-clamp retries use the existing service loop at most once per second.
A failed required clamp or publication withholds readiness; accepting a
private request is not a claim that the lamp is ready.

## Ring startup and retained limits

With output caps enabled, the ordinary ring starts dark until it accepts a
valid complete `output/caps` snapshot from Actions. The snapshot binds the
host boot, publisher session, sequence, monotonic stamp and durable
revision. Wrong identities, stale or future observations, lower revisions,
and conflicting equal revisions cannot loosen a limit. Failed intake stays
on the enabled, dark path rather than reverting to unrestricted output.
When Actions holds its caps back because their store is from a later build
(`actions.md`), it still publishes an empty snapshot each second, so a ring
that starts with it takes the product's ceiling rather than staying dark.
That isn't a failed intake: the snapshot is valid, and the caps are off for
as long as that build runs, which can be days if an update stays put back.
A ring dark all that time would be worse than one without night limits, and
Actions' health says why. A ring still running from before keeps its last
snapshot until those caps expire.

The ring retains an accepted snapshot through bus silence. Initialized wall
time can expire an individual cap. Unknown or implausible wall time, a cached
clock sample at least three seconds old, or monotonic time preceding that
sample retains restrictions. This does not detect every plausible backward
wall-clock adjustment or establish synchronization. A newer valid complete
snapshot can explicitly release them, including with unknown wall time. No frame receipt refreshes
a cap's deadline. The intake reads at most every 20 ms on a separate
thread; the output loop uses a nonblocking cache read. Tightening blanks
already-latched brighter pixels even if no new frame arrives.

Ceilings apply to ordinary turn and rest output. Existing separate asleep
and tap feedback peaks at 12, and privacy feedback at 10, within the minimum
accepted cap of 12. The existing user light-off behavior still includes its
small amber readiness ember (one pixel in four, RGB channel values 3, 1, 0);
it does not mean complete darkness. None of these limits grants input
readiness or bypasses privacy and output-owner checks.

## Export privacy

Rest requests, rest receipts and output-cap snapshots are excluded from
exports; only omitted-message counts are retained under their known topic
names. Skill identities, tokens and payload fields are not copied.

## Host evidence and required lamp comparison

The [host verification record](verification/bedtime-morning-host-20261019/README.md)
records complete gates across all 42 crates and subsequent integration
checks for every changed crate and reverse consumer, together with
formatting, strict all-target workspace Clippy and the source-only
workspace build. Update runs its entire suite
in the repository checkout because an inherited packaging test reads a
parent image fixture; the other 41 suites run from the standalone source
copy. The record gives exact commands, per-crate counts, source hashes and
earlier failures.
Native ARM64 results belong to the published PR head. These checks establish
software behavior, not lamp qualification.

A fake-sink test checks a tighter cap's bus publication to a black frame in
less than 150 ms, before the prior permit expires. That boundary is host
software and the fake sink, not optical darkness on a lamp.

Designed boundaries are the existing Skills observation cycle, a 20 ms
Expression planning cadence, 200 ms planning receipts, one-second rest
renewals, and the cap intake/update cadences above. The rest wait runs on a
skill run thread; cap socket and mixer work run off the Skills observation
loop. Durable daily claims still perform local sync before admission. A
mixer read, write and readback can each take the existing 800 ms timeout:
about 2.4 seconds plus process overhead is a failure budget, not measured
application latency.

Before enabling the core skill, record a lamp comparison covering:

- The default-off baseline, first use and repeated use; evening appearance,
  the small stretch, and the actual sound and onset of the hum.
- Real morning arrival with and without routine-owned rest; repeated faces,
  a consumed chance, restart, unrelated and owning-definition reloads, a
  missed window, a forward date jump and correction, and repeated dates.
- Name, touch, reminder and conversation interruptions during rest and
  stretch; manual sleep, physical privacy, pause and folded-body cases.
- Cap entry above and below current settings, user changes while capped,
  mute/unmute, overlapping restrictions and release without restoration.
- Actions/Skills/Expression restarts, unavailable or delayed dependencies,
  unsynchronized time after a power cut, loss and recovery of clock trust,
  durable recovery, a missed receipt within and beyond the rest lease,
  ring startup darkness, and measured request-to-mixer, optical, motion and
  acoustic response.

Keep the skill disabled until those observations establish acceptable
behavior. No device access, acoustic measurement, optical measurement,
physical-motion result or complete installed-system result is established
by the host tests.
