# Independent exact-head Linux formatting for #3344

Authored head `f653bc5bdcdaecb661f27aa076f395dea38474c7` passed
`cargo fmt --all --check` in an offline source-only Linux ARM64 container.
All **6,990** regular-file hashes stayed unchanged; source archive/tree and
image identities are retained in linux-fmt-current.json. The private
container had no host mounts/devices, network disabled, read-only source
and UID/GID 65534; owned removal and absence verify.

This is formatting only, with no local ARM64 build or test, no lamp action
and no privacy/source-read acceptance implied. Independent Voice review
gates run separately. No source/archive/executable is committed here.
