# Common current ARM artifact audit

ARM run 40101806082 succeeded for authored e9d00f935029a6d5054ddf35b0203fb8546f7995. Actual checkout: merge d06dd68881511d598968672746c3ebe979fa5c58, tree ae77c544a43f3c9dc43f031aa65d1896979875ec, parents 22b753df5db6434d21455f40f67926e0ecee9d91 and e9d00f935029a6d5054ddf35b0203fb8546f7995. Authored tree: 9c78587ee3e8bcd3cb865e6ed5c564a71f36cc5f. Whole trees differ. Of 36 owned paths, 35 match exactly, including 21 of 22 Rust paths. The only differing owned file, second_provider/worker.rs, changes cfg-separated test-clock imports, a test helper, and test clock calls. Its production private functions are unchanged. Exact diff and blob IDs are retained.

Artifact 12563657556 has 116,072,333 ZIP bytes. API and upload log agree on ZIP SHA256 9bbcb2cf219d0c23078c8d2be84d6cbd64313dc0680d68c4c977355faaba35d1. That ZIP digest was not independently recomputed: gh run download extracted the files. Independently checked every one of the 49 payload checksums and headers: all ELF64, little endian, AArch64 machine 183, type 3. Inventory: 47 current workspace binaries and 2 selected examples (schedule_check, replay_cost). SHA256SUMS is the 50th file; no extra files. Full artifact remains private under artifact/. No payload was executed.

Nextest: 11,371 unique cases passed, including 1 leaky pass; 0 failed; 106 skipped, with 2 binaries skipped. Leak: same_voice::tests::each_line_the_check_says_is_said_to_its_journal. There are 11,406 raw result lines because 35 identical results reappear in final stdout sections. Every duplicate was compared exactly before deduplication by position and identity. The separate process gate passed 12, failed 0. Nested Cargo summaries are not independent counts.

Selected controls: 139 distinct PASS results. Breakdown: memory_scope 34; speaking_faces 20; kept_name_callback::privacy 9; lookaway_authority_tests 19; pending_context 2; private_output_tests 1; private_context 31; second_provider remember_speaker 7; remember_speaker 16. Exact identities are in selected-control-results.json. The new combined real-callback-to-HTTP follow-up test is absent from this authored head and receives no credit.

Clippy 40101805913 succeeded, including pinned whole-workspace formatting and every-crate/target warnings-as-errors steps. Cloud 40101805980 and Surround 40101806045 succeeded. Their retained API metadata identifies authored e9d. ARM build/test succeeded; its signing/publication job was skipped.

Matching candidate pair from this same artifact:

- Voice: 28,744,208 bytes; SHA256 2728f0475f704650a2bf643847c3ced0b635ecb83437015d8d81159728d75243.
- Eyes: 26,432,592 bytes; SHA256 73b54aeb010839e23881b2a7124d0b829c491f74f855c5b3b4f68f06f29cd76f.

This pair is a source-bound candidate recommendation only. Pins and hardware remain HOLD. No installation, normal startup, camera, classifier, identity/liveness, or physical latency evidence is claimed. No formal READ-OK or activation recommendation.

The artifact contains no embedded source provenance or reader JSON, only the payload and checksum manifest. The retained chain is API run/artifact metadata, selected checkout log, current-Cargo-JSON packaging commands, independent payload checksums, and source blob comparison. It is not a signed attestation or independent reproducible build. The download handle was gone after compaction, so no native terminal receipt is asserted; extracted content completeness was independently checked. No source, index, refs, cache, target, container, or device was changed.

Public API metadata keeps execution and source fields while omitting actor/author identities and signature payloads. The public inventory hashes these selected views; no signature or archive recomputation is implied.
