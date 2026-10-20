# The first five minutes

`crates/voice/src/onboarding.rs`, #841. The privacy batch remains behind
the onboarding switch. Device qualification is outstanding; local evidence
and rollout limitations are recorded with each PR.

## What it is for

A new lamp knows nothing: not where its person sits, not their name or
face, not what they would rather it did. On lamp-4ace a person had to guide
it for twenty minutes. Here the lamp leads that itself, by voice, right
after setup (#806), one short step at a time. It should feel like meeting a
new creature, not filling in a form.

## The steps

The runner in the voice's coordinator owns the order and the timing. Every
line is the model's: each step begins as a written note in place of speech
(`ProviderInput::Written`, as the hello in [speaking-first.md](speaking-first.md)),
and only when the lamp is at rest (nothing heard, nothing owed, a provider
ready, not paused) and a face is in front of it.

| # | Step | The lamp | Ends when | Kept |
|---|---|---|---|---|
| 1 | `where_you_sit` | asks them to sit where they usually sit and look at it | the eyes' largest fresh face (`humans/faces/tracked`) is at least 0.14 of the picture's height with its centre within 0.25 of the middle, held 3 s while the head is still and no hand is on the lamp (every head joint, `base_yaw`, `head_pitch`, `head_roll` on `body/joint_states`, within 6 counts of its goal, slower than 25 counts a second, and the same goals as the last reading; `body/hand` not pushed; each reading fresh, within 0.7 s; any motion, or a reading stale or missing once there is any sign of a body, starts the 3 s again, and a hand not freshly released counts as still there. Any sign stays: either topic present at all (fresh, stale, unparseable, or a read that failed) or any `body/` topic or declaration on the bus. Only a lamp with no body at all goes without that gate): done, and the lamp says so. Short of that, after 8 s of quiet, one note (only one, ever) says what is missing: no face, or "a bit closer" / "turn me a little toward you" with "where you are is fine too". After that note any face held 3 s the same way is done where it is, its framing kept: a person sitting beside the lamp (row 10 on lamp-52e6, the face at x 0.12, y -0.63) would just say "there's fine". After 60 s: failed, and the lamp says they can turn it toward their seat later or redo the setup | the framing (size, centre) |
| 2 | `who_you_are` | asks what to call them; after a clear yes, `remember_my_face` ([eyes.md](eyes.md)) | the model calls `setup_step` | the face in the eyes' people file |
| 3 | `how_you_sound` | placeholder | — | — |
| 4 | `touch` | asks them, in a few words, to touch the top of its head gently, then stays quiet | a head-pad touch from Controls (`controls/touch`, `lamp_msgs/TouchEvent` from `controls`, `source: head`, enabled; `lamp-controls --touch`) after the ask: done. Short of that, after 8 s of quiet, one reminder; 60 s after the ask: failed. The model may only skip it. Not available without the voice's bus (`LAMP_BUS=on`) or the subscription to `controls/touch`; the base strip stays off (#1891) | the outcome only |
| 5 | `privacy` | led first, before step 1, whenever it is available (see "Physical privacy step" below); the core onboarding skill requests a short invitation naming the verified switch location | a delivered invitation, fresh physical close, the voice's verified microphone cleanup, then a fresh same-source open and normal readiness; the camera's close is not checked | bounded attempt evidence and completion identity |
| 6 | `giving_way` | placeholder | — | — |
| 7 | `preferences` | asks about quiet hours and whether it may say hello, one at a time; keeps each as a rule with `keep_rule` (#813) when the provider worker has both `LAMP_MEMORY_DAYS=on` and `LAMP_MEMORY_RULES=on` ([memory.md](memory.md), "What a person teaches and corrects"); without either, or with the legacy memory, there is no `keep_rule` and the model keeps them with `remember` if it has a memory at all and says that is everything | the model calls `setup_step` as soon as it has heard the second answer, before it says anything else (row 10 on lamp-52e6: asked and answered, never reported) | the rules, in memory |

A placeholder is the step of a part another builder owns: hearing (fourth,
#753) and whether the body gives way (fifth, #780). It is recorded as
`not_available` and passed over without a word: repeated "that isn't ready
yet" in a row would
sound like a machine. Each is marked `PLACEHOLDER (#841)` in the code.

A step the model leads ends with `setup_step {step, outcome}`, outcome
`done`, `skipped` (they asked to skip it) or `failed` (it could not be
done; the model then tells them what to try later). Only the step being
led is taken; a report of any other is refused and recorded
(`onboarding_report_refused`). `where_you_sit` is done only by what the
eyes measure, `touch` only by what Controls feels and `privacy` only by
the switch's receipts: the model may only skip them, and the worker refuses any other
outcome with words it can say. Never reported, a step is
failed after 90 s once the room has been quiet for 20 s, and the walk goes
on. Nobody in front of the lamp for 30 s: the step being led is dropped and
asked again when someone is back. The keeper's pause
([keeper.md](keeper.md)) stops it the same way: nothing is begun while
paused, and the step being led is asked again afterwards.

## Movement during onboarding

Onboarding does not request nods or gestures for placement, identity, touch, privacy
or preferences. Only a future qualified giving-way step may move the body while
the person is at the arm. That step remains unavailable until the body owner
supplies its yield/release evidence; ordinary safety and physical controls
continue to own their existing reactions.

## A step that was not ready, offered later

A step recorded `not_available` stays so in the file. When a later voice
has it, the runner offers just that step once the walk is over, at a rest
with someone there, never in the middle of another step. It is led like
any step to its end. Offered and left unfinished (they walked off, the
pause), it is offered again no sooner than a day later
(`offered_at_unix_s`). Its note is its owner's to word, beginning "one more
thing I can do now"; until then a placeholder note says so.

## Starting and redoing

It starts the first time the voice runs with the switch on and no step kept
yet, and someone is at the lamp. A walk left half done (they walked off,
the voice restarted) goes on at the first step with no outcome.

"Lamp, let's redo the setup": the model calls `redo_setup`, every outcome
is forgotten, and the walk begins again at the next rest. Skipping one
step is `setup_step … skipped`; skipping the rest is that for each step.

## Where it is kept

`LAMP_ONBOARDING_FILE` (JSON, mode 0600, written whole and renamed): each
step's outcome, when, and step 1's framing. Only the coordinator writes it.
The provider worker, which runs the model's functions, appends what the
model reported to a mailbox beside it (`FILE.told`), and the coordinator
takes that in twice a second. Once lamp-settings (#808) is merged this
moves there. Nothing leaves the lamp.

Each step is also in the session record: `onboarding_step` (asked, or its
outcome and framing), `onboarding_guide` (a word on where to move it, with
the face), `onboarding_stopped` (paused or nobody there),
`onboarding_redo` and `onboarding_finished`.

## The switch

`LAMP_ONBOARDING=on` and an absolute `LAMP_ONBOARDING_FILE` in the voice
service's environment. The coordinator passes the file to the provider
worker as `--onboarding-file`, and only then are `setup_step` and
`redo_setup` declared to the model. Without both: no function, no file
read or written, no turn
(`tools::tests::without_the_onboarding_switch_nothing_changes`,
`onboarding::tests::the_switch_is_on_only_with_on_and_an_absolute_file`).
While the lamp is leading a step it does not also say the hello.

## Tests

`onboarding::tests`, with a scripted room and face: the steps walked in
order and kept across a restart; a step skipped and the walk redone; a
step that fails saying what to do and moving on; the pause and an empty
room stopping the step being led; the nearest face as its framing.

## Not shown

A lamp run with a person (lamp-52e6, by the coordinator), which is to
measure: seconds per step from asked to outcome, steps redone or skipped,
step 1's guiding notes before the framing held and the face size and
centre reached, whether every line was the model's and short, and the
whole time. Whether the model calls `setup_step` reliably, and whether
"turn me a little toward you" without a side is enough for a person.

## Physical privacy step (#841)

Privacy is an explicitly measured step, not a model-reported success. The
`onboarding` core skill reads `onboarding/pending` and asks the existing say
service for the exact invitation token. The runner retains the words, evidence
and persistence; there is no second conversational runner and no automatic
voice fallback when the skill is absent. Stale, skipped, redone or restarted
attempts cannot borrow another invitation. This bridge adds no model tool and
grants no microphone, camera or body authority.

Privacy is led first, before step 1, whenever it is available, though its
record keeps the table's place. It is available only with a verified switch
location (below) and Controls' privacy receipts, `LAMP_PRIVACY_CONTROLS=on`
(the `privacy_bus.rs` worker): the GPIO
privacy worker sends no receipt, and without one no invitation could ever be
offered, so the step is `not_available` and the walk goes on. The product's
`lamp-voice.service` doesn't set `LAMP_PRIVACY_CONTROLS`.

With the receipt source on, a receipt that never shows the switch open and
fresh (faulted, stale, a reader that stopped) would hold the walk at privacy
with nothing offered. That wait is bounded by the sitting step's give-up time
(60 s, `SIT_GIVE_UP_US`), counted only while nothing is offered or in an
attempt: past it privacy is recorded `not_available`, the journal gets one
`onboarding_stopped` row (`why: no_open_privacy_receipt`) and the walk goes on.
The next open, fresh receipt makes privacy available again and it is offered
then, as privacy always is once available. Never done: the bound adds no
evidence. A restart re-arms the wait.

The runner gives the model a note to word the invitation itself, not fixed
words: the hardware profile's verified switch location, that the switch closes
the microphones and camera, to close it and open it again, and that the lamp
can't hear or see them while it is closed. An invitation is delivered only on the
voice turn's complete playback evidence; queue acceptance is insufficient.
An interrupted invitation can be retried once, then skipped. Software mute,
sleep, an initial open level, and model `setup_step done` are not evidence of
physical close followed by open.

Controls' existing fence provides the publisher, reader and acquisition
identity through the same privacy worker channel. Close observation only
captures bounded evidence in memory. The outer service persists a close only
after the voice's own revocation and verified teardown. That proves the
microphones closed, not the camera: the eyes are told nothing new on a mute
(the permit link only stops saying yes) and close the camera on their own
1.5 s permit timeout, which this step does not observe. Done's note to the
model claims the microphones only. A close read just before the
invitation's end is processed is the person acting early: the attempt is
offered again (within the two-try budget), not failed. The normal no-audio
privacy wait is unchanged. A later voice cycle can complete the step only
with fresh, advancing evidence from the same physical source and its normal
input-readiness gate. Source/clock changes, failed cleanup and missing evidence
leave the step incomplete. An attempt expires after five minutes, evaluated
on reopening; expiry never changes the switch or capture authority.

The product reserves `/var/lib/lampOS/voice/onboarding/` as private state, with
`progress.json` as the canonical progress path. Pending proof and bounded
retry metadata stay beside it. Completion is saved by attempt identity before
pending evidence is erased, so a crash does not repeat an acknowledgment.
A new service invocation discards unfinished close proof, and any
`privacy-pending.json.next-*` or `progress.storage-next` a crash left
mid-write, before its first privacy wait; normal physical-mute cycles within that service retain it.
Software mute or an unreadable source discards prior proof after cleanup. A
failed discard disables onboarding proof for that service while leaving the
normal privacy wait unchanged. Factory reset wipes all contents, including
temporary/mailbox files, and preserves the installed core skill. There is no implicit import from arbitrary
legacy environment paths.

The step also needs `LAMP_ONBOARDING_HARDWARE_PROFILE` naming an absolute path
to a hardware-owner supplied regular JSON file:

```json
{"privacy_switch_location":"<verified location on this hardware>"}
```

This is a schema example, not a location to deploy. No profile is supplied or
guessed by default. The loader rejects symlink files, malformed/unknown fields,
files above 4096 bytes, and empty/control-containing location phrases or
phrases over 160 characters. Missing valid location evidence keeps privacy
unavailable. Read the profile once in storage initialization, not on the interaction loop.
The product path configuration does not enable `LAMP_ONBOARDING`.

Immediate feedback remains independent of progress writes. A single storage
worker belongs to the whole service, including its physical-mute cycles;
progress, invitation budget, checkpoint and mailbox operations use its bounded
ordered queue. The interaction loop only submits or polls without waiting.
There is one operation in flight and bounded pending state, not one thread per
write. Late completions cannot restore an invalidated attempt: an epoch fence
hides old results immediately and a durable invalidation runs after any old
write before the tutorial resumes. No worker completion grants capture
authority. A slow or failed store can defer tutorial progress while normal
voice and physical privacy keep running. A new invitation is not published
before its budget commit, and Done is not acknowledged before its matching
progress commit. The provider keeps appending its bounded mailbox messages in
its separate process; the runner reads them only through the storage worker. The target is acknowledgment within
500 ms after accepted proof. Full device latency, slow-storage responsiveness,
and the physical close/open walkthrough remain unverified until measured on an
exclusively leased lamp. Local tests do not establish those hardware results.

Known limit: the say service trusts the skill name a process gives. A process
in the `lampos` group that reads the invitation token from the bus could ask
for the same invitation; it gets no data, only the lamp saying the invitation
the runner already offered.
