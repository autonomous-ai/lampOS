# Mock Eyes accepted-socket fixture correction

Executed on aarch64 macOS at 1f78ef3b8, rebased to fa82c279e on current
bcb540a08. Complete trees are identical: 8d8c615acf16f1dd865f64e957012220b0bb13c8.
Only the cfg(test) own_name_exchange accepted stream and fixture docs change.
The listener remains nonblocking; accepted streams become blocking before
the unchanged two-second socket read timeout. Assertions, cleanup and runtime
code are unchanged. The private helper census finds no other crate reader.

Mac fmt, strict Voice all-target Clippy and compilation pass. The direct
remember-speaker group passes 16/0. The original full Voice invocation remains
FAILED: library 3,742 passed, one failed, 15 ignored. The unchanged idle-pool
reuse assertion observed two connections instead of one. Cargo stopped there;
its later targets are observed in the separate remaining-targets command.
Exactly three standalone observations of that original failed case pass.
These are qualification evidence, not a rewritten full pass or cause claim.
Literal summaries, exact argv and all primary output are retained alongside
source guards. Compilation streams describe their own stages; executable
hashes are explicitly post-run observations.

The controlled standalone Rust host probe confirms inherited O_NONBLOCK,
WouldBlock on a withheld request, then blocking accepted I/O with listener
polling and the existing read timeout retained. It verifies request/reply and
owned-root cleanup. Cached Rustix linkage is disclosed; this is not an ARM64
Linux build, runtime experiment, physical test or proof of historical cause.
Probe binaries and private audit scripts are excluded. Local paths are redacted
and original/published hashes retained. Native PR checks and independent review
are separate, pending acceptance evidence.

The separately completed targets pass 289/0 with six platform-ignored cases:
main 1, integration 278, test-enabled examples 4 and 6; doc tests 0.
Taken with the original failed library population, this is 4,031 passed,
one failed and 21 ignored across distinct targets, not one successful full
invocation. Direct 16 and the three isolated passes are additional executions.
Pinned offline source-only Linux fmt passes on fa82c279e, all 7,027 files
unchanged and its owned container removed. No local Linux build/test ran.
