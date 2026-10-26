# Carrier promotion-fixture integration failure

Ready for lamp-voice: no (#3466).

Native ARM64 run [40251915590](https://github.com/autonomous-ai/lampOS/actions/runs/40251915590), job 121345479131, checked the preview of carrier `0dbf2a81f` on `756be347e`. The standalone promotion crate reports **3 passed / 21 failed / 0 ignored** across its observed Rust targets. All failures occur while sealing the common product-unit manifest: it declares updater level 3, while the carrier requires level 6. The workspace build and test phase did not start; there is no workspace count for this run.

The complete job output is preserved locally; this public receipt contains only the reviewed counts, failing test names, source identities and public Actions link. The one-line proposed fixture correction uses `manifest::PRODUCT_UNITS_UPDATER`; it preserves every promotion assertion, including the signed future-updater refusal and zero-write check. This is an integration repair, not a baseline waiver. Its complete standalone gates and new native run are pending in this receipt.

Current carrier Mac formatting, strict twelve-package Clippy, updater build, ten carrier tests and workspace build passed before this failure. Linux Clippy run40251915583 passed. The real product-installed GOOD/BAD rehearsal is still pending; no device action ran. The later c5 documentation-only base carry preserves all four patches equally and will be qualified before push.

Automatic approval review rejected republishing the unchecked complete raw log. It is omitted from this publication. The limited JSON receipt was inspected: it contains only public source/run identities, test names, counts and the synthetic manifest error; no credential, person, device or private path is included. No failure was retried or relabelled as passed.

## Fixture repair

Current repair `2d9194565` on `c5c7603a7` changes one field to `manifest::PRODUCT_UNITS_UPDATER`. The four earlier carrier patches range-diff equally across the documentation-only base change. Native x86 macOS formatting, strict all-target Clippy, complete standalone `lampos-promote` tests and build passed: **25 passed / 0 failed / 0 ignored**, including all 22 promotion tests and the existing signed future-updater rejection. The original ARM64 failure above remains failed.

Current ARM64 [40253271430](https://github.com/autonomous-ai/lampOS/actions/runs/40253271430) and Linux Clippy [40253271444](https://github.com/autonomous-ai/lampOS/actions/runs/40253271444) are running at publication. This standalone suite is not the full lampOS workspace. The mandatory real installed GOOD/BAD rehearsal remains pending; no device action ran.
