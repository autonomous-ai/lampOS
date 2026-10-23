# Linux formatting for the Eyes and Voice fixture PRs

Both current authored heads passed `cargo fmt --all --check` under Linux
x86_64, Rust 1.96.0 / rustfmt 1.9.0, after their native CI audit:

| PR | Exact authored head | Exit | Container command duration |
|---|---|---:|---:|
| #3156 | `a2da872217b20147e80e194401c5eb1300c2b24a` | 0 | 13.792 s |
| #3023 | `157b800a1c34af5e077607c43086ec6342a324fe` | 0 | 16.889 s |

An existing cached Podman x86 Linux image supplied the pinned toolchain.
The image ID, OS/architecture, digest and exact toolchain output are in
`environment.json`. Each input was `git archive --format=tar <head> lampOS`,
streamed into its own disposable container. The container had a read-only
root, temporary writable extraction directory, no network and no shared
Cargo target. No source was compiled; no ARM64 build or device operation ran.
The archive's complete byte count/SHA-256 and the `lampOS` Git tree are in
`results.json`; the binary TARs are not committed. Recreate an input from its
exact Git commit with the recorded archive command to check its digest.
Both original worktrees remained clean at the same HEAD before and after.

`results.json` preserves complete stdout/stderr, exact argument arrays,
timestamps, durations and exit codes; both format streams are empty on
success. `plan.json` records the two immutable inputs and 300-second watchdog.
`SHA256SUMS` covers every other file here. The published CI appendices correctly
recorded that no Linux fmt had run during those earlier CI audits; this later
receipt closes that separate gate. It does not qualify a later rebased head,
rerun failed tests, clear #3023's x86 failures or establish a physical result.
