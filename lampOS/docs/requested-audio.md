# Requested local audio (#1685)

Requested audio preserves admitted Memory scope and `VoiceTurn` through
the same spoken owner. The [host verification record](verification/requested-audio-host-20261019/README.md)
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
additional step. Both output steps
require `permissions.speak = true`. Grants do not relax this shape: a requested
recording cannot install automatic output restrictions before it is requested.
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
The catalog's optional top-level `body` metadata follows the same enabled and
media-off filters and first emitted definition as `tools`. An omitted or
shadowed definition cannot contribute a body grant; with no emitted body tool,
the catalog retains its tools-only shape.
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

Second's `LAMP_SECOND_SCOPED_CONTEXT` switch is independent of requested
audio. When enabled, system context, history and Days use the identity
captured at admission; revocation during context construction prevents the
model call. When disabled, legacy context construction remains in use, while
scoped tool dispatch and audio authorization keep their existing checks.
Written requests use room scope and cannot acquire spoken audio authority in
either mode. Transcript persistence happens once when an event leaves the
provider queue; retrying its IPC delivery does not repeat that write.

Scope checks run before dispatch and after the RPC returns for direct Skills
calls and translated `use_skill` calls, including requested audio. Ordinary
Memory calls keep the same captured scope when audio is enabled and use the
scoped Memory path rather than reading the current face again. A late reply
that fails the scope check loses both its model-visible result and any private
audio offer. These checks suppress stale completions; they cannot undo RPC
side effects that already completed. The provider repeats the scope check for
queued requested-audio offers at every provider-to-coordinator IPC send attempt,
including retries after output credit or socket capacity returns.
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
| Original issue | Recorded once by Voice at the tool dispatch; future or already-too-old contexts are refused |
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
that route's own identity.

## Definition revocation and speaker mute

Skills accepts at most 16 loaded requested-audio definitions. Its two-second
definition scan reads exactly `SKILL.md`, `skill.toml` and the fixed asset for
each: two files of at most 64 KiB and one WAV of at most 2,884,140 bytes.
It checks actual bytes and file/folder identities, not just size and mtime.
Reads and hashing run outside the authorization mutex. Replacement of the
loaded set conservatively revokes all prior media generations; removal,
permission edits, replaced files, unreadable assets and changed digests
invalidate their authority. A new Skills process has a new boot identity.

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
sound. A valid explicit mute intent advances the generation and publishes
the logical mute **before its mixer I/O**. An unmute cannot restore an old
recording's generation; a mixer failure leaves that mute intent protective.
Loss of a previously known mixer observation also advances the counter.
Volume retains zero/loss evidence inside a command, so a read that sees zero
followed by successful restoration in that same call cannot hide the
revocation. The final Latest value may be audible, but its generation has
changed. Explicit mute publishes before I/O; an independently observed zero
is published when its command completes.

With output caps enabled, clamp, release and retry operations use the same
tracked mixer reads and writes. Their zero/loss evidence also retires an old
recording; a later audible observation cannot erase it. Speaker state grants
no audio until the applicable volume restriction has confirmed compliance.
Durable acceptance of a cap is not confirmation that the mixer applied it.
After durable acceptance, a new or tighter volume restriction withdraws the
old speaker permission before mixer I/O; an unconfirmed retry does the same.
An unchanged light-only update or a relaxation of a confirmed ceiling does
not withdraw compliant sound. Actual readback within the active ceiling
restores evidence without a second mixer read. These observations do not
clear an explicit logical mute.

This boundary is when Actions handles the intent. Its existing requests
are serial; each `amixer` subprocess has an 800 ms timeout, and one volume
command can perform multiple reads and a write. A waiting command can
therefore wait longer than 800 ms. This does not promise immediate mute
publication from socket arrival or physical silence within a fixed time. Actions restart, stale/unknown
state and a lost observation also prevent old audio from continuing.

Voice monitors speaker state off its coordination loop on a 20 ms cadence.
The final output check requires a cache observation no more than 100 ms old,
matching host/publisher/mute generation, current owner/privacy/capture
authority and a still-live Skills lease. Speaker permits are bounded by the
existing 100 ms output permit, the 500 ms Skills ceiling and the **actual
remaining** lease and original finish deadline. No file or socket work is
added to that final check.

Admission reserves the one active slot before reading the speaker cache, so
a second offer refused for capacity cannot contend with the current item's
permission check. A failed cache lookup returns that reservation immediately.
The permission check remains nonblocking: cache contention, stale data, or
an observation newer than the caller's sampled time cancels the item. A later
fresh observation cannot revive it or extend its original deadlines.

Stop, interruption/new owner, microphone privacy closure, capture gap, speaker mute,
invalid speaker state, definition revocation, daemon loss and deadline expiry
are terminal for the item. Reopening microphone privacy, unmuting or restarting a daemon
does not resume it. A completed model generation or accepted final chunk is
not confirmation retirement: Voice requires the matching speaker completion,
with no gap, before selecting media under the same spoken owner.

## Source example and validation boundary

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

The host target is **at most 250 ms from confirmed confirmation retirement,
with the asset prepared, to first accepted media PCM**. That boundary excludes
cold file preparation and the spoken confirmation itself. It remains
separate from acoustic onset. The focused host integration run on 2026-10-19
measured **483 us on the first request and 592 us on the repeated request**
using authored speaker IPC receipts. These fixtures exercise the actual
admission, dispatch and retirement helpers with a small checked WAV; they
do not run a physical speaker or measure the complete 48-second recording.

The earlier standalone focused suites passed 34 Voice tests, 12 Skills
authorization tests, four renderer tests and an initial 183 Actions/message
tests. Subsequent review added retained-observation regressions, covered by
the standalone final gate. The example rendered,
packed and installed into a disposable host directory; all seven installed
files matched the staged files exactly. The generated WAV's SHA-256 is
`134362ab72a19c143c627dbd438f3d17ceabe9c6923bad6f1ea3f7d5fb9579fa`.
The completed standalone workspace gate and native ARM64 result are recorded
with their published revision separately. The later text-origin complete
ten-crate consumer gate passed 4,288 tests with zero failures and 20 ignored,
none filtered, with workspace formatting, strict Clippy and build passing.
The verification record keeps the full workspace and subsequent consumer counts separate.
Those historical results qualify only their recorded revisions. The current
source and its native qualification are identified in the verification record.

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

After an accepted decline, Live refuses later or parked action calls for the
same request and cannot promote a queued private Music offer. An action
already executed before a later decline remains acted on both providers;
the decline does not undo completed work. Consent withdrawal through
ReportsStop remains available, without reviving the declined answer's speech.
The separate Policy-held withdrawal behavior remains owned by its existing
ReportsStop path. These guards preserve the original request identity and
Memory lease rather than issuing a fresh playback context.
