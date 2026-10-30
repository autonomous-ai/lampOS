# Preparing to unplug (#1674)

The core `prepare-unplug` skill runs when the person explicitly asks to unplug
or turn off the lamp. It is installed with the product, but its tool is not
advertised unless Skills runs with `LAMP_UNPLUG=on`. Body, Expression and Voice
need the same opt-in. All defaults stay off until a physical lamp trial passes.

`body.prepare_unplug` is reserved to a `kind = "core"` skill declaring the
core-only grant `core = ["prepare_unplug"]` in `[permissions]` (the shipped
`prepare-unplug`, tool `prepare_to_unplug`). Packages cannot install, replace
or remove it, and an installable or add-on declaring the grant is refused. The
loader rejects a skill without the grant, as well as automatic triggers and
additional steps. Runtime admission rechecks the grant, and the bus request
boundary refuses a caller without it before reading or writing the latch. The
runtime never knows the operation by the skill's name: a renamed copy without
the declaration gets nothing of it. This is an operation restriction within the trusted local skill runtime,
not cryptographic authentication or proof of a person's spoken consent.
Installed skill definitions and callers of the local tool socket remain trusted;
this check does not exclude another local process with bus write access.

The composite step requires `body = true`, `speak = true`, and explicit read
grants for `body/unplug_request`, `body/unplug_state`, `expression/unplug_state`
and `ring/unplug_state`. The loader validates every grant; runtime admission
and the dedicated request/ready-speech paths check the run's captured grants
again. Failure speech also requires `speak`. No light action or camera access
is granted by this skill.

Permission lifetime follows the existing Skills contract: a run captures its
skill's grants when started and may finish on them even if the catalog reloads
or the skill is removed meanwhile. Reload affects future admissions; it does
not revoke an already admitted 32-second unplug run. The delayed speech uses
that same snapshot. Independently, live physical readiness still expires and
Voice cancels its completion reply if the body's proof is lost. This feature
does not add a new revocation protocol or change ordinary skill lifetimes.

