# Combined boot-clock and cue-peer Linux formatting

`cargo fmt --all --check` passed once on Linux x86, session 72569, exit 0.
The source is boot correction head `4e8ea747cd7dc745742283d6d08cb100297a8bfa`
on integration base `09f8e7412f92870e16ccca14e1b9cdc1b14add48`, plus the two
exact peer snapshots in `fixture_provider.rs` and `docs/voice-test-fixtures.md`.
No combined candidate commit existed at this run. All 6,709 archived lampOS
entries were checked against the frozen source map, including Git modes;
every non-overlay entry matched the committed Git blob before archiving.
The archive, Git identity, snapshots and before/after input manifests matched.

The container command took 15.908472 seconds,
including extraction and source hashing as well as rustfmt. Rust/cargo 1.96.0,
rustfmt 1.9.0-stable, host `x86_64-unknown-linux-gnu`. This is not a physical
hardware or interaction-latency claim.

The cached rootless Linux amd64 image ran as user 1000, with `pull=never`,
`network=none`, a read-only rootfs and private 1 GiB temporary filesystem.
No host mounts, volumes, shared target, tests, builds, device access or ARM
build were used. CPU was released at terminal before packaging. No retry.

[source-and-result.json](source-and-result.json) binds the committed source,
exact overlays, archive and equal input manifests. [commands.json](commands.json)
records every command, duration, exit and output digest.
[raw-streams.json](raw-streams.json) preserves complete formatting stdout/stderr
and other small command streams as exact UTF-8 with original lengths and SHA256.

The image response is explicitly projected to ID, OS, architecture and user
in [image-provenance.json](image-provenance.json); its original response hash
is retained. Attribution and build-history metadata stay external; the projection
is not called an unchanged raw response. [external-raw-index.json](external-raw-index.json)
indexes the excluded archive, source map and raw response. Local paths do not
promise remote availability; no Python runner or source export is published.
[validation.json](validation.json) and [manifest.json](manifest.json) record
receipt checks and payload identities.

This qualifies only formatting of the frozen combined source. Earlier boot
and peer controls, failed gates, current native tests and eventual committed
head qualification remain separate; this check does not waive any failure.
