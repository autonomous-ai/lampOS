# Memory release 6408 ↔ 6449: actual-binary controls

Part of #2809. The revised full-record fixture passes both actual published Memory **6408 ↔ 6449 cases: 2/0/0**, with **41 successful RPCs per direction**, on tested source tree `6b42fd91e4ed4e0e4b52194cbb257ac036f163a0`, based on `d7b85b1dd`. Fresh Mac and Linux formatting, strict Memory all-target Clippy, full Memory **314/0/8 ignored** and workspace builds pass. [Complete raw qualification evidence](https://github.com/autonomous-ai/lampOS/tree/eadd636ba7/lampOS/docs/verification/memory-survivor-recovered-20261024) binds the executed driver, source, assets, exports and cleanup. Official ARM64/Clippy on the pushed revision and renewed review remain required.

The fixture is `crates/memory/tests/release_pair.rs`. Earlier historical runs and their weaker survivor assertions are retained below; their results do not qualify the review repair. Records/session cleanup and whole-update/lamp acceptance remain separate.

First selected build **6408** as the rollback floor and the first published
post-update build **6449** as the current side. These Memory executables are
byte-identical: **4,738,136 bytes**, SHA256
`9f595301253679a6c9a1aea232d14b081147fe966d87b6776899ede19ccb8deb`.
The fixture requires both explicitly named inputs, checks their size/digest and
ELF64 little-endian AArch64 headers, then executes private read-only executable copies.
This proves actual published-reader behavior when run; it does not pretend two
labels are different implementations or substitute a rebuild of old source.
“Current” below means release 6449, not the source head on which this fixture is
compiled.

## Artifact binding

| Release | Source | Memory asset | Manifest envelope SHA256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 668607764 | `696c6ae70a5cdf164c4a319bdc0887e5d1286519c36157a9c0c8db2cc0d80734` |
| 6449 | `be16c3327524ccef8a1908a3b8ebf40c16efba8a` | 669530921 | `2f8f218c41b74576b52451248f0df2f9251634bfc68658b42062282bc0c68a6a` |

The two public binary downloads match those digests, sizes and ELF headers. Both
published envelopes decode to the selected sequence/source and the same exact
Memory file digest/size. This is a payload/manifest binding, **not independent
release-signature verification**: the envelope names key `54213f8d3aa2f241`,
and trusted-key acceptance remains separate. No artifact is vendored here.

## Two fixed cases

- `release_6408_reads_current_days_and_forget_survives_current_reopen`:
  6449 writes, actual 6408 reads and forgets, then 6449 reopens.
- `current_reads_release_6408_days_and_forget_survives_release_6408_reopen`:
  6408 writes, actual 6449 reads and forgets, then 6408 reopens.

Each case owns one disposable private store and runs one Memory process at a
time, with `--memory-days --utc-offset-minutes 0`. Requests use its real Unix
socket. A status reply establishes serving readiness; writes are acknowledged
before the owned process is killed/reaped and the other version opens the same
database. This is process reopen after acknowledged writes, **not a physical
power cut or a mid-transaction crash test**.

The fixed synthetic corpus contains two person scopes, the room and one person's
rules scope. It checks exact saved record fields/IDs/provenance links and the
persisted session snapshots. Forgetting a kept fact removes its cited exchange
and linked descendant, invalidates only its person's old snapshot, and leaves
unrelated kept data and the other person's snapshot intact. Forget-person then
removes that person's remaining entries and rules atomically. The original
reader must find no resurrection, preserve the room/other person's entries and
snapshot, and allocate a strictly higher ID for a fresh write. Read-only SQLite
checks after each stop verify integrity, version 1, exact entry counts, foreign
keys, provenance without orphans, private modes, and no unexpected newer-DB
archive/fresh-store fallback. No model or external service is used.

These cases do not qualify every Memory feature or future schema. In particular,
JSONL import is one-way: enabling SQLite imports and deletes legacy files before
serving, and a JSONL-only reader cannot reopen that directory. Build 6408 is
already a SQLite reader. A higher DB version is archived and a fresh store
starts; that preserves bytes rather than restoring active memory. Existing
migration, future-version, replacement, follow-up and observation tests remain
separate and unchanged. These cases do not claim an observation-extraction or
legacy-import release-pair result.

## Execution and resource boundary

