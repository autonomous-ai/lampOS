# Combined boot-clock and peer-closure local Mac full gate on 09f8e7412

The one prescribed combined-source local Mac full gate completed with **7,184 passed, 9 failed and 32 ignored**. Local execution session 20730 exited **1**. Source was rebased boot HEAD `4e8ea747cd7dc745742283d6d08cb100297a8bfa` on target `09f8e7412f92870e16ccca14e1b9cdc1b14add48`, plus exactly the reviewed peer fixture and combined documentation overlays. No final combined commit existed at execution time. Every failed observation remains unwaived.

| Phase | Passed | Failed | Ignored | Filtered | Exit | Wall seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Full Voice, default parallel | 3931 | 3 | 15 | 0 | 101 | 492.149965 |
| All eleven readers, default parallel | 3253 | 6 | 17 | 0 | 101 | 1963.711817 |

Both test phases compiled successfully and every actual target/case result binds to its independently listed binary census. No target was incomplete or unrun. Unsupported custom Update harness cases are ignored on Mac, not passed. [Census and outcomes](census-and-outcomes.json) retain each actual row and artifact binding; compilation is not execution. [Affected cases](affected-cases.json) separately maps all 88 selected identities: 86 clock fixtures, one loading-child fixture and one peer-closure integration case.

Actual failures:

- `lamp-voice:lib:lamp_voice`: `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture`.
- `lamp-voice:lib:lamp_voice`: `privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second`.
- `lamp-voice:test:it`: `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`.
- `lamp-body:lib:lamp_body`: `runtime::supervisor::tests::dropping_reaped_predecessor_does_not_remove_active_successor_sockets`.
- `lamp-expression:test:runtime`: `enabled_output_uses_a_new_session_and_expires_after_input_loss`.
- `lamp-expression:test:runtime`: `only_a_service_with_the_offline_face_draws_listening_unavailable_when_offline`.
- `lamp-expression:test:runtime`: `without_a_pause_said_expression_asks_exactly_as_before`.
- `lamp-expression:test:runtime`: `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`.
- `lamp-memory:test:it`: `voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh`.

[Complete failures](failures.json) preserve the full diagnostics and exact raw-stream byte offsets/hashes. [Light-call interpretation](light-call-boundary.json) identifies the unchanged sorted index 10 strict 25 ms assertion when it fails; the maximum is descriptive only. No failure is classified by source identity alone, waived, or retried.

| Independent gate | Exit | Wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 11.97472 |
| clippy | 0 | 92.054301 |
| workspace-build | 0 | 77.097187 |
| diff-check | 0 | 0.091331 |

The fixed sequential commands were fmt, strict all-target Clippy for twelve packages, full Voice, all eleven readers, workspace build and full combined diff check. Both test phases use `--no-fail-fast` and default parallelism, with `RUST_TEST_THREADS` unset. Ordinary completed test failures remain FAILED while independent planned gates continue. [Command results](command-results.json), [frozen plan](gate-plan.json) and [dependency closure](dependency-closure.json) retain exact arguments, watchdogs, exits and scope. The separate [exact-source Linux amd64 formatting receipt](https://github.com/autonomous-ai/lampOS/tree/1a11082e57/lampOS/docs/verification/voice-boot-clock-20261023/combined-09f8e7412/linux-fmt) passed before this gate; this Mac result does not replace required Linux CI or reviewer disposition.

[Fold proof](fold-proof.json), [range-diff](fold-range-diff.json) and [exact combined patch](authored.patch.json) establish the nine-file union requested by First. Old boot head `f121d6391` was archived remotely before rewrite. Its clean rebase preserved the patch; all eight Rust source blobs equal the reviewed boot/peer versions, and both documentation additions remain. All nine target-base blobs equal their preceding 810e versions. Historical controls/gates remain bound to their recorded sources and are not relabelled as current observations. This combined qualification is not a retry of an unchanged failed source.

All 10,055 tracked repository paths, including 6,709 lampOS entries, repository-root lamp fixtures, docs, modes and literal symlinks, were pinned. Freshness invalidation changed mtimes only; source guards before and after every stage ended at aggregate `f346596d5fa5ad67f19179739baf22ba50eb101a79e95e91f9e679d73501509d`. [Source/environment](source-and-environment.json) preserves the checks; the bulk map stays external. The two dirty overlays are explicitly identified; the other seven authored files are in the rebased boot commit.

[Self-review](source-self-review.json) preserves wrong-person, Stop/forget, default behavior, census and gate boundaries. The existing stop_words non-test import alias resolves to the same IPC function; this is equal function semantics, not literal production-token equality. Product deadlines, freshness limits and all original assertions remain unchanged. No lamp/device action or local ARM build ran.

Rust host is `x86_64-apple-darwin`; the captured test artifacts are Mach-O x86_64. Physical hardware ISA was not independently measured. The socket-enabled launch passed Unix-datagram and IPv4-loopback preflights. Jobs 4, debug 0, incremental off, offline/locked and the exclusively assigned MAIN target were used; target ownership was released at terminal.

[Local Mac streams](native-streams.json) retain exact stderr and non-Cargo stdout. Cargo stdout is an explicit byte-range projection removing only parsed build records; every human test row, summary and diagnostic remains exact. Original and retained hashes/counts are distinct. [Artifact bindings](artifact-bindings.json) were captured before target reuse. [External raw index](external-raw-index.json) identifies complete originals, inventory streams and limits: local paths do not promise remote availability. No binaries, Python, full Cargo message JSON or bulk source maps are published. JSON envelopes preserve raw whitespace without formatting exceptions.
