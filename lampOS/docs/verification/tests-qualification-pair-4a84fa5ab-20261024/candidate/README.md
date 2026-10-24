# #3190 matched qualification: candidate

One complete native x86 Mac run on clean committed source `93c995a6b1757af77d5d2a25f1567f314ad5f72d`: **7,234 passed, 2 failed, 34 ignored**, controller exit **1**. Its comparison base is `4a84fa5ab6a9133aeec555ec4bc0636dd953f660`. The original historical failures remain separately bound to their recorded sources; this is a new matched observation, not a retry-until-green or historical-cause finding.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3959 | 2 | 15 | 0 | 101 | 408.820701 |
| All eleven readers, default parallel | 3275 | 0 | 19 | 0 | 0 | 1923.443962 |

[Complete census](census-and-outcomes.json) binds every actual test row to a separately listed executable inventory. No target was incomplete or unrun. Compilation and ignored unsupported Mac cases are not passes. [Affected cases](affected-cases.json) audits 88 selected authored identities; the base has no authored-case selection but still has the complete executed census.

Failed observations:

- `lamp-voice:lib:lamp_voice`: `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`.
- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.

[Failures](failures.json) retain original diagnostics, byte offsets and hashes. [Light-call boundary](light-call-boundary.json) preserves the unchanged index-10 strict 25 ms predicate; the maximum is descriptive only. Classification appears only in the paired table above this folder, after both full runs and the prescribed standalone observations.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 13.251843 |
| clippy | 0 | 95.461553 |
| workspace-build | 0 | 92.088485 |
| diff-check | 0 | 0.088556 |

[Commands](command-results.json) and [frozen plan](gate-plan.json) retain the exact arguments, environment, watchdogs, exits and failed stages. Fmt, strict twelve-package all-target Clippy, full Voice, full eleven-reader closure, workspace build and diff check ran once. Test phases used default parallelism and `--no-fail-fast`; RUST_TEST_THREADS was unset. Ordinary test failures permit independent gates to finish and remain failures. Source/compile/timeout/census failures stop the attempt.

[Source/environment](source-and-environment.json) records guards for all 10,309 repository paths, modes and literal symlinks. Byte-preserving freshness invalidation and every subsequent guard ended at `52a8fb75ebb66fc38704735686a06d362938354872491c26dd7f3aedee5a5ced`. The source was clean and committed. [Self-review](source-self-review.json), [equal range-diff](range-diff.json) and [authored patch](authored.patch.json) preserve scope and limitations. The production local clock alias resolves to the same IPC function; literal token equality is not claimed. No product deadline or assertion was widened, no lamp was accessed and no ARM build ran locally.

[Native streams](native-streams.json) retain exact stderr and all human stdout. Cargo stdout uses the accepted explicit byte-range projection removing only parsed build records. [Artifact bindings](artifact-bindings.json) retain executable hashes before shared-target reuse; binaries are not published and their paths are not promised to remain available. [External raw index](external-raw-index.json) names complete original streams and inventory hashes; local paths are not a remote availability promise. No Python, executables, whole source exports or bulk source maps are published. Exact-head Linux formatting and native ARM64/Linux Clippy are separate receipts.
