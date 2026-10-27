# lampOS V2: live interaction release

Owner scope reaffirmed 2026-10-11: **first deliver natural, responsive live
interaction with a physical Lamp. Long-running tasks and new memory services are
future subsystems, not requirements or dependencies of this first release.**
Memory across days has since been built as its own process, `lamp-memory`
([memory.md](memory.md)), and the product runs it; the voice works on without it.

Current implementation has separate capture/DSP, speaker, physical privacy and
Gemini provider processes, optional ring output, and opt-in camera context.
Several eight-turn real-Lamp blocks and a three-turn camera conversation have
completed. The latest two short-answer playback cohorts contain no accepted-zero
speech gaps; earlier cohorts did. The conservative turn-taking baseline suppresses
overlapping input. Natural interruption and social admission remain unqualified.

Continuous `serve` and the independent motor worker are integrated. Motor output
is inactive. Camera startup/teardown corrections passed a native 31-frame check;
a later conversation queued a fresh camera frame for each of three replies.
This does not independently verify the model's visual descriptions. Optional
ring startup and runtime failures now disable presentation while retaining
healthy voice, with uncertain cleanup kept explicit.

The older 378.6-second reference-backlog failure remains recorded. A later
capture-clock session completed 16 exchanges over 21.6 minutes without a
reference fault. The latest candidate ordinary-conversation control completed
8/8 replies with kept-start enabled and no reported playback gaps or extra
inputs; it did not exercise interruptions. There is no qualified all-day service
or complete embodied performance. The owner now targets real-user pilot testing
by approximately 2026-10-12 02:24 UTC. See the
[latest native evidence](verification/provider-control-20261011/README.md),
[supervised pilot plan](supervised-pilot.md) and
[conversation evidence](connected-conversation-20261011.md) for measured limits.

The owner deferred V1 benchmarking. Preserve existing results in
[benchmark progress](benchmark-progress.md); continue V2 implementation and
physical acceptance tests. V1 source remains a reference for useful safeguards.
Source home: [top-level lampOS/](../README.md). Suggested human ownership, based
on V1 history, is in [team ownership](team-ownership.md).

## Product boundary

**Chat with a physical robot.** Lamp sits in a fixed position on the left or
right of a desk, beside one person using a computer. Speech, listening, head,
body and ring form one coherent interaction. Stillness and silence are valid.
A colleague, a call and computer audio are part of this desk environment.

The product goal is a believable, attentive character the owner can talk with
throughout the day. Judge whole interactions: noticing an addressed question,
listening through a hesitation, answering, yielding to a topic change and
settling again. Voice, light, head and body should express the same intention
with purposeful timing and variation. Do not turn each state transition into
a mandatory gesture. Camera, microphones and sensors supply evidence with
freshness and uncertainty, not invented awareness. Software tests establish
contracts; observation of the physical performance and the owner's judgment
establish whether the result feels natural.

The first version includes:

| Experience | Required behavior |
|---|---|
| Conversation | Gemini Live handles short and extended chat, follow-ups, short answers to Lamp's questions, corrections, hesitant speech and topic changes. Keep the current conversation and explicit interaction settings available. |
| Interruption | The person can start a new addressed question during an answer. Retain the opening words, yield speech promptly and follow the new meaning. Measure and distinguish a listener's brief acknowledgment from an interruption. |
| Expression | One choreographer coordinates the voice, head, body and ring using actual input/playback events. Use small purposeful gestures with cancellation and repetition limits; no gesture is required on every turn. |
| Vision | Use fresh camera evidence for desk presence, permitted gaze adjustment and objects the person shows. Admit relevant frames to the live conversation. Report uncertainty and camera unavailability honestly. |
| Sensors and controls | Acquire the installed sensors efficiently, preserve per-field freshness and respect privacy/touch/button controls. Sensor data supports conversation and local behavior; it does not trigger repetitive unsolicited commentary. |
| Desk setup | Verify left/right placement, a safe motion envelope, useful sight, hearing and audible speech. A saved pose is not fresh placement or presence evidence. |
| Social restraint | Remain quiet while the person works, talks to someone else or joins a call. Face presence, recognized voice or a passing mention of Lamp is insufficient on its own. Looking at the monitor must not disqualify clearly addressed speech. |
| Failure | An admitted request gets a useful answer, relevant clarification, honest failure or recorded user interruption. Lost connections must not leave false listening/working cues or silently lose accepted requests. |

