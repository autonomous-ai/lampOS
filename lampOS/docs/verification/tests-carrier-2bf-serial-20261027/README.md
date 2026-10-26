# Carrier 2bf complete serial qualification

Head `3be8fa45be42999980d29c1fac7c0971ee75b1a2`, authored-series base
`2bf5122dc76999f970556bb7c78840ab6e107ce0`, passed full Mac Update
**442/0/6** once serially in **2,020.191 s**, with all **448** actual
target/case pairs accounted for and zero ordinary filters. Separate promoter
qualification passed **25/0/0**, all its actual targets once serially, in
**0.449 s**. Documentation results remain separate. The native controller
exited **0/PASS**; no runtime was reused or retried.

Formatting, strict all-targets Clippy for the actual fourteen-package closure,
and workspace build passed before package-test compilation. Supported cleanup
was limited to Update, Setup and Body; only their strict artifacts were
required fresh:false. Other artifact freshness/profiles/features are observed,
without a blanket freshness claim. All **16 actual freeze events** are bound
in [audit.json](audit.json): one workspace normal-helper phase variant, ten
Update compiler executables and five promoter compiler executables. The
workspace helper is historical once package-test compilation selects its own
helper; it is not mislabelled as a concurrently unchanged current program.

All **12 carrier**, **35 head-touch**, **16 Round + 2 library Setup-lifecycle**,
and **9 head-preflight** controls passed once. These are subsets of the
ordinary case census. Round passed **195/0/0**. Its actual binary is
**16,984,844 bytes**, SHA-256
`e512543932ceff7d15bc9bb00cd6503cb5dbea419012bab08dda61524ae334db`,
not the historical a29 binary. All three earlier failed self-cut names passed
in this serial source; their full names remain in the audit.

All 10,676 tracked source files, modes, mtimes and index stayed unchanged.
All **231 prior + 16 new = 247 sealed copies** passed the original guard's
recorded content/mode/mtime checks. Current executable hash/size/mode/mtime
guards also passed. This packet does not retroactively claim full inode checks
that the historical controller did not perform. All owned process groups
closed, no unresolved handles remained, and the compiler lease was released.

The [original sixteen-thread 439/3/6](../tests-carrier-2d-parallel-failure-20261026/)
remains **FAIL**, without a cause or waiver. The
[unmodified Main Round control](../tests-carrier-unmodified-round-control-20261026/),
[isolated three-case control](../tests-carrier-candidate-three-control-20261026/),
[auxiliary launch probe](../tests-carrier-shell-launch-diagnostic-20261026/)
and [earlier 008 serial qualification](../tests-carrier-008-serial-20261027/)
retain their actual source and workload scopes. The later `63a/a3b` gate is
separate. No current native, device, idle-host or performance claim follows
from this prior serial proof. All outcomes, actual executable hashes and
private receipt hashes are exported without publishing binaries or raw logs.
