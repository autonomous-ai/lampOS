# Independent reader evidence for #3344

Authored head `f653bc5bdcdaecb661f27aa076f395dea38474c7` remained unchanged
through the Mac gate: **3,993 passed, 0 failed, 21 ignored**, formatting and
strict all-target Voice Clippy passed. Commands and durations are in
[result.json](result.json); private home paths are redacted in exported text.
All 6,990 captured source hashes match before and after. The checkout was
clean; these are pristine authored-head tests, without review probes.

The independently checked official ARM64 run **40072776658** completed with
**7,350 PASS plus one LEAK**, no failures, 45 skipped. Linux Clippy
**40072776682** passed. [Native audit](native-audit.json) binds the tested
merge, parents, four matching authored files, six additional base changes,
critical executed controls and private full-log hashes. No whole-tree
identity to the authored head is claimed. Exact source-only Linux formatting
is retained separately in [the format receipt](../room-enrollment-linux-format-20261024/).

Source review traced original microphone/transcription proof through the
captured originating lease, request equality, publication lock, current epoch,
cancellation flag and guarded remember transport. Lease revocation marks
inactive before cache cancellation; forget advances the epoch before clearing.
Late attribution cannot mint a new enrollment proof. Disabled mode removes
enrollment, and enabled mode without scope refuses publication.

No concrete new P1/P2 found. A direct authored regression that changes epoch
between initial and late publication remains a P3 coverage gap. No additional
probe, slow-filesystem/camera timing, physical behavior or activation is claimed.
