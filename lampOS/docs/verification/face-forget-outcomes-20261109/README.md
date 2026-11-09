# Face-forget cleanup outcomes — 9 November 2026

Third's separate follow-up to the adapter P2 in #5803, assigned in
[Queue comment 6735889315](https://github.com/autonomous-ai/lampOS/issues/5385#issuecomment-6735889315).
The face adapter must not turn primary-memory success plus incomplete
related cleanup into an unqualified memory-success answer.

## Change and provenance

The implementation and initial tests were offered by Second in the immutable
[handoff at dee08b9910](https://github.com/autonomous-ai/lampOS/tree/dee08b9910/lampOS/docs/verification/second-face-forget-handoff-20261109).
The retrieved patch's SHA-256 matched
`36fe6500a99b3185cfc11fbedbfee3a0baec4fb16027e25db9a401591b8764df`.
Third reviewed it, applied it without conflicts, independently reproduced
Reviewer-three's original failure, and added absent-field and unavailable-
memory coverage. Second's earlier checks are peer evidence, not Third's runs.

`face_and_memory` retains only the three fixed cleanup states: `cleared`,
`kept`, or `uncertain`. Present unknown values become `uncertain`; absent
fields stay absent. An explicit partial or malformed completion flag, or
any non-cleared cleanup state, produces `everything: false` and fixed
partial-memory words. A complete memory receipt cannot upgrade a partial
face result or its warning about old face records kept apart. Third narrowed
Second's handoff to never add `everything: true`: the inner memory call
bypasses the outer setup-name, place, voice-print, journal and reminder
cleanup. Their scope is not expanded to make a stronger claim. The face's
own result and note remain intact. No arbitrary memory error, path, person,
or stored text is copied by the new fields.

There are no new deletion calls, retries, deadlines, switches, persistence
fields, bus topics, or background work. Scope selection, final guards, and
the predicate for resetting the active conversation's context are unchanged.
This does not claim to fix all face-forget context handling.

## Baseline

The adapter branch is independent on `3d8afb415`. It was first prepared on
#5803's carried head `7fc6ca20e`, then unstacked before broad tests; every
authored added/removed line was compared and remained identical. The new
per-step producer outcomes still come from #5803. The adapter accepts their
absence, so it has no build dependency on that patch. An earlier Clippy
attempt belonged to the stacked composition; that pipeline was superseded
before broad tests, and final gates are run independently.

## Independent fail-first evidence

On the original adapter, Third ran:

```sh
cargo test -q -p lamp-voice --lib \
  face_forget_cleanup_partial_memory_result_stays_partial -- --test-threads=1
```

It failed as intended: 0 passed, 1 failed, 5,401 filtered out, exit 101.
`everything` was null instead of false; the old adapter returned only
`{"memory":"forgotten too","ok":true}`. The input had successful primary
memory deletion, `everything: false`, and kept/uncertain/cleared cleanup
states. No production change was present for this run.

An additional standalone test extracted the unchanged adapter directly from
Git and reproduced the same failure. This is only helper-level evidence;
the Cargo regression above is the project test. An initial standalone compile
selected a cached serde_json built by a different Rust version and did not
run; selecting a compatible artifact resolved that harness setup issue.

## Coverage and checks

Formatting and strict Clippy passed on the independent branch. The broad
package run completed with failures recorded below; the workspace build
passed. Native CI and reads are still required.

- Each cleanup field independently: kept, uncertain, private string, object,
  and null; both deleted and already-empty primary memory.
- Explicit partial completion stays partial after later cleared steps;
  malformed completion cannot become success.
- A partial face or old-record note cannot be upgraded by memory success.
- Missing fields remain absent. Unavailable/refused memory retains the face
  success but never claims memory success, even with contradictory completion.
- An Engine regression examines the actual next model request after the face
  call, including fixed outcomes and absence of an injected private canary.
  It does not invoke a live model or establish spoken behavior.

On the independent branch, all seven focused functional regressions passed:
six `remember::tests::face_forget_cleanup*` cases and the actual next-model-
request case. To avoid waiting on the broad Cargo build lock, these ran
from the freshly built library executable
`target/debug/deps/lamp_voice-0e0f35ef66c6fc0e`; its dependency information
names this adapter worktree. Arguments were `face_forget_cleanup --skip
face_forget_cleanup_adapter_timing --test-threads=1` (6 passed), then
`--exact second_provider::tests::held_forget::a_face_forgets_partial_cleanup_reaches_the_next_model_request --test-threads=1`
(1 passed). The legacy face/memory-result test also passed during the earlier
stacked focused run. Full-suite results remain separate.

Host: Mac, Rust 1.96.0, debug profile. Cargo uses two build jobs and a shared
local target cache. `cargo fmt --all --check` and strict Clippy with
`--all-targets -- -D warnings` passed for `lamp-body`, `lamp-cloud`,
`lamp-control`, `lamp-expression`, `lamp-keeper`, `lamp-lean`, `lamp-memory`,
`lamp-name`, `lamp-peek`, `lamp-setup`, `lamp-skills`, `lamp-update`,
`lamp-voice` and `lamp-voice-eval`. Full serial tests use the same package
list with `--no-fail-fast -- --test-threads=1`, followed by
`cargo build -q --workspace`. The final gate uses a short group-owned
`TMPDIR` to avoid the previously reproduced Mac group and socket-path limits.
The installer target passed all 160 tests. The following unchanged
`lamp-update --test it` journal-retention tests failed on macOS:

- `image_ramlog::a_boot_the_owner_forgot_is_not_kept_at_its_shutdown`
- `image_ramlog::a_failed_move_keeps_the_kept_boots_and_says_so`
- `image_ramlog::the_kept_boots_are_kept_72_hours_at_most`
- `image_ramlog::the_last_two_boots_are_kept_at_shutdown_only`

The failed-move output reports `stat: illegal option -- c` in
`lamp-journal-keep`; the other cases report missing retained files or the
resulting wrong retained-file count. The update crate and journal script
are byte-identical to #5803's carried source, where the same host failures
were retained and the native ARM64 gate passed. This is not an all-pass
Mac run and does not substitute that earlier CI for this branch's gates.
The broad serial run completed: 10,377 passed, 5 failed and 57 ignored
across 102 test targets. Besides the four update failures above,
`second_provider::tests::chat_second_try::off_no_second_try_goes` failed
with two observed requests instead of one. Its 3,001 ms timeout is expected;
the unexpected request count is not. This fixture receives no model answer
or tool call and never enters the face adapter. The unchanged test passed
in isolation, then the complete `second_provider::tests::chat_second_try::`
module passed serially using the same freshly built test executable. The
original failure is retained; its cause has not been established. The Voice
library target itself recorded 5,387 passed, 1 failed and 16 ignored.
`cargo build -q --workspace` passed (exit 0); the broad test command
returned exit 101. The retry module passed 27 tests, 0 failed.

Native ARM64 and Linux Clippy remain separate gates. No device was accessed
or deployed to.

## Latency boundary

Target: under 1 ms of added adapter bookkeeping. The manual timing fixture
measures `face_and_memory` entry to returned adapted value, separately for
its first invocation in a fresh process and repeated calls. This excludes
file deletion, IPC, process startup, model response, audio and device behavior.
The patch adds only a bounded three-field iteration and fixed result values;
it adds no blocking I/O to this boundary.

On the independent branch, the manual timing test passed in a fresh process:
first invocation 48,760 ns (0.048760 ms), repeated p95 2,285 ns (0.002285 ms),
maximum 2,721 ns across 100 repeats. Command from `lampOS/`:

```sh
cargo test -q -p lamp-voice --lib face_forget_cleanup_adapter_timing -- \
  --ignored --exact remember::tests::face_forget_cleanup_adapter_timing --nocapture
```

This passed 1 test with 5,403 filtered out. It measures the whole adapter,
not a before/after speedup, and adds no wall-clock threshold assertion.
