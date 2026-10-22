# Voice CI appendix, 22 October 2026

Ready for lamp-voice: **no, [#3023](https://github.com/autonomous-ai/lampOS/pull/3023)**.
Linux ARM64 qualification and the second Clippy attempt passed. The existing
Mac consumer failure and 18 failures in the candidate parallel library run
remain unresolved. Source head is `735b2cb5db21176a0ad80fcb42c48e739f8362fe`,
with authored parent `ec5428a34c0f66688e54200473cf26e6c3dd2b88`.

| Qualification | Run / attempt / actual job | Workflow and job conclusion |
|---|---|---|
| Linux ARM64 | [39816208565](https://github.com/autonomous-ai/lampOS/actions/runs/39816208565) / 1 / [119955315181](https://github.com/autonomous-ai/lampOS/actions/runs/39816208565/job/119955315181) | success / success |
| Linux Clippy, original | [39816208251, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/39816208251/attempts/1) / 1 / [119955314523](https://github.com/autonomous-ai/lampOS/actions/runs/39816208251/job/119955314523) | cancelled / cancelled |
| Linux Clippy, one same-head retry | [39816208251, attempt 2](https://github.com/autonomous-ai/lampOS/actions/runs/39816208251/attempts/2) / 2 / [119979869875](https://github.com/autonomous-ai/lampOS/actions/runs/39816208251/job/119979869875) | success / success |

Both successful jobs checked synthetic merge
`482eedcaf1e4c21c87b5752d50a57167097c603c` into
`0de443066a369eddd21a84f9416fcfc7fd74d316`. Raw checkout declarations and full
commit hashes are retained. This merge identity is distinct from the authored
branch parent; it does not establish current-base ancestry for a later merge.
The first Clippy attempt never reached checkout and has no tested merge.
All terminal workflow and actual-job records are included; these records have
no pending required job. No new test or job was started to prepare this appendix.

ARM nextest completed **10,552 passed / 0 failed / 67 skipped**, including one
passing `LEAK`. Parsing only the primary output before its first summary finds
exactly 10,552 unique binary/case rows. Voice library contributes **3,414**
passing rows; all seven assigned cases pass. Update's self-cut case passes in
**1.091 s**. [Summary and exact selections](arm64-summary.json) and
[selected raw lines](arm64-selected-raw-lines.txt) retain those observations.

This is a **full-workspace PR-profile parallel run**. The actual command uses
nextest profile `pr`, four reported cores with `-j num-cpus`, and partition
`hash:1/1`. Profile `pr` has `default-filter = all()`, including the realtime
group; that group runs one at a time alongside other work. The source-head and
base-parent configuration blobs are byte-identical, as recorded in the
[profile receipt](nextest-profile-source.json). This is a parent-blob check,
not a direct read of the synthetic merge file. The command-line filter excludes
`lamp-ipc::relay_signals` and `lamp-turn-end-wire::process`; `PICKED` is empty.
This is nextest process execution, not the Mac shared-process default-parallel
libtest command or the workflow's `full=true` serial mode. The retained
[command excerpt](raw/arm64-excerpts.json) shows the exact selection and
67-test/two-binary skip count. The 67 reported test skips are not a realtime
exclusion count; the primary output does not enumerate every skipped case.
The two excluded own-harness binaries run separately afterward.

[Separate results](arm64-additional-tests.json) preserve all three following
commands and their [complete raw step output](raw/arm64-doctests-own-harness.json).
Workspace doctests passed **4/0/0** across 46 harness summaries (Bus 2, IPC 1,
Msgs 1; the other harnesses reported zero tests). `relay_signals` printed
`relay_signals: ok` without a numeric case count. Turn-end-wire's `process`
harness reported **12 passed / 0 failed**. The terminal combined step is
success. These outcomes are separate from the 10,552 nextest count.

The logs are **not warning-free**. [Warning receipts](warning-summary.json)
retain output-filename collision diagnostics for `replay_cost`, `progress_cost`
and their `.dwp` outputs: four paths, each reported in the ARM release build
and again in nextest compilation. Action-runtime Node.js deprecation warnings
also appear; the Clippy retry includes rustup's existing-settings warning.
The jobs still concluded success, and the strict Rust Clippy command passed.
No source fix or warning suppression was made in preparing these receipts.

Clippy attempt 1 was cancelled during `apt-get update`. Checkout, Rust
installation and Clippy were skipped. The full attempted apt step through
cancellation and cleanup is retained in
[lossless diagnostic excerpts](raw/clippy-attempt1-excerpts.json). It ran for
roughly 45 minutes, but the API conclusion is **cancelled**; no more specific
cause is asserted and it is not a lint result. This failed setup attempt is
preserved alongside, not replaced by, the same-head retry.

Attempt 2 installed Rust `1.96.0-x86_64-unknown-linux-gnu` and completed
`cargo clippy --locked --workspace --all-targets -- -D warnings` in the
reported **58.76 s**. [Qualification excerpts](raw/clippy-attempt2-excerpts.json)
include the exact merge, toolchain line, complete Clippy command and output
through its Finished line. The terminal Clippy step, actual job and workflow
all report success. **A separate Linux formatting execution was not run.**
Installing rustfmt during toolchain setup is not a formatting check.

The full Mac consumer command remains **1,916 / 1 / 11**, exit 101. The fixed
EC/735 parallel pair remains **3,384 / 0 / 14** versus **3,366 / 18 / 14**;
all seven assigned cases passed both. [The complete fixed-pair receipt](https://github.com/autonomous-ai/lampOS/tree/fb175b1c6c8de028bc963b392722fc55bb89d6e8/lampOS/docs/verification/voice-ec-parallel-20261022)
retains raw failures, source maps, parser recovery and architecture limits.
Neither Linux success nor a single focused Update baseline pass clears those
failed Mac gates or establishes causality. The Mac receipt's process context
reports Darwin x86_64; physical hardware ISA and translation were not established.
The [separate unchanged-current-head control](https://github.com/autonomous-ai/lampOS/tree/e8f065cb61c5964f48cb75bbd04d0bbb333cf669/lampOS/docs/verification/voice-current-parallel-control-20261023)
at `bad114496` failed **3,387 / 7 / 14**, with two named overlaps with the
candidate failures. All seven assigned cases and kept-start passed there.
That changed-base observation establishes those two symptoms without this
patch; it waives none of the 18 candidate failures or the Update failure.
Older before/after ARM results used different upstream snapshots; this larger
inventory is not a same-inventory timing comparison.

[Qualification metadata](qualification.json) records each external full log's
path, byte count and SHA256. ARM's **1,803,747-byte** log remains external. The
**4,741-byte** first Clippy log and **33,131-byte** retry log also remain
external; portable excerpts omit unrelated runner/account/authentication setup.
API log availability may expire. Without those external files, readers can
verify the included segments but cannot reconstruct or independently hash the
complete logs from these excerpts. Terminal API records retain only selected
qualification fields and steps, with original metadata hashes; actor, author,
account and credential fields are omitted.

Each raw JSON segment contains exact original UTF-8 lines, inclusive source
line numbers, byte count and SHA256. Join `text_lines` without a separator to
recover its bytes. No excerpt line was rewritten. [Validation](validation.json)
checks terminal conclusions, checkout identities, primary-row counts, raw
segment hashes, field scope and local links. [Manifest](manifest.json) hashes
all appendix files except itself. No runner code, binary, Cargo machine-JSON
or device evidence is included.
