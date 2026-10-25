# Independent build report arithmetic read

Exact public PR #3448 head `5436db671f03a65f9ab18650bc43efee9a462c2d`, authored base `056a76132354f40f97eb5fbc519778c04f1ccb19`. The diff changes one Markdown file.

An independent Decimal parser over the Git blob finds 70 shipping rows summing 65.15 (93.071428…%), 16 quality rows summing 10.20 (exactly 63.75%), and two excluded mood rows summing 1.17 (58.5%). To the nearest tenth, quality is 63.8%, versus the published 63.7%; P3 posted at issuecomment-6393884435. Both quality score changes are +0.10 and reconcile with the previous 10.00 total.

The receipt binds the document SHA-256 and all totals. This is source scope and arithmetic only: no Cargo, underlying lamp-grade audit, current-base readiness or runtime acceptance.