Agentic tasks, Harness/Codex sessions, browser/computer automation, background
research and Hindsight are deferred. Memory across days is `lamp-memory`
([memory.md](memory.md)), off the live audio path.
Local camera/sensor reads and bounded light, volume or posture controls are part
of the interaction itself. The model must not claim to have run an excluded task.
No autonomous room search, stock spoken waiting fillers or random idle movements.

## Rust and reuse

The [V1 / ROS 2 reuse decisions](reuse-decisions.md) identify useful contracts,
legacy hazards and existing Rust candidates. Read that evidence before building
a new subsystem; source inspection and upstream benchmarks are not Lamp results.

All Lamp-owned runtime code lives in top-level `lampOS/` and will be Rust:
audio capture/playback coordination, provider transport, conversation control,
choreography, camera/sensor acquisition, motor/LED control and diagnostics.
The release has no Python runtime or Python HAL dependency. A temporary Python
bridge is not the migration plan for this release.

Port tested hardware protocols, calibration interpretation and safety behavior
as Rust implementations, using the existing code and device fixtures as
references. Existing constants and tests are evidence to check, not proof that a
port is correct; verify units, limits, timing and failure behavior on the device.
Retain useful model assets and established DSP/inference libraries through Rust
interfaces where appropriate. Review native dependencies, isolate unsafe/FFI,
and account for their memory and threading. An algorithm whose only available
integration requires Python must be ported, replaced or explicitly revisited
before release; do not silently add a Python sidecar.

Treat `lampOS/` as the repository root. Copying this directory into a new
repository must suffice to build, test and run V2 after its documented device
configuration and external dependencies are provided. Startup, configuration,
process supervision and all interaction duties required by V2 live here in Rust;
no sibling source, Python HAL service or Go os-server service may be required.
Credentials and per-device calibration are provisioned data, never committed
secrets or hidden path dependencies. Verify a source-only build outside the
parent repository.

Fleet management and the existing web UI are outside the first release scope.
Any setup, UI or update component needed by lampOS must be owned within this
repository boundary; it cannot become a sibling-service dependency. V2 must
install and operate while legacy HAL and os-server are stopped. A hidden legacy
fallback or Python/Go adapter is not an acceptable release. Retiring the old
implementation is conditional on complete device qualification.
Keep the existing runtime available for rollback, with exclusive device ownership
during each run. Rust provides useful ownership tools; bounded resources and
measured scheduling remain explicit design obligations.

## Runtime responsibilities

The entry point is `lamp-voice`. The target is a small interaction/supervision
core over Linux, with independent component workers. It is not a new kernel.
The I/O-free `lamp-turn-owner` crate defines ownership, privacy and cancellation;
`lamp-voice` integrates that policy with workers. A crate is a code boundary, not
automatically a process. Different worker roles can share an executable while
running as separate processes with their own lifetime and resource bounds.

The core is the coordination point. Each named worker below is a separate
process; the supervisor, interaction controller and choreographer are modules
inside the one core process. Owners shown are proposed human assignments.

```text
                        +---------------------+
                        | Gemini Live         |
                        | Cloud service       |
                        +---------------------+
                                   ^
                             WebSocket/TLS
                                   v
                        +---------------------+
                        | Gemini worker       |
                        | ndtran11-png        |
                        +---------------------+
                                   ^
                           Local request/reply
                                   v
INPUT WORKERS           ONE CORE PROCESS          OUTPUT WORKERS
+-------------------+   +---------------------+   +-------------------+
| Microphone + DSP  |-->| CORE: lamp-voice     |-->| Speaker           |
| ndtran11-png      |   | Owner: Leo          |   | ndtran11-png      |
+-------------------+   |                     |   +-------------------+
                        | Supervisor          |
+-------------------+   | Turn controller     |   +-------------------+
| Camera            |-->| Choreo: Darren      |-->| Ring / light      |
| loclt1011         |   |                     |   | Darren            |
+-------------------+   | One shared state    |   +-------------------+
                        | One current turn    |
+-------------------+   | Bounded queues      |   +-------------------+
| Physical privacy  |-->| Permission checks   |-->| Head / body *     |
| Kyle              |   | Cancellation / Stop |   | Kyle              |
+-------------------+   +---------------------+   +-------------------+
```

