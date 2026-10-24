# #3190 preserved base run on 88b1ad796

This is a **preserved earlier-base observation**, not the current-base qualification. Integration moved to `4a84fa5ab` while it ran; the [four-path delta includes a compiled Voice-eval document](source-delta-readers.json). The [prepared candidate was superseded without execution](superseded-unexecuted-candidate.json). A fresh matched full pair is required. [Shared-host context](shared-host-context.json) records source-checkout I/O; no idle-machine or equal instantaneous load claim is made.

One complete native x86 Mac run on clean committed source `88b1ad796e5045490ad678eb240eea2dab010c81`: **7,227 passed, 9 failed, 34 ignored**, controller exit **1**. Its comparison base is `88b1ad796e5045490ad678eb240eea2dab010c81`. The original historical failures remain separately bound to their recorded sources; this is an independent baseline observation, not a completed paired comparison or historical-cause finding.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3958 | 3 | 15 | 0 | 101 | 549.984881 |
| All eleven readers, default parallel | 3269 | 6 | 19 | 0 | 101 | 2273.152219 |

[Complete census](census-and-outcomes.json) binds every actual test row to a separately listed executable inventory. No target was incomplete or unrun. Compilation and ignored unsupported Mac cases are not passes. [Affected cases](affected-cases.json) audits 0 selected authored identities; the base has no authored-case selection but still has the complete executed census.

Failed observations:

- `lamp-voice:lib:lamp_voice`: `coordinator::tests::reminders::a_reminder_cancelled_while_muted_is_never_said`.
- `lamp-voice:lib:lamp_voice`: `provider_worker::tests::held_consent::an_unheld_reports_stop_is_done_and_answered_as_before`.
- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-memory:test:it`: `voice::forget_that_after_a_follow_up_leaves_nothing_of_its_conversation`.
- `lamp-skills:test:it`: `skills::the_asking_lines_late_words_do_not_end_its_search`.
- `lamp-update:test:round`: `self_cuts::cut_after_confirmation_clears_pending_keeps_new_runnable_on_restart`.
- `lamp-update:test:round`: `self_cuts::cut_after_pending_is_saved_keeps_old_runnable_and_recovers_before_next_round`.
- `lamp-update:test:round`: `self_cuts::cut_after_candidate_is_synced_keeps_old_runnable_and_reuses_candidate`.
- `lamp-voice-eval:test:it`: `serve_session::serve_readiness_comes_only_from_the_fresh_cue_never_from_stdout`.

[Failures](failures.json) retain original diagnostics, byte offsets and hashes. [Light-call boundary](light-call-boundary.json) preserves the unchanged index-10 strict 25 ms predicate; the maximum is descriptive only. No paired candidate or standalone observations ran for this superseded source pair; no classification or qualification is claimed from this preserved base run alone.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 15.358849 |
| clippy | 0 | 139.888527 |
| workspace-build | 0 | 131.331149 |
| diff-check | 0 | 0.095687 |

[Commands](command-results.json) and [frozen plan](gate-plan.json) retain the exact arguments, environment, watchdogs, exits and failed stages. Fmt, strict twelve-package all-target Clippy, full Voice, full eleven-reader closure, workspace build and diff check ran once. Test phases used default parallelism and `--no-fail-fast`; RUST_TEST_THREADS was unset. Ordinary test failures permit independent gates to finish and remain failures. Source/compile/timeout/census failures stop the attempt.

[Source/environment](source-and-environment.json) records guards for all 10,309 repository paths, modes and literal symlinks. Byte-preserving freshness invalidation and every subsequent guard ended at `f0f95c6d317432597653b6f3ac086f3be4a769a0c16a0170634eece8f4b27a56`. The source was clean and committed. [Self-review](source-self-review.json), [equal range-diff](range-diff.json) and [authored patch](authored.patch.json) preserve scope and limitations. The production local clock alias resolves to the same IPC function; literal token equality is not claimed. No product deadline or assertion was widened, no lamp was accessed and no ARM build ran locally.

[Native streams](native-streams.json) retain exact stderr and all human stdout. Cargo stdout uses the accepted explicit byte-range projection removing only parsed build records. [Artifact bindings](artifact-bindings.json) retain executable hashes before shared-target reuse; binaries are not published and their paths are not promised to remain available. [External raw index](external-raw-index.json) names complete original streams and inventory hashes; local paths are not a remote availability promise. No Python, executables, whole source exports or bulk source maps are published. Exact-head Linux formatting and native ARM64/Linux Clippy are separate receipts.
