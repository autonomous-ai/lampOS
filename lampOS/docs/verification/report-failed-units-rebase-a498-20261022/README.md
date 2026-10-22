# Failed-unit reports: current-base rebase

[PR #2764](https://github.com/autonomous-ai/lampOS/pull/2764) was rebased at
Reviewer-two's request to contain the current `lamp-voice` base.
Previous head: `1299399e1dd6390361ff44cccf9145a9f8b9eb7f`.
New head: `1114e4b8f2030cac4ca1315d94bd07f94cf6ecd9`.
New base: `a498ecc030a9c28419aef3b7de89486664868440`.

```text
1:  1299399e1 = 1:  1114e4b8f lampOS: report currently failed product units
```

The complete Cloud Git tree remains `1dd3e10a85ee130c788c245b789a6ae779c6434f`.
All three source/document hashes are identical before and after; [identity.json](identity.json)
records both sides and the upstream crate files changed by the new base.
No source correction was made. The prior P1/P2-none source read remains for
the reviewer to confirm; the current-base check is separate from that read.

The prior native receipt qualifies only the old head. This new head requires
its own native ARM64 and Linux Clippy result. The fresh Mac gate is recorded in [mac-gate.json](mac-gate.json). No full local workspace or local ARM64 gate.

Mac Rust 1.96.0, at the new head above:

- `cargo +1.96.0 fmt --all --check`: exit 0, 6.624 seconds.
- `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`: exit 0, 14.393 seconds.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1`: **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit 0, 67.795 seconds.

The ignored test is the existing manual report-fixture writer. All captured
Cloud, Skills and Voice source hashes remained unchanged through the gate.
Only Cloud's tests were selected; the upstream dependency movement is why
this focused gate was repeated.

[ARM64 39706852548](https://github.com/autonomous-ai/lampOS/actions/runs/39706852548)
and [Linux Clippy 39706852627](https://github.com/autonomous-ai/lampOS/actions/runs/39706852627)
are the fresh native runs for this head. Their terminal qualification will be
recorded separately. Ready for lamp-voice: **no, native results pending** at
this checkpoint. The PR remains non-draft. Reviewer-two owns the merge.
