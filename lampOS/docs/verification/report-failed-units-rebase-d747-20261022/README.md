# Failed-unit reports: rebase after Skills test changes

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), pushed head
`fa63fa363c18d4f66e82af602b7924eb4660d194`, on `lamp-voice` base
`d747ebb0f25d26996c6d273e8e3f8b8af6a477e5`. The remote base was verified immediately before push.
Previous head: `87154f0530c1b1cf3519f748cc9d5919424cf72e`.

```text
1:  87154f053 = 1:  fa63fa363 lampOS: report currently failed product units
```

[identity.json](identity.json) and [range-diff.txt](range-diff.txt) record an equal
patch and the unchanged entire Cloud Git tree
`1dd3e10a85ee130c788c245b789a6ae779c6434f`. All three Cloud source/document hashes
match before and after. Upstream changes are limited to Skills integration tests
and documentation; all crate `src` files, manifests, `build.rs` files and
`Cargo.lock` remain identical. No source correction was made.

At this new head, `cargo +1.96.0 fmt --all --check` ran freshly and passed:
exit **0**, **4.803 seconds**. Its empty-output SHA-256 is recorded in identity.json.

The strict Cloud Clippy and Cloud test results are **retained from
`87154f0530c1b1cf3519f748cc9d5919424cf72e`; they were not rerun at this new head**.
The [prior Mac checkpoint](../report-failed-units-rebase-2d953-20261022/README.md)
and its [exact gate receipt](../report-failed-units-rebase-2d953-20261022/mac-gate.json)
record:

- `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`:
  exit **0**, **12.759 seconds** at the prior head.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1`:
  **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit **0**,
  **64.179 seconds** at the prior head.

The ignored test is the existing manual report-fixture writer. These retained
results cover Cloud's selected tests; no fresh full local workspace or local
ARM64 gate is claimed. Source identity is the basis for retaining this focused
gate, not evidence that it was rerun.

New-head [ARM64 39710009811](https://github.com/autonomous-ai/lampOS/actions/runs/39710009811)
and [Linux Clippy 39710010002](https://github.com/autonomous-ai/lampOS/actions/runs/39710010002)
qualification is **pending** at this checkpoint.
Ready for `lamp-voice`: **no, new-head native qualification pending**.
Reviewer-two owns the merge after the current-base CI gate.

The [prior native receipt](../report-failed-units-rebase-2d953-native-20261022/README.md)
is historical and applies only to `87154f0530c1b1cf3519f748cc9d5919424cf72e`. Its results do not qualify this
new head. No artifact payload, device behavior or acoustic latency is verified
by this rebase checkpoint.

The two copied records have these SHA-256 values:

| File | SHA-256 |
| --- | --- |
| [identity.json](identity.json) | `a140be1be13711cec5e98c440a6b094f3c301e948a37091cbf54b5aa74457b98` |
| [range-diff.txt](range-diff.txt) | `80aa04c1d0b282467cacbe7957743619379bb069f45ace4dc08f84d3c1ba0ad1` |
