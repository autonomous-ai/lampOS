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

Handoff check state, 31 October 2026 at 18:38 UTC:

- Code head `7683361f5693f50d33b91024f04fa392dfe99a92`:
  [Linux clippy passed](https://github.com/autonomous-ai/lampOS/actions/runs/41229787297);
  [ARM64 build and tests are running](https://github.com/autonomous-ai/lampOS/actions/runs/41229787298).
  PR #4753 still needs exact-head ARM64 completion, independent reads, fixture
  review, and the live acceptance evidence. It stays a draft.
- Docs head `9260ad9d5e25b499a8fafc87d024fd379c49a5df`:
  [Linux clippy passed](https://github.com/autonomous-ai/lampOS/actions/runs/41229932006);
  [ARM64 build and tests are running](https://github.com/autonomous-ai/lampOS/actions/runs/41229929143).
  PR #4752 needs refreshed reads of this head, successful ARM64 completion and
  merge by the reviewer or first builder. Earlier reads at `707d79445` do not
  cover these changes. The docs checks were dispatched explicitly because
  prose-only changes do not trigger them automatically.

These are observations at handoff, not promises about the final CI result.
No device or acoustic measurement has been made.

### Preserved pins

Paths below are relative to this session's existing worktree. SHA-256 checks
were repeated during handoff and match the saved manifests. Binaries remain
local and are not committed. Do not replace a pin midway through a pass.

| Artifact | Revision | Local path | SHA-256 |
|---|---|---|---|
| Current runner, fake-service gates complete | `7683361f5693f50d33b91024f04fa392dfe99a92` | `.harness/bench-session/bin/lamp-memory-bench-7683361f5` | `3d86bd998a1bacd1f5717b08347f2bec503a075d362b1212072e417c250985d0` |
| Runner used for the complete recorded pass | `6420a2f490263df664a12c8f8848c95421c30abd` | `.harness/bench-session/bin/lamp-memory-bench-6420a2f49` | `c7e93b0cedd75a0211bdb714f77f51fb62d5232359cfa08c096de8935886edac` |
| Tiers adapter used for every candidate in that pass | `7d6d0a2364bc2052c323327334633a97450b12b3` | `.harness/bench-session/adapter-topics-7d6d0a236/lamp-memory-tiers-arm` | `ad0eef3379b5753bd01ce3df7aa29db998c1f366739e59a7d60cb005432d5f97` |

The current runner's manifest is
`.harness/bench-session/comparison-7683361f5-pin.json`. The adapter manifest is
beside its executable as `artifact.json`. The recorded pass is under
`.harness/topic-pass-6420a2f49-7d6d0a236/`; keep its ledgers, interrupted
attempts and evidence. Local validation logs are
`.harness/bench-session/comparison-{fmt,tests,clippy,workspace}.log`.

The earlier fifteen-setup recorded month used runner `6420a2f49` and Tiers
`7d6d0a236`. Its immutable ledgers are committed at `04a88bdf4`, including
retained prior interrupted attempts. They establish plumbing only. The
separate aging structure does not establish fading: live nonempty detail,
low importance and lack of recent recall still need inspection.

### Three prerequisites for real calls

1. The owner's model/key choice and spending approval. Keys are loaded from
   the environment only; their presence is not approval. The approved scope,
   exact models, prices, fixtures and executables must match the immutable
   policy hash and cap before a campaign can start.
2. lampOS review of the development fixture, frozen questions, source evidence
   and rubrics. The held-back repetition also needs an independently written
   and reviewed second persona. No candidate answer has been used to revise
   these registrations.
3. The complete current-product clock and drain-and-snapshot hook in
   [#4748](https://github.com/autonomous-ai/lampOS/issues/4748), followed by
   integration checks. The current adapter is explicitly partial current
   product and cannot fulfill the complete baseline acceptance check.

The Lead has these decisions and dependencies already; no duplicate approval
request is needed. Finish the native gates and independent reads noted above
before treating either PR as ready. The two-month Flash
planning estimate is $573.99 with allowances and 25% contingency; aging adds
$29.66 and stays outside the active plan until the proposed $610 extension is
answered. Optional outside systems remain unpriced and excluded. No real
service call, numerical memory result or device measurement has occurred.

After those prerequisites are satisfied, resume using the
[campaign instructions at the code pin](https://github.com/autonomous-ai/lampOS/blob/7683361f5693f50d33b91024f04fa392dfe99a92/lampOS/crates/memory-bench/README.md):
validate and approve the policy, create one campaign journal, build each
persona's speech cache once, then run the registered order with three attempts
per setup. Preserve failed attempts and spending reservations. The comparison
card requires verified complete live evidence; recorded and fake-service
results must remain plumbing checks with null scores.

### Most likely wrong here

Full current-product parity is still unproven. The
partial adapter omits voice observation and summary queues and must never be
called the complete baseline. The common in-turn forget restart is stronger
than today's product; that gap stays beside all comparisons and was reported
on the Queue without editing product code.

Fake services prove transport and accounting behavior, not compatibility with
every real provider response or the quality of the frozen judge. The fixture
and rubric review must check that the scored questions actually demand the
intended memory skills. The month does not exercise fading, a second known
resident, or the lamp's own generated replies and promises. Missing or unused
tier diagnostics mean unknown or not exercised, never no value.

The Lead requested this handoff only, then a stop. No build slot or local run
is held. All further project work and paid calls await reassignment and the
prerequisites above.
