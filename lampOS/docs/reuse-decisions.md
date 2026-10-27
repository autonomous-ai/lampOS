# Reuse decisions: V1, ROS 2 and existing Rust systems

## Cap only the future plan's derived head-pins output (2026-10-28)

Reuse the existing V2 head-pair evaluator, ordinary pretty-JSON bytes and
Serde's streaming writer rather than duplicating reader policy or formatting
an unrestricted String and checking its length afterward. The private
[plan-output entry point](head-touch-plan-output.md) selects a fallible 1 MiB
buffer, including the newline, and returns an owned staged vector only on
success. Ordinary callers keep their current formatter and diagnostics.

Keep V1's exact-input and durable-pending intent as the later recovery pattern;
this preparation seam adds no filesystem mutation. Policy reads still use the
supplied HeadInputs. Its output cap does
not replace signed-input bounds, root ownership, excluded writers, durable
publication or authenticated admission. No dependency, command, unit, allowlist,
guard or restore path is added. Physical acceptance remains later work.

## Persist closed head-input observations without native Stat layout (2026-10-28)

V1's signed-payload and pending-intent recovery patterns retain exact verified
bytes and recover before normal work. Reuse that exact-input intent for a future
authenticated plan, without copying its broad rootfs execution permissions.
The existing V2 head evaluator and bounded collector already define compatible
read semantics; keep those policies and native identity equality unchanged.

The private [saved-input codec](head-touch-codec.md) uses existing Serde, base64
and Ring dependencies with explicit portable fields, bounded streaming seeds and
a capped encoder. Decoding returns a different immutable closed provider rather
than inventing a native `Stat` or reopening uncaptured live paths. Preserve raw
OS bytes, null-device witnesses, negative facts and directory order. Unknown
nonsemantic fields retain their original encoded bytes; unknown semantics and
duplicate known fields refuse.

An external hash/size match identifies bytes, not trust, root ownership or
admission. Explicit fresh comparison is separate from decode/replay. No new
storage, command, unit, allowlist, grant, privileged writer or restoration path
is introduced. Authenticated finite-plan preparation, durable publication,
independent restoration and physical cuts remain later work.

## Refresh the fixed settings writer with independent recovery (2026-10-27)

V1's `scripts/provision/software-update` snapshots absent rootfs entries as
well as existing bytes, persists `pending-update.json` before moving live
files, and clears it only after restoration and health. Its bootstrap
recovery runs before version detection (`system/bootstrap/recovery.go`).
Keep explicit absence, durable intent and recovery before normal work;
do not reuse V1's broad rootfs permissions or claim runtime parity.

Reuse Update's signed envelope, updater-first confirmation, atomic synced
file replacement and bounded owned-child execution. Actual 7071's updater
already runs as root and may write systemd units, so a narrowly scoped root
one-shot is a viable bootstrap without changing its own unit. The old
`apply` allowlist cannot install the helper, and the old `back` path cannot
undo a new setting path after a partial apply. A saved `.prev` alone does
not solve either recovery problem.

The [settings-writer design](settings-writer-refresh.md) specifies a frozen
root-owned guard established before replacing the candidate writer, with
its own versioned preimages and failure/boot admission. Recovery restores
partial settings before the old writer and never depends on the broken
candidate. Unknown recovery semantics block admission. The unwired
`writer_refresh` record/admission foundation has eighteen focused local
tests; authentication, guard bootstrap and actual restore execution remain
unimplemented. First-use failure, process cuts, real Bookworm systemd and
a 7071 lamp rehearsal remain owed.

## Observe a future writer's files through directory descriptors (2026-10-27)

V1's `scripts/provision/apply-overrides.py` refuses symlinked/nonregular
top-level device JSON inputs before preparing writes. Keep that preflight
intent, not a claim that it supplies descriptor-safe recovery snapshots.
Reuse Setup reset's existing rustix `openat`/`DIRECTORY`/`NOFOLLOW` pattern
and Update head-pair reading's same-descriptor metadata check. The new
[read-only observer](writer-file-observation.md) has a stricter separate
file policy; existing symlink/null-mask compatibility does not change.

Bound relative path/component counts and content, refuse known special
files before open, and retain observed ancestor/file identities. This is
unwired and grants no authority. Stable root ancestry, caller ownership
and exclusive writer coordination remain preconditions. Metadata checks
are not race-proof snapshots, ACL/xattr capture or evidence that exact
restoration or privileged helper refresh works. Those belong to later
#3689 pieces and their own gates.

## Reuse one kernel socket witness for a camera hold (2026-10-27)

V1's `hal/privacy.py` publishes camera/speaker gates before slow teardown and
checks camera privacy again after capture. Keep that ordering and final-check
intent, not its same-process shared globals: Eyes and provider workers are
separate processes. A Voice-local Arc cannot observe an independent Eyes close.

Reuse Unix stream shutdown and SCM_RIGHTS duplication, and the safe rustix
OwnedFd ancillary handling already used by Voice's reminder transport. One
pair per hold gives all observers the same kernel revocation boundary without
a per-observer sweep or per-check request/reply. IPC's standalone primitive
uses fixed storage, an exact versioned frame and original caller deadline;
it neither installs a server nor changes any existing runtime path. No new
dependency, unsafe ownership conversion, shared mmap or client shutdown is
introduced. Normal IPC endpoints retain their existing same-user policy.

Keep producer incarnation/hold, original source age, accepted-audio identity,
request cancellation and privacy checks independent of an open socket. The
owner must explicitly revoke before replacement even while an Arc is pinned;
clients drop only their descriptor. Initial grants run off capture/stream polls,
and each hot check performs one non-consuming nonblocking receive. Exact-frame,
shared shutdown, client-drop, close-during-grant and malformed/extra/truncated
FD cleanup controls are authored but UNRUN. Host/ARM64 timing and actual
Eyes/Voice integration, cross-UID peer authentication and lamp checks remain
pending; this source-only primitive does not release Common's explicit-close
hold. Successful shutdown, not recv-return, is the proposed linearization.

## Bound the initial direct capture-ready handshake (2026-10-26)

V1's `hal/drivers/voice/aec.py` `prepare_playback` retains the output rate
without emitting playback/reference events; `configure` completes reference
preparation if the microphone opens later. Either device may open first.
Keep that startup tolerance and preparation-before-play intent, not V1's
shared globals, callbacks or in-process FIFO ownership. V1 has no equivalent
of this cross-process reference envelope's 100 ms initial-age refusal.

Reuse V2's existing one-second initial worker-control allowance only for
the speaker's first in-order `CapturePrepared`. Do not drop that one-shot
handshake, accept stale prime acknowledgements, widen PCM freshness or
bypass the restart guard. Playback still needs fresh authority, matching
privacy and the prime barrier. No queue, retry, wait or dependency is added:
the receive path remains nonblocking. The target is to consume the reported
143402 us startup message on the next speaker-loop pass (nominal 2 ms
tick), with zero added waiting; this is not a measured startup or acoustic-latency result.
The one-second bound is the base startup allowance; the existing receiver-stall
allowance remains, so it is not an unconditional wall-age cap. Exact one-second
expiry on a clean first look, boot, sequence, future stamps and later-message
behavior are checked on explicit clocks; Linux null-device evidence is limited to
real-loop consumption. V1 source was inspected, not run on a lamp.

## Preserve completed Setup through an update (2026-10-26)

V1's `scripts/provision/software-update` records prior device service
activation and restores only the services that were active or enabled
(`device_was_active`, `restore_device_service_state`). Its web update swaps
setup assets; it has no equivalent of lampOS's transient Setup process or
its per-part health watch. Keep the activation-preserving intent, not a
claim of equivalent V1 lifecycle coverage.

Reuse Update's exact noncomment unit comparison and conservative override
inspection pattern. Change restart selection only for the recognized
product Setup after completion: leave the phone's grace process alone,
while updating the files for a future setup. Apply the same decision after
restoration, including interrupted-switch recovery, without falsifying the
journal's prior activity. Do not relax `Watch` or treat arbitrary inactive
services as healthy. Custom conditions, unit overrides and uncertain local
inputs retain the ordinary restart/watch rules. Reuse the bounded systemd
query helper to check the effective identity, alias names, fragment and
drop-in paths too: filesystem inspection alone misses alias drop-ins and
loaded aliases whose files were removed. Explicit manager metadata in the
fake is independent of the filesystem classifier; unknown metadata is no
exception. There is no general systemd condition interpreter or new dependency.

The synthetic regression on unchanged code produced both the new-build and
fallback `lamp-setup.service stopped` errors. It models successful
`systemctl restart` with an unmet start condition independently of the
classifier. Controls cover completion after the activity snapshot, reset,
ordinary failure/rollback, recovery and bounded override inputs. These
are local filesystem/fake-systemd checks, not device expiry or latency
measurements. The restart decision has no intentional delay; regular
filesystem I/O has no wall-clock deadline, and marker rechecking is not
atomic coordination with reset.

## Refuse nonregular local head-pair inputs without waiting (2026-10-26)

V1's `scripts/provision/apply-overrides.py` checks that top-level override
JSON is a regular non-symlink file, skips non-file rootfs inputs, and
prepares reads and validation before writes. Keep the preflight-before-
mutation boundary for install. V1 has no equivalent of V2's shared
head-reader/pins pairing check, so these controls are not a V1 parity claim.

Reuse Update's existing nonblocking-open and same-descriptor type check
from `install::set_mode`, without its no-symlink policy: the head-pair
reader already accepts links to regular files and systemd's null-device
masks. Preserve those inputs and missing-file behavior. Read only regular
file contents; recognize the actual null character device by device
identity and return empty text without reading it. Other nonregular
inputs refuse. No mode inference, retirement, product switch, microphone
or runtime touch rule changes, and no dependency is added.

The original install regression reached a FIFO reader and then exceeded a
five-second child watchdog; the parent killed and reaped it before scratch
cleanup. That watchdog is a diagnostic bound, not a product timeout. The
implementation target is refusal without waiting for a FIFO writer. This
does not establish a global filesystem deadline, pathname-race protection
or physical touch latency; regular-file storage can still be slow.

## Preserve privacy refusal on a retained-read error (2026-10-26)

V1's `hal/drivers/privacy_button.py` retains its previous state when the
reconcile or watchdog read fails; `device_tap_actions.py` and
`button_actions.py` honor that retained privacy lock. Keep the distinction
between an absent route and a failed read. Expression previously discarded
`Bus::latest` errors and treated them as the no-publisher compatibility case.
It now treats errors as closed at tap/rest intake and final planning, using
the existing look cancellation and ordinary-rest cancellation paths.

Keep valid retained-lease semantics, the no-slide exception, no-publisher
compatibility and claimed night-sleep behavior. A valid open read permits
new input; reopening alone does not revive cancelled work. A successful
read with no publisher retains its existing compatibility exception. This adds no
I/O or delay and changes no message, switch or microphone route. V2's
conservative refusal on an unreadable envelope is a deliberate correction,
not a claim that V1 treats every unknown state as closed or proof of physical
privacy or latency parity.

## Require observed rest before factory head calibration (2026-10-26)

V1's `hal/drivers/ttp223.py` probes driven lines transiently and measures
candidate contact from kernel edge timestamps; it has no persisted
hands-away baseline report. Keep V2's measured-rest factory workflow and
the existing 100 ms acquisition freshness bound. Apply that bound to the
missing coverage inside the rest phase, and require an actual sample in
that phase before accepting later touch evidence. A skipped phase must not
turn its preinitialized zero transition count into a measured baseline.

Reuse the existing discovery error, CLI failure and factory restoration
path: an invalid scan stops, Controls resumes, and the pins stay unchanged.
Successful discovery adds no wait or I/O; no report field, live touch rule
or product switch changes. Deterministic sampled-time controls establish
this evidence boundary, not hardware calibration or V1 behavioral parity.

## Promote an already tested release to a stable feed (2026-10-25)

V1's `scripts/release/promote-ota.sh` separates promotion from building:
it changes a component's minimum version and signs updated metadata. Keep
the separate operator decision, but change V2 promotion to selecting an
existing signed build byte-for-byte. This avoids another signing operation
and preserves the source run, sequence and immutable payload URLs.

Reuse `lamp_update::manifest` for Ed25519 verification, schema validation,
asset names and bounds, with the independently provisioned public key.
Reuse the existing CI publisher's staged upload, retained previous asset,
readback and restoration semantics. GitHub's asset rename is not a
multi-asset transaction, so retain that limitation and refuse uncertain
heads rather than promising atomic replacement. A separate Rust CI tool
keeps GitHub credentials and promotion commands out of the lamp runtime.
The default feed and product units remain unchanged until their owner
activates the stable feed. Offline failure tests are not a lamp trial.

## Carry product services through the existing update transaction (2026-10-25)

V1's `system/bootstrap/recovery.go` recovers an unfinished update before
version detection. `scripts/provision/software-update` records absent files
as well as backups and checks signed OTA metadata before installation.
Keep those safeguards through the existing Rust signed manifest, journal,
exact unit/link fallback and settings helper rather than adding another
installer. The service carrier adds a reserved product entry and updater
level 6; the helper's signed settings and recovery records stay compatible.
Local tests cover selection, corruption and fallback, not a played device
update. The fixed updater fallback restores links only, so this carrier
does not replace its own service or auxiliary systemd units.

## Bind the requested name to the speaker's own declaration (2026-10-24)

The V1 self-enrollment camera flow assumes the requester is the subject and
prefers a name in the message; the earlier audit of
`skills/face-enroll/reference/self-enroll-camera.md` records that boundary.
Keep the existing Rust accent folding, spelling and letter-count checks.
Those establish that a name occurred in the words, not whose name it is:
`name_heard` accepts names said about a friend or child too. The guarded
speaker-bound path therefore needs a separate ownership check before any
Eyes request. A model instruction to name only the requester is insufficient.

The new check consumes a direct self-introduction, a bare name or a spelling
response, rather than searching mixed narration for any first-person phrase.
An unrelated "I'm Alex" cannot authorize the tool's candidate Kess elsewhere
in the same line. A request followed by a bare name can be a vocative:
"Remember my face, Kess" therefore needs a fresh explicit self-introduction.
The same applies to greetings or polite prefaces/suffixes ("Hi, Kess" or
"Kess, please"); stripping them cannot turn an addressee into the speaker. Ambiguous, reported, quoted, negated and tentative forms
refuse conservatively. Legacy/off behavior keeps its existing checks.
Both the heard line and the raw requested name are capped at 4 KiB before
validation; a padded model argument cannot evade that bound. Unsupported
punctuation is refused before folding can discard quotation or uncertainty.
The local check adds no I/O, inference or tool deadline. Its design target is
under 1 ms per bounded line on the host. A pre-final debug-profile probe
measured an ordinary introduction at 93.6 microseconds mean, but cap-sized
inputs at up to 9.55 ms mean; the target is not met for those debug inputs.
The probe excludes socket/proof construction and precedes the final
vocative refusal; it is not a final-build or device latency claim.

Reuse the existing Eyes refusal, but change guarded `known_as` wording: no
stored name is exposed, and no impossible replacement confirmation is
requested. Guarded replacement remains refused until a same-speaker
confirmation design exists. Success is still only Eyes' actual saved result.

## Bind live face enrollment to its speaker (2026-10-24)

Read V1's `hal/realtime/orchestrator.py` delegate description and
`skills/face-enroll/reference/self-enroll-camera.md`. Keep the spoken-name
check, refusal to borrow a stale current-user name, and confirmation only
after enrollment actually succeeds. Change the assumption that the visible
face belongs to the requester: V1's camera flow also permits a text request
while assuming its sender is in front of the camera. The guarded Rust path
requires evidence from the current microphone line and the face seen speaking
it; no remote text or prior person's words supply that consent.

Reuse Rust's speaking-face reader, bounded cancellable socket exchange,
camera permission holds, original driver frame times, existing SFace prints
and 0.363 matching threshold. A tracking ID is not identity: its IoU tracker
can assign the same ID to a replacement in the same position. Keep one
immutable speaking-frame print for comparison throughout capture; bound the
temporary anchor history to 16 prints and 30 seconds. Reuse the existing
People mutation path and its actual result, without a V1 runtime bridge,
new inference dependency or automatic privacy opening. Guarded renaming
waits for a future same-speaker confirmation design; it fails closed here.

The distinct internal `remember_speaker` operation makes mixed-version
Voice/Eyes pairs refuse unsupported proof instead of ignoring an added
field and enrolling unguarded. The switch is off by default on both sides.
The existing four-second capture and six-second tool deadline stay fixed;
no extra background inference or capture delay is introduced. Host regressions
do not establish physical enrollment accuracy, end-to-end latency, or V1
parity. Those remain separate lamp checks.

