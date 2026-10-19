# Voice identity: shadow measurement (#1675)

Stage 1 supplies enrollment and measurement infrastructure. It does not yet
recognize a speaker for conversation or memory. Issue #1675 remains open for
real-office measurements and safe request-bound identity routing.

This is system identity infrastructure, an exception to implementing new
behaviors as skills. It adds no autonomous motion, sound, light, greeting or
speech. A diagnostic candidate never changes attention, the provider session,
the conversation thread, the eyes' known-person file, or the memory scope.

## Enabling measurement

Only `LAMP_VOICE_PERSON=shadow` enables the voice worker. `on`, other values and
an absent setting leave it off. Both `LAMP_SAME_VOICE_MODEL` and
`LAMP_EYES_WHO_SOCKET` must also supply the existing model and eyes socket.
Only the Live provider and second-route processes receive the shadow switch;
capture and presentation workers do not. No repository deployment unit enables
it, including the product and lamp-4ace units.

`crates/voice/src/voice_person.rs` runs a separate supervised `lamp-same-voice`
child using the existing [CAM++ model](same-voice.md). The `serve-pinned` child checks the model's 29,596,978-byte size and SHA-256
on every load, including restarts, then parses those same verified bytes; a
mismatch leaves shadow recognition unavailable. Verification never runs on the voice loop.
The attention check's
child and its existing policy remain independent. Loading, inference and the
eyes socket exchange run outside the interaction loop. There is one queued
job; busy work is dropped instead of making speech wait. A model answer is
bounded by the existing two-second deadline; the shadow match RPC has a
200 ms deadline. The host bookkeeping target is under 1 ms per accepted audio
block; this excludes asynchronous inference and RPC. Missing model, unavailable child, unavailable eyes or missing
print leaves recognition unknown and ordinary voice work continues.

## The sample belongs to one request

Only an admitted spoken request supplies audio, after the worker's authority
and privacy checks and accepted input. A line begun over the lamp's audible
voice is ineligible. Audio is 16 kHz mono PCM, limited to the first three
seconds; fewer than 1.5 seconds is insufficient. A line marked as a sound
rather than speech is discarded. These gates do not prove that the room had
only one human speaker or that an arbitrary noisy recording is clean; those
cases must be measured.

Printing starts asynchronously after End. The print is usable only for that
exact request, while it is still current, within ten seconds of End. A newer
request, retirement, cancellation, privacy stop, forgetting or worker exit
invalidates the evidence. Late results cannot restore it. Live replay gaps or
refused input invalidate the shadow sample. Written requests never enroll.
No previous request's print is borrowed for a short "yes".

The PCM is transient in the current sample and bounded inference job; this
feature writes no recording. Successful enrollment persists a biometric
embedding, not PCM. No PCM, raw name or embedding is put in the shadow journal.
The journal contains request id, hashed candidate id or `unknown`, cosine, runner-up score, match-service availability and
End-to-diagnostic elapsed milliseconds.

## Explicit enrollment

The existing `remember_my_face` tool gains optional boolean `voice` only in
shadow mode. Its description tells the model to ask explicit permission to
remember **both face and voice**, and set `voice:true` only after a clear yes.
A name, a request to remember a face, or the checked-name flow is not voice
permission. Name validation and confirmed replacement still apply.

The model cannot supply an embedding. The trusted worker obtains it from the
same ended spoken request, then attaches it to the face RPC. A missing, short,
stale, invalid or not-yet-ready sample does not block face enrollment: success
then reports `voice_enrolled:false` and says only the face was remembered.
The model may report voice enrollment only after `voice_enrolled:true` from
the eyes. If the consent response was too short, another sufficiently long
spoken request with explicit permission is needed; the earlier introduction's
print is not silently reused.

The first-meeting name step offers this combined consent only with the shadow
switch and both configured model/socket paths; its normal face-only prompt is
unchanged when off. It asks for the name in a sufficiently long consenting
sentence, so checked-name validation can still use that request.

First voice enrollment requires successful recording of the one face in front
of the lamp. It is saved atomically with that face's existing person record.
There is no standalone name-to-voice enrollment RPC. Enrollment still needs
the camera even though later shadow comparison itself does not.

## Eyes protocol and storage

`crates/eyes/src/who.rs` owns the optional voice field in the existing private
people store. It accepts the pinned model's **512-dimensional** finite,
nonzero embeddings and normalizes them. The model tag is
`cam++:357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b`.
The actual pinned model's output established the dimension; it was not inferred
from the CAM++ family name.

