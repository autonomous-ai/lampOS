# Sensor 5 diagnostic recorder — source candidate

`lamp-sensor5-recorder` is a standalone Rust recorder for the consented Monday
pilot. It opens no camera or microphone device, grants no permission, changes
no service, and has no network/upload operation. No live capture, deployment,
board timing or physical A/V synchronization has been verified by this change.
The existing `capture.sh --live` remains held pending review and an authorized
operator session. Builds and tests below use synthetic data only.

## Reused owners and explicit opt-in

Source audit: lampOS `1e42209fce`, 28 November 2026.

- [Controls](../../../crates/msgs/src/controls.rs) is electrical evidence,
  expressly **not** a camera/microphone permit. A fresh unmuted switch alone
  never authorizes this recorder. Software mute remains authoritative.
- [Voice privacy](../../../crates/msgs/src/privacy.rs) grants the microphone
  lease. The recorder reuses [Ears' `LeaseGate`](../../../crates/ears/src/privacy.rs)
  for typed envelope, boot, controller, generation, original acquisition,
  expiry, sequence, restart and loss checks. A clip has one continuous lease
  era; it never reconnects after a loss or borrows a later lease for queued input.
- [Eyes](../../../crates/eyes/src/frame_link.rs) retains camera ownership and
  its existing Voice permission/hold. The recorder uses the existing
  [`FrameClient`](../../../crates/camera/src/frames.rs), retaining every original
  `CameraGrant` and `FrameMeta`. It never sends `camera:true`.
- Eyes' new `--frame-link-reader-policy sensor5-capture` selects **only** the
  same-UID kernel-identified `lamp-sensor5-capture.service` peer. It replaces,
  rather than adds to, the normal reader. The absent/default `seen` policy is
  unchanged: only `lamp-seen.service`. Root, user services, a similarly named
  service and an unknown peer get no special bypass. No service/unit file is
  installed or edited by this patch.
- Ears' existing `LAMP_BUS_AUDIO=on` export requires its existing privacy
  runtime (`LAMP_PRIVACY_CONTROLS=on` or `no-slide`). This recorder consumes only
  `mic1/audio` from `ears`: 16 kHz mono S16LE, 320 samples per native block.
  It additionally requires `LAMP_BUS_AUDIO_CAPTURE_PERMIT=on`, the default-off
  additive export of the owner's **original** capture ticket. An old/default
  PCM payload without that ticket fails closed. Its clock must equal this kernel
  boot; its nonzero Ears-local epoch must stay fixed with the Ears publisher boot.
  That epoch is never compared numerically with the recorder's lease epoch.
  The original `until_us` must exceed source completion, and validation and
  final archival admission must both precede it strictly. A later Voice lease
  cannot renew it. The boundary remains the existing Ears capture
  **read completion**, not an ADC timestamp. No `mic2` route is used.
  The 50 ms continuously fresh lease warmup is an additional conservative
  startup fence; it does not reconstruct missing source authority.

These shared libraries bring their existing inference dependencies into the
build graph. The recorder does not run their models. This reuses their tested
privacy and private-boots parser APIs without copying permission rules.

## Operator contract, after separate review and authorization

The parent directory must already exist. All generated directories/files are
new, private (0700/0600), local and retained; existing output is refused.
Participants are exactly P01–P04. The operator alias is O01–O99, distinct from
the participant. The local real-person map never enters Git or the archive.

```text
lamp-sensor5-recorder prepare --link-dir /PRIVATE/session-01/link
```

`prepare` creates new, distinct random controller/reader boots in
`link/frame-link.env`, in the existing Eyes format. It creates no socket and
changes no permission. A separately authorized operator must arrange Eyes to
use the **same** paths with:

```text
--frame-link /PRIVATE/session-01/link/camera.frames
--frame-link-boots /PRIVATE/session-01/link/frame-link.env
--frame-link-reader-policy sensor5-capture
```

The recorder itself refuses live `record` unless Linux names its current
cgroup as exactly `lamp-sensor5-capture.service`. An old/default Eyes refuses
this peer. There is no compatibility fallback, root shortcut or claim that a
service with the required configuration already exists.

```text
lamp-sensor5-recorder record --bus-root /run/lampos-bus \
  --link-dir /PRIVATE/session-01/link --output /PRIVATE/session-01/clip-01 \
  --consent-alias P01 --operator-alias O01 \
  --consent-record /PRIVATE/consent-P01.json \
  --consent-text /PRIVATE/consent-P01.txt \
  --consent-sha256 EXACT_SHA256_OF_JSON --seconds 10
```

The private consent JSON has exactly these fields:

```json
{"schema_version":1,"participant_alias":"P01","operator_alias":"O01","accepted":true,"scope":"sensor5-monday-local-av","accepted_at_utc":"2026-11-30T02:00:00Z","consent_text_sha256":"64 hex characters"}
```

The original accepted text must contain exactly one `Participant alias: P01`
line and exactly one `Consent accepted: yes` line, and exactly one nonempty
`Participant confirmation/signature (local only):` line. UTC accepts seconds or
1–3 fractional digits. Both supplied hashes must
match the exact bytes. Both sources are copied privately into the new archive.
This is an operator-provided consent audit, not automatic identity or consent
verification. Use 10 seconds for a pilot clip and 30 seconds for the playback
case. The generic recorder accepts 1–120 seconds, but the pilot adapter's
deliberate timeline limit is 35 seconds; longer archives are outside that adapter.
Recording stops on Ctrl-C, SIGTERM or
SIGHUP. It requires 50 ms of continuously fresh Voice permission before connecting
the frame client or subscribing to PCM. Initial lease wait plus warmup is bounded
at 1 s after archive setup, whose separate limit is 5 s. A loss ends the attempt,
without warmup restart. The requested duration begins at the capture-start
boundary after warmup, excluding setup/wait. Every media sidecar and the terminal
record retain `capture_started_us`; earlier source completions are refused.
Actual first/last sample times determine stream coverage. No capture readiness
is claimed during setup or while media have not yet arrived.

## Admission, stop and retention boundaries

The main loop targets a 2 ms poll. Privacy and PCM subscribers have separate
bounded workers; filesystem work in a Bus subscriber cannot hold the reader's
clock checks. Original source age must be strictly below 50,000 microseconds
at admission. Missing/stalled camera or PCM stops at 1,000,000 microseconds.
Invalid/expired/reordered privacy, queue loss, a publisher restart, changed
camera grant/worker/epoch, out-of-order media, source EOF, observer failure or
archive backpressure stops the whole clip. A terminal clip never resumes.

Encoding precedes a final privacy drain and a bounded real FrameClient probe
for queued camera EOF. A valid frame completed by a probe stays in one pending
slot for the normal consumer. After this final camera probe, only bounded
flag/clock/ticket checks precede enqueue: the original recorder ticket, the
immutable original Ears ticket for PCM, and a strict 50,000-microsecond source
age check. A camera close queued while encoding prevents retention. The
same observer/privacy/camera fence runs before duration success: observed loss
wins over `duration_elapsed`, even when both counters are nonzero. The
preallocated archive queue accepts at most 64 packets and approximately 4 MiB.
This enqueue is the **explicit archival admission** boundary. Data waiting in
the source buffers is not retained after permission loss. Already admitted
bytes may finish writing after closure: they were deliberately retained while
permission held. The archive child has no source handle, permission decision,
camera/microphone access or ability to request more frames.

All archive disk I/O is in that separate child. Stop first closes admission,
stops source workers and disconnects the camera client; it then allows up to
250 ms to finish admitted writes, and up to 500 ms total to kill/reap the child
process group. Linux also sets parent-death SIGKILL; a separate watcher checks
parent death on other platforms. A child blocked in uninterruptible kernel I/O
may not be observed reaped within that deadline; this reports failure rather
than extending the wait or claiming success. A normal process shutdown leaves
no archive child behind in the synthetic tests. Signals and bounds are source
logic, **not measured physical hardware latency**.

## Archive v1

`session.json`, original `consent.json` and `consent.txt`, `README.txt`, and
`inventory.jsonl` accompany `records/`. Every completed file is written with
`create_new` and SHA-256 listed in the inventory. An acknowledgement means
complete OS writes of raw/sidecar/inventory, not power-loss durability. The
inventory and result file are synced at completion; avoiding a disk barrier for
every 20 ms PCM block keeps slow storage from throttling capture. Raw media is unchanged:

| File | Sidecar, with `.json` appended to the raw filename |
|---|---|
| `00000001.frame.jpg` | `schema_version`, `record_index`, `payload_file`, `sha256`, `kind:"frame"`, original `frame:FrameMeta`, `received_us`, `source_frames_between`, current validated `privacy_evidence` |
| `00000002.audio.pcm` | Same identity/hash fields; `kind:"audio"`, complete Bus `header`, original `audio:Audio`, immutable `capture_permit`, `received_us`, `bus_gap`, `audio_sequence_gap`, `timestamp_boundary:"ears_capture_read_completed"`, `adc_timestamp:false`, `clock_provenance`, validated `privacy_evidence` |

Frame metadata keeps driver sequence and native timestamp/domain/point, host
dequeue start/end, copy time, camera worker/epoch/sequence, original grant,
MJPG format and dimensions. A camera sequence gap can include frames between
pull requests. A PCM sequence gap can include source loss/discontinuity. Neither
is an inferred duration. No gap is filled and no stream is made constant-rate.

`result.json` exists only after the child receives a terminal record. Its
`complete` is true only for duration completion with nonzero camera/audio counts
matching the parent's admitted counts. `result.reason` records closure/failure.
`INCOMPLETE.json` is best effort if a child detects EOF/failure. Missing terminal,
missing sidecar/hash, or `complete:false` is incomplete evidence. Every partial
file stays; **an unlisted partial file requires a recovery SHA-256 before any
analysis**. A blocked/killed writer cannot promise to finish hashing it. The
parent's stderr reports admitted versus acknowledged records without raw media.

All sessions say `physical_sync_validated:false`. Native camera cadence is not
guaranteed 25 fps; the actual Eyes pull cadence is preserved. A preview may
estimate the first PCM sample as read-completion minus 20 ms and use camera
dequeue completion, but those estimates are not physical synchronization.
Scoring needs separately checked beginning/end visible/audible anchors, offset,
drift, residual and uncertainty. Desk distance/light quality and real board
capture/closure latency remain Monday qualification work.

## Local verification

From `lampOS/`:

```sh
cargo fmt --manifest-path tools/sensor5-pilot/privacy-guard/Cargo.toml --check
cargo clippy --offline --manifest-path tools/sensor5-pilot/privacy-guard/Cargo.toml --all-targets -- -D warnings
cargo test --offline --manifest-path tools/sensor5-pilot/privacy-guard/Cargo.toml
cargo test --offline -p lamp-eyes frame_link
cargo test --offline -p lamp-eyes diagnostic_frame_reader
```

Tests use fake monotonic time, synthetic messages/JPEG/PCM and real local child
processes. A suspended child models blocked storage; it is not a real disk
fault or device run. The orphan test invokes its ignored helper in a subprocess.
Synthetic raw archives remain in the system temporary directory and tests print
their paths. No binary artifact or recorded media belongs in Git.
