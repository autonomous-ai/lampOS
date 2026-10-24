# Explicit Linux formatting gate

PR #3373, authored d57c2779225479b9dd9def546f07db97602a83e2, base 6902cf8e2. Third claimed the concrete gap found while qualifying #3369: rustfmt installation and green tests/Clippy did not establish formatting execution. Second explicitly yielded duplicate #3374; Fourth was asked to retain Common qualification rather than duplicate workflow files.

The two-file change installs rustfmt with Rust 1.96.0 and runs cargo +1.96.0 fmt --all --check in lampOS before cache restoration and strict Clippy. The README states this distinction. No Rust source/runtime/default/switch change. No local Rust build/test rerun for CI/docs-only edits.

Local Intel macOS: exact pinned fmt passed; Ruby YAML parsing checked command/directory/component/order, every run block passed bash -n, and diff whitespace checks passed. actionlint is unavailable. These are local validation results, not Linux execution.

Original Linux Clippy run **40089244359** succeeded. Its explicit named formatting step passed before strict whole-workspace Clippy. The checkout log names merge preview **822f8c584**; both authored changed files equal that preview (hashes and exact step metadata in native-audit.json). This is a checkout/source binding, not an uploaded before/after workspace hash-map claim. Raw job-log SHA256 is retained in the audit and the original log remains on GitHub. The preview includes newer base source; authored/preview whole-tree identity is not claimed.

Lead READ-OK was posted on authored d57c27792; their remaining concern that current base formatting be clean is answered by this actual successful Linux step. Original automatic ARM64 run **40089244321** is still pending at this checkpoint. Second separately reported exact #3369 head formatting on Linux, 7,020 unchanged files; this is not attributed as Third execution or a retroactive CI step.

No device, Mac ARM64 link/execution, runtime or physical acceptance. Latest integration stays with the train.
