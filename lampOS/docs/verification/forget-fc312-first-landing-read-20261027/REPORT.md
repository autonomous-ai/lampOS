# #3564 current first-landing fence evidence

Original head: `fc3125d3f36512d252044cec22e3c8ce2b94a67e`.
Original ARM64 run 40526027358 and Linux Clippy run 40526027398, attempt 1, both report success. No rerun or dispatch was requested.

The ARM checkout log binds actual merge `2be38c966fab995409c1cda672eb9e0286d8d5ea`, tree `a9600d7e6963f2851e3e1293e7e104e91755123c`, ordered parents `c3eb20be307d06a6d6278254ec54654b68698b0e` and the offered head. This is distinct from authored head tree `cbd5f07404c4c6334428fec7f475d4c3759222ac`. Direct tree API blob identities confirm exact offered source for known_faces.rs, memory_scope.rs, remember.rs, second_provider.rs, second_provider/tests/held_forget.rs and tools.rs.

Four selected outer nextest identities occur once PASS before the original summary, with no duplicate or bad selected terminal:

- `lamp-voice / remember::tests::forget_everything_through_the_tools_takes_the_face_only_when_marked` — PASS, 0.027s; original member line 9940.
- `lamp-voice / remember::tests::forgetting_everything_forgets_the_persons_face_too` — PASS, 0.008s; original member line 9942.
- `lamp-voice / remember::tests::forgetting_everything_says_the_setup_name_stays` — PASS, 0.006s; original member line 9943.
- `lamp-voice / second_provider::tests::held_forget::only_an_ask_the_guard_showed_marks_everything_for_the_face` — PASS, 0.153s; original member line 10847.

The fixture durations are whole test durations, not physical latency. Internal cases/child summaries do not add tests. Raw logs, original API responses, source-body hashes and exact outer line references are retained privately. No full-runtime census is claimed.

This evidence supports an independent first-landing fence read of #3564 only. It does not execute or qualify the conditional Common composition, erase the second-lander obligation, disposition #3706, or establish device persistence/physical erase. Root owns any READ-OK/HOLD disposition and public post.

Native collection 67634 exited 0 (650f99); source binding 17599 exited 0 (e7b65a). No local compiler/tests, target/source/ref/index changes, binary artifact downloads, application execution, or posts occurred.
