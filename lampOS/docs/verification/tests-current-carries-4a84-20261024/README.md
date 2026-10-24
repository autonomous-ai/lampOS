# Tests carries onto 4a84fa5ab

Ready for lamp-voice: no. Source carries and Linux x86 formatting only; fresh matched local qualification and exact-head native CI remain.

Each owned branch has a clean, conflict-free one-commit carry onto `4a84fa5ab6a9133aeec555ec4bc0636dd953f660`. Superseded prepared heads are preserved locally; earlier published heads remain in owned remote archives. These receipts do not push a PR branch.

| PR | Previous prepared head | Current head | Range-diff | Linux fmt exit |
| --- | --- | --- | --- | --- |
| #3190 | `5f87be93c` | `93c995a6b` | Equal | 0 |
| #3218 | `c46e6cfe1` | `a33454886` | Equal | 0 |
| #3273 | `2c244d708` | `64065a88e` | Equal | 0 |

Each JSON binds exact Git source, changed paths, commands, raw streams and terminal result. Offline rootless Linux amd64 ran `cargo fmt --all --check` on exact archives with no compilation, tests, ARM build or shared-target access. Source identities and archives stayed unchanged. Timings cover the whole container, not just formatting. Repository cwd and ephemeral hostname are explicitly projected; original stream hashes remain.

All three formatter commands completed before the fresh-base full test stages. The older 88b1ad796 base result is preserved separately: `docs/switches.md` changed and Voice-eval compiles it, so no old full run is relabelled as current. The fresh matched comparison uses twelve packages, fixed commands and environment; no result is claimed before terminal census verification.
