# PR 3162: explicit Linux formatting receipt

`cargo fmt --all --check` passed once on exact authored head
`22634785d607e5ae2d44cc45d540161246ee89de`: exit 0 in 13.981 seconds,
with empty stdout and stderr. Rust and Cargo 1.96.0, rustfmt 1.9.0-stable,
native `x86_64-unknown-linux-gnu`, cached container image
`eee1c1827d71132efdd4bfd72bf5407d7df568b0cf1d14911096745df03cfb6c`.

The complete committed lampOS workspace was archived immutably, including
workspace configuration, vendor inputs and third-party source. The root tree
is `87534e6e7590abe8b82027380e2e49989bb53ecd`; lampOS tree is
`fd36a5445df2af95abdc89c257bd186afe914411`. Archive SHA-256:
`2a40bc9eb52f8eeeb48090d467d5f32be4c69f1196f681f08a2a4310c4b3f72c`.
Source/archive identities were unchanged after the check.

The container used a read-only root, a private temporary filesystem and no
network. No build, test, ARM64 compilation, shared target or device operation
ran. This receipt covers the exact authored head, not its later train carry.
The PR owner and merge owner retain the other gates and final qualification.

`commands.json` records complete argv, durations and exit codes. `source.json`
and `result.json` retain source and terminal facts. `raw-output.json` contains
every stdout/stderr stream losslessly: UTF-8 encode each `text` and verify its
recorded byte count and SHA-256. `manifest.json` is the original external
manifest, including the source archive (not copied here; it can be recreated
from the recorded Git commit). `SHA256SUMS` covers these published payloads.
