# Failed-unit reports: rebase after Body, Skills and Voice changed

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), pushed head
`93d93ecb1415d1e3b97a7a94d3852fa10cb03018`, on `lamp-voice` base
`fab69b0c4c4333ff8d51459e063169af7355e1dc`. The remote base was verified immediately before push.
Previous published head: `a50f88224132066826946c452b1a52fd44568459`.

```text
1:  a50f88224 = 1:  93d93ecb1 lampOS: report currently failed product units
```

[identity.json](identity.json) and [range-diff.txt](range-diff.txt) record an equal
patch from the previous published head and the unchanged complete Cloud Git tree
`1dd3e10a85ee130c788c245b789a6ae779c6434f`. The two changed Cloud source/test files and
`docs/cloud.md` have identical hashes before and after. No Cloud source
correction was made. Body, Skills and Voice production changed upstream, so the
new head received a fresh focused Cloud gate.

The [fresh Mac gate](mac-gate.json) passed:

| Check | Command | Exit | Command seconds |
| --- | --- | ---: | ---: |
| Formatting | `cargo +1.96.0 fmt --all --check` | 0 | 4.767 |
| Strict Cloud Clippy | `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings` | 0 | 13.023 |
| Cloud tests | `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1` | 0 | 65.491 |

Cloud tests: **177 passed / 0 failed / 1 ignored / 0 filtered out**. The ignore is
`reports::tests::write_a_reports_fixture`, the existing manual fixture writer.
The gate's 453-file before/after source maps are identical. The copied receipt
retains the exact commands, summaries and original raw-log hashes. The Mac gate
receipts' `base` fields name their tested checkout heads; the corresponding
integration bases are recorded in identity.json. No full local workspace or
local ARM64 gate is claimed. Command durations are not device-latency evidence.

The [intermediate unpublished gate](intermediate-a523-mac-gate.json) is retained
separately for head `667b03d9e7cfa724de09f41f1f99a21c52b11c9d` on base
`a52357bb02895c2a0bedc139cf2aee31cf1601a9`. It passed formatting in **5.031 s**, strict
Cloud Clippy in **13.635 s**, and Cloud tests **177/0/1/0** in **65.847 s**, with
its 395-file source maps unchanged. **Push was withheld because the pre-push
check found the newer fab69 base, not because a test failed; no push was
attempted.** This successful intermediate run is historical and is not used as
the new head's gate.

At this checkpoint, new-head [ARM64 run 39713887288](https://github.com/autonomous-ai/lampOS/actions/runs/39713887288)
and [Linux Clippy run 39713887283](https://github.com/autonomous-ai/lampOS/actions/runs/39713887283)
were running. No terminal results are recorded here. Native qualification is
**pending**; ready for `lamp-voice`: **no, current-head native results pending**.
The reviewer owns the merge after the current-base CI gate.

The [historical 17f native receipt](../report-failed-units-rebase-17fdf-native-20261022/README.md)
qualifies only the previous published head `a50f88224132066826946c452b1a52fd44568459` and its
tested merge `42dc45153811c933b55c7124fe0d54ce894f6544`. It does not qualify this new head.
The [17f Mac checkpoint](../report-failed-units-rebase-17fdf-20261022/README.md)
also remains separate. No artifact payload, physical lamp, live service or
acoustic latency was verified by this documentation checkpoint.

All four source records below were copied byte-for-byte:

| File | SHA-256 |
| --- | --- |
| [identity.json](identity.json) | `a0a4d492282b4b98cc858fd23a323ac36e67055e73635a4f338f5a072162d6c2` |
| [range-diff.txt](range-diff.txt) | `62e964501d077a7a4affc1cf3a86e9c9daf1a593da46776b3c1f82f5be5f2a49` |
| [mac-gate.json](mac-gate.json) | `df9305c494cd2869161622acee4acf12446d527b486f6470ac12d6721c388af4` |
| [intermediate-a523-mac-gate.json](intermediate-a523-mac-gate.json) | `30e0dd5b0ebcafcf9c7d748d4198967a4c9d5ab9a17e40df6ce7d7de993e520e` |
