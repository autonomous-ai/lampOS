> Historical preparation plan, frozen before execution. Wording below such as
> “unexecuted” describes that preparation stage. The probes have now run;
> see the [completed verification record](../../README.md) and
> [command results](../commands.json). The two logical-clock opt-ins in the
> lease plan have since been folded into the submitted tests.

# Own-answers / held Music RPC overlap control

Scratch only: none of these hooks belongs in the candidate commit. Prepared,
not compiled or run. Base: `69f2228338a8df63a81d23087c3e55308fa125ca`.
The candidate differs for this control by only the three-line guard insertion
in `check_stop_during_music_rpc`; the own-answers fixture is unchanged.

`before-overlap.patch` applies to the exact base. `after-overlap.patch` applies
to that base plus `candidate-guard.patch`. The before/after hooks are identical;
only original hunk positions differ. `manifest.json` records source and patch
hashes. Both patches passed `git apply --check` against their frozen source,
and the after patch also passed against the current candidate worktree.
Every hook is an insertion: removing the inserted bytes recovers both original
files exactly, preserving every original fixture assertion and deadline.

## The controlled schedule

The one selected scratch test is:

```text
second_provider::worker::tests::own_answers_and_pending_music_share_no_conversation_state
```

1. A real thread runs the complete original own-answers fixture. It pauses just
   before judging “Yes, remind me after lunch.”, while holding its existing
   `one_thread()` guard. The control asserts that its real `THREAD` contains the
   lamp's question for request 3 and reports `OwnReady`.
2. A second real thread calls the complete original Music helper with
   `warm_stt=false`. Only this thread installs the scratch guard observer.
   At the existing guard, `try_lock()` reports `MusicContended` only after an
   actual `WouldBlock`, then takes the original blocking lock normally.
   There is no timed absence check.
3. Without the candidate guard, the Music helper takes and replaces `THREAD`,
   runs its real worker and local RPC peer, and reaches the existing context
   oneshot. The reply is still held by the original peer. The hook reports
   `MusicPending` and pauses before the original Stop measurement starts.
4. The driver resumes own-answers on the first positive event:
   - Baseline: `MusicPending`; the lamp's stored question has been lost.
     Its original reply-to-question assertion should fail.
   - Candidate: `MusicContended`; own-answers keeps its question, completes
     every original case, and releases its guard. Music can then reach its
     real `MusicPending` state.
5. After both own-answers completion and Music pending are observed, the
   driver resumes Music. All original Music assertions run, including scope
   revocation, no later audio/chat, and the unchanged 300 ms Stop bound.
   The original own-answers panic payload is resumed only after Music passes.

The scratch driver and pauses each have a 3 s watchdog. They are positive
rendezvous, not completion sleeps. The original 2 s dispatch wait, peer's 3 s
absolute RPC budget, 100 ms authority lease, and 300 ms Stop checks are unchanged.
No Cargo targets or product code are changed by the control.

## Replay in disposable native worktrees

Keep a clean baseline and a separate candidate at the base above. From each
checkout root, apply only its corresponding patches:

```sh
# Baseline checkout:
git apply /private/tmp/tests-codex-voice-events-evidence-20261022/controls/thread-overlap/before-overlap.patch

# Candidate checkout:
git apply /private/tmp/tests-codex-voice-events-evidence-20261022/controls/thread-overlap/candidate-guard.patch
git apply /private/tmp/tests-codex-voice-events-evidence-20261022/controls/thread-overlap/after-overlap.patch
```

Use dedicated native `CARGO_TARGET_DIR` values and unset `CARGO_BUILD_TARGET`.
From each `lampOS`, compile once and retain the output externally:

```sh
cargo test -p lamp-voice --lib --no-run --offline --message-format=json
```

Select that compile's `lamp_voice` test executable from its compiler-artifact
record. From `lampOS/crates/voice`, run the selected scratch test three times,
retaining each first result and complete output:

```sh
<lamp_voice-test-executable> --exact second_provider::worker::tests::own_answers_and_pending_music_share_no_conversation_state --nocapture --test-threads=1
```

Only the driver is selected; it starts both original fixtures on separate
threads. Do not also run their standalone tests in this controlled invocation.

Accept a baseline reproduction only with all of:

- `own_question_before=true`, followed by `MusicPending` before own-answers
  resumes, then `own_question_after=false`;
- the original reply-to-question array assertion fails;
- `THREAD_OVERLAP own_ok=false music_ok=true` and exit 101.

Accept the repair only with `MusicContended`, `own_question_after=true`, all
original own-answers assertions passing, then the real `MusicPending` event,
`THREAD_OVERLAP own_ok=true music_ok=true`, and exit 0. A watchdog, unexpected
event, RPC failure, or a different original assertion is not reproduction.
The original helper also serves the warm-STT variant; run both unchanged
standalone Music tests in the final candidate gates.
