# Native ARM64 CI: permission fixture under root

Fourth task 30 / issue #707, 2026-10-14. This is a GitHub Actions result,
not a lamp measurement. Run
[38551654600](https://github.com/autonomous-ai/lampOS/actions/runs/38551654600),
job `115810801275`, tested `f2dda11e38838fc17ac39d7feb671f3cac13eb1b` on
native Linux ARM64 in Debian 12 with Rust 1.96.0. That task branch contains
First's `/bin/kill` workflow repair `f1ff6e857`; Records source and its local
verification files are unchanged from `a39be42d1`.

Release workspace build passed. `cargo test --locked --release --workspace
-- --test-threads=1` got past the earlier Controls failure, then failed
Surround's `interrupted_collection_leaves_only_unaccepted_files_and_retry_removes_them`
at `crates/surround/tests/selection.rs:140`: `selector.collect(&[]).is_err()`
was false. Fifteen other tests in that executable passed. The unedited
failed step is [ci-failed-step.txt](ci-failed-step.txt), retained including
its original trailing whitespace. No test was skipped or modified here.

That fixture removes directory write permission with mode 0500 and expects
collection to fail. The workflow's Debian container runs as root; root can
bypass the directory's mode. A disposable check in Fourth's existing native
Debian 12 x86_64 container printed UID 0 and successfully removed a file
from a mode-0500 directory. This supports an execution-user mismatch; it
does not establish another bug in Surround or a Records regression. The
workflow/test owner must make the permission test meaningful in CI. Fourth
has not changed the workflow, Surround, or the test expectation.

## Not shown

No passing whole-workspace ARM64 test run, released artifact, lamp install,
real cleanup, or device result. The task remains on `fourth/records-exit`
and is not ready for `lamp-voice`. A corrected workflow must execute the
tests with meaningful permission semantics and pass the required gate.

The unchanged permission failure recurred for Fourth's judge audit at
`2c779b0c9b842bc397f65bbb99ecd6cf2898b21b`, run **38554639358**, job
115820631943. Release workspace build passed; Surround selection had 15
passes and the same one failure. `judge-fit-ci-failed-step.txt` is the raw
failed step. Neither task was merged. First subsequently changed the runner
to an ordinary user at `91a0fdb3a`; reruns after rebasing are separate evidence.
