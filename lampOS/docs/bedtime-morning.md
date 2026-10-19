# Bedtime and Morning (#1672)

The product installer includes the [core skill](../skills/bedtime-morning/skill.toml)
at `/var/lib/lampOS/skills/bedtime-morning/`, disabled with `enabled = false`.
It copies `skill.toml` and `SKILL.md`, excluding `tests/`: seven product skills
are installed and six are enabled. The additional core Light out, Light scenes,
Point the light and Light show folders remain outside the product install
pending their own qualification (`install::NOT_PRODUCT_SKILLS`). Bedtime's
hours, movements, sound and ceilings are authored defaults awaiting a lamp
comparison. Host validation does not
authorize an installation or establish physical, optical or acoustic behavior.

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
Skills behavior also needs a configured local zone or offset and a durable
`--daily-state` file. Actions requires an explicit persistent `--caps-dir`
and `--bus-root` when caps are enabled; these are absolute paths without dot
components. Its caps directory is separate from the volatile runtime
`--dir` and from other stores.

The [product units](install.md) supply
`--daily-state /var/lib/lampos-skills-state/daily.json` to Skills and
`--caps-dir /var/lib/lampos-output-caps` to Actions. Their respective
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
their own default-denied grant, independently of presentation. Removing that
grant can still release this service's already-owned exact tokens. Accepted
cap definitions carry a generation: a reconciliation overtaken by reload
cannot send a put or removal based on the old definitions. The final socket
write checks that generation without holding its lock over connection,
readiness waits or replies. A complete request sent before reload may still
finish in Actions; the new generation reconciles any owned token rather
than claiming that in-flight work was rolled back. Disabled core folders
remain listed and protected from removal or replacement, while contributing no active triggers, read subscriptions or catalogue functions.

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

## Local windows and daily chances

For an overnight window, early hours belong to the date on which that
window began. A spring clock change can skip a window; an autumn repeated
hour does not grant a second daily chance. Missing, implausible or stale
time cannot start a timed trigger. Time outside the window does not count
toward its required duration. The initialized wall-clock range is a sanity
check, not proof of clock synchronization.

`clock/local` is sampled inside the existing Skills loop. Its value carries
`known` and, when known, `day` and `minute`; Skills does not subscribe to an
external publisher of that name. Clock triggers use the same conditions
and Policy gate as sensor triggers.

Named daily chances use `--daily-state PATH` in a dedicated private directory
(mode 0700). The ledger is mode 0600, bounded to 128 skill/trigger pairs and
64 KiB, and locked to one process. It records the highest consumed date
before a Policy request or output leaves the engine. File and directory
sync precede a successful claim. Reload, restart or clock rollback cannot
grant the same date again. Malformed or newer-schema state is left intact
and daily starts fail quiet; added fields within the current schema are
tolerated. Ordinary skills continue if the daily ledger cannot be opened.

A definition reload cancels outstanding routine ownership. Run threads
carry the definition generation captured before they were started, so a
late old thread cannot request rest or presentation under a new definition.

## Leased rest and stronger reflexes

The [generic Expression contract](expression.md#leased-skill-rest-requests)
uses one `skills` request publisher with a global sequence. Each payload
identifies the skill, publisher session, target Expression session and an
increasing episode. Skills renews every second; each lease lasts at most
three seconds and must arrive with its original observation less than
500 ms old. A renewal does not restart settling or the stretch.

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
Relaxing or removing a restriction never writes back an old setting.

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
definition is disabled or deleted. With a definition still present,
unknown or regressed time retains its restriction. Window endpoints are
resolved in the configured zone, including skipped and repeated hours.

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
  a consumed chance, restart, reload, a missed window and clock changes.
- Name, touch, reminder and conversation interruptions during rest and
  stretch; manual sleep, physical privacy, pause and folded-body cases.
- Cap entry above and below current settings, user changes while capped,
  mute/unmute, overlapping restrictions and release without restoration.
- Actions/Skills/Expression restarts, unavailable or delayed dependencies,
  unknown time, durable recovery, ring startup darkness, and measured
  request-to-mixer, optical, motion and acoustic response.

Keep the skill disabled until those observations establish acceptable
behavior. No device access, acoustic measurement, optical measurement,
physical-motion result or complete installed-system result is established
by the host tests.
