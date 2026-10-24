# Corrected Memory survivor P2 read

PR #3333 `0882cb90474edb19f11130c0771d019509eed9cb`: complete returned
A-unrelated/B/room survivor arrays now match their originals across the
opposite, targeted-forget, forget-person and return boundaries. Original
ID/snapshot/count/fresh-write checks remain; B's snapshot excludes A/room
canaries. No new source P1/P2 found.

Independent exact-head Mac fmt, strict all-target Memory Clippy and full
serial Memory **314/0/8 ignored** pass with captured source hashes unchanged.
Both actual-release cases are ignored locally, not relabelled as executed.

Fresh packet eadd636ba7 independently verifies **95 payload hashes**, both
**41-successful-RPC** traces, exact corrected driver hash, complete returned
survivor values/order at every requested boundary, and final logical SQLite
contents/integrity. Actual published-pair result **2/0** uses identical
AArch64 payloads under Linux AMD64 kernel binfmt. Six reaps are fixture
source/terminal assertions, not independent child-PID event telemetry.
Logical SQL/JSON exports are not byte-identical databases; no FTS restore,
release-signature, native ARM-host actual-pair, power-cut or lamp claim.
Lost unpublished runs remain excluded.

Official ARM64 40077748435 **1,530/0,14 skipped**, process **12/0**; Linux
Clippy 40077748328 passes. Preview 00565e085 combines base 411965dcb with
this authored head. Four checked primary files match, but 44 base files
differ, including newer Memory days/protocol code and tests. This is not
whole-tree identity to the authored Mac run; its native result belongs to
the tested preview. Test-only PR integration remains the train's.
