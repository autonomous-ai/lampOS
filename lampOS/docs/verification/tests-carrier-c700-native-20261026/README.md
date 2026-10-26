# Carrier c700 native qualification

PR #3466 head `c7006af27` passed [ARM64 run 40309937640](https://github.com/autonomous-ai/lampOS/actions/runs/40309937640) and [Linux Clippy run 40309937656](https://github.com/autonomous-ai/lampOS/actions/runs/40309937656), original attempt 1 each.

Both checked preview `9980ef506`, parents `ba950f289` and `c7006af27`. Its tree `8ba18820a24509abb5517f93de4facd60da48b5d` differs from the authored c700 tree `2871a38df3ad3f0ba8c0f3da9ab258471c9f5310` on base `1aa409fcb`; the identities are not interchangeable.

Nextest reported **11,383 unique passes**, no failures/leaks/timeouts, and **106 skipped tests**, across **182 executed / 2 skipped binaries**, in **210.139 s**. All twelve carrier cases, including semantic GOOD and the comment-only control, executed exactly once and passed. Separate results: promotion 25/0, docs 4/0, custom process 13/0, and relay_signals `ok` once. These are not added to the nextest census.

Artifact 12645280926 retained ZIP: **116,060,343 bytes**, SHA-256 `ec7bdd3da7c938613e8e24b156d775818e3f6c18862221413d7f06264d3aa569`, matching GitHub's digest. Only `lamp-update` and `SHA256SUMS` were extracted. Updater: **4,816,208 bytes**, SHA-256 `72c8f4a06b431a48d68da2e159f6a0dad73871a95cfbb9df4ad4e6db19460732`, matching the checksum manifest; ELF64 little-endian, AArch64 machine 183. It was never executed, and no ARM64 build ran on the Mac. The old b635 artifact remains untouched.

The PR remains draft: real installed GOOD/BAD/fallback/restoration and same-cycle quiet-state evidence are still HOLD. This native result does not settle that acceptance decision. Raw logs, API metadata and downloaded binaries stay private; this record contains reviewed counts and identities only.
