# #3190 fixed comparison: completed base phase

One complete native x86 Mac run on clean committed source `4a84fa5ab6a9133aeec555ec4bc0636dd953f660`: **7,219 passed, 17 failed, 34 ignored**, controller exit **1**. Its comparison base is `4a84fa5ab6a9133aeec555ec4bc0636dd953f660`. The original historical failures remain separately bound to their recorded sources; this is the completed base phase of the fixed comparison, not a retry-until-green or historical-cause finding. The matched candidate 93c995a6b began separately after this phase completed; no candidate outcome or completed pair is claimed here.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3950 | 11 | 15 | 0 | 101 | 601.039786 |
| All eleven readers, default parallel | 3269 | 6 | 19 | 0 | 101 | 2492.554602 |

[Complete census](census-and-outcomes.json) binds every actual test row to a separately listed executable inventory. No target was incomplete or unrun. Compilation and ignored unsupported Mac cases are not passes. [Affected cases](affected-cases.json) audits 0 selected authored identities; the base has no authored-case selection but still has the complete executed census.

Failed observations:

- `lamp-voice:lib:lamp_voice`: `coordinator::optional_ring_tests::a_stop_during_a_restart_waits_for_it_and_ends_clean`.
- `lamp-voice:lib:lamp_voice`: `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture`.
- `lamp-voice:lib:lamp_voice`: `coordinator::tests::termination::sigint_retains_failure_and_acknowledged_synthetic_diagnostics`.
- `lamp-voice:lib:lamp_voice`: `coordinator::tests::termination::sigterm_retains_failure_and_acknowledged_synthetic_diagnostics`.
- `lamp-voice:lib:lamp_voice`: `hand_controls::tests::with_the_wait_a_head_touch_is_read_as_it_arrives`.
- `lamp-voice:lib:lamp_voice`: `coordinator::tests::second_route::the_second_routes_answer_nods_and_sways_the_head_as_it_is_spoken`.
- `lamp-voice:lib:lamp_voice`: `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`.
- `lamp-voice:lib:lamp_voice`: `session_reader::tests::group_lookup_failure_retains_private_logging_without_writer_failure`.
- `lamp-voice:test:it`: `diagnostics::explicit_reset_and_privacy_boundaries_are_retained_and_split_epochs`.
- `lamp-voice:test:it`: `diagnostics::capture_files_preserve_pcm_offsets_hashes_and_full_acknowledgement`.
- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-expression:test:runtime`: `enabled_output_uses_a_new_session_and_expires_after_input_loss`.
- `lamp-expression:test:runtime`: `only_a_service_with_the_offline_face_draws_listening_unavailable_when_offline`.
- `lamp-expression:test:runtime`: `without_a_pause_said_expression_asks_exactly_as_before`.
- `lamp-expression:test:runtime`: `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`.
- `lamp-memory:test:it`: `health::the_environment_alone_does_not_enable_reporting_or_create_bus_data`.
- `lamp-memory:test:it`: `voice::a_held_real_store_costs_only_the_note_and_call_deadlines_and_cleans_a_late_note`.

[Failures](failures.json) retain original diagnostics, byte offsets and hashes. [Light-call boundary](light-call-boundary.json) preserves the unchanged index-10 strict 25 ms predicate; the maximum is descriptive only. Classification remains pending until both full runs and the prescribed standalone observations complete.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 14.495769 |
| clippy | 0 | 126.172306 |
| workspace-build | 0 | 84.219168 |
| diff-check | 0 | 0.092646 |

[Commands](command-results.json) and [frozen plan](gate-plan.json) retain the exact arguments, environment, watchdogs, exits and failed stages. Fmt, strict twelve-package all-target Clippy, full Voice, full eleven-reader closure, workspace build and diff check ran once. Test phases used default parallelism and `--no-fail-fast`; RUST_TEST_THREADS was unset. Ordinary test failures permit independent gates to finish and remain failures. Source/compile/timeout/census failures stop the attempt.

[Source/environment](source-and-environment.json) records guards for all 10,309 repository paths, modes and literal symlinks. Byte-preserving freshness invalidation and every subsequent guard ended at `bfaedc43c590075135ab366e9a548f926ee3c8e8895b0174200099122fe8acd5`. The source was clean and committed. [Self-review](source-self-review.json), [equal range-diff](range-diff.json) and [authored patch](authored.patch.json) preserve scope and limitations. The production local clock alias resolves to the same IPC function; literal token equality is not claimed. No product deadline or assertion was widened, no lamp was accessed and no ARM build ran locally.

[Native streams](native-streams.json) retain exact stderr and all human stdout. Cargo stdout uses the accepted explicit byte-range projection removing only parsed build records. [Artifact bindings](artifact-bindings.json) retain executable hashes before shared-target reuse; binaries are not published and their paths are not promised to remain available. [External raw index](external-raw-index.json) names complete original streams and inventory hashes; local paths are not a remote availability promise. No Python, executables, whole source exports or bulk source maps are published. Exact-head Linux formatting and native ARM64/Linux Clippy are separate receipts.

[Parser recovery](census-recovery.json) preserves the initial stopped controller receipt and exact raw-byte mapping: one Voice-eval libtest row was split by child readiness JSON, followed by its actual `ok`. Re-parsing that retained row bound all 378 actual cases. No test was rerun. A separate continuation executed only the previously unrun workspace build and diff/source guards once; the original controller exit remains 1.

The included authored patch and self-review describe the paired candidate, not changes to this clean baseline.
