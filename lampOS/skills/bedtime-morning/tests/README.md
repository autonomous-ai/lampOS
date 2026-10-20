# Bedtime and Morning checks

The shipped manifest is disabled. No lamp recording has been collected for
this routine, and this directory is not evidence of a successful lamp run.

Host tests in `crates/skills/tests/daily_engine.rs` load the shipped disabled
manifest and check its three daily triggers and absent model function. Separate
test manifests exercise fresh presence and absence, daily claims, reload,
restart and clock changes. `routine_service.rs` runs the real Skills binary
with a stand-in manifest and fake Expression/Say owners; it does not execute
the shipped Bedtime schedule or verify physical behavior.
The `daily`, `routine` and `limits` module tests cover the durable journal,
correlated rest receipts and the private Actions socket. Expression, Actions
and Voice test admission and their final output boundaries separately.

The existing recording replay does not provide a durable daily journal or
Expression and Actions peers. It cannot establish an enabled routine's result
by replaying face messages alone. Do not add a passing empty replay for those
cases. Record the installed-system comparison and its timing boundaries using
the battery in [Bedtime and Morning](../../../docs/bedtime-morning.md#host-evidence-and-required-lamp-comparison).
