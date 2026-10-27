# Superseded onboarding host scope: interrupted result

Not qualified. No passing full gate is claimed.

The frozen source was Main `bf6f7bbdf5e936c2ef82621e1bb7592d511c1453`,
working tree `9ab408b27bf1471382b58e35cca2fb4967af0e10`. Its separate compiler
procedure completed all five stages with exit zero. Discovery succeeded;
two earlier admission procedures exited 125 before any test body.

The later broad runtime was intentionally interrupted after its scope became
obsolete following the exact onboarding wording merge. It spent most of its
time on unrelated Update round coverage; that local scope was too broad for
this activation. The next gate uses the current composed source and focused
local controls, with fresh ARM64/Linux checks as the broader gate.

The [actual receipt](receipt.json) retains controller **125**, nextest **100**,
**745 clean passes**, **two raw failed terminals explicitly reported SIGINT**,
and **4,874 selected cases unrun**. The [two interrupted cases](interrupted-cases.json)
retain their original events. No functional failure cause or passing result is
inferred from interruption. Eight JSON ignored terminals and 368 summary skips
remain separate; skips are not reclassified. Doctests were unrun.

All 115 input backings were independently copied and verified before the signal;
the interrupted Round executable has its own retained witness. Full source and
owned-process closure passed. Raw logs and binary backings remain private.
No old partial result qualifies the new composition, activation or a device.
