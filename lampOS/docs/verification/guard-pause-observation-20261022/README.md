# Guard pause observation restored

Part of #1806 / PR #2625. Reviewer-lead's pause P3 identified a lost
500 ms allowance after publishing the person's light-on state. The older
candidate kept the final 300 ms `ONE_PASS` assertion, but unchanged assertion
counts did not prove that its total temporal coverage was preserved.

The fixture now observes `messages/out` for that original 500 ms before
advancing the scenario and sending the next visit. It fails immediately on
a text. The original separate 300 ms post-visit check remains. No successful
completion is inferred from a sleep or a clock acknowledgement; this interval
checks that the paused guard stays silent. Product code is unchanged.

The publication is retained by the bus, but guard's registered topics exclude
`light/state`; publication or skip acknowledgement is not a processing barrier.
The old justification for deleting this allowance was insufficient. The
[historical record](../guard-mark-acks-20261022/README.md) now makes that limit
explicit. Its earlier timing samples, controls, failures and hashes remain
historical; this follow-up claims no new speedup.

The original 500 ms allowance is present in both the pre-PR parent `9752b0f`
and the current base. The 45 original assertion expressions remain, with one
explicit assertion added for the restored interval. The mark acknowledgements,
30 s renewal observations, app-owner/switch test and all product bounds remain.

## Rebase and fresh gates

Before the correction, the previous head `7b21fa72a` rebased cleanly onto
`a498ecc030a9c28419aef3b7de89486664868440` as `7c3396ba3`:

```text
1: 7b21fa72a = 1: 7c3396ba3 lampOS: wait for guard-mode mark acknowledgements in program tests
```

The follow-up then changes only the one fixture interval and its documentation.
Fresh gates on that corrected working tree all passed, native x86_64 macOS:

```sh
cargo fmt --all --check
cargo clippy -p lamp-skills --all-targets --offline -- -D warnings
cargo test -p lamp-skills --offline
cargo build --workspace --offline
git diff --check
```

Skills: **427 passed, zero failed or ignored** (138 library, 19 binary,
270 integration, zero doc tests). The full integration run includes the
corrected pause case. Formatting took 10.6996 s, Clippy 36.3876 s, the full
test command 110.3816 s and workspace build 69.4896 s; those wall times include
compilation and are not fixture latency measurements.

[Gate record](gates.json) identifies the tested source hashes, base, exact
commands, statuses, counts, complete output hashes and external log directory.
Cargo used the shared native target with jobs 4, dev/test debug 0 and
incremental compilation off. No target or test-thread override was set.
Tracked build-input mtimes were refreshed before Cargo, with every tracked
lampOS file byte-identical before/after freshness updates and all gates.
The complete successful logs and runner remain at the recorded local paths;
they are not portable repository attachments. Linux ARM64 validation follows
the pushed head. No ARM64 build on the Mac or device test was performed.
