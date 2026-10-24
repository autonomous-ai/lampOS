# Requested local audio (#1685)

Requested audio preserves admitted Memory scope through the same spoken
owner. The current integration uses the second voice route from #1712. The [host verification record](https://github.com/autonomous-ai/lampOS/blob/lamp-second/lampOS/docs/work-log.md)
keeps implementation revisions, local gates, native CI and previous failures
separate. Software qualification does not establish audible behavior on a lamp.

A person speaking to the lamp can ask a skill to play one fixed local
recording. Voice says a short confirmation, waits for its confirmed speaker
retirement, then plays the recording under the same live spoken owner.
`LAMP_REQUESTED_AUDIO=on` is required in **Voice, Skills and Actions**.
Only the exact value `on` enables it; it is off by default and is not enabled
in the repository's product or lamp units. Installing a skill does not
enable playback.

Voice also requires an explicit absolute `LAMP_SKILLS_DIR` for the existing
Skills socket. Missing or invalid optional playback configuration disables
requested audio while ordinary Voice continues.

This is an extension of the existing Skills tool route and Voice speaker
worker. Skills authorizes an asset; it does not play sound. Actions remains
the sole mixer owner. No player process, playback service, network fetch,
runtime synthesis, playlist or repeat queue is introduced. The existing
autonomous `do.sound`/Say cue limit remains three seconds.

## Skill format and asset checks

The loader accepts exactly two steps for a requested-audio skill:

```toml
kind = "installable"
version = "1.0.0"
tool = "play_local_recording"

[permissions]
speak = true

[[do]]
say = "Give one short confirmation that you will play the requested recording."

[[do]]
after = "say"
requested_audio = "sounds/recording.wav"
```

The first step is one nonempty plain `say` note of at most 240 UTF-8 bytes,
without control characters. The second is one `requested_audio` path with
`after = "say"`. It must have a tool and no automatic trigger, Policy ask,
`while` condition, choice/flag argument or `if_choice` selector, body, light,
light show, look, other sound, routine cue/rest, automatic `[limits]` or
additional step or add-on proposal. It also refuses `[history]`, `[search]`,
`[watch]`, `[stop]`, `[unplug]`, `most_a_day`, `[active]`, `[budget]`, `[rest]`
and `[quiet]`. Those features keep
their ordinary skill behavior; they cannot be mixed into a recording offer.
This includes a daily clock trigger even when its inputs are fully granted.
An add-on remains proposal-only and cannot carry a recording or advertise a
recording tool.
Both output steps require `permissions.speak = true`. Grants do not relax this
shape: a requested recording cannot install automatic output restrictions
before it is requested.
The ordinary `[again]` repeat guard and `still` note remain available; neither
provides spoken authority or extends an offer's time bounds.
Unknown keys and unknown enum values refuse the definition. This format
does not establish parity with autonomous quiet-hour or room-quiet rules;
the request must instead belong to an admitted spoken turn.

The path is relative to the skill, ends in `.wav`, and has at most four
plain components, each at most 100 bytes. Absolute paths, dot-prefixed
components, traversal, links and special files are refused. Initial loading
uses the existing bounded package reader and Voice's shared sound decoder.
`SKILL.md` and `skill.toml` are each limited to 64 KiB for this format, and
the exact definition bytes checked by the asset reader must match those
parsed by the loader.

| Asset property | Bound |
|---|---|
| Encoding | WAV, signed 16-bit integer PCM, mono, 24,000 Hz |
| Duration | 1 to 1,440,000 frames, at most 60 seconds |
| File bytes | At most 2,884,140, including bounded header/chunk overhead |
| Amplitude | Existing cue limits: no sample above -12 dBFS; no checked 10 ms window above -30 dBFS RMS |
| Identity | SHA-256 of the exact immutable WAV bytes, plus decoded frame count |

Voice reads the file on its preparation worker, checks these same bounds,
and compares its digest and frame count with the offer. The samples it
dispatches come from that immutable read, so a later path edit cannot change
the recording already in memory. There is no volume increase or normalization
at playback.

Voice keeps one monitor and one preparation/renewal worker for the life of
the process, including conversation restarts. Capacity is one active item,
without a future queue. Regular-file latency has no hard operating-system
bound: a stalled read exposes no samples, and its grant expires while it
waits. Cancellation and teardown do not wait for or join that read, and a
new conversation cannot accumulate additional preparation workers.

