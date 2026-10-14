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

## Not shown

No lamp was accessed. No new Voice publisher or actuator consumer was run on
the lamp; no Linux/ARM64 execution, optical/acoustic latency, safe physical
trajectory or natural-looking behavior was established by Third. The replay
fixture is written; a real lamp recording has not been supplied. Remaining
Third tasks are not yet built.
