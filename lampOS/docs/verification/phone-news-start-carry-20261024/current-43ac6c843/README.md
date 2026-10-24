# Current-base focused qualification

Base `43ac6c843`. Head binding and all 7,023 tracked input hashes are retained.
Current Mac fmt, strict all-target Voice Clippy, fresh compilation and focused tests: **1 passed / 0 failed**. Source hashes unchanged. Source-only offline Linux fmt passed and its owned container was removed.

The earlier full serial Voice gate remains bound to bdb95e3af: 4,006/0/21. The rebase changes exactly Expression tests/runtime.rs and docs/expression.md, with no Voice, Cargo or vendor source change. Observed Expression compiler targets are libraries, not its integration test. Literal references to Expression docs are comments only; no changed integration-test reference was found. Whole trees differ; the full suite was not rerun on this current head. Official ARM64/Linux checks and current reader qualification remain separate. No workspace or local ARM gate, historical cause, physical or signature-trust claim.
