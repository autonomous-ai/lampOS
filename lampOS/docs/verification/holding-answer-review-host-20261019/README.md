# Holding-answer review guards

Historical host verification of the #1688 follow-up at prior PR head
`471c8fe8736a7b7effa39e1174069181c21727f1`, based on `lamp-voice`
`c862c13bf9095b05be1499dd2c409d9d2223878c`. This ran on Apple ARM64 with the
repository's pinned Rust 1.96.0. Original source hashes, commands and test
output summaries remain unchanged in `gates.json`.

The follow-up was subsequently rebased onto
`d9ba8d0f2a5070569fef3225a0924cdcdc005aba`, preserving both the holding-answer
argument helper and #1713's Messages arguments. The results below belong to
the prior head. Rebased validation is recorded separately below, including
one diagnosed inherited host fixture failure. Native ARM64 CI of the new
head must pass before it is ready.

## Behavior and regression evidence

- A whole reply of “let me see” is no longer a holding answer. It can invite
  someone to show an object. Worker and matcher tests cover its exclusion;
  overflow, failure and generation tests use an eligible holding phrase so
  their original exclusions remain exercised.
- The coordinator uses the tested worker-argument helper. Only the exact
  value `LAMP_HOLDING_ANSWER=on` forwards the flag, and parsing preserves
  following worker flags. The default remains off.
- A production coordinator-loop fixture retires the holding reply through
  local speaker IPC, sends a capture gap 500 ms later, reopens capture with
  a new epoch and supplies at least another second of fresh quiet. It checks
  the real gap-cancellation trace and observes no queued or delivered
  continuation. The same fixture without the gap queues and delivers exactly
  one. The fixture never calls the continuation's clear method directly.

## Prior local gate

Commands run from `lampOS/`, with four build jobs, incremental compilation
and dev/test debug information disabled, and one external build cache.
Local socket/process fixtures are permitted. Existing ignored tests remain
ignored. Counts are copied from command output and include doc tests.

- `cargo fmt --all --check`: passed.
- `cargo clippy -p lamp-voice --all-targets -- -D warnings`: passed.
- `cargo test -p lamp-voice -- --test-threads=1`: **2230 passed, 0 failed,
  12 ignored**, none filtered out.
- `cargo build --workspace`: passed.

The names changed were searched across `crates/`; their changed uses are
private to Voice. Cargo metadata additionally identifies nine direct or
transitive reverse consumers, including test dependencies. All nine passed:

- `cargo clippy -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-skills -p lamp-update -p lamp-voice-eval --all-targets -- -D warnings`: passed.
- `cargo test -p lamp-body -p lamp-cloud -p lamp-expression -p lamp-keeper -p lamp-lean -p lamp-memory -p lamp-skills -p lamp-update -p lamp-voice-eval -- --test-threads=1`: passed.

| Crate | Passed | Failed | Ignored |
|---|---:|---:|---:|
| lamp-body | 454 | 0 | 0 |
| lamp-cloud | 36 | 0 | 0 |
| lamp-expression | 269 | 0 | 0 |
| lamp-keeper | 24 | 0 | 0 |
| lamp-lean | 61 | 0 | 2 |
| lamp-memory | 209 | 0 | 5 |
| lamp-skills | 88 | 0 | 0 |
| lamp-update | 121 | 0 | 1 |
| lamp-voice | 2230 | 0 | 12 |
| lamp-voice-eval | 539 | 0 | 0 |
| **Total** | **4031** | **0** | **20** |

No test was filtered out. The same four Rust source hashes were verified
before recording this combined result; no source changed between stages.

## Rebased validation

The first source-only attempt stopped in an inherited installer packaging
test: `lamp-update/tests/image_journal.rs` reads the parent repository's
`robots/lamp/rootfs` journald drop-in. Its output was **1,224 passed, 1 failed,
8 ignored** before stopping, preserved in [that attempt](source-only-attempt.json).
The final validation runs all Update tests in the checkout with that image
fixture, and all nine other complete Voice/reverse-consumer crate suites
plus the workspace build in the source-only copy. No test is filtered or
newly ignored for the fixture. Whole-workspace standalone test portability
therefore remains limited by this existing packaging check.

The next source-only run reached **3,911 passed, 1 failed, 19 ignored**,
then stopped at the inherited relay-clock deadline test. The unchanged test
reproduced intermittently in isolation on this host. A scratch diagnostic
located the wait at the second child probe, after signal acknowledgment and
the next ping, while relay clock reads continued. Further diagnostics caught
the host shell returning `prob` after the driver wrote `probe\n`; the
fixture discarded the truncated command. The test and relay implementation
are unchanged from the integration base. This is
[reported to Tests](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6259400709).
The failed command remains
in [its own record](relay-clock-attempt.json); it is not counted as a
successful gate.

The completed rebased host gate covers every selected crate and its doc
tests: **4,094 passed, 1 failed, 20 ignored**, none filtered out. The one
failure is the unchanged relay-clock fixture above. Formatting, strict
all-target Clippy for all ten packages and the source-only workspace build
passed. Every unrun whole target was completed after the failure, rather
than repeating the whole gate to obtain a clean pass.

| Crate | Passed | Failed | Ignored |
|---|---:|---:|---:|
| lamp-body | 454 | 0 | 0 |
| lamp-cloud | 47 | 0 | 0 |
| lamp-expression | 269 | 0 | 0 |
| lamp-keeper | 24 | 0 | 0 |
| lamp-lean | 61 | 0 | 2 |
| lamp-memory | 209 | 0 | 5 |
| lamp-skills | 114 | 0 | 0 |
| lamp-update | 122 | 0 | 1 |
| lamp-voice | 2256 | 0 | 12 |
| lamp-voice-eval | 538 | 1 | 0 |
| **Total** | **4094** | **1** | **20** |

The [rebased record](gates-rebased.json) preserves all seven command stages,
every test-output summary, the four changed source hashes, the complete
source-copy digest and the unchanged flaky-test/relay hashes. All source
hashes matched before and after validation. The seven remaining Voice-eval
targets passed **61/0/0**, all selected doc-test harnesses passed with zero
tests, and the complete Update suite passed **122/0/1**.

Under [the known timed-process host-flake rule](../../joining.md#one-pull-request-per-task-fifth-p6-decided-2026-10-14),
this host failure stays visible and requires green native Linux ARM64 on the
published PR head before readiness. No source fix, added ignore or test-name
filter was used to hide it. The native result belongs in the PR body.

## Limits

The loop fixture uses local IPC and a stepped clock. It does not establish
real-provider delivery, acoustic latency or a physical lamp result. No
cloud conversation or device was used. Native Linux ARM64 CI must pass on
the published PR head before the PR is ready; its run belongs in the PR body.
The switch remains off pending the existing lamp qualification.
