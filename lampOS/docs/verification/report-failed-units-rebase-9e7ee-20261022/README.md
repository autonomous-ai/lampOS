# Failed-unit reports: rebase after Eyes documentation and tooling

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), head
`c37fe695c9cfe92d6d0c489706ec2e7f036de7a8`, on `lamp-voice` base
`9e7eec6e5352d605598a6fd3bac2dd9fe924496c`. The remote base was verified immediately before push.
Previous head: `fa63fa363c18d4f66e82af602b7924eb4660d194`.

```text
1:  fa63fa363 = 1:  c37fe695c lampOS: report currently failed product units
```

[identity.json](identity.json) and [range-diff.txt](range-diff.txt) record an equal
patch and the unchanged entire Cloud Git tree
`1dd3e10a85ee130c788c245b789a6ae779c6434f`. All three Cloud source/document hashes
match before and after. This rebase changes only `docs/eyes.md` and
`tools/seems-session.sh` / `tools/seems-session-test.sh` upstream. All crate
sources, tests, manifests and the workspace lockfile are identical across this
rebase. No source correction was made.

At this new head, `cargo +1.96.0 fmt --all --check` ran freshly and passed:
exit **0**, **5.127 seconds**. Its empty-output SHA-256 is recorded in identity.json.

Strict Cloud Clippy and the Cloud test gate are **retained from
`87154f0530c1b1cf3519f748cc9d5919424cf72e`; they were not rerun at this new head**.
The [prior Mac checkpoint](../report-failed-units-rebase-2d953-20261022/README.md)
and [exact gate receipt](../report-failed-units-rebase-2d953-20261022/mac-gate.json)
record:

- `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`:
  exit **0**, **12.759 seconds** at that prior head.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1`:
  **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit **0**,
  **64.179 seconds** at that prior head.

The ignore is the existing manual report-fixture writer. These are Cloud's
selected tests. The unchanged Cloud tree and source identity support retention;
they do not mean the gate ran again. The [preceding rebase checkpoint](../report-failed-units-rebase-d747-20261022/README.md)
records the earlier Skills test/documentation-only base change. No fresh full
local workspace or local ARM64 gate is claimed.

New-head [ARM64 39710858994](https://github.com/autonomous-ai/lampOS/actions/runs/39710858994)
and [Linux Clippy 39710858812](https://github.com/autonomous-ai/lampOS/actions/runs/39710858812)
are running. Native qualification is **pending** at this checkpoint; no terminal
results are recorded here.
Ready for `lamp-voice`: **no, new-head native qualification pending**.
Reviewer-two owns the merge after the current-base CI gate.

The [immediately preceding historical native receipt](../report-failed-units-rebase-d747-native-20261022/README.md)
qualifies only `fa63fa363c18d4f66e82af602b7924eb4660d194` and its tested merge.
It retains the initial offline qualification refusal of mutable API PR metadata
and the corrected immutable run/checkout/parent evidence. Those native results
do not qualify this head. No artifact payload, device behavior or acoustic
latency is verified by this rebase checkpoint.

The copied records have these SHA-256 values:

| File | SHA-256 |
| --- | --- |
| [identity.json](identity.json) | `aafab55536bb7dbe6c29bd7e1cd70367be3a464cfa9a6ccc7b75759ec914789a` |
| [range-diff.txt](range-diff.txt) | `e1193c0bd786ca78b25864f61506ea4fa954b70b22b88da47fe6b40ceed59c2f` |
