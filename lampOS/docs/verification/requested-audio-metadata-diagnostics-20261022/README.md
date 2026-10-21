# Requested-audio metadata refusal diagnostics (second-P8)

Second, 2026-10-22, Mac fixtures; no device or sound action.
Implementation base: #1757's qualified
`0a878cd4d9d44867b2971e70c118244f0c21ad38`.

## Scope and limits

The existing file-stability refusal now identifies the fixed file role,
check phase, opened/listed differences, and original/opened/listed numeric
stamps. Only that refusal is enriched; folder, initial-open and content-hash
errors retain their existing behavior. There is no path, filename, content or
hash input to the formatter. The maximum-width numeric fixture requires the
single-line ASCII message to fit within 1,536 bytes without truncation.

The check uses the same metadata reads and content hashing. Formatting and
difference-name allocation happen only in the failure branch. Successful
scans gain no filesystem operation, hash, diagnostic formatting or logging;
all I/O remains outside the authority mutex. Boolean scans still discard the
error, so this does not add visibility for a transient scan-only refusal.
No new device or acoustic-latency result is claimed.

The retained packaging failure's cause remains unknown. These diagnostics
make a future refusal discriminating; they do not establish or fix the old
cause. The earlier evidence and failures stay separate.

## Fixtures and control

Four new tests cover six role/phase combinations, twenty stamp-field/side
changes, three non-regular/symlink replacements, and the maximum-width message.
The existing real path-replacement test now requires a listed-side identity
difference while the opened descriptor is unchanged. The parent replacement
keeps its distinct refusal. Equal-stamp content mutations and full payload
read accounting remain tested.

[control.json](control.json) records a compiled mutation that replaces only
`stability_refusal`'s body with the old generic message, keeping every guard.
The focused file-check suite exits 101 with **3 passed / 5 failed / 0 ignored /
129 filtered out**. The four diagnostic tests and the strengthened real path
replacement assertion fail; content/hash and parent refusals still pass.
The source is restored byte-for-byte before the final gate. The private driver
retains the complete log; the committed receipt records its SHA-256 and the
exact command, result and source hashes.

The first Mac gate passed fmt and stopped at strict Clippy's
`type_complexity` finding in the new stamp-change table. [attempt-1-gate.json](attempt-1-gate.json)
retains that failure; tests had not started. A test-only `StampChange` alias
replaced the repeated function-pointer type, with no lint suppression, runtime
or assertion change. Removing exactly that alias change reproduces the control's
source hash. The final gate below uses the corrected source; the control is
explicitly the pre-alias version, not a second final-source control.

## Mac gate

All final stages exited 0 and the source hash stayed unchanged. The
[gate receipt](gate.json) keeps every exact command, result line and log hash.

| Stage | Result | Elapsed including Cargo |
|---|---|---|
| `cargo +1.96.0 fmt --all --check` | exit 0 | 4.640 s |
| `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings` | exit 0 | 2.998 s |
| `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-skills -- --test-threads=1` | 425 passed, 0 failed, 0 ignored, 0 filtered out | 107.532 s |

A separate read-only review of the final source found no blocking correction.
CLI error propagation was inspected in source; this gate does not reproduce
the original pack race or claim a new real-process CLI fault injection.

The gate uses Rust 1.96.0, two build jobs, no incremental compilation and debug
symbols disabled for dev/test profiles. It records exact command output counts
and confirms the source hash stays unchanged. Only the changed Skills crate
is tested; there is no full local workspace gate or local ARM64 build.
`rg` across `crates/` finds the changed private helper/type names only in this
Skills module. Native qualification is recorded separately on the PR.
