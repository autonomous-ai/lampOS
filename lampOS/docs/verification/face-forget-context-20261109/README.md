# Face-forget model-context boundary — 9 November 2026

Third's next piece, assigned in
[Queue comment 6738675383](https://github.com/autonomous-ai/lampOS/issues/5385#issuecomment-6738675383),
was first gated on the landed #5803 baseline `429b1f3ba`. El's reviewed #5792
is carried as `89207a696`, with unchanged stable patch
`d8085cd28e60d0e069523b36845c1d88afda1379` from `41f543f241`; only its
commit message was made one line. Third's separate adapter #5874 is carried
as `7251a51e2` from `b7420fea1`. Both dependencies remain explicit; this
is not a claim that the context boundary already has their reads or gates.
The earlier preparation worktree remains a detached backup of the tests;
validation reuses the adapter's completed build worktree to avoid rebuilding
unchanged dependencies solely because their source paths changed. Work is
in progress; the focused regressions pass as recorded below, while broad
and native gates remain incomplete. No readiness claim is made yet.

## Fault and scope

An admitted `forget_my_face` can empty the known person's memory through
`forget_face_memory`. The previous Engine context boundary recognized only
`forget` and `forget_everything`. Consequently the next model request can
reuse the earlier memory note, history, pictures and tool results, and an
ordinary call can continue the remaining tool batch planned from those
inputs. A held face-forget can append its result to the old context before
round one. This is separate from whether deletion actually succeeded and
from whether its result was accurately reported.

The boundary reuses the existing static-instruction/current-question
reset after an admitted call returns, including uncertain receipts. It does
not add deletion calls, broaden who may authorize deletion, change final
scope/stop checks, or claim to retract generated or queued audio.

Two Engine regressions inspect the entire next request, not merely
its final tool result. Their fixture contains a private canary in the initial
system text, appended note and history. The ordinary case also schedules a
later tool call with the canary in its arguments. The held case confirms the
face forget with the same person's yes. Success, partial and uncertain
receipts are covered. Two additional
sanitizer regressions require independent face/primary-memory confirmations,
fixed cleanup outcomes, unknown values treated as uncertain, and no invented
aggregate success. A fifth Engine regression adds El's
exact rename proposal followed by "Forget my face." and checks the whole
next request in that same turn with the rename switch on and off. El's
existing next-turn and revoked/stopped-turn cases remain intact.

## Integration boundaries

- #5803 supplies content-free room/consent cleanup results. Third's separate
  face-result adapter carries those results without adding aggregate success.
  This change must preserve that reporting through the context sanitizer.
- El's #5792 already clears pending rename state before awaiting all admitted
  forgets, including face-forget, and suppresses cached rename notes when the
  context boundary ran. Reuse that change; do not duplicate its owner or
  alter its approved handling of a separately confirmed fresh rename.
- Fourth's #5856/#5858 cover provider and playback cancellation and expressly
  exclude face-forget. Keep their memory-only predicate distinct from this
  model-context predicate; a clean model request is not an acoustic claim.

A source audit found no normal same-yes setup-save or setup-name prompt
coexisting with a held face-forget: a name read-back rejects an answer
containing `face`, and a face-save question rejects `forget`. The fixed
setup-save rejection note contains no private name. Do not suppress unrelated
setup actions speculatively. A possible preexisting memory-forget/setup-name
interaction is outside this face-specific change and has not been reproduced.

## Latency and validation plan

Target: less than 1 ms of host bookkeeping from sanitized receipt/context
reset entry to the fresh message vector. Reuse already-loaded static
instructions and fixed-state outcomes; the bookkeeping adds no I/O, polling,
debounce or background task. Discarding a stale batch can require the model
to plan the remaining requested work afresh, within the existing round limit;
that model latency is separate from the local reset cost. Measure first use
and repeats separately. Deletion IPC, slow dependencies, model response,
provider cancellation, audio and device behavior are outside that measurement.

Run the new regressions before production edits, then after the fix; exercise
existing memory-forget, held-forget and rename integration coverage, formatting,
strict Clippy, relevant package tests and workspace build. Native Linux/ARM64
checks and two privacy reads remain required before readiness. Local Cargo
jobs share one cache and are serialized with the preceding adapter gate.
No device access or deployment is authorized by this task.

## Fail-first evidence

Before production edits, on composition `7251a51e2`, this command ran from
`lampOS/` with the existing shared target cache and short `TMPDIR`:

```sh
cargo test -q -p lamp-voice --lib face_forget_context_ -- --test-threads=1
```

Result: 0 passed, 5 failed, 5,421 filtered out, exit 101. Both receipt tests
failed because `face_forgotten` was absent. The ordinary and held Engine
requests retained the private canary in their system text, note and history;
the ordinary request also included the stale timer call and its result.
The rename test retained the old Marisol/Tove proposal in that same turn.
These were assertion failures after actual Engine runs, not compilation or
socket setup failures. No production context edit was present for this run.

The initial fix changed the two active-context admission predicates to include
face-forget and extended the fixed receipt sanitizer. It reuses the existing
reset and stale-batch cancellation; the memory-only predicate remains for
callers that need the narrower classification. The adapter's next-request
test now checks its fixed outcomes in the fresh system receipt and asserts
that the entire request contains neither private canary nor raw tool IDs.

### Cancellation follow-up: focused reproduction and fix

El's [review hold 6742634543](https://github.com/autonomous-ai/lampOS/pull/5878#issuecomment-6742634543)
identified earlier tool rounds retained for a later turn while an admitted
forget awaits its result. If that turn is cut or dropped during the await,
the after-return reset cannot clear those retained rounds.

The follow-up clears carried earlier tool rounds at both admitted-forget
sites, alongside `held_rename::forgot` and before awaiting the call. This
applies to `forget`, `forget_everything` and `forget_my_face`, including a
confirmed held forget. Calls held or refused before admission keep their
existing behavior. After return, the existing reset still rebuilds the
current local messages with the current question and fixed outcomes; no
deletion result is invented for a call whose result was not received.

The original face and plain-forget cases both failed on the actual resumed
model request: the prior recall call and result survived the cut. The first
command ran outside the Rust workspace and failed before testing because its
camera build settings were absent; that is not regression evidence. A first
held-confirmation fixture was anonymous and timed out before its runner;
then its face cache was loaded before camera authority invalidated it. Those
fixture failures are not product failures. The final control loads a known
person after camera authority is installed and reaches the blocked held
runner.

With both admission clears applied, the two reproduced regressions and the
confirmed-held interruption control pass **3/3**. Each drives a real Engine
with synthetic tool results, cuts the awaited forget with an empty line,
and inspects the entire resumed model request. The held control was not
shown failing on the old production code. Logs are
`/private/tmp/third-cut-forget-fail-first-root.log`,
`/private/tmp/third-cut-forget-fail-first-scoped.log` and
`/private/tmp/third-cut-forget-fixed.log`.

The follow-up local gate passed: formatting, strict all-target Clippy for
the 14 affected/dependent packages, workspace build, and 163 focused tests
with zero failures (one manual timing test ignored). The focused suites
cover context reset, held forget and rename, after-forget behavior, action
claims, onboarding, person scope and held save. Logs are
`/private/tmp/third-context-cut-fix-*.log`. Native CI for the new commit
remains pending; passing gates alone do not clear El's hold. The admission clear adds no I/O or
await; it only drops retained request-local messages under the existing
mutex. Prior reset timing measures the after-return sanitizer, not this
new admission clear; combined admission and acoustic latency remain with
Fourth's speech-stop acceptance. No device result is claimed. This change
does not retract queued audio or alter deletion calls, scopes or deadlines.

Formatting and documentation links passed (192 docs, 1,898 links, 0 broken).
The five new fixed regressions passed (one manual timing test ignored), and
the updated adapter next-request regression passed. The first existing
held-forget suite recorded 27 passed / 1 failed: its raw Tool-message
expectation was obsolete after a face-forget reset. It now requires the
current yes, a sanitized positive face receipt, and absence of raw tool IDs
and private context. The original failure is retained. The focused rerun passed: five new functional cases (one timing case
ignored), adapter integration 1/0, held-forget 28/0, held-rename 13/0 and
existing `second_provider::tests::after_forget::` integration 3/0. The
manual timing test passed separately in a fresh process. Native gates are
not yet established for this context change.

## Host latency and remaining gates

Mac, Rust 1.96.0 debug build. Fresh-process `status` entry through `context`
return measured 337,156 ns (0.337156 ms) on first use. Across 100 repeats,
p95 was 40,622 ns (0.040622 ms), maximum 67,575 ns. The fixture includes
partial face/primary-memory and all three cleanup outcomes. Target: under
1 ms of local bookkeeping; no wall-clock threshold assertion was added.
This is the full sanitizer/reset cost, not a before/after speedup. It excludes
file deletion, IPC, model response/replanning, audio and target hardware.

Commands from `lampOS/` (all functional filters use `-- --test-threads=1`):

```sh
cargo fmt --all --check
cargo test -q -p lamp-voice --lib face_forget_context_ -- --test-threads=1
cargo test -q -p lamp-voice --lib a_face_forgets_partial_cleanup_reaches_the_next_model_request -- --test-threads=1
cargo test -q -p lamp-voice --lib second_provider::tests::held_forget:: -- --test-threads=1
cargo test -q -p lamp-voice --lib second_provider::tests::held_rename:: -- --test-threads=1
cargo test -q -p lamp-voice --lib second_provider::tests::after_forget:: -- --test-threads=1
cargo test -q -p lamp-voice --lib face_forget_context_reset_timing -- --ignored --nocapture
```

The final gate runs strict Clippy with `--all-targets -- -D warnings` for
`lamp-body`, `lamp-cloud`, `lamp-control`, `lamp-expression`, `lamp-keeper`,
`lamp-lean`, `lamp-memory`, `lamp-name`, `lamp-peek`, `lamp-setup`,
`lamp-skills`, `lamp-update`, `lamp-voice` and `lamp-voice-eval`, followed by
`cargo test -q -p lamp-voice --lib -- --test-threads=1` and
`cargo build -q --workspace`. Strict Clippy and the workspace build passed; the full Voice library run
completed with the host failures recorded below. The
preceding adapter's larger package run and its known failures are separate
evidence, not a passing context gate. All Cargo jobs use two build workers
and the same local target cache, with a short group-owned `TMPDIR`.

## Full local run and targeted retries

The full serial Voice library run completed with **5,407 passed, 3 failed,
17 ignored**, exit 101. The three failed cases are unchanged from baseline
`429b1f3ba`, not authored by this context patch:

- `second_provider::tests::chat_second_try::a_scope_revoked_while_the_first_waits_sends_no_second_try`:
  two observed requests rather than one. Its unchanged isolated rerun
  passed. This test receives no model answer/tool call and does not enter
  the new face-forget boundary. Cause remains unconfirmed.
- `second_provider::wordless_call::tests::a_sure_hearing_after_many_unasked_ones_is_still_read`
  and `dropping_the_hearings_stops_their_reader`: the generated Unix socket
  path was 111 bytes against the Mac limit of 103. The initial short root
  was still too long for these fixture names. With a fresh group-owned
  `/tmp/q` root, the unchanged entire wordless-call module passed.

The retries used the freshly built library test executable, which reports
5,427 total cases; no production source was changed between the broad run
and retries. The original failures remain recorded. The whole retry-test module then recorded 26 passed / 1 failed: the
original revoked-scope case passed, but
`a_stop_before_its_time_and_no_second_try_goes` observed two requests rather
than one. No claim is made that the whole module passed. Its tests wait
for the first request before cancelling, while a second attempt becomes
eligible after 600 ms; host scheduling is a possible cause, not established
by these runs. The newly failed stop case passed unchanged in isolation (1/0). The short-root wordless-call module passed all six tests. `cargo build -q --workspace` passed
(exit 0). This is not an all-pass local suite; native ARM64 and Linux checks
on the published context head remain required.

## Carry onto train 754

After the original gates above, the three commits rebased cleanly onto
`aaa34f6185a3fe6b8551b9400c424841ff9693ce`. `git range-diff` reports all
three patches identical: El's dependency `89207a696` → `05e6f6af3`, the
adapter `7251a51e2` → `c608e05e1`, and context `802a4d65f` → `71522a05b`.
This verification-note addition is subsequent evidence only. No code or
test change was needed for the carry. The newly landed declined-round
claim fix moves claim evidence registration before the same batch loop;
the face reset still truncates only the unexecuted remainder correctly.

Carry gates passed: formatting, strict Clippy on the same 14 packages,
new face-context cases 5/0 (one timing case ignored), held-forget 28/0,
held-rename 13/0, reset integration 3/0,
`second_provider::tests::action_claim::` 26/0, the newly landed
`a_let_go_face_save_closes_who_you_are_within_20_s` 1/0, and workspace build
(exit 0). Documentation links passed: 192 docs, 1,898 links, 0 broken.
The new run uses `/tmp/q`. The original full-suite failures above remain
part of the record; no passing full rerun on train 754 is claimed. Native
CI will validate the published composition separately.
