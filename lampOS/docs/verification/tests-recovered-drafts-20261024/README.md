# Recovered Tests draft sources

Ready for lamp-voice: no. These are durable source snapshots and prospective controls, with no new test/build qualification.

Temporary Tests worktrees and unpublished evidence disappeared during the 24 October interruption. The three drafts were reconstructed from complete original edit operations and verified against their recorded hashes: privacy **7 files**, sign-in **8 files**, same-voice cleanup **2 files**. No missing source bytes were guessed. Complete original session transcripts are not published; these records retain only the owned source snapshots and control patches.

- **Privacy (#3315):** default-off Bus subscriber fixture clocks plus test-only LeaseReader clock ownership, preserving ordinary system-clock constructors, all freshness/silence/rescan/gap limits and original assertions. Both prospective host-delay controls are unchanged and unexecuted.
- **Sign-in (#2839):** the previously approved atomic Tasks code-claim correction and direct/queued/provider-loss fixtures, preserving its explicit delivery/revocation limits. All eight final hashes match. Pinned standalone rustfmt restored an earlier recorded formatting transition byte-for-byte; this recovery is not a new formatting gate. Legacy control v3 is unchanged and unexecuted.
- **Same-voice (#1569):** explicit owned completion for the three existing child fixtures, preserving worker/child bodies and assertions. The held-cleanup control is unchanged and unexecuted; the five-second watchdog does not hard-bound OS join/kill/wait.

The full old privacy validation-plan files were not recovered completely and are not certified by this record. Fresh plans, formatting, controls, applicable crate/dependent tests, Clippy, workspace build and official native CI remain required before each source PR is offered. The erased #3218 infrastructure attempts have no qualifying complete census or terminal result here. Published #3190 qualification remains intact in its separate records.

The frozen #3190 candidate is now public at `tests/archive-3190-qualified-93c995a6-20261024`; its original matched base remains its parent, closing the fetchability P3 after the native report was written. Historical raw-log digest limitations remain explicit. No local ARM build, lamp install or played action ran during recovery.
