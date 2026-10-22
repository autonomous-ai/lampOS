# Failed units in health reports

Recorded 2026-10-22 07:17 +07, on public `lamp-voice`
`e2b2f596d71dcc8112cdb1ee508667a4ee4b0bbe`. The task branch is
`second/report-failed-units`, assigned in [Queue #2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6324418282).

A failed oneshot with zero restarts appears in the fixed-name
`failed_units` list, then disappears on a fresh successful recovery sample.
Restart counts and failed names come from one bounded command and one
cached snapshot. Consent, due/urgent timing and voice deferral precede it.
The existing `waits` implementation, restart parser and bounded command
helper are unchanged. Unknown/ambiguous names or states are omitted; an
empty list on query failure is not a clean bill of health. Schema stays `/1`.

Mac, Rust 1.96.0, in order:

- `cargo +1.96.0 fmt --all --check`: exit 0, 5.181 s.
- `cargo +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`: exit 0, 22.708 s.
- `cargo +1.96.0 test --locked --offline --no-fail-fast -p lamp-cloud -- --test-threads=1`: **177 passed / 0 failed / 1 ignored / 0 filtered out**, exit 0, 48.723 s.

The existing ignore is `reports::tests::write_a_reports_fixture`, a manual
fixture writer. [gate.json](gate.json) retains all four exact result lines,
command/status/timing, raw-log hashes and source hashes.

The compiled control replaces only `health_body`'s emitted list with an
empty vector. `reports::tests::failed_units_share_the_gated_restart_query_and_cached_recovery`
fails at the first emitted list: `[]` instead of `["lamp-update.service"]`.
The result is **0 passed / 1 failed / 0 ignored / 150 filtered out**, exit
101, 30.280 s. [control.json](control.json) retains the receipt. The source
was restored byte-for-byte before the final gate; both source hashes match.

Coverage also includes no query without consent or before due voice
deferral, one query for both fields, cached success and failure, recovery
with unchanged restart counts, malformed/duplicate records, unknown-name
canaries, sorted unique allowlisted output, schema, retransmission, waits
independence and the 4 KiB boundary. Changed-name/qualified-type search
across `crates/` found consumers only in Cloud. Read review found no P1/P2.

The source fixture executes a local stand-in through the production command
path. It does not exercise live systemd or a lamp. No full local workspace
gate, local ARM64 build or device action was run. Native ARM64 and Linux
Clippy qualify the PR after push; they are not yet claimed here.
