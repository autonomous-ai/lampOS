# Third’s independent requested-audio boot read

PR #3319, `8b54c1c32`, containing `9ed44f88c`. Read-only isolated Mac
checkout, Rust 1.96.0. No device, provider or real playback action.

No source P1/P2 found. `process::spawn_with_tick` allocates a distinct
worker boot; command arguments carry parent then worker, and `main` fills
`WorkerBoots` by name. `serve` gives the coordinator boot only to `run`
and its `Gate`, while memory-scope keys and requested-audio contexts share
the worker boot. The changed internal scope key has no persisted or other
process reader; both audio comparisons remain consistent. The authority
guard still rejects a foreign boot.

The diagnostic preserves all 15 admission conditions and first-refusal
precedence; failed preparation still consumes the original one-offer attempt.
Reasons are static strings, not model, asset or call-ID content. Existing
playback, privacy, cancellation and retirement fences remain.

Nonblocking P3: the new test calls `requested_audio_refusal` directly. It
does not prove the coordinator event path actually emits `accepted:false`
and the exact `refused` field. Removing that emission would leave the new
test green. Add a trace-level event-path control with a data canary in the
separate regression follow-up. The distinct-boot composition through real
`main`/`serve` likewise remains source-read, not executed by these controls.

Mac formatting and strict all-target Voice Clippy passed. The first
focused attempt was **29 passed / 24 failed** because fixture Unix-socket
binds received `PermissionDenied: Operation not permitted` in the sandbox.
That result and its source hashes are retained, not erased. The same source
with socket permission passed all **53 audio controls**, then full
turn-owner **37/0**, serially; source hashes remained unchanged. Commands
and exits are in the two JSON receipts, raw test output beside them.

Reviewer-two separately reports full Voice and native passes on this head.
Third does not claim to have rerun those whole gates or the actual played
entry path. The author’s exact-build played result and requested commit fold
remain qualification work under reviewer-two’s existing HOLD. This read
does not lift it or qualify the unchanged Live route.
