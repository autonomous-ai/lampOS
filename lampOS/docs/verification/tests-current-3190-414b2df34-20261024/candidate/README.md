# #3190 current-head candidate qualification

One complete native x86 Mac run on clean committed source `414b2df34f5398d164affb128780fdad2ba78d63`: **7,249 passed, 3 failed, 34 ignored**, controller exit **1**. Its integration parent is `9ed44f88c573837c50451c94f4d88f010e8a6721`; no new full baseline is part of this gate. The original historical failures remain separately bound to their recorded sources; this is a separate current-head observation, not a relabelled frozen result or a historical-cause finding.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3976 | 1 | 15 | 0 | 101 | 467.602025 |
| All eleven readers, default parallel | 3273 | 2 | 19 | 0 | 101 | 1725.324852 |

[Complete census](census-and-outcomes.json) binds every actual test row to a separately listed executable inventory. No target was incomplete or unrun. Compilation and ignored unsupported Mac cases are not passes. [Affected cases](affected-cases.json) audits 88 selected authored identities; the base has no authored-case selection but still has the complete executed census.

Failed observations:

- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-memory:test:it`: `voice::a_follow_up_forget_preserves_an_unrelated_keep_with_shared_evidence`.
- `lamp-memory:test:it`: `voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh`.

[Failures](failures.json) retain original diagnostics, byte offsets and hashes. [Light-call boundary](light-call-boundary.json) preserves the unchanged index-10 strict 25 ms predicate; the maximum is descriptive only. Current-source failure classification and any prescribed standalone observations are recorded separately. The completed frozen comparison remains at its explicit earlier source identities.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 14.591956 |
| clippy | 0 | 120.530324 |
| workspace-build | 0 | 66.6945 |
| diff-check | 0 | 0.096145 |

[Commands](command-results.json) and [frozen plan](gate-plan.json) retain the exact arguments, environment, watchdogs, exits and failed stages. Fmt, strict twelve-package all-target Clippy, full Voice, full eleven-reader closure, workspace build and diff check ran once. Test phases used default parallelism and `--no-fail-fast`; RUST_TEST_THREADS was unset. Ordinary test failures permit independent gates to finish and remain failures. Source/compile/timeout/census failures stop the attempt.

[Source/environment](source-and-environment.json) records guards for all 10,313 repository paths, modes and literal symlinks. Byte-preserving freshness invalidation and every subsequent guard ended at `091edb1c89bde9f592e99c109c33d2b08da86efc39dd718826e0a0f3b0ed138e`. The source was clean and committed. [Self-review](source-self-review.json), [equal range-diff](range-diff.json) and [authored patch](authored.patch.json) preserve scope and limitations. The production local clock alias resolves to the same IPC function; literal token equality is not claimed. No product deadline or assertion was widened, no lamp was accessed and no ARM build ran locally.

[Native streams](native-streams.json) retain exact stderr and all human stdout. Cargo stdout uses the accepted explicit byte-range projection removing only parsed build records. [Artifact bindings](artifact-bindings.json) retain executable hashes before shared-target reuse; binaries are not published and their paths are not promised to remain available. [External raw index](external-raw-index.json) names complete original streams and inventory hashes; local paths are not a remote availability promise. No Python, executables, whole source exports or bulk source maps are published. Exact-head Linux formatting and native ARM64/Linux Clippy are separate receipts.

[Parser validation](parser-validation.json) records four retained raw-stream replays and seven rejected malformed or ambiguous projections, with no executable invocation. Current interleaving mappings are independently checked against exact stdout byte offsets. [Integration delta](integration-delta.json) and [shared-host context](shared-host-context.json) distinguish this current gate from the completed frozen comparison.
