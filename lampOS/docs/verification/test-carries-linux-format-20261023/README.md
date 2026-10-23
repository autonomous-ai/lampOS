# Exact-head Linux x86 formatting receipts

All three source-only `cargo fmt --all --check` commands passed once on the
immutable heads below, each parented by `cf462adb04f64d699c226dd3b1781c0ad927068f`.

| PR | Head | Exit | Container wall time |
| --- | --- | --- | --- |
| #3255 | `e228f6452` | 0 | 17.497 s |
| #3218 | `4e1e00e52` | 0 | 17.879 s |
| #3190 | `d5d0383ac` | 0 | 18.327 s |

Wall time includes container setup, archive extraction, formatting and source
hashing; it is not rustfmt-only time. The toolchain was Rust/Cargo 1.96.0 and
rustfmt 1.9.0-stable, host `x86_64-unknown-linux-gnu`. Each source archive contains
all 6,680 tracked lampOS inputs from the named Git commit. Parent/tree/archive
identity checks passed, and before/after source fingerprints match.

The reviewed rootless cached amd64 image used `--pull=never --network=none
--read-only`, a private 1 GiB tmpfs, default user 1000:1000, no host mounts, no
persistent/shared targets or volumes, and no added capability or privileged
flag. No compile, test, ARM or device action ran. CPU was released at 09:56:31Z.

Each PR directory contains `result.json`, `source.json`, `commands.json`,
`raw-streams.json`, `image.json`, `plan.json`, `preparation.json` and `external.json`.
Commands retain exact arguments and per-command elapsed/exit/hash metadata.
Raw text streams are lossless UTF-8 JSON; image configuration is projected to
identity/platform/user with the complete external response hash-bound. Full
source inventories, source archives and the measuring script stay external.

The [earlier 0c2 archive timeout](prior-archive-timeout.json) is preserved. It
stopped before any formatter invocation, so it is neither a formatting pass
nor a Rust-formatting failure. The new plans change only that command's timeout
from 60 to 180 seconds; the container remains bounded at 300 seconds and one-use
markers prevented repeat execution. No result here qualifies tests, build
behavior, current-parent readiness or any retained failing gate.

[Summary](summary.json), [validation](validation.json) and [SHA256SUMS](SHA256SUMS)
bind the compact records. No branch or publication was changed by this packaging.
