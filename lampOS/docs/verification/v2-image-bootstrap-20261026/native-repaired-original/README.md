# V2 imager: repaired original native results

Audit prepared 26 October 2026, 02:04 +07 from completed public CI for
[PR #3501](https://github.com/autonomous-ai/lampOS/pull/3501), repaired head
`4ace70555becb72bb6ddfa9624474e921c0f2a7c`. No test was rerun for this packet.

| Check | Attempt 1 result | Evidence |
| --- | --- | --- |
| Linux ARM64 | 11,308 passed; zero LEAK or failed terminal rows; 106 skipped | [40222685198](https://github.com/autonomous-ai/lampOS/actions/runs/40222685198) |
| Ubuntu 24.04 imager | 31 signature, 5 bootstrap, 8 output, 5 input checks; 31 pytest tests passed | [imager job](https://github.com/autonomous-ai/lampOS/actions/runs/40222685128/job/121257468751) |
| Linux Clippy, every crate and target | Success | [40222685236](https://github.com/autonomous-ai/lampOS/actions/runs/40222685236) |

The imager workflow and selected job both succeeded. Its five bootstrap
observations include the expected signature, payload and recorded-build refusals,
and successful pinned retries and local-channel handling. Refusal exits are
preserved as expected negative results, not relabeled successful installs.
Clippy is bound by completed run/job metadata; its full log was not downloaded
again for this record.

## Source identity and landing scope

ARM64 and the imager actually checked out
`000f71af11cbf7fe5357c42001f599fea5dddfd5`, the merge of the repaired head into
`4913713548aff7b40f0f4e0b049b5e4000e0baac`. Both the repaired head and tested
checkout have complete tree `dc94a4d8faf269ffcbca2e2a7ec8431256965da2`. The ARM64 event
head is kept separate from its tested checkout; public commit metadata verified
their tree equality.

[Train #3502](https://github.com/autonomous-ai/lampOS/pull/3502) landed the older
`861532feb` source at `a6ccb1aa3` and auto-closed #3501. This packet does not
claim that the repaired head landed through that train. The missing size/profile
changes are carried by [follow-up #3503](https://github.com/autonomous-ai/lampOS/pull/3503),
head `d8cf817d6f6a6beb53fe10647683d6d9e94dd7a7`. The coordinating builder verified that
its whole tree is also `dc94a4d8faf269ffcbca2e2a7ec8431256965da2`. Tree identity is recorded
without claiming another execution: the follow-up has its own separate CI.

## Census and selected evidence

The ARM64 census counts only terminal rows after its single Starting line and
before Summary: 11,308 literal PASS rows, 11,308 unique binary/test pairs and
every ordinal from 1 through 11,308 exactly once. No LEAK, FAIL or TIMEOUT
terminal occurs. All 35 PASS rows repeated after Summary refer to already-counted
cases and are excluded. Nextest announces 179 binaries, skips 106 tests and two
binaries, and reports terminal rows from 153 distinct binary labels. This is the
nextest count, not a sum with the separate imager suites or doc tests.

The initial audit parser expected a leaky annotation in Summary and rejected
this run's annotation-free success line. Allowing both summary forms reconciled
the existing log; no test was repeated or converted to success.

[census.json](census.json) contains the exact run identities, full-log SHA-256
values, counts and scope. [selected-results.txt](selected-results.txt) retains
literal checkout, start, summary and selected imager outcome rows with their
GitHub timestamps. Only repeated job/step prefixes are removed; the bootstrap
summary containing a temporary path is omitted in favor of all five literal
case results. [SHA256SUMS](SHA256SUMS) binds the three public files.

The imager tests use ephemeral signing material, mocked platform commands and
disposable files. No image was built, flashed or booted by these checks. Native
CPU test timings do not establish physical settling, interaction latency,
attended updater power-cut acceptance or the signed-release qualification.
