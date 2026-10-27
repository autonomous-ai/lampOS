# Child 4cd original native evidence

Ready: no. Published #3577 head 4cd9d4995 passed its original [ARM64 run 40590263211](https://github.com/autonomous-ai/lampOS/actions/runs/40590263211) and [Linux Clippy run 40590263139](https://github.com/autonomous-ai/lampOS/actions/runs/40590263139), both attempt 1.

Both original logs independently identify checkout f951521809844e3196490e98d872cc3aebeded1d, tree a8f00d31ec80620a8411982e3e5ba5ee1b85d77f, with parents c1fa2bc9f and 4cd9d4995. All two child and three parent authored blobs and modes match. Compared with the authored tree 61b850743, only install.md differs; no full-tree equality is claimed.

ARM recorded 7,805 unique passes, zero bad outcomes, duplicate identities, retries or selected unrun, and 54 aggregate skips. All 18 diagnostics passed exactly once; the sequence case is one of those 18. Separate results are four doctest passes across 47 targets, one relay aggregate pass and 13 process passes. No local 275 or prior native counts are transferred.

The parent’s original ARM 40588908587 remains failed, and the earlier child a47 original ARM 40551384357 remains failed. The three cases that failed in the parent passed in this independent child run with identical source blobs. That observation neither reclassifies the parent failure nor establishes its cause. Parent dependency, current-base, reader and physical acceptance conditions remain separate.

The export log reports 48 binaries and two examples, and the artifact API ZIP digest matches the upload log. This audit downloaded no artifact payload, executed no binary, checked no ELF header and derived no pins. The named ledger and source metadata are reviewable here; original raw logs remain private and their hashes are retained in receipt.json.