The loader, packer and installer can validate this format with the switch
off. Skills omits its tool from advertisement and refuses execution then.
An `enabled = false` package remains validated inventory with its tool name
reserved, but it is neither advertised nor installed into audio authority.
For primary declared catalog tools, the optional top-level `body` and `core`
metadata follow the same enabled and media-off filters and first emitted
definition as `tools`. An omitted or shadowed definition cannot contribute
metadata for that primary tool. The existing auxiliary unplug-cancellation
tool is separate: a supplied inventory entry can contribute it even when its
primary tool is omitted. Each metadata key is omitted when its list is empty.
The ordinary tool wire continues to work for other skills. The recording is
fixed by the loaded definition; a model argument cannot choose a path or
provide authority.

## Private context and authorization

Voice adds `requested_audio_context` beside `op`, `name` and `args` on an
existing Skills `call.sock` request. It is never a model argument. The
context binds these exact fields:

| Type | Fields |
|---|---|
| `RequestedAudioContext` | `worker_boot`, `provider_session`, `request`, `privacy_generation`, `call_id`, original `issued_us` |
| `RequestedAudioOffer` | `skills_boot`, `definition_generation`, `offer_id`, complete `context`, `skill`, checked absolute `asset`, `sha256`, `frames`, `issued_us`, `start_before_us`, `finish_before_us` |
| `RequestedAudioLease` | `skills_boot`, `definition_generation`, `offer_id`, `until_us` |

Skills returns the offer in `result.requested_audio` beside the ordinary
`next` confirmation note. Voice removes that private object before sending
the result to the model. Both direct skill calls and `use_skill` preserve
the original request/call identity. Missing or invalid spoken context
refuses media. Written requests, Messages requests and Messages
`speak=true` never create this context or grant playback.

A typed direct skill call or translated `use_skill` call whose tool is named
in catalog `body` metadata is refused before calling the Skills socket,
whether requested audio is off or on. Ordinary typed non-body calls and
spoken body calls retain their routes. Typed media calls through either
direct or wrapped routing send no private context; any unexpected
`requested_audio` offer in the reply is removed and discarded before the
model receives the ordinary result. Only the verified spoken Skills route
can retain the private typed offer with its original binding.

Scoped dispatch carries an internal `ScopeLease` alongside
`RequestedAudioContext`. The lease retains the Memory identity captured when
the request was admitted: a later face change cannot redirect that request's
Memory calls to a different person. Room-scoped requests carry a valid room
lease. On a route configured for scoped dispatch, a missing or revoked lease
refuses the call instead of falling back to unscoped routing.

The second route retains its current context construction and captured tool
scope. Music does not import a separate scoped-system-context feature.
Written requests cannot acquire spoken audio authority. Transcript persistence
happens once when an event leaves the provider queue; retrying IPC delivery
does not repeat that write. The audio runner reuses the existing tool/search
dispatcher. A task answer retains its private `news_key` only until Engine
extracts `NewsAnswered`; the model receives neither that key nor the
recording offer. If the same turn tells task news and plays a recording, the
news is acknowledged once only after both the confirmation and recording
retire successfully and the playback ticket wins its completion transition.
A cancellation that wins that transition cannot be reported as completed.
Cancellation, playback gaps, expiry, a changed owner or microphone privacy
leave that news unacknowledged. Reminder and Say keep their existing
acknowledgement rules.

Due reminders do not interrupt requested Music: they wait until its reply
completes or is cancelled, then pass the existing quiet and readiness gates.
The recording is at most 60 seconds; that is not a deadline for reminder
delivery.

Scope checks run before dispatch and after the RPC returns for direct Skills
calls and translated `use_skill` calls, including requested audio. Ordinary
Memory calls keep the same captured scope when audio is enabled and use the
scoped Memory path rather than reading the current face again. A late reply
that fails the scope check loses both its model-visible result and any private
audio offer. These checks suppress stale completions; they cannot undo RPC
side effects that already completed. The provider repeats the scope check for
queued requested-audio offers at every provider-to-coordinator IPC send attempt,
including retries after output credit or socket capacity returns.
The second worker also rechecks offer structure, original request/session and
`issued_us <= now < start_before_us` on each retry before spending output
credit. Waiting for credit never extends the original deadline.
Revocation while an offer waits in that queue discards it;
ordinary generation completion alone does not revoke an otherwise current
request. The scope lease stays internal to the provider, outside the wire
offer and playback ticket. After handoff, the separate playback guards remain
authoritative; camera-only scope revocation is not a playback cancellation
signal. Playback's privacy checks use microphone permission and generation.

