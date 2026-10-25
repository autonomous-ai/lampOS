# Body unplug-gap: original and train native results

Audit of completed public CI, 26 October 2026, 01:18 +07. No test was
rerun for this record. [PR #3497](https://github.com/autonomous-ai/lampOS/pull/3497)
landed through [train #3498](https://github.com/autonomous-ai/lampOS/pull/3498).

| Scope | ARM64, attempt 1 | Linux formatting/Clippy, attempt 1 | ARM64 nextest output |
| --- | --- | --- | --- |
| Original PR | [40218244607](https://github.com/autonomous-ai/lampOS/actions/runs/40218244607), success | [40218244592](https://github.com/autonomous-ai/lampOS/actions/runs/40218244592), success | 11,296 passed (1 leaky), 106 skipped |
| Train | [40218484025](https://github.com/autonomous-ai/lampOS/actions/runs/40218484025), success | [40218484011](https://github.com/autonomous-ai/lampOS/actions/runs/40218484011), success | 11,296 passed (1 leaky), 106 skipped |

Each ARM64 log contains 11,295 literal PASS rows and one LEAK row before its
Summary: 11,296 unique binary/test pairs, every ordinal 1 through 11,296
present exactly once, and no failed terminal row. Both executions have the
same selected cases. Nextest announces 179 binaries, with 106 tests and two
binaries skipped; 153 distinct binary labels have terminal rows. The announced
binary count is retained separately from labels with executed tests.

All 614 Body cases pass in each run: 446 in the library, 125 integration tests,
and 43 across the five executable test binaries. Both changed cases pass:
`unplug::tests::a_gap_in_samples_withdraws_ready_and_only_an_unsafe_one_fails`
and `unplug::tests::the_enabled_adapter_publishes_its_latch_and_joins_after_stop`.
The one LEAK in each run is
`lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
It remains a leaky pass; this record neither asserts leak-free execution nor
assigns a new cause.

## Exact source identity

| Role | Full commit |
| --- | --- |
| Base | `9e9e6733a1ed32ba1a14b6920f7120035b89396a` |
| Original PR head | `382e2573ad69f17739580d8c210e8240d5120f2c` |
| Original actual checkout | `94c19509ec8bbae733a763064717929d78fadc96` |
| Train head | `2bc56ea1ca55d3b3c74545fb3762dbe905d03916` |
| Train actual checkout | `4c62b4551e69c1410a8d7d9cc07d12fec694fc26` |
| Landed commit | `14c64a8db1578ca3b5095b056463fbbf5aaebb3c` |

The five head, checkout and landed commits share complete tree
`b6ecaf8e6b18695bd08de8d8060fa148baef3b03`; the base is outside this equality claim.
The full checkout rows in the excerpts identify what CI actually tested.
The train checkout's tree was independently checked against its
[public commit metadata](https://api.github.com/repos/autonomous-ai/lampOS/commits/4c62b4551e69c1410a8d7d9cc07d12fec694fc26).
The event head is retained separately from its tested merge checkout.

## Census and retained scope

The parser accepts padded ordinals and counts terminal rows only between
Starting and Summary. Both logs contain 35 later PASS rows for already-counted
cases; those report echoes are excluded. The first audit assertion counted
only Body's library label (446); including its six integration/executable
labels reconciles the expected 614. This parsing correction was not a test
run or a test failure.

[census.json](census.json) records run metadata, full-log SHA-256 values,
ordinal checks, Body binary counts and both changed-case outcomes.
[original-selected.txt](original-selected.txt) and
[train-selected.txt](train-selected.txt) retain literal checkout, start,
changed-case, leak and summary rows, with their GitHub timestamps. Only the
repeated job/step prefix is removed. [SHA256SUMS](SHA256SUMS) binds these four
public files; full workflow logs, inventories and binaries are not copied.

The [original Mac proof](https://github.com/autonomous-ai/lampOS/tree/382e2573ad69f17739580d8c210e8240d5120f2c/lampOS/docs/verification/body-unplug-gap-clock-20261026)
remains 609/0/0, direct gap 1/0, and five distinct intended 0/1 negative-control
observations. Reviewer-two's independent Mac 609/0/0 is a separate read on
[#3497](https://github.com/autonomous-ai/lampOS/pull/3497), not another native
run or a number added to these totals. The historical failed observations
and their scope remain unchanged.

The seconds in native test rows measure process-test execution, not physical
settling or lamp response latency. These results do not qualify an attended
updater power cut, a signing key, a device, or the signed-release run.
