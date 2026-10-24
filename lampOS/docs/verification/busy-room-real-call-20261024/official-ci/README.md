# Official carry CI

For #3380 head `b33ade24771fb8e72f56df4ca1155eb591534d45`, both official
runs completed successfully: [ARM64 40093839677](https://github.com/autonomous-ai/lampOS/actions/runs/40093839677)
and [Linux fmt/Clippy 40093839666](https://github.com/autonomous-ai/lampOS/actions/runs/40093839666).
The latter's actual pinned whole-workspace formatting and strict Clippy
steps both succeeded.

The selected parallel PR nextest profile gives **7,373 PASS + 1 LEAK**,
53 summary skips, zero failed/unrun, across 12 packages. Ordinals 1–7,374
are complete; repeated rows printed with captured output are deduplicated
only when identity and result agree. All three required literal-name and
busy-room controls pass exactly once in that census. The journal leak is
retained as LEAK, not silently converted to an ordinary pass. Separate
turn-end-wire harness output reports 12/0. These are not full realtime or
all-package test counts.

Checked merge `1bd5ce7b951ca415e0e386f3b1b830db30cfe481` integrates this
head onto `2e2eee360`; its entire repository tree equals the authored tree
`36be6093499933827764110ef03be3d94a87326a`. No later base is qualified.
Official metadata, complete unique nextest row census, selected exact log
lines and hashes of both full private-retained logs are included. No
binary or archive is published. This does not qualify played-room,
acoustic latency or physical behavior; First retains that acceptance.
