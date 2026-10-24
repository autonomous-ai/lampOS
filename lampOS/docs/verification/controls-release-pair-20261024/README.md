# Controls release-pair driver gates

PR #3346 authored head `27b3695f0235bc7a721dfb216cd21203c249491d`.
Intel Mac full Controls **509/0/3 ignored**, formatting, strict all-target
Clippy and workspace build pass. The four Rust/manifest file hashes match
both unchanged gate source and committed blobs in local-gates.json.

The two actual-release cases remain ignored locally: this checks the
current CLI driver, not published-binary compatibility. Dedicated native
ARM64, ordinary ARM64, Linux Clippy and independent review remain pending.

The earlier sandbox gate has 15 existing Unix-socket `Operation not
permitted` failures. It stopped before later test targets/workspace; its
source hash predates the final deadline/provenance hardening. The final
socket-enabled gate is separate evidence. Raw logs are unchanged.

YAML parsed and all eight run blocks passed `bash -n`; actionlint is absent.
The workflow guard audit exercised valid receipts and refused missing,
duplicate, symlinked, 17-reap and zero-case receipts. GNU source/archive
checks and actual ARM64 executables were not run here. The fixture audit
found no P1/P2; failed-command provenance was strengthened, while output
size remains checked after each bounded command rather than a live quota.
No lamp, hardware, acoustic or real touch-latency acceptance.
