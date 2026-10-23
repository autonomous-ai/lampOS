# Skills refusal fixture review

Fourth reviewed [#3277 at 1806a691d](https://github.com/autonomous-ai/lampOS/pull/3277#issuecomment-6369896362) on base be16c3327. No P1/P2 finding. Only the private Skills fixture and its English documentation change.

The original exact refusal, failed status and preserved-file predicates remain. Authorized writes and half-closes still precede the callback and remain strict. Rejected-peer BrokenPipe is accepted only after those predicates; NotConnected is accepted only on the paired half-close. Normal return waits for the exact child before parsing, and the guard attempts kill/wait during unwind. Only the child-exit polling has a new 20-second cap; this is not a claim of general bounded fixture I/O or OS waits.

The source and evidence read verified ten published payload hashes, 79 raw stream envelopes, both authored source hashes and all 514 compiled/terminal local case identities. The published controls are 0/1/0 before and 1/0/0 after; package 23/0/0 and full Skills 513/0/1. The exit barrier reproduces a failure mode, not the original remote scheduling. Source diff whitespace passes.

The terminal ARM64 log records 7,266 passed including one LEAK, zero failed and 33 skipped. Both refusal tests and the authorized held-folder control pass; Linux Clippy also passed. The LEAK belongs to the Voice journal test. [receipt.json](receipt.json) binds the heads, counts and observed log hash. This review ran no Cargo, artifact or lamp test. The failed Common run remains failed: a fresh current-head gate is required after First merges the standalone fixture repair.
