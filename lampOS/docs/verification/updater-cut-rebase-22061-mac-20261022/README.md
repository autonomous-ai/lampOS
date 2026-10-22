# Updater cut tests: rebase onto 22061b427

Original PR [#2885](https://github.com/autonomous-ai/lampOS/pull/2885) was
pushed as `82651436fce1e851e541c95b6ba1e005eeada15c`, parent
`22061b427805e4f6131cd5b15f2f1f287107fdf2`. Reviewer subsequently carried
it with an equal range-diff into merged [#2908](https://github.com/autonomous-ai/lampOS/pull/2908).

The [authored range](range-diff.txt) remains equal. The whole Update tree
changed only in inherited `tests/install.rs`: #2860 added the speaking-face
product-unit fixture. Production Update source and owned cut fixtures are
byte-identical to the preceding head. The initial tree guard stopped before
gating or pushing; [composition-review.json](composition-review.json) records
the reviewed integration before the fresh gate.

Fresh Mac Rust 1.96.0: formatting **5.335 s**, strict Update all-target Clippy
**13.379 s**, complete serial Update tests **298 passed / 0 failed / 2
ignored / 0 filtered**, **278.500 s** including compilation. The extra pass is
the inherited install fixture. All **33** scoped source/manifests/doc hashes
match before and after. Exact commands, results and log hashes are in
[mac-gate.json](mac-gate.json). [Native proof](../updater-cut-rebase-22061-native-20261022/README.md)
qualifies this head separately from the later carry.

The original twelve controls remain historical executions. Their selected
service-entrypoint failure leaves retained copies or the fixed fallback;
#2810's stronger acceptance wording remains First's decision. The
post-refusal/pre-cleanup prefix is reconstructed, not an actual kill at that
instruction. No power-loss, fsync, daemon, device or latency claim is made.
