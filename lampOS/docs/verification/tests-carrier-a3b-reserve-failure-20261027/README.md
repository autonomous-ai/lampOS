# Carrier a3b: partial reserve failure

The original `63a2a9579f1434ee9c4f323209c89073d918c614` run on `a3b9c46814` remains **controller FAIL / Cargo -15**. At 113.637 s, the reserve guard observed 2,206,986,240 free bytes below its unchanged 2,214,592,512-byte reserve and sent SIGTERM to its owned group. This was a reserve abort, not a completed test verdict.

Three drivers completed: library **125 PASS / 2 IGNORE**, binary **3 PASS**, functions **5 PASS**. Install then reported **9 additional PASS** before interruption. Thus the exact observed terminal ledger is **142 PASS, 2 IGNORE, 0 FAILED rows**; install itself did not complete.

The uniquely pending serial line was `install::a_failing_step_keeps_the_record_and_a_later_run_finishes`. It has **no outcome / INCONCLUSIVE**. Install has **91 unstarted cases**. Five wholly unstarted drivers contain another **212 cases**: `it` 13, `round` 195, `release_pair` 2 declared ignored, and the two self-exec/process drivers 1 declared ignored each. All 448 frozen-inventory identities partition into 144 completed rows, 1 interrupted case and 303 unstarted cases. [audit.json](audit.json) lists every identity and binds the raw receipts.

Update doctests and every standalone-promoter stage remain **unrun**. The original terminal records unchanged source, 258 sealed witnesses and current executable hashes, with owned groups closed and the lease released. Only originally bound attributes are claimed; legacy witness rows do not gain retroactive inode evidence.

The continuation plan is **disabled**. It excludes all completed PASS/IGNORE cases, selects only the 91 unstarted install cases and five unstarted drivers, and separates docs/promotion. Re-observing the interrupted case would require one explicit new infrastructure-recovery authorization after disk safeguards; its original INCONCLUSIVE result stays retained. Combined observations must never be called one full Cargo run.

No runtime, retry, cleanup, source/target/code change, compiler or CI action was performed by this audit. The older 3be full/native 442-PASS history is separate and does not qualify this partial 63a result. Root decides any recovery after reviewing disk headroom and the exact plan.
