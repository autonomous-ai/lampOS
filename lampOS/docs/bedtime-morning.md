# Bedtime and Morning (#1672)

The product installer includes the [core skill](../skills/bedtime-morning/skill.toml)
at `/var/lib/lampOS/skills/bedtime-morning/`, disabled with `enabled = false`.
It copies `skill.toml` and `SKILL.md`, excluding `tests/`: eight product skills
are installed and seven are enabled. Desk tutor, Desk tutor marks, Light out,
Not feeling well, Light scenes, Point the light and Light show remain outside
the product install (`install::NOT_PRODUCT_SKILLS`). Bedtime's
hours, movements, sound and ceilings are authored defaults awaiting a lamp
comparison. Host validation does not authorize an installation or establish
physical, optical or acoustic behavior.

## Default-off controls and ownership

Three independent process switches accept exactly `on`; every other value
leaves that feature off.

| Switch | Owner and effect |
|---|---|
| `LAMP_SKILL_ROUTINES` | Skills admits routine definitions and runs their rest manager. Disabled manifests still contribute no active triggers or tools. |
| `LAMP_REST_REQUESTS` | Expression accepts leased rest intents and replaces its automatic night/absence and morning/face decisions with skill scheduling. Its existing `--enable` remains necessary for actuator requests. |
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
the new features: all three switches remain unset in the product units,
and the manifest stays disabled with no active trigger or advertised tool.

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
| Evening | 21:00–23:00; a nearby person for 30 seconds | One warm, slow eight-second light breath at level 2, with low level 1. |
| Bedtime | 23:00–06:00; a fresh empty face list continuously for 15 minutes | Ask Expression to sustain this skill's rest, renewable until 10:00. |
| Morning | 06:00–10:00; a nearby person for three seconds | Wake only this skill's still-owned rest; after its planned stretch completes, ask Say for a hum. |

Evening, bedtime and morning each have one daily chance and require Voice to
have reported resting for 30 seconds. They retain the process's existing
`LAMP_SKILLS_ASK` mode. The product unit currently sets `skip`, so it does not
ask Policy. With `policy`, their `nudge` declaration becomes an `unprompted`
request: Policy may refuse it, including silent rest, because its existing
question judges speaking up and its fallback refuses known absence and
hours outside 08:00–22:00. There is no reminder exception or relaxed Policy
rule for this skill. With neither mode, no automatic presentation starts.
Record the ask mode in the lamp comparison; the policy-mode night path is
not qualified by the host tests.

Evening and morning may wait up to one minute for admission. A failed,
interrupted or refused chance is consumed; there is no later catch-up that
day. Missing, malformed or stale faces never count as an empty desk.

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
