# Dependency-ordered updater restarts

Forward restart calls follow the candidate managed unit graph. Rollback and
interrupted recovery reconstruct the saved old graph in startup order.
Unchanged and inactive services participate in ordering but gain no restart.
Voice drains queued speech and checks quiet at its own restart boundary.

The nine integration regressions were written first on unchanged
`f477d6c49`: **0 passed / 9 failed**, all behavioral assertions. With the fix,
all nine pass. Removing only the Voice boundary block while retaining the
new order makes both speech cases fail again, **0 / 2**; the source was
restored exactly. Seven parser cases include the shipped managed graph,
transitive dependencies, repeated/folded declarations and cycle refusal.

Final Mac gate, Rust 1.96.0:

```
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -q --locked --offline -p lamp-update --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test -q --locked --offline -p lamp-update --no-fail-fast -- --test-threads=2
```

**325 passed / 0 failed / 4 ignored**; formatting and strict Clippy pass.
Two ignored cases are the manual fixture writer and live GitHub download;
the other two need Linux tracing. The PR's official ARM64 workflow is the native gate. No local ARM64 build
or test run was substituted. Grep finds the changed ordering helpers and
callers only in `lamp-update`; no public API or dependent crate is changed.

[Complete raw failures, controls, source binding and gate receipts](https://github.com/autonomous-ai/lampOS/blob/lamp-second/lampOS/docs/verification/updater-restart-order-20261023/README.md)
retain the first full gate's two failures too: old tests asserted Voice-first
order for fixtures with no dependency declarations. Only their restart
lists/comments changed to name-order ties; all outcome/file/refusal checks
remain. The final gate ran after those corrections.

This is simulated-systemd and fake-clock evidence, not a lamp result.
Ordering does not establish readiness or make an update atomic. Drop-ins,
aliases, symlinked units and external/socket nodes are not resolved. A turn
can still begin after the final quiet check but before systemd stops Voice.
No device, physical cut, acoustic latency or real-key release was exercised.
