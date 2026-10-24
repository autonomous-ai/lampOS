# Corrected Skills deadline and composed-package host gates

| Head | Mac fmt / strict Skills Clippy | Full Skills tests | Tracked source hashes |
|---|---|---|---|
| Rested `e4005f25a` | Both exit 0 | 516 passed / 0 failed / 7 ignored | 6,974 unchanged |
| Composed package `4f44e25b4` | Both exit 0 | 516 passed / 0 failed / 9 ignored | 6,976 unchanged |

Commands use Rust 1.96.0, `--locked --offline`, all-target strict Clippy
and only lamp-skills tests. Both ordinary complete/dribbling reply controls
execute. Native artifact cases remain ignored on the Mac; the two official
selections must execute six and eight cases respectively, filtering those
ordinary controls. No native artifact executable ran locally.

Exact committed source-only Linux formatting passes on each head, with
matching before/after manifests and owned containers removed. Retained
receipts bind the source archive/image/command and cleanup; source archives
are excluded. Private home prefixes are redacted, with original/published
hashes and replacement counts retained. No diagnostic or result is removed.
The first failed compile at a nested caller is preserved separately under
skills-deadline-first-compile-20261024; it is not one of these passes.
