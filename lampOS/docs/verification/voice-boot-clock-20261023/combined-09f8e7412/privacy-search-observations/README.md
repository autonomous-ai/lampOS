# Fixed privacy/search observations on base and combined source

The accepted sequence completed exactly eight observations and four compilations,
without retries: four normal passes and four deliberately forced original
assertion failures. Controller exit 0 means all observations and forced controls
validated; **four actual tests exited 101**. No failed local gate is waived.

Base is `09f8e7412f92870e16ccca14e1b9cdc1b14add48`; candidate is the preserved
original combined tree `72a62a51c065002eeff4407dba77c56088e3649f`, not a new
execution of the carried PR head. Both received only the separately accepted
scratch test patches in [the executed plan](executed-plan.json). Original source
assertions, clocks, 50 ms lease freshness, retry count, 500 ms privacy guard,
24-second search host watchdog and cleanup remained unchanged. The fixed plan
was accepted in [review](https://github.com/autonomous-ai/lampOS/pull/3190#issuecomment-6369744229).

| # | Arm | Variant | Case | Pass/fail/ignored | Actual exit | Process seconds |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | base | observation | privacy | 1/0/0 | 0 | 0.066360 |
| 2 | base | observation | search | 1/0/0 | 0 | 2.713879 |
| 3 | base | forced_boundaries | privacy | 0/1/0 | 101 | 0.504533 |
| 4 | base | forced_boundaries | search | 0/1/0 | 101 | 24.165375 |
| 5 | candidate | observation | privacy | 1/0/0 | 0 | 0.066514 |
| 6 | candidate | observation | search | 1/0/0 | 0 | 2.811764 |
| 7 | candidate | forced_boundaries | privacy | 0/1/0 | 101 | 0.511024 |
| 8 | candidate | forced_boundaries | search | 0/1/0 | 101 | 24.189017 |

Every invocation used its newly compiled, x86_64 Mach-O library executable and
`--exact CASE --nocapture --test-threads=1`. All four compiled inventories matched
3,668 names, including 15 ignored; each selected case ran with 3,667 filtered and
zero ignored. These isolated observations do not recreate the earlier full
parallel workload. Each compiler artifact, complete census, source map and
before/after guards is bound in the accompanying records. No ARM or device run
occurred. The outer caps were 60 s for privacy and 420 s for search; no timeout
or interruption occurred. All runner-owned child processes were reaped.

Privacy's forced 30 ms pause occurred after reader construction but before fresh
lease 2 publication. All ten attempts in each forced arm returned the original
`privacy publisher unavailable` error, then failed the original ten-attempt
assertion. Captured fresh lease ages at read completion were
**591–2085 µs on base** and
**606–1325 µs on candidate**, all below
50,000 µs. This demonstrates that the unavailability boundary can be reached with
a fresh kept lease on both sources. It does **not** distinguish Late from
SourceGone or establish the historical cause. No extra notice, receive or
state-judging call was made.

Search's forced pause followed fixture push publication, **without observing
coordinator consumption**, until the existing host deadline. Both arms failed
the original `Run::assert_clean` with `scenario host watchdog`, before later
search assertions and the unpushed comparison. Recorded close probes had an
already-expired scope deadline while the supervisor's separate cleanup budget
still had time. Each role/PID and actual probe result is retained; aggregated
errors are not repeated waits on one PID and do not establish a hung child.
The test's original cleanup ran; runner reaping does not independently prove
that detached descendants never existed. Normal search cases passed on both
sources. These are forced mechanism controls, not reproductions of historical
semantic progress or cause.

Both source trees were restored to their exact original bytes/modes; all source
and artifact guards passed. The initial prepare-only attempt failed because its
tracked-path check ran relative to lampOS; it compiled and ran no tests. Its
receipt is preserved in [preparation attempts](prepare-attempts.json). The fixed
runner then passed prepare-only checks before the one granted execution.

[Raw streams](raw-streams.json) preserve complete output for all eight cases,
original panics, full traces, compiler stderr, inventories and other small
commands with original byte counts/hashes. [Observations](observations.json),
[commands](commands.json), [source/execution guards](source-and-execution-guards.json)
and [result](result.json) keep actual outcomes separate from controller validity.
Full compiler JSON, bulk source maps and Python remain external under
[explicit availability limits](external-identities.json). No changes to product
behavior or latency bounds follow from these observations. Instrumentation's
formatting, allocations, locks and clock reads add unmeasured overhead.
