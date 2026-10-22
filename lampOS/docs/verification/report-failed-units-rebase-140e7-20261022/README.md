# Failed-unit reports: final authored rebase

PR [#2764](https://github.com/autonomous-ai/lampOS/pull/2764) was rebased and
pushed as `0b125f49b20d8a48c45eca0507383076482865d5`, parent
`140e7ac066062d7fe4700c257cac9db866efa220`. It has since merged.

The [range-diff](range-diff.txt) is equal to the preceding `27ab70a2c`
patch. The complete Cloud tree remains
`df643415a4ba38a75c584c0a825b54f86cc6d785`. No owned source correction was
introduced. [identity.json](identity.json) records the remote base matching
immediately before push; later target advances do not change that observation.

Fresh Mac Rust 1.96.0 gate: formatting **5.260 s**, strict all-target Cloud
Clippy **12.807 s**, full serial Cloud tests **178 passed / 0 failed / 1
ignored / 0 filtered**, **64.693 s** including compilation. All three added
fixtures pass; the ignore remains the manual fixture writer. All **464** scoped
source/manifests hashes match before and after. No warning was found.

[mac-gate.json](mac-gate.json) retains exact commands, counts, terminal exits,
raw-log hashes, Cloud file hashes and full-map digests. It is a compact
normalized receipt; the raw receipt and logs remain private. [Native proof](../report-failed-units-rebase-140e7-native-20261022/README.md)
is separate. Previous gates and the historical failed native run are retained
at [the preceding proof commit](https://github.com/autonomous-ai/lampOS/blob/ed4ae1ba7c412c33e3cd06fcaf38a3beb2b7d354/lampOS/docs/work-log.md).
No device or latency result is claimed.
