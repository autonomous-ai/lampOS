# Body loop compilation source read

Public PR #3449 head `f430a182ba081caab3840356fec2f9b1b5c9ca1f` on `056a76132354f40f97eb5fbc519778c04f1ccb19`. Exact diff: one file, 18 added lines. Source-only disposition posted at issuecomment-6394261995, no P1/P2 found.

An independent reconstruction removes the thin wrapper and new `run_on_dyn` signature from the head and exactly reproduces the entire base body.rs byte sequence. The wrapper forwards all arguments in their existing order. The three mutable FnMut references erase closure types; port/runtime remain generic. No new thread, queue, allocation, cancellation decision or actuator policy appears in the diff.

The receipt binds both file hashes and reconstruction assertions. No independent local compiler/test gate ran, no runtime dispatch cost or hardware latency was measured, and no READ-OK, device acceptance or merge decision is claimed. Those boundaries stay separate from the author's CI and compile-time measurements.
