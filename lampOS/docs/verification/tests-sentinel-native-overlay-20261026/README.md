# Sentinel fixture: native result on the tested merge

Ready for lamp-voice: no — the latest carry still needs qualification.

PR #3543 head `96e3986a2`, authored on `1ec33957c`, passed [ARM64 run 40294010686](https://github.com/autonomous-ai/lampOS/actions/runs/40294010686) and [Linux Clippy run 40294010677](https://github.com/autonomous-ai/lampOS/actions/runs/40294010677), attempt 1 each.

The actual ARM64 preview `307b99005` has parents `1cad670df` and `96e3986a2`. Its tree differs from the authored head's original tree. The retained own-branch reflog carry `e9bc02f3a` has parent `1cad670df`, the identical tested preview tree `dd67d0b0333d411494c3b585d32f78355b6f286f`, and one equal patch in the preserved range-diff from the original authored commit. This proves the tested merge composition without relabelling the original base.

Nextest ran **7,496 unique passing tests**, with no failures, leaks or timeouts; **53 tests skipped**, across **70 executed and 114 skipped binaries**, in **169.962 seconds**. All 14 ordinary Linux Reader cases, including the changed sentinel, executed exactly once and passed. The privileged DAC test remains ignored and unrun. Separate promotion tests passed 25/0, custom process tests 13/0, docs 4/0, and `relay_signals` reported `ok` once; these are not added to the nextest census.

The audit contains reviewed counts, case names, source identities and receipt hashes only. Original logs, API records, tested source and range-diff remain private. The result qualifies the tested `1cad670df` composition; it does **not** qualify the latest local `6588c7098` carry on `1aa409fcb`. No rerun, local compiler or device action was used for this audit.