The audio context's worker, provider session and request must match the
scope lease. Scope authority does not replace the original call identity,
privacy generation, spoken-owner checks or audio start/finish deadlines.
Those checks remain separate and required. Translating `use_skill` retains
the original call ID and context; it does not issue a new deadline. A current
Memory lease alone cannot authorize a recording, and an audio context cannot
restore a revoked scope.

The coordinator records each offer the second worker hands it as
`requested_audio_offered`, with `accepted`. A refused one also has `refused`,
the first condition that refused it, and the journal says
`request N: requested audio refused: <why>` once. The conditions, in order:
requested audio off or unavailable; not the turn in hand; not a spoken turn;
the turn's one offer already made; the turn's provider speech not started;
the provider idle; the turn's speech not the provider's; a local-only,
reminder or Say turn; the speaker belonging to another owner; the microphone
not allowed; an offer that does not check; another worker boot, request,
provider session or privacy generation; and last, a recording that could not
be prepared. On lamp-4ace on 24 Oct a first "play me something to relax" was
refused with nothing to say which (#2793). The reason it then gave was "the
offer is from another worker boot": the second worker stamped its offers
with the coordinator's boot, not its own, so every offer on the second route
was refused. It now stamps its own, while its gate still takes the
coordinator's authority by the coordinator's boot (`WorkerBoots`, #3319).
The Live worker stamps the same way, and
is left for its own change, since the product's voice is the second route.

A recording accepted and then ended early says why, once, in the journal.
`request N: requested audio cancelled: <why>` comes from the recording's
own checks: the service stopped, it wasn't ready, the speaker's state was
being read past the clash spin, the speaker was muted, silent or its state
unknown, its state changed, was stale, or was read after the check's own
time, its Skills lease lapsed, it was past its time, Skills refused its start or a renewal, or its recording
couldn't be read or wasn't the one offered. `request N: requested audio
revoked: <why>` comes from the coordinator, when the turn is revoked.

The same socket accepts two private operations:

- `audio_authorize`, with the complete `offer` and `phase` of `start` or
  `renew`, returns a lease under the usual `ok`/`result` envelope.
- `audio_finish`, with the complete `offer` and `outcome` of `completed` or
  `cancelled`, consumes it and returns `{"finished":true}`.

These socket structures reject unknown fields, operations, phases and
outcomes. They are strict private receipts, distinct from the bus's additive
message compatibility rule. Exact identities are checked; neither a path
nor a caller-supplied label authenticates a new owner. The existing local
socket and process ownership boundary remains the trust boundary.

All deadlines use the same host monotonic clock:

| Boundary | Limit |
|---|---|
| Original issue | Recorded once by Voice before local dispatch; the second route uses the original tool-call event time. Future or already-too-old contexts are refused |
| First media dispatch | Strictly before `issued_us + 10,000,000` |
| Absolute finish | No later than `start_before_us + 60,000,000`, at most 70 seconds after original issue |
| Skills lease | At most 500 ms and no later than the original finish boundary; Voice requests renewal every 200 ms |

A `start` authorization may prepare the recording before the confirmation
retires. It does not authorize early output: Voice still waits for retirement
and checks the original start boundary at the first media dispatch. Renewal
never moves either original deadline. A second `start`, an expired renewal,
a spent/cancelled offer or a mismatched identity cannot revive playback.

Skills remembers at most 16 offers through their original finish boundaries,
including terminal offers. A full table refuses new work instead of evicting
replay protection. The stable worker/session/request/call identity remains
consumed even if a duplicate changes its timestamp or privacy generation.
Voice also accepts at most one media offer for a live owner. There is no
later retry or queued restart.

An accepted recording remains bound to the original provider worker and
session after intake. It prevents fallback replay or rebinding of that turn;
another provider's confirmation cannot authorize its playback. The worker,
session, owner and privacy generation are checked again on the coordination
loop before media dispatch. Offers originating on either spoken route use
that route's own identity. A Ready renewal can update connection readiness
without rebinding an accepted offer: the reply and every Skills authorization
retain the original worker/session/request/call tuple. The authored-link host
test covers that coordinator boundary, not a live provider reconnect.

## Definition revocation and speaker mute

With `LAMP_OUTPUT_CAPS=on`, a rejected volume argument that never touches the
mixer leaves an already-confirmed restriction confirmed. Actual mixer reads
and writes still update the cap observation. This behavior also applies when
requested audio is off; it is governed by the caps switch.

Skills accepts at most 16 loaded requested-audio definitions. Every two-second
scan opens `SKILL.md`, `skill.toml` and the fixed asset for each, without
following links or blocking on special files. It hashes all current bytes:
two definition files of at most 64 KiB each and one WAV of at most 2,884,140
bytes. The existing `ring` SHA-256 implementation keeps the same lowercase
digests and length-prefixed definition encoding. Scans retain no WAV payload
cache and do not trust matching metadata to skip reads.

These scans run synchronously on the Skills main loop and can delay ordinary
tool calls, timer processing and reloads. The roughly 27 ms result below
measures recently used files on a Mac; scan CPU/I/O cost and Skills IPC latency
on the lamp remain unmeasured.

File identity, size, permissions, owner, modification time and filesystem
change time are checked before and after reading. The root, intermediate
folders and opened path must still identify the same plain files. These
checks detect replacement and observed concurrent changes; the content hashes
also detect edits whose timestamps are unchanged. Linux documents
[same-tick inode timestamps](https://cdn.kernel.org/doc/html/latest/filesystems/multigrain-ts.html).
A writable shared mapping can also receive later stores without a new page
fault: Linux's [filemap write-fault handler updates timestamps](https://github.com/torvalds/linux/blob/v6.8/mm/filemap.c#L3370-L3397)
before making the page writable. Waiting for a timestamp to settle therefore
does not establish an immutable file image. The scan uses actual content
verification on every supported host and filesystem.

When the file-stability check refuses a file, its existing error prefix is
followed by fixed labels for the file's role (description, manifest or
recording), the check phase (after-read or final), the opened/listed stamp
fields that differ and the original/opened/listed numeric stamps. The message
contains no paths, filenames, hashes or contents and fits within 1,536 bytes.
It is constructed only on failure from metadata already read. Load and pack
return it through their existing error path; boolean scans still return false
without adding a journal line. Folder, initial-open and content-hash refusals
retain their separate errors. These details help identify a future refusal;
they do not establish the cause of the retained packaging failure below.

All file I/O and hashing run outside the authorization mutex. Only a successful
scan of the same accepted generation and key set can refresh freshness. A
stale successful result cannot restore inventory already revoked by another
scan. Byte-identical edits remain accepted after full verification. Replacement
of the loaded set conservatively revokes all prior media generations; removal,
permission edits, replaced files, unreadable assets and changed digests
invalidate their authority. A new Skills process has a new boot identity.
Voice independently compares the digest of the exact immutable playback bytes.

Earlier zero-payload host scan measurements describe the superseded metadata
cache, not this implementation. The native same-size-edit regression failure
is retained as evidence; that run did not record its actual timestamp values.
Current scan measurements count every payload byte and time the complete
`Scan::unchanged` call, excluding fixture creation and loading. They cover the
first and repeated scans of 16 maximum-duration recordings. A digest-only
benchmark measures CPU cost and is not a disk-scan or device-latency result.

When routine limits are also enabled, a reload must first accept the complete
cap-definition batch. Audio authority, routine generation, permissions and
the Engine then use that same accepted list. A refused cap batch cannot
install new audio definitions. Asset invalidation runs before that decision,
so refusing a reload cannot preserve authority over a changed recording.

Authorization fails when the last successful scan is three seconds old,
including if the main loop or a scan stalls. Revocation is therefore bounded
by detection/freshness, not instantaneous at an arbitrary filesystem write:
an existing lease can last up to another 500 ms. The fixed read budgets are
resource bounds, not measured disk or scheduling latency.

With the switch on, Actions publishes optional `speaker/state` as
`lamp_msgs/SpeakerState`, `Latest`, every second with a three-second lifespan.
It carries a host `ClockStamp`, publisher `session_id`, `mute_generation`,
optional `volume_raw` from 0 to 100, and `muted`. Unknown volume never grants
sound. A valid explicit mute intent advances the generation and attempts to publish
the logical mute **before its mixer I/O**. An unmute cannot restore an old
recording's generation; a mixer failure leaves that mute intent protective.
Loss of a previously known mixer observation also advances the counter.
Volume retains zero/loss evidence inside a command, so a read that sees zero
followed by successful restoration in that same call cannot hide the
revocation. The final Latest value may be audible, but its generation has
changed. Explicit mute attempts publication before I/O; publication of an
independently observed zero is attempted when its command completes.

With output caps enabled, clamp, release and retry operations use the same
tracked mixer reads and writes. Their zero/loss evidence also retires an old
recording; a later audible observation cannot erase it. Speaker state grants
no audio until the applicable volume restriction has confirmed compliance.
Durable acceptance of a cap is not confirmation that the mixer applied it.
After durable acceptance, a new or tighter volume restriction attempts to
withdraw the old speaker permission before mixer I/O; an unconfirmed retry
does the same.
An unchanged light-only update or a relaxation of a confirmed ceiling does
not withdraw compliant sound. Actual readback within the active ceiling
restores evidence without a second mixer read. These observations do not
clear an explicit logical mute.

An optional speaker-state publication failure at startup, during a heartbeat
or call, or at shutdown latches this publisher unavailable until Actions
restarts. Actions logs the failure once and makes one best-effort unknown
snapshot; after that it stops optional mixer polling and publication, even
if the bus recovers. Ordinary light, volume and physical-mute calls continue.
Retained mixer observations still reach caps, whose confirmation and readiness
rules are unchanged. Health reports `speaker_state_available=false` with a
warning. Cap/storage failures retain their existing fatal behavior. Invalid
configuration or failure to construct the bus/publisher still rejects startup
before this optional service exists.

If every revocation write fails, the last readable positive snapshot can
remain usable until its original three-second wire deadline. Repeated reads,
later commands and renewed Skills leases cannot extend that deadline. This
does not establish instantaneous remote revocation when publication is broken.
Once the ticket expires or is cancelled, later audible state cannot revive it.

This boundary is when Actions handles the intent. Its existing requests
are serial; each `amixer` subprocess has an 800 ms timeout, and one volume
command can perform multiple reads and a write. A waiting command can
therefore wait longer than 800 ms. This does not promise immediate mute
publication from socket arrival or physical silence within a fixed time.
Actions restart, stale/unknown state and a lost observation also prevent old
audio from continuing once observed or the retained evidence expires.

Voice monitors speaker state off its coordination loop on a 20 ms cadence.
The final output check requires a cache observation no more than 100 ms old,
matching host/publisher/mute generation, current owner/privacy/capture
authority and a still-live Skills lease. Speaker permits are bounded by the
existing 100 ms output permit, the currently validated speaker snapshot's own
three-second wire deadline, the 500 ms Skills ceiling and the **actual
remaining** lease and original finish deadline. Healthy newer snapshots can
advance speaker freshness; rereading one snapshot cannot extend its deadline.
No file or socket work is added to that final check.

Admission reserves the one active slot before reading the speaker cache, so
a second offer refused for capacity cannot contend with the current item's
permission check. A failed cache lookup returns that reservation immediately.
The permission check remains nonblocking: a clash for the cache is tried
again, spinning and never sleeping, for about 200 µs, and a cache still held
after that, stale data, or an observation newer than the caller's sampled
time cancels the item. Whether the item ended or the service stopped is
looked at again once the cache is read, since a cancellation can land
during the spin. A later fresh observation cannot revive it or extend
its original deadlines. The spin is there because the monitor writes the
cache every 20 ms and the worker reads it every 20 ms, so the two meet
often: on lamp-4ace on 24 Oct (#2793) a recording accepted at 13:18:46.68
was cancelled 0.66 s later by one such clash, which a single `try_lock`
treated as a loss of permission.

Stop, interruption/new owner, microphone privacy closure, capture gap, speaker mute,
invalid speaker state, definition revocation, daemon loss and deadline expiry
are terminal for the item. Reopening microphone privacy, unmuting or restarting a daemon
does not resume it. A completed model generation or accepted final chunk is
not confirmation retirement: Voice requires the matching speaker completion,
with no gap, before selecting media under the same spoken owner.

## Source example and validation boundary

The [planned lamp-4ace run](requested-audio-lamp-run.md) covers first and
repeated requests, audible onset, Stop/privacy/mute and the disabled paths.
First owns that device run; this document records no lamp qualification.

[`examples/relaxing-music`](../examples/relaxing-music/README.md) is an original
48-second instrumental source example. Its score and integer Rust renderer
generate the recording at build time, never while a lamp plays it. The
source, renderer and generated recording are Apache-2.0; the example carries
its [provenance](../examples/relaxing-music/PROVENANCE.md) and license.
Generated WAVs, staged packages and disposable installs live under ignored
`artifacts/skills/relaxing-music/`; no WAV is committed and the source template
is not placed in live `skills/`.

From the `lampOS` root, the example README gives rendering, packaging and
disposable host installation commands. They reuse `lamp-skills pack/install`
and the existing size/SHA-256 package format. They do not deploy to a device
or enable the feature.

The host handoff target is **at most 250 ms from confirmation retirement,
with the asset prepared, to first accepted media PCM**. It excludes cold file
preparation and the spoken confirmation itself; authored speaker IPC receipts
do not measure acoustic onset. The full-content scan has a separate **250 ms
host target for sixteen maximum-duration recordings**, timing the complete
`Scan::unchanged` call after loading. The scan always reads every payload byte.
Neither host boundary establishes physical speaker latency.

The current candidate is integrated on
`f349d63c7935dd16decc0badd4dd4e18c10094eb`, preserving core-tool priority,
memory-event observation, sign-in news ordering and continued-line tone
handling. Its worker success test
uses the actual worker loop, WAV reads and Unix exchanges with acknowledged
20 ms fixture steps through the original 200 ms renewal boundary. It keeps
the 100 ms cache limit and original three-second wire deadline. This checks
protocol and retirement deterministically, not independent monitor scheduling
or real elapsed renewal latency. Separate tests retain terminal cancellation
on contention before and after playback starts. Production uses the original
monotonic clock and parking; permission guards and real IPC timeouts are
unchanged. The digest-mismatch and revoked-renewal fixture uses the same
acknowledged worker steps. A bad digest must finish cancelled after Start,
without prepared samples. With a valid recording, no renewal is allowed
before 200 ms; at that boundary the fixture waits for the exact Renew and
Cancelled finish, then worker completion. It leaves prepared samples
unconsumed and the original grant unexpired, proving that revocation still
prevents playback and sample access. A separately scheduled monitor can
legitimately cancel on cache contention or missed freshness before Renew;
this fixture isolates the denial path without retrying that cancellation.

The Live-worker handoff fixture takes the shared worker-state guard before
starting its fake Skills peer. Its three-second accept budget therefore does
not run while another parallel fixture owns that state. It retains the real
socket exchange, the original tool reply before the private offer, both direct
and wrapped calls, and confirmation audio for the same request. Production
deadlines and the fake peer's timeouts are unchanged.

Qualification follows the current plan: Mac formatting, Clippy
and changed-crate tests, then new-head PR ARM64 CI. The historical records
below qualify their own source maps only. The [retained five-crate Mac gate](verification/requested-audio-mac-20261021-3/README.md) on base `4d932488e`
passed **4,665 tests, zero failed and 14 ignored**, plus formatting and strict
Clippy. After a clean rebase to `f349d63c7`, the history assertion follows the
new `Skill.history.within_us` field; formatting, strict Skills Clippy and
**421 Skills tests, zero failed or ignored**, pass. No authored runtime code
changed in this rebase. The baseline comparison records all 86 earlier Linux
failures by name on unmodified `4d932488e`; current PR ARM64 CI remains separate.

The earlier integration on `1c6b0152a2bf8d936a9ca729a49426f6fd96fa78` preserves
the reviewed Actions correction alongside the current Voice and Skills APIs.
It keeps sign-in sanitization before the final output check, current
held-forget and query-aware search behavior, and separate add-on and
recording formats. Its [complete Mac gate](https://github.com/autonomous-ai/lampOS/tree/c00616bb10715a84496fc4903e59bff59e6e1e04/lampOS/docs/verification/second-music-integration-mac-20261021)
finished with **9,268 passed, two failed, 49 ignored and none filtered**
across 37 consumers, actual process exit **1**, and all 17 stages terminal.
The inherited Eyes `/dev/full` fixture and Add-ons' missing idle ring request
both failed. Workspace formatting, strict all-target Clippy, workspace build,
renderer, packaging and disposable installation passed. Neither failed test
was waived or rerun for a passing result. The proof binds the working source's
**2,291 non-document inputs plus 30 parent fixtures**; the base commit alone
does not identify the tested bytes. The [complete local Linux ARM64 gate](https://github.com/autonomous-ai/lampOS/tree/c1f31fc1afa48d5b8a4e35c16daa6694737c739c/lampOS/docs/verification/second-music-integration-linux-20261021)
on those identical frozen inputs also exited **1**: **9,280 passed,
86 failed and 52 ignored**, plus two doctest targets that failed before
summaries. Formatting, strict workspace Clippy and workspace build passed.
The full-test failures comprise 69 explicit EACCES errors and 17 related
indirect fixture failures. A separate controlled probe confirmed that the
original temporary-filesystem options prohibit execution; its corrected
mount probe does not qualify the project tests. The focused worker
cancellation and packaging's metadata-stability refusal remain separate.
A bounded diagnostic reproduced a live worker's terminal cancellation on
cache contention; that instrumented result does not establish the original
failure's cause or replace the failed gate. Current qualification follows
the Mac checks and new-head PR ARM64 CI plan above. Authorized lamp checks
remain separate and pending. The current runtime head and its CI results
are recorded on [PR #1757](https://github.com/autonomous-ai/lampOS/pull/1757).

On this integrated Mac snapshot, the [scan check](https://github.com/autonomous-ai/lampOS/blob/c00616bb10715a84496fc4903e59bff59e6e1e04/lampOS/docs/verification/second-music-integration-mac-20261021/scan-timing.json)
read **46,085,516 bytes on each pass**, taking **24.126208 ms first,
23.963417 and 23.460833 ms repeated**. The [prepared-handoff check](https://github.com/autonomous-ai/lampOS/blob/c00616bb10715a84496fc4903e59bff59e6e1e04/lampOS/docs/verification/second-music-integration-mac-20261021/handoff-timing.json)
measured **194 us first and 194 us repeated**. The stated host boundaries and
limitations apply; these are not cold-disk or acoustic measurements.
The records below qualify their own earlier source snapshots only.

The earlier [complete Actions-correction host record](verification/requested-audio-actions-20261021/complete/README.md)
records **8,773 passed, one failed, 48 ignored and none filtered** across
36 consumer crates. All 17 stages completed; the gate exited with failure.
The unchanged Eyes test `seems_log::tests::a_failed_write_is_said_once` opens
`/dev/full`, absent on the macOS host, and failed before its assertions.
Workspace formatting, strict all-target Clippy, workspace build, renderer
tests and render/package/install passed. The 23 Actions speaker integration
tests and actual Voice Ticket expiry/no-revival regression also passed.
All 2,204 frozen source inputs and five checkout-only parent fixtures stayed
unchanged. This record identifies the uncommitted correction by source hashes;
the starting checkout commit alone does not qualify it. Native qualification
of these corrected bytes remains pending.

The [focused Actions record](verification/requested-audio-actions-20261021/focused/README.md)
retains the old-runtime regression failure and intermediate cap-fixture
timestamp failure, followed by the corrected 120-test Actions pass. Focused
repeats and four renderer tests are not added to the complete-suite total.
The reviewer's separate [fresh-source correction gate](https://github.com/autonomous-ai/lampOS/pull/1757#issuecomment-6318619095)
passed **120 Actions tests and 17 Voice lifetime tests**, with the old runtime
failing the deterministic heartbeat control and the restored correction
passing. That result qualifies the correction checkpoint on `c74b0f710`, not
the newer integration above.
That historical [scan check](verification/requested-audio-actions-20261021/complete/scan-timing.json)
read **46,085,516 bytes on each pass**, taking **26.336500 ms first,
26.798833 and 26.505500 ms repeated**. Its [prepared-handoff check](verification/requested-audio-actions-20261021/complete/handoff-timing.json)
measured **208 us first and 198 us repeated**. The same host boundaries and
limitations above apply; these are not cold-disk or acoustic measurements.

The earlier [full-content verification record](verification/requested-audio-content-20261021/README.md)
binds the correction to 2,180 frozen non-document inputs on integration base
`99f2ecfcd4ebb3b9279750bee42f219c0c4895ed`. Deterministic tests force equal
observed timestamps for changed permission and audio bytes. Five independently
weakened variants demonstrate that the content, listed-path, parent and stale
scan guards fail their intended behavior assertions when removed. Focused
checks passed. The fresh complete 36-consumer gate passed **8,685 tests, with
zero failures, 44 ignored and none filtered**: 8,426 passes in the source-only
copy and all 259 Update tests in the checkout for its parent-image fixtures.
Workspace formatting, strict workspace/all-target Clippy and workspace build
passed; every frozen source hash still matched after completion. Focused
repeats and the four renderer tests are recorded separately. These host
results bind the pre-rebase feature head
`ae3384f301c364938eb02afb23e68096b2d0825e`.
[ARM64 run 39550250450](https://github.com/autonomous-ai/lampOS/actions/runs/39550250450)
passed 9,271 tests (one leaky), with 62 skipped, four doctests and 12 own-harness
tests, on merge `0629e393242da32565286b3ad90c81bb30315d67` against
`796d53b2490f8622a30f7746657642a954c1dd78`; x86 Clippy also passed.
The [native receipts](https://github.com/autonomous-ai/lampOS/tree/dbc1cc5f05684ff667c78f58e89bb3736143be52/lampOS/docs/verification/second-current-checkpoint-20261021)
retain the known Voice journal-test leak, four shared-example filename
warnings and verified artifact inventory. The 21 October rebase snapshot
`c74b0f710ecac48f8668c6ab8258777788068c0a`, on
`bd43346de29bba069f741baa3eb32d03c6d5794f`, preserved the implementation patch
and added documentation clarifications. Its green CI predates the later
Actions publication-failure correction and does not qualify that correction.
Current qualification is recorded on
[#1757](https://github.com/autonomous-ai/lampOS/pull/1757).

That earlier gate's completed [scan check](verification/requested-audio-content-20261021/scan-timing.json)
read **46,085,516 bytes on each pass** across sixteen 60-second recordings:
**23.550625 ms first, 22.874458 and 23.183833 ms repeated**. Fixture creation,
loading, snapshot cloning, authority refresh and logging are excluded. The
files were recently written/read during fixture setup, so this is not a
cold-disk measurement. Its separate [prepared-handoff check](verification/requested-audio-content-20261021/handoff-timing.json)
measured **186 us first use and 165 us repeated**, from confirmation retirement
to authored speaker IPC acceptance. Both focused checks passed; neither
measurement establishes acoustic or physical-lamp latency.

Both gates' render, package and disposable host install passed, and all
seven installed files matched their staged hashes. Its generated WAV SHA-256 is
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`.
These are local artifact checks and do not enable or deploy the skill.

Historical results remain tied to their recorded source revisions. The
[earlier host gate](https://github.com/autonomous-ai/lampOS/blob/d2e171f9ac5af88955e631812fa0038b24fa1af8/lampOS/docs/verification/requested-audio-host-20261021/README.md)
passed 8,620 tests, with zero failures and 36 ignored. The
[subsequent host record](https://github.com/autonomous-ai/lampOS/blob/d2e171f9ac5af88955e631812fa0038b24fa1af8/lampOS/docs/verification/requested-audio-host-20261021-current/README.md)
retains its initial 8,682 passes, one unchanged Setup failure and 44 ignored,
plus the separate unchanged Setup invocation that passed all 133 tests. Its
intermittent connection assertion was not diagnosed as duplicate keyed HTTP
requests. Neither that later pass nor this correction replaces the failed run.

[Native ARM64 run 39542590327](https://github.com/autonomous-ai/lampOS/actions/runs/39542590327)
then failed the immediate same-size WAV rewrite regression: 4,684 passed, one
failed, 59 skipped and 4,525 not run after fail-fast. It did not record actual
stamp values. The [retained failure](verification/requested-audio-content-20261021/earlier-native-failure.json)
qualifies the superseded metadata cache; zero-read timings and cache-specific
mutation receipts also remain historical. The later successful native scan
qualification is tied to the separate head and merge recorded above.

Cancellation tests cover terminal tickets and delayed preparation through the
coordinator's cancellation helper. The separate binding test checks Controller
privacy revocation at dispatch; the cancellation matrix itself does not claim
full main-loop event routing. No device playback, sound-quality judgment,
quiet-hours parity or physical cancellation timing is established by these
host checks.

Qualification must include first and repeated requests, busy/private states,
Stop/interruption/capture loss, mute followed immediately by unmute, stalled
or restarted Actions/Skills, changed/removed assets, expired leases and original
deadlines, face changes after admission, scope revocation during an RPC, and
Written/Messages requests including `speak=true`. Keep the
person's volume unchanged and measure the host boundary separately from
audible start and physical stop on an authorized lamp.

## Declined answers and executed actions

With requested audio enabled, the legacy worker refuses later or parked action
calls after an accepted decline and cannot promote that request's private
Music offer. Its audio-off behavior remains unchanged. The second route
checks a mixed music/decline call list before dispatch in either order. An
accepted recording offer remains acted after a later decline; a decline
does not undo completed work. Ordinary lookups and declaration requests
retain the current room-accounting behavior with requested audio off or on. ReportsStop retains its existing consent
withdrawal behavior; this change does not add a held-line model round.
The guards retain the original request identity and Memory lease.

On the second route, fresh current-request microphone input and nonempty
raw final speech-to-text words are required for Music authority. Reused audio,
carried words from an earlier request and the synthetic bare-name fallback
cannot supply a new Music context. The Called note does not suppress a real
current spoken request. Prepared model responses cannot dispatch tools before
the final words pass admission. They can be reused only while both the words
and their context notes still match; a note arriving after preparation
invalidates that response before its calls can run.
