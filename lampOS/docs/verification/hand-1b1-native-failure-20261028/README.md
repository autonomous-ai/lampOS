# Parent 1b1 original native failure

Ready: no. Original [ARM run 40588908587](https://github.com/autonomous-ai/lampOS/actions/runs/40588908587) failed; original [Linux Clippy run 40588908613](https://github.com/autonomous-ai/lampOS/actions/runs/40588908613) passed. Both original logs independently identify checkout 22cf6a693, tree 79c7011a3, with parents c1fa2bc9f and published 1b1da9ae3. All three authored files match. The tested merge preview differs from the published full tree only in install.md.

The original nextest window has 6,466 unique passes and three failures: 6,469 of 7,805 selected cases ran, leaving 1,336 selected cases unrun. There are 54 aggregate skips, no duplicate terminals or retries, and no nextest LEAK or TIMEOUT terminal. Internal fixture timeout assertions still count as failures. Scheduling stopped after the failure; the workflow was not canceled or rerun.

The failed identities are recorded with exact source paths, reported assertion lines and whole-file hashes in failed-identities.json. The two-face fixture observed Unknown where Busy was expected. The trace fixture's writer-completion wait timed out. The privacy fixture's stand-in did not end within its bounded wait. All three complete source files match the actual Main parent and published head; that equality does not identify the cause or justify a failure waiver.

Eight of 23 required controls passed once; 15 have no terminal, including the Linux-only control. Nine of 16 privacy controls passed once; seven have no terminal. Because this run excluded binaries and no complete JSON discovery was retained, those absent privacy identities are not classified as selected-unrun. Cohort counts are not added together.

Doctests, relay, process and export stages were unrun after the failure. The artifact API is empty. No failed executable was retained, and no executable equivalence is claimed. The original raw logs remain preserved privately; their digests are in the receipt. No artifact download, runtime retry or source change was performed. The separate local 4,177 passes and earlier native results remain source-specific and do not replace this failed gate.
