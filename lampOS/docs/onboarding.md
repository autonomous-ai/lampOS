# The first five minutes

`crates/voice/src/onboarding.rs`, #841. The privacy, near-call (#1803) and giving-way (#1800) steps remain behind
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
| 3 | `how_you_sound` | the core onboarding skill requests a short prompt: stay in their usual seat and call the lamp's name once (#1803, below) | three accepted calls make a bar and its file is written: done; five prompts can't reach three, or the calls spread over 10 dB: failed | the prompts and accepted tries (`rise_db`, `score`); the bar in `/var/lib/lampOS/voice/near-call.json` |
| 4 | `touch` | asks them, in a few words, to touch the top of its head gently, then stays quiet | a head-pad touch from Controls (`controls/touch`, `lamp_msgs/TouchEvent` from `controls`, `source: head`, enabled; `lamp-controls --touch`) after the ask: done. Short of that, after 8 s of quiet, one reminder; 60 s after the ask: failed. The model may only skip it. Not available without the voice's bus (`LAMP_BUS=on`) or the subscription to `controls/touch`; the base strip stays off (#1891) | the outcome only |
| 5 | `privacy` | led first, before step 1, whenever it is available (see "Physical privacy step" below); the core onboarding skill requests a short invitation naming the verified switch location | a delivered invitation, fresh physical close, the voice's verified microphone cleanup, then a fresh same-source open and normal readiness; the camera's close is not checked | bounded attempt evidence and completion identity |
| 6 | `giving_way` | the core onboarding skill requests a short invitation: take its head gently, move it a little, let go (#1800, below) | in the minute after the invitation's playback, the body says a hand pushed it, a head joint moved more than the body's `HAND_MOVED` (15 counts) from where it was asked to be with its goal held there, and then the hand let go: done; two invitations without that: failed; no body, or the keeper's pause while it is led: `not_available` | the outcome only |
| 7 | `preferences` | asks about quiet hours and whether it may say hello, one at a time; keeps each as a rule with `keep_rule` (#813) when the provider worker has both `LAMP_MEMORY_DAYS=on` and `LAMP_MEMORY_RULES=on` ([memory.md](memory.md), "What a person teaches and corrects"); without either, or with the legacy memory, there is no `keep_rule` and the model keeps them with `remember` if it has a memory at all and says that is everything | the model calls `setup_step` as soon as it has heard the second answer, before it says anything else (row 10 on lamp-52e6: asked and answered, never reported) | the rules, in memory |

A placeholder is the step of a part another builder owns. It is recorded as
`not_available` and passed over without a word: repeated "that isn't ready
yet" in a row would
sound like a machine. Each is marked `PLACEHOLDER (#841)` in the code.

A step the model leads ends with `setup_step {step, outcome}`, outcome
`done`, `skipped` (they asked to skip it) or `failed` (it could not be
done; the model then tells them what to try later). Only the step being
led is taken; a report of any other is refused and recorded
(`onboarding_report_refused`). `where_you_sit` is done only by what the
eyes measure, `touch` only by what Controls feels, and `how_you_sound`,
`privacy` and `giving_way` only by their measured evidence: the model may
only skip them, and the worker refuses any other
outcome with words it can say. Never reported, a step is
failed after 90 s once the room has been quiet for 20 s, and the walk goes
on. Nobody in front of the lamp for 30 s: the step being led is dropped and
asked again when someone is back. The keeper's pause
([keeper.md](keeper.md)) stops it the same way: nothing is begun while
paused, and the step being led is asked again afterwards.

## Movement during onboarding

Onboarding does not request nods or gestures for placement, identity, touch,
privacy or preferences. Only the giving-way step asks the body for one move, a `nod`, once
it is done, after the hand has let go. The step itself asks the body for
nothing: the body gives way to a hand by its own rule (`--give-way`,
[body.md](body.md)); ordinary safety and physical controls keep their reactions.

## A step that was not ready, offered later

A step recorded `not_available` stays so in the file. When a later voice
has it, the runner offers just that step once the walk is over, at a rest
with someone there, never in the middle of another step. It is led like
any step to its end. Offered and left unfinished (they walked off, the
pause), it is offered again no sooner than a day later
(`offered_at_unix_s`). Its note is its owner's to word, beginning "one more
thing I can do now"; until then a placeholder note says so.

## Why a step is not_available

Each measured step needs these, by switch, checked at every poll of the walk:

- `how_you_sound`: `LAMP_SAY=on` (the say socket made), `LAMP_BUS=on` with
  the bus up when the session starts (the ears' subscriptions to
  `name/heard`, `ears/level` and `voice/state`), and onboarding storage
  started.
- `giving_way`: `LAMP_SAY=on`, `LAMP_BUS=on` with the bus up when the
  session starts (the feel's subscriptions to `body/joint_states` and
  `body/hand`), `LAMP_BODY_MOOD_SOCKET` (the body link), and onboarding
  storage started.
- `touch`: `LAMP_BUS=on` and the walk's own subscription to `controls/touch`;
  it is not part of the record below.

The session record `onboarding_inputs` names which one is false:
`{"kind":"onboarding_inputs","says","bus","ears","body","feel","storage","at_us"}`,
booleans only, no paths, recorded while the walk exists, once at the first
poll and again whenever one of them changes (row 10 on lamp-52e6, where
both steps were `not_available` with every switch set).

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

## How you sound (#1803)

The person calls the lamp by name from their usual seat; the lamp learns how
loud that call is at the room microphone, for the busy-near rule's bar
(`lamp-near-call`, [attention.md](attention.md), Fourth's #753). The runner is
`crates/voice/src/onboarding/near.rs`, modelled on the privacy step: one exact
invitation token per prompt on `onboarding/pending` (`step: how_you_sound`),
asked for by the same core skill (a step it does not know, from a newer voice,
is read as `unknown` and its token still asked for: the say service decides), and only the prompt's complete playback opens
a try. A prompt counts once its budget commit lands, before it is published;
one dropped before the model took it (nobody there, the pause) is given back.

**A try.** At the moment the prompt's whole playback is known, the runner reads
the Unix clock: that is the prompt's end. The coordinator's monotonic clock is
used only for the try's deadline and to catch a clock jump; it is never taken
for a Unix time. Evidence comes from the bus, dated by its headers:
`name/heard` (a hearing begins at the header's time less `ago_ms`),
`ears/level` (mic1's level and the room's quiet, every 100 ms) and
`voice/state` (when the lamp spoke). Collection runs to 7 s after the prompt's
end (the 6 s window plus the 1 s rise of a hearing begun at its very end),
keeping 1.5 s of earlier history so the half second of the lamp's own speech
before a hearing is known; the try is judged 0.3 s after that for delivery.
`lamp_near_call::near_try` judges it: one hearing in the 6 s at 0.95 or more,
mic1's largest rise over its quiet in the second after it began, and the lamp
not speaking from 0.5 s before the hearing to 1 s after (speech elsewhere in
the window does not spoil it). The prompt itself counts as the lamp speaking
until its end.

A try is refused, and the person asked again, when the Unix clock moved more
than 1 s apart from the monotonic one over the try, when a source began again
or a `name/heard` was lost during it, or when any message during it was
invalid (a score that is not finite or outside 0 to 1, an impossible
`ago_ms`, a level outside -120..0 dBFS). A valid score under 0.95 stays a
hearing, so the try says it was unsure. No sound, no mic2 ratio and no voice
identity is used or kept.

**The reply is held.** An answer that starts within a second of the name
spoils the try, and a model hearing its name answers ("Yes?"). So while a
try's window is open, from the prompt's end to 7 s on, the voice holds its
reply to the call: an utterance in it that is only the name ends unanswered
when it ends, however long it runs past the window, as a sound does. Only the
name: the step must have heard it (`name/heard`, in the window, beginning no
more than 0.5 s before the utterance's first speech, as the call wait's
`CALL_BEFORE_US`), and the utterance holds no more speech than a call (1.2 s,
`CALL_SPEECH_BLOCKS`, #782). Anything else said in the window, "skip that"
with no name or "Lamp, can we skip this?" with more than the name, goes on as
any other turn (the reviewer on #1964). The rule is the step's own
(`Onboarding::near_holds_reply`, `NearFlow::holds_reply`), read by
`coordinator.rs`'s `near_call_window` on every block of a turn and at its End,
and the `AdmissionStep::End` arm drops the turn. Its End never goes to the
model and nothing of an answer is played, on Gemini Live and on the second
route (`LAMP_VOICE_ROUTE=second`) alike, since both are the coordinator's
`rig.provider`. Its record says `onboarding_near_call` (the endpoint's field
and the turn's outcome), not `not_speech`, so it is not counted as a short
sound. Outside the window, and with no near-call try open, nothing changes;
Stop, mute and physical privacy are not touched. Tests:
`coordinator::tests::second_route::a_call_in_the_near_call_window_is_not_answered_on_either_route`,
`coordinator::tests::second_route::a_line_that_is_more_than_the_name_is_answered_in_the_near_call_window`,
`onboarding::near_flow_tests::the_reply_is_held_only_in_the_try_window_and_the_held_call_counts`.

**The end.** At most five prompts, and the step stops asking once five can't
reach three accepted (`asks_again`). Three accepted calls make the bar
(`near_rise`: the quietest less 3 dB, held to 16–24 dB); the bar and its
tries are written to `/var/lib/lampOS/voice/near-call.json` (0600, whole) and
only after that write completes is the step `done`. Too few accepted calls, or
calls more than 10 dB apart, make no bar: any earlier file is removed, the
step is `failed`, and the rule keeps its 18 dB. The model can only skip it
(`setup_step how_you_sound skipped`); a skip leaves an earlier bar as it is.
Redo forgets the prompts and tries. Nobody there for 30 s or the pause drops
the prompt or try in flight; a voice restart keeps the committed prompts and
accepted tries and goes on. Fourth's #1892 reads the bar
(`LAMP_ATTENTION_NEAR_CALIBRATED`, off by default) and the factory reset
removes the file; this step only writes it.

**Kept and checked.** `progress.json` gains `near_call_prompts` (at most 5)
and `near_call_tries` (at most 3, never more than the prompts). Every load and
commit checks each kept try (finite rise 0–120 dB, score 0.95–1) and that a
kept `done` has three tries that make a bar; a file that fails is refused
whole, as any malformed progress is.

**Latency and the loop.** From the prompt's end to the outcome: the try is
judged in memory on the coordinator's loop (at most 256 buffered numbers)
7.3 s after the prompt; the next prompt waits for the usual 3 s of quiet. After
the third accepted call, the bar's write (`lamp_near_call::write`, a
synchronous write, fsync and rename) runs on the onboarding storage worker; the
loop only submits it and polls. The step is recorded `done` on the poll after
that write completes and is then committed by the same worker. Nothing on that
path waits on a file: a held write leaves capture, the privacy switch and Stop
running (`a_held_near_call_write_does_not_interrupt_capture_privacy_or_stop`,
`a_held_near_call_write_never_holds_the_loop_and_nothing_is_done_before_it`).
The bus is drained without waiting, at most every 20 ms.

Tests: `onboarding::near_flow_tests`, `onboarding::storage::tests::the_near_call_bar_is_written_or_removed_only_on_the_worker`
and the coordinator's held-write test; the record is in
[verification/onboarding-near-call-host-20261019](verification/onboarding-near-call-host-20261019/README.md).
Not shown: a lamp run with a person.

## Giving way (#1800)

The lamp invites its person to move its head gently by hand, gives way, and
says so. The runner is `crates/voice/src/onboarding/giving_way.rs`, modelled on
the near-call step: one exact invitation token on `onboarding/pending`
(`step: giving_way`), asked for by the same core skill, and only the
invitation's complete playback opens a try. It runs the same on Gemini Live and
on the second route (`LAMP_VOICE_ROUTE=second`): the invitation is a written
turn to the coordinator's `rig.provider`, whichever route that is.

**The rule it uses is the body's** (Fifth's #780, [body.md](body.md), "Giving
way to a hand" and "A hand on the bus"): a joint that gave way and is then more
than `HAND_MOVED` counts (15, `lamp_body::HAND_MOVED`, used by
`in_hand_after` in `crates/body/src/body.rs`) from where it gave way was moved
by a hand; one within `HAND_MOVED` of its goal is not in a hand. No new number
is made. From the bus, in the minute after the invitation's end (Unix
milliseconds by the headers, as near-call's; the body's own `since_us` is its
monotonic clock and is never read as a time):

1. `body/joint_states` with no hand on it: each head joint's goal (`base_yaw`,
   `head_pitch`, `head_roll`), where it was asked to be.
2. `body/hand` says `pushed`.
3. While it does, a head joint is more than `HAND_MOVED` from that goal and its
   own goal is within `HAND_MOVED` of where it is: the hand moved it and the
   body holds it there (it gave way, under `return` and `stay` alike; a body
   that pushes back keeps its goal and is not counted).
4. `body/hand` says not pushed: done, and the lamp says so.

A push that does not move the head that far (a bump), a hand still on it when
the minute ends, a reading from before the invitation ended, from another
publisher, unparseable, non-finite or with a missing head joint, a body that
began again or lost a message mid-push (that push is not judged), and a Unix
clock more than 1 s apart from the monotonic one over the try, are not done. A
minute without it asks once more; two invitations without it: failed, and the
lamp says they can redo the setup later. The minute is the one "where you sit"
gives (`SIT_GIVE_UP_US`), and two invitations are the privacy step's budget.

The model can only skip it (`setup_step giving_way skipped`); a skip, a redo,
nobody there for 30 s or the keeper's pause drops the try in flight, so later
evidence is not counted. Without a body (`LAMP_BODY_MOOD_SOCKET` unset), the bus or the
say service it is `not_available`, and the pause while it is led records
`not_available` too; either is offered again later as any such step. The
invitation budget is kept with the walk: `progress.json` gains
`giving_way_tries` (at most 2, a file with more is refused whole), committed
before each invitation is published, as near-call's prompts are; an offer
never handed to the model gives its try back. So a voice restart or a mute's
end, which rebuild the walk, cannot ask a third time: a walk rebuilt with both
spent fails the step. A redo starts the count again. Evidence
is judged as it is drained from the bus (at most 32 a topic every 20 ms, no
waiting); the outcome is then kept by the storage worker like any other, and a
held write neither holds the loop nor says done before it lands.

Tests: `onboarding::giving_way_flow_tests` (a fake body on a scripted clock:
done only by a hand that moves the head and lets go, the model refused; a hand
still on, two unfelt tries failing; a bump, a body pushing back, a restart, a
forged publisher and early evidence not done; a clock jump; no body and the
pause `not_available`; a skip and a redo ignoring later evidence; a held
progress write; a rebuilt walk not asking a third time; a corrupt kept count
refused). The record is in
[verification/onboarding-giving-way-host-20261019](verification/onboarding-giving-way-host-20261019/README.md).
No coordinator test drives an onboarding invitation through the say service on
either route (privacy and near-call have none either). Not shown: a lamp run
with a person, and the latency from the hand letting go to the lamp's word.
