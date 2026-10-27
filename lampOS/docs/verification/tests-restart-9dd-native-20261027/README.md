# Restart 9dd original native CI

Original pull-request ARM64 [40471968680](https://github.com/autonomous-ai/lampOS/actions/runs/40471968680) and Linux Clippy [40471968675](https://github.com/autonomous-ai/lampOS/actions/runs/40471968675) both succeeded, attempt 1, exact offered head `9ddce42c961044c63bd6f9969d658a5339f092f3`. The bounded watch closed at 854.34 seconds, native session 23735, exit 0. No CI rerun occurred.

**Actual native census: 7,630 unique PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT, 0 selected unrun; 53 aggregate skipped.** These are PR-profile release results for 12 selected packages, hash partition 1/1, with realtime-group tests included. The header reports 70 selected binaries; execution records contain 63 nonempty binary labels. Individual skipped identities were not emitted and are not invented. This is the recorded selection, not a whole-workspace runtime claim.

The execution window is raw lines 880–8512, with all 7,630 unique identities and ordinals accounted for and no nextest TRY/RETRY/FLAKY records. There are 34 post-summary reprints: 22 realtime and 12 privacy-worker, each matching its earlier status, duration and ordinal and the actual nextest success-output=final rule. They are excluded by the execution boundary, not blind deduplication. Internal fixture retry loops are not ruled out.

**Required cases:** all 21 source-derived SameVoice tests passed once, including the changed hung-child case and journal-worker case. All 14 ordinary Linux Reader controls passed once. The privileged isolated-root/setpriv DAC test remains explicitly ignored, and the non-Linux fallback is not compiled. No privileged-DAC acceptance is claimed.

**Actual source:** both raw job logs checked out preview `12037cbcfe0b758ebe7fc4ec5aba4890d96ffcea`, ordered parents `[75571bd0a439cc2dd4b18e0f4b6c3de9ff3153a8, 9ddce42c961044c63bd6f9969d658a5339f092f3]`, tree `3326069f80b3f0388181eb27cbe41b033ba1574e`. Offered head's parent is `f9a9404de` and tree is `edb4de88bba622c1c5f96756ee8f33e4e6c4e7ee`. Both authored full blobs (`same_voice.rs`, `same-voice.md`) equal the preview. All 26 tree differences and seven immutable text/config bindings are retained. Offered/preview tracked entries are 10,720/10,723; no binary equivalence is inferred from source equality.

Linux ran whole-workspace fmt and strict all-targets Clippy, plus separate promoter checks. ARM's separate promoter 25/0, workspace docs 4/0, custom process 13/0 and relay one successful result are not added to nextest. Exact-head checks are six success and three workflow skips; the skipped promotion/sign-publish/report jobs are distinct from Rust's 53 skips. No artifact API, archive or payload was requested.

The independent historical Cloud witness remains 11,458 PASS and one LEAK, run `40439732017`, head `553e0d65`, actual preview `ac5bd8f6`. It is not relabeled as this Restart result, clean PASS or causal proof. Current source differences and prior host/resource outcomes remain separate. This audit makes no full Voice host, device or historical-cause claim.

New discussion since the push contains only the author's status comment, no new external issue/review/inline finding. Draft/read readiness is Root's separate decision. No GitHub write, source/ref/target/compiler/runtime or device action occurred.

The post-close final file manifest covers every evidence file except itself and its verification receipt. The earlier collection manifest is retained as the audit producer's input inventory; the final manifest is authoritative for end-state hashes.
