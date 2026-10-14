# Third builder's work log

Third works on `lamp-third`, with one `third/<task>` branch per task.
The running list belongs to First on `origin/lamp-voice`.
All measurements below are from the development machine unless a different
machine is explicitly named. Third has no lamp access.

## 2026-10-14 — expression — taken

- Task: 4, Expression: one part that authors what the lamp shows.
- Base: `origin/lamp-voice` at `1e1fb807a3486944cf55298cf3b1c2640b0cc3eb`.
- Branch: `third/expression`, based on `lamp-third` after this claim.
- Scope: new `crates/expression`, its bus types in `crates/msgs`, workspace
  membership, clock-driven tests and replay fixtures, and English documents.
  The body and voice integration remains with First. Outputs will require
  explicit enablement and expire; the existing lamp behavior stays in place.
- Host identified so far: Darwin 25.6.0, x86_64. The repository pins Rust
  1.96.0; installation and the unchanged workspace build are the next checks.
- Latency targets to test with an injected clock: interruption light request
  within 100 ms, body transition about 350 ms; return requested before the
  scheduled first sound; pause nod suppressed when speech resumes. Hardware
  lag and how any motion looks remain lamp-only checks.
- Questions: none yet. Any missing input contract will be recorded here with
  a proposed interface and its limitations.

## 2026-10-14 — expression — done on lamp-third

- Task branch: `third/expression`.
- Code and its documents: `b0c5cdd1ac862e0b101de70085f7255b3c1cf909`
  (`lampOS: add an optional expression planner and replay`), fast-forwarded
  onto `lamp-third`. No changes to hands-off crates, deployment or the list.
- Built `lamp-expression`: a bounded, injected-clock planner for the five
  JSON-authored behaviors, normalized gaze and calibrated-owner radian
  offsets, expiring body/light requests, persistent light/concentration
  settings, explicit session/turn cancellation, diagnostics and a one-owner
  process. It is shadow-only unless `--enable` is supplied. It opens no
  hardware and installs no service.
