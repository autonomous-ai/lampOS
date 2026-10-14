# Fourth builder's log

Fourth writes `lamp-fourth` and works on `fourth/<task>`. The task list is
`docs/work-split.md` on `origin/lamp-voice`; Fourth never edits it. All
results here are from the development machine unless explicitly attributed
to the first builder's lamp measurements. There is no lamp access here.

## 2026-10-14: task 13, memory-days — taken

- Taken from `origin/lamp-voice` at
  `aceb39518ea3e11c5aa8acb01130dd89eae26a89`.
- Task branch: `fourth/memory-days`.
- Scope: `crates/memory` and its English documentation. Leave diagnostics
  to task 8. No changes to the voice, other builders' crates, or `deploy/`.
- Build ranked retrieval, dated and linked observations, transitive
  forgetting, expiry, a bounded session note, and a post-conversation pass
  behind a model interface with scripted tests. Keep changed behavior off
  unless explicitly enabled. Run the public LongMemEval-S retrieval
  comparison and local scripted multi-day cases without a model or key.
- Evidence planned: pre-change workspace build and memory gate, regression
  and failure tests, baseline versus ranked retrieval on one harness,
  first and repeated request timings with exact machine and boundaries.
  Voice callers already allow 300 ms for their initial note and 1.5 s for
  a tool request; target reads below 50 ms p95 with 6,000 exchanges on this
  development machine, leaving device performance explicitly unverified.
- The hand-back will contain exact lamp commands, expected results, and
  likely device-specific failure points. Questions and blocked dependencies
  will be recorded here while independent work continues.

## 2026-10-14: task 13, implementation and evidence

Implementation is on `fourth/memory-days`; the final gate below passed
before fast-forward to `lamp-fourth`. No new code has run on a lamp.

Built inside `crates/memory`: an off-by-default `--memory-days` SQLite
store with FTS5 ranking and English/Vietnamese normalization; source-dated
observations and supersession history; transactional transitive forgetting;
unused-observation expiry; frozen 1,200-byte session notes; person-scoped
records and jobs; bounded conversation prepare/apply operations; and a
nonblocking model interface driven by scripted tests. No real adapter,
network call, key, account, payment, voice wiring or deployment was added.
The default JSONL mode remains available for unmigrated directories.

The public evaluation is a separate Rust binary in the same crate. On all
500 cleaned LongMemEval-S questions, official user-only session retrieval
had 419 eligible questions: whole-word recall-all@5 69.69%, FTS5 81.86%.
Both-speaker sessions had 470: 66.38% and 82.13%. Full context is only a
retrieval ceiling; no answer model was used. The report includes other
cutoffs, true turn-level retrieval, question types, exclusions, byte counts,
dataset hash and pinned sources. A separate calculator checked 42,672
metric values across 1,778 scored question/track records. See
[memory-evaluation.md](memory-evaluation.md) and its compact JSON artifacts.

**Machine:** development Mac, macOS 26.6.2 (25G83), x86_64 Intel Core
i7-10700K at 3.80 GHz, 16 logical CPUs, 64 GiB RAM. Rust/Cargo 1.96.0,
host `x86_64-apple-darwin`, bundled SQLite 3.53.2. Cargo is installed at
`/Users/autonomous/.cargo/bin/cargo`, outside this shell's PATH. Timings
and scores in this entry belong to this machine, not lamp-4ace.

**Retained failures and corrections:**

- The first bare `cargo` invocation failed with exit 127 because of PATH;
  subsequent commands used the absolute executable above.
- Baseline socket tests and timing failed with permission errors in the
  sandbox. Approved local-socket test runs outside that restriction passed
  the original 36 tests. The pre-change workspace build passed too.
- Initial SQLite compile errors required rusqlite's `fallible_uint`
  feature for checked unsigned conversions; a Clippy copy warning was
  corrected. No ignored lint was added.
- The first full public evaluation rejected duplicated distractor session
  IDs. Thirteen questions have them. The harness now preserves occurrences
  with a regression test; the failed log remains in the local research
  directory described by the evaluation report.
- The first paired timing attempt used a fixture source timestamp a few
  seconds after its injected clock. Preparation correctly refused it.
  The fixture now selects an exchange that has already happened.
- Review exposed blank search returning newest entries, delayed old
  evidence superseding newer facts, relevant old preferences falling
  outside the 32-item comparison window, snapshot invalidation on an empty
  person's forget-all, a migration/default-open race, and rejected tidy
  requests creating empty person metadata. Each was corrected and covered
  by a regression. Transaction fault injection covers partial proposals
  and deletion failures.
- Two new tests initially demanded exact repetition of prior-selection
  hints. The actual contract is immutable prepared input: omission returns
  the same input; a new prior absent from it is refused. Tests now verify
  that contract and reject context mutation.
- The durability review changed SQLite from FULL to EXTRA plus fullfsync,
  and a live-connection test checks the requested modes. Earlier timings
  under the weaker settings are not the final timings.
- The first full final gate caught module ordering in `cargo fmt` and a
  legacy documentation test consuming opt-in examples. Ordering was fixed;
  both documentation sections now have their own protocol checks.
