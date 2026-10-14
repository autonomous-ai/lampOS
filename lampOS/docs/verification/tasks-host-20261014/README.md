# Tasks host verification — 2026-10-14

This is Third's local evidence, not lamp evidence. Task base:
`fd6705dd9e107817b33309d777cfa2d5c85e4062`; claim `679d42b2f`.
The source hashes are in [gate-source-sha256.json](gate-source-sha256.json).
All 16 code/manifest hashes match both the final working tree and the
source-only copy used below. Documentation and this evidence ledger were
finished afterward without changing those sources. No binary is committed.

## Host and boundaries

Intel Core i7-10700K at the reported 3.80 GHz, 16 logical CPUs, 64 GiB RAM;
macOS 26.6.2 build 25G83 / Darwin 25.6.0, x86_64. The workspace pins Rust
1.96.0 (`ac68faa20`, LLVM 22.1.2) and Cargo 1.96.0. [Host commands](host.json).
Scratch `/private/tmp` is backed by `/dev/disk1s2`, APFS on
`/System/Volumes/Data`, not tmpfs: [df](host.json),
[selected mount entry](filesystem-mount.json). Initial metadata collection
ran from the parent checkout and saw its different default Rust 1.97.1;
sandbox sysctl reads failed. That raw [attempt](host-sandbox-root-context.json)
is retained; it is not the compiler used by the gate. `diskutil info` first
received an unsupported path, then its framework was unavailable in the
sandbox ([second attempt](filesystem.json)); `df` plus the selected `mount`
line establish the actual backing filesystem without that framework.

No live Harness executable, daemon, account, remote task, credential, model,
network, Voice or physical output was used. The executable fixture is Rust
and reads only its own private HOME plan. Confirmation and played receipts
are authored. Clock pauses/restarts in tests are injected/process events,
not real power loss, sound or remote execution.

## Required gate

| Check | Result | Command wall time |
|---|---|---:|
| Pre-task workspace build | pass | 26.218 s |
| `cargo fmt --all --check` | pass | 3.405 s |
| `cargo clippy --locked -p lamp-tasks --all-targets -- -D warnings` | pass | 3.014 s |
| `cargo test --locked -p lamp-tasks -- --test-threads=1` | **101 passed, 0 failed, 0 ignored** | 50.677 s |
| `cargo build --locked --workspace` | pass | 4.771 s |
| Release process + portable checker build | pass | 10.465 s |

The touched Rust package is `lamp-tasks`; no workspace package depends on it.
Only workspace membership/lock metadata and Tasks sources changed; no existing
crate was edited. Thus the touched/dependent package gate is Tasks, followed
by the required full workspace build. Each `gate-*.json` / `.txt` pair records
argv, cwd, UTC start, elapsed command time and unfiltered output. Test totals:
32 engine, 3 protocol, 10 runtime helpers, 1 live SQLite settings test,
23 adapter integration, 7 service/CLI integration, 3 process-failure fixtures,
and 22 storage integration. Cargo examples and doc-test harnesses had zero
separate cases; the executable qualification below actually runs its assertions.

## Standalone source copy

Copied 1,221 tracked or task-owned source/evidence files to
`/private/tmp/lamp-third-tasks-source-20261014`, without `.git`, prior `target`,
or parent-repository sources. Refreshed the final Tasks code and manifests
before building. [Copy manifest](source-copy-manifest.json) and
[metadata check](standalone-metadata-check.json) verify all 30 workspace
package manifests and all local path dependencies stay inside that copy.
Ordinary cached external Rust dependencies remain explicit.

- `cargo build --offline --locked --workspace`: pass, 83.977 s.
- `cargo test --offline --locked -p lamp-tasks -- --test-threads=1`:
  **101 passed, 0 failed, 0 ignored**, 41.929 s.

See [build output](standalone-workspace-build.txt) and
[test output](standalone-tests.txt). These are fresh copy build artifacts,
not binaries copied from the original target directory.

## Portable release measurement

Ran once after all Third builds/tests ended:

```sh
target/release/examples/tasks_check --scratch-parent /private/tmp --samples 8
```

Exit 0 in **13.998392566 s**; all assertions passed, private scratch removed.
Eight fresh-store cycles supply **72 raw durations**: one first and seven
subsequent values for each boundary. Two additional untimed scenarios check
malformed scripted JSON and unresolved pending receipt across process restart.
There are ten scripted `new` calls in total, sixteen `show` calls and ten
machine preflights. No real work was dispatched. Other host load was not
controlled. “Subsequent” does not mean the database was warmed: each cycle
creates a fresh private store. The earlier one-cycle debug preflight is
retained separately and is not pooled into these numbers.

| Boundary | First ms | Subsequent median ms | Subsequent max ms |
|---|---:|---:|---:|
| Fresh service / fixture startup | 220.371 | 219.479 | 221.780 |
| Prepare round trip to durable receipt | 98.459 | 86.018 | 87.838 |
| Confirm round trip to queued receipt | 132.785 | 82.716 | 86.183 |
| Confirm send to stored scripted creation observed | 309.902 | 250.181 | 264.601 |
| Observe send to stored scripted transcript observed | 306.758 | 250.706 | 275.755 |
| Claim round trip to durable grant | 78.916 | 80.827 | 84.278 |
| Authored played-ack round trip to durable receipt | 83.405 | 81.720 | 84.971 |
| Status round trip beside one silent client | 7.563 | 7.540 | 11.671 |
| Start reopened service to successful status | 101.304 | 105.184 | 112.867 |

