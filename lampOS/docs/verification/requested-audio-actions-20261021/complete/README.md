# Complete host gate after the Actions speaker-publication correction

**This gate failed: process exit 1.** All 17 stages completed. The complete
36-consumer suites recorded **8,773 passed, 1 failed, 48 ignored and 0 filtered**.
Focused repeats and the four renderer tests are separate from that total.
[host.json](host.json) retains every command, exit, count, duration, failure
identifier and raw-log SHA-256. Exporting the receipts did not rerun tests.

| Stage group | Observed result |
| --- | --- |
| Workspace formatting and strict workspace/all-target Clippy | passed |
| 35 source-only consumer crates | 8,514 passed, 1 failed, 46 ignored, 0 filtered; command exit 101 |
| Complete Update crate in the checkout | 259 passed, 0 failed, 2 ignored, 0 filtered |
| Workspace build | passed |
| Renderer tests | 4 passed, 0 failed, 0 ignored, 0 filtered |
| Render, package and disposable install | passed; all seven installed files match staged hashes |

The sole failure was
`lamp-eyes::seems_log::tests::a_failed_write_is_said_once`.
It unconditionally opens `/dev/full`; on this macOS host, that open returned
`NotFound` at `crates/eyes/src/seems_log.rs:312`, before the failed-write
assertions ran. The failure remains in the complete-suite result. The other
stages completed successfully; this receipt does not establish a green gate
or native Linux qualification.

The complete gate also passed the full Actions speaker-state integration
suite (23 tests) and the exact Voice
`a_retained_speaker_snapshot_expires_an_actual_ticket_and_cannot_revive_it`
regression (1 test). It therefore supplies the Voice execution missing from
the earlier [focused correction record](../focused/README.md). Additional
focused stages passed: four Skills file checks, one scan-cost check, 12 Voice
integration tests, 50 Voice audio tests, 30 Skills audio tests and one prepared
handoff test. [Scan timing](scan-timing.json) and
[prepared-handoff timing](handoff-timing.json) retain their exact measured
boundaries and host limitations.

The frozen inputs contain 2,204 non-document files on integration base
`bd43346de29bba069f741baa3eb32d03c6d5794f`, with aggregate SHA-256
`1e624e3d1ca0f315399c851ef1076668bcef57b11c86970ce934d70d9a4c2d40`.
Checkout HEAD `c74b0f710ecac48f8668c6ab8258777788068c0a` identifies the starting
checkout; the gate includes four uncommitted Actions/Voice corrections whose
hashes are recorded explicitly. That commit alone does not identify the
verified working tree. The [source delta](source-delta.json) reconstructs the
exact map from the immutable archived baseline, and the
[terminal source check](source-check.json) confirms that every input and all
five checkout-only parent fixtures remained unchanged. The
[consumer audit](consumers.json) records the selected dependency closure.

The [example receipt](example.json) records the 48-second generated WAV,
package and disposable installation. No audio was played and no device was
accessed. The [earlier full-content host record](../../requested-audio-content-20261021/README.md)
and [historical native qualification](../../../requested-audio.md) qualify
their recorded source versions, not these later Actions/Voice corrections.
Physical-lamp playback and acoustic timing remain outside this host record.

[export.json](export.json) binds the seven structured payloads and original
input receipts. [SHA256SUMS](SHA256SUMS) covers this README and all eight JSON
files. Full raw logs, the whole source map and generated binaries/audio are
not duplicated here; their relevant hashes remain in the compact receipts.
