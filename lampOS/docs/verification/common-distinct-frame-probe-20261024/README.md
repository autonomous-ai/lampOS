# Common distinct-frame correction: helper probe only

The production SpeakingFaces reader converts a transient bus read error to a missing sample and may then reread the same unexpired persisted publisher frame. The old `look` method cleared its dedup key on that missing sample. One frame, missing, same frame became three looks with two positives; repeated gaps could also satisfy requested enrollment with only one original frame. This is a source-reachable read-error/recovery trace; no bus fault or lamp experiment was executed here.

Original carry tree `a4c9ffa283506dc79576d74b3aed1729b37c92ff`; corrected staged tree `665d44694def1048095deb236a27d9677dc084fa`, base `9872cb69288b9e113bb95e6b25140a60a1e91e19`. The active old source was not edited. `correction.patch` gives only the new three-file correction relative to that original carry: the reader bookkeeping, three crate regressions, and matching English/Vietnamese documentation. The corrected full branch is not yet qualified or pushed.

## Observed diagnostic outcomes

Using native Mac Rust 1.96.0, the exact extracted `look`, `positive_speaker`, `enrollment_speaker`, data structures and qualified-ID predicate were compiled with minimal message-type scaffolding. The helper source-section hashes and whole source-file hashes are in `before.json` and `after.json`; both source extracts, raw compile/test streams and explicit exits are included.

- Before: **2 passed / 2 expected failures / 0 ignored**, exit 101. The read-error/recovery trace wrongly admitted private disclosure; the repeated-gap trace wrongly admitted enrollment. Existing consecutive-repeat rejection and the genuine two-fresh-frame case passed.
- After: **4 passed / 0 failed / 0 ignored**, exit 0, using the same four assertions and scaffolding. A recovered identical frame is ignored; two distinct valid frames still work.
- Initial scratch setup first used the wrong source parent / unavailable PATH rustc, then lacked PartialEq on the scaffold enum. Those setup errors were corrected before the recorded before/after test runs. They are not source defects or test passes.

The fix retains the last publisher boot/sequence across missing reads. Missing-sample rate limiting instead examines the last recorded look, preserving its 500 ms bound. It adds no I/O, wait, worker or queue; the reader's 100 ms poll and existing bounded history remain unchanged. The action-to-proof path is the same off-audio-path reader followed by the existing final speaker check. No first-use, repeated-action or slow-dependency latency was measured on a lamp; no hardware acceptance is claimed.

These are isolated exact-helper diagnostics, **not a full crate, native ARM64, actual Eyes/bus, identity, liveness or lamp result**. The three newly added crate regressions have not run in this packet. Full Mac/Linux formatting, strict Clippy, builds and package gates, plus exact library regression controls, are being prepared against the corrected frozen source. Privacy reads, physical acceptance and the voice-freeze merge hold remain. The older source's Linux gate was deliberately interrupted and its Mac gate failed; neither qualifies this correction.

No Python runner or binary is committed. `files.json` binds every other file in this packet; raw streams and patch whitespace are preserved.
