# Boot-clock carry onto train 29

Ready: **NO**. This is a source comparison, not execution on the new head.

The eight-file patch rebases unchanged from `172389869` on `70cbefcee` to
`0f2e6439e` on `e5cd73aca`. The range-diff is equal. All 1,827 assertion
macro spans in the seven Rust files are byte-identical to this new base.
The complete clean worktree matches the carried commit.

Upstream changed the second worker's counted memory note and exposed two
existing test helpers. Those changes remain intact; the retained upstream
diff and authored patch make the shared-file carry reviewable. The previous
ARM64 and Clippy successes belong to the earlier head, not this rebase.
New full-package qualification is pending, and the five prior Mac failures
remain unclassified. No product code is changed by this patch.
