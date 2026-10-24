# Failed native reminder/task reader qualification

Second, 24 October 2026. Original PR #3312 head `ba7031f58` on base
`88b1ad796`. These failed attempts are retained separately from any later pass.

Dedicated native ARM64 run [40051251652](https://github.com/autonomous-ai/lampOS/actions/runs/40051251652)
executed all twelve named cases: **8 passed, 4 failed, 0 ignored**.
Schedule passed 4/4. Tasks failed 2/2 before readiness. Voice passed 4/6;
both unfinished-delivery cases failed at the Ack-ID assertion.

Tasks stderr reports that the socket parent is not private. The fixture used
an independent temporary socket directory without setting mode 0700. Both
released runtimes correctly reject that directory. The correction explicitly
creates it with private permissions and records/asserts ownership and mode.

Both Voice cut snapshots retain `first_attempt: 1`. The actual replacement
consumer offers attempt 2 with a new consumer correlation, then sends Ack ID
`due.<ledger_id>.1`. Both published `delivery::make_ack` implementations use
`entry.first_attempt` for this stable idempotency key and the current attempt
for the completion proof. The fixture incorrectly expected `.2`. Later
settlement and return-reader assertions were not reached in those failures.
The correction preserves the newer completion attempt assertion and checks
the Ack ID against the original saved first attempt, without weakening exact
replay, one accepted Ack, or original-reader reopen checks.

The full CI artifact is identified by SHA-256 in `artifact.json`; its raw
per-crate output, failing Tasks stderr and failing Voice snapshots/transcripts
are retained here as text. Binary databases and executable files are omitted.

Ordinary native ARM64 run [40051251716](https://github.com/autonomous-ai/lampOS/actions/runs/40051251716)
failed separately: **9,768 passed (one leaky), one failed, 1,349 unrun,
83 skipped**. The raw failure excerpt is retained. The unchanged worker test
subtracts 400 and 90 seconds from host monotonic uptime. An uptime between
those values makes release arithmetic wrap only the first timestamp, so the
room remains Unknown. This is a source-supported explanation, not a measured
uptime of that failed runner. A later successful retry cannot itself repair
that host-age dependency. No fixture qualification, lamp, spoken delivery,
physical power cut, real task completion or independent signature trust is
claimed by these failed runs.
