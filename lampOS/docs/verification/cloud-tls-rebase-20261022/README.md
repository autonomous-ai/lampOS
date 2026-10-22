# Broker TLS carry, 22 October 2026

Draft [#3062](https://github.com/autonomous-ai/lampOS/pull/3062) carries
[#2587](https://github.com/autonomous-ai/lampOS/pull/2587) at source
`60d4efb86f025e9731202b956c90af9b1410e508`, as Reviewer-lead requested.
Owned carry head `6622e83e2b931110482304da1843570bed4b254b` has parent
`c6d50559aa54fe58699da5ba1c0e2d79f1a70dd8`. Source first/cloud-tls was not
modified. Its remote head and every source-PR comment were rechecked before
publication. Part of #1396; off unless LAMP_CLOUD_TLS is exactly on.

The original four files remain the complete scope. The original TLS wrapper,
constructor/test and both documentation additions are byte-identical. The
source audit reverses those deltas and three conflict adapters to recover
upstream online.rs exactly. The adapters propagate Option through
options_with_will, guard initial construction, and fail closed/drop the old
client/events when Wi-Fi reconstruction cannot configure TLS. The entire
Will payload/properties, credentials, client ID, expiry, packet bounds,
reconnect bookkeeping and upstream tests stay. The range-diff is preserved
losslessly; its changes are these adapters and moved surrounding context.
Existing nonblocking exact-on parser coverage P3 is unchanged.

## Native gates as run

Native x86_64 macOS, rustc 1.96.0, offline dependencies, two compiler jobs,
debug info disabled and incremental disabled. Each test binary runs serially.
This native target was exclusive to the carry; identical source-content
mtimes were refreshed before switching trees to invalidate stale Cargo
artifacts. Concurrent root Update filesystem tests used a different target.

- cargo fmt --all --check: exit 0.
- cargo clippy -p lamp-cloud --all-targets --offline -- -D warnings: exit 0.
- cargo test -p lamp-cloud --offline -- --test-threads=1: exit 0;
  library 164 passed / 0 failed / 2 ignored, integration 30 / 0,
  binary unit 0 / 0 and doc tests 0 / 0.
- cargo build --workspace --offline: exit 0.

results.json records exact argv, durations, platform, environment, hashes and
terminal results. source.json inventories tracked crates and Cargo/toolchain
inputs; it is not an exhaustive inventory of docs read by include_str.
The source was unchanged through the gate, and the worktree was clean.
Each txt.json stores complete original UTF-8 bytes with SHA-256 and byte
count; escaping retains newlines and whitespace. No Python runner, binary,
credential or physical recording is committed.

## Qualification still open

Exact-head ARM64 run 39803867884 and Linux Clippy 39803867845 are active.
Readiness also needs the final current-base guard and Reviewer-lead's
resolution read. No current native CI success or merge is claimed here.
The backend's TLS port/certificate and a real lamp online/command row remain
unrun; the product switch is still off. No SSH, device change or ARM64 build
on the Mac occurred.
