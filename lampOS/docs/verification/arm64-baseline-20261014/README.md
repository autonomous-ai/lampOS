# Required ARM64 baseline gate, 2026-10-14

Before task 29 or 30 code was committed, GitHub run
[38546842373](https://github.com/autonomous-ai/lampOS/actions/runs/38546842373)
tested `eb4ae60cce7fb3398fe5d60b95c142fda5fb1554` on the workflow's
`ubuntu-24.04-arm` runner inside Debian 12, Rust 1.96.0. This is CI,
not a lamp measurement. The release workspace build passed. The release
workspace test stage failed: Controls `sigterm_finishes_recording_and_releases_the_actual_serve_owner`
tried to spawn `/bin/kill` at `crates/controls/tests/program.rs:340` and
received OS error 2, `No such file or directory`. Six other tests in that
executable passed. The missing executable in the minimal image is the
likely cause; this is not a Records or Memory test failure.

The unedited failed-step output was retrieved with:

```sh
gh run view 38546842373 --repo autonomous-ai/lampOS --log-failed
```

`ci-failed-step.txt` also retains the inherited duplicate `replay_cost`
example filename warning and two Body warnings. The workflow skipped
binary hashing and artifact upload after the failure. No retry, test skip,
source fix or change to the workflow was made by Fourth for this finding.
First owns the workflow/current Controls integration; the required green
gate remains outstanding.
