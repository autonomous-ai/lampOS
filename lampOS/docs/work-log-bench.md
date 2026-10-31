# Bench work log

## 31 October 2026: offline execution and complete comparison (#4745)

Ready for lamp-voice: no

Code is pushed on `bench/memory-bench` at
`7683361f5693f50d33b91024f04fa392dfe99a92`, in
[PR #4753](https://github.com/autonomous-ai/lampOS/pull/4753). It remains a draft
because the assigned live baseline and integration evidence do not exist.
The separate approved design and test plan are on `bench/memory-design` at
`9260ad9d5e25b499a8fafc87d024fd379c49a5df`, in
[PR #4752](https://github.com/autonomous-ai/lampOS/pull/4752). The latest plan
rulings need refreshed reads and exact-head checks before merge.

Classification: a standalone host evaluation executable, off unless explicitly
invoked. It is an ordinary default binary target so native CI builds and lints
it. No product crate links it; no product or deployment code was changed.
No lamp is assigned to this builder.

The benchmark has the fictional month, 200 frozen questions, realistic timing,
shared scripted acknowledgements, real-speech cache plumbing, three reference
arms, the versioned adapter contract, twelve registered Tiers setups, isolated
probes, one budgeted model client, blind multi-turn judging and a complete
comparison card. Approved campaign settings pin models, prices, fixtures,
audio, executables and scope. The exclusive journal reserves spending before
dispatch and retains uncertain charges across failure. Keys come only from
the environment and are not passed to adapters.

Validation on macOS, Rust 1.96.0, from `lampOS/`:

- `cargo fmt --all --check`: pass.
- `cargo test -q -p lamp-memory-bench`: 156 passed, zero failed.
- `cargo clippy -q -p lamp-memory-bench --all-targets -- -D warnings`: pass.
- `cargo build -q --workspace`: pass.
- The full fake HTTP/WebSocket path completed a shared speech cache, 200
  probes, 224 answer turns and 76 blind-judge checks. Every accuracy mean is
  null. Corrupted cache and adapter evidence remain visible as invalid slots.
- The worker-completion resource race was reproduced before its join fix.
  Other regressions cover privacy/cache invalidation, source eligibility,
  byte caps, malformed provider frames, retained spending and probe restoration.
- Build profiles used `CARGO_PROFILE_DEV_DEBUG=0`,
  `CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_INCREMENTAL=0`. The shared build slot
  was released after preserving the runner and cleaning 3.5 GiB of output.

The preserved macOS runner's SHA-256 is
`3d86bd998a1bacd1f5717b08347f2bec503a075d362b1212072e417c250985d0`.
The local artifact is under `.harness/bench-session/bin/`; it is not committed.
Exact-head ARM64 run 41229787298 and Linux clippy run 41229787297 are pending;
local checks are not native Linux or device evidence.

The earlier fifteen-setup recorded month used runner `6420a2f49` and Tiers
`7d6d0a236`. Its immutable ledgers are committed at `04a88bdf4`, including
retained prior interrupted attempts. They establish plumbing only. The
separate aging structure does not establish fading: live nonempty detail,
low importance and lack of recent recall still need inspection.

Needs: the owner's model/key choice and spending approval, development human
audit, the independent held-back fixture, the complete current-product hook
in #4748, exact-head native gates, reads and docs merge. The two-month Flash
planning estimate is $573.99 with allowances and 25% contingency; aging adds
$29.66 and stays outside the active plan until the proposed $610 extension is
answered. Optional outside systems remain unpriced and excluded. No real
service call, numerical memory result or device measurement has occurred.

Most likely wrong here: full current-product parity is still unproven. The
partial adapter omits voice observation and summary queues and must never be
called the complete baseline. The common in-turn forget restart is stronger
than today's product; that gap stays beside all comparisons and was reported
on the Queue without editing product code.
