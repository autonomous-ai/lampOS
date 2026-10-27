# Speaking-face consent: native proof and exact current-source equality

Ready: no.

The original [ARM64 run 40575097415](https://github.com/autonomous-ai/lampOS/actions/runs/40575097415) and [Linux Clippy run 40575097494](https://github.com/autonomous-ai/lampOS/actions/runs/40575097494), attempt 1 on published head `2ced256ffdc19dfb5aa84a8adf75638a0da94420`, both passed. ARM executed **11,829 unique cases, all passing**, with zero failures, leaks, timeouts, retries, duplicate terminals or selected unrun cases. Its 107 summary skips remain aggregate counts.

All **56 required controls passed once**: 29 new consent controls, 16 retained Voice controls, and 11 Eyes controls. [The named ledger](required56-ledger.json) records each original terminal and source-function hash. Every file containing these controls is byte-identical through the published/local-tested and actual CI blob bindings.

Both original checkout logs name `d2b33e18b69d478bd8a4a4e2d1ef6825f7722b78`. Its ordered parents are Main `dadacc1bc2fe2e285ed5125a32dd6fe483c01991` and the published head. Its complete tree is `43da54b09c99c0fa4a584ac1b292d224d02e2ec9`, with 10,920 entries. The independently read rebased commit `bc06a6eff458f736b992ad4190d76127f551acd9` has **exactly that full tree**. This equality was checked directly, not inferred from a matching subset of owned files. The authored publication tree is different; upstream composition changed onboarding code and documentation, as recorded in [the receipt](receipt.json).

Fourth uses this observed full-tree ARM run as the runtime gate for that rebased source. Fresh Mac formatting, metadata, strict workspace/all-target Clippy and workspace build also completed successfully on the same tree. The prepared second Mac runtime is **UNRUN**, with zero additional runtime credit. The earlier 4,563-case Mac result belongs to tree `ab92a0a49` and is not added to the native census.

Separate from nextest, 47 doctest targets produced **four passes**, the relay harness produced **one aggregate pass**, and the process harness produced **13 passes**. The export log reports 48 binaries plus two examples and complete SHA256SUMS. Artifact 12766004878 is 118,857,309 compressed bytes; its API ZIP digest matches the upload log. Only metadata was collected: no archive download, payload hash or ELF verification, installed-byte claim, or Voice/Eyes pin pair.

The original failed native and local compiler/runner epochs remain failed and retained; this proof does not overwrite them. Reviewer, consent-policy, privacy/composition, latest-base and physical-lamp conditions remain separate. Main had advanced to `61c2a7264` during proof preparation; this packet qualifies the exact tree above and makes no qualification claim for that newer Main. No runtime or workflow was replayed to obtain this evidence.
