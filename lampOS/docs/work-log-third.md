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

## Not shown

No lamp was accessed. No new Voice publisher or actuator consumer was run on
the lamp; no Linux/ARM64 execution, optical/acoustic latency, safe physical
trajectory or natural-looking behavior was established by Third. No Controls
pin reader, owner subscription, applied mute/clear or hand-to-silence timing
was run on a lamp. Replay fixtures are written; real lamp recordings have not
been supplied. Surround has not changed real Wi-Fi, started physical owners,
installed a production signed release or erased a real personal store. Its
production supervisor, trusted feed, lamp storage durability and full-service
latency remain unqualified. Schedule and Tasks passed their host gates; their
real storage, clock, Harness and Voice integrations remain for First to qualify
on the lamp. No unclaimed Third task remained in the list read for Task 11;
the next fetch determines the next assignment.
