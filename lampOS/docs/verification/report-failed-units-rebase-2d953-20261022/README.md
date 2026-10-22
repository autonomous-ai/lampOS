# Failed-unit reports: rebase after Controls landed

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764), current head
`87154f0530c1b1cf3519f748cc9d5919424cf72e`, on `lamp-voice` base
`2d953bb5a241b9e4c71d3c82af32dc9b14efb38c`. The remote base was verified immediately before push.
Previous head: `1114e4b8f2030cac4ca1315d94bd07f94cf6ecd9`.

```text
1:  1114e4b8f = 1:  87154f053 lampOS: report currently failed product units
```

The entire Cloud Git tree remains `1dd3e10a85ee130c788c245b789a6ae779c6434f`.
All three source/document hashes are identical before and after; [identity.json](identity.json)
records both sides and the upstream changes. Controls is used by Cloud's
Voice and Update dependencies, so a fresh focused Cloud gate was run.
No source correction was made. Reviewer-two's current-base gate is separate
from the unchanged source read.

Mac Rust 1.96.0, at the new head above, [exact receipt](mac-gate.json):

- `cargo +1.96.0 fmt --all --check`: exit 0, 4.889 seconds.
- `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`: exit 0, 12.759 seconds.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1`: **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit 0, 64.179 seconds.

The ignore is the existing manual report-fixture writer. Captured Cloud,
Skills, Voice and Controls hashes remained unchanged throughout the gate.
Only Cloud's tests were selected. No full local workspace or local ARM64 gate.

Fresh [ARM64 39708347268](https://github.com/autonomous-ai/lampOS/actions/runs/39708347268)
and [Linux Clippy 39708347385](https://github.com/autonomous-ai/lampOS/actions/runs/39708347385)
are the new-head runs. Terminal qualification is recorded separately.
Ready for lamp-voice: **no, new-head native qualification pending** at this
checkpoint. The PR remains non-draft; Reviewer-two owns the merge.

The [previous head's native receipt](../report-failed-units-rebase-a498-native-20261022/README.md)
records 9998 passed (one leaky), 0 failed, 63 skipped, four doc tests,
twelve process-harness tests and relay success. It qualifies only `1114e4b8f`;
its result is not reused for this rebased head.
