# Ready fixture: original native result and current carry

Ready for lamp-voice: no — this receipt qualifies the original head; the current carry needs fresh gates.

PR #3548 original `5713bece4` on `cced42943` passed [ARM64 run 40278416601](https://github.com/autonomous-ai/lampOS/actions/runs/40278416601) and [Linux Clippy run 40278416455](https://github.com/autonomous-ai/lampOS/actions/runs/40278416455), attempt 1 each. Nextest ran 7,492 unique passing tests, no failures, leaks or timeouts, and 53 skipped tests, across 70 executed and 114 skipped binaries, in 156.944 seconds. The changed speaking-Ready case passed once. Thirty-four repeated terminal display lines were deduplicated with no conflicting outcome.

The actual preview `1581f6e36` has the exact original base/head parents, and its tree equals the authored tree. Separate promotion tests passed 25/0, custom process tests 13/0, docs 4/0, and the relay signal check reported `ok` once. These separate checks are not added to the nextest census. Original logs and source/API records remain private, with hashes in the reviewed audit. No ARM64 build ran on the Mac and no device action occurred.

The task was then rebased to `db84407d0` on `d5dc2e87f`: one equal range-diff patch, both authored file blobs identical, clean checkout. This is source carry evidence only. No current-carry compiler, runtime or native qualification is claimed, and the original native result is not relabelled.
