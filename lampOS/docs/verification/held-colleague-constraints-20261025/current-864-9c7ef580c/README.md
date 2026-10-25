# Held-constraint carry on the current release base

Native Mac runtime head `9c7ef580c`, parent `92511dc09`, release base
`86416a0d3`, Rust 1.96.0. Formatting, strict all-target Voice/VoiceEval
clippy, workspace build and full default Voice/VoiceEval tests all exited
0: **4,672 passed / 0 failed / 21 ignored**, 11 harness summaries. All ten
own added controls executed and passed. No disk error marker was found.

The own four-file patch retains every added/removed line from `15b7c85af`.
The parent has its separately qualified late-verdict change and evidence.
Tests used the same owned build workspace only after the parent gate ended
and was pushed, detached at the exact child tree. Source stayed frozen and
clean throughout. The identity file records the source and settings;
commands, UTC times and exit codes are in `gate-results.json`, alongside
literal harness result lines and hashes of the retained raw logs.

One compiler job, serial tests, existing cache, incremental off and
zero development/test debug info. No all-fresh compilation or independent
pre-execution artifact attestation is claimed. Old forget negatives,
restricted/load failures and earlier passes keep their own identities in
the PR; this result does not erase them. The new original Linux runs must
qualify the pushed head separately. The child stays parent-first and must
be carried onto the actual release head after its parent lands.

No local ARM64, lamp, acoustic latency, calibration or activation claim.
The same-owner explicit recall limitation and generic missing/weak-current
personal-scope acceptance remain separate from these retained constraints.
