# Own-keys continuation after the stable-feed and waking-unit base change

This packet records `bb40e10327fa847128d2dc76d205990b958247d6` on `756be347e13d2e52e7268414295dda8fc554c267` after the completed 274 continuation and its separate proof-only commit `a7b68b978844a46f3f68258d10989000a50423fe`. The upstream change is exactly five paths: the product updater and waking units, switch/updates documentation, and the imager's default URL. No Rust source, crate manifest, workspace lockfile or pinned toolchain changed. Their complete tree listings equal the 274 tested head `4a0f0cc4291f4d1d9917c8d7efbe5412200f96fb`.

The added product bytes are nevertheless new compiled inputs. Formatting and strict all-target Clippy for all nine selected crates pass. The current command selections below cover install's PRODUCT units, comparison of the actually installed product, its startup graph, the updater fallback unit, existing pure waking controls, and both switch-document census readers. Counts are **passed / failed / ignored per selection**.

| Selection | Counts |
|---|---|
| install | 99 / 0 / 0 |
| installed-product | 1 / 0 / 0 |
| startup-graph | 1 / 0 / 0 |
| updater-fallback | 1 / 0 / 0 |
| waking | 7 / 0 / 0 |
| switches | 2 / 0 / 0 |

`bash -n ../scripts/imager/build-orangepi-v2.sh` also passes. It checks syntax only: no image is built, no release feed is fetched, and no promotion or installation runs. The updates document has no compiled content reader in the audited crates. The literal five-path delta is retained in [results.json](results.json).

The source inventory covers all lampOS tracked/untracked nonignored files and the changed outside-directory imager script. All 7,178 entries match the pre-command inventory after every phase and when this packet is generated. Exact commands, purposes, source count, summaries and raw-log hashes are in the results; this controller records no executable manifest.

Every existing proof blob and mode, including the completed 274 packet, remains unchanged. Each carried commit has equal changed-content identity using zero-context stable patch IDs. [range-diff.txt](range-diff.txt) preserves Git's literal comparison; adjacent switch-row context may differ, so this packet does not invent an all-equal range-diff label. The complete base delta and carry delta also have equal changed-content identity.

This is a focused content-reader continuation, not a new Voice or full nine-crate suite. Earlier 274 selections, d21 full Voice and 98f60 native runs keep their own scopes. Fresh original ARM64/Linux checks must qualify the eventually published head. Waking tests do not establish cold-boot timing or real ring behavior; no provider, real-key, physical-lamp or latency acceptance ran. Own-keys product activation remains HOLD for #2094 and fresh current-base acceptance.