- The Linux ARM64 build attempt stopped at E0463, missing the target
  `core` crate. No ARM64 binary or successful cross-build is claimed.

**What the first builder must do:** follow
[memory-days.md](memory-days.md#native-build-and-first-lamp-checks). Build
with an ARM64 Linux C toolchain for bundled SQLite. On the lamp, from a
source checkout with that toolchain, run `cargo test -p lamp-memory`, then
`cargo test -p lamp-memory --release --test days_timing -- --ignored --nocapture`.
These tests use disposable directories. The binary test exercises migration,
append, scripted observation, linked reply, SIGKILL/restart, forget,
restart, stale-job rejection and the no-flag downgrade refusal. All should
pass, with no forgotten text returned and IDs never reused. The timing
test emits first, repeated, p95 and maximum socket times for both stores,
plus import/reopen cost and database size. Record lamp hardware and build
separately. Physical power interruption still needs a disposable-store test.

**Most likely to be wrong on the lamp:** native C linking and SD-card sync
latency; migration before readiness; lexical Vietnamese ambiguity; the
future model's semantic correction choices; and voice callers failing to
send provenance IDs or discard previously supplied provider context after
forgetting. Exact new JSON request lines and caller obligations are in
[memory.md](memory.md#additional-requests-with---memory-days) and
[memory-days.md](memory-days.md#request-contract).

## 2026-10-14: task 13, memory-days — done on Fourth's branch

The final gate ran from `lampOS/` with
`/Users/autonomous/.cargo/bin/cargo` for each `cargo` command:

| Command | Final result on the development Mac |
|---|---|
| `cargo fmt --all --check` | Pass |
| `cargo clippy -p lamp-memory --all-targets -- -D warnings` | Pass, no warnings |
| `cargo test -p lamp-memory` | 106 passed, zero failed; two opt-in timing tests ignored by the normal gate |
| `cargo build --workspace` | Pass; pre-change workspace build also passed |
| `cargo tree -i lamp-memory --offline` | No Rust crate depends on lamp-memory; socket clients retain old response shapes |
| `cargo build -p lamp-memory --offline --locked` in the standalone source copy | Pass |
| `cargo test -p lamp-memory --offline --locked` in the standalone source copy | 106 passed, zero failed; the same two timing tests intentionally ignored |
| `cargo test -p lamp-memory --release --test days_timing -- --ignored --nocapture` | Pass, one final paired timing invocation after functional suites finished |

The standalone copy is `/private/tmp/lamp-fourth-source-99_tldlv`, containing
workspace manifests, crate sources and the in-tree documentation fixture,
outside the parent repository, with its own target directory. Socket tests
needed approved execution outside the sandbox's local-bind restriction.
No Go or web source changed, so their language-specific gates do not apply.

Final timing boundary: connect, serialize/send request, read complete reply;
JSON decoding excluded. Each operation ran once, then 50 repeated calls.
Filesystem cache was not evicted. With the same 6,000-exchange/200-kept
fixture, keyword-recall median/p95 was 15.650/15.975 ms for files and
11.451/12.843 ms for SQLite. SQLite newest/day/kept reads were slower than
files, still below 7 ms p95. New search p95 was 12.924 ms; pending-model
recall p95 13.012 ms. Snapshot creation took 75.343 ms once, then 1.765 ms
p95 for repeated reads. Import took 1,848.618 ms once and reopen 80.086 ms.
All repeated reads met the stated 50 ms development target. The complete
table, artifacts, build size and limitations are in
[memory-evaluation.md](memory-evaluation.md#local-store-and-socket-measurements).

The public benchmark and these functional tests need no lamp or model.
There are no remaining failing tests or owner questions. The real-model
adapter and voice integration are explicitly left for the first builder's
lamp work as the task requires; the protocol and test double are complete.

## 2026-10-14: task 14, said-to-whom — taken

- Fetched the list again after publishing task 13. Rebased `lamp-fourth`
  onto `origin/lamp-voice` at `8335dfbce`; the memory implementation is now
  commit `b70c200c5`. The integration changes are outside memory and did
  not conflict. Only `lamp-fourth` was updated, with a lease.
- Next task branch: `fourth/said-to-whom`.
- Scope: `docs/said-to-whom.md` and this log. Research only; no runtime,
  model, data download requiring an account, device change or deployment.
- Read the latest attention, policy, group-talk and name documents, then
  study primary research and shipped-assistant/robot documentation for
  device-directed speech and addressee detection. Compare cues, published
  metrics and evaluation conditions, public recordings and reuse limits,
  then propose what this lamp can use and what it would need to measure.
- Published figures will be distinguished from the first builder's lamp
  observations. No new lamp performance will be claimed. Questions, if any,
  will be written here without stopping unrelated work.

## Not shown

No lamp access or deployment, Linux ARM64 execution, physical power-loss
test, real extraction model, speaker recognition, natural multi-day spoken
test, provider-context deletion, or end-to-end lamp latency. All new
behavior remains behind `--memory-days` until the first builder qualifies
it on the lamp. The log contains no unanswered question requiring the
owner to relay a message.
