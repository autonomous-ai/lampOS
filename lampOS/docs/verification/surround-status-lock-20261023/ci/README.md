# Surround status-lock fixture: terminal Linux CI

PR [#3165](https://github.com/autonomous-ai/lampOS/pull/3165), authored head
`7a36051bd80d643d502c069cfeb77fa0604e6bd9`. Both workflows and actual jobs
completed successfully, attempt 1:

- ARM64 [39879786924 / 120174115863](https://github.com/autonomous-ai/lampOS/actions/runs/39879786924/job/120174115863).
- Strict Linux x86 Clippy [39879786906 / 120174115540](https://github.com/autonomous-ai/lampOS/actions/runs/39879786906/job/120174115540).

They checked merge `d03a53dc464696823b46019084de8d6cf809480f` onto
`f477d6c49732dbd5cba84cbe18ea568692c056e6`. Its recorded parents and tree
`c7a854b66d8950852e6253634b38d9f338ef7da9` match a local `git merge-tree
--write-tree` of those two exact parents. This is a composed-head qualification;
it is not an assertion that the authored head already contains that base.

ARM64 selected the whole workspace because the verification receipt contains
JSON. Its actual command was release nextest, profile `pr`, four-core parallel,
partition `hash:1/1`, excluding only the two custom harnesses run separately.
**10,823 passed, 0 failed, 67 skipped**, in **188.589 s**, including one
passing LEAK in Voice's existing journal fixture. The raw primary interval
contains exactly 10,823 unique identities: 10,822 PASS and one LEAK.
Surround has **265 passes** (2 library, 263 integration). The changed
`worker::slow_network_does_not_hold_status_lock_and_join_has_a_fixed_deadline`
passes in **0.035 s**. Separate doctests **4/0**, process harness **12/0**,
relay harness ok. Strict Linux Clippy covered every crate and target.

The source PR records Mac full Surround **265/0**, Mac formatting, strict
Clippy and workspace build, plus separate Linux formatting and all four
controls. This appendix does not repeat those commands or waive a source read
or later base drift. No CI rerun, local ARM64 build or device operation occurred.

All workflow/job metadata is complete. `summary.json` records exact command,
checkout and result lines, all binary counts and the changed test. The full
logs are lossless UTF-8 strings in `raw-output.json`; encode each `text` and
verify its byte length and SHA-256 to reconstruct the downloaded job log.
Artifact metadata is recorded, but its ZIP was not downloaded or independently
checked. `SHA256SUMS` covers each published payload. Executable target names
are nextest identities, not independent compiled-binary byte hashes.
