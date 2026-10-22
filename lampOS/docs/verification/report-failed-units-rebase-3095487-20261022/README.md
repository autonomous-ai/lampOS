# Failed-unit reports: rebase onto 3095487

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764) was pushed at
[`27ab70a2c6dbf1e4769de767082fc66291a87c8d`](https://github.com/autonomous-ai/lampOS/commit/27ab70a2c6dbf1e4769de767082fc66291a87c8d),
with parent `3095487ad6330c89566e7d6db6208dcc882f47e9` (including #2780). The captured
[identity](identity.json) records the same remote base immediately before
push. [The owned patch remains equal](range-diff.txt).

**Native qualification is pending:**
[ARM64 39720328972](https://github.com/autonomous-ai/lampOS/actions/runs/39720328972)
and [Linux Clippy 39720329001](https://github.com/autonomous-ai/lampOS/actions/runs/39720329001)
were running at this checkpoint. This is a fresh Mac integration gate,
not a current-head native result.

| Check | Command elapsed | Result |
| --- | ---: | --- |
| Workspace formatting | 5.479 s | Exit 0 |
| Strict Cloud Clippy, all targets | 14.959 s | Exit 0; no warnings |
| Complete Cloud tests | 68.087 s | 178 passed / 0 failed / 1 ignored / 0 filtered |

[mac-gate.json](mac-gate.json) retains exact commands, raw-log hashes,
terminal statuses and all four summaries: 151 library passes plus 27
integration passes; the binary and doc-test targets each run zero tests.
The ignore is the existing manual `reports::tests::write_a_reports_fixture`.
All three added failed-unit fixtures pass. The captured **464-file** source
maps are identical before and after this gate. No test or Clippy warning was
found. Command durations include build work and are not device latency.

The **whole Cloud tree changed** from
`1dd3e10a85ee130c788c245b789a6ae779c6434f` to
`df643415a4ba38a75c584c0a825b54f86cc6d785`. Upstream
[#2864](https://github.com/autonomous-ai/lampOS/pull/2864) changed `answer.rs`
and `lamp.rs` for the capability map; the new passing test is
`answer::tests::system_info_says_each_switchs_kinds_only_while_it_is_on`.
The owned `reports.rs` and `tests/it/report_contract.rs` remain byte-identical.
`docs/cloud.md` retains the upstream system-info `can` row addition; its
failed-unit section is unchanged according to the captured composition
review. [verification.json](verification.json) records the precise scope.
The [pre-gate review](pre-gate-composition-review.json) is historical: its
“gate required” status is superseded by this fresh passing gate.

The separately retained [historical local gate](historical-beb1a412/mac-gate.json)
for `beb1a412386a69af7687b5c17dfbf6602603afea`, parent `5c8b03a2eb2f7755262397c46216be2af41b4a95`,
passed formatting in **5.074 s**, strict Clippy in **13.448 s**, and Cloud
**177/0/1/0** in **65.793 s**. Its 464-file maps also match before/after.
Publication was withheld when main advanced; **that candidate was not
pushed and has no native qualification here**. Its complete Cloud tree was
the older tree above; it does not qualify the changed current integration.

The earlier [unpublished d05 proof](https://github.com/autonomous-ai/lampOS/blob/f90b1d028ba6cd2a76f4188fc10b18265f94b799/lampOS/docs/verification/report-failed-units-rebase-2d1d3-20261022/README.md) and
[failed 93d93 native proof](https://github.com/autonomous-ai/lampOS/blob/f90b1d028ba6cd2a76f4188fc10b18265f94b799/lampOS/docs/verification/report-failed-units-rebase-fab69-native-failure-20261022/README.md) remain at immutable proof commit
`f90b1d028ba6cd2a76f4188fc10b18265f94b799` and are linked rather than copied.
The failed native run remains **7,885 passed / 1 failed / 63 profile skips /
2,132 selected tests unrun**; fresh local passes neither erase that result
nor establish its failure's cause.

Identity, range-diff and pre-gate composition records are byte-exact copies.
The two gate records are compact normalized exports, with original receipt
hashes, exact commands/counts/log hashes, complete Cloud file hashes and
verified full-map digests. Raw logs and full source maps remain private.
[SHA256SUMS](SHA256SUMS) binds every file here. This preparation performed
no Git operation, test, build, network request, publication or device action.