Both tests are ignored by default because they require explicit artifact inputs
and exclusive execution. Linux with native or already configured kernel binfmt
AArch64 support is required; failed exec/unsupported hosts are failures, never a
silent pass. The Rust driver may be Linux AMD64 while the actual Memory processes
are AArch64. No Docker CLI, network bridge, extra transport service, production
flag or permission bypass is added by the fixture.

After an explicit target grant, run inside the approved Linux environment with
the artifacts mounted read-only and a disposable writable scratch directory:

```sh
LAMP_PAIR_6408_MEMORY=/artifacts/6408/lamp-memory \
LAMP_PAIR_CURRENT_MEMORY=/artifacts/6449/lamp-memory \
cargo test --offline --locked -p lamp-memory --test release_pair \
  -- --ignored --nocapture --test-threads=1
```

The original per-case work budget is 120 seconds, startup readiness at most
10 seconds, each reply at most 5 seconds/65,536 bytes, and owned process
kill/reap at most 5 seconds. Reads retain one absolute reply deadline even for
partial records. Each phase retains stderr and complete synthetic RPC JSONL,
with per-request elapsed time, in the printed private scratch folder; a terminal
receipt is written only after all assertions and three confirmed reaps. Failed
evidence is retained. No raw person data is used. External runner/container
cleanup and a whole-process ceiling remain required containment for blocking
kernel/process/filesystem operations; the fixture does not promise a userspace
hard deadline for those operations.

Request durations are local socket/SQLite boundaries including response parsing
and exclude container startup. They are not lamp, acoustic, model or interaction
latency measurements. No performance acceptance threshold is inferred.

## Checks and holds

The corrected scoped Linux gate passed on the exact tree above:

- Both explicit actual-binary cases: **2 passed / 0 failed / 0 ignored**.
- Ordinary full Memory: **314 passed / 0 failed / 8 ignored**, including the two manual cases ignored in this separate invocation.
- Strict Memory all-target Clippy and workspace formatting: exit 0.
- Source guard: all 10,311 tracked files retained bytes, modes and mtimes; semantic index unchanged. All five owned containers were removed.
- Each direction retained 30 successful RPCs and three confirmed process reaps. The complete 276,480-byte export, SHA256 `1494336ed748cd7faeeebce5d066519be894195b1bfce4f099a9142bb0fb5c58`, holds 14 regular files. Four regenerated executable copies were verified before omission; no other evidence files were omitted.

