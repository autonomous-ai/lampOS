# Phone gate: newly observed fixture-directory collision

The completed phone full-Voice command failed with 3868 passed, 2 failed and
15 ignored. The library had 3588 passed, 1 failed and 15 ignored. Its additional
failure is `tools::tests::scoped_audio_rejects_revocation_and_mixed_request_keys_before_dispatch`:
`create_dir(...).unwrap()` at `tools.rs:2270:36` returned OS error 17,
`AlreadyExists`. The changed messages test passed; the separate light-call
failure and the overall failed qualification remain in the original receipt.
This audit neither reran nor waived anything.

`tools.rs` is identical between exact base `e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc`
and the uncommitted two-file phone candidate. The allocator is inside
`#[cfg(test)] mod tests`. It names a directory
`lvs-audio-scope-{process ID}-{fractional nanoseconds in hex}`, omitting whole
seconds. It calls strict `create_dir` without a uniqueness counter or collision
retry. Two parallel-capable tests share this label: the live-lease test calls
it four times and the rejected-dispatch test twice. Cleanup happens at the end
of a successful iteration. There is no cross-test ownership reservation beyond
successful directory creation.

The panic proves an existing pathname at creation time. It does not identify
that pathname, process ID, owner, iteration or creation time. The displayed
Rust test-thread ID is not a process ID. A repeated timestamp within the same
process and a stale directory after PID reuse are possible explanations, not
established causes. No existing temporary directory was inspected or removed.
The failure precedes the affected iteration's catalog/socket setup and its five
original dispatch assertions; it does not itself demonstrate a dispatch defect.

The earlier exact-e5 full invocation passed this same test. Two older retained
before/after rows also passed. No prior matching failure was found in the
bounded archive/log search recorded here. Therefore this is newly observed in
this audit, not classified as inherited. Source equality alone cannot explain
its occurrence. Captured `file` outputs identify both exact-e5 and phone library
artifacts as Mach-O x86_64. Their full recorded executable SHA256 values differ;
the shared filename is not proof of compiled identity. Actual target files were
not read during the active reader gate.

## Evidence

- [Complete panic diagnostic](failure-diagnostic.txt) and
  [extraction metadata](failure-extraction.json).
- [Exact completed commands, totals, selected rows and captured artifact identities](observations.json).
- [Source identities](source-identities.json) and
  [line-numbered fixture excerpts](source-excerpts.txt).
- [Earlier observations, bounded history search and ownership context](history-search.json).

The line-numbered source projection omits the trailing display space after a
line-number colon when the source line is blank. The
[original projection bytes](source-excerpts-original.json) are preserved
losslessly as JSON text with their SHA256. Exact source hashes and all actual
diagnostic bytes remain unchanged. This is the corrected first publication;
the original external audit remains intact.

The complete raw command streams remain at the external paths in
`observations.json`, with byte counts and SHA256 values verified during this
audit. This compact package contains the complete relevant panic block, not
those full Cargo streams or executable binaries. No mutable top-level gate
results, live phone source, reader output or actual target artifact was read.

## Prospective control only

If a separate fixture-allocation piece is claimed, freeze a deterministic
old/new control before execution: supply an identical PID/time-derived seed to
two same-label fixture allocations and keep the first allocated directory alive
through a bounded handshake. The old allocator must produce this exact
`AlreadyExists` setup failure; a proposed unique-ownership allocator must return
distinct directories without adopting or deleting the first fixture's contents.
Also pre-create that same candidate path to model a stale collision separately.
Keep the existing five case assertions, socket behavior and all real bounds.
This would establish the naming mechanism under controlled inputs, not prove
which interleaving caused this observed failure. No repair, control, test,
cleanup or new claim was made by this audit.
