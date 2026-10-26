# IPC full-journal fixture: current-base host continuation

Head `a8d6a4970dcf73a9007850703d9f27af24ea74e5` is based on `1c884f16cf4de53a700bba7512c129c3ee01d72d`. The carry is one equal patch; all seven authored files, including the original controlled-test proof, remain byte-identical with unchanged modes. The full source inventory differs only in the new base's twelve paths. Production before the test module remains unchanged.

On this exact clean source, Mac Rust 1.96.0 formatting, strict IPC all-target Clippy, test compilation and the full IPC crate gate passed. Results: **26 ordinary tests / 0 failed**, **1 doctest / 0 failed**, and separately **`relay_signals: ok`**. The ordinary groups are 8,0,3,1,1,13. The relay harness reports no numeric case count. This is a crate gate, not a full workspace run.

The gate and this audit agree on all 7204 source entries. All eight compiled executable outputs retain their recorded hashes: seven test-profile targets plus the ordinary probe binary. Commands, artifact hashes and receipt/log hashes are in results.json; raw receipts and executables remain private. No executable was rerun to prepare this packet.

The earlier controlled-test proof is carried unchanged at `lampOS/docs/verification/ipc-log-no-wait-20261026`; its failed first blocking control and pre-execution syntax error remain retained. This continuation does not rerun or relabel that campaign or the previous head's green native CI. Original native CI for this head is audited separately and was pending when this host packet was prepared.

No physical latency, persisted journal delivery, release publication or device acceptance is claimed. The unchanged Polled-pipe race with another writer remains outside the fixture. Initial rebase metadata creation was denied by the sandbox with the tree unchanged; the authorized escalated rebase succeeded and both private receipts are retained.