**While the tool is not offered** (`LAMP_UNPLUG` off), the model is told it
cannot get itself ready to be unplugged: told "I'm unplugging you", it never
says OK as if it were ready (launch lamps did, four times). Only with the
app's off button on (`LAMP_SHUTDOWN=on` in the voice's unit, `system.shutdown`,
cloud.md) does it say to turn it off from the app first and unplug it once it
is off; without it, it says plainly that pulling the plug is the only way for
now, and names no app (`lamp-voice`'s `without_a_tool`, the first builder's
option (b) on #4595). The voice hands the switch to its provider worker as
`--shutdown-from-app`, as it does `LAMP_SLEEP_ASKED`. It is told this on the
Live route while its session has no `prepare_to_unplug` (on its own or
through `use_skill`), and on the second route while the skills' catalogs
offer none; with the tool there it is not told.

This prepares the arm; it does not run an operating-system shutdown or cut
power. Turning the lamp off safely, with the body let go and a light that says
it may be unplugged, is shutdown.md. An ordinary bedtime request belongs to the bedtime skill.

## From the request to the answer

1. The model calls `prepare_to_unplug`. The tool admission says preparation is
   starting and explicitly says `ready: false`. It never says the arm is safe
   merely because the request was accepted.
2. A dedicated skill run writes one kept `body/unplug_request`, carrying a
   request identifier. Repeated calls keep the original request. The body's
   input thread polls it every 50 ms; the 20 ms motor loop reads an atomic
   latch without waiting for bus I/O.
3. The latch expires the ordinary wake window immediately. The existing
   calibrated, guarded folding trajectory runs; moods, name calls, gaze,
   gestures, Expression and idle motion cannot stand the lamp up. This is not
   `lamp/pause`: ordinary pause can leave the 90-second wake window open.
4. Body alone proves physical settling. Its `body/unplug_state` says
   `folding`, `ready`, or `failed`, with the same request identifier and a
   reason on failure. It is refreshed every 50 ms and expires after 500 ms.
5. Expression publishes the existing resting presentation with the `asleep`
   and `ready to unplug` behaviours only for a matching fresh Ready. Its
   `expression/unplug_state` acknowledges that publication. When Expression
   has no light of its own to send (its last voice turn was cancelled, or
   voice timing is not fresh), it acknowledges that it presents asleep and
   asks nothing of the body. Without this, lamp-52e6 never acknowledged a
   Ready body (2026-10-20). In shadow it never acknowledges. The active voice
   ring worker renders the asleep pixels through its existing permission and
   sink-write checks; `ring/unplug_state` acknowledges a successful write.
   The ring worker starts with a cleared environment, so Voice hands it
   `LAMP_UNPLUG=on` (and no other worker), as it does each worker's switches.
   There is no second ring writer and no synthetic privacy/mute event.
6. Skills requires all three fresh, matching receipts before asking Voice for
   the skill's `ready` line, “My arm is folded. You can unplug me now.”
   Every line it says is the skill's own, in `skills/prepare-unplug/skill.toml`'s
   `[unplug]` (`ready`, `failed`, `stay`, `timeout`, `check`), read only
   with its core grant (#940, the first move of "Everything is a skill",
   21 Oct). A file from before `[unplug]` says the words lamp-skills kept
   before, for this release only, and the journal says so. The request carries
   `unplug_request`; Voice keeps that completion guard while the reply is
   queued or under way. An ordinary tool answer or an old asleep frame is
   never readiness. Privacy, playback ownership and the voice's own gates
   remain authoritative.

The tool response target is under 150 ms on a warmed lamp. This target does
not include speech recognition or the model deciding to call the tool. Body
intake plus its next step targets 70 ms under normal scheduling (50 ms poll +
20 ms motor step). Folding uses the lamp's configured trajectory and needs a
further 500 ms of physical settling. These are design targets, not device
measurements. Body fails preparation if it is not Ready within 30 s (a Ready proven again after a gap is not held to that limit); the skill waits at most 32 s
for completion and the asleep presentation. Readiness speech uses priority 8
(the existing one-second quiet rule) and a 500 ms queue expiry; an expired,
unbegun attempt can be retried only inside that bounded wait, with proof read
again each time. A begun or dropped line is never replayed automatically.

## What counts as physically folded

The trajectory fraction reaching zero is insufficient. The body requires
at least 500 ms of contiguous, fresh readbacks with:

- all five joints within 24 encoder counts (the body's rest slack,
  `REST_SLACK`) of their configured folded targets: lamp-52e6's elbow rests
  13 counts past its folded goal every time it folds;
- movement no more than two counts per 20 ms step;
- no hand hold, tipping, active gesture, limp state or status fault;
- no invalid position or temperature substituted with a previous sample;
- each joint's reported load below 100 and temperature below 55 C.

The fold goes to the configured home, never to a runtime home: once the
latch is set, a head or base turned aside by a look, a gesture, Expression or
a rest kept where it was put is brought back to the configured head and base
home before readiness is judged (lamp-52e6, 2026-10-20: the base rested 60
counts toward the person and Ready never came). A home shifted at a `--limp`
start is not attested as the fold.

The existing arm windows and 50-count per-write guard (#1192) still judge every
write. No skill supplies a servo goal or relaxes a bound. A blocked arm or
invalid read does not become ready just because time passed. After Ready, a
step judged unsafe permanently fails that preparation, and the failure names
the first check that failed (`unplug::UNPLUG_UNSAFE`, e.g. "physical readiness
was lost (a joint moved)"). A gap in the evidence is not a step judged
unsafe: a step with a read missed or bad (a substituted position) or a goal
not yet sent is not sampled, and no fresh sample for 100 ms (the
motor thread late) withdraws Ready (`folding`) until 500 ms of fresh safe
samples prove it again. Voice cancels the readiness line meanwhile, as for
any loss of proof. On lamp-52e6 on 2026-10-20 a second voice turn during the
readiness line ended in "physical readiness was lost" for good; host tests
show that nothing a turn sends (moods, a face, Expression offsets, a
gesture) moves a latched lamp. At 09:46 the same day one bad read while the
readiness line played failed it ("a read was bad or a goal unsent"), which
is now a gap. While latched, lamp-body journals what its evidence says
(`lamp-body: unplug step N: <why>; head/base … arm … loads … temperatures …
bad reads …`): an unsafe step at once, a change between gaps and evidence at
most once a second, and the same again every 5 s.

These conservative tolerances may refuse a real lamp. Do not relax them to
make a test green: record the physical fold, calibration, loads and timing and
review the evidence first.

## Lifetime and restart

Once a request is accepted, only a matching cancel ("never mind", below) or a
reboot brings the arm back. No other request, service restart or switch change
clears the latch: the body refuses `torque_write(_, 1)` while recovery is
inhibited.

The latch is not cleared by a cancelled speech, skill reload or completion.
With the default volatile bus at `/run/lampos-bus`, an uncancelled request
stays until reboot clears that bus. If `LAMP_BUS_ROOT` points to persistent storage, reboot
alone does not remove the request; a persistent bus is not a supported automatic
recovery path for this first version. No software resume command is provided.
A failed preparation must be inspected before an operator plans recovery; never
clear the request merely to make a startup pass.

A kept or malformed request with no matching cancel makes a restarted body
refuse startup before actuator access, **even when `LAMP_UNPLUG` is now off**.
A kept request with its matching Skills cancel starts normally, unless the
body's last state for it was Failed. Removing the switch is
not a way to clear accepted intent. Off with no kept request still creates no
live adapter, publisher or polling worker; startup only checks the kept path.
An unreadable bus/request fails closed rather than assuming no latch exists.
Startup failures report the kept request and inhibition, without promising that
reboot clears a custom persistent bus. Requests during enabled startup or
recovery also inhibit automatic waking.

Topics, all kept Latest:

| Topic | Publisher | Payload | Lifetime |
| --- | --- | --- | --- |
| `body/unplug_request` | skills | `{request}` | kept; reboot clears the default volatile bus |
| `body/unplug_state` | body | `{request, phase, why}` | 500 ms |
| `expression/unplug_state` | expression | `{request, asleep: true}` | 500 ms |
| `ring/unplug_state` | ring | `{request, asleep: true}` | 500 ms |
| `body/unplug_cancel` | skills | `{request}` | kept; matches one request only |

While already running, Body accepts a request only from the `skills` source
label. A readable foreign-source header is ignored before parsing its payload,
so even a malformed foreign payload does not initiate folding or readiness.
This label is a provenance check inside the shared-trust bus, not authentication:
a writer with bus access can claim the same label. On restart, **any** kept
request still inhibits startup before actuator access, regardless of source or
payload validity. An unreadable live wire message or malformed `skills` request
retains the existing fail-closed inhibit.

Receipt readers check source, request identifier and monotonic age; none uses
the bus expiry grace to extend readiness. The Expression receipt confirms its
asleep presentation (a bus publication, when it has a light to send); only the
ring receipt confirms a successful sink write.

## Never mind: cancelling an unplug that doesn't come (#2069)

Two paths publish the same kept `body/unplug_cancel {request}` from Skills,
and the body has one path for both:

- **Spoken.** `stay_plugged_in` is a second reserved tool of `prepare-unplug`
  ("never mind", "don't unplug", "I'm not unplugging you", "stay on"). The
  model recognises the words; there is no keyword list. It is in the
  skills' `tools.json` whenever `prepare-unplug` is loaded, latch or not, so
  a Live session that asked for the unplug already has it (Live takes a
  changed catalog only between conversations); with no Skills request in
  force, or one already cancelled, the call is refused. No skill may declare
  that tool name.
- **Timeout.** After `LAMP_UNPLUG_STAY_MIN` unbroken minutes Ready (default
  10, whole minutes over zero) with the same Skills process still up, Skills
  cancels once with "I'm still plugged in, so I'm getting back up." The timer
  starts only for a request that process made, and looks every 500 ms; a look
  that is not Ready (stale, or proving the fold again after a gap) starts the
  minutes again. It never fires from Failed: a preparation that was Ready and
  then fails (a hand on the arm before any cancel) makes Skills say once
  "I can't get up by myself; I need to be checked.", and it stays down.

Skills checks first that the latch is in force and the body's own state is
fresh (500 ms), names that request, and is Folding (spoken only) or Ready,
never Failed. It then asks Voice for the line ("Okay, I'll stay." or the
timeout's) at priority 8, and publishes the cancel only once Voice says the
line has begun. A line that never begins cancels nothing.

The body checks again itself. It accepts a cancel only if its source label is
`skills`, its identifier equals the request it accepted, and the preparation
has not failed. A foreign, malformed or other-request cancel is ignored and the
latch stays. A cancel during Folding lets the fold finish to its rest pose; it
never reverses mid-fold. The body clears its latch only when it is Ready on
fresh, safe evidence (the same proof as above: no hold, contact, fault or
stale read) and at least 3 s after it saw the cancel, so the line comes
before motion. After the cancel, an unsafe sample (a hand on the arm to
help it up, a joint moved) withdraws Ready as a gap does and never fails the
preparation: the latch holds while it lasts, and the stand waits for 500 ms
of fresh, safe evidence again, so a hand only delays it. That delay is
bounded: a preparation not Ready 60 s after the cancel (an arm resting on a
book, a joint that keeps reading as moved) fails and stays latched, and the
Skills thread that published the cancel, watching the body's state until it
stands, says the same "I can't get up by myself; I need to be checked." once.
Before any cancel, an
unsafe sample after Ready still fails it, and a failed one is never
cancelled. The stand ends only its own request's preparation: a newer request
already in force (a "never mind", then "actually, get ready to unplug" within
the 3 s) is taken at once with the latch held. The motor loop then opens the ordinary wake
window, and the lamp stands through the same guarded rise as when it is
spoken to; a keeper pause still keeps it down. Expression hands the body
back on the same matching cancel. The cancelled request no longer latches or
inhibits startup, and the next `prepare_to_unplug` mints a fresh identifier.

A model call is enough consent to cancel, the same standard as the latch: like
the request, the cancel is not proof of a person's intent, and the body's own
checks are what keep motion safe. A kept matching cancel also lets a
restarted body start normally, unless the body's last state for that request
was Failed (for example a cancel during Folding whose fold then failed, or a
body stopped before it stood): a failed preparation still needs inspection,
and only a reboot clears it.

Host tests cover the cancel at Ready, during Folding, from Failed, stale,
foreign, other-request and with no request, a hand after the cancel (during
and past the 3 s), a new request within the 3 s, restart with request and
cancel, the tool refused with no latch, the line before the cancel, a single
timeout, unbroken minutes Ready, the line after a failure once Ready, and
no proof after a cancel (not failed at 59.9 s, Failed at 60 s, then the line).
A lamp trial must still measure the line-to-stand time, a one-minute timeout,
and a hand on the arm after a "never mind", let go.

## V1 baseline and what remains unverified

V1's `hal/drivers/button_actions.py::shutdown_action` announced, waited five
seconds, called servo release, then shut down. `release_servos` stopped vision
tracking; the animation service moved to rest before releasing torque. V2 keeps
the ordering and suppresses competing writers, but uses guarded Rust motion
and verified readbacks instead of a timed, optimistic “done”. No V1 service is
required.

Host tests cover latched fold despite later speech/name/gesture requests,
invalid read substitution, a stalled arm, foreign-source request rejection,
restart inhibition with foreign or malformed kept requests and real filesystem
errors even with the switch off, stale or mismatched receipts, negative completion, and output permission failures. A lamp trial must still
measure first/repeated request latency, actual settling, a blocked fold,
privacy closure, body restart and the visible/played confirmation. No unplug,
power cut, motor-safety or hardware latency result is claimed by the host tests.

The visible asleep face uses the existing five bottom LEDs, peak RGB
`(12, 5, 1)`, with a four-second breath and the existing hardware channel ceiling.
A person's light-off setting suppresses this override and its receipt: this
version then cannot confirm successful preparation through all three gates.
Voice's readiness monitor reads on the bounded say-client thread every 20 ms;
the coordinator checks only an atomic deadline. Missing reader progress for
100 ms or body evidence older than 500 ms revokes the guarded readiness reply,
including while the model is preparing audio. Those software windows are not
an acoustic stop-latency measurement.
