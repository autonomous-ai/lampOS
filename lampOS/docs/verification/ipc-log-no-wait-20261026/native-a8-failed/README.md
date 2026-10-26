# PR #3528: original native failure on current base

Head `a8d6a4970dcf73a9007850703d9f27af24ea74e5`, base `1c884f16cf4de53a700bba7512c129c3ee01d72d`. Both jobs checked preview `7b528af489ed64e4acc0a4f8b1b47e727a79013b` with those exact parents; preview tree `66c720ba130835604fabe3c8d454921641e1ef56` equals the authored tree.

- [Original ARM64 attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40260992363/job/121373140459): **FAILURE**. Nextest selected 11,345 and executed 10,041 unique cases: **10,040 PASS / 1 FAIL / 0 LEAK / 0 TIMEOUT**; **106 profile-skipped** and **1,304 selected but unrun after fail-fast**. The 24 replayed display rows are excluded.
- The failure is `lamp-voice session_reader::tests::linux::replaced_record_never_changes_an_outside_sentinel`, at `lampOS/crates/voice/src/session_reader/tests.rs:160:53`: an unwrap received `PermissionDenied`, `session object changed during ACL inspection`.
- The changed IPC full-journal case **passed exactly once**, ordinal 3081. This does not make the failed workspace gate green.
- Workspace doctests, relay/process harnesses and artifact export/upload were skipped. There is no result for those steps on this attempt.
- The earlier separate promotion-helper command passed **25/0 overall**, groups **2,0,22,1,0**. 22 is only the promotion integration component.
- [Original Linux Clippy attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40260992391/job/121373140725): **SUCCESS**.

The failing test file is byte-identical at this head, its base and the preceding 6e177 head. The preceding original ARM64 run 40258770126 passed that same case. This is context, not a diagnosis or base-only reproduction. No rerun, local compilation or mutation was performed for this audit.

Raw logs/metadata remain private, bound by hashes in results.json. Native watchers used a 300-second interval and one-hour deadline. No physical latency, persisted journal delivery, release publication or device acceptance is claimed. The host continuation proof and all prior native packets remain separate and unchanged.