The store remains limited to twenty people, five face prints per person, and
at most three voice prints per person in the schema. Current enrollment writes
one voice print with the new face record; it does not accumulate samples from
ordinary conversation. The people file is bounded at 1 MiB, mode 0600 in its
private directory. Unknown model tags or malformed records are refused or
set aside under the existing [eyes storage rules](eyes.md#who-is-there).
An older face-only file remains readable.

The local request reader is bounded at 32 KiB. In addition to existing requests:

- `remember` accepts optional `voice_print`, a 512-number array supplied only
  by the worker, alongside the existing `one_face` and `replace` controls.
  Success with a print reports `voice_enrolled:true`; face-only success reports
  `voice_enrolled:false`.
- `voice_match` takes a 512-number array and returns `candidate` (name or null),
  `cosine`, `runner_up` and `shadow:true`. It compares only compatible stored
  embeddings and never writes the known-person file.
- A standalone `voice_print`, a voice print on `forget`, malformed dimensions,
  nonfinite/zero vectors and mixed request shapes are refused.

A candidate requires cosine at least 0.75 and a gap of at least 0.15 over the
next person, when there is one. These are **provisional diagnostic thresholds**,
not calibrated identity or security guarantees. The best score remains useful
for measurement even when no candidate passes. Existing synthetic same-voice
attention results do not calibrate this person-recognition use.

Renaming/replacing a face removes its old attached voice prints; a new voice
print requires fresh explicit enrollment. Face-only replacement does not keep
the old voice association. `forget_my_face` removes both the face and attached
voice prints, including matching set-aside records, and cancels pending
enrollment so a delayed write cannot recreate them. Product reset clears the
same people store and set-aside directory. These operations do not delete
conversation memory; [memory scopes](memory.md#each-persons-own-764) keep their
existing semantics.

## Evidence and remaining work

An existing release binary on the development Mac, with the pinned model,
measured 2,215 ms to load and 287 ms to produce a 512-number print. The newly
built debug `serve-pinned` binary also passed on the same synthetic fixture:
13,141 ms load, 1,420 ms inference, 512 numbers. The build profiles differ;
these are compatibility checks, not a latency before/after comparison. This establishes model compatibility and
shape only. It is not a lamp latency measurement, an office-speaker accuracy
result, or proof of safe memory access. A host debug test of bookkeeping alone
(100 starts and 30,000 accepted 10 ms blocks) measured start p50 0.292 us,
maximum 378.875 us; block p50/p95 0.125 us and maximum 80.750 us. It excludes
global dispatch, inference and socket I/O, and therefore establishes only the
local bookkeeping boundary, not acoustic latency. No real-office recognition
result is claimed for this stage.

Install `lamp-voice`, `lamp-eyes` and `lamp-same-voice` from the same build for
measurement: the new child mode and eyes request fields are a joint protocol
change. No deployment unit is changed by this implementation.

Before anything trusts a match, the authorized lamp measurement must:

1. Record the exact hardware, microphone path, runtime/model hashes and
   switch settings. Compare first use and repeated requests with shadow off
   and on; measure End-to-print, End-to-diagnostic, response onset, queue drops,
   child recovery and resource usage separately.
2. Enroll consenting real office participants with clear face-and-voice
   permission. Check face-only consent, refused consent, short consent,
   corrected names, unavailable camera and ambiguous/multiple faces.
3. Label quiet speech from each enrolled speaker and unenrolled visitors,
   including turned-away/no-face speech, short replies, different distances,
   background conversation, overlapping speakers and lamp playback. Retain
   recordings only under separately agreed measurement consent.
4. Compare same-person and different-person score distributions and runner-up
   margins. Report false matches, missed matches and unknown/ambiguous rates,
   with counts and clip duration; choose thresholds from held-out evidence.
5. Exercise microphone privacy, cancellation, overlapping/replaced requests,
   restart, late inference, missing model, unavailable eyes, rename, forget
   during pending enrollment and reset. Confirm there is no stale print or
   recreated identity and that normal input readiness remains truthful.
6. Prove that all shadow candidates still leave memory scopes, private session
   context, greetings and attention unchanged. Only a separately reviewed
   request-bound routing change may enable personal memory without a face;
   it must cover both voice routes, tools, journals, snapshots and forgetting.