The left-to-right arrows show observations entering the core and commands leaving
it. Workers return progress, rejection, completion and failure messages to the
core. The Gemini worker alone owns this diagram's cloud connection. The core
owns turn admission, cancellation and permissions; the choreographer converts
that shared state and observed playback progress into purposeful presentation.

Two explicitly permitted data paths avoid routing large or tightly timed payloads
through the core:

```text
Speaker ------ accepted playback reference ------> Microphone / DSP
Camera ------- selected, authorized image --------> Gemini worker
```

Local worker channels use Unix sockets. Priority control is separate from data;
Stop, privacy and authority updates retain their own channel when audio is full.
Messages carry process identity, ordering and time. Output commands also retain
turn ownership and expiring permissions, checked again at the hardware boundary.
Camera payloads have their own bounded stream. These are scheduling and ownership
contracts, not a claim of zero latency or hard real-time Linux scheduling.

`*` The motor worker is connected through an opt-in live supervision path.
The gesture producer and physical motion qualification are still pending;
ordinary conversation does not dispatch a motor segment. This map does not
claim every component is deployed.

```text
ADDITIONAL WORKERS                PROPOSED OWNER       STATUS
Perception / gaze                loclt1011            Candidate: face bearing only, eyes.md
End-of-turn inference            ndtran11-png          Candidate
Buttons / touch                  Kyle                 Pending
Environment sensors              Kyle                 Later priority

CROSS-COMPONENT RESPONSIBILITIES
Linux / systemd / install        Elvis (69tc)
Desk setup UX                    Tony (jolhada)
Hardware setup / calibration     Kyle
Whole-system integration         Leo
Whole-interaction acceptance     Dedicated QA: unassigned

FUTURE SUBSYSTEMS: outside the live conversation path
Harness task adapter             Owner: TBD

OUTSIDE THE LIVE CONVERSATION PATH, BUILT
Memory across days               Builder fourth       Product unit lamp-memory, memory.md
```

These are **proposed human assignments**, based on the
[V1 ownership evidence](team-ownership.md), not confirmed staffing. Preserve
Git account names where the person's name is not established. Darren is proposed
as the expression owner; V1 ring-driver history supports Leo as its technical
reviewer. Tony and Kyle share the setup handoff, not one hardware writer.

The diagram is the V2 responsibility map, not a claim that every worker is
already deployed. `process` means an independent OS lifetime; the choreographer
is policy inside the core, not another authority or actuator writer. Capture and
speaker are separate processes even though they share the audio crate. Perception,
end-of-turn inference and motor gestures still require their own
integration/qualification gates. The current physical privacy process does not
imply that every future button/touch behavior is implemented.


Keep continuous capture, echo processing and local cancellation independent of
network calls, inference, presentation and persistent storage. The capture
worker may contain its tightly coupled DSP/VAD stages; this does not require one
process per function. Camera acquisition and heavy perception run separately.
End-of-turn model inference also belongs in a supervised worker, not a thread
inside the core. One motor worker owns the serialized five-servo bus; each
actuator bus has exactly one writer. Do not create competing hardware owners
to make a diagram look more modular.

The core handles small bounded state transitions and scheduling decisions.
It does not decode images, execute models, wait for disk writes, wait for child
exit in a live loop, or carry out cloud/tool calls. Logging is diagnostic
evidence, not the authoritative conversation state. Audio and image payloads
have bounded data paths; privacy, stop and cancellation have independent priority
control. Capacity, deadlines, drop/refusal behavior and worker generations are
explicit contracts. Increasing queues is not a substitute for meeting them.

### Failure containment requirements

These are release requirements; current gaps are listed above.

