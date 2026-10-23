# Voice fixture carry onto ae4ab0022

The owned branch moved from `735b2cb5d` to `157b800a1` by a clean rebase onto
`ae4ab0022`. No conflict or manual edit occurred. The original and rebased
source patches, complete changed-path list and range-diff are retained here.
`source-patches.json` stores both complete patches as lossless UTF-8 strings
with byte counts and SHA-256 values. Encoding each `text` value as UTF-8
reconstructs its original patch exactly, including the blank context-line
spaces required by patch format. This representation keeps those spaces out
of repository whitespace checks; none of the evidence bytes are trimmed.
Authored added/removed lines are identical across all 104 changed paths;
96 historical docs/evidence blobs are identical. The range-diff difference
is upstream context around the DeclineMusic `_one` guard.

`source-proof.json` records exact heads, eight Rust paths/hashes, 204 preserved
assertion macros, production-prefix checks and preserved upstream fixture
functions. This is a scoped source comparison, not a test execution or a
claim that historical failed gates have passed. The earlier snapshot's
`cargo_tests_builds_targets_push_log_edits: false` describes that source audit;
subsequent qualification is recorded separately. The historical metadata in
`before.json` likewise describes preparation, not current PR readiness.

Run `shasum -a 256 -c SHA256SUMS` to verify every payload file here.
