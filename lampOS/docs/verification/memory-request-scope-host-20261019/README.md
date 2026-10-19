# Request-bound memory scope host verification

Part of #1675. This is a default behavior correction for memory-enabled Voice,
not authorization to enable the shadow voice matcher. Provider session history
and snapshots remain a separate isolation task. No lamp was changed or recorded.

The final checkout gate used Rust 1.96.0 on macOS, two build jobs, from `lampOS/`:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -p lamp-voice -p lamp-turn-owner --all-targets --offline -- -D warnings
cargo +1.96.0 test -p lamp-voice -p lamp-turn-owner --offline
cargo +1.96.0 build --workspace --offline
```

All exited zero. Test harness summaries total 2,336 passed, zero failed,
12 ignored. The complete local log is
`/private/tmp/sixth-memory-scope-final-gates.log`; the earlier gate is retained
separately and is not the evidence for the final peer fixes.

Regression coverage includes source-age expiry, bounded file reads, contended
cache snapshots, atomic invalidation with a read already in flight, owner-drop
revocation, first-tool scope before an answer, a changed face after admission,
End preserving scope, retirement/replacement, reused request numbers, stopped
intake, direct and coalesced camera privacy changes, resend, canceled-before-RPC,
late private replies in legacy and Days memory, and suppression before a second
chat round. Typed app requests remain room-scoped.

The admission target is under 100 microseconds, with no filesystem/network I/O
or waiting on a cache mutex. This target is not a measured lamp result. Hardware
first-use/repeated latency and real-person/session isolation are unverified.
Already-accepted RPC writes or already-sent model context cannot be retracted.

A standalone copy of `lampOS/` at `/private/tmp/sixth-memory-scope-source-only`
also passed `cargo +1.96.0 build --workspace --offline` (exit zero), with
1,346 Rust/C/header/Cargo source hashes matching the checkout. It has no parent
repository source. Log: `/private/tmp/sixth-memory-scope-source-build.log`.