## Face repair writer prerequisite (2026-10-23)

Read V1's `system/server/device/delivery/mqtt/face_enroll_handler.go` and
`face_owners_handler.go` directly again for this ownership boundary. Both
acknowledge `starting` and run HAL outside the MQTT callback under the same
`faceEnrollMu`; removal cannot overlap enrollment. Keep truthful progress,
validation before admission and exclusive mutation. Change the per-request
goroutines waiting on that lock to one fixed Rust worker, one repair and one
priority forget. Do not port the label/error logging, HAL folder deletion or
claim that a request timeout cancels the underlying write.

Reuse actual `People::replace_prints_at` checkpoints and
`People::forget_at`, including the latter's set-aside/pending-forget behavior,
stable-ID lookup and by-name guard. Only the checkpoint enum/method become
crate-private for the writer; ordinary `remember`/save behavior is unchanged.
The adapter owns the exact supplied store rather than cloning or reopening it.
Keep actual pre-rename `NotWritten` and post-rename `Indeterminate` distinct
from a missed response deadline. Before-rename refusal cannot promise to stop
an already executing rename/fsync. Accepted forget survives caller loss and
runs when that same worker returns; a permanent stall remains pending.

The [inactive writer](eyes.md#face-repair-writer-prerequisite-inactive) retains
live work and one-use completion outside its four acknowledged receipts.
Those receipts keep only operation IDs and fixed status/counts; no plaintext
name or private stable ID remains after acknowledgement. Match names for
mutation retries only while live/unacknowledged. An acknowledged operation
number is `StaleId` for mutation regardless of the supplied label; read the
retained result through `forget_status`. The worker drops its execution name
before publishing the result, and acknowledgement drops the live retry name.
No hash, zeroization or physical RAM-erasure guarantee is added. Trusted
runtime operation numbers reject conflicting/old replay but are not account
credentials. Actual forget results preserve `unsearched`; no early
`forgotten` or memory/rules removal is inferred from admission. Worker failure
and restart remain unknown, with no replacement writer or fabricated rollback.

This is only the already-approved store handoff. The target/`CommitWork` must
be prepared before transferring `People`; live reservation, recognition and
all ordinary mutators still need one ownership protocol. Trusted forget entry
must invalidate the controller first and suppress stale publication. No
Cloud/Voice/camera ingress, environment read, RPC, capability or activation is
added. All 16 deterministic real-store controls passed on the development Mac;
first/repeated-maximum status and forget admission were 3,713/443/15,688 ns
at the held directory-sync checkpoint, within the 1 ms host target. These
include bounded explicit-contention retries, not a cold/device measurement.
Native/full-runtime qualification remains separate; no V1 parity, device
latency or power-loss claim is made.

## Face repair controller core (2026-10-22)

The V1 paths were checked directly in this checkout:
`system/server/device/delivery/mqtt/face_enroll_handler.go` validates label,
base64 and photo size before a `starting` acknowledgement, then calls HAL
from a goroutine under `faceEnrollMu`.
`system/server/device/delivery/mqtt/face_owners_handler.go` shares that lock
for removal. Keep early validation, truthful progress and one writer, with
slow work outside the input callback. Change the unbounded per-request
goroutine admission to one pending RAM transaction and four terminal
receipts in the inert Rust core; a future writer still needs runtime wiring.

`hal/routes/sensing.py::face_enroll` calls
`hal/drivers/sensing/perceptions/processors/faceid/perception.py::enroll_from_bytes`,
which saves a photo before training. Its `remove_photo` deletes the person's
directory when the last photo goes; `remove_person` deletes that directory
and reloads the remaining people. These are photo enrollment/deletion
paths, not atomic repair preserving a stable memory record. Do not port
them as repair. Reuse the Rust `RepairTarget`/`replace_prints` handoff and
explicit `NotWritten`/`Indeterminate` outcomes instead.

The [controller core](eyes.md#face-repair-controller-core-inactive) adds
account-generation/ticket binding, an exact boot-qualified candidate,
original frame acquisition times and a consumed completion. Its
`ConsentProof` trusts ingress that is not yet implemented; it does not
verify a speaker or activate a capability. Keep frame-delivery rejection,
but require trusted runtime privacy/account changes and forget entry to call
`invalidate(now)`: they must drop pending RAM without depending on another camera
frame or an old account ticket. The future forget adapter must call
`invalidate(now)` first, before potentially slow store I/O, including when that
I/O later fails. The store's revision guard refuses a
stale replacement but does not release the controller's separate target snapshot
and captured prints. The actual-forget control covers that ordering; no adapter
is added here. A consumed write stays `Committing` until
its real completion, with forget ordered and reconciled by the exclusive writer;
invalidation makes no rollback promise. The API registers no RPC or app/model route.
Proposed source time bounds and
the unmeasured under-1-ms host target are documented separately from any
first-builder decision or device result. No V1 consent parity, runtime
integration or reboot rollback guarantee is claimed.

## Atomic face repair store (2026-10-22)

The evidence here is the existing Rust `crates/eyes/src/who.rs`, not a V1
audit or a lamp comparison. Keep `Person`/`Kept`, the private 0600 file,
print bounds and recognition threshold. `People::remember` delegates to
`remember_as`, which rebuilds the record and resets its added time and
familiarity. `remember_one` owns ordinary recognition/name policy;
`keep_prints` preserves named-visit consent during its recognized-face update.
Leave those behaviors intact. None is the repair operation: repair must keep
the old name reserved, retain the existing ID and metadata, and clear consent
when replacing the biometric prints.

Reuse the store's file-sync and rename pattern without delete/re-add or a
new ID. Add an opaque, consumed snapshot bound to the live writer and its
save revision, compare the whole record, and check every other record for a
matching print. Only this new operation adds parent-directory sync and an
explicit `Indeterminate` outcome after rename; ordinary save durability is
unchanged. The narrow fault hook covers write/sync/rename boundaries without
a general I/O abstraction. This is an [inactive store component](eyes.md#atomic-face-repair-store-inactive):
account approval, fresh capture, proved lamp consent and revocation inputs
remain caller work. The controller core adds only bounded RAM staging and
expiry. No runtime, V1 parity, biometric proof or device result is claimed.

For the interrupted-write review on #3072, keep Setup's existing exact-path,
symlink-safe reset implementation. Replace the variable repair filenames
with one `people.json.repair.new`, created exclusively, and include it and
the ordinary `people.json.new` in every live reset plan. Reuse startup's
exclusive writer ownership to unlink just those two uncommitted siblings;
unexpected cleanup failure refuses opening the store. Recheck mode 0600
before rename. Historical Update plan fixtures keep their original bytes
and hashes as migration evidence; the new product-plan hash is added to
`RESET_PLAN_SHIPPED`. No broader deletion or ordinary-save durability change
is introduced.

The follow-up forget review keeps the same two exact unlink operations and
shares them with `People::forget_at`, before any canonical or RAM mutation.
An already-open store must not report a completed forget while an unfinished
repair copy still contains the person; cleanup failure returns an error.
Actual-forget fixtures check full-copy removal, unchanged other records and
IDs, unremovable directories and temporary symlinks before any reopen. The
repair fault fixture now checks the exact `.repair.new` path before startup
could hide a cleanup failure. No directory sweep, secure-erasure guarantee,
or runtime repair activation is added.

## Bedtime and Morning as a core skill (#1672, 2026-10-19)

V1's `hal/app_state.py::_wake_sleepy_peripherals` restores only sleep-owned
mute state and leaves privacy authoritative; `hal/routes/emotion.py` closes
input before sleep work. The ambient service fails quiet without a known HAL
state. Keep the ownership and unknown-state protections. Do not port the
Python callbacks, spoken sleep announcements, peripheral shutdown or a
second actuator writer into lampOS.

Reuse the current Expression sleep/wake renderer and the body's #1192
fold/pause guard. Move automatic local-night/absence and first-person morning
selection into skill data when `LAMP_REST_REQUESTS=on`; retain manual sleep,
privacy, name, reminder and conversation reflexes. A session-bound leased
intent, terminal episode history and a separate planning receipt keep an
old skill or owner from resuming cancelled work. The morning Say hum waits
for the planned wake to finish because Say itself changes the Voice owner.

Reuse `lamp-local-time` for local windows and `lamp-schedule::store::Store`
for durable caps. Reuse `lamp-schedule::client::exchange` for the bounded
nonblocking private Actions hop, including connect and the complete exchange.
Reuse light requests for the evening glow and the ring/Volume owners for
ceilings. Min composition and release without restoration preserve a person's
off, mute or quieter setting. The fixed quiet hum reuses `head_cue::hum` through
Say's existing sound guard; no sound binary or model call is added.

All routine switches remain off and the core manifest is disabled. Host tests
do not establish acoustic volume, optical brightness, applied motion, flash
power-loss behavior or installed-system timing. The first builder owns the
[lamp comparison](bedtime-morning.md).

## Requested local music, #1685 (2026-10-19)

The source comparison uses the V1 files retained in integration tree
`dab3f1242`, not a measured lamp run. Keep explicit spoken requests and one
short confirmation (`skills/music/SKILL.md`), mute refusal and terminal
speaker stop (`hal/routes/music.py`), cancellation during preparation and
playback (`hal/app_state.py::music_cancel_active`,
`hal/drivers/button_actions.py::single_click_action`), and privacy closure
before slow work (`hal/privacy.py`). V1's three-second cancel window becomes
an exact, terminal owner/call identity: elapsed time or unmute cannot revive
an earlier item. Its declared audio quiet-hours rule is not implemented by
this first tool-only format; no parity or arbitrary replacement night
window is claimed.

V1's `hal/drivers/voice/music_service.py` waits for TTS with bounded polling
and starts a separate player. Requested audio instead joins the existing
Voice owner's actual final confirmation retirement, then uses its same
speaker worker, 240-frame dispatch and final ownership/privacy permits.
Reuse the installed `hound` WAV decoder, SHA-256, bounded Unix-socket client,
Skills package/permission checks and local-recording cursor. No new decoder,
player service, ALSA writer or runtime dependency is necessary for a fixed
24 kHz PCM asset. Extend only the explicit-request reader to 60 seconds;
autonomous cues retain their three-second bound.

The example is an original 48-second score rendered from integer Rust source
at build time, with Apache-2.0 provenance. Generated WAVs and packages are
ignored artifacts. Do not port V1's media search/download, arbitrary file
arguments, personal history, automatic volume restoration or movement.
Actions supplies a passive speaker-state observation; Skills supplies short
renewable leases. Neither can create a Voice owner. The exact cancellation,
scan/freshness limits and separate host/device qualification boundaries are
in [requested audio](requested-audio.md). Passing a host fixture does not
establish acoustic onset, perceived musical quality or device interruption.

## Said to the lamp from close to it, task 49 (#753, 2026-10-15)

Keep V1's gaze rule for the face (`hal/drivers/tracking/gaze.py`,
`user_check.py`, `hal/config.py` `GAZE_*`): a share of looks rather than
one, from 1.5 s before the speech (`GAZE_WINDOW_S`), at least two looks
(`GAZE_MIN_SAMPLES`), 0.4 of them facing (`GAZE_BEARING_MIN_FACING_RATIO`,
chosen there after a still, centred user measured 50% against 0.6), and a
face at least 0.15 of the frame's height (`GAZE_BEARING_MIN_FACE_HEIGHT_FRAC`,
measured on green-lamp frames on 2026-09-30: colleagues 8.3% to 13.6%, the
user 19.2% to 46%). Change: the per-look facing judgement is the Rust eyes'
own (`facing: true`), not V1's yaw cone, and the share is over the voice's
request rather than V1's wake trigger. Keep the voice's existing near sign
(`busy-near`'s room-microphone rise, 18 dB, measured on lamp-4ace). Leave
out the voice microphone's level: its own processing evens it out and the
two streams do not share a clock ([said-to-whom.md](said-to-whom.md),
[microphone-topology.md](microphone-topology.md)). No fitted model: the
published studies in said-to-whom.md do not transfer to this lamp, and a
combination needs this lamp's labelled recordings, asked for on #753.
Publish both signs per request on `attention/addressed` so the Policy can
fit them later. Off unless `LAMP_ADDRESSED=on`. See [attention.md](attention.md).

## Speech-backed name wake, task 47 (#749, 2026-10-15)

Reuse Expression's accepted `voice/activity` evidence and existing boot,
source-age, ordering and owner checks; keep the name detector unchanged.
The name's `ago_ms` dates its onset, so bus delay cannot make a name fresh.
V1's wake helper restores sleep-owned audio state and respects the hardware
mic mute (`hal/app_state.py:617`); its Harness capture refuses sleeping or
muted input (`hal/drivers/voice/voice_service.py:539`), and ambient behavior
treats unavailable sleep state as paused (`system/ambient/service.go:122`). This
Expression-only rule does not reopen capture or change those owners. No
V1 two-second name/speech rule was found in the inspected paths. The current
Rust detector contract already supplies person-speech observations; another
detector, transcript heuristic or room-level threshold is unnecessary.
The added correlation uses bounded state and the injected clock, behind the
default-false `alive.sleep.name_requires_speech` switch. See [alive.md](alive.md)
for exact timing and limits. Acoustic false positives, delayed real bus
delivery and physical waking remain for First's lamp minute and night watch.

## Physical privacy, task #755 (2026-10-15)

Source comparison, not a hardware measurement: V1's `privacy_button.json` and
GPIO driver use chip 1/line 9, pull-up and LOW mute; its privacy policy closes
capture before slow teardown, and its muted preset is dark red. Reuse the
existing pinned `gpiocdev` 0.8.0 request/value semantics, ALSA 0.12.1 capture
adapter and Rust Voice's 50 ms freshness / 60 ms stable-open gate. The pinned
GPIO crate defaults to active-high values, so the adapter intentionally maps
electrical `Inactive` to muted without also setting an active-low flag.

Change ownership once: selected Controls reads the physical line and Voice
consumes acquisition-stamped evidence; the old Voice GPIO reader is not opened
in parallel. Keep microphone permission with Voice, including its durable
software latch. Ears now obeys a short Voice lease and closes its own MIC1
handle, which the existing Voice MIC2 worker could not do. Keep SPI in a
separate supervised process and permit only a fixed dark-red mute indication
under fresh denied authority; reuse the existing exclusive ring lock and
verified black-write/exit handoff. Do not reuse V1's shared callback lifetime,
or treat a red light as evidence that a microphone handle closed.

Retained implementation references: [GPIO builder/value contract](https://docs.rs/gpiocdev/0.8.0/gpiocdev/request/struct.Builder.html),
[privacy switch contract and qualification](privacy-switch.md),
[service lifecycle](service-privacy.md). Host replay and fake handles cannot
establish electrical cutoff, acoustic silence, optical appearance or lamp
timing; the PR's native ARM64 run and First's lamp measurements remain separate.

## A wake cue waits for privacy readiness (#2487 follow-up, 2026-10-22)

Re-read V1 `hal/drivers/button_actions.py::announce_listening_cue`: it
respects the hardware mute and disables the spoken cue because it could
make VAD lose the person's first words. Keep the Rust wake's existing local
hum through its owned speaker/reference path, with microphone permission,
quiet and listening readiness still required. Reuse privacy's 60 ms stable
reopening rather than bypassing it for a cue. Change only the pending wake's
refusal handling: wait for startup permission inside its original 1.5 s
window, with a strict expiry before accepting readiness. The 500 ms
wake-input-to-first-write target and real-hand qualification remain with
Fifth on lamp-52e6; a host fixture cannot establish either acoustics or the
person's first words being retained.

## Touch strip and head, Task 33 (2026-10-14)

Third read V1 `hal/drivers/mpr121.py`, its initialization/configuration/spatial
gesture tests, the head's TTP223 fast-pulse behavior and hardware wiring before
adding the separate `--touch` path. The existing Controls path stays unchanged.
Retained safeguards: reset/Stop/write/Run ordering, checking reset state,
initial held-contact suppression, fault as unknown rather than release,
5 ms pad/10 ms contact and chord qualification, three-pad touch minimum,
strict ordering/age, bounded delivery and inhibition before teardown. Changed
product meanings: base Touch asks Stop or Attend; head Touch asks Stop or
nothing, its short tap Attend or Wake, and a head hand Stroke at 1 s and Sleep
at 3 s (#711); head/slow-base Stroke is separate from physical speed; stationary two-second base Hold asks Sleep
or Wake according to the owner's contact-start lifecycle. No `Pet`, blind
toggle, shutdown or direct owner callback is imported.

| Choice | Evidence and consequence |
|---|---|
| Reuse Linux transport | Pinned MIT/Apache `i2cdev` 0.6.2 provides combined transfers; every returned transfer count is checked. No hand-written ioctl/unsafe adapter, force-address, bus scan or adapter-global timeout/retry change. [Linux interface](https://docs.kernel.org/i2c/dev-interface.html). |
| Keep device policy local and injectable | Inspected [`mpr121-hal` 0.5.0](https://docs.rs/mpr121-hal/0.5.0/src/mpr121_hal/mpr121.rs.html) drops reset-write failure and masks overcurrent in `get_touched`; its baseline accessor also treats adjacent byte registers as a word before narrowing. The local register trait tests each failure and all status bits. `linux-embedded-hal` was considered but adds no needed policy to this transport. |
| Explicit per-unit profile | [NXP Rev. 4](https://www.nxp.com/docs/en/data-sheet/MPR121.pdf) defines ECR0x8F as all twelve pads and debounce0x22 as three detections each polarity. [AN3889](https://www.nxp.com/docs/en/application-note/AN3889.pdf) auto-configuration tunes charge, not touch noise thresholds. First's 20:05 hardware answer confirms faulty E4/E8 on lamp-4ace; the working set and each pad's thresholds are per-unit settings. The earlier register snapshot is historical evidence, not calibration. |
| Preserve sparse geometry | Three to twelve explicitly selected pads; sparse lists are ascending/descending to retain full physical electrode slots. Known exclusions are skipped for connectivity, not distance. Inactive selected pads still separate a sparse footprint; actual zero/fault samples never become invented contact. Full-twelve behavior is retained for comparison. |
| Verify runtime state | Status/configuration/status/ECR checks reject OVCF, working-electrode OOR, ACFF/ARFF, proximity/reserved status, reset/stop and static drift; dynamic CDC/CDT changes remain allowed with auto-configuration. ECR cannot disable holes inside its contiguous prefix: excluded electrodes still scan and may cause a global fault, which remains fatal. This strengthens V1's status-only fault coverage; electrical recovery remains unproved. |
| One disposable child owns I²C | A stuck ioctl cannot block GPIO polling. Fixed per-device advisory lock is held by the child, including after parent death, and never unlinked. Seven-second startup/150 ms packet deadlines, 1 KiB packets, no replacement and bounded truthful reaping prevent stacked workers. Cooperating locks cannot prevent an unrelated external I²C writer or a physical shared-bus fault. |
| Keep actual sampled evidence | GPIO and strip freshness are independent. TTP223 low gaps do not become an invented resting hand; normalized centroids are not millimetres. The raw 57 changed masks cannot prove continuous acquisition: the 995 filled samples and their clock/session/sequence are explicitly authored. |

What has not been shown: per-electrode calibration/noise rejection, a physically
qualified working set on any other unit, real pad polarity and sustained-head behavior, I²C load
alongside Air, cold/repeated physical latency, owner-applied sleep/privacy or
safe light/body following. The [Controls procedure](controls.md#touch-and-slides-task-33)
and Third's log separate Mac tests, native ARM64 CI and First's required lamp
qualification. No third-builder lamp measurement exists.

Review date: 2026-10-10. This is a source-based decision record, not a performance
comparison. V1 means main commit `d5efe9d7b73cc529b34cd4abe97624682a82ca94`.
The current Rust checkpoint is `64529dee4d1228dd9373298a807d506f94421dca`.
External links below were inspected on the review date; rolling documentation
can change. Implementation status is deliberately separate from the design.

The rule is to preserve useful behavior and proven interfaces, reuse maintained
implementations where they fit, and write Rust for the remaining Lamp-owned
responsibilities. A new language alone does not fix timing, echo, ownership or
interaction quality. Do not label an entire system obsolete before examining
its current mechanisms and measuring the relevant alternative.

## What V1 already gets right

Source links in this section are pinned to the baseline commit, so this document
remains useful if lampOS becomes a separate repository. Detailed protocol and
unit evidence lives in the local [hardware contract](hardware-contract.md).

| Preserve | V1 evidence | Rust decision and present status |
|---|---|---|
| Per-field sensor freshness and partial readiness | Environment [service](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/environment/service.py#L109) and [aggregation](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/environment/group.py#L58) exclude stale/faulted samples and retain individual metric timestamps. | Preserve original acquisition times, missing fields and explicit sources. `ObservationTracker` already supplies incarnation, ordering and age checks. The new [environmental snapshot contract](environment-snapshots.md) implements typed values, per-field freshness and fault/restart behavior without I/O. Sensor acquisition and device qualification remain missing. A fresh temperature must not refresh CO2. |
| Privacy closure before slow teardown | [Privacy gates](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/privacy.py#L24) close before service shutdown; camera checks privacy around acquisition. | Keep local authoritative denial and recheck at delivery. Rust capture/camera authority and final output guards implement this principle. Privacy does not wait for a cloud response. |
| Latest camera observation instead of a growing backlog | [Capture](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/camera/video_capture_device.py#L166) replaces the latest response; perception shares frames. | Rust camera has bounded scratch/pending/in-flight buffers and retains the original capture grant. Real camera and conversational vision qualification are still pending. |
| Discard obsolete camera buffers before using a frame | The idle capture loop [grabs twice before reading](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/camera/video_capture_device.py#L555). | Rust discards otherwise valid old startup frames one per poll, preserving sequence/timestamp continuity, the original two-second first-frame deadline and 250 ms age limit. No old frame is exported. After one accepted frame, stale timestamps remain terminal. V1 callback timestamps and old-frame fallback are not reused. [Native qualification](camera-runtime.md#bounded-startup-discard-and-next-native-check) remains pending. |
| Fast touch acquisition independent of slow actions | [MPR121](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/mpr121.py#L642) separates polling/action execution, uses a capacity-two queue, invalidation generations and event age checks. | Port the recognition and cancellation cases. Route qualified events to the interaction owner; avoid copying direct voice/agent side effects. Ordinary touch/buttons are not yet implemented in Rust. |
| One serialized motor bus, with an atomic overload lockout | [Motor service](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/motors/animation_service.py#L1153) blocks new goals before disabling torque on overload. | Preserve one bus writer and distinguish cancellation from torque release. Rust now has the finite MotorControl boundary plus the opt-in [supervised motor worker](motor-worker.md). It retains local inhibition, exact profile/readback checks and position-only writes. Automatic choreography and physical motion qualification remain absent. Limits, units and adapter lifecycle need physical qualification. |

These are meaningful examples of good V1 engineering. V1 runs many hardware
owners as threads inside one HAL process; it is not correct to say that it has
no separation. The change is independent failure boundaries, explicit output
authority and bounded communication, while preserving useful subsystem logic.

## What must change

For the first connected ring policy, retain V1's
[replaceable pending event](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/base.py#L37)
and [stale-restore regression](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/test/test_led_restore_generation.py#L38)
semantics through one pending frame and existing turn/presentation generations.
V1's [speaking wave](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/rgb/effects.py#L284)
is a random simulated equalizer, not actual playback evidence. Its timed effect
thread stop also cannot prove the old writer terminated. The Rust
[ring integration](ring-choreography.md) instead uses fixed prototype phase cues,
matched speaker receipts, one supervised writer and explicit shutdown evidence.
It adds no animation thread, delayed bare `off()` callback, privacy color or
motion. Visual quality and crash recovery still require physical qualification.

| Observed problem | Replacement rule | Verification required |
|---|---|---|
| V1 motor events append to an unbounded list with no expiry or interaction owner ([source](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/motors/animation_service.py#L364)). | Bounded admission, explicit replacement semantics and owner/expiry checks before each physical write. | Flood the queue, cancel/restart owners and verify obsolete gestures cannot execute. |
| V1 camera shutdown clears its thread handle after a timed join without confirming exit ([source](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/d5efe9d7b73cc529b34cd4abe97624682a82ca94/hal/drivers/camera/video_capture_device.py#L692)). | A stop request is not completed shutdown. Retain ownership until actual exit/reaping; report failure. Rust process shutdown already checks exit status. | Stuck worker, late completion and restart tests; no two device owners. |
| Legacy startup can configure torque and queue motion; normalized position values are not degrees. | Read-only inspection first; explicit per-device units, calibrated bounds and current placement evidence before motion. | Fresh whole-device view, limits, current/load protection and stop behavior on Lamp. A successful serial write is insufficient. |
| V1's ordinary TTS capture suppression avoids some self-interruption but discards user overlap. | Preserve continuous capture and a stable render reference; qualify interruption admission separately. | Matched reply-only and quiet/normal overlapping-speech trials, with lost opening words and acoustic stop measured. |
| Current Rust treats six high-VAD blocks as permission to cancel the old reply. Playback residue has triggered that path. | Separate candidate detection from qualified admission; retain the beginning of speech while deciding. | A gate must reject echo without rejecting genuine quiet overlap. No threshold or classifier is selected merely because it passes one recording. |

The opt-in [V1-style turn-taking baseline](turn-taking-baseline.md) now ports
V1's ordinary own-playback suppression and fresh-prefix behavior before further
interruption experiments. Rust retains continuous capture/AEC and uses original
accepted-write/cursor and host-read evidence. Its declared 100 ms tail is not a
measured echo bound, and overlapping user speech is intentionally withheld.
`vad-only` remains the default until physical parity is qualified. This is a
policy reuse decision, not a new DSP engine or a claim of natural full duplex.

The last two rows are documented in the [microphone audit](microphone-v1-v2-audit.md).
They show why the new runtime also needs criticism: process separation and Rust
ownership do not themselves solve conversational perception.

The [pinned AEC source audit](aec-alignment.md) found no sample-rate or float
normalization mismatch. Keep Sonora's existing processing while gathering its
internal alignment separately from the supplied queue hint. V1's delay default,
idle bypass and adaptation-preserving buffer reset are different mechanisms;
copying one constant does not reproduce them or establish better duplex audio.

The [provider output capacity contract](provider-flow-control.md) also preserves
V1's useful bounded frame queues and cancellation-aware synthesis producers.
Capacity must propagate back to generation without pausing local microphone or
control handling. The Rust implementation reserves at most two provider packets
in flight, retains strict IPC age checks and keeps the 30-second coordinator
backlog bound. The later [provider integration](provider-recovery-integration.md)
adds playback-paced worker regressions and preserves the distinction between
modeled playback and real acoustic delivery.

For [conversation lifecycle cues](live-session-cues.md), preserve V1 main
`d5efe9d7b`'s `hal/telemetry/voice_metrics.py` distinction between monotonic
observations, interaction ownership and unknown endpoints. A software callback
does not prove acoustic onset. Reuse Rust's existing bounded `CueSink` and Unix
datagrams for both providers; no new bus, callback authority, timer thread or
network dependency belongs on this observation path.


## Borrow from current ROS 2

**Lifecycle:** ROS 2 distinguishes configuration, inactivity, activation and
terminal states. Adopt that separation in worker supervision: opening a device
does not prove useful input or output readiness. A worker restart must establish
new identity and fresh evidence. The current finite Rust runtime fails visibly
and reaps workers; a general automatic restart supervisor is not implemented.
[Managed nodes](https://design.ros2.org/articles/node_lifecycle.html)

**Actions:** ROS 2 separates an accepted goal, execution, cancellation in progress
and terminal outcomes. Use those distinctions for speech and motion. A request
to stop, device acceptance and observed physical completion are different
events. Keep a bounded result with the original action identity; a delayed
callback cannot complete a newer action. This is a contract to reuse, without
requiring the entire ROS action transport in the first release.
[Actions](https://design.ros2.org/articles/actions.html)

**Communication policy:** ROS 2 makes history depth, reliability, durability,
deadline, lifespan and liveliness explicit. Use per-channel policies and expose
incompatibility or loss. Current Rust Unix IPC already bounds messages and
separates control from audio. Every new channel needs its own overflow and
freshness rules; one generic queue setting is insufficient.
[QoS policies](https://github.com/ros2/ros2_documentation/blob/rolling/source/ROS-Framework/interfaces/topics/About-Quality-of-Service-Settings.rst)

**Scheduling:** ROS 2 supports callback groups and assigning them to different
executors. Its current event executor reduces polling work, but the documented
event queue can grow without bound under overload. Borrow explicit scheduling
and ready-event handling, while keeping bounded queues and independent audio
workers. Do not assume that using an executor, async Rust or more threads proves
a deadline. Measure scheduling delays under concurrent load.
[Executors](https://github.com/ros2/ros2_documentation/blob/rolling/source/ROS-Framework/client-libraries/About-Executors/About-Executors.rst)

**Real-time discipline:** avoid unbounded blocking and cold allocation on timing
paths, report overruns and distinguish mean throughput from worst-tail timing.
The OS, drivers and device buffers remain part of the path. Rust's memory
safety is useful but does not supply a real-time scheduling guarantee.
[Real-time programming](https://github.com/ros2/ros2_documentation/blob/rolling/source/Capabilities/Motion-planning/Real-Time-Programming.rst)

**Deployment:** a ROS node is a logical component, not necessarily an OS process.
Lamp's separate physical workers are a deliberate isolation choice. The five
servos still need one owner because they share a bus. Do not create competing
drivers simply to give each joint a process.
[Launch design](https://design.ros2.org/articles/roslaunch.html)

### Channel contracts for Lamp

These are design requirements. Existing audio/control implementations are
described in [live runtime](live-runtime.md); the remaining channels must be
implemented and qualified before claiming the whole graph runs.

| Channel | Required policy |
|---|---|
| Microphone and accepted render PCM | Ordered, bounded, timestamped blocks; sequence gaps and reference loss are explicit faults. Do not arbitrarily drop old blocks as if PCM were independent camera frames. |
| Camera | Latest useful frame, bounded in-flight ownership, acquisition age and original privacy grant. Drop/replace obsolete pending frames deliberately. |
| Environmental values | Latest per field, explicit units/source, unavailable and fault states, per-field age. A new value for one field does not refresh another. |
| Touch/button events | Bounded ordered recognized events, acquisition times, no invented multi-taps after overflow, generation/expiry checks before an action. |
| Speech/ring/motion commands | Original turn/action identity through the final writer; reject expired or superseded output. Never replay old movement commands after restart. |
| Privacy/revocation | Independent of bulk-data congestion; denied by default after lost authority. A middleware delivery receipt is not proof of physical silence. |
| Diagnostics | Bounded and nonblocking; distinguish dropped diagnostics from a clean measurement. Preserve source/configuration/fixture identities for replay. |

## Reuse existing Rust implementations before inventing infrastructure

Keep the already selected Rust libraries and reviewed DSP interfaces where they
fit. Do not rewrite working resampling, cryptography, audio backends or model
inference just to call the project a rewrite. Lamp owns the runtime policy,
hardware contracts and end-to-end experience; dependencies remain explicit.

| Candidate | Useful existing work | Decision now |
|---|---|---|
| [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) | Rust shared-memory IPC, bounded-resource communication mechanisms and large-payload transfer without serialization copies. | Evaluate before implementing a custom cross-process camera frame pool. No replacement of working control IPC is selected. ARM64 support and crash recovery need qualification; upstream desktop results are not Lamp results. |
| [dora](https://github.com/dora-rs/dora) | Rust dataflow runtime, process lifecycle, declared graphs, record/replay and observability. | Evaluate reuse for launch/supervision/tooling before building a general robot framework. Verify cancellation, bounded overload, device exclusivity and our local timing targets. No runtime dependency has been added. |
| [Zenoh / ROS 2 integration](https://github.com/ros2/rmw_zenoh/blob/rolling/docs/design.md) | Current ROS 2 can use Zenoh; its design separates discovery from direct same-host peer communication. | Consider when remote components or ROS interoperation are actually required. Network discovery does not belong in the immediate local interruption path. No network middleware is required for the desk-chat slice. |

These are inspected candidates, not benchmark winners. Any dependency decision
must include license, maintenance, platform, failure behavior and memory review.
A replacement must pass the same workload and failure cases before it displaces
current code. Retain failed trials as well as successful ones.

## Next implementation and proof

1. Finish physical validation of the already-built terminal audio shutdown fix.
   Then use paired cached-reply tests to distinguish false interruption from
   real overlap. Preserve the quiet-speaker case; do not solve echo by deafening
   Lamp during its own answer.
2. The small environmental snapshot contract is now implemented and host-tested
   with explicit units, missing values, per-field freshness and restart/fault
   regressions. Keep it I/O-free until configured sensor workers are
   implemented; qualify the installed hardware before integrating it.
   Do not auto-probe the bus.
3. Reuse V1 touch recognition and safety regression cases as those workers are
   added. Require real device exit before restart and preserve acquisition time.
4. The optional ring policy now shares actual playback events and interaction
   ownership. Qualify its physical timing/visual quality and complete crash
   recovery, then add safe motion and fresh vision. Stillness is valid. No
   automatic startup motion.
5. Compare reusable infrastructure candidates only on the workload they would
   replace: control latency under camera/audio load, bounded memory, dropped
   observations, cancellation, worker death and restart. Keep cloud response
   time separate from local runtime costs.

The general-purpose opportunity is reusable ownership, lifecycle, device
contracts and diagnostics, with robot-specific profiles and drivers. Lamp is
the first qualification target. It does not yet establish support for arbitrary
robots or a tenfold speedup. There is still no accepted matched V1-main/V2
end-to-end p50/p95 comparison.

## Candidate admission before interruption

The [input admission contract](input-admission.md) borrows V1's distinction
between local speech suspicion and destructive cancellation. It does not copy
V1's unqualified energy thresholds, three-second hardware warmup assumption,
or transcript/provider confirmation. In the current explicit-activity Gemini
configuration, Start itself requests interruption; its response is circular
evidence. The bounded local gate preserves original audio and candidate identity
while leaving pending/rejected output ownership intact. Directed qualification
still chooses VAD-only acceptance and has no new classifier accuracy result.

## Reply startup buffering experiment

Before changing playback timing, the native V1 producer and persistent TTS
stream were inspected at the pinned V1 commit. V1 requests at least 120 ms
driver latency and immediately writes available native audio; its pending
audio handles speaker contention rather than a fixed startup delay. Retain
continuous speaker ownership and bounded cancellation, while reusing the Rust
`7f24ea81d` startup-gate experiment behind explicit `--playout-start-ms 0..200`,
default zero. The [contract and held-out enqueue replay](playout-start.md)
distinguish queue-horizon modeling from driver and acoustic measurements.
A language rewrite or a passing replay does not establish V1 playback parity.

## Continuous chat core (private proposal, 2026-10-11)

The finite Rust coordinator already owns provider sessions, accepted audio,
privacy, ring phases and cancellation. The `serve` slice reuses that path and the
existing `rtrb` SPSC dependency instead of introducing another conversation engine
or a logging service dependency. It removes only the finite session/ring ceilings
and replaces qualification trace retention with bounded best-effort metadata.
Typed receipt lineage is extracted from trace lookups so dropping observations
cannot change authority. See [continuous chat core](serve-runtime.md) for limits,
including the separate mute/reopen lifecycle integration and native qualification.

## Continuous-service physical mute supervision

Keep the existing physical GPIO worker, 50 ms observation freshness, 60 ms
unmute stability and strict per-runtime privacy checks. A service-level GPIO-only
wait precedes opening any audio/provider worker; fresh physical mute returns to
that wait only after verified shutdown. Reuse the same bounded metadata writer
and termination registration through cycles. Do not revive permits, replay PCM
or claim provider-memory continuity. See [service privacy](service-privacy.md).


## Optional ring startup isolation

Inspected V1 main `d5efe9d7b73cc529b34cd4abe97624682a82ca94`:
`hal/runtime.py::_init_led` catches optional driver startup errors; RGB open failure
in `hal/drivers/rgb/rgb_service.py::start` leaves the light unavailable while audio
initializes separately. Keep that optional-component boundary. Reuse the Rust
private socket contract and existing optional ring stop/kill/status supervisor;
retain an unready child instead of dropping ownership with its startup error.
The Rust implementation adds a per-attempt socket namespace and inode-checked
cleanup, plus strict separation of mandatory heartbeat errors. It does not copy
V1 shared callback/thread ownership or add a recovery worker. No new dependency,
optical measurement, V1 speed claim or physical parity claim is implied. See
[worker startup](worker-startup.md).

## Expression: authored timing and bounded requests (2026-10-14)

This addition reviews the current checkout, separately from the pinned audit
above. It is a source review for [Expression](expression.md), with no new
hardware or performance measurement. The earlier lamp observations below are
the first builder's records in [Body](body.md), not results from Third's machine.

**Keep from V1:** the named owners in `hal/drivers/motors/hold.py`, and the
generation checks demonstrated by `hal/test/test_led_restore_generation.py`,
`test_emotion_status_expiry.py` and `test_rgb_shutdown.py`: an old restore or
late frame cannot overwrite its successor or relight a stopped output. Keep
gesture data and explicit joint units. Replace the growing motor event list
(`animation_service.py::dispatch`) with bounded, expiring requests. Do not copy
the random values in `rgb/effects.py::speaking_wave` as evidence of speech, or
the privacy and sleep changes inside `routes/emotion.py` as expression policy.

**Keep the body's measured protections:** `crates/body/src/gaze.rs` waits
400 ms of stillness, takes the median of three sightings, widens its deadband
while speaking and never searches. `body.rs` starts a thinking glance at 36
encoder counts and leaves an arrived joint alone until the new target is at
least 20 counts away. The first builder recorded three played exchanges each
way on lamp-4ace: the base was 8.3–11.7 degrees from its listening direction at
voice onset before the glance, and 1.4–3.2 degrees after it. This showed a
smaller late return, not return completed before speech, and nobody had judged
its appearance. Expression must preserve calibrated gaze and local motor
limits. A tiny requested nod can be swallowed by that real servo deadband.
The existing `clips/data.rs` nod lasts about 3.1 seconds, with over half a
second before its offsets start changing; it is not a listening-pause nod.

| What exists; primary source inspected on 2026-10-14 | What Expression takes |
|---|---|
| [BML 1.0](https://projects.cs.ru.is/projects/behavior-markup-language/wiki/Wiki) describes gesture sync points, composition, progress and warning feedback, and dropping a behavior whose timing constraints cannot be met. | Preparation, stroke and relaxation in authored data; a late gesture shrinks or is omitted; requested timing is separate from observed completion. No XML realizer dependency or BML compliance claim. |
| Maintained [ROS `twist_mux`](https://raw.githubusercontent.com/ros-teleop/twist_mux/rolling/include/twist_mux/topic_handle.hpp) selects by priority and expiry. Expired motion is masked; an expired safety lock remains locked. | Short leases and explicit precedence at the output owner. Expression expiry restores fallback; it must never clear a privacy or safety denial. |
| The current [Reachy Mini API](https://huggingface.co/docs/reachy_mini/API/reachymini) evaluates recorded moves at a chosen frequency, cancels at the next playback iteration, and composes audio-derived motion with the target pose. | Pure sampling on an injected clock, bounded cancellation and additive offsets. Reuse the pattern, without a Python service or SDK dependency. |
| [ELEGNT](https://arxiv.org/html/2501.12493v1) reports 21 valid video-study participants: expressive motion helped especially in social tasks, while motion and motor noise could interfere with lighting tasks. [Murray et al.](https://proceedings.mlr.press/v164/murray22a.html) found learned nod timing preferable to random and rule-based baselines in their robot study. | Stillness while working and sparse, event-linked listening cues. These published results motivate tests; they do not validate this lamp's pause heuristic or comfort over weeks. |

**Change the timing interface:** reuse `lamp_ipc::monotonic_us()` and identify
its shared host clock separately from publisher instances and turns. The bus
header's `at_us` has no cross-publisher clock promise. A real playback deadline
and stamped level, user speech/pause edges, interruption/cancellation and an
explicit concentration state are missing from the existing bus types. The
current `voice/state` also omits the early return and duplex-yield overrides
sent privately by `coordinator.rs` to `BodyLink`. `room/state` describes room
speech, not concentration. First wires these inputs and the body/ring owners;
Expression cannot infer the missing facts from transcript arrival.

## Records: bounded closed-session retention (2026-10-14)

This source review supports [Records](records.md). V1 was inspected at the
task's source base `96e5b83a7059fb82283381665c8667b670d9c376`, separately from
the earlier baseline above. No lamp access or new performance measurement
is part of this review.

**Keep bounded writers:** the V1
[Go logger](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/96e5b83a7059fb82283381665c8667b670d9c376/system/lib/logger/logger.go)
uses 2 MB files and ten backups; the
[HAL logger](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/96e5b83a7059fb82283381665c8667b670d9c376/hal/server_support/log_setup.py)
uses 20 MiB server files and 5 MiB usage files, each with three backups.
Preserve writer-owned rotation. V1's
[flow cleanup](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/96e5b83a7059fb82283381665c8667b670d9c376/system/lib/flow/flow.go)
removes dated files older than seven calendar days at startup, without an
aggregate byte limit. Age alone neither bounds heavy recent traffic nor
proves a writer has closed its output. Add age and allocated-byte limits
across recognized closed voice invocations; retain the active writer's files.

**Reuse filesystem and ownership primitives:** rustix is already used by
[lamp-ipc](../crates/ipc/Cargo.toml); the new
[store](../crates/records/src/store.rs) reuses it for descriptor-relative,
no-follow operations and recoverable quarantine. Reuse the existing bus and
diagnostic type. The
[systemd invocation contract](https://raw.githubusercontent.com/systemd/systemd/v255/man/systemd.exec.xml)
identifies a runtime cycle, but an invocation alone cannot exclude older
surviving or manual writers. The [guard](../crates/records/src/guard.rs)
combines manager observations with bounded host-procfs inspection, after a
fixed inventory. Its safety also depends on the voice producer exclusively
creating new output directories. Unknown ownership refuses cleanup; no age,
quiet-period heuristic or process signal substitutes for that evidence.

**Keep native journal ownership:**
[journald](https://raw.githubusercontent.com/systemd/systemd/v257/man/journald.conf.xml)
owns its active files and archived-file retention. The present journal is
shared; a unit filter is not a deletion boundary. A proposed dedicated
namespace needs the deployment owner's qualification and explicit opt-in.
No journal vacuum or competing journal writer is introduced.

**State the implemented scope:** the adapter recognizes only the private
voice-service metadata layout. Optional
[bus recordings](../crates/bus/src/main.rs), policy daily logs, finite
experiment archives and other writers lack a supported cleanup ownership
contract here; their aggregate lifetime retention is not covered. Memory
keeps its own retention and forgetting rules. Configuration defaults off,
and a successful voice cleanup does not establish a whole-disk bound.

## What this Expression review has not shown

No V1 or Rust tests were run for this source review, and no lamp was accessed.
The reviewed implementations were not benchmarked against each other. The
100 ms light response and roughly 350 ms body transition remain design targets;
the inherited Disney attribution, doubled-stumble figure and 0.6/0.3-second
gaze claim were not verified here. Playback prediction error, motor lag, gaze
calibration, glare, acoustic interference and whether the result looks natural
still require the first builder's measurements and observation on the lamp.

## Controls: keep the recognition cases, change the actions (2026-10-14)

This is a source review of the current V1 checkout for [Controls](controls.md),
not another hardware measurement. The control surface on the base is MPR121;
the TTP223 pads on the head are pet input only. GPIO and the base must stop
speech; a pet must not interrupt it. The detailed V1 behavior is in
[`physical-controls.md`](../../robots/lamp/docs/physical-controls.md).

**Keep the button's edge rule:** `hal/drivers/gpio_button.py::_on_edge`
suppresses repeated presses for 200 ms and repeated releases independently for
200 ms. A press and release less than 200 ms apart still count. Ignore a release
without a matching press, commit a hold only on release, and never delay the
first Stop for the 400 ms multi-click window. V1 measures GPIO hold duration at
callback arrival; the new injected-clock recognizer instead preserves the
adapter's acquisition times so delayed delivery cannot lengthen a hold.

**Keep the base's qualification:** `hal/drivers/mpr121.py` separates the
5 ms footprint filter from the 10 ms contact/chord qualification in the Lamp
configuration. Three selected pads must be active together; separate short
intervals do not accumulate. Qualification survives staggered lift, and the
falling sample closes the qualifying interval before it is cleared. A contact
held at startup is ignored until release. Keep the tests for these cases,
stationary footprint growth versus travel, and the measured mask sequences in
`hal/test/test_mpr121_swipe.py`. The chosen full-release grace for a qualified
stationary contact is 30 ms, shorter than V1 Automatic's 120 ms handoff grace
and equal to its fast stationary tap-to-talk floor. Moving/unqualified contacts
retain 120 ms and produce no tap or mute. This is an explicit change for the
new Stop-only tap mapping; sampled mask replay cannot establish a physical
minimum touch duration.

**Keep the head's grouping:** `hal/drivers/ttp223.py` waits 500 ms after
initialization; either edge restarts its 200 ms contact timer. The first-session
chime is at that session's end, not the first edge, and does not mean readiness.
Each completed session restarts a 1.2 s decision window; early pet recognition
can resolve sooner in V1. Controls keeps generic pet grouping and omits V1's
directional labels and early spatial-reversal path. Each response attempt starts
a 1.5 s cooldown, extended by contacts during it. The 20 ms LOW rule qualifies
only an undetected candidate pad, not every touch. Controls uses explicitly
configured known pads; discovery, pin polarity and any probe belong to First's
adapter. Do not guess wiring or
reuse the legacy fallback pins: V1's button fallback conflicts with the privacy
switch, and its touch fallback conflicts with the reset button.

**Change the action policy:** short, double and repeated taps request Stop;
a released hold of at least 2 s requests one-way `SoftwareMute`. Five seconds
and longer still mean mute, with no shutdown/reset escalation. Neither Stop nor
a missing Controls process clears a mute. Reboot, reset, sleep, Harness routing
and voice-mode toggles are not copied. Stop must reach the existing owners of
following, current and pending speech, and music; it must not cancel agent work
or directly release motor torque. Pet feedback stays small and non-interrupting;
V1's brain notification is limited to once per minute.

| Primary source inspected on 2026-10-14 | What is taken |
|---|---|
| [gpiocdev 0.8.0](https://docs.rs/gpiocdev/latest/gpiocdev/) and its [upstream repository](https://github.com/warthog618/gpiocdev-rs) provide a pure Rust Linux character-device interface. Voice already pins this version. | Reuse it for First's Linux adapter instead of another GPIO implementation; keep recognition independent of the library. No new hardware adapter is supplied by Third. |
| [Official libgpiod Rust bindings](https://libgpiod.readthedocs.io/en/master/rust_api.html) wrap the C library. | A maintained alternative where that library is already packaged; no second GPIO dependency is selected here. |
| [Linux line requests](https://www.kernel.org/doc/html/latest/userspace-api/gpio/gpio-v2-get-line-ioctl.html) are exclusive. Kernel debounce waits for a stable level and timestamps the end of that interval. [Edge events](https://www.kernel.org/doc/html/latest/userspace-api/gpio/gpio-v2-line-event-read.html) carry sequence numbers that reveal queue overflow. | Do not substitute 200 ms kernel debounce for V1's per-polarity suppression. Preserve capture time and report lost transitions as a fault; never manufacture a release from missing input. |
| [NXP AN3892](https://www.nxp.com/docs/en/application-note/AN3892.pdf) defines MPR121 touch/release hysteresis, chip debounce and overcurrent status. | Preserve the adapter's distinction between a clean electrode mask and a fault. Chip debounce `0x22` means three consecutive detections, independently of the software clocks. OVCF must not become an all-released sample. |

The physical slide remains exclusively with Voice's existing GPIO worker and
[supervised privacy lifecycle](service-privacy.md): 50 ms observation freshness,
60 ms stable unmute, and verified cleanup before reopening. A Controls message
is a request, never a microphone/camera permit. The receiving owner must retain
an accepted software inhibit through Controls loss/restart, restore it before
opening workers, and expose a separate explicit clear procedure. Bus delivery
alone is not proof that capture or playback stopped. The exact persistence and
consumer integration boundaries belong in [Controls](controls.md).

## What this Controls review has not shown

No tests or benchmarks were run for this review and no lamp was accessed. V1's
recorded traces and configured timing constants are inherited evidence, not
new results from Third. Native GPIO/I2C behavior, installed pad sensitivity,
touch-to-silence latency, physical privacy, output-owner integration and the
user's interpretation of the feedback still require First's qualification.

## Lean: static COM and a measured support footprint (2026-10-14)

This source review supports [Lean](lean.md), using task base `6a1d2745c`,
separately from the earlier baseline. It includes no physical measurement.
The library has no Body wiring; its supplied geometry is wholly synthetic and
does not borrow V1's estimated masses or limits.

**Keep the whole-body check:** V1's
[recording gate](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/recording_stability.py#L141)
uses forward kinematics, mass weighting and an offending-pose report after a
real fall. Keep that preflight intent. Add local COM vectors for every link
and the base, an actual contact footprint and explicit inward margin. The
[URDF](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/robots/lamp/urdf/lamp.urdf#L10)
contains estimated masses and zero COM origins; V1 also treats normalized
positions as degrees. Its 22/31.6/17.7 figures are not calibrated millimetres.
Require complete valid input rather than passing missing geometry or joints.

**Check the path, not only authored knots:**
[V1 resampling](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/6a1d2745c/hal/drivers/motors/recording_timing.py#L55)
interpolates joint angles after checking endpoints. Nonlinear COM can cross a
boundary in between. Use a conservative displacement/clearance bound with
bounded subdivision and an unresolved result when proof runs out. This covers
the specified interpolation, not asynchronous physical tracking or overshoot;
[OMPL](https://ompl.kavrakilab.org/core/stateValidation.html) documents the
corresponding limitation of unbounded claims from discrete validation.

**Reuse maintained rotation math:** pin
[glam 0.33.12](https://raw.githubusercontent.com/bitshifter/glam-rs/0.33.12/Cargo.toml)
with `default-features = false`, `std` and `f64`. It is MIT OR Apache-2.0,
supports Rust 1.68.2, and adds no normal transitive dependencies with those
features. Its published archive matched the registry checksum. No rotation
library was already cached; existing ndarray is array algebra.
[nalgebra](https://www.nalgebra.rs/docs/user_guide/points_and_transformations/)
and [k](https://docs.rs/k/0.32.0/k/struct.Chain.html) offer broader transform/URDF
machinery; [geo](https://docs.rs/geo/latest/geo/) offers general polygons.
The fixed chain and bounded convex footprint need less machinery.
[Drake](https://drake.mit.edu/doxygen_cxx/classdrake_1_1multibody_1_1_static_equilibrium_problem.html)
illustrates the contact-force and joint constraints absent from a static COM
test. Neither library reuse nor a measured-data label establishes motor safety.
Lean documents the ruler/scale measurements and future final-writer boundary;
the first builder owns physical calibration and opt-in integration.

Not shown.

## Surround: keep recovery evidence, replace shared ownership (2026-10-14)

This review uses the current V1 checkout for [Surround](surround.md). The
reported V1 faults are inherited records, not new measurements on Third's
machine. The complete per-piece verdict and remaining lamp work are in that
document; no V1 runtime is required by the new crate.

Keep setup's attempt identities, cancellation and distinct network/runtime
failures from `system/device/{setup,setup_runtime,setup_failure}.go`. Keep the
90-second network-loss grace, 120-second retry, 45-second join and hotspot
client guard from `system/network/{recovery,setup_failure}.go`. Preserve a
healthy LAN after runtime failure, but require fresh selected-device proof;
Ethernet cannot masquerade as a successful Wi-Fi join. Add durable generation
fences so a repeated display ID cannot revive an old result. Do not carry
forward logged passwords, credential command arguments, hardware-ID-derived
admin codes, an open recovery hotspot or a guessed network manager.

Keep the updater's lock, journal before publication, whole-file sync/rename
and previous installation from `system/bootstrap/state/state.go` and the
failure history in V1 `docs/bootstrap-ota.md`. Replace the unsigned feed and
downloaded executable script with signed metadata and targets from the first
installation. Preserve known-good recovery without accepting corrupt trust
as empty state. Reset was read beyond the earlier verdict's excerpts:
`system/server/system/factoryreset.go` wipes multiple personal stores, logs
individual failures and reboots. V2 instead requires every configured store's
durable freeze and empty receipt; partial erasure cannot become success.

| Maintained primary source inspected | Decision |
|---|---|
| [systemd service model](https://github.com/systemd/systemd/blob/main/man/systemd.service.xml), [network-online semantics](https://systemd.io/NETWORK_ONLINE/) | Independent units remain First's integration boundary. Reuse supervision semantics, but a PID or boot network target cannot stand in for a fresh capability. Startup policy supplies generation-bound plans and requires exit, quiet and rest before replacement. |
| [TUF specification](https://theupdateframework.github.io/specification/), [Tough 0.24.0](https://docs.rs/tough/0.24.0/tough/struct.RepositoryLoader.html) | Reuse signature/expiry/rollback verification. Add bounded transport, explicit trusted-time evidence, an atomic trust envelope and interruption recovery around Tough's ordinary cache writes. Never disable expiry. |
| [NetworkManager D-Bus](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html) | Reuse an installed selected manager's device/checkpoint mechanism through optional `zbus`; do not implement Wi-Fi authentication. Checkpoint commit is not atomic with profile persistence; report uncertainty and qualify device restart reconciliation. Unknown AP client count prevents automatic retry. |
| [RAUC](https://rauc.readthedocs.io/en/latest/integration.html), [SWUpdate](https://sbabic.github.io/swupdate/swupdate.html), [systemd-sysupdate](https://github.com/systemd/systemd/blob/main/man/systemd-sysupdate.xml) | Keep as evaluated installation alternatives. Whole-image adoption requires the actual boot layout/version and still needs lamp-specific quiet/rest acceptance. This task implements immutable signed application sets, not an unqualified A/B image installer. |
| [Axum service interface](https://docs.rs/axum/latest/axum/serve/index.html) | Reuse HTTP parsing/routing, with exact-origin authentication, private transport, bounded connections/requests, and a separate durable maintenance worker. Static page assets require no legacy server or frontend build service. |

The new configuration document wraps the existing Voice provider payload
without changing its strict unversioned reader. Stage complete private
generations and select only under the supervisor's quiet/exit fence. The
release-set selector similarly requires real owner proofs rather than
successful process commands; it never controls motors or privacy itself.

An integration test exposed a dependency-level regression: Tough's
unconditional Rustls default features enabled AWS-LC alongside Voice's Ring
provider, making an existing implicit TLS client panic. The pinned snapshot
in `vendor/tough` disables only that manifest default; runtime source hashes
match Tough 0.24.0 and licenses/provenance are retained. Surround explicitly
selects Ring for HTTPS. Tough's unchanged native AWS-LC TUF signing dependency
remains; its cross-compilation cost is not hidden. See the host evidence and
regression gate in Surround's handoff.

## What this Surround review has not shown

The V1 source and documented failure cases were reviewed, not rerun against
V2 on hardware. No relative V1/V2 performance, actual Wi-Fi/AP handover,
calibrated Body rest, Voice worker exit, customer setup, real flash power-cut
durability or production signing/time service was measured. Local signed
fixtures, mock manager calls and authored owner receipts are not those proofs.

## Hand controls consumed by Voice (task 21, 2026-10-14)

This review used the parent V1 sources at task base `52cc4ec8e` and the existing
Rust Controls receiver, mute file, turn owner, bus and supervised privacy
lifecycle. It adds no library or GPIO dependency. The implementation and exact
operator switches are in [Controls](controls.md#voice-and-physical-privacy).

**Keep local cancellation ahead of network work.** V1's
[button action](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/52cc4ec8e/hal/drivers/button_actions.py#L69)
starts remote speech cancellation on another thread while the click stops local
playback. Its [TTS stop](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/52cc4ec8e/hal/drivers/voice/tts/service.py#L650)
invalidates synthesis, clears pending speech and wakes parked drain queues.
Rust reuses owner revocation and the speaker's queued-PCM checks. It sends the
speaker's authority before provider, metadata or persistence work and rejects
late stopped-turn output. No new playback flag or provider cancellation wait is
needed.

**Keep privacy above gestures and teardown behind inhibition.** V1's
[mute route](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/52cc4ec8e/hal/routes/voice.py#L502)
sets software state before slow teardown; its
[hardware reconciliation](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/52cc4ec8e/hal/drivers/privacy_button.py#L158)
does not mistake an unchanged switch reading for a new user action.
Rust retains the dedicated physical reader, 50 ms freshness, 60 ms stable-open
gate and verified cleanup. An explicit software clear grants no physical permit.

**Change the action meanings deliberately.** V1's
[released holds](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/52cc4ec8e/hal/drivers/button_actions.py#L479)
escalate to sleep, shutdown and reset; clicks can unmute/wake, and a head pat can
speak. The existing Rust recognizer keeps release/debounce discipline but maps
short taps to Stop and holds of at least 2 seconds to mute, with no escalation.
Voice explicitly opts into a fresh second-hold clear through Receiver's staged
save/commit API. A retained one-way mute, publisher restart or stale queue can
never act as a toggle. Pet is a report with `action: none`.

**Reuse the durable latch, close its restart gaps.** One `HandControls` owner
survives audio cycles; a bounded pump isolates bus/filesystem reads from the
coordinator. Before audio, a durable muted recovery record guards against a
crash before a later hand request can be saved. Clean verified shutdown can
store the actual preference; a fault restart conservatively stays muted. The
mute value survives machine reboot while boot-relative timestamps are rebased.
Retained pending/replacement markers make interrupted commits explicit, at the
cost of possible operator recovery. No uncontrolled async clear write can
overtake a newer mute.

First's probe and 20-second watch showed only resting GPIO levels. In
particular, high-on-touch head polarity was inferred, not physically touched.
The two-millisecond observation-to-speaker-send target excludes bus delivery and
acoustic response. Local test/timing results must be reported with their machine;
none of these source decisions establishes lamp response time.

Not shown: physical press/pat behavior, acoustic Stop latency or fault-recovery
qualification on the lamp.

## Expression: held turns and sparse rest (2026-10-14)

This source review supports task 18 in [Expression](expression.md), starting
from `origin/lamp-voice` at `d83834cb2`. It reuses the earlier Expression
review rather than introducing another motion runtime. First's lamp results
in [Body](body.md#what-expression-may-add) are inherited evidence; Third
performed no physical measurement.

**Keep temporary yield separate from cancellation.** V1
`hal/drivers/voice/voice_service.py` ducks a local speech candidate without
cancelling its reply; a qualified provider interruption cancels the matching
turn. `hal/test/test_aec_live_reply.py` covers that distinction and a late
interrupt that must preserve a newer reply. The generation boundary in
`hal/realtime/voice_agent/openai_realtime.py` and
`hal/realtime/voice_agent/base.py`, together with
`hal/test/test_tts_turn_supersession.py`, protects against old queued or
deferred speech returning. Keep that ownership invariant, not the old
wall-clock turn-string cutoff or Python callback pipeline.

Task 18 uses a complete opt-in pair, `VoiceProgress` and `VoiceSound`, with
positive per-turn audio generations. Holding drops queued sound and the
producer's unpublished accumulator before advancing its generation. Latest
heartbeats retain hold state; resume accepts newly committed sound in that
generation. A missed hold still cannot admit a previous generation, and a
terminal event or expired Voice cannot be undone by resume. The original
pair remains the default because First owns the actual Voice publisher.

**Keep stillness and owner-specific release.** V1's named claims in
`hal/drivers/motors/hold.py` release only their own holder.
`hal/routes/emotion.py` rechecks stillness before its delayed idle release,
and `hal/drivers/tracking/body.py` rechecks tracking and holds before handing
the body back. Their regressions are `hal/test/test_still_idle_hold.py` and
`hal/test/test_body_release.py`. Do not copy a timer that can clear another
owner's hold, a growing callback queue or a random idle oscillator. Current
Rust Body already suppresses expression at settled rest and has its own
optional idle author. This task changes neither; First must choose and
qualify the eventual final-writer boundary.

**Change the authored motion, behind a catalogue.** First's five silent
lamp-4ace trials on 2026-10-14 measured 17–38 counts of actual head dip for
40/55-count requests, with roughly 0.3 seconds from requested peak to
observed peak. This is not a universal onset lag or a result from Third.
The new 1.2-second nod carries an already-qualified Listening pause into
Thinking and tapers to a committed audio schedule without delaying speech.
Its +0.08-radian clip is added to existing layers; it is not a promise of a
55-count total dip. New speech, stale evidence and concentration cancel it.
`body_lag_us` remains unknown.

Rest is one four-second +0.045-radian pitch shift after 45 continuous
eligible seconds, with 90 seconds after completion before another. It needs
fresh Resting/input-admission, Quiet-room and joint evidence, and no explicit
concentration. Losing eligibility cancels and restarts the dwell. Validation
requires zero endpoints, quiet/focus gates, finite duration, no arm/yaw/light
modulation or repetition, and bounded segment speed/acceleration. Explicit
concentration remains latched; absence now defaults false without a warning.
Quiet is not a claim that nobody is working. The original built-in catalogue
is unchanged, and actual rest motion is still outside this task's authority.

| Primary source rechecked | What is taken and what is not |
|---|---|
| [Reachy Mini SDK](https://huggingface.co/docs/reachy_mini/API/reachymini) and [conversation motion worker](https://raw.githubusercontent.com/pollen-robotics/reachy_mini_conversation_app/main/src/reachy_mini_conversation_app/moves.py) | Explicit sampling/cancellation and one motion owner. Its breathing after a short idle interval is not copied; this lamp's rest is finite and sparse. No Python dependency is added. |
| [Pipecat frame processor](https://raw.githubusercontent.com/pipecat-ai/pipecat/main/src/pipecat/processors/frame_processor.py) | Separate pause/resume from interruption and remove interruptible queued work. Independent lamp bus topics additionally require the generation and creation-time fences above. |
| [BML 1.0](https://projects.cs.ru.is/projects/behavior-markup-language/wiki/Wiki) and [ROS twist_mux](https://raw.githubusercontent.com/ros-teleop/twist_mux/rolling/include/twist_mux/topic_handle.hpp) | BML's historical start/stroke/relaxation vocabulary and current actuator priority/expiry semantics. Neither missed motion nor lease expiry may clear a safety or privacy denial. No BML compliance claim. |
| [Murray et al.](https://proceedings.mlr.press/v164/murray22a.html) and [ELEGNT](https://arxiv.org/html/2501.12493v1) | Motivation to test turn-linked cues and retain stillness during functional work. Published studies do not validate this 240 ms pause rule, rest frequency, amplitude or comfort at a customer's desk. |

The new [replay cases](expression.md#commands-and-replay) use authored arrival
times for hold/resume, give-up, a carried nod, default concentration, quiet
rest, busy/focus suppression and expired evidence. They test requested
behavior. They cannot demonstrate speaker cancellation or resting motor
motion; the associated host gate belongs in Third's log.

## What this Expression rest review has not shown

V1 regressions were inspected, not rerun for this review. No new lamp test,
relative V1/V2 speed measurement, acoustic or optical response measurement,
physical rest, combined-nod appearance or long-term comfort was established.
First must wire the producer, preserve the actual output owners, qualify
the rest author and film the resulting movement. Body lag remains unknown.

## Expression: recent playout evidence (2026-10-14)

This addendum reviews task 18's new playout paragraph at task base
`23843b78e`. First's [lamp-4ace report](work-split.md#18-expression-holding-back-for-someone-and-a-lamp-at-rest)
records 565 cumulative input refusals since 09:43, mostly playout. A separate
recording over four answers contained 353 reports: roughly 20 ms pieces,
published a median 1.7 ms after their onset, with at most 18 ms remaining.
None in that recording was too far ahead or overlapping. These are different
populations, and the 353-message recording was not supplied to Third. First
plans earlier publication from Voice's 350 ms buffer; this consumer change
does not establish that producer boundary.

**Keep a bounded meter, with truthful evidence.** V1
`hal/drivers/voice/_internal/live_playback.py` retains a level for 250 ms on
a monotonic clock. Its `playback` function computes post-duck RMS and stamps
the meter before the speaker write; `level` also applies the observed mixer
gain. Successful writes have a separate `record_written` timestamp, from
which `is_playing` expires after 250 ms. The ordering in
`hal/drivers/voice/tts/service.py::_WatchedStream.write` confirms the
distinction. `hal/test/test_live_playback.py` covers failed/pending writes
not activating playback, meter expiry, identical speaker/AEC PCM and mixer
scaling. Keep bounded recent evidence; do not treat PCM processing, driver
acceptance and acoustic observation as interchangeable. No duck policy,
mixer probe or new external dependency is taken into Expression.

**Change the current-interval-only assumption.** Voice's
`crates/voice/src/expression_out.rs::Timing` gathers about 20 ms of sound,
flushes an odd piece after 15 ms, uses the maximum constituent ring level,
and publishes only while at least 2 ms remains. `sounds` dates it from the
speaker queue readback. Its existing piece regression assumes 30 ms queue
lead. At the task base, Expression's `accept_playout` rejected an interval
already ended at receipt; `advance` removed ended intervals before a tick.
A fresh, correctly dated report could therefore be refused or missed by
the next 20 ms tick.

The selected contract adds `timing.recent_playout_us`: zero by default, or
20,000–200,000 microseconds when enabled. The separate
`crates/expression/playout-behaviours.json` selects 100,000 microseconds with
the legacy Voice protocol. The built-in and `rest-behaviours.json` catalogues
remain unchanged. The initial 100 ms is an unmeasured tuning choice.

Retain one latest ended interval and fade its level linearly with age from
its immutable end time, reaching zero at the configured deadline. Receipt,
heartbeats and repeated ticks cannot renew it. A current interval wins,
including a level of zero; a future interval cannot supply light early or
replace recent evidence merely because it arrived. Expression's existing
anticipation for Body may still use committed future sound, while recent evidence
is sampled only at real `now`. Recent modulation requires an owner already
Speaking; it cannot promote Thinking or claim input readiness. A just-ended
valid report is heard evidence, not an expired-interval fault or proof that
sound is still acoustically present.

Preserve the existing boot/session/turn/request, sequence, interval-order,
generation and resume boundaries, 200 ms publication expiry and bounded
future queue. Hold, resume, terminal events, owner changes and stale Voice
must clear recent evidence with the other transient sound state. Neither a
cached level nor a missed hold may revive old audio. Final behavior and gate
evidence belong in [Expression](expression.md) and Third's log. First owns
Voice and lamp-4ace integration; Fifth now owns Body and qualifies its response
on its own lamp. Third accesses neither lamp.

## What this recent-playout review has not shown

This source review ran no tests or benchmarks and claims no implementation
gate result. Third did not receive or replay First's 353-message recording.
No acoustic, optical or motor timing, 100 ms tail appearance, or improvement
on a lamp was measured. The grace window and fade need physical observation;
First's planned earlier publication remains separate work.

## Local Policy judge (task 17, 2026-10-14)

The existing [addressee study](said-to-whom.md) is the alternatives review:
retain a small local classifier of available signs, then qualify a slower
semantic judge separately. This implementation adds no inference library,
downloaded asset or Rust dependency. Serde, the existing `Decider` contract,
private JSONL writer and bus already provide the required interfaces. Seven
small logistic heads and a separate local combination make missing features,
coefficients and fitting partitions directly inspectable. A larger model is
not justified by independently labelled data available in this checkout.

**Keep V1's distinction between evidence and ownership.** At task base
`574c2e5ed`, V1's `hal/drivers/voice/_internal/wakeword_focus.py` grants
follow-up focus only after a caller enforces the wake gate, keeps turn/run
identity and cancellation, and waits for authorized processing/playback to
settle. Its `is_addressed` returns wake/focus evidence when wake gating is
enabled. V1's `hal/realtime/resources/system_prompt.md` additionally says
an open conversation window or a name mentioned to someone else is not
sufficient to establish address. The local judge therefore does not become
an actuator gate. Its 20-second Policy context is only a sign; stable source
identity, playback completion and semantic attribution are not ported by
adding this model. Existing Voice ownership/privacy/cancellation stay at
their final boundaries. No behavior parity with that whole V1 path is claimed.

**Reuse the existing question contract, keep the calibrations separate.**
The local `Decider` answers the exact same seven typed questions, but the
signs cannot recover all their semantics. Initial coefficients are authored
and marked unfitted; two unsupported semantic heads remain constant priors.
Jev's byte-pinned text and fitted coefficients are unchanged. A separate
local choice head operates on centered local probabilities; only the two
resulting choice probabilities are mixed. Session-disjoint train,
calibration and test partitions prevent the fitter's measured test labels
from selecting model weights. The seed mixture is not passed off as fitted.

**Change the stand-in explicitly.** The original Policy fallback can answer
when the room is quiet. The new opt-in path chooses `look_up` when there is
no timely slow answer and no call evidence. It publishes that hypothetical
choice once. One outstanding slow request and a fixed admission deadline
prevent a late reply from revising an older choice or attaching to a newer
utterance. No new worker is created for an overdue request. The deadline
tests hold the slow request explicitly; they are not measurements of a
cloud service or of a lamp speaker.

**Reuse the strict busy floor as an explicit comparison.** The source
mirror in `policy/baseline.rs` is audited against Voice's `called` and
`decide_busy(strict=true)` at `574c2e5ed`. Tests cover its token matches,
fail-open unknown room, acoustic-name insufficiency and explicit snapshot
requirements. Policy's completed text, 20-second conversation and near-face
receipt view differ from Voice's first-output prefix, 45-second state and
any-face expiry. The comparison keeps those two evidence tracks separate
instead of claiming a replay of the live gate from missing inputs.

**Keep filesystem work away from the new decision path.** The opt-in log
writer retains the existing private file behavior but adds one bounded
worker and nonblocking queue admission. Tests exercise a deliberately held
append, queue overflow, cancellation, real append failure and private modes.
Successful queue admission, successful append and durable storage are three
different facts; only the first two are measured here. The legacy synchronous
recording path remains unchanged unless the second judge is enabled.

The exact workloads, machine, timings and failures are in the
[task 17 verification record](verification/local-judge-host-20261014/README.md).
No comparable V1/V2 workload or labelled real-office dataset was run here.

**Re-audit after the next fetch.** First added a `named_sure` per-request
latch in `e07449e13`, before calling the earlier busy rule: an eligible score
of at least 0.999 permits output even without a facing face. Task 17's
initial mirror was correct for its pinned base, but omitting this new branch
would make the current comparison stale. The follow-up therefore audits
`first_output_busy` too, requires the actual latch in a captured first-output
snapshot, and uses a declared threshold projection only for Policy's
`same_snapshot` track. Missing latch data is rejected rather than defaulted.
The local no-call guard accepts the same strong evidence; the weights and
judge combination are unchanged. A nine-case authored comparison adds a
sure-name call, with the old eight-case evidence retained under its original
revision. Neither comparison is a live Voice replay or an accuracy claim.

Not shown: fitted lamp accuracy, improvement over the busy-room floor,
acoustic response or interruption latency, speaker-to-face association,
actual ARM64 execution, or any lamp action qualification.

## Schedule: durable reminders and timers (2026-10-14)

Task 6 starts from `origin/lamp-voice` at `d0a2b5c00`. The implementation
and local interfaces are in [Schedule](schedule.md). Third inspected the
parent V1 sources; no V1 schedule test or lamp experiment was run for this
review. The 108 top-level test functions counted in `system/schedule` are
an inventory, not 108 passing comparisons.

**Keep useful calendar cases, change delivery ownership.** V1's
`system/schedule/spec.go`, `spec_test.go`, `times_test.go`, `runner.go`,
`runner_test.go`, `store.go`, `intent.go` and schedule handlers explain
[GO-3.19](v1-verdicts.md): a backend-confirmed list delivered over MQTT,
local intents with retry identities, strict future selection, calendar
recurrence, busy deferral and honest reporting that success is a handoff.
The runner skips work more than 30 minutes late. Its minute poll, five-minute
interval floor and roughly five-minute daily jitter serve paid agent prompts;
they do not suit a requested short timer. The new task explicitly preserves
missed reminders, so none of those three delays or the discard window carries
over. Atomic file replacement without syncing, and speech before saving its
result, do not prove power-loss durability or heard-once delivery. EXT.16's
unrequested wellbeing prompts remain excluded.

**Use familiar controls, make their limits explicit.** The published
[HomePod guide](https://support.apple.com/en-nz/guide/homepod/apd82db5cb0d/homepod)
describes named timers, remaining time and pause/resume; snooze there is an
alarm feature. Google's [Tasks guide](https://support.google.com/googlehome/answer/16722329)
describes dated/repeating requests and rescheduling. Amazon's
[reminder guide](https://digprjsurvey.amazon.com/csad/help/node/GVMW5E8YCNR6AC6E)
describes explicit snooze and missed-reminder indications. Take identifiable
items, a clear date/time confirmation, explicit cancel and due-item snooze,
and daily/weekly repetition. This first component does not implement paused
countdowns, alarms that ring continuously, monthly rules or personal identity.
None of those vendor pages establishes an outage replay or acoustic receipt
contract for this lamp.

**Reuse Jiff, not a current-offset date calculator.** Voice's
`local_time.rs` formats the present offset from TZif; it has neither named
future civil-time resolution nor a trust proof. Jiff 0.2.38 is already in the
workspace. Use its calendar and explicit
[disambiguation](https://docs.rs/jiff/0.2.38/jiff/tz/enum.Disambiguation.html),
with the locked bundled IANA database so the same build uses the same rules
on this host and the lamp. A one-off gap is refused; a fold requires a choice.
Repeats use the documented Compatible policy while retaining the requested
civil date and time. Even two dates mapped to one instant by a whole-day
zone skip retain separate delivery tokens. Elapsed timers use Linux BOOTTIME,
with a stored wall deadline only for recovery on another boot after clock
trust is available. SystemTime corrections do not retime same-boot countdowns.
Changing bundled zone rules requires qualification of existing descriptors;
there is no silent rule migration. Jiff's MIT/Unlicense licensing and bundled
data provenance remain upstream; no custom timezone parser is added.

**Reuse SQLite transactions and typed IPC, retain the pending work.** The
workspace already uses rusqlite 0.40.2 with bundled SQLite. For the small
128-item bound, Schedule stores one versioned snapshot in a private SQLite
file with DELETE journalling, FULL synchronization, `fullfsync` where supported,
and directory synchronization before acknowledgement. See SQLite's
[synchronization contract](https://www.sqlite.org/pragma.html#pragma_synchronous)
and [atomic-commit assumptions](https://www.sqlite.org/atomiccommit.html).
It rejects unknown/corrupt schema and insecure paths instead of treating
errors as an empty schedule. A failed post-commit sync is ambiguous: the
owner stops, and restart/load discovers what committed.

The local JSON-line boundary resembles Memory and Actions, but bounded client
workers keep a slow reader away from due work; a separate observer withdraws
readiness when the storage owner stalls. The socket returns a durable receipt.
Compare-and-set guards prevent late or reordered mutations from silently
repeating an action; the exact last mutation's receipt survives restart.
The typed `schedule/due` Latest value carries only the oldest pending token
and counts. It is a discovery hint, not the queue, speech permission or an
acknowledgement. Voice may be busy, gone, muted or offline without consuming
it. First retrieves a current token privately, owns all speech admission,
and acknowledges completed output. A crash between output and acknowledgement
can repeat a notice; exactly-once audible delivery is not claimed.

## What this Schedule review has not shown

Vendor documentation was studied, not benchmarked. V1 tests were read, not
rerun. This source review establishes no lamp flash flush behavior, power-cut
survival, real clock synchronization, Linux suspend/reboot timing, future zone
migration, privacy/admission integration, audible completion or due-to-speech
latency. The separate host test and timing evidence is reported as host
results, and First still owns Voice integration and lamp qualification.

## Memory health (task 28, 2026-10-14)

Keep the existing `lamp_msgs::Diagnostic` contract: component name, level,
fixed message and string key/value pairs. Actions already reports once a
second, but its `serve_reporting` publishes from the socket loop. Memory
changes that ownership: one bounded worker owns all bus work and retries;
the socket owner writes fixed atomic state. A bus publisher's directory
scan is filesystem I/O even when its datagrams are nonblocking.

V1's `hal/runtime.py` health response reports hardware availability, not a
Memory service. `system/healthwatch/service.go` qualifies failures and spaces
restarts; this task adds no restart policy. The context manager in
`hal/realtime/context_manager/base.py` preserves unsummarized records after
empty model output, with regression cases in
`hal/test/test_realtime_summarizer_health.py`. Keep that safeguard and do not
call an unconfigured summarizer a Memory fault. The bounded telemetry queues
in `hal/telemetry/client.py` and `system/telemetry/telemetry.go` distinguish
delivery failures from operation failures. Keep that distinction, with no
queue of per-request health events and no V1 runtime dependency.

Current ROS 2's [DiagnosticStatus definition](https://raw.githubusercontent.com/ros2/common_interfaces/rolling/diagnostic_msgs/msg/DiagnosticStatus.msg),
[Updater](https://raw.githubusercontent.com/ros/diagnostics/ros2/diagnostic_updater/diagnostic_updater/_diagnostic_updater.py)
and [windowed checks](https://raw.githubusercontent.com/ros/diagnostics/ros2/diagnostic_updater/diagnostic_updater/_update_functions.py)
support reusing this component-level report and explicit recent window.
No ROS client, new message type or external crate is needed. Use the existing
SQLite transaction machinery for optional TEMP aggregate counters, so
reporting adds no competing database reader. Legacy counts stream off the
socket thread and reject snapshots overlapping a request.

The [Memory contract](memory.md#optional-health-reports) distinguishes
handler time from client deadlines, completed answers from in-flight work,
and unknown counts from zero. Tests cover real socket/bus behavior, rollback,
count expiry/cascades, typed storage failure/recovery and a held publisher.
First enables Memory's explicit flag and adds `--expect memory` to Health;
no deployment files are changed here.

Not shown: report cost on lamp flash, long-lived filesystem stalls, physical
power loss or the actual Health unit. Host results belong to Fourth's log;
they do not establish the lamp's response deadlines.

## Task 11: keep Harness's client policy and V1's uncertainty rules

Reviewed on 2026-10-14 at OpenHarness
`748269145cb76d1a164141f6bc55758e013334b0`, refreshed to
`0e6b16d78fd80697dea252376f19d7acf49d240f`.
[The pinned contract review](tasks-contract.md) records the response variants,
caller identity, source links and unresolved boundaries. Use the installed
CLI with its [account and caller policy](https://github.com/autonomous-ai/openharness/blob/0e6b16d78fd80697dea252376f19d7acf49d240f/cli/src/agentClient/commands.ts)
and existing machine trust. Do not port V1's
Device pairing/crypto or bypass CLI policy with a raw daemon mutation.

Take V1's reservation-before-send, durable result-before-cursor ordering,
identity-bound replay handling, permission-notice rules and distinction
between admission and played speech. Rebuild those as bounded Rust state
and fixtures, without copying the old shared runtime or assuming its Device
result correlation exists for CLI tasks. Existing `new`, `show` and `wait`
provide launch/state/transcript observations, not a correlated completion
receipt. A lost initial launch reply remains uncertain and never authorizes
automatic resend. A known pending creation ID also has a source-identified
lookup gap after reconciliation to a started agent.

Two checks remain especially easy to overstate. Explicit `ask` preserves
Codex/OpenCode's installed permission configuration; it does not create a
fresh sandbox. A full machine ID and listing preflight do not close the
CLI resolver's later label-fallback race. Returned-ID validation can detect
a mismatch after launch. These need upstream/owner qualification, not more
confident local wording.

Reuse the reviewed Schedule storage semantics in Tasks without depending on
the Schedule service: a private locked SQLite snapshot, strict schema, bounded
reads/pages, DELETE/FULL synchronization and directory sync, and a poisoned
owner after uncertain commits until drop/reopen recovery. Tasks raises the
snapshot/database bounds to 8/32 MiB, with a 40 MiB journal bound. Harness's
[MIT CLI](https://github.com/autonomous-ai/openharness/blob/0e6b16d78fd80697dea252376f19d7acf49d240f/cli/package.json)
remains an external provisioned dependency; existing serde,
rusqlite and rustix supply local serialization/storage/platform primitives.
No new MCP/WebSocket client or copied authentication material is needed.

## What this Tasks review has not shown

The contract review exercised no real Harness, account, agent or lamp.
Scripted tests cannot establish the installed release, strict target binding,
remote completion correlation, permission behavior, flash power-cut survival,
audible delivery or end-to-end latency. Storage test evidence and runtime
limits are reported separately in [tasks.md](tasks.md); V1 tests cited here
were inspected as cases, not rerun. No permission, completion or successful
speech is inferred from a process exit or observed transcript.

The Task 33 touch review also supplies no lamp calibration, physical contact
latency, electrical contention result or owner-applied Sleep/Wake/Stop. Its
host fixtures and native build gate do not establish a physical response.

## Mood: bounded inner state (task 35, 2026-10-14)

This review starts from `79c98be3b67015f9e305f1a4dcea4040191895f0` and
[Creature](creature.md), for [#713](https://github.com/autonomous-ai/lampOS/issues/713).
[Mood](mood.md) records the component contract, authored constants and
qualification boundaries. Research and source inspection precede building;
neither establishes a result on the lamp.

**Keep character state separate from output ownership.** Anki's
[goal-based animation patent](https://patents.google.com/patent/US11203122B2/en)
describes a simulated state with one or several dimensions that influences a
separate animation selector. It explicitly allows dimensions that are not
identifiable human emotions. This is a vendor design disclosure, not a
validation of emotion inference, decay rates or companionship. Take that
separation and a small state representation; do not import its animation
search, behavior engine or a dependency on an Anki runtime.

**Prefer continuous style with exact sleep rules.** A discrete collection of
calm/curious/glad/startled/shy states makes combinations and transitions harder
to explain. A larger appraisal model or language-model judgment would add
inputs, latency and private content without evidence of benefit here. Use
bounded awake, energy and warmth values, one attention target and a separate
sleep latch. The numeric nudges, decay, day bands, cooldowns and thresholds
are authored controls to qualify, not biological constants. In particular,
quiet returns warmth to its calm baseline; it does not punish being ignored.

**Use movement evidence within its actual limits.**
[Lasseter's original paper](https://www.cs.cmu.edu/afs/cs/academic/class/15462-f13/www/lec_slides/Lesseter.pdf)
supports readable staging and changing the style of the same action with
character state; it is animation practice. The
[ELEGNT study](https://arxiv.org/html/2501.12493v1) compared video ratings from
21 valid participants. Its stronger social-task results and complaints about
unexplained or exaggerated movement favor restrained, contextual expression.
Its functional-task result is no significant difference in selected metrics,
not proof that expression never helps a functional task. The
[Haptic Creature study](https://doi.org/10.1145/1957656.1957820) found arousal
easier to communicate than valence through touch; it does not validate a
brightness-to-emotion mapping for this lamp. Mood therefore publishes facts
about its own authored state, with no new motion, light or speech consumer.

**Keep sleep stable, without acquiring privacy authority.** Amazon's
[Astro documentation](https://digprjsurvey.amazon.com/csad/help/node/G2HDFAZG923JG4W7)
provides a vendor example of do-not-disturb exceptions for scheduled items;
it is not measured evidence of noise immunity. Creature's stricter allowlist
is the requirement here: after sleep, only a fresh name, touch or permitted
user-set reminder wakes. Morning, company and incidental noise do not.
V1 `system/ambient/service.go` clears its local sleep flag for broad sensing
and chat events; change that policy rather than porting it. Its
`system/ambient/sleep_test.go` and `hal/test/test_sleep_persistence.py` expose
useful boundaries: restart must not silently clear an authoritative mute,
and an unavailable owner must not be guessed ready. Keep those boundaries,
but do not copy HAL sidecars or add private Mood persistence. Task 35 starts
neutral from the current local clock, with no attention; any durable privacy
state remains with its existing owner. Mood waking is never permission to
open a microphone, clear a mute, move, light the eye or speak.

**Reuse typed observations and bounded expiry.** Existing name, Controls,
Eyes, Ears, Air and Schedule metadata provide the available inputs. A current
face track is temporary and scoped to its publisher, not an identity or a
familiarity history. Keep freshness, source ordering and bounded input work;
do not classify voice words to invent thanks, laughter or a sleep outcome.
The Speech adapter checks only whether text is blank before discarding it;
no words enter retained state. First's
[#713 resolution](https://github.com/autonomous-ai/lampOS/issues/713#issuecomment-6167222890)
keeps thanks/laughter engine/replay-only and leaves future #711 Sleep/Wake
intents distinct from mute. Current Stop/Pet metadata is advisory. The pure
state engine owns no inference, file, socket, bus call or worker; reuse Jiff
for named local time and rustix for the monotonic clock outside it. Bounded
publication belongs to a separate worker, with freshness checked by any
future final consumer.

Not shown: calibrated emotional interpretation, live semantic feedback or
completed Controls sleep integration, any recorded real day, real-device
latency or a change in the lamp's visible behavior. First accepted a synthetic
24-hour replay for handoff; it is authored evidence of deterministic policy,
not observation of desk life. The promised real-day recording is still pending.
Host gates and timing results must be reported separately after they run;
the V1 tests above were inspected, not executed for this review.

## Task 31: Schedule tools and due speech

Reviewed 2026-10-14. V1's provider prompts in
`hal/realtime/resources/system_prompt{,_gemini,_pipecat,_openai}.md` delegate
reminders because their realtime runtime owns no scheduler. Keep the rule
that a reminder is confirmed only after the actual service succeeds. Replace
that delegation narrowly with the six opt-in local Actions tools backed by
the existing Schedule protocol; do not copy V1's shared callback ownership or
add a runtime dependency on HAL.

The maintained [Gemini Live tools guide](https://ai.google.dev/gemini-api/docs/live-api/tools),
[WebSocket contract](https://ai.google.dev/api/live) and
[FunctionDeclaration reference](https://ai.google.dev/api/generate-content#FunctionDeclaration)
were checked on that date. Keep the existing session/call-ID response path.
This link uses `parameters`, not `parametersJsonSchema`, so the new declarations
use the supported small Schema vocabulary. Actions independently enforces
unknown-field, byte, time and cross-field constraints. A tool cancellation or
missing response does not establish that a durable mutation was undone.
No model migration or shared Gemini wire change is needed.

Reuse Schedule's checked time resolution, stable store/item/delivery identity,
revision guards, exact mutation receipts and private SQLite DELETE/FULL store.
Add a separate bounded delivery ledger, without reminder text, that records
preparation before admission and qualified whole-turn completion before Ack.
The existing one retained mutation receipt may be overwritten: reconcile it
with the exact current item, never treat absence as a newly proved Ack. The
later restart policy (16 October) allows a new exclusive Voice claim to retire
an earlier process's uncompleted attempt and offer it again. This deliberately
prefers a possible repeat to a permanently lost reminder; a recorded completion
still reconciles without replay. The successor-admission repair does not change
that policy: it records an already qualified completion before its Reply can
be discarded by a new line.

Rechecked against V1 main `1b5f7a78b` on 25 October:
`system/schedule/runner.go` defines successful `RunReport` as handoff, not
completed or heard speech, and `fire` advances a successful occurrence. Keep
Rust's stronger provider-terminal/final-cursor join instead of substituting
that handoff success. Reuse the existing coordinator completion path and
retained-input ownership update; add no second acknowledgement path, wait,
worker or storage operation to admission. The V1 source was inspected, not
executed for this comparison.

Reuse Voice's actual Controller, input admission, provider session/privacy
checks, output permits and speaker `SpeechRetired` handling. One generation's
retirement is insufficient when more PCM follows; completion joins successful
provider terminal with every generation's final retirement and the ordinary
whole-reply settlement. Local failure audio, empty output, reset, gaps,
interruption and cancellation cannot count. Reminder text cannot invoke any
tool or request the autonomous camera continuation. First settled the room
policy on [#709 at 02:13 on 2026-10-15](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386):
a due reminder is requested work and speaks at the next local silence with
normal readiness, whether RoomTold says Unknown, Busy or Quiet. Room
classification does not delay submission or withdraw its output. Keep the
existing actual-input, privacy, owed-follow-up and output-ownership checks.

Follow-up reviewed 2026-10-25: V1's actual
`hal/realtime/resources/system_prompt_gemini.md` distinguishes a delegation
acknowledgement from a completed action; its ordinary prompt likewise delegates
reminders to the scheduler. Keep that distinction. Reuse V2's existing shared
reminder completion slot for the skill's say result, after the coordinator's
completion join. Change only the inconsistent success report: a reset-invalidated
receipt cannot abandon the reminder while telling its skill `spoken`. Read the
recorded `Complete` result without consuming or recreating it; readiness to
complete is a different, earlier state. Keep the existing playback, owner,
privacy, failure and durable-Ack checks, with no extra wait, I/O or dependency.
Authored ticket/cursor fixtures cover the joined outcomes; real-device delivery
and acoustic quality remain separate evidence.

Use existing serde, rustix and SQLite dependencies. The new cumulative-deadline
Unix client supplies only Schedule's paths; ordinary off-state tool behavior
is preserved. All ledger/filesystem/socket work lives in one supervised child
outside the urgent audio loop and across mute cycles. Anonymous datagrams and
a shared claim FD bound the parent's polling without trusting a PID, timeout
or reopened store as proof that old audio retired. No replacement consumer is
spawned in that process; the claim is held until the actual Voice process
exits, including after failed cleanup. Process creation and a kernel-stalled
filesystem operation have no claimed userspace hard deadline.

The checked-in lamp-4ace Voice unit selects realtime RR20. Linux preserves
that policy across fork/exec; a new process group does not reset it
([Linux scheduling](https://man7.org/linux/man-pages/man7/sched.7.html),
[Rust process groups](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.process_group)).
Use the safe API of MIT-licensed `thread-priority` 3.1.1, pinned only for Linux,
to put the child in normal Other/static priority 0 with Min/nice 19 before
claim or storage work. Read back policy and niceness and refuse startup on
failure. Its [implementation](https://docs.rs/thread-priority/3.1.1/src/thread_priority/unix.rs.html)
sets policy and niceness separately; partial success is still a startup failure.
The library's Rust 1.85 minimum fits this workspace's pinned 1.96 toolchain.
No inline unsafe, parent-policy change, external `chrt` command or privilege
increase is required. Lower normal priority can delay storage under load;
this is isolation policy, not a measured latency improvement.

Host fixtures cannot establish real-model schema acceptance, semantic fidelity,
physical privacy/Stop latency, actual ALSA/acoustic end, Orange Pi flash stall
cost or power-cut survival. The completion-before-Ack design also cannot close
the interval after sound finishes but before completion persistence: it keeps
that occurrence uncertain. The exact boundaries and First's lamp cases are in
[Schedule](schedule.md#optional-voice-consumer) and [Actions](actions.md).

## Second voice route (task 11, #754, 2026-10-15)

The [second voice route](second-voice.md) speaks V1's services from Rust.
What was taken and what was not:

| Choice | Evidence and consequence |
|---|---|
| #258, V1's OpenAI-compatible STT/TTS (Python, on `main`) | Took only the idea of the endpoint shape: an OpenAI-compatible base URL for chat and speech. No code: lampOS is Rust only. |
| #681, Phonon | Not taken: weights under NDA installed by hand, a Python worker, English only, a 350 MB model. |
| V1's configuration field names | `llm_base_url`, `llm_model`, `stt_base_url`, `stt_language`, `tts_base_url`, `tts_provider`, `tts_voice` as in `system/domain/device.go` and `hal/models.py`, with V1's fallback (empty STT or TTS URL or key is the LLM's). Each `*_api_key` becomes a mode-0600 `*_credential_file`, as `provider.json` has it, so a V1 `config.json` with inline keys is converted by whatever carries V1's data over. |
| V1's protocols | Deepgram's websocket with V1's nova query (`hal/drivers/voice/stt/{deepgram,autonomous}.py`), streamed `/chat/completions`, OpenAI `tts-1` and ElevenLabs PCM as `hal/drivers/voice/tts/{openai,elevenlabs}.py` ask, the proxy's `/ai` to `/ai/v1` rule (`_ensure_openai_v1`) and the ElevenLabs voice names (`VOICE_IDS_BY_LANG`). V1's Deepgram default, `flux-general-en`, is not used. |
| Dependencies added to `lamp-voice` | `tokio-tungstenite` 0.30.0, `futures-util` 0.3.34, `rustls` 0.23 (ring, tls12), `webpki-roots` 1 and `reqwest` 0.13 with `default-features = false` and `rustls-no-provider`, so no OpenSSL on the lamp. All were already in `Cargo.lock` through `lamp-gemini` and `lamp-surround`; the lock gains no package. `tokio` gains the `net`, `sync` and `io-util` features. |

## One lost spoken line retried (#705, Step B, 2026-10-18)

V1's `hal/drivers/voice/_internal/realtime_turn.py::_commit_turn_output`
rebinds and uploads a captured turn once if its session changes during
flush/commit. It deliberately does not replay a failure while consuming a
committed response; `hal/test/test_capture_session_replay.py` checks that
recovery and audio upload are not repeated after such a failure.

Keep the single retry and the prohibition on repeating a tool-bearing or
partly answered request. Change the eligibility boundary for this opt-in:
`LAMP_RESEND_LOST_LINE=on` can retry an ended request on the Rust worker's
replacement connection when no answer or tool call was observed, even if
its End had already been sent. The existing supervisor's request/session
lineage and stage tracking supply that evidence; cancellation and microphone
generation remain authoritative. This is a deliberate extension beyond V1's
uncommitted-upload retry, not a claim of behavioral parity.

Reuse the existing Gemini supervisor, bounded input queue and 2 ms worker
upkeep. Keep at most 15 seconds of PCM and accept a retry only within five
seconds of End; do not add a transport, service, dependency or retry loop.
The worker and supervisor both enforce one accepted retry. Queue-full End
waits without blocking control. The [host verification](verification/resend-lost-line-host-20261018/README.md)
covers lost connections, output/tool exclusions, repeated loss, capacity and
cancellation. The switch remains off pending the lamp check; reconnect and
acoustic latency on hardware have not been measured.

## A holding phrase followed by the promised answer (#1579)

V1's `hal/realtime/voice_agent/gemini_live.py::_async_receive_turn` uses a
bounded response-outcome check for extended thinking, and
`hal/realtime/response_outcome.py::spoken_response_complete` treats a future
promise as incomplete. Keep the interruption, routing and playback checks;
change the recovery to one same-session written note for an exact, bounded
holding-only transcript with no function call. Reuse V2's normal turn owner,
speaker retirement and quiet-input gates. This adds no semantic classifier,
fallback route or retry loop. The switch is off until the lamp check; the
[behavior and host verification](holding-answer.md) distinguish queued-note
latency from the unmeasured audible answer.

## Presence and the light going out (#1677, 2026-10-19)

V1's `hal/drivers/sensing/presence_service.py` dimmed the light to 20%
after 5 minutes without camera motion or a voice turn or gesture
(`HAL_IDLE_TIMEOUT_S`), cleared it after 15 (`HAL_AWAY_TIMEOUT_S`), told
the agent to announce it, and restored the person's light on motion, never
while asleep and never over a light they had turned off. Keep: the
person's own light restored as it was, nothing while the lamp sleeps,
nothing over a light already off. Change: the evidence is the eyes' faces
and a voice heard (`voice/heard`), since V2's eyes find faces, not motion;
one stage after 10 minutes instead of two, faded over 30 s by the ring
(`LightRequest::fade_ms`) so it reads as falling asleep; out is the ring's
ember, never dark. Drop: the announcement. The signal is one topic,
`room/presence`, said by the eyes for every reader (the light and #1673's
Messages), never "nobody" while the eyes are not looking. Off unless
`LAMP_PRESENCE=on`; not run on a lamp. See [presence.md](presence.md).

## A dedicated reader of service events (#2789, 2026-10-23)

V1's `system/lib/voicefile/voicefile.go` provides retained-root confinement
and regular, bounded file reads. Keep those protections and the V2 service
writer's exclusive files and retained descriptors. V1's profile/sample
access and flow-log permissions do not define who may read this service's
private sessions, so they are not copied as a sharing policy.

Reuse the workspace's rustix descriptor operations and existing Unix group
mechanism, with a bounded local group-file lookup. Do not introduce NSS or
network lookup on startup, global umask changes, a privileged reader daemon,
recursive historical widening or per-turn permission work. A named explicit
group applies only to new service event files, and Records retains its
role-specific ownership, layout, ACL and deletion checks. Private defaults,
health and quarantine remain private. See [Records](records.md#optional-session-reader-group-2789)
and [Peek](peek.md#session-reader-access) for the rollout contract.

The group lookup and fixed permission setup run once before service workers;
the host target is under 50 ms from preparation entry to grant or private
failure. Those permission operations add no per-turn or rotation work.
Timing and a dedicated account's actual access must be measured separately
from metadata-stat assertions. A bus view that excludes raw speech before
delivery, its subscriber permissions and the authorized lamp install/restart
remain separate acceptance work. Filtering inside Peek cannot protect data
already received; no device result is claimed by this source change.


### Share a producer projection, not the private trace (#3211)

V1's `system/lib/flow/flow.go::Event` carries an arbitrary `Data` map, and
`system/server/agent/delivery/http/handler_api_flow.go::FlowLogs` downloads
those JSONL bytes behind the existing authenticated route. That is not a
field-level privacy contract for a dedicated Unix reader. V2's original
`ServiceTrace::submit` likewise serialized arbitrary Values, including the
coordinator's transcript text; Peek's field filtering did not prevent another
program under the reader account from reading those words.

Keep private/finite traces, bounded packets/queue/rotation, exclusive retained
descriptors, health reporting, and verified private rollback unchanged. After
a successful sharing grant only, serialize a typed flat projection of the
fields actually consumed by Peek's Turns reader: fixed kind/status vocabularies
and unsigned timing/count fields. An unknown record becomes a fixed generic
row; no original unknown kind, nested object, words, path or identity survives.
This changes only stored observations, not the coordinator's in-memory events,
cue interpretation, capture, privacy or output authority. No new dependency,
account, bus capability, filesystem permission or default is introduced.

Controls inspect every shared segment's bytes for planted private values,
verify retained structural timings and counts, exercise the actual coordinator's
Transcript through the shared writer, and inspect all files from the dedicated
GID. Private, finite, lookup-failure and verified-rollback traces retain their
original contents. The serialization/enqueue host target is under 1 ms per
record; asynchronous filesystem stalls remain outside that target. The authored
controls still require their Mac/Linux gates; no lamp result is claimed here.

### Room attribution preserves the current enrollment proof (24 October 2026)

Reuse `speaking_faces::saw_line_scoped` for the background room-speaker
publish, as the forget path already does. The original path replaced the
same request's line with `enrollment: None`; a deterministic regression
reproduced loss of an existing microphone-bound proof. Carry only that
proof and its lease, retaining cancellation under the existing cache lock.
No new I/O, inference, polling, topic, switch or protocol field is added.
The known-file read remains background work; the immediate publish target
is under 1 ms without added I/O, with no device latency claim. The combined
switches still need the physical two-person/cancellation enrollment trial.


## Requested music confirmation before handoff (#2793, 24 October)

V1 `skills/music/SKILL.md` asks for a play marker followed by one short
confirmation and distinguishes playback errors. Its “Playing” example does
not establish that sound started. V2 retains the short confirmation and
its existing Voice-owned retirement/handoff; it must not copy that example
as a success receipt for an offer. Reuse the shared checked tool-result
boundary to tell both voice routes that the recording is only offered,
not playing or ready, and may be refused. No new round trip, wait, timer
or playback authority is added. Target: no additional I/O or scheduling
on the confirmation path. Model wording and acoustic latency remain lamp
acceptance, not established by prompt-context assertions.

## A known reminder interruption returns the same occurrence (26 October 2026)

V1 `system/schedule/runner.go` postpones busy delivery and retries a failed
occurrence on a later tick; its successful handoff is not proof of hearing.
Keep the useful retry behavior while retaining V2's stronger completion rule:
only the whole provider/speaker retirement join can acknowledge an occurrence.
Reuse the existing `OwnerFenced` retirement and durable consumer retry path.
A new admitted user turn may retire a partly spoken reminder only after old
output is cancelled and cleared and replacement authority publishes successfully.
Do not generalize ordinary receipt Drop, provider failure, privacy cancellation,
clock reset or crash uncertainty into permission to retry in the same process.

This adds no worker, protocol field, dependency, I/O or waiting to input
admission. The receipt operation only queues the existing consumer command.
The latency target is no additional capture opportunity on interruption;
retry retains the five-second attempt pace and all current silence/readiness
gates. The bounded consumer and durable Schedule occurrence remain the owners
of later delivery. Synthetic coordinator and ledger controls do not establish
acoustic timing, word repetition or physical stop behavior; the lamp replay
must check interruption followed by conversation and later reminder delivery.

The reminder interruption review keeps another conservative boundary: any
qualified own generation retirement prevents automatic retry. The last words
may already have played even without a provider terminal join. This also holds
multi-generation output interrupted after an earlier generation retired;
no whole-turn completion or Ack is inferred. The live same-pump final-retirement
control stays held, while accepted/unretired audio still retries after fencing.

## Darwin unnamed peers and bounded ancillary records (2026-10-27)

The first real Mac IPC gate failed: valid unnamed socketpair tickets were
refused, and excess rights reached rustix1.1.5's truncated-cmsg drain/drop panic.
Those failed receipts remain failed; later qualification is scoped separately.

Reuse successful native getpeername as connection evidence: cached rustix maps
Darwin's connected AF_UNIX empty path to None. Still require SO_TYPE=STREAM,
local AF_UNIX and a successful syscall; ENOTCONN is an error, not that None.
INET, datagram, unconnected and missing-CLOEXEC observer checks remain. Validate
the control socket's actual family/type too, not merely its Rust wrapper type.

Use one shared IPC CONTROL_FD_CAPACITY constant:512 on Mac,2 elsewhere. Its
cmsg_space covers Darwin's complete native UNIX control record, so every excess
OwnedFd can be drained and dropped before rejection. Native UNIX send control
is capped by MCLBYTES,2048 on supported x86/ARM; internalize requires one exact
SCM_RIGHTS message; receive stops before the next control record. This avoids
the known Darwin truncated-header parser path without unsafe/new dependencies,
a guessed large allocation, ignored truncation or leaked rights. Proof pins:
[sockargs](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_syscalls.c#L3105),
[UNIX validation](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_usrreq.c#L2366),
[receive boundary](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_socket.c#L3328),
[x86 bound](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/i386/param.h#L102),
[ARM bound](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/arm/param.h#L78).

The IPC grant receiver uses that constant. The separately prepared Eyes request
receiver consumes it in its own unqualified integration. The
8-right production rejection/all-peer-EOF control stays; its low-level8/32
repeated-right control checks Mac full receipt without CTRUNC and every copy
closed, versus Linux actual CTRUNC and kernel/OwnedFd cleanup. No hundreds of
FDs or global FD counts. All17 original test identities remain. The source
repair requires current full IPC/native gates; earlier7passes are not replayed
as qualification of this changed production source. Target/latency remains
unmeasured, and Eyes/Voice integration holds remain.

## Dedicated camera control nonblocking mode (2026-10-27)

The corrected unnamed-peer epoch passed 42 IPC cases, but its backpressure case
stalled in the filler send before creating its one-second deadline. The owned
process stack and controlled stop are retained; the epoch remains unqualified.
Pinned [XNU send-space handling](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_socket.c#L1960)
checks the socket's nonblocking state for that wait; the per-call flag alone
only covers an earlier lock wait. Reuse explicit nonblocking setup on each
dedicated control endpoint, after original deadline and socket validation.
Never change shared witness observer flags or use a private kernel flag value.

The existing control test now fills in nonblocking mode, restores blocking mode
before the first production grant poll, and keeps its one-second expiry and
terminal assertions. A fixture-only three-second rescue owns only that control
peer; cancellation and join run on normal exit and unwinding. It exposes an old
blocking sender as a bounded assertion failure instead of a hung suite. This is
not a hard scheduling or production latency guarantee. All 17 test identities
remain. Each source epoch requires its own IPC/native qualification; previous
passes, failures and the owner-stopped case keep their actual epochs.

## Darwin full-control EMSGSIZE (2026-10-27)

The dedicated-nonblocking epoch completed 42 IPC passes and one prompt OS40
EMSGSIZE failure. The original failed driver and 21 actual debug backing files
are retained; this result remains failed. Pinned XNU
[UNIX send accounting](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_usrreq.c#L660)
reduces the current send high-water mark as the peer queue fills.
[sosendcheck](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/uipc_socket.c#L1939)
returns EMSGSIZE when ancillary length exceeds that remaining mark, before its
normal nonblocking branch. SO_SNDBUF reports the configured total after adding
queued UNIX data back, so it cannot preflight the remaining space.

Treat this result as pending only on Darwin for the first validated fixed
48-byte, one-descriptor grant send. It accepts no data or rights, never reports
Ready, and never renews the original deadline. Errors after partial transfer
remain terminal. Preserve all 17 original controls and add one real
drain-and-resume case to verify recovery without duplicate or changed authority.
Fresh source gates remain required; Linux, composed Eyes/Voice and physical
latency acceptance are separate.

## Concurrent observer flag comparison (2026-10-27)

The 90ca and b234 source epochs each pass the full Mac IPC census, 44/0/0,
with relay 1/0 and doctest 1/0 plus workspace formatting, strict all-target
Clippy and build. These observations do not erase the earlier failures.

An independent review reports a Mac-only shared-binary flake in the added
recovery control: concurrent descriptor garbage collection can set FMARK while
rights are closed. Pinned XNU unp_gc clears and sets that internal mark; the
numeric 0x1000 mapping and reproduced failure counts remain attributed to the
reviewer. The new assertion checks only whether NONBLOCK changed, which is the
property under test. It retains every original test, deadline, frame, rights
and recovery assertion. Current correction qualification includes a separate
default-thread camera binary run as well as the full IPC census; results are
recorded on [#3632](https://github.com/autonomous-ai/lampOS/pull/3632).

No production syscall, protocol or caller changes in this correction. The
receiver still relies on caller peer authentication. Eyes/Voice integration,
wrong-peer credential controls, Linux/current-head gates and device latency
remain separate acceptance evidence.

## Updater output reader completion (27 October 2026)

V1 `system/bootstrap/bootstrap.go::detectVersion` and
`system/bootstrap/recovery.go::recoverPendingUpdateAt` give
`system/lib/core/system/system.go::Run` a ten-minute context. That helper
uses `exec.CommandContext` and `CombinedOutput`; it supplies neither a
bounded output buffer nor a whole-process-group pipe-completion guarantee.
Keep V2's existing per-caller query allowance, normally five seconds, without
copying V1's recovery-wide budget into a read-only query.

Reuse the corrected nonblocking reader and unreaped-leader/group ownership
mechanics from the Setup query change (#3631, 220251c70), including the
failure-before-reap repair. Keep a private updater helper: Setup already
depends on Update, so importing Setup would create a cycle, and introducing
a shared public executor API is unnecessary for this fix. Use the already
pinned Rustix process feature explicitly in Update's production dependency.
No new crate or runtime service is introduced.

Update's existing test accepts a 300,000-byte response and remains unchanged.
Its deliberate limit is 1 MiB, distinct from Setup's 64 KiB. Complete valid
responses through that limit stay readable; oversized or invalid UTF-8
responses return none rather than a partial string. The four readers keep
their existing failure policy. `Systemctl::states`, stop/restart allowances,
systemd job cancellation and reset policy are outside this piece. Exclusive
child ownership and normal-success descendant limits remain explicit; host
controls do not establish installed update acceptance or a hard kernel-I/O
deadline.

## Reset query output and time bounds (26 October 2026)

Keep the delivered reset's starter/writer ordering, journal, custom stop and
recovery policy. V1's `system/server/system/factoryreset.go::runFactoryReset`
resets the agent, wipes device state and schedules a reboot; it does not
supply the V2 systemd membership-query barrier. Reuse the owned-process-group,
nonblocking pipe and unreaped-leader mechanics from the preserved Rust reset
runner (258b2a9f10, #3583), without importing its alternate stop policy. Use
the updater's existing five-second setup-unit query allowance and a 64 KiB
stdout cap for Setup's read-only queries. No new dependency or public setting.

The leader is not reaped while a descendant may still hold stdout or an
unsuccessful exit still needs group cleanup. Exit status is observed with
`waitid`/`NOWAIT`, then a failed group is stopped before the leader is reaped,
including descendants that closed output. If the OS reports that another
reaper already consumed the child, the query fails without signalling its
unproven identity. With exclusive child ownership, this retains the identity
through error cleanup. Successful EOF and leader
exit end ownership; detached descendants or descendants closing stdout are
not claimed as cleaned up. Stderr was captured but never exposed; it is now
discarded without a buffer. Stop commands retain their budgets (Wi-Fi has a
two-minute unit allowance); neither a shared 60-second stop limit nor a new
reset-wide deadline is introduced. Query failures keep data and the journal
and take the existing reboot path. Host subprocess controls do not prove
installed reset acceptance or a hard bound on kernel spawn/reap/filesystem I/O.

## Reminder skills offer another chance while work remains (26 October 2026)

V1 `system/schedule/runner.go` revisits due work after a busy tick or failed
handoff. Keep that opportunity without requesting on every positive heartbeat:
the opt-in Skills `changes` trigger compares a bounded scalar tuple. Reminders
selects store identity, pending count, oldest ID and delivery state. First
eligibility, a changed tuple, or restored eligibility rearms one chance;
renewal timestamps, revision and lateness do not. The new mode requires a
scalar trigger comparison and keeps selected paths independent of same-topic
while fields; overlapping configurations are refused at load. Existing `appears` and
`present` definitions retain their behavior. The durable Voice consumer still
owns selection, completion, retry pace and acknowledgements.

Retain resting, ready and enabled guards, one executing run and the Voice-begun
body gate. The target is a chance on the next eligible observation of changed
work or restored eligibility, without sixty requests for sixty unchanged held
heartbeats. No new worker, protocol or model round trip is added. Synthetic
Engine and runner controls distinguish opportunities from delivery and gaze;
physical latency and motion remain unverified.

The old Skills parser rejects the new trigger and omits Reminders if it reloads
a new bundle before its binary restarts; unrelated valid skills remain loaded.
The updater's `skills_still_load` guard refuses this bundle for a bundle-only
update that keeps the old binary. A complete update writes the bundle before
restarting parts, so it still has an old-reader interval. Normal rollback
restores skill files before restarting the old binary; crash recovery requires
Update to run again. Voice's existing ten-second fallback is an eligible-offer
wait, not guaranteed end-to-end delivery or proof of mixed-version parity.
Keep these crossings explicit in validation; no atomic-update claim is made.

## Cloud manager query ownership and complete output (2026-10-27)

Cloud's report reader joined a stdout thread after reaping a successful
leader, so a descendant retaining the pipe could bypass the two-second query
budget. Its 256 KiB `take` also accepted a truncated successful prefix and
ignored read/UTF-8 errors. Source review found three callers: exit-note
metadata, cached unit health, and the independent recovery timer report.

V1's nearest health query, `readHALUptime` in
`system/server/health/delivery/http/handler.go`, uses an unbounded
`exec.Command(...).Output()` and returns zero on failure. It supplies no
bounded group or complete-output safeguard to port. Keep Cloud's existing
two seconds, 256 KiB, null stdin/stderr, `Option<String>` API and each caller's
failure policy; reject incomplete, oversized or invalid UTF-8 responses.

Reuse the corrected private Setup/Update query ownership mechanics: Rustix's
pinned nonblocking descriptor and process APIs, `waitid` with `NOWAIT`, a
retained leader through validation, and cleanup before reaping on failure.
Cloud declares its own Rustix features explicitly. No service, shared public
executor, new dependency version, systemctl state parser or stop policy is
introduced. Direct controls exercise Cloud's public wrapper and its own
256 KiB boundary. Mac, original ARM64 and installed-lamp acceptance remain
separate evidence; successful detached-descendant cleanup and hard kernel
spawn/reap bounds are outside this contract.

## Preserve head-pair policy behind a read boundary (#3689)

Keep the existing literal unit/pins projection, conservative override refusal,
regular symlink/null-mask handling and conditional Body calibration. Introduce
only text and lazy-directory operations so a future guard can freeze the same
policy inputs without copying its rules. Reuse the ordinary reader unchanged;
a strict regular-file observer cannot silently replace compatible read-only
links or masks. Test replay is not a collector, filesystem transaction,
authentication or restoration. See [the boundary](head-touch-inputs.md).

## Capture the same head-pair inputs through bounded descriptors (#3689)

Reuse that `HeadInputs` seam and its literal policy, with the pinned Rustix
`openat`/`statat`/bounded `readlinkat_raw`/directory APIs already maintained in
Records, Setup and Update. Reuse the strict observer's same-descriptor identity
and cap-plus-one read pattern, but do not substitute its links/null/hardlink
refusal for compatible head reads. A separate collector resolves bounded links,
captures typed absence/null/regular facts and all directory names, and compares
them again before returning closed replay. A future caller can explicitly
revalidate that view at its write boundary. No dependency, ordinary-reader
default, filesystem authority or shared privileged engine is added. The
256-key and 16,384-work bounds are conservative collector-only limits, not
measured deployment coverage. See [capture and its limits](head-touch-capture.md).

## Raw setting ownership capture (#3689)

Keep Update's existing three-party ownership decision, including local
deletions, omitted no-ops, writing records and legacy tmpfiles/image
ownership. Extract its pure table without changing the ordinary planner's
filesystem behavior; the future guard's read-only adapter uses hashes of
bounded descriptor observations rather than rereading paths. Reuse the
existing `writer_files` boundary and borrow only selected input records.
Do not clone/default whole state, treat a raw Write as authority, or claim
head-touch projection, authenticated execution or restoration from this
intermediate capture. See [the contract](writer-settings-capture.md).

Recheck retained observations through the same bounded descriptor observer,
including negative entries and ancestor identities. Cap each reread at its
original byte count and compare complete evidence; do not recalculate an
ownership plan from changed files. This remains a read-only prerequisite:
exclusion through the mutation boundary, projection dependencies and durable
independent recovery are separate requirements.