| Failure | Required effect |
|---|---|
| Camera, perception or an optional environmental sensor | Mark that capability unavailable; retain healthy audio. Never reuse stale observations as current evidence. |
| Ring | Inhibit further ring output and record degraded presentation; retain healthy conversation. A missing light cannot claim readiness. |
| Motor | Reject further motion, perform only the qualified local safety action, and retain voice. Do not claim the mechanism physically stopped without evidence. |
| End-of-turn inference | Use the documented bounded silence fallback; never block capture, privacy or cancellation. |
| Microphone/audio integrity | Stop accepting input and remove listening readiness; preserve safe controls and report unavailability through a working permitted output. |
| Gemini transport | Preserve local privacy and controls, report affected turn outcomes, and recover only under the provider's explicit session contract. |
| Core | Worker authority expires locally, outputs take their safe state, and the OS supervisor may restart a fresh core. Stale callbacks cannot regain authority. |

Inhibit a failed capability before starting teardown. Recovery has bounded
attempts and requires actual old-worker exit, a new identity, fresh permissions
and verified readiness. A cancellation request, a process kill and a completed
hardware stop are different observations. Never restart a second bus owner
while the first may still hold the device. Standard Linux scheduling and Rust
alone do not establish hard real-time behavior; verify service tails under load.

Code enforces privacy, output ownership, expiry, cancellation, readiness and
motion bounds. Gemini supplies meaning, language and optional expressive intent.
Whether someone is addressing Lamp remains a fallible perception decision:
evaluate false acceptance and false rejection, not just prompt compliance.
Each observation carries its age and uncertainty. Each output retains its turn
and playback ownership through the final write. A newer turn invalidates old
speech and conversational gestures without requiring a model-generated stop.

## Future task and memory boundaries

The task service below is a placeholder. Do not add idle services, mock
production APIs, model dependencies or release gates for it in this phase.
The memory service is built: see the end of this section. The live interaction
release must start, converse and handle interruptions without either subsystem
installed or reachable.

**Task service:** reserve an independent adapter to Harness for long-running
work. Reuse Harness's task/session registry, routing, execution and recovery
contracts after inspecting their then-current implementation. Do not build a
duplicate agent manager now. A task has a durable identity and lifetime separate
from the current spoken turn. A topic change or interruption of speech does not
implicitly delete a task or its result. Completion is an event that the
interaction owner schedules appropriately; Harness or an agent never writes
directly to the speaker, lights or motors. Agent-engine selection is deferred.

**Memory/context service:** reserve one shared boundary for useful information
from both conversations and task outcomes. Treat Gemini's active conversation
context as temporary working context, not Lamp's durable source of truth.
Do not assume that an engine's session history is automatically shared across
engines or constitutes an owner memory system. The future service should provide
a bounded, ready-to-use context snapshot to the conversation layer and relevant
context to tasks, with provenance and explicit correction/deletion behavior.
Extraction, persistence and larger retrieval happen outside the live audio path.
Ordinary conversation must not wait for a memory search on every turn.