- Added seven optional message contracts without replacing existing types
  or `light/command`. Added bus recording import, deterministic replay,
  bounded output recording, the 42-input written scenario and a host cost
  example. [Expression](expression.md) gives the exact publisher/consumer
  contract; [reuse decisions](reuse-decisions.md#expression-authored-timing-and-bounded-requests-2026-10-14)
  records V1/current code and primary research inspected before building.

### What was run, and where

Third's development Mac: Intel Core i7-10700K @ 3.80 GHz, 64 GiB RAM,
x86_64-apple-darwin, macOS 26.6.2 / Darwin 25.6.0, Rust 1.96.0. The unchanged
workspace built before the task. These are host results, never lamp results.

From `lampOS/`, the final required gate passed:

```sh
cargo fmt --all --check
cargo clippy --offline -p lamp-expression -p lamp-msgs -p lamp-actions -p lamp-body -p lamp-voice -p lamp-voice-eval -p lamp-eyes -p lamp-health -p lamp-name -p lamp-policy --all-targets -- -D warnings
cargo test --offline -p lamp-expression -p lamp-msgs -p lamp-actions -p lamp-body -p lamp-voice -p lamp-voice-eval -p lamp-eyes -p lamp-health -p lamp-name -p lamp-policy -- --test-threads=1
cargo build --offline --workspace
```

Tests: **1,701 passed, zero failed, five existing ignored** across 88 suites.
Expression has 67 passing tests (32 engine, 21 replay, nine runtime, three
process and two CLI); its eight new message-contract tests also pass. None
of these new tests is ignored. The five inherited ignored cases are the
model-backed endpoint case and long-duration Voice/reference soaks, named in
the [machine-readable evidence](verification/expression-host-20261014/results.json).

A source-only copy outside the parent repository built the workspace and
passed 92 Expression/message checks, including the existing doctest. All
23 local packages resolved inside that copy; no parent source was needed.
The independent target directory was
`/private/tmp/lamp-expression-standalone-3n2l05g5/target`. Its commands were
`cargo build --offline --workspace` and
`cargo test --offline -p lamp-expression -p lamp-msgs`, run from the copied
`lampOS/`. Unix socket tests needed host permission outside the default
sandbox. No device permission was used.

Failures kept: initial compile used the bus's private declaration checker;
the replay now validates locally. Strict clippy rejected a Boolean expression;
it was simplified. The initial written fixture passed 18/20 replay tests:
missing Voice heartbeats expired its turn; the fixed fixture has explicit
heartbeats and all 21 final replay tests pass. Default sandbox process tests
hit `Operation not permitted` for sockets; the authorized host run passes.
The first full dependent run, with default test parallelism and an overlapping
source-copy build, failed 28 existing Voice library tests (702 passed, five
ignored), including socket-buffer exhaustion and fixture deadlines. The same
code passed serially after the build finished; no Voice code was changed.
The [full failing assertions](verification/expression-host-20261014/parallel-test-failures.txt)
are retained. Unrestricted offline metadata also lacked a cached Linux-only
ALSA crate; host-filtered metadata passed. No Linux build is claimed.

Two release benchmark invocations ran 100 repetitions each of the same
42-input, two-simulated-second replay (101 ticks, zero rejected inputs).
The final series measured engine construction, input parsing, planning and
JSON serialization to a sink: first iteration **0.284349 ms**, median
**0.192103 ms**, worst **0.296518 ms**. Catalogue parsing precedes timing.
This excludes process startup, bus/disk work, scheduling jitter and all
physical response. Both series are in the evidence; this is not a lamp
latency or a before/after improvement.

### Exactly what First runs and wires

With the built binary on `PATH`, from the standalone `lampOS/` source root
on the lamp, this one command exercises all five written behaviors without
opening hardware:

```sh
lamp-expression replay --input crates/expression/fixtures/five-behaviours.jsonl --clock-id written-scenario --enable --until-us 2000000 --output /var/tmp/expression-written-20261014.jsonl
```

Use a new output filename on a repeat; existing files are refused. Expect
42 inputs, 101 ticks and zero rejected inputs. The requested yaw reaches
zero at 1,160,000 us, 40 ms before the written first-audio time. That is
authored timing, not measured joint motion.

First's wiring is deliberately outside this task's permission:

1. Voice publishes `voice/timing`, `voice/activity` and `voice/playout`
   using the shared `lamp_ipc::monotonic_us()` clock and Linux boot identity,
   with Voice session/turn/request identities. Publish state/activity at
   least every 100 ms and keep terminal events in the heartbeat until the
   next turn. `WorkerEvent::Playback` supplies useful cursor/queue evidence;
   ALSA acceptance is not a measured acoustic onset. Renew room evidence;
   today's change-only room publication expires after five seconds here.
2. Body subscribes to `expression/body`; the ring owner subscribes to
   `expression/light`. Each needs its own switch, off by default. Validate
   clock, session, sequence, turn, lease, privacy and local safety at the
   actuator boundary. These are relative offsets and normalized gaze for
   Body's calibrated solver. Actions takes priority. Expiry restores the
   existing behavior; an expired request must never release a privacy stop.
3. Supply actual light settings and explicit concentration. Do not infer
   concentration from room silence. In shadow mode inspect behaviors and
   missing inputs; actuator payloads are deliberately absent.

After those input publishers exist, record one real stretch, with the built
tools on `PATH` and their usual `LAMP_BUS_ROOT`. Run the shadow planner in
one terminal and the recorder in another while exercising the five cases:

```sh
lamp-expression serve --seconds 65 --record /var/tmp/expression-shadow-20261014.jsonl
lamp-bus record /var/tmp/expression-inputs-20261014 voice/timing voice/activity voice/playout attention/concentration body/joint_states humans/faces/tracked room/state light/command --seconds 60 --max-mb 4
lamp-expression import --clock-id "$(cat /proc/sys/kernel/random/boot_id)" /var/tmp/expression-inputs-20261014/*.jsonl > /var/tmp/expression-inputs-20261014.jsonl
lamp-expression replay --input /var/tmp/expression-inputs-20261014.jsonl --clock-id "$(cat /proc/sys/kernel/random/boot_id)" --enable --output /var/tmp/expression-replayed-20261014.jsonl
```

Use fresh paths for each capture. A latched setting hours older than the
clip can exceed replay's ten-minute bound: record a fresh setting publication
or supply an explicit replay record at its known load time. Import cannot
recover receipt times, clock steps or cross-topic arrival order.

Only after inspecting that plan, run `lamp-expression serve --enable` and
explicitly enable the two consumers. Measure actual joints/light against
audio: return before the first audible word, nod inside a person's pause,
cancel on resumed speech, light before the 350 ms interruption transition,
and stillness during concentration. Kill Expression and verify fallback
after its at-most-200-ms leases expire. Reject late old-turn/session input
and verify privacy/input loss never leaves a listening face. Never delay
speech to finish a gesture.

**Most likely wrong on the lamp:** the small nod/sway falls below Body's
20-count deadband, and unknown motor lag makes planned return differ from
observed return. Check these first, with actual placement/clearance before
tuning amplitudes. Then check clock-domain conversion, cancellation lineage,
and room evidence that stopped renewing. The body-lag default is unknown;
setting a guessed value would only hide that missing measurement.

Questions blocking further Third work: none. First owns the producer and
consumer integration, calibration and acceptance on the lamp.

## 2026-10-14 — controls — taken

- Task: 7, the button and the touch pad; next in Third's fetched order.
- Base: `origin/lamp-voice` at `8335dfbce6a461963e25695d11cb1f1be792c1a6`,
  after rebasing `lamp-third` and pushing it with a lease.
- Branch: `third/controls`, based on `lamp-third` after this claim.
- Scope: new `crates/controls`, a small pin-reading trait and stand-in,
  injected-clock gesture recognition, additive message contracts, bounded
  process/replay tests and `docs/controls.md`. First owns the actual GPIO
  adapter and Voice/Body consumers; this task touches neither owner.
- Research starts with V1 button/touch code and tests, the controls verdicts,
  maintained input interfaces, and `docs/service-privacy.md`. Physical privacy
  remains authoritative. A gesture must never grant capture permission or
  clear a privacy fault. Optional behavior requires explicit enablement.
- Machine: the same x86_64 development Mac described above; no lamp access.
  All timing tests here will identify their simulated or host boundaries.
- Questions: none blocking the independent implementation. Any proposed
  mapping or integration that needs a lamp decision will be written here.

## 2026-10-14 — controls — done, awaiting the lamp

Tested code: `fc6f0c07ba507c214099a04bdc7710475198bf9a`, kept on
`third/controls` and fast-forwarded onto `lamp-third`. The implementation and
its docs are one commit. Rebase may change the copy's hash on `lamp-third`;
the task branch identifies the source that passed this gate.

### Built

- Optional `lamp-controls`, disabled unless `--enable`, with one nonblocking
  `PinReader` interface. `serve` deliberately ships with `MissingPins`; First
  replaces that construction with the board reader. Nothing under `deploy/`
  or in Voice/Body was changed.
- Injected-clock mechanical tap/double/hold recognition, V1 per-polarity
  debounce and startup-held suppression; MPR121 qualification/noise cases;
  explicitly configured TTP223 head-pet grouping. Fresh observed time alone
  advances hold feedback. Lost, stale, invalid and reordered input cancels
  recognition and requires a fresh released baseline.
- Additive `controls/state` and `controls/gesture` types, independent retained
  Stop/mute identities, bounded publisher/session fences, diagnostics and an
  owner receiver. Short/repeated taps request Stop; released holds of at least
  two seconds request one-way software mute. Five seconds still means mute.
  Head petting is advisory. No gesture opens capture or clears inhibition.
- An owner-side private, atomically saved boot-scoped mute record and an
  explicit durable clear transaction; shadow messages cannot act. Physical
  privacy remains exclusively with Voice. An accepted request is not an
  applied-state receipt.
- Bounded acquisition recording, written fixtures, replay without hardware,
  real CLI/SIGTERM tests and a replay-cost example. Recording loss inserts
  fault evidence rather than silently joining observations across a gap.

Details, V1 differences, bounds and the consumer contract are in
[Controls](controls.md). In particular, qualified stationary base release
uses 30 ms grace instead of Automatic mode's 120 ms; moving input remains
consumed, with 120 ms grace. First must qualify that choice on real electrodes.

### Evidence on Third's development Mac

Intel Core i7-10700K at 3.80 GHz, 64 GiB RAM, macOS 26.6.2 / Darwin 25.6.0,
`x86_64-apple-darwin`, Rust 1.96.0. No lamp or Linux/ARM64 build was used.
The [evidence record](verification/controls-host-20261014/results.json)
contains source hashes, every final suite summary, the five ignored names,
the corrected test failure and all 100 benchmark iterations.

The unchanged workspace built before implementation (Cargo reported 7.95 s).
The final gate ran from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy --offline -p lamp-actions -p lamp-body -p lamp-controls -p lamp-expression -p lamp-eyes -p lamp-health -p lamp-msgs -p lamp-name -p lamp-policy -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings
cargo test --offline -p lamp-actions -p lamp-body -p lamp-controls -p lamp-expression -p lamp-eyes -p lamp-health -p lamp-msgs -p lamp-name -p lamp-policy -p lamp-voice -p lamp-voice-eval -- --test-threads=1
cargo build --offline --workspace
```

All passed. The final test invocation passed **1,839 tests, zero failures,
five existing ignored tests, across 99 suites**, with no concurrent heavy
build. Clippy reported 19.78 s; the final incremental build reported 1.26 s.
These build times are different cache states, not a performance comparison.
Controls contributes 121 tests; its new message contracts contribute 13.

A source-only copy outside the repository,
`/private/tmp/lamp-controls-standalone-i4bwde4z/lampOS`, built the whole
workspace offline with its own target directory (Cargo reported 1m 14s).
All 24 local packages resolved inside the copy. Controls/message tests there
passed **159 checks, zero failures or ignored tests, across 15 suites**.
The 28 Controls/message source files matched the tested checkout byte for
byte. The copy contained 692 source files and used no parent sources.

One early new-test failure is retained: the nominal polling-phase test
expected a 1,800 us maximum but its original phases produced 1,001 us. The
test's phase coverage and expectation were corrected; the engine was not
changed for that failure. The final 16 first/repeated releases at eight
phases give 0–1,999 simulated us to recognizer dispatch on a 2 ms schedule.
Forty accepted written base pulses at 10 ms polling give 32.45–39.95 simulated
ms from authored release to dispatch, exactly 30 ms after the sampled release.
Neither measure includes electrical, bus, owner, audio or motor delays.

One release-mode invocation repeated the 901-record written replay 100 times:
first 3.340848 ms, median 2.975859 ms, worst 3.340848 ms. The timed boundary is
input parsing, engine construction, injected replay and JSON output to a sink;
configuration parsing happens before it. Each replay has 900 observations,
one explicit overflow, 4,601 ticks, six gestures and zero rejected inputs.
These are host costs, not physical latency, cold startup or an improvement
over V1.

### Exactly what First runs and wires

From the standalone source root with the built binary on `PATH`, one written
replay exercises the gestures without a bus or hardware:

```sh
lamp-controls replay --config crates/controls/fixtures/controls.json --input crates/controls/fixtures/hand-controls.jsonl --clock-id written-controls --enable --output /var/tmp/controls-written-20261014.jsonl
```

Use a new output filename. Expect six gestures: button Stop at 1,160,000 us,
double Stop at 1,410,000 us, base Stop at 1,690,000 us, released-hold mute at
4,000,000 us, head Pet at 5,950,000 us and recovery Stop at 8,580,000 us.
The hold spanning the 6,800,000 us overflow emits no mute, and the moving
base contact emits no action. These are authored times.

First's integration, before enabling any owner:

1. Implement `PinReader` using the actual configured lines/electrodes. Preserve
   acquisition time in `lamp_ipc::monotonic_us()` and the Linux boot ID;
   use a new reader session after restart, consecutive snapshot sequences,
   explicit loss/error reports and fresh complete observations under 100 ms.
   Do not guess fallback pins or read the physical privacy slide here.
2. Voice and Body subscribe to `controls/state` and use `Receiver` to validate
   and deduplicate retained requests; optional pet consumers also subscribe
   to `controls/gesture`. Give each consumer an explicit switch, off by
   default. Stop reaches local cancellation without waiting on the provider.
   Mute revokes permissions immediately; persistence and bounded cleanup
   follow outside the urgent path. Body owns its safe controlled stop.
3. Restore `MuteRecord` before opening Voice workers. Wire a separate explicit
   local software-clear operation, using
   `Receiver::clear_after_explicit_user_request`, and expose its applied-state
   receipt before enabling the long-hold mapping. A successful clear still
   needs Voice's fresh physical-privacy and readiness checks. No automatic
   restart may erase a live inhibit after a persistence failure.

With the adapter installed, record shadow input, retaining the printed counts:

```sh
lamp-controls serve --config /etc/lampos/controls.json --record /var/tmp/controls-real-20261014.jsonl --seconds 60
lamp-controls replay --config /etc/lampos/controls.json --input /var/tmp/controls-real-20261014.jsonl --clock-id "$(cat /proc/sys/kernel/random/boot_id)" --enable --output /var/tmp/controls-real-replay-20261014.jsonl
```

Without First's adapter, that serve command records Missing faults truthfully.
After reviewing the real replay, explicitly enable Controls and the owners.
Measure physical observation to owner revocation, then separately to acoustic
silence and safe motion stop. Exercise first/repeated taps, both hold tiers,
head pats, untouched motor/speaker activity, held boot, stalled provider,
overflow/unplug, Controls death/restart, Voice restart while muted, physical
switch cycles, and explicit clear while the physical switch is closed. No old
reply or capture may resume. Retain owner acknowledgements and audio/motion
evidence; bus publication alone proves none of those outcomes.

**Most likely wrong on the lamp:** base qualification/release timing with its
actual electrode noise and handoffs, then missed GPIO transitions or clock
conversion. Try these first. Head Pet timing deliberately excludes V1's early
directional recognition, and requires a consumer's one-per-minute brain limit.

Questions blocking further Third work: none. Physical integration and the
explicit software-clear owner path are First's prerequisites. A crash before
an unacknowledged mute is durable is not guaranteed recoverable; physical
privacy remains the independent control.

## 2026-10-14 — surround — taken

- Task: 12, starting, setting up and updating a lamp; next in Third's
  fetched priority table (which takes precedence over the older "last"
  sentence in the task body).
- Base: `origin/lamp-voice` at `7d3371dd34a4f67480c44f054cc43890322211ab`,
  after rebasing `lamp-third` and pushing it with a lease. First's Expression
  adoption and interruption-transition fix are now inherited.
- Branch: `third/surround`, from `lamp-third` after this claim.
- Scope: study V1 startup/setup/network/update/recovery/reset and maintained
  alternatives, then implement the independent Rust Surround work that needs
  no lamp, with deterministic clocks, durable operation records, fault tests
  and `docs/surround.md`. No legacy runtime dependency, no deployment edits,
  and no changes to hands-off hardware owners. Changes require explicit
  enablement; owner readiness and safe handover must never be inferred from
  a live PID or a successful command alone.
- Machine: the same x86_64 development Mac; no lamp, credentials, account,
  remote device or production update feed will be used.
- Integration questions for First, not blockers for the independent build:
  which network manager/interface actually owns this lamp's Wi-Fi, and what
  trusted release root/feed will be provisioned? V1 image scripts disagree
  about the network manager. The current owners also need explicit quiet,
  stopped-at-rest and per-store reset acknowledgements before a live update
  or complete reset can be enabled. The document will specify these contracts
  and exercise them with scripted owners; missing evidence must refuse an
  operation, not be replaced with a guess.

## 2026-10-14 — surround — done, awaiting the lamp

Tested code: `4086c0300fa146a7e61f70473a864017a10b22c7`, kept on
`third/surround` and fast-forwarded onto `lamp-third`. The task's source,
documents, dependency provenance and evidence are in that commit. Rebase may
change the copy's hash on `lamp-third`; the task branch retains the tested
source. No hands-off owner, deployment file or task list was changed.

### Built

- Independent `lamp-surround`: strict private configuration, read-only doctor
  and startup planner, authenticated bounded local operator page, a dedicated
  maintenance worker and durable operation records. Maintenance requires both
  configuration `enabled: true` and `serve --enable`. Default operation cannot
  change Wi-Fi, configuration, software, owners or personal records.
- Whole-generation provider configuration, a selected-interface network
  boundary with an optional Linux NetworkManager adapter, fresh setup/runtime
  status and explicit uncertain network commits. The adapter needs its own
  switch and Cargo feature; no network manager or interface is guessed.
- TUF-authenticated complete Voice/Body/Eyes releases, bounded private staging,
  signed metadata/root/time floors, fenced whole-set selection, retained
  previous-set recovery and collection preserving referenced sets. The
  source-pinned Tough library has unchanged runtime source and a documented
  manifest-only TLS feature correction, preserving Voice's Ring provider.
- Journaled update and reset coordinators with injected clocks and private
  owner RPCs. Update requires 45 seconds of quiet, Voice admission closure and
  worker exit, Body rest before exit, fenced selection and fresh capabilities.
  Reset freezes every named store before erasure, preserves privacy inhibits
  and requires explicit resume after interruption. First supplies the actual
  qualified supervisor and store owners; a live PID is never their substitute.
- Self-contained browser assets, signed release fixtures, five installation
  replays and an operator/persistence benchmark. V1 keep/change decisions,
  contracts, bounds and primary sources are in [Surround](surround.md) and
  [reuse decisions](reuse-decisions.md).

### Evidence on Third's development Mac

Intel Core i7-10700K at 3.80 GHz, 64 GiB RAM, macOS 26.6.2 / Darwin 25.6.0,
`x86_64-apple-darwin`, Rust/Cargo 1.96.0 and Node 26.7.0. The unchanged workspace
built before implementation (Cargo reported 15.67 s). All following commands
ran from `lampOS/`; these are host results, not lamp results:

```sh
cargo fmt --all --check
cargo clippy --offline --locked -p lamp-surround --all-targets -- -D warnings
cargo clippy --offline --locked -p lamp-surround --all-features --all-targets -- -D warnings
cargo test --offline --locked -p lamp-surround
cargo test --offline --locked -p lamp-surround --all-features -- --test-threads=1
cargo test --offline --locked -p lamp-gemini -- --test-threads=1
cargo build --offline --locked --workspace
node --check crates/surround/web/app.js
node --test crates/surround/web/app.test.cjs
```

All passed: **260 default-feature and 263 all-feature Surround tests**, with
zero failures or ignored tests; **250 Gemini tests**, zero failures and two
pre-existing ignored long recovery soaks; **nine browser-script tests**.
The additional Gemini run checks the shared TLS dependency change. The whole
workspace built; the whole workspace test suite was not rerun for this new
crate, which has no dependents. Required touched-crate gates passed.

A source-only copy at
`/private/tmp/lamp-surround-standalone-aue7a241/lampOS` built the workspace
with a separate target directory and passed all 263 feature-enabled tests
and nine browser-script tests. It contained 785 source files when copied;
all 26 local packages, including vendored Tough, resolved inside it. Runtime
source hashes matched this checkout. The cold copied build took 303.624 s
wall time with two build jobs. No parent source was used. Local private-socket
tests needed host permission outside the default sandbox; no lamp permission
was used. Linux/ARM64 and Rust 1.91 MSRV were not tested. The pinned non-FIPS
AWS-LC signature dependency requires the target C/C++ compiler; ARM build and
binary size remain First's checks.

The [evidence record](verification/surround-host-20261014/results.json) contains
commands, outputs, hashes, host facts and measurements. The
[failure ledger](verification/surround-host-20261014/failures.md) retains
initial private-directory fixture errors, actual TLS provider and root-rotation
recovery failures, strict-JSON and periodic-freshness bugs, sandbox failures,
review findings and corrected fixture assertions. All final gates are green.
The update/reset suites exercise 66/28 authored power-cut positions; these
are persisted-state replays, not physical power-loss measurements.

Two operator benchmark executions used the real HTTP router and atomic
file/directory sync with a fixture worker. Final first status: 0.114197 ms;
100 warm status calls: median 0.079708 ms, maximum 0.146823 ms. First durable
admission: 41.014442 ms; 100 warm admissions: median 39.003407 ms, maximum
76.701022 ms. During an authored 500 ms minimum dependency delay, 100 status
calls had median 0.092662 ms and maximum 0.241976 ms; the observed delay was
509.585420 ms. All 102 journal readbacks were redacted. This excludes sockets,
browser, actual adapters, supervisor and hardware.

The actual worker has a different measurement boundary, channel send to
durable admission: 331.266 ms in the final focused test, 330.900 ms in an
earlier isolated run and 1605.898 ms under concurrent fsync load. **The
provisional 100 ms complete-service admission target is not established.**
The test proves that stalled idle network probes do not block command handling;
its functional timeout is not a latency claim. HTTP admission uncertainty is
reported after two seconds, while status handling remains independent.

Local Chrome checks passed at desktop and 390×844: login, truthful Unknown
readiness, default-off controls, readable update state and logout. A discovered
`[object Object]` state rendering bug was fixed and regression-tested. This
preview used disposable local configuration, no provider/network/owner and
no production TLS proxy; the tab was closed afterward.

### Exactly what First runs and wires

From the standalone source root, these commands need no provider or hardware:

```sh
cargo run --locked -p lamp-surround --example update_replay
cargo run --locked -p lamp-surround --example operator_bench
```

The replay must report five passing authored scenarios: happy installation,
failed-candidate recovery, missing Body rest refusal, disabled operation and
post-switch power-cut recovery. The benchmark reports its own machine and
boundary; run it on the lamp before attributing any result to that machine.

First provisions the service UID and 0700 roots/0600 files, unique setup code,
canonical TLS origin, real network manager/interface/recovery profile,
qualified provider defaults, independent component units, TUF root/feed and
trusted-time source. The supervisor must implement the exact owner protocol
in [Surround](surround.md), including fenced selection and every personal
store's reset barrier. The release manifest input is a plain filename such
as `stable.json`, never a path. Build the optional adapter with
`--features network-manager`. With the built binary on `PATH`, inspect the
provisioned configuration and run the disabled service first:

```sh
lamp-surround doctor --config /etc/lampos/surround.json
lamp-surround serve --config /etc/lampos/surround.json
```

Check physical privacy in both positions and the absence of invented Listening.
Only qualified integrations get their switches enabled; live maintenance then
requires both configuration enablement and `serve --enable`.

On disposable lamp data, measure page status and admission on first use and
100 repeated operations, separately from completion, with a slow provider
and concurrent release download. Record network path, hashes and percentiles.
Exercise wrong Wi-Fi credentials, lost DHCP/AP/page, retained Ethernet,
NetworkManager restart between profile save and checkpoint completion,
full disk, corrupt staging, expired metadata and stale UTC. Speaking must
restart the 45-second quiet window; no old Voice worker may survive handover;
Body must reach rest before exit. Verify whole-set rollback, failed rollback,
stale capability refusal, duplicate commands, every durable boundary after
power loss and partial reset/resume with software privacy still inhibited.

**Most likely wrong on the lamp:** actual NetworkManager/checkpoint ownership
and AP reachability, then the supervisor's worker-exit, servo-rest and fenced
selection proofs. Try these first. macOS tests and successful D-Bus returns
do not qualify those physical facts. Root/feed provisioning, real AP behavior,
supervisor/store integrations and flash durability remain First's work.

Questions blocking further Third work: none. The manager/interface and trusted
release root/feed questions from the claim remain integration prerequisites,
not assumptions made by this implementation.

## 2026-10-14 — expression-rest — taken

- Task: 18, holding back for someone and a lamp at rest; now the first open
  Third assignment in the freshly fetched priority table.
- Base: `origin/lamp-voice` at `d83834cb2a96f8db72e84e3bbee7c4c6a0de4422`.
  Rebased `lamp-third` and pushed with an explicit lease. First's Controls
  adoption dropped out automatically. Workspace/lock/research additions were
  combined without removing or upgrading any locked package identity;
  Surround runtime/vendor sources remain identical to the tested task branch.
- Post-rebase checks on the same development Mac: workspace build passed
  (Cargo 29.10 s), formatting passed, and the TLS-provider regression passed.
  Offline metadata first lacked newly added `ort`; after fetching the pinned
  public crates, Cargo normalized ordering and one dependency disambiguation.
  No lamp or production service was accessed.
- Branch: `third/expression-rest`, from `lamp-third` after this claim.
- Scope: `crates/expression`, additive `crates/msgs` contracts, replay fixtures
  and English documents. No Voice/Body implementation or deployment changes.
  Study V1's real pause/idle paths, current Voice/Body integration and maintained
  motion/turn-taking sources before extending the catalogue and engine.
- Build explicit held/resumed audio generations, a finish-of-turn nod that may
  continue into thinking, a default non-concentrating state, and bounded quiet
  rest behavior. New motion remains optional and catalogue-controlled; no
  missing or busy room evidence may authorize it. Retired sound stays retired.
  Face response is tested against the 100 ms target using an injected clock;
  First owns actual audio disposal, motor timing and camera judgement.
- First's measured head delay/amplitude in `docs/body.md` informs the written
  nod; it is First's lamp evidence, never a measurement by this machine.
- Questions blocking independent work: none. The most likely lamp limitation
  is how rest looks, and today's Body deliberately ignores expression offsets
  at rest; the handoff will specify that integration without changing Body.

## 2026-10-14 — expression-rest — done on lamp-third

- Task 18; task branch `third/expression-rest`.
- Tested code and documents: `e656c4d998f9868b15b812e3631b991c8ecb90b7`
  (`lampOS: add resumable expression and quiet rest`), fast-forwarded onto
  `lamp-third`. No Voice, Body, other hands-off crate, deployment or task-list
  change. The original Voice message layouts and event variants are intact.
- Added optional `VoiceProgress` / `voice/progress` and `VoiceSound` /
  `voice/sound`. Their positive audio generation and persistent held state
  fence delayed sound, missed holds and resumed turns. Terminal events and
  expired Voice still retire the whole turn. Late activity from a held
  interval cannot reconstruct a nod after resume. This clears planner
  evidence; **Voice must clear its actual audio queue and pending `Piece`.**
- Added catalogue-controlled continuation of an already-started Listening
  pause nod into Thinking, default-false concentration without its old
  missing-input warning, and finite quiet-only rest. No concentration
  publisher is required at startup; an explicit true setting stays true until
  explicitly cleared. Unknown/busy room, stale Voice/joints, input inhibition
  or concentration cancels rest and requires a new continuous eligible dwell.
- The built-in catalogue is unchanged: legacy Voice protocol, original nod,
  no rest. The supplied `rest-behaviours.json` is an explicit `--catalogue`
  selection, sets `resumable_voice=true`, authors the 1.2-second finishing
  nod, and permits a four-second pitch-only rest after 45 eligible seconds,
  followed by 90 seconds after completion/cancellation. All actuator output
  still requires `serve --enable`; final consumers need their own switches.
  Live service selection keeps 12 input subscriptions and the existing
  32-message per-topic drain bound; decoding recognizes both complete pairs.
- Added eight authored replay fixtures, a cost example, 56 Expression tests
  and six message tests. The [contract and lamp checklist](expression.md)
  and [research decisions](reuse-decisions.md#expression-held-turns-and-sparse-rest-2026-10-14)
  record V1 source, current owners, primary alternatives and exact limits.

### What was run, and where

Third's development Mac: Intel Core i7-10700K @ 3.80 GHz, 64 GiB RAM,
x86_64-apple-darwin, macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0.
The pre-task workspace build passed after the rebase. No lamp was accessed.
The final gate, run serially after the source-copy build, was:

```sh
cargo fmt --all --check
cargo clippy --offline --locked -p lamp-actions -p lamp-body -p lamp-controls -p lamp-expression -p lamp-eyes -p lamp-health -p lamp-lean -p lamp-msgs -p lamp-name -p lamp-policy -p lamp-records -p lamp-surround -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings
cargo test --offline --locked -p lamp-actions -p lamp-body -p lamp-controls -p lamp-expression -p lamp-eyes -p lamp-health -p lamp-lean -p lamp-msgs -p lamp-name -p lamp-policy -p lamp-records -p lamp-surround -p lamp-voice -p lamp-voice-eval -- --test-threads=1
cargo build --offline --locked --workspace
```

All passed: **2,353 tests, zero failures, eight inherited ignored cases**
across 137 suites. Metadata confirmed these are all 14 touched/dependent
workspace packages. The ignored names are retained in the
[evidence](verification/expression-rest-host-20261014/results.json): two Lean
cost runs, the Records cost run, the model-backed endpoint case and four
long Voice/reference runs. None of the 62 added tests is ignored. Expression
has 125 passing tests; messages has 44 including its doctest. The serial
full test command took 1,704.159 s wall time; the workspace build took 17.428 s.

The source-only copy at
`/private/tmp/lamp-expression-rest-standalone-g9_pn9ed/lampOS` contained 881
files at copy time, built all 27 workspace members, and resolved all 28
local packages inside the copy, including vendored Tough. It used a separate
target directory and two build jobs. `cargo build --offline --locked --workspace`
passed in 248.258 s; `cargo test --offline --locked -p lamp-expression -p lamp-msgs
-- --test-threads=1` passed all 169 tests with none ignored. Runtime hashes
matched after the gates and benchmark. Final documentation/evidence was added
without changing those sources. No Linux/ARM64 or Rust 1.91 MSRV run is claimed.

The [failure ledger](verification/expression-rest-host-20261014/failures.md)
keeps two demonstrated bugs and their passing regressions: a missed hold
observed after resume incorrectly retiring a turn, and delayed held-period
activity reconstructing a pause. It also retains the initial JSON fixture
comparison and Clippy failures, sandbox CPU/memory inventory failures,
terminal whitespace check, and a stopped superseded gate. That first gate
was stopped during its copied build when the final audit changed two files;
the entire gate above ran again against the corrected frozen source. Tests
used local private Unix sockets and scripted processes with host permission.
No warning suppression, skipped new test or hands-off code change was used.
All 99 checked local documentation links resolved. A log-preparation
bookkeeping assertion initially expected 106 links instead of the 99 found;
the expected count was corrected, with no broken link or runtime change.

One release command ran a first iteration plus 100 warm iterations for each
scenario, without another Third-owned test or build; unrelated host load was
not controlled. The timed boundary is Engine construction, authored JSONL
parsing, injected-clock planning and JSON serialization to a sink. Catalogue
parsing, process startup, transport, recording and hardware are excluded.
The warm median is the upper middle sorted sample, index `warm_runs / 2`.

| Scenario | Inputs / deliberately refused / ticks per run | First iteration | Warm upper median | Warm maximum |
|---|---|---|---|---|
| Held/resumed, 300 ms authored | 18 / 4 / 16 | 0.172494 ms | 0.059406 ms | 0.124608 ms |
| Finishing nod, 1.8 s authored | 37 / 0 / 91 | 0.195416 ms | 0.185896 ms | 0.353597 ms |

All 202 iteration times and the earlier exploratory debug run are retained;
they are not a before/after comparison. Separately, injected arrival→face
request is **18,999 us** for hold (41,001→60,000 us) and **19,999 us** for
resume (120,001→140,000 us). Quiet rest starts at 45 s, completes at 49 s
and starts again at 139 s; busy/focused fixtures have no rest, and expired
room evidence cancels an active rest at 47 s. These are authored clock facts,
not measured optical, acoustic or motor response. A direct built-CLI replay
also returned 18 records, 14 accepted, four intentional refusals and 16 ticks.

### Exactly what First runs and wires

Build the `rest_cost` example in First's usual ARM64 build environment
(`cargo build --release --locked -p lamp-expression --example rest_cost`,
with First's target/toolchain settings). With that built example on the
lamp's `PATH`, the first hardware-free command is:

```sh
rest_cost 100
```

Expect exit 0, the two input/refusal/tick counts above, one first and 100
warm samples each, `physical_or_transport_measurement:false` and
`body_lag_known:false`. Its numbers will be the lamp's only when run there.
The example embeds its catalogue and fixtures; it needs no source tree or
Rust toolchain on the lamp. To inspect requested output with the built
`lamp-expression` binary on `PATH`, from a copy of the standalone source:

```sh
lamp-expression replay --input crates/expression/fixtures/held-resumed.jsonl --clock-id expression-rest-fixture --catalogue crates/expression/rest-behaviours.json --enable --until-us 300000
```

Expect the same 16 ticks and four specifically explained refusals, not zero
refusals. Compare to the retained `held-resumed-plan.jsonl`; substitute the
other seven fixtures with their default duration to inspect each case.
The shipped catalogue still reports unmeasured body lag. A single healthy
status test supplies an authored zero lag; that is not a physical measurement.

First's producer/consumer work, with its own default-off switches:

1. In Voice, discard old queued audio and unpublished `Piece` before advancing
   generation for a hold. Keep `held=true`, Listening and that generation in
   every Latest heartbeat; preserve turn/request and truthful input readiness.
   Resume with active progress in the same generation before newly committed
   matching sound. Never restamp or relabel discarded sound. Another hold
   advances generation again; terminal events stay terminal in heartbeats.
   The [full contract](expression.md#held-turns-the-protocol-first-must-publish)
   specifies reordering, expiry and missed-hold behavior. Until wired, keep
   the built-in catalogue and legacy pair on the lamp.
2. Record hold→resume and hold→give-up with delayed old sound. Measure hold
   receipt→face request separately from actual ring change and acoustic
   silence. Check capture-disabled remains Unavailable, and that a stale
   face target never causes a guessed look. Body currently ignores Expression
   gaze and has its own face tracking; verify the reactive look there.
3. Film the finishing nod with ring, joint feedback and committed audio times.
   Interrupt it with renewed speech and an earlier audio schedule. The +0.08
   rad clip is about 52 requested counts **before** composition with base
   posture/thinking offsets and clamping. First's prior five silent dip trials
   informed its length; their roughly 0.3 s peak delay is not a universal
   onset/acoustic lag. `body_lag_us` remains unknown.
4. Choose exactly one rest author before changing Body's settled-rest rule.
   Body currently ignores Expression at settled rest and has its own optional
   idle clip. Compare the new rest against disabled rest during real desk work,
   with explicit focus, busy/unknown room, input inhibition and expired inputs.
   Preserve local joint safety, privacy, Actions priority and command expiry;
   verify fallback when Expression is killed. No speech may wait for a nod.

**Most likely wrong on the lamp:** how rest looks and how much of the small
slow pitch request survives the servo deadband, followed by composition of
the longer nod with Body's posture and tracking. Only a camera and actual
joint feedback can settle those. Today's settled-rest suppression and the
independent idle author must be resolved before judging the rest plan.

Questions blocking further Third work: none. New Voice publishing, final
consumer wiring, one rest author and physical qualification belong to First.

## 2026-10-14 — expression-rest playout addendum — taken

- The between-task fetch found a new task 18 requirement added by First in
  `e3d32c89f`, while Third's original task gate was running. First reports
  565 refused inputs on lamp-4ace and a 353-message recording across four
  answers: correctly dated approximately 20 ms playout pieces were often
  already finished when read. First's median publication delay was 1.7 ms;
  these are First's measurements, not Third's.
- Complete this added requirement before Schedule: recently finished playout
  is evidence of heard sound, not a fault; follow its bounded recent level
  without requiring advance arrival. Preserve session/turn/generation, hold,
  cancellation, ordering and freshness fences. Any added visible behavior
  remains behind an explicit default-off catalogue option. Do not manufacture
  future audio, acoustic onset or a measured hardware delay.
- Base: `origin/lamp-voice` at
  `23843b78e939a41590c6569f7a65e29bc10a6d03`. Rebased `lamp-third` cleanly
  and pushed with the exact lease; First's adopted Surround commit dropped
  out automatically. Expression/message source hashes still match the
  completed task. First's new Voice/Audio changes require a fresh baseline
  build and the applicable final gate for this addendum.
- Addendum task branch: `third/expression-rest-playout`. Keep the already
  published `third/expression-rest` checkpoint intact; no task branch is
  force-pushed. Scope remains Expression, authored replays/tests and English
  documents. No Voice, Audio, Body or deployment edit.
- Recheck V1's actual post-write level expiry, today's `Piece` publication
  and First's new evidence before designing the bounded observation policy.
  A synthetic 20 ms-piece replay will be labelled authored; no lamp recording
  has been supplied to Third.
- The latest list gives Body to Fifth on its own lamp. First still owns
  lamp-4ace integration; Body changes and the rest/nod qualification now
  follow that ownership split. Surround's first lamp checks are recorded by
  First in `work-split.md`, including that its `/tmp` benchmark used memory
  storage and did not measure flash durability.
- Questions blocking independent work: none. First plans an earlier sound
  publication from Voice's 350 ms buffer; this addendum must also work with
  today's short observed intervals, without editing that publisher.

## 2026-10-14 — task 18 playout addendum done

- Task branch: `third/expression-rest-playout`,
  `584205c9633b7f147d7a33f73213b14ebf57af9a`; based on
  `origin/lamp-voice` `23843b78e939a41590c6569f7a65e29bc10a6d03`, after the
  pushed claim `39043ab933cc6779be57e5387e5d6bc7f86d7e9e`. This completes the
  paragraph First added during the previous task's gate. The earlier
  `third/expression-rest` checkpoint remains intact.
- Added `timing.recent_playout_us`, default **0**, with a nonzero range of
  20,000–200,000 us. The separate `playout-behaviours.json` selects **100,000**
  with today's original Voice protocol and original clips. The built-in and
  `rest-behaviours.json` files are unchanged. The option does not enable the
  planner or any actuator consumer. A custom rest catalogue can explicitly
  combine the option with the new protocol after First wires that producer.
- A just-ended piece can be admitted as heard evidence, and a queued piece
  can become recent when it ends before a tick. Retain one recent level,
  linearly fading from its immutable end, never from receipt or heartbeat.
  Current sound including zero wins over recent; committed future sound can
  still anticipate Body, while reactive fallback samples at real `now`.
  Recent evidence only modulates an owner already Speaking. Hold/resume,
  generation/identity changes, phase edges, terminal events, stale Voice and
  clock faults clear it. All prior ordering, overlap, queue, stamp and header
  checks remain. There is no historical gesture queue or new first-audio time.
- Rechecked V1's actual meter and Voice's `Piece`. V1's RMS timestamp is set
  **before** the write; successful writes have a separate activity timestamp.
  This corrects the shorthand “post-write level expiry” in the claim. Voice's
  piece level is the maximum ring level, not an acoustic measurement. First's
  565 cumulative refusals and separate 353-message/four-answer recording are
  recorded only as First's results. That recording was not supplied here.
- Added **46 tests**: five catalogue, 29 engine, eight authored-replay and four
  transport tests. This includes defaults, exact expiry, missed ticks, current
  zero, future sound, body lag, immutable deadlines, stale headers/payloads,
  hold/resume and missed holds, terminal/identity fences, queue bounds, clock
  failure, shadow mode and local Unix-bus publication. The two fixtures are
  authored, use `recent-playout-fixture`, and contain no lamp recording.
- Gate on Third's Intel i7-10700K 3.80 GHz, 64 GiB, macOS 26.6.2 / Darwin
  25.6.0 x86_64, Rust/Cargo 1.96.0: `cargo fmt --all --check`; strict all-target
  Clippy and serial tests for `lamp-expression`, `lamp-lean`, `lamp-voice`,
  `lamp-voice-eval`; `cargo build --workspace`. Cargo used `--offline --locked`.
  **1,461 passed, zero failed, 7 inherited ignored**.
  No added test is ignored. The refreshed pre-change workspace build passed
  in 15.545791 s; this is a host build duration, not runtime latency.
- A source-only copy at
  `/private/tmp/lamp-expression-playout-standalone-9k1jxhv4/lampOS` contained
  927 files at copy time, built all 27 workspace members, and resolved all
  28 local packages inside the copy. Its Expression gate passed **171 tests,
  zero failed or ignored**. Runtime hashes matched after the gates and
  measurement. No Linux/ARM64 or Rust 1.91 run was made by Third.
- Exact commands, outputs, source hashes, ignored cases and development
  failures are in [the evidence](verification/expression-playout-host-20261014/results.json)
  and [failure ledger](verification/expression-playout-host-20261014/failures.md).
  The before-fix refusal was reproduced with zero passed/one failed, then
  corrected. An initial transport run had two Unix bind denials in the
  sandbox; the unchanged four-test suite passed with host permission. No
  assertion was removed, and no failing test is handed over.

**What was timed, here.** One release invocation of `recent_playout_cost`
runs a first iteration and 100 warm iterations per mode on the same written
15-input/21-tick trace. Strict runs first, then the optional window; this is
one build in two modes, not a historical-binary speed claim. It times the
catalogue clone, Engine construction, JSONL parse, injected planning and
serialization to a sink. Catalogue parsing, process startup, bus, disk and
hardware are outside the boundary. No other Third-owned build or test was
running; unrelated host load was not controlled. Warm median is the average
of the two middle samples. All 202 timings are retained.

| Mode | Accepted / refused per run | First iteration | Warm median | Warm maximum |
|---|---|---|---|---|
| Strict default | 13 / 2 | 0.151605 ms | 0.0514965 ms | 0.057421 ms |
| Optional 100 ms | 15 / 0 | 0.056098 ms | 0.0510775 ms | 0.055327 ms |

The first three authored pieces last 20 ms and are published 1.7 ms after
onset; the two delayed arrivals separately add 20 ms of authored transport.
Each receipt reaches its next planner tick in **18.3 ms**. The optional plan
follows the reported level there; strict mode misses the first at its end
and refuses the two late pieces. This is planned timing on an injected
clock, not lamp latency. A direct built-CLI replay produced 15 accepted,
zero refused and 21 ticks through 400,000 us; its output is retained.

**First: what to run.** Build the example with the usual ARM64 environment
(`cargo build --release --locked -p lamp-expression --example recent_playout_cost`)
and run **`recent_playout_cost`** once on the lamp. The binary embeds its
fixture and catalogue, needs no Rust installation/source checkout there,
and checks the counts above before printing timings. Keep the machine name
and the raw output. For integration, select `playout-behaviours.json` in the
existing Expression invocation, keeping its boot clock and bus root. It works
with today's original Voice topics; do not select the resumable catalogue
until the corresponding producer is in place. Inspect requested output with
actuator consumers disabled, then qualify the chosen consumers as usual.
With the original protocol, terminal cancellation must clear the tail;
testing resumable hold also needs the custom combined catalogue and new producer.

Record real publication, receipt/planner, ring and sound boundaries separately.
Expect short valid pieces to stop increasing the refusal count; a current
zero should stop modulation, and expired/mismatched/retired sound must still
be refused. Confirm hold/cancellation clears the tail, test a stalled planner,
and repeat when Voice publishes from its earlier buffer. First owns Voice and
lamp-4ace integration; Fifth owns Body's physical response. No producer,
actuator, deployment or other builder's branch was changed here.

**Most likely wrong on the lamp:** the 100 ms recent tail feels visibly late,
or a ring level is mistaken for acoustic evidence. The window is a bounded
initial choice, not a measured optimum. First's future publication may allow
it to be reduced; it must not retime ended sound or weaken cancellation.

Questions blocking independent work: none. Next: fetch the list and own log,
rebase `lamp-third`, and take the next unclaimed Third task.

## 2026-10-14 — task 6 taken: Schedule

- Taking `schedule` on `third/schedule` after the completed Expression
  playout addendum. Fetched `origin/lamp-voice` at
  `d0a2b5c00092087763d1e001bfe26fefa7a16dce`, read its running list and this
  log, rebased `lamp-third`, and pushed it with the exact prior-head lease.
  Rebase resolved one append-only `reuse-decisions.md` conflict by retaining
  both First's hand-control review and Third's Expression rest review.
  Expression and Msgs runtime trees are identical to the preceding gated
  handoff. Third's rebased handoff is `3f9644ae1`.
- Scope: new Rust `crates/schedule` / `lamp-schedule`, its own durable store,
  injected clock, Unix JSON-line set/list/cancel interface, retained due
  notices and diagnostics. All service effects require an explicit enable
  switch. No Voice, hardware owner, deployment or other builder's branch
  changes. Propose model tools and delivery acknowledgement in
  `docs/schedule.md`; First owns wiring a spoken turn.
- Read the task and referenced component contract. Preparation inspected
  V1's schedule source/tests and verdict, the current Memory/tools/bus/clock
  boundaries, and primary assistant/time/storage documentation. V1 drops
  work over 30 minutes late; the new task explicitly must not. A bus send
  does not prove durable receipt or that a reminder was heard. Detailed
  keep/change decisions will accompany the implementation.
- First-use and repeated local request, durable-commit, due-to-publication
  and slow-client behavior will be measured separately on this host.
  Injected-clock results will be labelled simulated; no lamp, clock-sync,
  flash-durability or acoustic latency result is claimed here.
- Questions blocking independent work: none. The delivery contract will
  state exactly what First must acknowledge and what happens across a crash,
  busy Voice, clock correction and restart. No speculative speech is added.

## 2026-10-14 — task 6 done: Schedule

- Task branch: `third/schedule`,
  `d78c4ef7b2215697c5a0b4f481f284be3b2422b1`; based on
  `origin/lamp-voice` `d0a2b5c00092087763d1e001bfe26fefa7a16dce`, after the
  pushed claim `ffd111fdee9008ad8871fc47926d32d092d7bcf5`.
- Added `lamp-schedule`, an independent process **off unless `--enable` is
  supplied**. It owns a private SQLite store, a private bounded JSON-line Unix
  socket, retained `schedule/due` metadata and its diagnostics. It opens no
  audio, model or actuator. No Voice, Actions, deployment, work-split or other
  builder's branch was changed.
- Set/list/get/cancel/snooze/exact-delivery Ack use strict typed requests.
  Mutations require store identity, current revision and a caller request ID.
  The last exact mutation and its original receipt survive lost replies and
  restart. Due work is committed before publication, remains pending while
  Voice is unavailable, and is never removed merely because a notice was sent.
  Text stays in the private store/socket; bus reports contain only metadata.
- Duration timers use the elapsed boot clock on the same boot and a stored
  wall-time fallback after reboot. Calendar reminders take human-sized time,
  date and zone fields; daily/weekly recurrences use civil calendar rules.
  Clock trust loss, wall jumps, DST gaps/folds, skipped dates, recurrence
  backlog, late recovery, cancellation and snooze semantics are explicit in
  [schedule.md](schedule.md). One pending occurrence advances only on its exact
  Ack; it does not silently discard missed dates. The bounded store holds 128
  items with no eviction. A failed write/sync stops the owner for recovery.
- Reviewed V1 and primary assistant, Jiff and SQLite sources before building;
  decisions are in the part document and `reuse-decisions.md`. Proposed all
  six model tools and First's Voice contract, including privacy/quiet admission,
  text fetch/revalidation, interruption, durable consumer deduplication and
  qualified completed-playout Ack. There is no claim of exactly-once speech.
  First must register this personal store in the reset manifest before use.
- Added **141 tests**, none ignored: 37 calendar, 22 storage, 36 engine, four
  protocol, 18 runtime, two failure/liveness, ten qualification-client, and 12
  message tests. The liveness cases include full unread stderr, a stalled
  storage owner withdrawing ready/oldest metadata, and an expired queued
  mutation not starting. Other cases cover corrupt files/receipts, write/sync
  failure, lost replies, restart, bounded paging and fragmented/slow clients.
- Gate on Third's **Intel i7-10700K 3.80 GHz, 16 logical CPUs, 64 GiB,
  macOS 26.6.2 / Darwin 25.6.0 x86_64, Rust/Cargo 1.96.0**:
  `cargo fmt --all --check`; strict all-target Clippy and serial tests for all
  15 touched/dependent packages; `cargo build --workspace`, using Cargo
  `--offline --locked`. **2,618 passed, zero failed, nine inherited ignored**.
  The pre-change workspace build passed. The complete package list and commands
  are in [results.json](verification/schedule-host-20261014/results.json).
- The first release qualification failed on this Mac: setting a socket timeout
  after the server closed returned `EINVAL` despite buffered reply bytes. The
  second full run identified the exact stage; a standalone socket reproduction
  confirmed it. Corrected only the qualification example and its Cargo
  feature/test declaration, using nonblocking I/O and cumulative poll deadlines.
  The production service, engine and Msgs source stayed identical to the full
  gate. The [final supplement](verification/schedule-host-20261014/supplement-results.json)
  passed workspace format, all 15 packages' strict all-target Clippy, **129
  Schedule tests with zero failed/ignored**, and the workspace build.
- A source-only copy outside the checkout contained 978 files initially,
  28 workspace members and 29 local packages, all local paths inside the copy.
  It built the whole workspace; after refreshing the two corrected files, that
  build passed again and Schedule + Msgs passed **185 tests, zero failed or
  ignored**. Final source hashes matched after gates and measurement. No
  Linux/ARM64 or Rust 1.91 run was made here.
- Development failures, including the failed full qualifications, sandbox
  socket denials and a corrected large-reply test fixture, are retained in
  [the failure ledger](verification/schedule-host-20261014/failures.md).
  Raw Cargo output retains its original whitespace. No failed test is being
  merged and no assertion was removed to conceal a failure.

**What was measured, here.** One successful release invocation of
`schedule_check --scratch-parent /private/tmp --samples 100`, after the fixes,
completed in 26.364 seconds and retained **510 individual timings**. The scratch
parent and workspace were on the APFS data volume `/dev/disk1s2`, not tmpfs.
Five operations have a first call and 100 warm calls; five more observations
are single runs. First means first operation after its labelled setup, not a
cold process. Set grows the store from one to 101 items; Ack shrinks it; List
reads one page of up to eight items. Warm medians average the middle two values.
No other Third build or test ran concurrently; unrelated host load was not
controlled. The exact samples, boundaries, source and binary hashes are in
[qualification.json](verification/schedule-host-20261014/qualification.json)
and [measurement.json](verification/schedule-host-20261014/measurement.json).

| Operation, milliseconds | First call | Warm median (100) | Warm maximum |
|---|---:|---:|---:|
| Direct durable Set | 75.448850 | 75.2997125 | 94.280184 |
| Direct List first page | 0.194719 | 0.139454 | 0.163894 |
| Direct durable Ack | 76.050170 | 75.2261955 | 96.577391 |
| Unix durable Set roundtrip | 64.582320 | 81.8781535 | 112.466039 |
| Unix Status roundtrip | 5.208467 | 5.9593745 | 14.440621 |

Direct writes include validation, serialization, SQLite commit and directory
sync. Unix Set additionally includes connection, queue and reply read/parse;
fetching its guard is outside the measured call. Single observations were:
fresh Store + Engine open **218.521855 ms**, existing reopen **0.671332 ms**,
one pending commit for all 101 overdue timers **75.228700 ms**, authored clock
jump through scan/commit/publication to bus receipt **136.055890 ms**, and Status
beside a silent connection **7.582396 ms**. These are host observations, not
lamp latency or durability figures. The 250 ms response target is a reference.

The clock and one-hour downtime are authored. All 101 ten-minute timers were
retained 50 minutes late. The binary verified restart identity, exact delivery
Acks and immediate Ack replay, zero pending after Ack/reopen, every runtime
pending page and a reply beside a silent client. It removed its own scratch
directory after shutdown. The host release binaries were 3,894,400 bytes for
Schedule and 4,061,568 for the check; neither binary is committed.

**First: what to run.** Build in the usual ARM64 environment:

```sh
cargo build --release --locked -p lamp-schedule --bin lamp-schedule --example schedule_check
```

Place the example through the usual deployment process. Choose a short absolute
scratch parent on persistent storage, verified with `findmnt -T "$HOME"`; if
home is persistent, run once:

```sh
./schedule_check --scratch-parent "$HOME" --samples 100
```

No Rust installation, source checkout or fixture files are needed on the lamp.
The binary creates only a private `sc-*` child, runs local storage/socket/bus
owners with authored clocks, then removes the child once owners stop. Expect
exit 0, `ok:true`, 101 Set receipts in each path, 101 verified runtime pending
items, and zero pending after direct Ack/reopen. Keep raw output with the lamp
name. It opens no sound or motors and does not touch production Schedule data.

Then qualify the actual service's Linux BOOTTIME/boot identity, synchronization
marker, suspend and reboot, trust loss/correction, real flash recovery, late
recurrences and cancel/snooze. First owns Voice wiring: preserve privacy and
quiet admission; fetch and revalidate the exact token; leave interrupted or
unqualified playout pending; test completed-playout Ack and the consumer crash
window. The document gives request shapes and default-off startup commands.
No lamp was accessed here.

**Most likely wrong on the lamp:** the clock-trust marker and stored wall-time
fallback after reboot, real flash sync/power-loss behavior, or Voice confusing
a bus publication with completed speech. The authored-clock qualification
cannot establish these. Provider tool declarations, personal-data reset wiring,
final speech pacing and timezone-database upgrade recovery remain First's
integration checks.

Questions blocking independent work: none. Next: fetch the list and own log,
rebase `lamp-third`, and take the next unclaimed Third task.

## 2026-10-14 — task 11 taken: Tasks through Harness

- Taking `tasks` on `third/tasks`. Fetched and read `origin/lamp-voice` at
  `fd6705dd9e107817b33309d777cfa2d5c85e4062` and this log. The new list moves
  task 8 (`defaults`) to Fourth after tasks 28/29, so Third will not implement
  it. No defaults source was written. The preparatory finding worth carrying
  over is that Msgs already depends on Bus: a log helper must not create the
  reverse dependency, and a part's request loop must not wait on bus fan-out.
- Rebased `lamp-third` and pushed with the exact previous-head lease. Kept both
  Keeper and Schedule in the workspace and both append-only reuse reviews.
  First's adopted Expression changes dropped out. Schedule and Msgs source
  trees match the gated `third/schedule` checkpoint exactly. The rebased
  Schedule commit is `9ba3aeab1`; its rebased log head is `dfdee9337`.
- Read root `docs/harness.md` and brain-build's chosen CLI route and historical
  Device route. Scope: a new independent Rust `lamp-tasks` process, off unless
  enabled, a private durable task/delivery ledger and local interface, a bounded
  client of Harness's existing CLI, scripted-Harness tests, diagnostics and
  `docs/tasks.md`. No new transport, pairing, account, credentials or raw daemon
  mutations. No Harness operation will be run against a live account here.
- Read-only preparation inspected V1's journal/results/permission handling and
  Harness's public source at `748269145cb76d1a164141f6bc55758e013334b0`; that
  source is being refreshed before finalizing the adapter. The local Harness
  checkout was older. Source review is not installed-release interoperability
  or a real task result. The existing CLI enforces agent-origin policy and
  offers explicit-target launches and bounded observations. Its initial
  creation reply can be lost with no caller-supplied idempotency key; `new`
  will therefore never be automatically retried. Idle state and transcript
  text will not be advertised as correlated completion.
- Host work proceeds with scripted subprocesses and injected clocks. The
  immediate admission/status path must remain responsive while the remote
  operation is slow. Durable admission, CLI startup/observation and voice
  delivery are distinct timing boundaries; each host measurement will identify
  its machine and include retained failures.

**Questions for First / the Harness contract, published here rather than sent
as messages.** None blocks the independent ledger, process runner or fixture
work:

1. Which installed CLI/daemon build on the lamp and selected target supports
   outside-agent identity, explicit `--mode ask`, JSON launch/pending replies
   and remote-child observation? Source capabilities do not qualify 0.3.94.
2. The planned caller is a stable `AI_AGENT=lampos` and private-store
   `HARNESS_AGENT_SESSION`, with unrelated pane/Claude/Codex identity removed
   from the child environment. Confirm the supported reset/account-change rule
   and engine settings; `ask` does not override every engine's own settings.
3. The source reviewed exposes no caller-provided creation key and no
   request-correlated result identity on the chosen CLI route. Is there a
   supported newer contract for either? Until demonstrated, a lost launch reply
   remains uncertain with no automatic resend, and transcript/idle observations
   never mark task completion or automatic final-result speech.
4. What supported chosen-route API supplies focused-agent identity/revision,
   follow-up input, and remote question/permission identity? Until established,
   use explicit provisioned targets, no follow-up send, and generic needs-
   attention observations only. Never answer a permission prompt by voice.
5. How should known pending creations be reconciled after Harness's ledger
   expires or a remote agent disappears? Missing, gone, or a same-name match
   will not prove that nothing ran, or bind a different task.
6. First's Voice adapter must attest the spoken pre-dispatch confirmation and
   qualified playout identity. Which final receipt will it use? Durable speech
   claims will survive restart as uncertain; socket or model acceptance cannot
   mark an answer as spoken. No Voice source is changed in this task.

## 2026-10-14 — task 11 done: Tasks through Harness

Ready for lamp-voice: no

Kind: off unless set. Host gate complete; the newly required native ARM64 CI
gate is pending after the 17:00 fetch and rebase described below.

**Checkpoint:** `third/tasks` at
`3ad6dd152edd3aa453b4f28fc28066e3d9f7471e` (`lampOS: add durable Harness task
tracking`), pushed, then fast-forwarded onto `lamp-third`. This entry and the
implementation are published together on `lamp-third`. The independent client
and scripted qualification are complete; real Harness and Voice qualification
remain First's work. No live task or completed spoken result is claimed.

**Built.** New `crates/tasks` and `lamp-tasks`, disabled unless `--enable` is
set. No existing crate, Voice tool registration, Actions or deployment changed.
The process owns a private SQLite ledger, a public request socket, a separate
trusted Voice socket, bounded workers, and independent health reports. It keeps
the immutable proposed payload and target, the person's exact confirmation,
the one launch attempt, observed output and Voice's played receipt across
restart. Both sockets are private to the service user; another process with
the same UID is inside that trust boundary.

The model chooses only an operator-provisioned alias and bounded text. Targets
require full machine IDs, an explicit project and `codex` or `claude`, always
`--mode ask`. The adapter uses the existing CLI with stable generic caller
identity, an explicit environment allowlist, direct argv, output limits and
process-group cleanup. It runs an exact-ID machine preflight, one `new`, then
only explicitly requested `show` for the accepted agent. It never retries a
launch, guesses a same-name child, answers permissions, or adds a transport,
login, polling loop or credential reader. A timed-out or malformed launch
receipt remains uncertain. Known pending creation IDs remain unresolved where
the owner's current CLI cannot safely reconcile them.

The ledger records `attempting` before granting the single launch. Restart
invalidates unlaunched confirmations and retains attempted work as uncertain.
Every response says completion is unverified: idle state, transcript text and
CLI success do not establish completion of the requested work. Distinct output
observations are deduplicated; a speech claim is granted once, within the
original ten-minute age and Tasks session. A repeated claim cannot replay
speech, including after a Voice-only restart. Only the exact trusted played
ack records reported speech; it is an attestation, not acoustic proof.

The store retains at most 16 tasks and four observations per task, refusing
new work when full instead of evicting uncertain history. It uses private
ownership/modes, no-follow and single-link checks, a lifetime directory lock,
strict schema/invariants, bounded SQLite files and full synchronization.
Uncertain writes poison the owner until reopen. Workers and queues are bounded;
slow CLI work does not hold the request owner. Missing owner progress makes
readiness false. Terminal errors and full stderr cannot indefinitely hold exit.
Exact limits, request shapes, lifecycle and trust boundaries are in
[Tasks](tasks.md); the source review is in [the contract note](tasks-contract.md)
and the append-only [reuse review](reuse-decisions.md).

**Tested here.** Intel Core i7-10700K, reported 3.80 GHz, 16 logical CPUs,
64 GiB RAM; macOS 26.6.2 / Darwin 25.6.0, x86_64. Rust and Cargo 1.96.0 from
the workspace toolchain. Scratch and workspace were APFS on `/dev/disk1s2`,
not tmpfs. These are host results, never lamp results. The pre-task workspace
build passed. Final gate from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy --locked -p lamp-tasks --all-targets -- -D warnings
cargo test --locked -p lamp-tasks -- --test-threads=1
cargo build --locked --workspace
```

All passed: **101 tests, zero failed, zero ignored**. The test command took
50.677 seconds; formatting 3.405, Clippy 3.014 and the workspace build 4.771.
Tasks has no reverse dependents, and no existing crate source changed. Tests
cover confirmation/expiry, CAS/retry behavior, restart uncertainty, once-only
dispatch and speech, malformed/duplicate JSON, target and executable identity,
CLI timeouts/output bounds, cancellation, descendants holding or redirecting
pipes, storage permissions/corruption/limits, stalled owners, blocked stderr
and socket cleanup. A source-only copy outside the repository built all 30
workspace packages offline in 83.977 seconds, then passed the same 101 tests
in 41.929 seconds. All 16 source/manifest hashes match the gated tree.

Failures are retained in the [host evidence ledger](verification/tasks-host-20261014/README.md):
unfinished parallel compile attempts; sandbox socket denials; a real Darwin
zombie/process-group `EPERM` case reproduced outside the sandbox; incomplete
pipe cleanup; partial fixture output; overly short fixture startup budgets;
and two test helpers missing explicit private directory modes. The final
tests pass without weakening production permissions or hiding a regression.
Review also added duplicate-key rejection before CLI connection and a bounded
terminal error writer. No failing test was fast-forwarded.

**Measured here.** One release invocation of
`tasks_check --scratch-parent /private/tmp --samples 8`, after all Third builds
and tests ended, exited 0 in **13.998392566 seconds**. It reported all assertions
passed and removed its scratch. Eight fresh-store cycles produced 72 durations:
one first and seven subsequent samples per boundary. Two additional untimed
scenarios cover malformed scripted JSON and unresolved pending creation across
restart. Ten scripted launches, sixteen observations and ten machine
preflights were made against the checker's own executable, never live Harness.
Unrelated host load was uncontrolled. Each cycle's store is fresh; subsequent
samples do not claim a warmed database.

| Boundary, milliseconds | First | Subsequent median (7) | Subsequent maximum |
|---|---:|---:|---:|
| Fresh service and fixture startup | 220.371124 | 219.478985 | 221.779840 |
| Prepare to durable receipt | 98.459184 | 86.017812 | 87.837577 |
| Confirm to durable queued receipt | 132.784766 | 82.716055 | 86.182955 |
| Confirm to observing stored scripted creation | 309.901767 | 250.180816 | 264.600759 |
| Observe to observing stored scripted output | 306.758039 | 250.705720 | 275.754903 |
| Speech claim to durable grant | 78.916394 | 80.827056 | 84.278012 |
| Authored played ack to durable receipt | 83.405350 | 81.719521 | 84.971112 |
| Status beside one silent client | 7.562604 | 7.539896 | 11.670755 |
| Reopened service start to successful Status | 101.304453 | 105.184206 | 112.867036 |

Round trips include local socket/queue/validation/storage/reply work. Creation
and observation include scripted process execution and polling committed state;
confirmation returns its queued receipt earlier. Speech rows measure authored
database attestations, not sound. The evidence ledger retains all samples,
command boundaries, source/binary hashes and the separate debug preflight.

**First: the first lamp check.** Build in the usual ARM64 environment:

```sh
cargo build --release --locked -p lamp-tasks --bin lamp-tasks --example tasks_check
```

Copy through the authorized workflow, choose a short persistent scratch parent,
then run once (if home is persistent):

```sh
findmnt -T "$HOME"
./tasks_check --scratch-parent "$HOME" --samples 8
```

The checker needs no Rust installation, checkout or fixture files: it launches
itself as scripted Harness. It uses no account, network, sound, motors or
production store. Expect exit 0, `all_assertions_passed:true` and
`scratch_removed:true`. Keep raw output with the lamp name. A failure retains
private scratch evidence. This check does not establish real task delivery.

Then follow [real Harness and Voice qualification](tasks.md#real-harness-and-voice-qualification-still-required):
record installed CLI/daemon/engine versions and permission settings, obtain an
authorized disposable exact target, qualify the resolver limitation below,
then exercise spoken confirmation, lost/malformed/pending replies with no
resend, old and changed observations, needs-input, missing agents/ledgers and
engine restarts. First owns Voice's private confirmation/playout wiring,
privacy, quiet admission, staleness and interruption checks. Qualify Linux
cleanup, boot/suspend, real flash sync and reset/account rollover separately.

**Contract questions for First / Harness.** The current owner source is pinned
at `0e6b16d78fd80697dea252376f19d7acf49d240f`; the relevant CLI files were unchanged
from the earlier claim review. It does not establish installed 0.3.94 behavior.
The six claim questions above remain. Two newly verified gaps are material:

1. The machine resolver falls back from an exact ID to labels/prefixes. The
   `machines --json` preflight and `new` lookup are not atomic; a disappearing
   full-ID target could resolve to a different matching label. Reply checking
   can discover that only after a possible launch. Which supported strict-ID
   route resolves this? Do not enable real delivery until that boundary is
   resolved; a successful preflight alone does not prove it.
2. The CLI can move a pending creation to Started before its lookup, while
   Started lookup does not match the child's creation ID. Which supported
   reconciliation preserves that identity through the transition? Until one
   is available, a pending ID stays unresolved without private-ledger reads or
   guessed name/agent binding.

There is still no caller-provided launch idempotency key or request-correlated
completion/result identity in this chosen route. No retry, automatic final
result announcement, focused-agent follow-up or voice permission approval is
inferred. `ask` preserves the chosen engine's settings; it is not a new
permission sandbox. These are explicit integration boundaries, not omitted
host tests.

**Most likely wrong on the lamp:** installed CLI JSON/caller policy and wrapper
environment, the strict-target/pending-ID contract gaps, Linux process-group
cleanup, or real flash sync exceeding the 1.1-second receipt deadline. Voice
must not turn transcript text into completion or a speech claim into proof
that sound played. No lamp, real Harness, live account, credential, remote
execution, acoustic latency, actual confirmation, power loss or completed
speech was exercised here.

Questions blocking further independent Task 11 work: none. Next: fetch and read
the current list and own log, rebase `lamp-third`, and take any new Third task.
If none is assigned, check again every half hour.

## 2026-10-14, 17:00 — adopt the issue queue and ARM64 gate

Ready for lamp-voice: no

Fetched `origin/lamp-voice` at `462c18b924cd397f04df45f72503362b6a607b60`.
Read the new [joining rules](joining.md), [Third role](../builders/third.md),
[Queue #705](https://github.com/autonomous-ai/lampOS/issues/705), and Third's
issues [#695](https://github.com/autonomous-ai/lampOS/issues/695) and
[#696](https://github.com/autonomous-ai/lampOS/issues/696), including comments.
The queue has no new Third task beyond the already-built Schedule and Tasks.

Rebased `lamp-third` onto that integration head. First's adopted Schedule
source, message module/tests, document and evidence match `third/schedule`
exactly, so its duplicate commit was skipped. Kept upstream Ears membership,
message exports and Memory's reuse review. Tasks' rebased implementation is
`07373490b6662c76c0feac00ea0a62a504d32d8d`; its crate, documents and original
evidence are byte-identical to `third/tasks`. Workspace membership now also
contains the upstream Ears crate. No other builder's source was edited.

The previous Tasks push preceded reading the new CI/ready-line rule. It is
explicitly not ready for adoption until the new Linux ARM64 run is green.
The rebased push starts that run; its number and result will be recorded here.
On this same Intel Mac after rebase, the required local gate passed again:
fmt 3.454 seconds, strict all-target Tasks Clippy 5.959, all 101 Tasks tests
(zero failed/ignored) 54.083, and the full workspace build 28.888. Exact
commands and raw output are retained as `rebase-*` in the
[Tasks evidence](verification/tasks-host-20261014/README.md).
The supplied checker is built by `--examples`, but the current artifact-copy
list includes only `schedule_check` and `replay_cost`, not `tasks_check`.
First will need to include that example in the CI artifact before using it
on the lamp without rebuilding. No workflow source was changed by Third.

First's #695 comment reports Schedule running on lamp-4ace, and a parallel
Mac failure of `two_guarded_mutations_from_the_same_snapshot_do_not_both_win`
(3/4 failed; serial 18/18 passed). Those are First's measurements, not Third's.
The serial handoff gate passed here; this newly reported parallel failure is
being inspected separately and is not erased by that earlier result.

No lamp or live Harness was accessed. The next new task will come from Third's
issue queue; questions stay recorded here and linked from the task issue.

### Schedule's reported parallel test failure: audit evidence

Inspected First's #695 report in a private source copy on this Intel Mac.
All 15 Schedule source/test/manifest files matched the adopted checkout; Bus
and IPC matched too. The earlier copy's workspace and Msgs lack the later Ears
additions, recorded in the source comparison. Only that scratch copy received
instrumentation. No adopted source or runtime deadline was changed.

One full parallel runtime suite passed **18/18**, test harness time 2.30 s.
Its healthy revision-zero guard produced one success, one conflict and one
stored item. First's natural failure was not reproduced. Then one authored
1,250 ms delay immediately before SQLite commit produced two `uncertain`
client replies after 1,111.921 and 1,111.925 ms. The original assertion that the
two `ok` booleans differ failed: **0/1**, exit 101. This is a retained diagnostic
failure in scratch, not a failing test merged into the repository.

Exact replay of the identical requests returned one original success receipt
and one conflict. After service stop/reopen, SQLite still held exactly one
item at revision one and `next_id:2`. The compare-and-swap invariant held.
The artificial sleep actually took 1,258.226 ms; it is not natural storage or
lamp latency. Neither run saw an invalid/null guard. These two runs distinguish
an overstrong test assertion from a proven storage race; they do not identify
the unknown reply bodies in First's original failure.

[The audit](verification/schedule-cas-host-20261014/README.md) retains exact
commands, full replies, source hashes, both raw runs and the audit-only diff.
For First's adopted test: assert the Status used for a guard is valid, retain
both replies, reject two successes, reconcile ambiguous outcomes with bounded
identical-request retries, then require one success/one conflict and exactly
one stored item. Do not increase production deadlines, invent a fresh guard,
or accept two false replies without checking the committed state. First owns
the amendment to the adopted piece; the audit supplies the narrow correction.

## 2026-10-14 — ARM64 gate result and Controls test portability

Ready for lamp-voice: no

Native GitHub Linux ARM64 / Debian 12 run
[38547291775](https://github.com/autonomous-ai/lampOS/actions/runs/38547291775)
tested `5bfab7b17b74fedcd176afc4dd917323ba1826a1`. The full release workspace
build passed. The serial test command stopped in Controls:
`sigterm_finishes_recording_and_releases_the_actual_serve_owner` could not
spawn `/bin/kill` (`ENOENT`, `program.rs:340`). Its binary ran six passing tests
and this one failure; Tasks tests were not reached. Artifact hashing/upload
were skipped, so this run produced no qualified binary artifact. Raw failed
step and run metadata are retained in
[the ARM64 evidence](verification/tasks-arm64-20261014/README.md).

First's #696 comments say it already adopted Tasks as `189b744fa` (from the
original `3ad6dd152`), ran the default-off command on lamp-4ace, and found one
emulated ARM64 adapter expectation differed (`preflight_failed` versus
`preflight_spawn_failed`). Those are First's results. Native ARM64 Tasks tests
must still decide that fixture behavior; no expectation was relaxed here.
The fresh integration head is `7ab11846d81fd4a30971259af3ee2b9b0280ef4e`.

Taking the small test-only Controls portability repair on `third/controls-ci`
as part of finishing the new gate. Controls is Third-owned; replace only the
test's external signal utility with the existing Rust syscall wrapper pattern
already used by Voice, Schedule and Tasks. Keep the actual SIGTERM and shutdown
assertions, production code and deadlines unchanged. Run the gate for Controls
and its dependents and retain the native result before calling it ready.

Found outside this repair: `turn-end-wire/tests/process.rs` and Voice's
`coordinator/tests/endpoint_session.rs` also invoke an external `kill` utility.
Their owners or First must qualify those against the minimal CI image; Third
will not edit those crates. The artifact list still omits `tasks_check`.

## 2026-10-14, 17:51 — CI repair superseded; new Third queue

Ready for lamp-voice: no

Evidence only; no runtime or test change is handed back. Fetched
`f8f0b496cec739c92bfd05386b9470ee5afb766c` and read Queue #705 and Third's
issue comments. First added `procps` to CI in `f1ff6e857` and explicitly said
to leave the Controls test unchanged. Discarded all four uncommitted trial
changes (test, dev dependency, lockfile and Controls doc). The original code
is intact. The trial diff and all measurements are retained as
[superseded experiment evidence](verification/controls-ci-host-20261014/README.md).

That trial's formatting and strict Clippy for Controls/Voice/Voice-eval passed.
All 144 Controls tests passed. The combined gate then failed in unchanged
Voice: 816 passed, 1 failed, 6 inherited ignored, at
`coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`,
`motor/tests.rs:722`. One isolated rerun passed (0.30 s test time); the original
failure remains a failure. Voice-eval independently passed 284 tests and the
workspace built. The fresh standalone trial copy built all 31 packages and
passed 144 Controls tests. Same Intel Mac, Rust 1.96.0 and APFS; no lamp.

Found outside Third's edit scope: the failed Voice fixture checks the original
owner after creating a motor and four peer workers, while `active_reply()`
sets 500 ms capture/admission leases before that setup. The original failure
contains no actual authority payload or setup timing, so this is only a place
for First to inspect, not a proven expiry diagnosis. No Voice source changed.

The new Third queue is #711 (touch strip/head pads), #714 (idle/sleep/waking),
#709 (Voice schedules), then #699 (shared settings/log, returned from Fourth).
Read #711 and both comments, including First's measured masks and the hardware
team's per-unit wiring/calibration corrections. It is the next task to claim.
Tasks #695 and #696 are closed as adopted by First. No lamp was accessed, and
no discarded trial code is marked ready or merged.

## 2026-10-14 — task 33 taken: touch strip and head pads (#711)

Ready for lamp-voice: no

Taking [#711](https://github.com/autonomous-ai/lampOS/issues/711) on
`third/touch`. The issue is labelled `status:taken`; the first claim comment is
this session's (`third-20261014-violet4`, no lamp). Integration base:
`f8f0b496cec739c92bfd05386b9470ee5afb766c`. Read Third's role, the new Queue
order, the creature design, both issue comments, the hardware wiring and
lamp-4ace's preserved MPR121 register dump. The superseded CI test change is
discarded; all runtime source currently matches the integration branch.

Scope: Controls, its message contracts and documents. Add a configurable
MPR121 path with explicit startup configuration, ordered pad positions and
bounded failure behavior; preserve unknown/read/over-current faults as unknown.
Recognize continuous positions, directional slides and strokes, and publish
the new tap/hold intentions for the actual Voice/Expression owners to apply.
Keep standard-unit primary/reset/privacy paths distinct and use per-unit head
discovery/configuration, not two hard-coded pads. Existing lamp behavior must
remain unchanged without an explicit new switch. No Voice, Expression, Body
consumer or deployment is being enabled here.

Before implementation: inspect V1's real MPR121/TTP223 configuration and
failure paths, the chip's primary documentation and maintained Rust options.
Read the existing Controls engine, reader, recorder/replay and receiver
contracts. Keep input and stop handling independent of slow I2C; define
queue/worker bounds and timing targets before choosing the adapter. Tests will
cover startup-held input, partial/lost samples, read/configuration faults,
reversed/slow/interrupted slides, per-unit mapping, repeated holds and replay.

**Integration questions/constraints, carried in the issue too:**

- Third's role owns no deploy folder. Provide the exact per-lamp pins/profile
  candidate and validation command in the handoff; First applies the
  lamp-4ace deployment correction and performs discovery with a person's hand.
  Standard line 100 is reset-only and the privacy input stays authoritative.
- The hardware document describes active-low head pads, while the measured
  lamp-4ace edges include low-to-high touches. Preserve explicit/detected
  per-unit polarity evidence; neither description silently becomes a universal
  hard-coded mapping.
- The supplied 50 Hz mask log contains only changed rows. Preserve those rows
  as First's real evidence. Any filled-in constant-mask samples used for
  deterministic hold/slide tests will be labelled authored reconstruction,
  not newly observed hardware samples. Full acquisition continuity still
  needs the recorder on the lamp.
- Sleep/tap actions must be applied by the current Voice/privacy owner; a
  Controls publication cannot itself prove microphones are off or listening
  has begun. The final handoff will state the exact freshness/receipt boundary.

No question blocks the independent reader/state-machine/replay work. No lamp,
live GPIO, I2C write, microphone, speaker or motor is accessed here.

## 2026-10-14 — native baseline fails before touch implementation

Ready for lamp-voice: no

Found while Task 33 is being built: native ARM64 run `38552800341`, on
`a2b5e127600fd202d823becaf82c96b6ab9e02b2`, built all release binaries/examples
but failed in unchanged Surround's
`interrupted_collection_leaves_only_unaccepted_files_and_retry_removes_them`
at `selection.rs:140`. The test makes a directory mode 0500, then collection
unexpectedly succeeds. CI's default-root Debian container can bypass that
permission; this is the source-based explanation, not a measured UID dump.
[The raw failed step and counts](verification/touch-baseline-arm64-20261014/)
are retained. Task 33 had no source in this run. No test was skipped, weakened,
or merged as passing; later workspace tests did not run.

Question for First, copied to #711: please qualify the CI test process as an
unprivileged user, or choose a deterministic interruption fixture that remains
strict under root. The workflow is outside Third's ownership. The original
Surround test has not been changed. Continue the independent touch build on
`third/touch`; this is a known baseline gate failure, not a ready handoff.

The task's new software path targets GPIO qualification after 20 ms of known
contact, base acquisitions every configured 5–50 ms, and position publication
at most every 20 ms or an actual event. These are authored software budgets;
MPR filtering, calibration, scheduling and Voice/application latency remain
unmeasured. I2C initialization/read runs in one child, with a 7 s initial
deadline and 150 ms later silence deadline; no automatic replacement. Parent
GPIO sampling continues independently. All new behavior requires `--touch`
and commands additionally require `--enable`; neither has been enabled here.

## 2026-10-14 — Task 33 — final gate and revised handoff in progress

Ready for lamp-voice: no

First answered the baseline CI question on #711: `91a0fdb3a` runs native
Ubuntu 22.04 ARM tests as an ordinary user. The earlier Debian-container
root failure is not a reason to weaken Surround's permission test; it stays
unchanged. Fetched through `7afa6876f` at 18:43+07 and rebased this log branch.
The task's code alone is now `third/touch` at
`42bdf65a2144e2168d2643494d8b9e9598404a35`, based directly on that integration
head. Under First's newly adopted `fifth-P6`, this branch keeps the log;
finished code will be offered as one non-draft task PR with green native
ARM64 CI. No code has been moved onto `lamp-third` or offered ready.

The new path is explicitly `--touch`; enabling requests additionally needs
`--enable`. It preserves all twelve electrodes, independent GPIO/strip
acquisition, original times, fault-as-unknown semantics and the installed
primary protocol. Sleep/Wake are separate alternatives bound by the final
owner to lifecycle at contact begin, as First requested at 18:25. Primary
Stop/SoftwareMute and five-second release-only FactoryReset also remain
explicit while asleep. Neither Controls nor a Wake grants microphone access.

Review fixed a child-device ownership hole across different bus roots, queued
packets bypassing a deadline, a malformed head observation retaining a new
primary action, and a strip spawn failure stopping GPIO. A child-held fixed
device lock survives parent failure until the child actually exits; no
replacement starts. A GPIO-open failure now also leaves the strip available,
without a startup stderr write that could block on an unread pipe.

Focused Mac suites passed: 32 coordinator, 22 receiver, 15 message, 38 strip,
16 replay, eight CLI, 12 wiring, 11 runtime and 32 MPR checks; worker/discovery
and recorder checks also passed. These overlap the complete gate and must not
be summed as independent repetitions. The first combined Controls/Msgs run
stopped at 329 passes and one failure: a runtime test still looked for the
old `strip_error` diagnostic key after it was clarified to `last_strip_error`.
The corrected test still requires the actual sanitized startup error and
working primary Stop; its complete 11-case runtime suite then passed. Raw
development failures will accompany the final verification folder.

The gate now runs on the rebased source for Controls/Msgs and every dependent
package (20 total), plus the workspace build. An independent source-only copy
is building outside the parent repository. Its first extraction helper failed
before any compile because this Mac's Python lacks `tarfile.extractall(filter)`;
the host tar utility is used instead, with no runtime/source change. Native
CI and the complete host/portability gates remain pending, so there is no PR
or ready line yet. Third still has no lamp and has accessed none.

On the lamp, First must use the exact sequence in `docs/controls.md`, “Run
without a lamp, then qualify on one”: qualify all twelve electrodes and head
polarity, start untouched with the explicit per-unit profile, record actual
samples in shadow on an isolated bus, test holds/slides/faults and only then
connect owner-side fresh, deduplicated requests. Most likely wrong there are
the profile/noise margin, baseline absorbing a startup hand, weak E4/E8,
TTP223 pulses not proving a sustained hand, paused slide speed, shared I²C
load and service-user lock access. The 57 changed masks supplied by First are
preserved; the additional 995 samples and monotonic identities are authored.
No continuous physical recording, physical latency or applied response has
been measured by Third.

## 2026-10-14 — Task 33 — Linux compile fix and baseline Records failure

Ready for lamp-voice: no

Native run **38559498265** on `42bdf65a` failed before tests: new Linux-only
`strip_worker.rs` needed `let result: io::Result<()>` for its child-loop
closure (E0282). The annotation is fixed locally and will be folded into the
single task commit. It changes no Mac execution; native compilation must
confirm it. No task Linux test or binary artifact passed in that run.

**Found, not mine:** First's corrected-runner baseline **38556357172** built
successfully, then stopped in Records' two JSON atomic-write tests at
`json_file.rs:255` and `:298`, EPERM for the ordinary user. Those match the
previously documented O_NOATIME traversal of ancestors the user does not own.
Counts before stopping: 1,866 passed, two failed, 12 inherited ignored in 126
completed harnesses; remaining tests were not reached. Neither Records nor
the workflow is changed here. The routing request is on
[#711](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6167873860),
with both raw failures in
[touch-native-20261014](verification/touch-native-20261014/README.md).

The independent source-only Mac build passed and its 405 Controls/Msgs checks
passed with no failures/ignores; all 33 local packages including the patched
dependency resolved within that copy. The complete Mac dependent-package
gate continues. No failing test has been removed or weakened, and the task
has no ready PR while the native gate is red.

## 2026-10-14 — Task 33 — final host checks passed; native tests running

Ready for lamp-voice: no

Kind: **off unless set**. The one task commit is now
`721d7c0722320281ff02d0b7d97cd91c3c23e730` on `third/touch`, rebased on
`c14eab1ee`. The Linux child-loop type fix and all development evidence are
folded into it. No task code is on this log branch. `--touch` selects the new
path and `--enable` separately enables requests; the final built binary still
matches the old binary byte for byte without `--touch`, both shadow and
legacy-enabled, including stderr. Exact hashes/commands are in
[touch-final-host-20261014](verification/touch-final-host-20261014/README.md).

The complete Mac gate passed: format, strict all-target Clippy for twenty
packages, 3,213 tests passed with zero failures and fifteen inherited ignores
in 203 harnesses, then the workspace build. The ignored-case list is retained;
it includes timing/extended simulations and one pinned-model case. The run
compiled the original Mac source before the Linux annotation; the later build
includes that annotation. Both source manifests and per-stage notes are kept,
not relabelled as one clean head. ARM64 Linux cross-target Clippy for all
Controls/Msgs targets then passed with the annotation (compile/lint only on
the Mac, not ARM64 execution).

On the exact rebased task head, formatting and all twenty packages' strict
Clippy passed again; Body/Records passed 364 tests (one inherited benchmark
ignored), and all four changed Voice local-time tests passed. The workspace
build passed. A fresh source-only copy built the whole workspace and passed
all 405 Controls/Msgs checks with no failures/ignores; all 33 local manifests
resolve within it. These overlapping runs are not added together. The same
Intel i7-10700K x86_64 development Mac ran all of them; none is a lamp result.

Native ARM64 run [38564855931](https://github.com/autonomous-ai/lampOS/actions/runs/38564855931)
on this exact task head passed its release workspace build; its serial
workspace tests are still running. First announced its own baseline green
(38559471390, `21c24fc0a`) at 19:36, resolving the Records/workflow blocker.
The next periodic fetch reached `5c8e0e6c9`; after the task's base its changes are only
deployment/documentation, and no runtime source or workflow. The Third queue
is unchanged: #711, #714, #709, #699. No task PR or ready-yes handoff yet.

The lamp procedure remains `docs/controls.md`, “Run without a lamp, then
qualify on one”: use the native binary first with no `--touch` for the required
minute; qualify all twelve electrodes and actual head polarity; then record
`serve --touch` in shadow on an isolated bus with per-unit files before enabling
or wiring owner requests. Most likely wrong are the profile/noise margin,
startup baseline absorbing a hand, pulsed TTP223 holds, pause-dependent slide
speed, shared I2C load and service-user lock permissions. Physical timing and
actual acoustic/privacy/motion results remain First's lamp qualification.

## 2026-10-14 — Task 33 — native gate passed; hardware answer adds working pads

Ready for lamp-voice: no

Native ARM64 run [38564855931](https://github.com/autonomous-ai/lampOS/actions/runs/38564855931)
passed on `721d7c0722320281ff02d0b7d97cd91c3c23e730`: release workspace
build, then 3,987 tests passed, zero failed and 29 inherited ignored in 263
harnesses. The machine was GitHub's Ubuntu 22.04.5 ARM64 runner with Rust
1.96.0, not a lamp. [Raw output, hashes and artifact identity](verification/touch-arm64-20261014/README.md)
are kept. No count is added to the overlapping host gates.

At 20:05 First relayed the hardware team's answer on #711: lamp-4ace's
E4/E8 are faulty, sensing through the shell is uneven, and the working set
and each pad's thresholds must be per-unit settings. Its head pads are
37/96 active-high; other units must discover their own two. The periodic
fetch reached `888be65e8` and the Third queue is unchanged.

The explicit twelve-value touch/release arrays already preserve per-pad
settings, but the task's all-twelve geometry restriction needs changing.
The task remains on `third/touch`, with no PR or ready-yes handoff while
that change is built. Known excluded electrodes will be omitted from contact
topology while physical strip positions retain their full span; excluded-only
bits cannot create or hold an action. Actual read errors and unexplained
gaps remain unknown. The original all-twelve replay remains a comparison,
not a lamp-4ace calibration. Each working electrode still needs calibration
and qualification on the particular lamp; no threshold is invented as a
measurement here.

The new tests, gate and exact-head native run will follow before the PR.
No lamp was accessed. #714 has only been read for dependencies, not claimed
or implemented; its unchanged Expression baseline was saved in scratch.

## 2026-10-14 — Task 33 — working pads built; replay timeout corrected

Ready for lamp-voice: no

The task is now `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca` on
`third/touch`, rebased on `a9992f272`. The per-unit working set accepts three
to twelve electrodes; sparse orders retain the full strip span across faulty
slots. Only explicitly excluded individual OOR bits are ignored; whole-chip,
selected-pad, configuration and I/O failures still invalidate the strip. Raw
acquisitions retain all twelve bits; excluded levels cannot hold a gesture.
Each electrode's thresholds remain explicit, and the unequal example values
are authored rather than a calibration. The complete-twelve and lamp-4ace
ten-pad reconstructed replays are byte-identical.

The first fresh source-only build passed, but its Controls/Msgs test run on
the development Mac stopped at 320 passed and one failure: the output-file
CLI test exceeded its five-second deadline. The partial output and full raw
stream are retained in the task's
[touch-output-host-20261014](https://github.com/autonomous-ai/lampOS/tree/08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca/lampOS/docs/verification/touch-output-host-20261014).
Inspection found unbuffered writes in the new replay output-file path. It now
uses an 8 KiB buffer and still checks the final flush. The unchanged deadline,
private mode and overwrite rejection remain, and the test also compares the
complete file bytes and summary to stdout. The focused 26-test CLI/replay
rerun passed; no failure was hidden or disabled.

Formatting and strict twenty-package Clippy passed on the corrected source,
as did strict ARM64 Linux cross-target Clippy for all Controls/Msgs targets
(compile/lint only on the Intel development Mac). The obsolete full host run
and native run 38571800855 were interrupted/cancelled after the source fix;
neither is a passing gate. Fresh upstream Eyes, Policy, Records and Voice
changes were then picked up before restarting the complete gate.

The final source-only build/tests and full twenty-package serial host gate
are running; host test suites are serialized. Native ARM64 run
[38573575450](https://github.com/autonomous-ai/lampOS/actions/runs/38573575450)
is running on the exact task head. No PR or ready-yes handoff until all pass.
The switch remains `--touch`, with `--enable` separately authorizing requests.
The required lamp commands and likely physical failures remain in
`docs/controls.md`, “Run without a lamp, then qualify on one”: qualify each
working pad's thresholds through its shell, discover that unit's head lines,
cross missing pads, test real fault isolation and final-owner privacy/actions.

The 20:35 queue read shows #714 moved to Second. It was only inspected here,
with no claim or source changes. The remaining Third queue is #711, #709,
#699. Read-only Schedule integration preparation continues while touch gates
run. No lamp, provider account or actuator was accessed.

## 2026-10-14 — Task 33 — final native green; inherited host timeout retained

Ready for lamp-voice: no

The exact task head remains `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`.
Native ARM64 [run 38573575450](https://github.com/autonomous-ai/lampOS/actions/runs/38573575450)
passed: release workspace build, 4,054 tests passed, zero failed and 29
inherited ignored in 264 harnesses. The Controls binary SHA-256 is
`37924f2bfea364a214243e7b935a85ca1e8302167393c34590530a2908fc18f1`.
This was GitHub's Ubuntu 22.04 ARM64 runner with Rust 1.96.0, not a lamp.

**Found, not mine:** the full serial development-Mac gate stopped in the
unchanged Voice motor cleanup test at `coordinator/motor/tests.rs:441`,
`owned child timed out: None`: 2,804 passed, one failed, fifteen inherited
ignored in 142 harnesses, 1,819.333908 seconds. All completed earlier harnesses passed;
later harnesses and the workspace build were not reached. The complete raw
failure and diagnostic reruns are in
[touch-host-timeout-20261014](verification/touch-host-timeout-20261014/README.md).

The test has a two-second wait while production startup permits three seconds.
That is a possible timing sensitivity, not an established cause: the failure
has no child-mode/startup trace. A focused unchanged Cargo rerun passed, then
three serial runs of the exact failed library binary all passed, without
source/deadline/assertion changes. No leftover matching test process was seen
in the subsequent process check. The full twenty-package serial test command
is running once more on the same unchanged source; no task PR until it and
the workspace build pass. The earlier failure will remain in the handoff.

The fresh source-only workspace build and all 446 Controls/Msgs checks passed,
zero failed/ignored. Its final binary is still byte-identical to the original
legacy replay without `--touch`, in shadow and enabled modes. Per-unit pad
qualification, physical timing and applied Voice/privacy remain First's tests.

The 21:04 queue read is unchanged for Third: #711, #709, #699; #714 belongs
to Second. Fetched integration `8be0b9dd2` adds opt-in Voice first-output
logging, with no Controls/Msgs, dependency or workflow change. This log branch
is rebased on it. The tested task head stays fixed; its handoff installs only
the Controls artifact, not an older Voice. Schedule preparation remains
read-only and unclaimed. No lamp or provider account was accessed.

## 2026-10-14 — Task 33 ready — touch strip and head pads (#728)

Ready for lamp-voice: yes (#728)

Pull request: [#728](https://github.com/autonomous-ai/lampOS/pull/728), one task commit
`08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`, based on `a9992f272`. Kind: **off unless set**.
The task branch is `third/touch`; this branch contains only the log/evidence.
The tested task head stayed fixed while later unrelated Voice changes arrived.
The log is rebased on `34f1319d5`; install only this task's Controls binary,
keeping the newer Voice. No deployment file or physical device was changed.

The selected `--touch` path independently reads configured head/button GPIO
and a supervised MPR121 child, initializes the chip, qualifies touch/hold/slide
gestures, and publishes fresh typed evidence and separate Sleep/Wake choices.
Requests also require `--enable`. The final owner selects the action from the
contact-start lifecycle and its current speech/privacy state; publication is
not an acknowledgement that capture, sound, light or movement changed.
Working-electrode lists preserve the physical strip span, per-pad thresholds
are explicit, and excluded levels cannot prolong a contact. Whole-chip,
selected-pad, timing and I/O faults invalidate the strip. The fixed device
claim remains held until the old child actually exits. Head discovery and
bounded private record/replay tools are included. Standard primary/reset paths
remain configurable; First removes the absent primary in lamp-4ace's profile.

**Gate on the development Mac:** Intel i7-10700K at 3.80 GHz, 64 GiB,
x86_64 macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0. Workspace formatting,
strict all-target Clippy for all twenty touched/dependent packages, their
complete serial tests, and the workspace build passed: **3,283
passed, zero failed, 15 inherited ignored in
204 harnesses**. Exact commands, source hashes, elapsed times,
ignored cases and unedited output are in
[the final host evidence](verification/touch-complete-host-20261014/README.md).
Formatting/Clippy passed in the earlier invocation; tests/build are the later
complete unchanged rerun. They are not described as one invocation.

The earlier Mac invocation failed an inherited Voice motor cleanup timeout
after 2,804 passed, one failed and fifteen ignored. Its cause was not established.
One unchanged focused Cargo rerun and three executions of the exact failed
binary passed, then the complete rerun above passed. The failure, diagnostic
reruns and source review remain retained. No Voice/Body source, assertion,
deadline or filter was changed. The earlier fresh-copy replay-file timeout is
also retained with its corrected buffered-output rerun; no failure was hidden.

The final source-only copy built the workspace with all 33 local manifests
inside the copy and passed all **446 Controls/Msgs checks**, zero failed or
ignored, 29 harnesses. Registry/git caches were reused. The final binary's
legacy replay stdout and stderr are byte-identical to the pre-task binary
without `--touch`, in shadow and legacy-enabled modes. Strict ARM64 Linux
cross-target Clippy also passed on the Mac; this was compilation/lint only.

**Native gate:** [38573575450](https://github.com/autonomous-ai/lampOS/actions/runs/38573575450),
exact task head, GitHub Ubuntu 22.04 ARM64, Rust 1.96.0, ordinary user, **not a
lamp**. Release workspace build and **4,054 passed, zero failed, 29 inherited
ignored, 264 harnesses**. Counts overlap host checks and are not added.
[Raw native evidence](verification/touch-complete-arm64-20261014/README.md)
also preserves the obsolete cancelled run as cancelled, not passing.

**First's lamp sequence:** download artifact
`lampos-arm64-08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca` from run 38573575450 (artifact ID 11900607240,
expires 2026-10-21 14:05:30 UTC). The `lamp-controls` SHA-256 is
`37924f2bfea364a214243e7b935a85ca1e8302167393c34590530a2908fc18f1`.
Keep rollback and run that binary for the required minute without `--touch`.
Then use `docs/controls.md`, “Run without a lamp, then qualify on one”:

1. Stop the previous GPIO/I2C owner, establish the unit's working electrodes,
   and calibrate every working pad's touch/release thresholds through the
   assembled shell. The ten-pad example thresholds are authored, not calibration.
   Check that excluded pads do not cause global auto-configuration faults.
2. Start hands-away with `lamp-controls discover-head --seconds 20`; touch and
   release each physical pad twice. Use the observed candidates/polarity and
   omit lamp-4ace's absent primary in the unit profile.
3. Run `lamp-controls serve --touch --config <unit-controls.json> --pins
   <unit-touch-pins.json> --bus-root <isolated-bus> --seconds 60 --record
   <new-touch.jsonl>` first in shadow, then with `--enable` on that isolated bus.
   Check recording/child-cleanup counters. Use a new record path each time.
4. Tap, hold, slide both ways, reverse, pause, lift, hover, cross the missing
   electrodes and start with a hand present. Check missing-chip, lock and stale
   acquisition behavior while GPIO remains usable. Replay the complete record
   with its recorded clock ID and exact configuration. Measure first/repeated
   physical contact-to-response using a separate physical reference.
5. Wire the final consumers and qualify actual Stop, microphone closure,
   asleep touch Wake, and no Sleep from the same still-held waking hand. Keep
   physical privacy authoritative; qualify light and motor response separately.

**Most likely wrong there:** threshold/near-hand margins, startup baseline,
excluded-pad global chip faults, an unobserved narrow finger over a dead pad,
pulsed head holds, slide speed around pauses, I2C contention, lock permissions,
GPIO polarity and final-owner acoustic/privacy/motion latency. Host and native
software tests establish none of those physical results. Next Third task is
#709, then #699; #714 remains Second's.

## 2026-10-14 — Task 31 taken — Schedule tools and due speech (#709)

Ready for lamp-voice: no

Taken by Third, session `third-20261014-violet4`, no lamp. The current Queue
puts #709 next after touch PR #728, then #699. The
[first claim](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6171323467)
was reread and is this session's. The task branch will be
`third/schedule-voice`, directly from `origin/lamp-voice` at `34f1319d5`;
`third/schedule` is the earlier standalone Schedule task and is not reused.
This claim is pushed before code changes.

Scope is the six Schedule tools in Actions, their declaration/routing in
Voice only with exact `LAMP_SCHEDULE_TOOLS=on`, and the single durable due
consumer only with exact `LAMP_SCHEDULE_SPEAK=on`. Both absent/malformed/off
states must preserve the existing catalog, wire messages and no-consumer
startup behavior. The task explicitly permits these narrow Actions/Voice
changes. No Gemini, audio, Body, deployment or unrelated Voice refactor.

Read-only preparation inspected V1's reminder delegation, the existing
Schedule engine/private protocol/Store, real Voice ownership and final-playout
paths, and official Gemini function-schema documentation. Reuse the current
Schedule service and separately tagged Store for a delivery ledger, rather
than replacing the scheduler or treating a bus heartbeat as a speech command.
Storage/IPC stays in one supervised optional child for the whole Voice service
lifetime. The urgent capture/hand/privacy path polls bounded messages without
waiting for storage or Schedule. Prepared identity must be durable before
speech, and whole-playout evidence plus the exact Ack request must be durable
before acknowledgement. Unknown/interrupted outcomes stay pending.

Latency targets to verify on this development Mac: healthy local tool request
through Actions to a confirmed durable Schedule result within 250 ms; one
absolute 1,250 ms adapter deadline across all socket hops; no storage/socket
wait added to the urgent Voice loop, whose bounded optional poll targets 2 ms.
These exclude model latency, OS scheduling guarantees and physical sound.
Measure first/repeated local work and stalled dependencies; First qualifies
real storage, model schema acceptance and acoustic behavior on lamp-4ace.

**Question for First on #709:** `RoomTold` starts Unknown and becomes Quiet
only after five minutes. Should a due reminder wait for explicit Quiet after
a provider/privacy restart, or may Unknown allow it after fresh local silence
and normal readiness, while known Busy always vetoes it? Strict Quiet-only
would delay a cold-start one-minute timer. Until answered, the dependent
Unknown-room admission decision remains unqualified; the bounded client,
Actions tools, ledger and completion/cancellation tests can proceed.

Tests will use a fake Schedule socket through the real Actions/Voice paths,
real coordinator and provider fixtures for busy room/pending speech/privacy,
multiple PCM generations, interruptions, cancellation/snooze races, child/disk
stalls, late callbacks, lost Ack replies and restart. Exact Ack replay includes
its original revision guard; a new guard is a new persisted CAS attempt after
same-delivery reconciliation. A Get-not-found without store identity cannot
prove absence; use an identity-bearing current response. Keep all unresolved
ledger entries when full and defer new speech rather than evicting evidence.

No implementation, provider request, lamp run or new measurement is claimed
at this taken checkpoint. Touch PR #728 remains under review; its native and
ordinary repository CI checks are green, and no review comment was present
at this queue read.


## 2026-10-14 — Task 31 host gate failures retained; startup rebase

Ready for lamp-voice: no

The complete serial host gate on `3773c1fb` failed with 1,111 passed,
one failed and eight ignored: the inherited motor
`frozen_config_and_executable_are_exact_and_source_changes_are_not_adopted`
waited two seconds for its inert child and timed out. The exact focused test
and all 28 motor tests then passed unchanged; that did not qualify the full
gate. Source and raw results remain retained for the task handoff.

The next complete invocation, on `d5d765862`, failed with **1,111 passed,
two failed and eight ignored**, 16 completed harnesses. The failures are
`crash_and_stalled_control_never_allow_replacement_or_false_cleanup` and
`failed_reap_after_stopped_is_latched_and_reported_once`, both the same
inherited motor helper at line 441, `owned child timed out: None`.
Formatting and strict all-target Clippy passed; the workspace build was not
reached. [Unedited evidence](verification/schedule-voice-d5-host-failure-20261014/README.md)
records exact commands, all 675 source hashes, counts and the development Mac.
No lamp result is claimed and no task code has been offered as ready.

**Found, not mine:** these motor fixtures and their production path are
unchanged by Task 31. The cause is not established. No test was weakened or
filtered from a complete gate. First's newly fetched `7fbb03e1f` fixes an
own-socket bind/chmod startup race in IPC/Voice transport; the task is now
rebased onto that integration, preserving every dirty task file byte for byte.
That is a reason to verify the new source, not evidence that it fixes these
failures. A clean baseline, diagnostics and a new complete gate are next.

The optional consumer's Linux child now explicitly sets and reads back
`SCHED_OTHER`, static priority 0 and nice 19 before paths or storage, so it
cannot silently inherit Voice's audio scheduler. Its exact module and Linux
test compile and pass strict Clippy for ARM64 from this Mac; Linux execution
and the lamp's privileged RR20 transition remain unqualified. The parent
policy remains untouched. Both integration switches remain off unless set.

The queue/issue read at approximately 16:48 UTC is unchanged: #709 remains
taken by this session; #699 is next and unclaimed. The Unknown-room policy
question has no answer, so the implementation conservatively waits for
explicit Quiet. The resulting cold-start timer delay is documented.


## 2026-10-15 — Task 33 rebased and ready again (#728)

Ready for lamp-voice: yes (#728)

The requested rebase is complete: `third/touch` now holds
`a7c0073da9b2314ba123efee8a25bc80ce01b211`, based on `d5d765862`.
The only manual conflict was the appended reuse notes; both Touch and Mood
sections survive. Controls source is byte-identical to the previous task
head; Msgs adds the upstream Mood module and registration. Kind remains
**off unless set**: `--touch` selects the new path, with requests separately
requiring `--enable`. No deployment or device access by Third.

[The new evidence](verification/touch-review-20261014/README.md) preserves
exact commands, source hashes and unedited output. On the development Intel
x86_64 Mac with Rust/Cargo 1.96.0, format, strict all-target Clippy for all
21 affected/dependent packages, 446 Controls/Msgs tests (zero failed or
ignored, 29 harnesses), and the workspace build pass. The new binary's whole
legacy replay stdout and stderr match the preserved pre-task bytes in both
shadow and legacy-enabled modes; the legacy input blobs also match.

The exact-head native ARM64 [run 38599296426](https://github.com/autonomous-ai/lampOS/actions/runs/38599296426)
is green: Ubuntu 22.04 ARM64, ordinary user, Rust 1.96.0; release workspace
binaries/examples build and serial workspace tests. Standard summaries report
**4,192 passed, zero failed and 31 ignored across 276 harnesses**, plus
**12 passed, zero failed** in the separate turn-end-wire process runner.
The native workspace run supplies all dependent-crate tests; the host and
native counts overlap. The native workflow does not run Clippy. It reports
two inherited Cargo `replay_cost` output-name collisions in build, repeated
in tests, and no Rust source diagnostic warning. Raw warnings are retained.

**First's artifact:** `lampos-arm64-a7c0073da9b2314ba123efee8a25bc80ce01b211`,
ID `11914551344`, expires **2026-10-21 17:01:30 UTC**. Install only its
Controls binary, keeping the newer Voice. Controls SHA-256:
`8c7b8a1a6c3437853d616ae01244042057800a5a6bbc540006d78ce730b56ceb`.
The evidence README gives the hash check and minute without `--touch`, using
verified legacy configuration/pins and the installed legacy enable choice.
Then follow `docs/controls.md` for per-pad qualification and hand tests.

First's earlier lamp-4ace minute, reported in the PR, applies to old Git head
`08a32c9d8` and old Controls binary SHA `37924f2bfea364a...`; it is not a
measurement of the new artifact. First found base E4/E8 faulty and requires
per-pad thresholds. Tap, hold, slide and enabled final-owner behavior still
need hands at the lamp. Most likely wrong there remain per-pad noise and
startup baseline, global chip faults from excluded pads, pulsed head holds,
blind strip positions, I2C contention/lock permissions, and actual Stop,
Sleep/Wake, privacy and physical response latency.

Task 31 remains taken and not ready while its complete host gate is being
diagnosed. The clean integration checkout also reproduces its inherited
motor fixture timeouts; the independent final Schedule source-copy checks
pass (280 tests and 72 default-off cases). Those focused results do not
replace the complete gate. Task #699 remains next and unclaimed.

## 2026-10-15 — Task 31 inherited motor gate failure; follow-up ordering

Ready for lamp-voice: no

The complete serial host gate on base `dcc2e6b1e` passed formatting and
strict all-target Clippy for Actions, Schedule, Voice and Voice-eval, then
failed: **1,111 passed, 3 failed, 8 ignored** across 16 completed harnesses,
658.218369088 seconds. Voice lib had 918 passed / 3 failed / 8 ignored in
491.25 seconds. The workspace-build stage was not reached. All 675 recorded
Rust/manifest hashes stayed unchanged. The machine remains Third's Intel
i7-10700K x86_64 Mac, macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0.

[The retained evidence](verification/schedule-voice-dcc-host-failure-20261015/README.md)
includes exact commands, unedited failed logs and source maps. All three
failures use the inherited two-second helper at `motor/tests.rs:441`,
`owned child timed out: None`:

- `crash_and_stalled_control_never_allow_replacement_or_false_cleanup`, wait at 645;
- `late_started_callback_cannot_restore_an_offer_after_owner_replacement`, wait at 865;
- `native_head_initial_privacy_acquisition_is_not_a_revoke_or_grant`, wait at `native_head.rs:491`.

**Found, not mine:** a clean `7fbb03e1f` source copy without Task 31 also
failed three motor fixtures. Separately instrumented exact cases and one
28-test motor module passed, with the original deadlines/assertions. They
did not reproduce a failing phase, and do not establish a loader cause or
qualify a gate. No production motor file, assertion or deadline was changed.
No task commit is offered for merge and no result is attributed to a lamp.

**Question for First:** assign the inherited motor-fixture failure to its
owner, or authorize Third to investigate and repair that boundary in a
separate scoped task. The present #709 scope permits tool declaration and
due speech, not the motor supervisor. A repair must establish the failing
boundary and preserve truthful startup/reap evidence; a timeout increase or
passing narrow retry alone does not resolve the retained failures.

Independent Task 31 work continues. The dcc rebase preserves the incoming
short-sound rule and prevents silent reminders from creating room activity
or renewing a session as unanswered speech. Its first reminder-filtered run
passed 62 tests, failed none and ignored two separately measured timing
probes. Review then found a real ordering bug in the new path: a reminder
could displace an owed camera follow-up or occupy the existing second-look
deadline. That is being corrected with sequence regressions; it is not a
claimed fix for the inherited motor timeouts. The newer task-announcement
code at `bc3fccb54` must be incorporated before the next source freeze.

At the approximately 17:38 UTC Queue read, #709 is first and #711 is second;
#699 has moved after the MVP and remains unclaimed. The Unknown-room policy
question has no answer, so reminders continue to wait for explicit Quiet.
That can delay a cold-start one-minute timer; the delay is documented.

Touch review was answered in
[PR #728](https://github.com/autonomous-ai/lampOS/pull/728#issuecomment-6174368155):
the current `a7c0073da` head is mergeable and its exact ARM64 run is green;
the renewed conflict report named the superseded head. The broad defaults
sweep #734 was closed as a misunderstanding. Its clarification and the new
joining rule assign existing switches' default flips to First after lamp
qualification. The qualified Touch artifact and legacy-path check remain
available for First's minute and enabled hand tests; Third claims no new
lamp result and makes no default flip before that qualification.

## Not shown

No lamp was accessed. No new Voice publisher or actuator consumer was run on
the lamp; no optical/acoustic latency, safe physical trajectory or
natural-looking behavior was established by Third. Native CI results apply
only to the explicitly identified runs above. No Controls
pin reader, owner subscription, applied mute/clear or hand-to-silence timing
was run on a lamp. Replay fixtures are written; First supplied 57 changed
touch masks, not a complete continuous touch recording. Surround has not changed real Wi-Fi, started physical owners,
installed a production signed release or erased a real personal store. Its
production supervisor, trusted feed, lamp storage durability and full-service
latency remain unqualified. Schedule and Tasks passed their host gates; their
real storage, clock, Harness and Voice integrations remain for First to qualify
on the lamp. Task 33 is offered in #728; no hardware qualification is claimed.
Task #709 is taken and not ready; #699 waits until after the MVP.
