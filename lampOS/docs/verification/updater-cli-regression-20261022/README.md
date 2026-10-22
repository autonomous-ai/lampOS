# Actual CLI regression checkpoint, 22 October 2026

Part of [#2810](https://github.com/autonomous-ai/lampOS/issues/2810), extending
[PR #3012](https://github.com/autonomous-ai/lampOS/pull/3012)'s seven library-process
cuts with one ongoing CLI checker. No runtime updater source changes.

`self-cli-exec` is a single-threaded custom driver. Its one named case runs:

- The actual built old CLI against a disposable signed local channel. The
  parent sees initial old exec and its same-PID candidate exec; verifies PID
  start time, executable path/hash, argv, exact bounded environment and the
  real pending/current/state prefix; then detaches. Actual candidate main
  must confirm once and the next ordinary round remains at sequence 2.
- An off-switch control which exits 0 at that same sequence but is rejected
  specifically for missing candidate exec, before checking any pending prefix.
  No candidate, pending or recovery query may have been created.

Old/new are copies of the same built ELF with different fixed trailers beyond
its original file segments. Readiness systemctl is an exact fixture. Files
and channel are private temporary fixtures; only a disposable signing key is
used. This is not a cross-release, fixed-fallback, actual service/timer,
production quiet/health timing, cold-boot or physical power-loss proof.
`PTRACE_O_EXITKILL` protects attached test-child cleanup; this checker makes
no observer-death-survival claim. Successful paths did not exercise emergency
timeout cleanup. The owned containers were removed and VM stopped.

## Completed development checks

[linux.json](linux.json) binds the final source hashes and exact normalized
Podman commands to strict targeted Linux Clippy (**23.956 s**), direct ARM64
checker **1 passed / 0 failed** (**3.265 s** including command overhead),
and nextest **1 passed / 0 skipped** (**0.140 s** test time). Both paths are
inside that one test; they are not two added suite tests. Initial missing
pinned-dependency and missing-nextest-tool failures are retained separately.
The successful nextest attempt mounts the already used ARM64 tool. These
are targeted development checks, not the PR's official ARM64 gate.

[mac.json](mac.json) binds fmt (**5.704 s**), strict all-target Update Clippy
(**22.855 s**) and full Update tests (**253.527 s**): **307 passed / 0 failed /
4 ignored / 0 filtered**. Four listing/ignored/filter checks passed separately.
The initial sandbox cache-lock failure remains in the record; authorized
cache-write escalation passed. The new Linux checker is ignored on Mac.

These runs use base head `97ca7b322` plus the recorded test delta. Later
integration/dependency changes need their own qualification. The separate
[attended exec-cut worksheet](https://github.com/autonomous-ai/lampOS/blob/8faa729c9/lampOS/docs/verification/updater-attended-exec-cut-20261022.md)
remains unexecuted and First-owned; #1812's real signed-release run follows it.

No raw logs, host paths, keys or binaries are published. SHA256SUMS binds the
normalized public receipts and this document, not their private raw inputs.

## Final local carry onto integration 464438fc5

[mac-latest.json](mac-latest.json) records another complete changed-crate gate
after the latest-base carry, including the upstream installer assertion:
fmt **5.781 s**, strict all-target Update Clippy **14.719 s**, and Update tests
**239.034 s**: **307 passed / 0 failed / 4 ignored / 0 filtered**. All 36 bound
Update/workspace inputs stayed identical during the run. The receipt names
the integration and precommit head plus source hashes; no commit hash is
misrepresented as including its then-uncommitted new test files. The next
public replacement still needs its own official ARM64 and Clippy runs.

## Official qualification and review, 23:22 +07

[native.json](native.json) binds exact head `baacc2746` to tested merge
`6199378` against `17167b1ba`. [ARM64 39810342673](https://github.com/autonomous-ai/lampOS/actions/runs/39810342673)
and [Linux Clippy39810342675](https://github.com/autonomous-ai/lampOS/actions/runs/39810342675)
completed successfully. Native nextest: **10,531 passed / 0 failed /
67 skipped**, one retained existing leaky Voice pass. CLI checker **0.147 s**,
seven-cut checker **3.592 s**, both explicit PASS. Separately four doctests,
twelve process checks and relay passed. Artifact12441490647 is API metadata
only; its archive/payloads were not independently downloaded or verified.

[Reviewer-lead's fresh CLI read](https://github.com/autonomous-ai/lampOS/pull/3012#issuecomment-6345696778)
found no issue and requested removal of draft after ARM64. The PR is now
non-draft on that qualified head; [ready.json](ready.json) records this. The
noted P3 is that SIGKILL of the entire checker may leave its temporary
fixture directory; normal/drop/panic cleanup is not a SIGKILL guarantee.
Integration advanced to `a37144927` during CI. Reviewer-lead has the final
current-base carry/merge; old-base success is not relabelled as qualification
of that newer integration. The attended unplug and real release remain
unexecuted and assigned to the people identified in the worksheet.
