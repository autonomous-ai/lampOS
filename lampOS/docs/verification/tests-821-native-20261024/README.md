# Native checks on the 821 carries, 24 October UTC

Update head `be88244fb`: ARM40120126122 passed with 7,408 PASS plus one LEAK, all 7,409 selected tests terminal, 53 skipped, zero failed/unrun. NoPause head `7c0cf8d33`: ARM40120157609 passed with 7,398 PASS plus one LEAK, all 7,399 selected terminal, 53 skipped, zero failed/unrun. These are the PR affected-package profile counts, not all nightly/workspace cases.

Each primary case projection uses only terminal rows before the one actual nextest Summary, with distinct `(binary, case)` keys and contiguous 1..N indices. Later echoed detail blocks are excluded. The two Linux runs40120126124/40120157622 passed actual pinned `cargo +1.96.0 fmt --all --check` and strict `cargo clippy --locked --workspace --all-targets -- -D warnings` steps.

`complete-tree-proof.json` records fetched actual checkout parents and complete checkout/authored tree equality. Both have parent821 plus their exact task head. Raw GitHub console logs, run metadata and case projections are included; no binaries or local ARM build. The first collection script rejected a too-narrow `cargo fmt` literal because actual formatting includes `+1.96.0`; the read-only correction uses unchanged downloaded logs and actual successful command steps. No CI or test run was repeated for that correction.

These 821 executions remain historical after train100 advanced to8db. The task branches were rebased again with equal patches; latest-base checks run separately. Frozen427 local gates and their failures are also separate; no source identity or historical HOLD is relabelled. No device action or product change.
