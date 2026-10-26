# Current product-carrier qualification

Head `b635671c2` on `cced42943`, PR #3466. All five carried patches remain
equal. The Mac gate passed formatting, strict Clippy for fourteen actual
reverse dependents, workspace/updater/promotion builds, 126 unit tests (two
ignored), ten carrier cases, 34 head-touch cases and 25 promotion tests.
These are focused Update checks, not a full Mac Update runtime result.

Native [ARM64 40273208241](https://github.com/autonomous-ai/lampOS/actions/runs/40273208241)
and [Linux Clippy 40273208181](https://github.com/autonomous-ai/lampOS/actions/runs/40273208181)
passed on their first attempts. The selected nextest census is 11,372 unique
passes, no failure/leak/timeout and 106 skips, across 182 executed binaries,
in 208.945 seconds. All ten carrier cases pass. Separate promotion 25/0,
process 13/0 and docs 4/0 are not added to that census. Exact preview parents
and the equal authored/preview tree are recorded in `native-audit.json`.

The prior 1c artifact-parser failure and 0c compilation failure remain failed.
No prior result is substituted for this carry. Source and executable guards
passed; raw logs remain private and are bound by their hashes.

The PR remains draft for the mandatory installed-product GOOD/BAD rehearsal.
No lamp, SSH, signed trial publication, physical acceptance, local ARM64
build or performance improvement is claimed.

The exact-head artifact ZIP digest matches GitHub's digest. Only the checksum
manifest and AArch64 `lamp-update` were extracted privately. The updater is
4,816,120 bytes, SHA-256
`49286ba0b831c373a8a1240fe589b0578817fd55ac12b2c0006e673db4f4bc31`,
matching the artifact manifest. It was not executed. `artifact-audit.json`
records the source/run identity; no binary artifact is committed.
