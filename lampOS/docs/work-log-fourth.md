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

## 2026-10-14: task 14, said-to-whom — done on Fourth's branch

**Built:** a research document only,
[said-to-whom.md](said-to-whom.md), from `fourth/said-to-whom` based on
`8335dfbce` plus Fourth's memory work. No crate, model, runtime default or
deployment file changed. The study distinguishes wake-word detection,
false-trigger mitigation, follow-up/open-room directedness, human addressee
selection and the recipient of a robot's own reply. Primary sources cover
acoustic streaming, ASR uncertainty, context, missing modalities, distillation,
multimodal LLMs, shipped assistants, robot studies and public recordings.

The recommendation is a local, turn-bound shadow decision using real existing
cues, with timely text as additional evidence. First establish IDs for all
speech candidates, including rejected ones; separate source and delivery time,
then evaluate real labels before fitting new weights. The hardware record
has three capsules but two mono USB streams, not a verified synchronized
array. Face orientation is not eye gaze or proof that the facing person spoke.
The proposed latency targets are explicitly targets, not measurements.

**Verified on the development Mac:** macOS 26.6.2 (25G83), Intel Core
i7-10700K at 3.80 GHz, 16 logical CPUs, 64 GiB RAM; Rust/Cargo 1.96.0,
`x86_64-apple-darwin`. From `lampOS/`, using
`/Users/autonomous/.cargo/bin/cargo`:

- `cargo fmt --all --check` — pass after the document was written.
- `cargo build --workspace` — pass; the same updated base also built before
  this documentation change.
- No crate was touched, so there is no affected-crate clippy/test command
  under this task's gate. Task 13's tests remain reported above. No Go or
  web source changed.
- An inline Python document check verified ten relative links/anchors,
  balanced code fences, no trailing whitespace and the final limitations
  section. The proposed shell snippet was extracted to
  `/private/tmp/lamp-fourth-addressee-pilot.sh`; `bash -n` passed. The snippet
  was **not executed**, locally or on a lamp.
- Source review checked table/operating-point distinctions and current
  support pages, rather than treating abstract gains or historical product
  announcements as current guarantees. Separate review corrected the
  ResLSTM comparison's aggregation, the smartphone timing's single-utterance
  scope, rejected-candidate attribution and the recorder's missing receipt
  timestamps.

**What was actually measured:** no DDSD model accuracy, runtime or lamp
latency. All classifier/product figures in the study are published, and the
lamp observations are the first builder's existing records. One bounded AMI
HTTP-range audit on this Mac read 612,715 archive response-body bytes for
metadata and four small XML entries, excluding network headers. Three sampled
dialogue-act files contained 142,
217 and 156 addressee attributes. An audio URL returned HTTP 200 to a header
request; no waveform was downloaded or listened to. This is an access/schema
check, not a benchmark or a whole-corpus coverage count. The XML buffers were
not retained; source URLs and sampled filenames are in the study.

**Limits and failed access kept:** some web PDF screenshot requests returned
cache errors; the papers' text/tables were checked separately. Vernissage's
original HTTP pages were reachable, while HTTPS certificate validation failed;
it was not bypassed. The original data still require a request and their
commercial rights remain unresolved. Ego4D requires agreements and credentials,
not obtained. Noncommercial or request-only corpora were not downloaded,
licensed, trained on or presented as ready commercial training data. No key,
account, payment, owner message or lamp access was used.

**What the first builder must do on the lamp:** no installation is needed
for this research task. If conducting a consented staged timing observation,
run the exact finite `lamp-bus record` block in
[the first lamp observation](said-to-whom.md#a-bounded-first-lamp-observation)
with the existing publishers running. It creates a private new directory,
records seven existing metadata topics for ten minutes, and caps each file
at 16 MiB. Spoken text is included; PCM and images are not. Pair it with
independent intended-addressee/utterance observations and state clock alignment.
Expect declared topic records and identifiable missing/stale cues; this
capture alone cannot measure subscriber delivery delay or qualify a new
detector. The matched English/Vietnamese, near-name, overlap, off-camera,
look-away and stop scenes and required counts are specified in the document.

**Most likely to be wrong on the lamp:** binding a delayed transcript/name or
a facing bystander to the wrong speaker/turn; transferring private-English,
wearer-view or staged robot results to direct Vietnamese office speech;
assuming USB direction information or a cloud decision arrives before sound;
and measuring fewer unwanted answers while silently losing calls or stops.
AMI is usable for annotation tooling under its stated licence, but supplies
no robot-directed positives. A real consented lamp-view evaluation remains
necessary before any acting switch is enabled. There is no unanswered owner
question blocking completion of this study.

## 2026-10-14, 01:49 UTC: queue and integration check

- Fetched `origin` and reread the full list on `origin/lamp-voice`, now
  `a3e36ce93`. Tasks 13 and 14 remain Fourth's only assignments; both are
  done in this log. No new task was taken and the list was not edited.
- Rebased `lamp-fourth` cleanly onto that head. Memory is now `d5d20548f`
  and the addressee study `993df3d22`. A content diff confirms the memory
  crate, its manifests/lockfile/docs and the study are unchanged by rebase.
- The new upstream commits contain the first builder's mends to the
  second builder's voice review. In particular, the name/note/second-look
  paths now restrict evidence to the request it belongs to. The study's
  local-evidence section explicitly remains a historical reading of
  `8335dfbce`; it does not claim those earlier paths are still unfixed.
- On the same development Mac, `cargo fmt --all --check` and
  `cargo build --workspace` both passed after rebase. No memory code or
  dependency changed, so the passed memory suite was not repeated.
- Only `lamp-fourth` is lease-pushed. Next fetch/list check is due at
  02:19 UTC (09:19 Asia/Ho_Chi_Minh), then every half hour while no new
  Fourth assignment is available. There is no owner question or blocker.

## 2026-10-14, 02:20 UTC: task 15, records-bound — taken

- The scheduled fetch found tasks 15 and 16 newly assigned to Fourth.
  Task 14 is already complete here even though the shared list still says
  taken. Task 15 is next; task 16 follows after it.
- Rebased cleanly onto `origin/lamp-voice` at `96e5b83a7` and lease-pushed
  only `lamp-fourth`. Task 13 is now `3971c6742`, task 14 `d4564810d`.
  On the same development Mac, `cargo fmt --all --check` and
  `cargo build --workspace` passed on that base before this task.
- Task branch: `fourth/records-bound`. Build new `crates/records` and
  `docs/records.md`, with explicit dry-run/apply behavior, typed retention
  roots, age/size bounds and diagnostics. Memory storage is excluded.
- Establish which sessions are active from authoritative service state,
  including restart races and unavailable state; never infer it from age.
  Protect against links, replaced paths, disappearing entries and full disk.
  Test on disposable trees with injected clock/sizes and failure conditions.
- This is background housekeeping, outside the reply path. Measure scanning,
  planning and removal costs separately on this Mac and state bounded work
  and the exact lamp dry-run command; no local result will be called a lamp
  result and no real lamp records will be removed here.
- The task asks for a service/timer for deployment while the general loop
  forbids edits under `deploy/`. Deliver their templates with the new crate
  and exact installation instructions for the first builder, leaving
  `deploy/` untouched. All acting behavior must remain opt-in.

## Not shown

No lamp access or deployment, Linux ARM64 execution, physical power-loss
test, real extraction model, speaker recognition, natural multi-day spoken
test, provider-context deletion, or end-to-end lamp latency. All new
behavior remains behind `--memory-days` until the first builder qualifies
it on the lamp. The log contains no unanswered question requiring the
owner to relay a message.
