# Reminder/task pair: Mac gates and first CI setup failure

Second, 24 October 2026. PR #3312 at `3e6248467`, based on `88b1ad796`.
The code uses published 6408 and 6498 artifacts; these Mac results do not
execute those Linux binaries. Native qualification remains separate.

- Schedule and Tasks: `cargo +1.96.0 test -q --locked -p lamp-schedule -p lamp-tasks`
  **434 passed / 0 failed / 11 ignored**, exit 0.
- Voice: `cargo +1.96.0 test -q --locked -p lamp-voice`
  **3,961 passed / 0 failed / 21 ignored**, exit 0.
- The 12 new pair cases are among these skips. Their compilation/listing is
  not a released-reader result. The Voice gate preceded a display-only
  timestamp correction in the ignored fixture; that final target separately
  recompiles and passes strict Clippy, with all six cases explicitly ignored.
- Strict all-target Clippy passes for all three changed crates, and
  `cargo fmt --all --check` passes on Mac. Initial constant-assertion and
  Linux-only FIFO compile diagnostics were corrected before the final gates.
- Exact-commit Linux source-only formatting passes. All 6,973 regular-file
  hashes match before/after, and container removal was verified. No local
  ARM64 compilation or test gate ran.
- Workflow YAML and four shell blocks parse. The archive control retains
  only regular evidence, excluding a fixture FIFO/socket/executable; Mac
  metadata was disabled for that archive control.

Raw command outputs, exit files, derived counts and Linux-format provenance
are beside this note. The source-name census shows no runtime/library reader
change; no switch, bus topic, service unit or message field was added.

Native compatibility run **40050840938** failed during setup: Git rejected
checkout ownership before artifact download or any case execution. The raw
failure remains here. The correction scopes `safe.directory` to the runner's
exact checked-out workspace for Git reads, without a global wildcard. The
ordinary ARM64 and Linux Clippy runs are independent and were still running
when this receipt was first written; Clippy subsequently finished green before
publication. Original output whitespace is preserved in the raw text files.
No lamp, acoustic, real-task completion,
physical power-cut or independently trusted signature result is claimed.