This boundary is built as `lamp-memory` (`crates/memory`, the fourth builder's),
its own process and user. The product unit starts it with `--memory-days`, a
SQLite store with ranked keyword search and 30 days kept, and the voice uses it
through the Days client (`LAMP_MEMORY_DAYS=on`). A new session gets a frozen
note of what it was asked to keep within 300 ms; earlier conversations are
looked up with a `recall` tool call, not searched on every turn. The design,
storage, retention and forgetting are in [memory.md](memory.md) and
[memory-days.md](memory-days.md). Hindsight is not a dependency. Do not claim
that a preference was saved for future sessions when it was not.

## Qualification

These are V2 acceptance targets, not measured improvements. The owner deferred
the [V1 comparison protocol](comparison-protocol.md); do not run that comparison
until it is resumed. Live conversation on the physical Lamp is the primary
evidence: cached speech
from the iMac speaker, Lamp's real microphones and Gemini, and the answer from
Lamp's speaker. A continuous independent room recording establishes the audible
boundaries. Modified V1 is excluded from the comparison. Component measurements
diagnose the result; they cannot establish a conversation speedup.

| Measurement boundary | Initial gate |
|---|---|
| User speech end to first substantive audible answer word | p50 ≤2 s; p95 ≤4 s. Waiting phrases and nonverbal cues do not count. |
| Admitted interruption to acoustic speaker silence | p95 ≤200 ms. Also report speech onset to admission, lost opening words and false interruptions. |
| Valid input/playback state transition to visible ring change | p95 ≤50 ms. Readiness requires actual retained input. |
| Local control event to controller acceptance under concurrent load | p99 ≤5 ms. Measure physical effects separately. |
| Motor, camera and sensors | Verify joint safety, control-cycle jitter/overruns, observation age, acquisition cadence and faults on the actual installed hardware. |

Use comparable before/after runs on the same device, voice, model settings and
fixtures. Start with at least 100 conversational turns and 50 addressed
interruptions, plus separate acknowledgment and false-interruption cases.
Report all failures and separate first use, warm turns and reconnection.
Include extended listening, partial thoughts, mid-sentence corrections,
camera-off/darkness/occlusion, an object shown at the desk, left/right placement,
privacy transitions, slow providers and component restart.

Run 4 hours of mixed background speech/calls/media with no unsolicited speech or
social gestures in that test, and an 8-hour concurrent audio/vision/sensor soak.
Synthetic voices support repeatable timing tests; real people are needed to
validate multiple speakers and comfortable body/head/light behavior. Zero
failures in these samples is not a claim of universal reliability.
The owner must find the combined experience natural in normal desk use.

Implement in complete, measured slices: first continuous audio with local
interruption and ring ownership; then safe head/body choreography and fresh
visual/sensor context; then integrated behavioral and sustained-load
qualification. An audio-only demo does not complete this release.

## Implemented test tooling

The `lamp-sound-fixtures` crate contains a versioned catalog of 20 desk scenarios and
24 reusable utterances. Ownership, transport and audio processing now have Rust
implementations as described below. Gemini transport and a directed voice process
integration are described in [live runtime](live-runtime.md). The directed runtime
still has no qualified addressee classifier or integrated
motor choreography. Its opt-in camera context and ring cues do not qualify the
complete desk experience. Native checks run in an isolated build directory on
Lamp; installed HAL services have not been replaced.

The catalog covers two colleagues, speech to another device, calls, media,
overlapping voices, synthetic ventilation/keyboard noise, hesitant speech,
short answers, topic-change interruptions, listener acknowledgments, visual
questions and fast follow-ups. Scenarios specify preconditions, expected
behavior and a start/speech-start/speech-end trigger. The current renderer
creates assets; a future runner must observe and execute those triggers.

From `lampOS/`:

```sh
cargo run -p lamp-sound-fixtures -- list
cargo run -p lamp-sound-fixtures -- render .cache/acoustic .cache/render-first.json
cargo run -p lamp-sound-fixtures -- render .cache/acoustic .cache/render-repeat.json
cargo run -p lamp-sound-fixtures -- verify .cache/acoustic
```

The ignored `.cache/acoustic/` directory keeps rendered PCM and asset manifests
on this worktree's disk for reuse. Keep it when cleaning build output; it is not
under `target/`. Binary audio is not committed. Speech is keyed by text, voice,
rate, explicit voice revision, macOS build, speech-engine executable hash and
render version. Mixes are keyed by their audio inputs and composition. Changing
an expected answer or an event trigger does not regenerate identical audio.
The renderer uses the installed Mac voices; rendering calls no language model
or cloud TTS service.

Each cache read verifies the WAV hash, format, frame count and signal metadata.
A timed interprocess lock prevents duplicate synthesis of the same asset.
Complete objects publish atomically; failures do not publish partial objects.
Decoded speech is bounded to 300 seconds per catalog; each scene is at most
60 seconds. Overlapping sounds are attenuated together to avoid clipping,
and the attenuation is recorded. Noise has deterministic seeds and a digital
dBFS level; neither dBA in the room nor acoustic SNR is implied.

Reports preserve their existing contents, record reuse/generation counts and
explicitly say `rendered_not_run_or_scored`. Assets alone are not evidence that
Lamp passed a scenario. Acoustic recordings, synchronized observation and
scoring will be separate run artifacts.


## Implemented runtime components

`lamp-turn-owner` is a fixed-size, I/O-free controller with immutable turn and
output-plan tokens. An asynchronous callback retains its original plan; it cannot
renew a cancelled turn or an obsolete listening/speaking cue. The final writer
checks a short output permit against fresh controller authority immediately
before each bounded write. Capture retention and input admission have independent
leases, so playback does not turn off hearing. Input leases have a one-second
ceiling; output and controller authority have 250 ms ceilings. Those are failure
bounds, not measured interruption latency. Camera and microphone privacy have separate lineages. Capture uses its own
privacy generation so a coalesced mute/reopen discards retained audio without
reopening the microphone on every conversation turn.
Observations distinguish acquisition time, source incarnation, uncertainty and
freshness. This controller does not infer who someone addresses.

`lamp-ipc` provides connected private Unix datagrams, an 8,192-byte cap, truncation
rejection, finite kernel queues and nonblocking backpressure. Control and bulk
PCM use separate endpoints. Timestamps use the same host monotonic clock across
processes, with independent boot IDs to exclude data from an earlier process.
`lamp-ipc::camera_authority` also supplies a standalone, currently unused
single-hold socket witness. One owner shutdown revokes all SCM_RIGHTS duplicates
of the same observer endpoint; clients only close their own descriptors, never
shutdown or change shared file-status flags. A fixed 48-byte V3/token/nonce grant
uses caller-owned connected control streams and deadlines, incremental
nonblocking steps, exactly one owned FD and exact control EOF. The receiver
checks liveness before exposing a ticket; final checks use one non-consuming
`PEEK | DONTWAIT` receive (only WouldBlock permits, other outcomes latch loss).
The primitive does not authenticate peers or prove camera permission freshness,
a person's identity, captured-frame origin or request ownership. Callers must
supply those independent checks and safe close/reopen ordering. It has no Eyes
or Voice consumer, no enabled feature or new service, and does not resolve the
Common explicit-close hold by itself. Revocation linearizes at successful owner
shutdown, not datagram receipt or a later physical camera stop; a subsequent
close cannot recall bytes already dispatched. There is no hidden connection,
wait, readiness loop or deadline renewal. Target first/repeat grant and hot-check
latency remains unmeasured; one syscall is a work bound, not a wall-time bound.

The `lamp-ipc-probe` executable measures real parent/child control roundtrips
while the bulk endpoint stays full; it has finite deadlines and child cleanup.
It does not measure speech recognition, model time, audible output or visual cues.

`lamp-audio` wraps Sonora 0.2.0, a pure Rust WebRTC audio-processing port, with
16 kHz capture and 24 kHz render reference in 10 ms blocks. AEC3 and moderate
noise suppression are enabled; automatic gain changes are disabled. Reference
must come from audio actually accepted by the speaker, never generated but
unplayed provider audio. Delay values outside 0–500 ms are rejected. Lost frames,
privacy transitions and device faults require explicit discontinuity handling.
The synthetic benchmark measures cold initialization, first block, warm
processing percentiles, missed 10 ms budgets and an echo fixture; live room
alignment, clock drift, double-talk quality and audibility remain unqualified.

The Linux speaker boundary uses safe ALSA bindings and the board's configured
speaker alias, preserving its mixer path. It requests mono 24 kHz PCM, at most a
10 ms period and 40 ms buffer, and rejects a negotiated format outside those
bounds. Each nonblocking write is at most 240 samples and checks ownership at the
write boundary. Cancellation drops queued audio rather than draining old speech.
Transport failure latches authority closed. Native tests use ALSA's `null` plugin
and produce no sound; the physical speaker and its acoustic stop latency still
require measurement. The complete runtime must enforce one speaker owner and
service urgent control and expiry independently of new audio arrival.

Queued speaker audio retains the permit for every accepted chunk, including its
camera lineage and deadline. A newer chunk cannot hide a revoked or expired
older chunk still in the device buffer. ALSA queue observations retire metadata
only for frames reported consumed; this is not an acoustic silence measurement.
A rejected old control or PCM packet cannot stop a newer valid reply. A failed
hardware discard preserves pending metadata, closes authority and returns an
error instead of reporting that playback stopped.

`lamp-ring` encodes the installed 32-pixel GRB ring into one 1,028-byte SPI frame.
It validates the configured channel ceiling (maximum 120), checks fresh output
ownership at the final write and preserves newer valid cues when stale packets
arrive. Revocation, expiry and transport loss blank the ring; write failures
latch it closed. The Linux transport checks SPI mode, bit order and 6.4 MHz
settings and rejects short writes. Its 10 ms write budget is measured when the
syscall returns, not a hard timeout. A supervised worker must service control
and call its expiry tick at least every 10 ms. Cooperative file locking does
not exclude legacy HAL or boot/shutdown services; cutover must ensure one writer.
`lamp-voice` now supplies that supervised worker and a bounded phase policy;
its cooperative tick is 2 ms. Tests cover codec, cancellation, cross-channel
ordering, stale feedback and fault paths. Physical light timing and brightness
remain unmeasured. See [ring choreography](ring-choreography.md).

The camera-ticket receiver uses a shared platform-bounded ancillary capacity
(512FD storage on Darwin,2 elsewhere; still exactly1FD accepted). This covers
the pinned native Darwin UNIX control-record bound so excess rights are safely
owned/dropped before rejection, avoiding the cached library's truncated-control
panic. The grant receiver uses it; the separately prepared Eyes request
receiver consumes it in its own unqualified integration. A successful
Darwin unnamed getpeername returns no address in rustix; that is distinct from
an ENOTCONN syscall error. Family/type/connection/CLOEXEC checks remain required.
Retained failures and subsequent source repairs are described in reuse-decisions.

The grant sender explicitly sets its dedicated control stream nonblocking after
checking the original deadline and socket. Darwin MSG_DONTWAIT alone does not
exclude its full-buffer space wait. Observer flags are never changed. The
backpressure fixture fills nonblocking, restores a blocking control sender, and
checks the API under the original one-second deadline. A separate three-second
control-peer rescue cancels and joins on normal exit or unwinding; it cannot
revoke a camera witness. Current qualification is recorded on
[the standalone IPC pull request](https://github.com/autonomous-ai/lampOS/pull/3632).

On Darwin only, the validated first grant send can return EMSGSIZE when the
UNIX peer has consumed the remaining send high-water mark. For the fixed
48-byte frame with exactly one descriptor, this is pending under the original
deadline; no bytes or rights were accepted. Continuation errors still fail. A
new drain-and-resume control complements the unchanged expiry control.

The recovery control compares the shared observer's NONBLOCK bit, the property
that control setup must leave unchanged. Darwin descriptor garbage collection
can change its internal FMARK bit while unrelated rights close, so whole-status
flag equality is not an application invariant. The original seventeen controls,
fixed deadlines and exact-once frame/descriptor assertions remain.

Camera grant CONTROL framing and OBSERVER authority have separate validation.
The caller authenticates the connected control peer before the challenge; each
transfer still requires a native UNIX STREAM. Both control write halves may
already be closed before queued bytes/rights are consumed: Darwin can return
EINVAL from getpeername at that point despite a valid queued reply and EOF.
Control parsing therefore does not require a still-connected peer. The received
observer keeps strict connected-UNIX/type/CLOEXEC and post-handshake silent-open
checks; source/capture/request/expiry guards remain independent. No generic
EINVAL acceptance or deadline extension. The named-socket/two-observer control covers queued rights and EOF after both
control write halves close; source-scoped gate results are tracked on
[#3064](https://github.com/autonomous-ai/lampOS/pull/3064).

The Eyes opaque-name fixture preserves exact byte-path checks. Linux must bind
that actual opaque name; macOS uses it too when supported. Only a native EILSEQ
refusal allows a separate real ASCII endpoint for every existing-server and
replacement-inode check; every other startup error still fails the fixture.
This adapts a host filesystem fixture without changing production path policy.
