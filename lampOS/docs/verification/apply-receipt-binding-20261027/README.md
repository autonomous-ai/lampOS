# Apply receipt identity: separately qualified sources

PR #3808, Part of #3689. The parent checks its own requested operation and sequence before accepting any setting record. This does not authenticate the helper, validate supplied records or implement writer replacement.

- `initial-e362`: original production negative controls (two library failures and one round failure), then fixed full library 152/0/2 ignored plus 65 selected round passes on actual e362 source. The worktree source was not committed at that execution; source and actual/preserved driver hashes are retained.
- `integration-1fb`: added real-applier rollback controls, full library 155/0/2 ignored and 67 selected round passes, on actual 1fb source.
- `current-c688-c287`: clean patch-identical carry onto c287, fresh fmt/strict Clippy/full library 155/0/2 and the two added real-applier controls 2/0, with 199 round cases filtered out. The earlier 67 are not current-carry execution.

Counts in each packet describe their actual source and scope. New controls are included in totals, not added again. Native original CI and device qualification are separate. These text receipts omit local paths and payloads; raw logs/compiler JSON and actual executable bytes remain private.
