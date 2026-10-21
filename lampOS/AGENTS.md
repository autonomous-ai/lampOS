# lampOS development rules

Treat this directory as the root of a standalone repository. Follow the
[V2 live interaction scope](docs/architecture.md).

A session that opens with "you are <name>" is a builder's: read [joining.md](docs/joining.md) as it is on `origin/lamp-voice` and follow its "Starting a session".

For the three-agent effort, follow [agent coordination](coordination/README.md).
Read assignments and device reservations from the fetched integration branch.
Use the built-in Harness Agent Messages (`harness send`) for communication
between Main and the two agents. The owner enabled this route on 12 October
2026, superseding tmux delivery. Publish code checkpoints and durable decisions
on your own work branch; Git is not a message queue. Never paste peer reports
into the owner-facing composer.
The owner does not relay routine agent reports.

- First release: chat with a physical robot beside one person at a desk.
  Lamp is fixed on the left or right; calls and visiting colleagues are negative
  and mixed-attention test cases.
- Implement Lamp-owned runtime code in Rust. Do not introduce Python services,
  scripts or adapters into lampOS, or require Python HAL for the release.
  Native DSP/inference dependencies require explicit interfaces and review.
- Reuse tested protocol semantics, calibration data, safety rules and regression
  fixtures. Do not copy the old callback ownership or shared execution model.
  Confirm units and behavior on actual hardware before accepting a port.
- Before designing a new subsystem, inspect the corresponding V1 behavior and
  current maintained alternatives. Record keep/change/reuse decisions and their
  evidence in [reuse decisions](docs/reuse-decisions.md). Reuse suitable Rust
  libraries; rewriting runtime policy does not require reimplementing every
  dependency. Compare failure behavior and measured workloads before replacement.
- Owner instruction: V1 main is the behavioral baseline for the rewrite.
  Before debugging a replacement, inspect V1's actual code, defaults and
  tests for that path. Port useful safeguards, prove the corresponding
  behavior in Rust, then compare one improvement at a time. Record which
  protections are present, missing or deliberately changed; do not treat
  a language rewrite or passing unit tests as behavioral parity. See the
  [voice parity plan](docs/v1-voice-parity.md).
- Keep capture and interruption independent of cloud, vision inference and
  presentation. Bound queues, buffers, worker counts and retry work.
- Keep the interaction core small. Heavy inference runs in independent supervised
  processes. Optional camera, light or sensor failure must not terminate healthy
  voice. Inhibit failed outputs before bounded teardown, verify old-worker exit
  before replacement, and retain truthful readiness and cleanup evidence.
- One interaction owner coordinates voice, head, body and ring. Validate
  ownership, freshness, privacy and cancellation at the final output boundary.
  Keep exactly one writer for each actuator bus.
- Enforce motor range, velocity and cancellation locally. Gemini may propose
  intent; it cannot bypass safety or directly set unchecked joint targets.
  A frame showing the actual placement and clearance is required before motion.
- Stillness and silence are valid. No automatic room scans, stock spoken
  fillers, mandatory gestures, fabricated visual facts or forced eye contact.
- Camera and microphone privacy controls remain authoritative. Never show
  listening readiness unless input can actually be retained.
- Keep Harness, agent sessions, browsing and new long-term memory infrastructure
  out of this release. Preserve only the future task and shared-context boundaries
  in the architecture; do not implement them or make live chat depend on them.
- Owner instruction: all code comments, documentation, and notes are English
  only. Keep documentation in this directory, normally `docs/`. This explicit
  instruction overrides any inherited translation requirements.
- Use English by default for all user-facing communication, including progress
  updates, questions, and final responses. Switch to Vietnamese or another
  language only when the user explicitly requests it; do not infer a language
  switch solely from the language of their message.
- Keep the build, tests, configuration, startup, deployment tooling and runtime
  self-contained. No required source files, path dependencies, configuration
  files or running services from sibling directories. Ordinary external Rust
  crates, Linux drivers, model assets and cloud APIs remain explicit dependencies.
- This is a full runtime replacement. Do not ship a bridge, hidden fallback, or
  permanent dependency on the legacy implementation. A clean installation must
  run while legacy HAL and os-server are stopped. Keep the old system only as
  a temporary validation/rollback option; do not delete it before V2 qualifies.
- Own all required setup, dependency declarations, model asset management,
  configuration, driver setup and future UI/update components inside lampOS.
  Build and deployment must work with this directory as the repository root.
- Implement every responsibility required by the V2 runtime in Rust, including
  functionality currently split across Python HAL and Go coordination. A
  dependency on the legacy HAL or os-server is not a standalone implementation.
- Prove portability by building and testing a source-only copy outside the
  parent repository. Keep credentials and per-device calibration separate from
  source; document provisioning without embedding secrets.
- With Rust implementation changes, run formatting, clippy and applicable tests.
  Replays and unit tests do not establish acoustic latency or motor safety.
  Report real-device measurements separately, with exact boundaries.
- Keep command output small: long build and test logs use up an agent's
  usage limit. Build and lint quietly (`cargo build -q`,
  `cargo clippy -q --message-format=short`), and test with
  `cargo nextest run --status-level fail --final-status-level fail` or
  `cargo test -q … | tail -40`, with `set -o pipefail` so the pipe keeps the
  test's failure. Send a whole log to a file and read only its result lines
  (`grep -E '^test result|FAILED|panicked'`). On a lamp, read `journalctl`
  through `grep` or `tail`, never a whole log.
- Do not commit, push or deploy without the authorization required by the
  repository and current conversation. For a lampOS builder that authorization
  is standing for its own branches (`<name>/*`, `lamp-<name>`): it commits,
  pushes and opens its pull request once its gate passes, as `joining.md` and
  the root `AGENTS.md` say, and never pushes to `lamp-voice` or `main`.
