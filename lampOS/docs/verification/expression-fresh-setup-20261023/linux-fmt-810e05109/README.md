# Expression fresh-setup Linux formatting

`cargo fmt --all --check` passed once on Linux x86, session 67126, exit 0.
The exact source is base `810e05109bced3beca88a5b602930cfed5fba4e5` plus
the two declared Expression test/docs snapshots. No candidate commit existed.
All 6,687 archived lampOS entries were checked against the frozen source map;
the archive, Git identity, snapshots and before/after input manifest matched.

The container command took 14.819576 seconds, including extraction and source
hashing as well as rustfmt. Rust/cargo 1.96.0, rustfmt 1.9.0-stable, host
`x86_64-unknown-linux-gnu`. This is not a physical hardware or latency claim.

The cached rootless Linux amd64 image ran as user 1000, with `pull=never`,
`network=none`, a read-only rootfs and private 1 GiB temporary filesystem.
No host mounts, volumes, shared target, tests, builds, device access or ARM
build were used. CPU was released at terminal before packaging. No retry.

[source-and-result.json](source-and-result.json) binds the exact base/overlays,
archive and equal input manifests. [commands.json](commands.json) records every
command, duration, exit and output digest. [raw-streams.json](raw-streams.json)
preserves the complete formatting stdout/stderr and other small command streams
as exact UTF-8 with original lengths and SHA256, without normalization.

The image response is explicitly projected to ID, OS, architecture and user
in [image-provenance.json](image-provenance.json); its original response hash
is retained. This avoids attribution/build-history metadata, and is not called
an unchanged raw image response. [external-raw-index.json](external-raw-index.json)
indexes the excluded local archive, source map and raw response. Those local
paths are not promises of remote availability; no Python or source export is
published. [validation.json](validation.json) and [manifest.json](manifest.json)
record the receipt checks and payload identities.

This result qualifies formatting only. The full native gate remains separate;
the six earlier mechanism controls remain bound to their original 2615 base.