The driver ran in offline rootless Debian 12 AMD64, with actual AArch64 release executables through existing kernel binfmt and Unix sockets in native `/tmp`. This is not a native ARM64 host result. The selected compiled test driver used `--ignored --show-output --test-threads=1`; its two-case internal duration was 2.48 seconds, while the outer test/export stage was 4.21 seconds. Full Memory took 82.00 seconds. [Published evidence](https://github.com/autonomous-ai/lampOS/tree/7d99d8afd2/lampOS/docs/verification/release-pair-progress-20261024) retains the exact receipts, failures and source binding.

The first status RPC once the ready file and socket were available took 14,377 / 22,047 / 21,185 microseconds across the 6408→6449→6408 phases, and 13,958 / 18,375 / 17,187 microseconds in the reverse direction. Each measurement starts immediately before socket connection and ends after complete JSON parsing, before evidence I/O. These are six host observations, not startup time, an acceptance bound or lamp UX latency.

The previous fixture carry onto base `9ed44f88c`, keeping the actual-pair driver byte-identical (SHA256 `321113c0513df44ebec303b8fb6be8f1c434ffc558aaf00393f639efa6b66a72`). Current candidate tree `69bf9af35f7a79b4f67f4dfc74e4bf794307b1c1` passed workspace formatting on both hosts, strict Memory all-target Clippy on Mac, strict workspace all-target Clippy on Linux, the Linux workspace build, and fresh full Memory tests **314/0/8 ignored on each host**. The fresh suite covers the changed Voice transport used by Memory integration tests; the unchanged direct actual-binary pair was not repeated. All 10,315 tracked files retained bytes, modes and mtimes, semantic index stayed unchanged, and all owned processes/containers were cleaned up. This final result-only README update changes no executable source. Exact-head native ARM64 and Clippy PR gates remain required before readiness.

The initial current-base gate launcher stopped in preflight because its validated release-receipt projection omitted the target field. No Cargo/container/target writer started in that failure. The scratch bookkeeping correction preserved source and commands before the successful current gates. The earlier `db30c396c` source also separately passed strict Linux workspace Clippy and workspace build; neither that result nor the actual **2/0** artifact result is relabelled as a current native ARM64-host run.

Release signature trust, Records' actual systemd/procfs/openat2 cleanup pair,
Voice-owner read, attended whole-lamp update/unplug and First's acceptance remain
outside this fixture. Neither this document nor ignored tests close the
row.


## Retained failed attempts and correction boundary

Both earlier attempts used source tree `9d7e75d67ac6b2de3df296c39c1870474c5a030c`.
The initial compile passed. The first actual pair was **0 passed / 2 failed**
before readiness, with `EINVAL` and no fixture RPC. A native infrastructure
control established that the host evidence mount allows socket bind but rejects
chmod to 0600 with errno 22; the identical private-directory/bind/chmod/rename
sequence succeeds in native tmpfs. A bind-only control had not exercised that
required chmod. Two separate diagnostic setups failed before their intended
observation (non-executable original download and unavailable Python); the
corrected QEMU stderr trace was affected by the guest's stderr relay and did not
locate the original error. Those records are preserved, without an unsupported
emulator-only or historical-cause claim.

The runner-only native-tmpfs attempt reached real Memory protocol work but was
**0 passed / 2 failed** at `forget_person`: the original fixture used generic
Memory IDs (`p-a1`/`p-b1`) rather than the stricter Eyes ID shape required by that
operation. The fixture now uses two synthetic `p-` plus 16-hex IDs, checks the
actual known-ID predicate and exact corresponding rules scope before use, and
keeps every existing semantic expectation and deadline unchanged. No production
parser, permission or forget behavior is relaxed.

That second attempt also had an **incomplete evidence export**: test exit 101,
export timeout 124, effective gate exit 125. Raw test output and a partial
2,048,000-byte archive remain, but complete RPC/database retention is not
established. It is not complete failed-case evidence. Source guards and named
container cleanup passed; full Memory/Clippy/Cargo-fmt stages did not run after
the stopped gate.

The corrected runner executes each entire case in native `/tmp`, retains regular stderr/RPC/database/terminal evidence, and verifies the pinned size/hash of regenerated executable copies before omitting only those copies from export. Archive names/types, exit receipts and both complete terminal receipts were checked before the corrected run was accepted. The original test/export deadlines were not enlarged. The successful result does not erase either preceding failed attempt.

## Survivor-record review repair and fresh execution

The historical execution checked A's complete initial records but only survivor IDs after forget/rollback. Reviewers identified that as insufficient to qualify survivor content and scope preservation. The revised fixture captures complete A-unrelated, B and room records from the original writer and compares every JSON field and order through opposite-reader, targeted-forget, forget-person and return-reader boundaries. Existing ID, snapshot, count and fresh-write assertions remain. B's note must contain its own synthetic canary and exclude A/room canaries. Both strengthened cases passed in the fresh recovered execution above.

The exact tested fixture SHA256 is `1f551d5b71416047d609e6b65f6519084725877bc0bbb6b146454acf9339af6f`; compiled Linux test driver SHA256 is `6c8f62a35e6d278d1684f3400c10015528153735681d7e7e5dc21e656b51a64d`. Raw archive SHA256 is `6619cb8d9c505c4f352d8aff546bf481e08dd29fbe117b9fffdf09607b4cd90d`. Every tracked source file retained bytes, modes and mtimes through the fresh gates and pair execution; semantic index, owned-container removal and host process-group absence receipts pass. This final result-only README update changes no executable source.

The six reaps are fixture terminal assertions backed by source finish calls, not an independent raw child-event trace. Both raw binary databases and archive remain local; the published packet contains complete logical SQL/all-table JSON exports with original byte hashes and integrity/version/foreign-key receipts. FTS SQL-dump restoration is not claimed.

The initial recovered Linux attempt failed connecting to the stopped local VM before Cargo or a released executable ran; the complete fresh attempt supersedes its qualification and retains its failure. Earlier ENOSPC failures and unpublished pre-cleanup receipts were lost when the temporary workspace was cleared; their committed historical observations remain, and are not used as fresh proof. [Original 30-RPC evidence](https://github.com/autonomous-ai/lampOS/tree/c189a237e6/lampOS/docs/verification/memory-raw-pair-20261024) remains independently auditable.
