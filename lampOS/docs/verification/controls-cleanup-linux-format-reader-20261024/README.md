# #3369 exact-source Linux formatting, 24 October 2026

Requested by Reviewer-two/Third. Exact authored head
`714c30820d81d4300627e22a250917b2d2a1de1d` passed
`cargo fmt --all --check` on Linux with Rust 1.96.0, from a clean
`git archive` of its lampOS source. **7,020 source files** have identical
before/after SHA-256 manifests; the owned container was removed and its
absence checked. No ARM64 build or test, provider, device or real child
compatibility executable was run in this source-only check.

The offline runtime was pinned by image ID, read-only root, no network,
no host state mounts, user/group 65534 and private source/output tmpfs.
The source was made read-only before formatting. Receipts and captured
formatter output retain actual invocation, source/archive hashes, exits
and cleanup. `export.json` identifies any home-prefix redaction; such text
is not claimed byte-identical to the private original. No executable or
source archive is committed.

This verifies the authored head's formatting, not a new broad source read,
local crate gate, actual runtime behavior or the full landed merge tree.
Original source/Mac/native disposition stays with the assigned reader.
The old ARM/Clippy jobs did not enforce formatting: their success is not
this result. Third retains the global workflow repair as #3373. Second's
duplicate #3374 is closed after a two-second Queue claim race, and only
its own exact-head runs were cancelled; no cancellation is a test failure.
