# Original CI audit for #3472 at c0cf52f96

Ready: no. ARM64 failed; Linux Clippy passed. No test or workflow was rerun.

Both runs checked out merge `0f4ba582efc0f17954536bf3a69e8fd90e605aad`, tree `0f1556bbf4d0488503c99df754758197b142d7cd`, with parents `0d1d1f2b944581169c9f96adba9f40ab88adba92` and the published head `c0cf52f9690d178f81d105e31e22c7694cc5738f`. This is not the current working tree. All six required-control source files match the published head; 13 of 15 relevant files match. The two upstream differences are the product Voice journal-resume setting and switches documentation; enrollment settings are unchanged.

ARM run 40541161543, attempt 1: 9,067 unique outer passes, one failure, zero leaks or duplicate terminals, 106 summary skips. Of 11,760 selected tests, 9,068 ran and 2,692 remained unrun. The nextest step exited 100; the native watch handle 5776 exited 1. Linux run 40541161690 completed successfully, native watch 12314 exit 0.

Twenty of 21 required controls passed exactly once. `remember_speaker::tests::remember_speaker_wire_uses_internal_evidence_and_old_eyes_never_get_a_retry` has no terminal: it was not credited after fail-fast. The local 0063 evidence remains separate; the only local-tested-tree to published-tree change is the verification README.

The single failed case is `remember::tests::after_a_forget_the_next_conversation_is_journalled_and_ended`. Its nested child failed at `remember.rs:17068`: the old session closed before the new session opened, while the fixture expected the reverse. Its parent propagated that result at line 16867. Source lines 5467–5469 queue the old close and then open the new session synchronously; the independent writer handles that queue. The observed operations are therefore consistent with a legal cross-thread ordering that this fixture rejects. This source-based diagnosis does not credit its remaining assertions or dismiss the failed gate. The journal-resume fixture belongs to upstream N13 work, outside this enrollment change. Preserve the exact methods and session identities, append/end order, source IDs and inhibited-state assertions when repairing its ordering check.

Docs, relay, process and artifact export stages were skipped. The artifact API exposed no artifacts, so no failed executable bytes or binary hash could be retained; there is no new Voice/Eyes pin pair. Original raw logs and the actual failed-source blob are retained privately. No earlier failures were relabelled. Draft, current-base and physical holds remain unchanged.
