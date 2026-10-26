# Hand-pump fault-control receipt reconciliation

PR #3544, head `55eb90cf4`, has three deliberate fault controls. Each
completed one exact case with exit 101 and its intended assertion failure.
They remain separate from the 4,086 passing fixed-source Voice cases.

The generic process controller left `case_execution_status` at its initial
`not yet observed` placeholder. This supplement reconciles that field against
the retained exact-case terminal, summary, intended assertion and process
closure. Original receipts and raw output are not rewritten; their hashes
are recorded in `reconciliation.json`. No test was rerun to repair metadata,
and none of the failures was changed into a passing case.

The raw local packet is
`/private/tmp/fourth-hand-pump-combined-gates-20261026`. The local per-case
watchdog is not a new CI configuration; native CI keeps its existing job
containment. No lamp, throughput or thermal result is claimed.
