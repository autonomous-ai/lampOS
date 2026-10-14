# Touch: complete corrected-source host verification

Task #711, source `08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`, based on `a9992f272`.
All commands here ran on Third's Intel i7-10700K at 3.80 GHz, 64 GiB,
x86_64 development Mac, macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0.
No lamp was accessed. The working-pad and buffered-output fixes are both
included. Evidence is on the log branch so the tested task head stays fixed.

## Complete required gate

Workspace formatting, strict all-target Clippy for all twenty affected
packages, serial tests for those packages, and the workspace build passed.
The tests report **3,283 passed, zero failed,
15 inherited ignored, 204 completed harnesses**.
`gate-results.json` holds every exact command, exit code, elapsed host wall
time, source check and count; `ignored.json` lists the inherited cases.
The four text files are unedited command output. No touch test is ignored.
Format and Clippy passed before an inherited host timeout; the complete serial
test rerun and workspace build then passed on unchanged source. These are
explicitly separate invocations, not one relabelled run.

Packages: `lamp-actions`, `lamp-air`, `lamp-body`, `lamp-controls`, `lamp-ears`, `lamp-expression`, `lamp-eyes`, `lamp-health`, `lamp-keeper`, `lamp-lean`, `lamp-memory`, `lamp-msgs`, `lamp-name`, `lamp-policy`, `lamp-records`, `lamp-schedule`, `lamp-surround`, `lamp-tasks`, `lamp-voice`, `lamp-voice-eval`.

`final-buffered-source.json` records SHA-256 for Controls/Msgs source, tests,
fixtures and workspace/toolchain manifests. Every file matched after every
gate stage and at collection. The whole working tree and task head were
unchanged throughout this final run. Previous interrupted, failed or older
source runs are not counted as this gate and are not summed with it.

`later-upstream.json` identifies integration changes fetched while these gates
ran. They change Voice, not Controls/Msgs, dependencies or the workflow. First
installs only the exact Controls artifact for this task; this gate does not
qualify the later Voice code. `motor-timeout-source-review.md` records the
separate read-only review of the inherited failed test and its timing limits.

## Standalone build and default-off comparison

The fresh source-only copy `/private/tmp/lamp-third-touch-standalone-buffered-final-jrv1f76e` built the entire workspace,
then passed all **446 Controls/Msgs tests**, zero failed/ignored in 29
harnesses. All 33 local package manifests, including the patched dependency,
resolve inside that copy; its target directory is also inside it. Ordinary
Cargo registry/git caches were reused. `standalone-results.json` records the
commands, paths, source checks and elapsed times; both raw streams are kept.

The final source-only Controls binary has SHA-256
`229e4a9cb32df14f8f96783e31053f5a7b6b0850d2731c965140e47b763812ae`. `default-off.json` identifies the original
pre-task `d672844cceaa9ff5488c0c0a42c8fcbdc0df4478` binary and both exact replay
commands. Without `--touch`, stdout and stderr are byte-identical in shadow
mode and with legacy `--enable`. Each has 901 fixture records, 900 observations,
zero rejected, one authored fault, six gestures, 4,601 ticks and 98 frames.
One copy of every identical stream is retained. This is a host comparison;
First still runs the required switch-off minute on the lamp.

## Linux path and earlier failures

`linux-clippy.json` and `.txt` record strict all-target Controls/Msgs Clippy
for `aarch64-unknown-linux-gnu`, compiled/linted on this Mac. No ARM64 binary
was executed here. It used `buffered-source.json` before the final rebase;
Controls/Msgs and workspace manifest bytes match the final source manifest.
The separate [native gate](../touch-complete-arm64-20261014/README.md) built
and executed the exact final head on GitHub's ARM64 runner.

The task commit's `touch-output-host-20261014` preserves the fresh-copy
five-second CLI timeout, incomplete output, unchanged deadline, corrected
26-test rerun and interrupted host gates. The new replay-file output uses
an 8 KiB buffer and final checked flush; the same file/stdout bytes, private
mode and overwrite refusal are tested. The cancelled obsolete native run is
retained beside the final native evidence. No failing test was removed or
weakened. Earlier full and focused runs overlap these runs and are not added.

The first corrected-source full Mac test invocation stopped with 2,804 passed,
one failed and fifteen ignored: the inherited Voice motor crash/hang cleanup
test exceeded its two-second helper wait, reporting no recorded worker failure.
One focused Cargo rerun and three runs of the exact original test binary passed
unchanged. The cause was not established; the helper's two-second wait versus
production's three-second startup allowance is only a possible sensitivity.
[The complete failure and diagnostic reruns](../touch-host-timeout-20261014/README.md)
remain retained. The final complete twenty-package rerun above changed no
source, test assertion, filter or deadline. It is the passing test evidence;
the failed invocation is never called a pass.

## Not shown

Physical electrode calibration, continuous head sensing, touch-to-response
latency, I2C contention, service-user lock access, and applied Voice/privacy,
light or motor behavior remain unmeasured. The ten-pad example thresholds
are authored, not a lamp calibration. First's exact lamp sequence and likely
failure points are in `docs/controls.md`, “Run without a lamp, then qualify
on one”, on the task commit. A host pass does not qualify those boundaries.
