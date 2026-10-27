# Diagnostic fixtures: original a47 native failure

Ready: no.

The original [ARM run 40551384357](https://github.com/autonomous-ai/lampOS/actions/runs/40551384357), attempt 1, **failed** on published head `a47fe22e56d181f6d7946bc5b98d90dd9560293e`: **5,097 unique passes, one failure, 2,698 selected unrun**, with 53 aggregate summary skips. It executed 5,098 of 7,796 selected tests. No duplicate outer terminal was counted. Original [Linux formatting/Clippy 40551384353](https://github.com/autonomous-ai/lampOS/actions/runs/40551384353) passed.

Actual checkout `1fd8aacaa31bdd2486fbcc3d4810c64c7b90c6b7`, tree `a88b383c84d92beb258df392237899830eae2cba`, has ordered parents `3aa752f73914d0f29fc90edb010a3a7fa1cf526b` and `a47fe22e56d181f6d7946bc5b98d90dd9560293e`. This is the CI merge composition, not an assumed authored or current workspace tree.

The sole failure was `lamp-voice remember::tests::after_a_forget_the_next_conversation_is_journalled_and_ended`, a journal close/open order assertion. Its one outer failure is in [failure-ledger.json](failure-ledger.json). All eighteen owned diagnostics cases lacked a terminal after fail-fast and remain unrun in this original native run. The separately scoped local 275 passes and the parent's green native gate do not make it green.

The later delivered #3891 repair can be consumed by a new source carry; it does not change this result. The original artifact API listed zero artifacts, so no failed executable was available to retain; no executable or environment equivalence is claimed. Logs and the original unique-terminal ledger remain retained privately, with digests in [receipt.json](receipt.json). No raw user content, binary, or private path is published. No runtime was replayed for this audit. Current-base gates, independent reads, privacy/composition and physical conditions remain separate.
