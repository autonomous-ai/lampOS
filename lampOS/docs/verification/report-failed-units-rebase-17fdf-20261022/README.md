# Failed-unit reports: rebase after Voice changed

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head
`a50f88224132066826946c452b1a52fd44568459`, on `lamp-voice` base
`17fdfda1d757324b8437d997304e855e8ecbf9dd`. The remote base was verified immediately before push.
Previous head: `c37fe695c9cfe92d6d0c489706ec2e7f036de7a8`.

```text
1:  c37fe695c = 1:  a50f88224 lampOS: report currently failed product units
```

[identity.json](identity.json) and [range-diff.txt](range-diff.txt) record an equal
patch and the unchanged entire Cloud Git tree
`1dd3e10a85ee130c788c245b789a6ae779c6434f`. All three Cloud source/document hashes match
before and after. No Cloud source correction was made. Upstream changed Voice
production code and its documentation, so formatting, strict Cloud Clippy and
Cloud tests ran freshly at the new head; these are not retained earlier-head
results.

The [fresh Mac gate](mac-gate.json) passed:

| Check | Command | Exit | Command seconds |
| --- | --- | ---: | ---: |
| Formatting | `cargo +1.96.0 fmt --all --check` | 0 | 4.894 |
| Strict Cloud Clippy | `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings` | 0 | 13.216 |
| Cloud tests | `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1` | 0 | 65.981 |

Cloud tests: **177 passed / 0 failed / 1 ignored / 0 filtered out**. The ignore is
`reports::tests::write_a_reports_fixture`, the existing manual fixture writer.
The receipt's 394-file before/after source maps are identical; they cover Rust
sources and manifests in Cloud, Controls, Skills and Voice. It retains the exact
summary lines and original raw-log hashes. Its `base` field names the tested
checkout head `a50f88224132066826946c452b1a52fd44568459`; the integration base is the separate
`17fdfda1d757324b8437d997304e855e8ecbf9dd` above. No full local workspace or local
ARM64 gate is claimed, and command durations are not device-latency measurements.

At this checkpoint, new-head [ARM64 run 39711676351](https://github.com/autonomous-ai/lampOS/actions/runs/39711676351)
and [Linux Clippy run 39711676412](https://github.com/autonomous-ai/lampOS/actions/runs/39711676412)
were running. Their terminal results are not recorded here. Native qualification
is **pending**; ready for `lamp-voice`: **no, current-head native results pending**.
The reviewer owns the merge after the current-base CI gate.

The [historical c37 native receipt](../report-failed-units-rebase-9e7ee-native-20261022/README.md)
qualifies only `c37fe695c9cfe92d6d0c489706ec2e7f036de7a8` and tested merge
`e6a427b813b10e8e55adaa97bbc42c3bbeb93f5a`. Its three files are copied unchanged, including their
original checksums and limits. Those native results do not qualify this new
head. The [earlier rebase checkpoint](../report-failed-units-rebase-9e7ee-20261022/README.md)
remains separate. No artifact payload, physical lamp, live service or acoustic
latency was verified by this documentation checkpoint.

The copied records have these SHA-256 values:

| File | SHA-256 |
| --- | --- |
| [identity.json](identity.json) | `ab1104c1e05b09475d1a817211a231ce53ce27f898cf95c0da89b065f7fa95bb` |
| [range-diff.txt](range-diff.txt) | `16660bf44a1020b45b9f232b2007c434a14e70e4a9c5cef2f069d932da43b6e6` |
| [mac-gate.json](mac-gate.json) | `91f6f2b5c00bd8c468dd9a957f3466bd0ed8effcae826fab89373fc4f417cedc` |
