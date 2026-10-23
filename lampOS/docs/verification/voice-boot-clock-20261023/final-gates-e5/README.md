# Current-base boot-clock Mac gates: terminal failure

**Ready: NO.** All six planned local gate commands completed on
`0f2e6439e018680af247080983c9ec0c258c3229`, based on
`e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc`. The runner finished with exit **1**:
the full Voice and reader test gates each exited **101**. Neither failure is
waived by the successful formatting, strict Clippy, workspace build or diff check.
There was no retry and no additional exact-case/timing run in this gate.

| Gate | Exit | Command wall seconds |
| --- | ---: | ---: |
| fmt | 0 | 11.757663 |
| clippy | 0 | 83.775037 |
| voice-default-parallel | 101 | 361.240300 |
| readers-default-parallel | 101 | 1219.158933 |
| workspace-build | 0 | 69.771501 |
| diff-check | 0 | 2.862386 |

These wall times include compilation where applicable. Per-case elapsed times
were not emitted; [targets.json](targets.json) retains emitted target times.
Two platform-skipped Update custom harnesses emit no target duration; their
complete ignored rows and summaries are retained with a null duration.
All commands stayed within their watchdogs. There are no unrun/incomplete targets,
active steps or unfinished local commands in this terminal receipt.

## Test outcomes

- Full Voice: **3,869 passed / 1 failed / 15 ignored**, 6 complete targets and
  3,885 case identities. Library: **3,589 / 0 / 15**. The only failure remains
  the light-call `<25 ms` upper-middle assertion at `tests/it/light_call.rs:90:5`.
- Eleven reader packages: **3,203 passed / 1 failed / 17 ignored**, 68 complete
  targets (57 test executables and 11 zero-case doctests), 3,221 case identities.
  The only failure is VoiceEval integration; that target is **237 / 1 / 0**.
- Combined candidate test execution: **7,072 passed / 2 failed / 32 ignored**,
  74 targets and 7,106 case identities. These are the selected 12-package gates,
  not a full-workspace test result. Workspace **build** succeeded separately.

The exact package commands and all target counts are recorded in
[commands.json](commands.json), [results.json](results.json) and
[targets.json](targets.json). Both test commands use default parallel libtest
execution, `--no-fail-fast`, and no test-thread override. Strict all-target
Clippy covered Body, Cloud, Expression, Keeper, Lean, Memory, Peek, Setup, Skills,
Update, Voice and VoiceEval with `-D warnings`.

The [published Voice pair](../full-comparison-e5/README.md) retains all 86 selected
passes in both arms, all ten baseline failure comparisons and the historical
five-failure comparison. The [e5 baseline](../baseline-e5/README.md) remains
3,860 / 10 / 15, failed and unwaived. This terminal receipt supersedes only the
pair receipt's then-pending overall gate status; it does not rewrite its evidence.
[Published dependency pins](published-dependencies.json) identify the immutable
publication commits and manifests.

## Sole reader failure and its source boundary

`relay_hardening::a_closed_runner_transport_stops_the_runtime_and_its_workers`
panics at `crates/voice-eval/tests/it/relay_hardening.rs:126:54`:

```text
called `Result::unwrap()` on an `Err` value: ParseIntError { kind: Empty }
```

This is the shared `read_pid` helper: the file read succeeded, then its trimmed
text could not be parsed as an integer because it was empty. The failure occurs
while evaluating the final `process_gone(read_pid(...))` assertion's argument;
that worker-gone check was not completed. The earlier elapsed-time, exit-code,
killed/error and trace-count assertions had not panicked. The complete diagnostic
is retained in [failures.json](failures.json) and full reader stdout.

The whole VoiceEval crate, including this fixture, its private-directory helper,
Cargo manifest and `physical/session.rs`, is byte-identical between e5 and the
candidate. [Source audit](reader-source-audit.json) records exact hashes/blobs,
with [line-numbered excerpts](reader-source-excerpts.json). This case explicitly
runs `/bin/sh` as its fake runtime, not lamp-voice. The relay uses unchanged
Voice option/Continuation types. The audited path reads its generated trace and
PID file; it does not read the eight authored test/helper/doc files. The changed
normal-build clock expression still uses `lamp_ipc::monotonic_us`; the added
fixture clocks belong to test modules or `cfg(test)` branches.

