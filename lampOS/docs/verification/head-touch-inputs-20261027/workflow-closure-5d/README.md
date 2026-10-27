# Original workflow closure addendum

PR #3856 head `5d866fd9c7fcb55720db478058f5e930b6a63e6e`: original attempt-1 ARM64 run **40518278416**, job **122237712287**, and Linux run **40518278177**, job **122237712716**, now both report workflow and required-job status **completed / success**. Both original bounded watchers exited 0 and the owning controller drained 0. No watch or compiler handle remains owned by this audit.

Readiness was first based on completed successful required checks while Linux workflow-level metadata lagged. This addendum closes only that delayed envelope; it does not change the sealed Mac, ARM or Linux qualification packets or transfer any counts. Both final raw job-log hashes equal their sealed packet hashes. Linux also executed `cargo +1.96.0 fmt --all --check`; the pinned whole-workspace formatting step completed successfully, separately from the helper's formatting and strict workspace Clippy.

A proposed local read-only watcher retirement under the verified no-job-left condition aborted at its fresh PID existence check because the watcher had already exited naturally. No signal was sent, no GitHub job was cancelled or rerun, and no runtime was replayed. Promotion/publication/report jobs remain skipped. No merge, key/signing, collector/authentication, physical or power-cut acceptance is claimed.
