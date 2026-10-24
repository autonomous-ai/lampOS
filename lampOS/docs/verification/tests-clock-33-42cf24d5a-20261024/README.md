# #3190: eleven additional failures, three observations each

Head 42cf24d5a: exactly 33standalone observations passed, three for each of eleven failed full-run cases. The full run remains 7,267 passed / 17 failed / 50 ignored, exit 1. Six failures have separate reviewed frozen-base assertion-phase evidence; the other eleven now meet the agreed three-pass local category. This does not identify any historical cause or make the full suite green. Current-head ARM64/Linux Clippy remain separate gates.

The three full-run test executables, their modes/profile/features and whole-source guards were checked before and after every observation. Each invocation produced exactly one actual passing row. Owned process groups were absent and direct leaders waited. No source change, recompile, resampling, threshold change or additional observation occurred.

Two Memory health cases launch compile-time child paths whose bytes the later workspace build had replaced. Exact retained reader children were frozen privately, staged at only those two compiled paths for this batch, and verified before/after every observation. Finally the saved workspace bytes and original modes were restored and independently rehashed. The batch used exact reader artifacts; no equal-feature-union claim is made for the differing workspace builds. In-process fixtures and the PATH-resolved system cat have separate scope/identity guards.

Every command, original stdout/stderr, source/artifact identity, staged/restored path transition and final receipt is retained. The JSON controller wrapper is external measurement code, never a runtime dependency. Binaries stay private and are not committed. Signal/error cleanup is recorded; no unconditional SIGKILL, host-crash or storage-durability guarantee is claimed.
