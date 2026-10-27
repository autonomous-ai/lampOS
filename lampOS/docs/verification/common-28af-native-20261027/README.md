# Common 28af native CI and artifact integrity, 27 October 2026

Ready for lamp-voice: no

[ARM64 40513174138](https://github.com/autonomous-ai/lampOS/actions/runs/40513174138)
and [Linux Clippy 40513174060](https://github.com/autonomous-ai/lampOS/actions/runs/40513174060)
completed successfully, with supported watcher exits zero. Generic CI
[40513165486](https://github.com/autonomous-ai/lampOS/actions/runs/40513165486)
also passed. Published head is `28af4573a66c2844cdd0a91936d2eca8c566fd5b`,
authored base `36a42f30a756b6fd6f31c9b6102cb66e5008416c`, tree
`364776260748a4c355cf625768c2065a32f510d0`.

Both native checkout logs and the commit API bind the tested source to merge
`0bacfde9a344c1b7cd07d21cde6a23b1967daaa5`, tree
`66435569942c9118e5bf84ee1e3dad89448e8b7a`, with parents
`bf6f7bbdf5e936c2ef82621e1bb7592d511c1453` and the published head.
The [receipt](receipt.json) does not qualify a mutable current worktree or later carry.

ARM64 records **11,922 unique outer passes, zero failures, leaks, timeouts or
duplicate pairs**, and **106 summary skips**. Original GitHub step logs reproduce
the exact outer identity set. Repeated final statuses and nested libtest output
are not extra executions. No raw nextest JSON/discovery or complete skipped-case
identity census was exported, so skipped identities are not invented.
Profile `pr` selects `all()` in the pinned configuration; stale workflow comments
are not the selection evidence.

All [112 required controls](required112-terminal-ledger.json) and all
[525 current contracts](current525-terminal-ledger.json) passed once. The 25
overlaps are not added; the mapped union is 612. The mappings select names for
this new log; their historical source qualification remains separate. The separate
[Linux fixture](supplemental-linux-repair.json) passed once. Its eight original
assertions plus one negative ticket assertion cover ticketless-publication refusal
and invalid stale-source cleanup while preserving enrollment authority. It does not
establish valid V3 authority or live-grant revocation. Its 0.004-second test duration
is not acoustic or physical latency.

Separate outputs: **47 workspace doctest targets, 4 passes / 0 failures**;
**one relay aggregate OK**, with no inferred internal case count; and
**13 process cases / 0 failures**. None is added to nextest.

The [artifact verification](artifact-integrity.json) downloaded exact artifact
`12738463524`, 118,424,136 bytes, under a 160 MiB compressed limit and a
payload-plus-2-GiB disk admission. Its full ZIP hash matches API/upload evidence.
The archive is below the 1 GiB aggregate uncompressed cap, with safe member paths
and types. All [50 payloads](validated-export-manifest.json)—48 binaries and two
examples—match `SHA256SUMS` and ZIP CRC, and have ELF64/little-endian/AArch64
headers. No binary was extracted or executed.

| Payload | Verified SHA-256 |
| --- | --- |
| `lamp-voice` | `d63d6e188ee734f2e8bb08a48a654e3a522326af435c30760a50fab7de315854` |
| `lamp-eyes` | `16dd64d986d0d3e01fc038a6f81f04a8448ffd2125aa005a319b56b1b4201063` |

These are the pair from this successful artifact, not a mixed-build compatibility
or installed-pin claim. The [earlier 314 packet](../common-314-native-20261027/README.md)
and its digests retain their own source. Its 11,905 passes supply no credit here.
The [earlier failed ARM run](../common-3e9-native-failure-20261027/README.md)
remains failed: 2,780 passes / one failure / 8,961 selected cases unrun.
Earlier local compiler/runtime/artifact failures remain as recorded. The separate
local 5,595-pass result is not counted as native CI evidence.

Latest-base qualification and current-head readers remain **HOLD**. The
**#3564 second-landing composition P2 and Engine case** and the **#3315
independent privacy disposition** remain separate holds. Core privacy acceptance,
installed matching pins, and lamp, device, physical and recognition acceptance
are not established by this packet. No deployment or Ready transition is claimed.
Raw logs and binaries remain private.