[Raw output](release-check.stdout.json) includes every sample; the
[run record](release-check.json) identifies command, UTC start, source match,
profile binary hashes and sizes. The release process is 3,718,032 bytes;
the self-scripted checker is 3,801,880 bytes. The creation/observation boundaries
include local durable transitions, process execution and polling of committed
state; the confirm receipt returns before the separate creation observation.
The speech rows measure database attestations, not played sound. No cold-cache,
remote, acoustic, ARM64 or flash-latency claim follows from these samples.

## Failures retained and corrected

- Early compile attempts ran while delegated files were being written: absent
  Engine, an unfinished `agent_id` brace, or a temporarily absent receipt helper.
  [Adapter check](harness-check-first.txt), [store attempt](store-tests-first.txt),
  [intermediate compile](harness-tests-host.txt). The root's first check saw the
  same unfinished brace; its tool result was recorded in the session, followed
  by [check 02](check-02.txt). No test had run in those compile-blocked attempts.
- First service integration run: one pass and five Unix-socket `EPERM` failures
  under the tool sandbox ([raw](runtime-01.txt)). The host-permission repeat
  passed six tests ([raw](runtime-02.txt)); the later duplicate-field CLI
  regression brings the final suite to seven. The sandbox failure is separate
  from the real process-lifecycle bug below.
- First adapter suite and its host-permission repeat: **3 passed / 17 failed**.
  Darwin returned `EPERM` for a process-group signal when its direct child was
  already a zombie. This was reproduced independently, not dismissed as a
  sandbox failure. The pinned
  [XNU source](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_sig.c#L1612-L1621)
  skips zombie members then returns `EPERM` when none qualified. The host kernel
  identifies a later XNU build; the local [reproducer](process-repro.rs) and
  [v2 output](process-repro-v2.txt) establish the observed behavior here.
- After the narrow normal-exit fix, adapter run two was **16/20**. Capped-output
  cleanup had discarded pipe handles before proving EOF; it now retains and
  discards their bounded tails within the same cleanup budget. A fixture read
  also saw a partially appended JSONL line; the reader now accepts only complete
  lines. Run three was **19/21**: copied-executable tests' artificial one-second
  cold-start budget was insufficient. Those identity tests use the production
  ten-second read budget. An 80 ms preflight fixture was likewise too short.
  No cold-start performance claim is inferred from these fixture adjustments.
- The final adapter run was **23/23** ([raw](harness-tests-fourth.txt)). Darwin
  `EPERM` is accepted only with observed direct-child exit, both pipe EOFs and
  successful reap; live/held-pipe failures remain poisoned. Tests cover a live
  descendant with redirected output as well as one holding pipes. Descendants
  that escape the group or elevate privileges remain outside this wrapper.
  An invalid-format spawn fixture was replaced by missing CWD because this host
  did spawn the former; the new case proves a local spawn failure portably.
- First full gate: **44 unit tests passed / 2 failed** because two test helpers
  used temporary socket parents without explicit 0700 permissions
  ([raw](gate-tests-first.txt)). Corrected the helper to create private parents;
  the production permission check remained strict. Final full gate passes.
- Review found duplicate JSON keys were collapsed by the request CLI's temporary
  `Value`; it now parses typed `Request` first. Review also found terminal
  `eprintln!` could hold an error exit on a blocked sink; its writer now gets a
  ten-millisecond exit-report budget. The new storage-error process test passed
  on first execution ([raw](runtime-storage-failure-first.txt)); its final
  three-test suite verifies both sockets are removed, the replacement inode
  preserved and no CLI call made despite a full stderr pipe.

The gated failure-fixture timings are authored host experiments, not service
benchmarks or real disk stalls. Raw logs preserve output whitespace, including
trailing spaces/blank final lines emitted by Cargo; source/prose whitespace is
checked separately. No failing test or ignored new regression is accepted.
The [development manifest](development-evidence-manifest.json) hashes copied
raw evidence and records its original temporary paths.

## Rebase onto the new integration and CI rules

After the original handoff, fetched integration head
`462c18b924cd397f04df45f72503362b6a607b60` and rebased Third. First had already
adopted Schedule; its duplicate commit was skipped. Kept the newer Ears
workspace member and Memory reuse review. Tasks' code is byte-identical to
the original checkpoint: [source identity](rebase-source.json). The original
16-file hash comparison above describes the pre-rebase gate; the workspace
manifest/lock now also contain upstream additions.

Repeated the required gate on the same host after rebase, with no other Third
build/test running concurrently: fmt passed in 3.454 s, strict all-target Tasks
Clippy in 5.959 s, all **101 tests passed / 0 failed / 0 ignored** in 54.083 s,
and the full workspace build in 28.888 s. [Commands and exact durations](rebase-results.json),
[fmt](rebase-fmt.txt), [Clippy](rebase-clippy.txt), [tests](rebase-tests.txt),
[build](rebase-workspace-build.txt). The newly required native ARM64 CI is a
separate gate; its eventual run/result belongs in Third's log. It has not yet
been qualified by these Mac checks.

## What has not been shown

No lamp, live Harness/account, remote permission state, actual spoken yes or
sound was exercised. The current CLI still lacks atomic strict-ID dispatch,
reliable pending-ID reconciliation and request-correlated completion. No
exactly-once hearing, suspended/rebooted device, flash power loss, installed
0.3.94 compatibility, Linux process cleanup or lamp latency is established.
First's precise handoff and qualification list are in [Tasks](../../tasks.md).
