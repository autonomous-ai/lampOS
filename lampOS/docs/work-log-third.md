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

## Not shown

No lamp was accessed. No new Voice publisher or actuator consumer was run on
the lamp; no Linux/ARM64 execution, optical/acoustic latency, safe physical
trajectory or natural-looking behavior was established by Third. No Controls
pin reader, owner subscription, applied mute/clear or hand-to-silence timing
was run on a lamp. Replay fixtures are written; real lamp recordings have not
been supplied. Remaining Third tasks are not yet built.
