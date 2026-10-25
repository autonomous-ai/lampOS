# Late-verdict carry after train 131 and the Actions clock change

Two execution scopes are kept separately, on native Mac with Rust 1.96.0:

- `8b9debb80` on `0ddd07377`: formatting, strict all-target Voice/VoiceEval
  clippy, workspace build and full default Voice/VoiceEval tests passed
  **4,675 / 0 / 21 ignored**, eleven harness summaries. All three own
  late-verdict controls ran once and passed. Source stayed frozen and clean.
- `2aca8150f` on `bf39e4188`: formatting, strict all-target
  Actions/Voice/VoiceEval clippy and workspace build passed. All Actions
  schedule tests passed **19/0**, Voice tools readers passed
  **82/0/1 ignored**, and the VoiceEval switches census passed **2/0**.
  Each selection ran nonzero tests. Source stayed frozen and clean.

The base advanced during the full gate by exactly two files:
`crates/actions/src/schedule.rs` and `crates/actions/tests/it/schedule.rs`.
They add the local clock field to schedule tool results and its controls.
The full source-tree difference between the two tested heads is exactly
those files; Voice/VoiceEval sources, manifests, fixtures and documentation
are unchanged. Actions is a Voice dev dependency, not a runtime link. The
second gate re-executes the schedule adapter, real Voice schedule integration,
probe/catalog readers and census against that changed dependency. The full
Voice/VoiceEval execution remains on its recorded 0ddd source; it is not
relabeled a new execution on bf39. Original full Linux CI must qualify the
final offered composition separately.

Both own commits have equal range-diffs across each base update. The first
retains the reviewed three-file behavior; the second carries older verification
records unchanged. This new folder is evidence only. Commands, UTC times,
exit codes, literal summaries and retained raw-log hashes are alongside it.
One compiler job and serial tests used the existing owned cache, incremental
off, with development/test debug info zero. No all-fresh or independent
pre-execution artifact attestation is claimed. No disk error marker was found.

Earlier failed runs, restricted/load failures, the non-reproducing natural
chronology, and synthetic continuation controls keep their own source and
failure limits. This gate does not erase them or establish natural production
reachability or voice-print calibration. The switch remains off in product
units. No local ARM64, lamp, acoustic-latency or activation result is claimed.
