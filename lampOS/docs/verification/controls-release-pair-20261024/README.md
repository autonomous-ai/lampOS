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

## Native result on 27b3695f0

Dedicated 40075828326 is **2/0/0 ignored/1 filtered**. Independently verified
195 regular evidence files: 36 command starts/reaps, 24 successes and
12 expected refusals; nine byte-equal 47-frame replays and three equal full
written pins files per case. Source maps stay unchanged over 10,347 paths;
preview 76ad068a7 has the authored tree. Native ordinary 40075828246 is
**11,196 PASS plus one LEAK, 92 skipped**, process 12/0; Linux Clippy
40075828298 passes. Terminal metadata, all command receipts and the full
artifact inventory/hashes are retained here. Raw artifact is on that run;
NUL-delimited maps and executable copies are not committed.

Second's independent READ-OK/Linux fmt and Lead's READ-OK are on the PR.
Documentation-only d3a291bf8 now states single-implementation replay and
historical native status explicitly; all Rust/Cargo/workflow bytes match.
Current-head CI/read carry remain separate. No hanging-child cleanup
negative control or live output quota is claimed. No hardware qualification.
