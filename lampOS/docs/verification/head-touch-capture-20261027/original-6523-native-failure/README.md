# Collector #3892: original failed native receipt

This is the immutable original `6523fa886b/0d1d1f2b94` result, not a current carry qualification. ARM run [40543995512](https://github.com/autonomous-ai/lampOS/actions/runs/40543995512/job/122329568735) failed after **2,508 PASS / 1 FAIL / 53 summary skips**, with **5,294 selected tests unrun**. All 21 collector controls ran (20 PASS/1 FAIL); the full 52 required controls are **25 PASS / 1 FAIL / 26 UNRUN**. Linux Clippy [40543995574](https://github.com/autonomous-ai/lampOS/actions/runs/40543995574/job/122329571280) is green.

Both original checkouts are preview `44e46f978`, ordered parents `[0d1d,6523]`, complete tree equal to the authored head. The physical Linux raw-name subsection passed once, separately from the failed gate. The failed same-descriptor control loops through two unlabeled cases; the Result and metadata values were not printed, so its exact failed subcase and cause remain **UNESTABLISHED**.

Linux timestamps can coincide, as explained by the [kernel timestamp contract](https://docs.kernel.org/filesystems/multigrain-ts.html). That is a general limitation, not proof of this instance's cause. The subsequent fixture correction changes primitive post-read length and adds full-collector equal-length/modelled-equal-metadata byte checks, with no production relaxation. It requires a fresh source-bound gate and new original native jobs.

No failed source was retried, and the summary's repeated FAIL is a reprint. Doc/custom/export/upload were skipped; no executable or published-artifact binding is claimed. Raw logs/APIs remain private, bound by hashes in the receipt. These text-only files contain no raw actor metadata or machine paths. Verify `SHA256SUMS` before use.