A plausible ordering is visible in the unchanged fixture: its closer drops the
runner transport as soon as `worker.pid` **exists**, while shell redirection
creates/truncates that file before `echo $!` writes the PID. The relay then kills
the shell group when the transport closes. Closure between creation and writing
can leave the empty input observed here. This is source reasoning, **not a
controlled reproduction or proven cause**. The PID file bytes were not captured.
No unchanged-e5 reader suite was executed in this comparison (the baseline was
Voice only). Therefore this receipt does not label the reader failure inherited,
attribute it to host load or waive the failed gate.

The separate [historical empty-PID receipt](../relay-pid-history/README.md)
retains an earlier occurrence at this exact diagnostic boundary. Its historical
tree had a Memory-note overlay and lacks complete source/artifact provenance.
It supports prior occurrence, not a matched-base reproduction, causal diagnosis
or exemption from this failed gate. Its independently verified manifest is
pinned in [published-dependencies.json](published-dependencies.json).

## Source, environment and completion proof

[Source proof](source-proof.json) independently reconstructs all **9,992 tracked
paths** and exact modes/content from the committed Git tree, including root
fixtures and the literal tracked symlink. Every entry matches the captured source
map. The full aggregate is
`8a578a9ee91a8bc541bf4bebef46dda8cdee53afa48738600d67542c47e7034c`
before freshness refresh, after every gate and at final completion.

This gate used unmodified committed source and applied no source mutation, so
no overlay restoration was needed. Final content/mode proof is equal to the
initial proof. The separate baseline's borrowed phone-worktree restoration remains
in its own receipt. No live source or MAIN artifact was read during packaging;
MAIN may already have been reused for another authorized gate.

[Environment](environment.json) now fills the completed Voice step's earlier
scope limit: Rust 1.96.0 `x86_64-apple-darwin`, prescribed environment and target
settings were captured and matched to baseline. Build jobs 4, debug 0,
incremental 0, offline/locked resolution and cleared test-thread/target/flag
overrides are preserved. All 62 listed test binaries have captured Mach-O x86_64
identity. Physical hardware/translation and matched host load are not established.
Local PATH and host-node/build metadata are omitted; diagnostic streams remain
lossless and unredacted.

## Raw output and census

The [reader census](reader-census.json) binds all 57 actual test artifacts to
full raw list and ignored-list outputs. All execution rows, ignored names and
68 summaries match their independent inventories. The Memory ` - should panic`
display annotation resolves only to its unique compiled identity; no case is
dropped. The Voice pair retains the corresponding proof for its five artifacts
and three annotations. Listing is never counted as passing execution.

[Artifact metadata](artifacts.json) retains the 81 reader and 47 workspace-build
executable identities. The published Voice pair retains its 13 artifact records.
Clippy emits no executable artifact in this capture; it is proven by its successful
command and compilation record. [Compilation](compilation.json) retains all
build-finished outcomes and diagnostic metadata separately from test results.

[Commands](commands.json) contains 177 newly packaged commands (six non-Voice
root/census-independent commands plus 171 reader file/list commands); the 16
Voice commands are reused from the pinned sibling receipt, for **193 commands**
and **386 original stdout/stderr streams** audited in total. Full reader human
stdout/stderr and all remaining gate streams are lossless. Only recognized,
complete Cargo machine-record lines are omitted from portable stdout, with
original byte offsets/lengths/hashes and the full original stream hash/path.
No test/failure/progress text is omitted. Compiler stderr remains complete.

[Raw payloads](raw-payloads.json) gives exact payload offsets/lengths/hashes.
[Completed run seal](completed-run-seal.json) pins every original run file.
[External files](external-files.json) identifies bulk machine streams, full
source maps and redundant metadata by original path, size and hash. No scripts,
binaries or Cargo machine streams are packaged. Earlier failure receipts remain
intact, including the candidate Voice step's shell missing-file diagnostic.

[validation.json](validation.json) records the independent checks;
[SHA256SUMS](SHA256SUMS) covers this publication folder. Publication and any
further failure diagnosis remain separate actions owned by the parent.
