# lampOS

A standalone Rust runtime for natural conversation with a physical Lamp beside
one person at a desk.

**Status, 2026-10-11: six varied Rust conversation turns completed on lamp-4ace,
with no extra admissions or cancellations. Four playback gaps remain. The
conservative baseline suppresses overlapping input, so natural interruption is
not solved. Camera context is integrated but native teardown needs correction;
motor choreography and continuous service are not yet integrated. Full social
and embodied interaction qualification remains.**

Lamp-4ace is reachable again and the approved iMac recording/playback setup
works with the screen locked. The installed legacy runtime remains the
stationary rollback. The owner deferred V1 benchmarking; current work is V2
implementation and real-device acceptance. Read the
[handoff](HANDOFF.md) and [current integration status](coordination/main.md) for
exact source, evidence, reusable assets and unfinished work.

See [benchmark progress](docs/benchmark-progress.md) for the retained trials and current blockers.

Treat this directory as the repository root. It must build, test, install and
run after extraction, without source files or services from sibling projects.
All code comments, documentation and notes are English. Read the
[project rules](AGENTS.md), [release scope](docs/architecture.md), and
[hardware port contract](docs/hardware-contract.md), and
[deferred V1 comparison protocol](docs/comparison-protocol.md).
The [reuse decisions](docs/reuse-decisions.md) record what we preserve from V1,
what we borrow from ROS 2 and which existing Rust projects need evaluation.

Startup, configuration, supervision, hardware control and interaction logic
belong here in Rust. Keep dependency declarations, model asset management,
board setup, installation and any future UI/update components inside this
project. External Rust crates, Linux drivers and Gemini remain declared
infrastructure dependencies. Provision credentials and per-device calibration
explicitly; never embed secrets or silently depend on a legacy configuration.

The first release is live chat with coordinated voice, head, body and light,
using camera and sensor observations. Agentic tasks, new long-term memory,
legacy web UI and fleet management are outside that release scope. The existing
implementation remains available for validation rollback until V2 qualifies.
The [architecture](docs/architecture.md#future-task-and-memory-boundaries)
reserves future Harness task and shared-memory boundaries without making them
runtime dependencies. Suggested team assignments are in
[team ownership](docs/team-ownership.md).

## Build and check

Use the pinned Rust toolchain in `rust-toolchain.toml`. Linux audio builds also
need ALSA development headers and pkg-config. macOS runs the portable core and
fixture tooling. Linux hardware modules require separate Linux/ARM64 checks
and real-Lamp qualification; a passing Mac suite does not exercise those paths.

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

## Current components

| Crate | Responsibility |
|---|---|
| `lamp-turn-owner` | Immutable turn/output ownership, cancellation, privacy lineage, readiness and observation freshness. |
| `lamp-ipc` | Private bounded Unix datagrams, shared monotonic timestamps and a separate-process control latency probe. |
| `lamp-audio` | Pure Rust Sonora AEC3/noise processing in 10 ms blocks; privacy-checked Linux ALSA capture and guarded speaker output. |
| `lamp-ring` | Fixed-size WS2812 encoding, bounded brightness and ownership-checked Linux SPI writes. |
| `lamp-camera` | Bounded frame lifecycle, privacy, latest-frame delivery and Linux V4L2 backend; opt-in live context integrated, native teardown correction and physical qualification pending. |
| `lamp-body` | Five-servo protocol, per-unit calibration, guarded motion core and Linux port; inactive in live interaction, with worker integration and physical qualification pending. |
| `lamp-environment` | Typed latest sensor values, per-field freshness, partial readiness, faults and worker identity; I/O-free contract, drivers/integration pending. |
| `lamp-sound-fixtures` | Fixed desk scenarios, cached voices/noise mixes and asset integrity checks. |
| `lamp-gemini` | Bounded Gemini Live WebSocket transport, explicit activity, immutable response ownership and cancellation barriers. |
| `lamp-voice` | Finite directed qualification with separate capture, speaker, physical privacy and provider processes, optional ring and camera workers; continuous service and complete failure isolation remain in progress. |
| `lamp-observer` | Mac room recording and native microphone permission preflight; explicit cached iMac-speaker playback with delivery/failure evidence. |
| `lamp-voice-eval` | Event-triggered conversation acceptance runner and evaluator: pre-registered scenarios, append-only attempt ledger, fake runtime, trace import and a prepared physical runner. See [voice evaluation](docs/voice-eval.md). |
| `lamp-station` | A production test station's record of one run, passes included, and its verdict, from the lamp's self-test, the station's answers and the versioned coverage matrix (`production/`). Runs on the station, never on a lamp. See [production testing](docs/production-testing.md). |
| `lamp-peek` | A read-only page on an office lamp, off unless `LAMP_PEEK=on`: one page with a sidebar (Overview, Turns, Camera, Body, Health), each turn's timings from the voice's own records, the eyes' faces as boxes, never pictures, the joints against their goals, each part's health, and the bus's states and counts, never words. On 127.0.0.1 unless given a token, or `http://lamp-xxxx.local/` on an office lamp's drop-in; never in `deploy/product`. See [peek](docs/peek.md). |

See the [directed runtime](docs/live-runtime.md),
[conversation/ring ownership](docs/ring-choreography.md),
[input admission](docs/input-admission.md),
[room observer](docs/room-observer.md), [motor contract](docs/motor-runtime.md),
[environmental snapshots](docs/environment-snapshots.md), and
[native component qualification](docs/component-qualification.md) for
frozen-source checks and measurements. Component tests do not establish
end-to-end conversation, acoustic latency,
addressee accuracy, physical privacy controls or motor safety.

## Reusable qualification tools

```sh
cargo run -p lamp-sound-fixtures -- render .cache/acoustic .cache/render-first.json
cargo run -p lamp-sound-fixtures -- verify .cache/acoustic
cargo run --release -p lamp-ipc --bin lamp-ipc-probe -- --samples 1000
cargo run --release -p lamp-audio -- bench-aec 6000
cargo run -p lamp-voice-eval -- fake-run --out NEW_RUN_DIRECTORY
```

Keep `.cache/acoustic/` when cleaning build output. Rendering uses installed Mac
voices and reuses verified audio; it does not require cloud TTS or an LLM.
Use a new report path for every render. The IPC probe measures control roundtrip
between processes with a separate saturated bulk queue. The audio benchmark
measures synthetic DSP processing and signal integrity. The voice-eval fake run
simulates the directed turn policy against declared stimuli. None of these is an
acoustic speech-to-answer measurement or a completed live Lamp test.
