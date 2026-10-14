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

## 2026-10-14: task 15, records-bound — done on Fourth's branch

**Built:** new `lamp-records`, [records.md](records.md), a disabled example
configuration, and three unit templates inside `crates/records/systemd/`.
The task branch is `fourth/records-bound`, from `96e5b83a7` plus Fourth's
completed work. Workspace registration adds only this crate; existing
dependency versions are unchanged. Nothing under `deploy/`, a hands-off
crate, or the shared task list changed.

The timer job retains recognized closed voice-service metadata by allocated
bytes and newest mtime, oldest first, proposing 500,000,000 bytes and 14 days.
It inventories first, then obtains bounded systemd plus host-procfs evidence
for current, surviving and manual producers. Unknown evidence refuses the
whole root. Active sessions survive every age/size limit. The safety contract
requires today's trusted voice producer to exclusively create new output
directories; it cannot acquire an existing candidate after the scan.

Descriptor-relative validation rejects links, hard links, mount crossings,
changed identities, unexpected layouts and wrong ownership/permissions.
Same-root private quarantine permits interrupted removal to be resumed.
Partial progress is accounted for without claiming a whole session removed.
Dry runs write only stdout. Removal needs both `--apply` and `enabled:true`;
disabled apply does not open retention roots. No transcript is read, and
memory files are outside the recognized adapter. A separate reporter
publishes bounded content-free diagnostics every second, binding the receipt
to current settings by SHA-256 and exposing missing, failed and stale results.

**Measured and verified on the development Mac:** macOS 26.6.2 (25G83),
x86_64 Intel Core i7-10700K 3.80 GHz, 16 logical CPUs, 64 GiB RAM;
Rust/Cargo 1.96.0. Commands ran from `lampOS/` with
`/Users/autonomous/.cargo/bin/cargo`:

| Command | Final result |
|---|---|
| `cargo fmt --all --check` | Pass |
| `cargo clippy -p lamp-records --all-targets --offline -- -D warnings` | Pass, no warnings |
| `cargo test -p lamp-records --offline` | 96 passed: 87 library, five CLI, four integration; one timing test normally ignored |
| `cargo build --workspace --offline` | Pass; workspace also built before the task |
| `cargo tree -i lamp-records --offline` | No dependent workspace crate |
| `cargo +stable clippy -p lamp-records --all-targets --target aarch64-unknown-linux-gnu --offline -- -D warnings` | Pass; cross-target source checking, not ARM execution |
| `cargo test -p lamp-records --release --test records records_cost -- --ignored --nocapture` | One invocation passed; finite fixture timing below |

For 1,000 disposable sessions, each with five 1,024-byte logical files, first
inventory was 153.317235 ms. Thirty more inventories had median/p95/worst
148.485177/158.1607/170.073146 ms. Thirty pure plans had
0.172239/0.192564/0.286491 ms. Thirty single-session removals, including syncs,
had 2.045649/2.15702/2.188294 ms and unlinked 614,400 allocated bytes. Cache
was not evicted; fixture creation/store opening were excluded. The live set
was injected, so none of these numbers times the real systemd/procfs guard,
the lamp, its filesystem or concurrent conversation. The stated subsecond
development inventory target passed. No before/after lamp claim is made.

**Standalone Linux checks:** copied 704 source files to
`/private/tmp/lamp-fourth-records-final-nq595cud`, without the parent repository,
`.git`, credentials or build outputs. The local Podman VM runs Linux
6.12.13-200.fc41.x86_64, six configured CPUs and 8 GiB RAM; despite its name
`lampos-arm64`, this execution was x86_64. The final suite passed 97 tests
(88 library, five CLI, four integration), with two deliberate normal ignores.
The cached image supplies Rust/Cargo 1.96.0. Exact invocation:

```sh
/usr/local/bin/podman run --rm --network none \
  -v /private/tmp/lamp-fourth-records-final-nq595cud:/source:ro \
  -v /Users/autonomous/.cargo/registry:/root/.cargo/registry:ro \
  -v /Users/autonomous/.cargo/git:/root/.cargo/git:ro \
  -w /source localhost/lampos-arm64-cross:latest \
  /bin/sh -c 'CARGO_TARGET_DIR=/tmp/records-target cargo test -p lamp-records --offline --locked --target x86_64-unknown-linux-gnu'
```

The same container command with `--cap-add SYS_ADMIN` and the test selector
`--test records bind_mounts_are_refused_even_on_the_same_filesystem --
--ignored --nocapture` passed one additional test. It mounted only disposable
fixture objects within the container's isolated namespace; same-filesystem
file and directory bind mounts were refused and the outside sentinel stayed
unchanged. Source/cache mounts remained read-only. The ignored release-cost
test ran on the Mac as reported above, not in this VM.

The three templates passed systemd 252.39-1~deb12u2 syntax verification in
disposable `debian:bookworm` (image SHA-256 starts `94926f0fd968`). Exact
substantive command, after mounting the template directory read-only at
`/templates` and installing `systemd` only in that disposable container:

```sh
systemd-analyze verify /templates/lamp-records.service /templates/lamp-records-report.service /templates/lamp-records.timer
```

Initially it exited 1 with the two expected missing-executable errors. After
container-only `mkdir -p /opt/lampOS/bin` and
`ln -s /bin/true /opt/lampOS/bin/lamp-records`, the unchanged verify command
emitted nothing and exited 0. The `--rm` container was removed. This checks
unit parsing/directives, not startup, the real executable, or systemd on the
lamp. No device or host service was touched.

**Failures kept:** an early portable special-file fixture exceeded macOS's
Unix-socket path limit and sandbox binding permissions; it was corrected to
test portable unknown entries plus a real Linux FIFO. A transitional missing
report-fingerprint initializer and one clippy collapsible-if diagnostic were
fixed before the final gate. An initial offline container build lacked the
workspace's cached git dependency; mounting public Cargo caches read-only
resolved it. ARM checking first failed E0463 because the named `1.96.0`
toolchain lacked the target standard library; `+stable` has that target and
the same compiler commit, and passed without downloading a toolchain.
Review also fixed unrelated `serve` processes being mistaken for voice,
reserved report-temp names colliding with configuration, and invalid trash
entries escaping the shared enumeration bound. All have regressions. No
failing test remains in this task's handback.

**One first command on the lamp:** after First installs the qualified ARM64
binary and the disabled example configuration:

```sh
sudo /opt/lampOS/bin/lamp-records check /etc/lampOS/records.json
```

Expect `mode:"dry_run"`, `enabled:false`, zero removals, the running invocation
protected in `active`, oldest known closed IDs in `planned` when over either
bound, and plausible nonzero disk capacity. A guard error must leave the plan
empty; historical unknown layouts must remain refused. No present disk
percentage/count is asserted from this machine. First inspects that evidence
before installing/enabling the timer/reporter and changing `enabled` to true.
The document describes those steps and the reporter's boot gap: `/run` loses
the receipt, so an enabled reporter may show an error until the next timer
run. Qualification should also compare voice latency/drop counters during
cleanup and verify recovery on a disposable fixture.

**Most likely to be wrong on the lamp:** the loaded `ExecStart` serialization,
host procfs visibility and identification of surviving/manual writers; then
the layout/permissions of old sessions. Strict refusal is intentional; do not
weaken it to obtain a removal count. A producer that reuses old directories
or a malicious root namespace writer is outside the current contract.

**Coverage left explicit for First:** the current shared journal stays under
journald's native ownership. Proposed dedicated-namespace limits are documented,
not installed. Bus capture directories and policy logs lack a supported
completion/ownership adapter and are not globally bounded by this crate.
First must assign their stable roots and closure contract before such adapters
can be safe. This does not block the voice-root handback or task 16. No account,
key, paid service, owner relay, lamp access or deployment was used.

## 2026-10-14: task 16, lean-check — taken

- Task 15 was fast-forwarded and pushed, then the latest list was fetched.
  First has integrated tasks 13 and 14 and added task 17 after this one.
  Rebased `lamp-fourth` onto `6a1d2745c`; task 15 is now `eb57fa71f`.
  The memory-document add/add conflict was resolved by keeping First's
  newer document and lamp measurements verbatim. The memory code was already
  integrated; the remaining memory/addressee commits retain this log only.
- Lease-pushed only `lamp-fourth`. The records crate and its bus/message
  dependencies are unchanged by rebase. The base at `702cab8ed` passed
  formatting and workspace build; the two subsequent end-of-turn commits
  are being checked before implementation.
- Task branch: `fourth/lean-check`. Scope is the new `crates/lean` library,
  one clearly marked stand-in geometry file, `docs/lean.md`, reuse notes
  and workspace registration. Body, Expression, deployment and the shared
  list remain untouched. No call is wired to real movement.
- Read V1's actual `recording_stability.py` and tests, the fall verdict,
  current joint units/calibration and Expression's offset semantics before
  choosing the model. Use explicit SI units and validate every pose. Measure
  the centre of mass against the footprint with an explicit inward margin.
- Supply pose, complete planned-step and composed clip checks, with worked
  arithmetic and rejection tests. Document exactly how First measures masses,
  centres, axes, lengths and the contact footprint, and where Body calls the
  library later. Static balance cannot establish dynamic tipping safety.
- This future gate belongs before output; target under 1 ms per pose and
  under 10 ms for 1,000 checked poses on this development Mac, measured in
  release mode with first/repeated boundaries stated. These are targets,
  not lamp results. No hardware access or owner answer is needed to start.

## 2026-10-14: task 16, lean-check — done

Implementation `98dccf7fa` on `fourth/lean-check`, fast-forwarded to
`lamp-fourth`. New `lamp-lean` library, one synthetic geometry JSON, a finite
read-only catalogue example, tests, [Lean documentation](lean.md), and reuse
notes. Body, Expression, messages, deployment and the running list are untouched.
The base at `6a1d2745c` passed formatting and workspace build before the task
(52.95 seconds), completing the check mentioned in the taken entry.

**Built:** five named joints in explicit physical serial order, SI units,
encoder-angle conversion, measured zero/axis conventions, each rigid part's
own COM including the last wrist, and the fixed base's COM. Validated convex
support geometry supplies true signed distance to an inward-margin polygon,
including outside corners and edges that disappear during inset. All input
values, sizes and work are bounded; malformed geometry never passes through.
The wholly invented five-rod data are labelled `stand_in` and cannot establish
anything about the lamp's real mass or dimensions.

One call checks a pose; one checks every prescribed movement step and its
declared joint-linear interpolation; one checks a complete isolated Expression
smoothstep cycle on a fixed absolute posture, with explicit weight, limits
and applied-joint mask. Conservative COM-displacement bounds cover between
evaluations. Outside witnesses and unresolved bounds are distinct refusals.
At most 10,000 prescribed samples, 100,000 total evaluations and 20 subdivision
levels are allowed. Clip admission, exit, interruption, overlapping behaviours,
moving base postures and asynchronous servo motion require separate owner
composition/qualification; those are not silently inferred from samples.

The example reads only supplied bounded JSON and counts, reports both all-five
and today's head/yaw mask, and always emits `actuation_qualified:false`.
Nothing is wired to the lamp or changes its behavior. Future integration needs
an explicit off-by-default switch owned by First. The document specifies the
final Body write sites and covers ruler/scale measurements of masses, local
COMs, origins, axes, zeros, supported contacts and uncertainty.

**Verified on the development Mac:** macOS 26.6.2 (25G83), x86_64 Intel
Core i7-10700K 3.80 GHz, 16 logical CPUs, 64 GiB RAM; Rust/Cargo 1.96.0,
compiler `ac68faa20c58cbccd01ee7208bf3b6e93a7d7f96`. From `lampOS/`, with
`/Users/autonomous/.cargo/bin/cargo`:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | Pass |
| `cargo clippy -p lamp-lean --all-targets --offline --locked -- -D warnings` | Pass, no warnings |
| `cargo test -p lamp-lean --all-targets --offline --locked` | 63 passed; zero failed; two finite timing tests normally ignored |
| `cargo build --workspace --offline --locked` | Pass; workspace also built before the task |
| `cargo tree -i lamp-lean --offline --locked` | No dependent workspace crate |
| `cargo +stable clippy -p lamp-lean --all-targets --target aarch64-unknown-linux-gnu --offline --locked -- -D warnings` | Pass, cross-target source checking only |

The tests comprise 10 footprint, 30 pose, 15 movement/clip, three catalogue
and five example tests. They cover hand-worked standing/folded/level-arm
answers; farther forward never improving clearance; all joint/rotation/order
conventions; malformed data; safe endpoints with an unsafe interior; the
actual clamped curve differing from clipped-endpoint interpolation; true
quarter-time smoothstep; every prescribed step despite a refusal; and separate
depth/evaluation-cap exhaustion. The current five behaviours have 16 declared
state/mask combinations, compared against independent dense composition.

**Standalone Linux:** 727 source files copied outside the parent tree to
`/private/tmp/lamp-fourth-lean-final-jwpt8hc9`; no credentials, `.git` or ignored
build outputs. The existing local VM is Linux 6.12.13-200.fc41.x86_64, six
configured CPUs, 8 GiB RAM. Its name `lampos-arm64` does not imply ARM execution.
The cached Rust/Cargo 1.96.0 image passed the same 63 tests, with the two timing
tests deliberately ignored there. Exact command:

```sh
/usr/local/bin/podman run --rm --network none \
  -v /private/tmp/lamp-fourth-lean-final-jwpt8hc9:/source:ro \
  -v /Users/autonomous/.cargo/registry:/root/.cargo/registry:ro \
  -v /Users/autonomous/.cargo/git:/root/.cargo/git:ro \
  -w /source localhost/lampos-arm64-cross:latest \
  /bin/sh -c 'CARGO_TARGET_DIR=/tmp/lean-target cargo test -p lamp-lean --all-targets --offline --locked --target x86_64-unknown-linux-gnu'
```

**Measured only on that Mac, release mode:** one invocation of
`cargo test -p lamp-lean --release --test cost --offline --locked -- --ignored
--nocapture` while `arithmetic_cost` was the sole ignored test. Then one invocation
of `cargo test -p lamp-lean --release --test cost inconclusive_cost --offline
--locked -- --ignored --nocapture` after adding the work-cap timing test. Each
workload measured its first call and 30 subsequent calls, with resident inputs,
`Instant` boundaries and no cache eviction:

| Boundary | First / median / p95 / worst (ms) |
|---|---|
| Parse geometry and construct model | 0.061448 / 0.004308 / 0.006650 / 0.008095 |
| Check one pose, model already constructed | 0.000667 / 0.000209 / 0.000281 / 0.000305 |
| Check ordinary 1,000-step path, including allocation | 0.277907 / 0.203836 / 0.240004 / 0.256831 |
| Check a 1.2-second repeating clip, 61 evaluations | 0.024176 / 0.015126 / 0.015775 / 0.017979 |
| Exhaust all 100,000 evaluations on a near-boundary path | 19.921472 / 19.950070 / 21.012518 / 21.340156 |

The ordinary path is 1,000 sinusoidal small-angle poses at 20 ms intervals.
The clip uses all-five application over a fixed synthetic posture, also at
20 ms. Neither needs adaptive extra evaluations. The work-cap case always
returns `Unresolved` and its timing includes two result assertions. File I/O,
process startup, Body composition, transport and physical motion are excluded.
These are first in-process calls, not CPU/disk-cold runs. The stated 1 ms pose
and 10 ms ordinary-path targets passed; near-boundary work is explicitly
slower and belongs outside immediate feedback. No before/after or lamp timing
claim is made.

The native release example exited 0 with 16 positive stand-in results for
`2048,2048,2048,2048,2048`. The level-arm fixture
`2048,3072,2048,2048,2048` exited 1 at −0.07 m baseline clearance; an invalid
4096 count exited 2 with no JSON result. All are arithmetic inputs, never
commands to a motor.

**Failures kept:** no crate test or clippy stage failed. An initial worker
format invocation lacked the Cargo-bin `PATH`; the explicit binary passed.
Sandboxed `sysctl` initially refused the hardware read; an authorized read-only
retry supplied the machine identity. Review added the clamp/time/bound-limit
regressions above before the final gate. ARM checking used the already-installed
`+stable` target with the identical 1.96.0 compiler; it did not execute ARM code.

**One first command on the lamp:** First cross-builds `--example check`, names
that executable `lean-check`, and places it beside `geometry.stand-in.json`
and the exact Expression `behaviours.json`. From that directory:

```sh
./lean-check geometry.stand-in.json behaviours.json 2048,2048,2048,2048,2048
```

Expect stand-in provenance, COM `[0,0,0.25]` within rounding tolerance,
clearance +0.09 m, 16 `within_margin` results,
`model_within_margin_all_checks:true`, `actuation_qualified:false`, and exit 0.
Then substitute fully measured geometry and all five freshly observed counts,
compare predictions with independently supported scale readings, and qualify
the actual final-command paths before adding a disabled-by-default Body gate.
The all-2048 tuple is **not** a prescribed physical pose.

**Most likely wrong on the lamp:** the masses and which rigid assembly owns
each motor/cable/attachment, then local COM vectors, zero/axis signs and the
loaded contact footprint. Correct arithmetic with wrong physical data can
approve the wrong static model. This library does not establish dynamic tipping,
collision, friction, compliance, servo tracking, stopping or recovery safety.
No lamp access, account, key, paid service or deployment was used. No owner
answer is required; task 17 is next after fetching the list.

## 2026-10-14: task 21, controls-voice — taken

- Task 16 and its handoff were pushed (`075179e26`, implementation
  `98dccf7fa`). The next fetch found task 21 inserted before task 17; take
  the new first eligible task, not the previous next-task assumption.
- Rebased `lamp-fourth` onto fetched `52cc4ec8e4a6e3923eceddb96df36a79cbceb849`.
  First has integrated task 15 and documented its real-lamp dry run; those
  are First's measurements. The records implementation dropped out, retaining
  this log. Cargo/reuse-note conflicts were resolved by preserving First's
  Controls registration/review and appending only Lean. Lean is now
  `bbbd82fc9`, handoff `e7ae4f7f0`. The remaining diff against that pinned base
  is only Lean, workspace registration/reuse notes and this log.
- The refreshed Lean gate passed again: formatting, strict all-target clippy,
  63 tests, then workspace build (43.87 seconds). Lease-pushed only
  `lamp-fourth`. No protected branch or running-list edit.
- Task branch: `fourth/controls-voice`. Scope: the `controls/state` receiver
  at Voice's coordinator, its startup/privacy/cancellation support, extensions
  to `crates/controls` as needed, and `docs/controls.md`. Each new action has
  its own off-by-default switch. Body/deployment and Second's endpoint work
  remain outside this task.
- Read the current Controls/Receiver/MuteRecord contract and Voice's existing
  physical-privacy and cancellation paths before implementing. Revoke and
  dispatch local speaker cancellation before any persistence/network work;
  restore software inhibition before opening any workers. Use an explicit local
  clear, with the physical slide always authoritative, and retain inhibition
  through failures/restarts. Report pets without using them as Stop.
- Target observation-to-speaker-cancel dispatch within one 2 ms coordinator
  polling interval for an already delivered valid request, measured with an
  injected clock. Account separately for source acquisition, bus delay,
  persistence and acoustic silence. Test first/repeated requests, stale and
  duplicate events, thinking/listening/speaking/idle, control-process loss and
  recovery, Voice restart while muted, slide cycles and a simultaneous link
  drop. No device latency is inferred from injected clocks.
- The handoff will list five exact physical actions and expected behavior.
  The highest lamp risk remains pad polarity, read only at rest by First;
  software tests cannot establish touch polarity or physical silence.
  No question, account, key, paid service or lamp access is needed to start.

## 2026-10-14: task 21, controls-voice — done on Fourth's branch

**Built:** implementation `ddb2aac304545c6fd392fdabd57ca0131187541f`,
fast-forwarded from `fourth/controls-voice`, from claim `0aed5103e` and fetched base
`52cc4ec8e`. Voice consumes typed `controls/state` through the Controls
Receiver. `--controls-stop` and `--controls-mute ABS_PRIVATE_RECORD` are
independent, off by default. `--controls-bus-root ABS_PATH` only selects the
bus; it enables neither action. A producer still needs its own `--enable`.
Neither action requires a provider round trip. No deployment or message type
was changed. Voice now depends on Controls; Voice-eval is its only workspace
dependent, and both were included in the gate.

Stop revokes the owned turn and sends the local speaker its new authority
before provider traffic, diagnostics, persistence or presentation. Pending PCM
and input commands are discarded, and an abandoned utterance drains through
its endpoint instead of becoming another question. Old output cannot revive
the answer or schedule a look. Tests cover speaking, thinking, listening,
idle, repeated/stale Stop, pending and accepted speech under playback
suppression, classifier cancellation and silence fallback, and a real
coordinator tick with Stop and a provider fault queued together.

Software mute revokes microphone/camera/output authority. Its record is
restored before any worker, including metadata and GPIO, and survives service
and machine restart. A fresh second released hold can clear only after a new
press beyond the current start/publisher/fault/gap fence. The physical slide
still requires its own fresh stable opening; opening it cannot clear software
mute. A new runtime follows only verified old-worker destruction. Head pats
are metadata only, including while software-muted. One bounded bus pump
survives service cycles; missing, stale, malformed, overflowing or restarted
Controls never clears the latch or terminates healthy unmuted Voice.

Persistence deliberately favors silence: before audio opens, a durable muted
recovery record is armed without changing the live preference. A crash,
provider fault or failed cleanup therefore requires a fresh local hold after
restart, even if no hand mute had been requested. Clean verified shutdown can
disarm it. Saves keep interrupted-write evidence; either `.pending` or
`.replacement` refuses later startup and requires explicit operator recovery.
The active guard already stores muted before a hand request; its newer request
watermark is saved in the next quiescent poll, or on terminal service exit.
File/directory sync is outside the urgent audio path, with no hard userspace
bound. Tests inject failure before and after replacement. They are not a
physical power-loss qualification.

**Gate on Fourth's Mac:** macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K
3.80 GHz, 16 logical CPUs, 64 GiB; Rust/Cargo 1.96.0. Commands ran from
`lampOS/`, using `/Users/autonomous/.cargo/bin/cargo`:

```sh
cargo fmt --all --check
cargo clippy -p lamp-controls -p lamp-voice -p lamp-voice-eval --all-targets --offline -- -D warnings
RUST_TEST_THREADS=1 cargo test -p lamp-controls -p lamp-voice -p lamp-voice-eval --offline
cargo build --workspace --offline
```

All four stages passed. Final totals: Controls **142 passed**, Voice **973
passed / six ignored**, Voice-eval **284 passed**: **1,399 passed**, no failed
tests. The six ignored entries are opt-in probes; the newly added latency probe
was run explicitly below. Strict Clippy finished in 11.65 seconds and the final
workspace build in 17.41 seconds as reported by Cargo. These builds reused
Cargo caches and incremental outputs; no clean-build performance is claimed.
The workspace also
built earlier in this task (24.99 seconds). No source or manifest changed after
this final test compilation; subsequent edits are documentation/evidence only.

**Standalone Linux copy:** 764 tracked/nonignored source files, without the
parent repository, Git metadata, credentials or a target tree, copied to
`/private/tmp/lamp-fourth-controls-final-s2r9nzee`. Podman VM `lampos-arm64`
actually runs **x86_64**, Linux 6.12.13-200.fc41.x86_64, six vCPUs, 8 GiB,
on the same Mac. The name is not evidence of ARM execution. The task-only
image `localhost/lampos-fourth-native-check:latest` adds native ALSA/udev
development packages and `procps` (`/bin/kill`) to the existing cross image.
Tests ran with network disabled, read-only source and Cargo caches, and the
task-owned target volume. Inside `/source`:

```sh
CARGO_TARGET_DIR=/target CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=1 cargo test -p lamp-controls -p lamp-voice -p lamp-voice-eval --offline --locked --target x86_64-unknown-linux-gnu
CARGO_TARGET_DIR=/target CARGO_BUILD_JOBS=4 cargo build --workspace --offline --locked --target x86_64-unknown-linux-gnu
```

Final complete rerun, with no other build/test from this session: Controls
**142 passed**, Voice **977 passed / six ignored**, Voice-eval **284 passed**:
**1,403 passed**, no failed tests. Workspace build passed in 19.18 seconds
as reported by Cargo. It still warns about two inherited Body items: unused
`inspection::run` and unnecessary `mut` on the `eyes` closure in the Body
binary. Neither was edited. The source-only build uses its own cached target
volume; it is a portability check, not a cold-build benchmark.

[Machine-readable evidence](verification/controls-voice-host-20261014/results.json)
keeps commands, totals, code hashes, timing workload, earlier failed test names
and the additional checks below.

An additional ARM64 strict all-target Clippy attempt failed on the inherited,
hands-off `lamp-body/src/inspection.rs:615` unused `run` function. It is
unchanged at the pinned base. The same command with `--no-deps` passed for
Controls, Voice and Voice-eval in 20.02 seconds, retaining that dependency
warning. This is cross-target source checking, **not** a clean strict
dependency gate, an ARM link or ARM execution. The required native strict
Clippy gate above passed without exclusions.

**Failures retained:** intermediate native Clippy caught an unnecessary unwrap,
which was replaced by an `if let` before the final gate. A check during active
implementation saw the test module before that file had been written; the
final gate includes it. An initial Cargo command used the wrong working
directory and was rerun from `lampOS`. These are not counted as passes.
The first parallel Mac library run had 751 passes,
24 failures and five ignored tests. One was our real regression: treating all
admission drains like hand-abandoned input changed an existing privacy-loss
case. The bypass now belongs only to explicit hand Stop, with hard capture
authority still checked. The other failures reported local socket ENOBUFS or
synthetic worker startup timeouts; the complete serial gate passes them without
changing their assertions. An initial Linux attempt lacked native ALSA build
metadata and another lacked `/bin/kill`; the isolated image was corrected.
Linux also exposed an inherited test-only `capture_on` call missing the newer
reference argument; supplying `Reference::AsGiven` preserves its old default.
ARM checking exposed an unnecessary `mut` in Controls' existing Linux probe;
that one-token warning fix is included.

The first Linux serial library run had 778 passes, three failures and six
ignored tests. The new bus-restart test found a real connection bug:
`voice-hand` is not an allowed subscriber name. It is now `voice_hand`, with
a portable regression that opens both actual typed subscriptions on macOS as
well. The two older failures were the dense ring trace's 100 ms real transport
envelope and the rapid-mute fixture's 50 ms privacy freshness check while a
separate container build ran. Scheduling pressure is consistent with those
errors; exact ages were not recorded. Without that build, all three cases
passed three consecutive exact runs (nine passes), with production limits and
test timeouts unchanged. The next full Linux run passed 783 library tests but
failed the older single-question privacy fixture with a recorded 57,167 µs
peer observation gap, beyond its unchanged 50 ms limit. Ten exact runs of
that case on the untouched pinned base passed; that narrow comparison does
not reproduce the full-suite failure or prove its cause. Final review also
caught pats discarded during the
quiet wait; their accepted metadata now reaches the same reporting helper,
with a duplicate-pat regression.

**Latency:** the injected-clock cases assert actual speaker send before the
receipt sample at authored intervals 0, 700, 1,500 and 2,000 µs. Separately,
one release-mode invocation on this Mac measured 100 fresh cancellation
fixtures: first 41 µs; next 99 median 6 µs, nearest-rank p95 9 µs, worst
16 µs; worst overall 41 µs. Timing starts immediately before `hand::cancel`
and ends at `lamp_ipc::monotonic_us` sampled after the successful speaker
Authority datagram send. Setup/allocation/verification/teardown are excluded. Each fixture holds 480
queued PCM samples, one pending speaker receipt and two provider input commands;
this is not a worst-case backlog measurement. Other local tests were running; this is no cold-boot or cold-cache claim.
It excludes button recognition, bus delivery, speaker scheduling, playback
and acoustic silence. Reproduce with:

```sh
cargo test -p lamp-voice --release --lib coordinator::tests::hand_controls::local_hand_stop_dispatch_latency --offline -- --exact --ignored --nocapture --test-threads=1
```

**First on the lamp:** cross-build Voice and Controls from this task. First
shadow-records the primary button and each head pad with its current pin files:

```sh
lamp-controls serve --config deploy/lamp-4ace/controls.json --pins deploy/lamp-4ace/controls-pins.json --record NEW_PRIVATE_RECORD
```

There is no `--enable` on that first command. After the rebase onto `1750c3d08`,
First's installed `lamp-controls` service is already the GPIO owner. Stop it
before a foreground recording, or add recording to that same service invocation;
never run two readers. Use its current installed config and pin paths beside
the build in place of the source-tree paths above, then restore one service
owner. Correct polarity from an actual touch before enabling actions.
For the installed root-owned Voice service,
prepare `/var/lib/lampOS/voice/hand` as root-owned mode 0700 and append
`--controls-stop --controls-mute /var/lib/lampOS/voice/hand/mute.json` to its
existing arguments, preserving the same record across restarts. Add
`--controls-bus-root` only if using a nondefault bus root, and run Controls
with `--enable` after the shadow check. Do not run a second Voice alongside
the installed owner. These are instructions for First, not actions performed
by Fourth.

| Five physical sequences | Expected result |
|---|---|
| Tap/release the primary button during an audible answer; repeat rapidly. | The old answer is cut off locally. Duplicate delivery cannot restore it; a tap cannot unmute or start an answer. |
| Tap while waiting for an answer, then while listening/idle. | Pending/late output is abandoned. Idle Stop makes no spoken acknowledgement or new turn. |
| Hold at least two seconds, release, then restart Voice after the mute save succeeds. | Action is on release. Capture remains closed across restart using the same file/flag. |
| While software-muted, close/open the physical slide; make a fresh second hold, and separately clear with the slide closed. | Slide opening alone never clears mute. A software clear through a closed slide cannot open capture; new audio still needs normal fresh physical permission/readiness. |
| Pat each head pad, including while muted. | `hand_pat_observed`, `action: none`; no Stop, speech, mic opening or clear. |

Read `hand_control_cancel` in the invocation's `metadata/events-*.jsonl` for
the coordinator-observation and speaker-send
timestamps; measure physical release-to-silence separately. Repeat under a
provider/network stall and with Controls killed/restarted. Confirm neither an
old answer nor held input resumes. A successful local send is not an acoustic
acknowledgement.

**Most likely wrong on the lamp:** head-pad polarity. First observed resting
levels only: button 99 high, pads 37/96 low, with active-high touch inferred.
Fourth has no physical touch evidence. Next are actual bus/scheduling delay,
speaker buffer retirement, and private-storage sync/marker recovery on the
lamp's filesystem. The change establishes neither Body's independent stop nor
physical acoustic latency. No lamp access, deployment, account, key or paid
service was used. No owner answer is required; fetch the list before taking
the next task.


## 2026-10-14: task 21, rebase and verification

Rebased Fourth onto the fetched `origin/lamp-voice` snapshot
`1750c3d085c048aa56fb49de22ec76848f2671c4`. Git dropped the already integrated
Lean implementation. The only conflict was the append to `reuse-decisions.md`;
both Surround's review and task 21's review remain. Task 21's rebased code is
`61c087911`; the original `ddb2aac30` evidence above remains evidence of that
original build. The handoff now accounts for First's installed Controls GPIO
owner: stop that owner before a foreground recording, use its installed pin
and config files, and restore exactly one owner. No deployment was performed.

On the same development Mac, after the rebase:

```sh
cargo fmt --all --check
cargo clippy -p lamp-controls -p lamp-voice -p lamp-voice-eval --all-targets --offline -- -D warnings
RUST_TEST_THREADS=1 cargo test -p lamp-controls -p lamp-voice -p lamp-voice-eval --offline
cargo build --workspace --offline
```

The final gate passed: Controls 144, Voice 983 and Voice-eval 284 tests,
**1,411 passed, zero failed, six explicitly ignored** as listed in the original
evidence. Clippy finished in 14.47 seconds and the workspace build in 81 seconds;
these are Cargo's elapsed times with the existing cache, not cold-build or lamp
measurements. No Rust source changed during this verification.

Keep the first post-rebase failure: the Voice library run passed 792 tests but
`coordinator::tests::endpoint_session::a_worker_failure_falls_back_through_recovery_and_then_resumes`
failed with `capture discontinuity`. That fixture and its detector were unchanged
from the earlier passing task build, and its controls are disabled. The detector
rejects skipped sequences, regressed timestamps and capture gaps over 50 ms;
the emulated capture peer timestamps actual thread execution. The failing run
retained no adjacent capture timestamps, so its cause is not established.
Five consecutive isolated exact runs passed, followed by the complete passing
gate above. Neither a production limit nor a test assertion was relaxed.

Local logs are `/private/tmp/lamp-fourth-controls-rebase-tests.log` (failure),
`lamp-fourth-controls-rebase-recovery-{1,2,3,4,5}.log` (five passes),
`lamp-fourth-controls-rebase-tests-2.log` (full pass), and
`lamp-fourth-controls-rebase-build.log`, under the same directory. Rebase
verification was on macOS only; the earlier standalone Linux result was not
rerun for this integration snapshot.

The unchanged Policy baseline also passed, before task 17 implementation:
`cargo clippy -p lamp-policy --all-targets --offline -- -D warnings` and
`RUST_TEST_THREADS=1 cargo test -p lamp-policy --offline`: **81 passed, zero
failed or ignored**. Its logs are `lamp-fourth-policy-baseline-clippy.log` and
`lamp-fourth-policy-baseline-tests.log` in the same local directory. No service
was contacted outside the test machine.

## 2026-10-14: task 17, local-judge — taken

- Taken from the running list read for this cycle at `1750c3d08`; task 21 is
  complete on Fourth. The later list visible at `23843b78e` leaves task 17
  unchanged and open. Fourth's verified base is `7a4eded41`.
- Task branch: `fourth/local-judge`. Scope: new files in `crates/policy`,
  `policy.rs` only for asking and combining the second judge, the part's
  English docs, and the new model's data/provenance. No changes to existing
  `main.rs`, `lib.rs`, Cargo manifests, message types, Voice, Body or deployment.
- Implement the same seven typed questions from explicit nullable local cues,
  with one weights file. Preserve the existing Jev state and calibration.
  Keep unknown orientation distinct from observed-away; current transcript
  receipt times cannot supply acoustic speech duration.
- Add an explicitly enabled, shadow-only entry point. Take the local answer
  immediately and publish one final choice under a short deadline. Keep a
  single slow request outstanding, log late answers against their original
  decision, and keep later local decisions independent of a slow service.
  With no timely slow answer and no call evidence, choose `look_up` rather
  than answer. Compare against the busy-room rule using declared snapshots.
- Fit question heads, local combination and evaluation on separate session
  partitions. Synthetic fixtures test the machinery; seed weights will be
  marked unfitted. No paid service, account, key or lamp access is needed.
- Latency target: local `Decider::ask`, including input parsing, p95 at most
  20 ms and p99 at most 50 ms; measure first use separately. Target a final
  internal shadow choice within a 100 ms decision budget, excluding existing
  transcript gathering, ASR, bus scheduling and acoustic response. Keep
  network and disk work off the new decision arithmetic path.
- The existing Policy baseline passed all 81 tests and strict Clippy on the
  development Mac, as recorded above. Finish with the full required gate and
  a standalone source-copy check, keeping failures and exact machine details.

**For First, in this log:** the labelled played-scene records are not present
in this checkout. Please supply the independent directed/not-directed label,
nullable labels for the seven questions, session/group identity and rights to
use the records. To reproduce Voice's actual busy-room gate, also keep the
text prefix, 45-second conversation state, own-turn flag and facing result
available at its first output; Policy's completed text and 20-second context
are different inputs. The precise JSONL schema will accompany the fitter.
Implementation and synthetic checks continue without these records. Most likely
wrong on the lamp: the initial weights and receipt-time association of cues.

## 2026-10-14: task 17, local-judge — done on Fourth

Implementation: `96b0c3c7c80945d80d230a10d2fb44f362e778e5`, built on
`fourth/local-judge` and fast-forwarded onto `lamp-fourth`. This is a
completed shadow implementation and host handoff, not a qualified lamp
classifier. The independently labelled played-scene records requested in
the claim above were not available; the shipped seed is explicitly unfitted.

**Built:** a second `Decider` answering the same seven typed questions from
nullable cues, one authored weights file, a separate local probability
head, and an offline fitter with session-disjoint train/calibration/test
partitions. Unknown cues contribute zero evidence; unsupported semantic
heads remain priors. The model is 3,078 bytes, ours under Apache-2.0, with
reproduction and SHA-256 in [its source record](../models/policy/SOURCE.md).
The [Policy document](policy.md#the-local-second-judge-task-17) specifies
every JSONL fitting field, bound and failure case.

The new `lamp-policy-judges` binary requires `--local-judge` and always
publishes `shadow: true`. The existing executable remains unchanged. It
takes the local answer immediately, accepts a slow answer only when both
completed and observed strictly before the fixed eligibility deadline,
and publishes one choice. One outstanding slow request cannot hold later
local decisions; a late reply is logged against its original run/number
and cannot revise a choice. With no timely slow answer and no call evidence,
the choice is `look_up`, including in a quiet room. A failed judge leaves
the valid judge's probability standing. New judge/outcome logging uses one
bounded worker; queue admission, completed append, drops and abandonment
are reported separately. Append is not a durability guarantee, and bounded
queue memory does not bound the daily files' disk use.

No existing main/lib file, manifest, message type, Voice code, Body code,
deployment file or work list changed. New modules and tests are inside
Policy; existing `policy.rs` only connects the optional path. V1 review,
reuse decisions and the exact busy-floor snapshot distinction are documented.

**Gate and machines:** final source passed on the development Mac and a
source-only Linux copy: **142 tests passed, zero failed, two manual timing
tests ignored by the normal gate in each environment**. Both measurement
tests were separately executed in release mode on the Mac. The earlier
81-test Policy suite remains included and unchanged. No workspace crate
depends on Policy. The [verification record](verification/local-judge-host-20261014/README.md)
retains source hashes, machine-readable counts, commands, timing boundaries
and failures; all 32 manifest entries matched both tested source trees.

Mac: macOS 26.6.2 (25G83), x86_64 Intel Core i7-10700K at 3.80 GHz,
16 logical CPUs, 64 GiB RAM; Rust/Cargo 1.96.0, rustc commit
`ac68faa20c58cbccd01ee7208bf3b6e93a7d7f96`, LLVM 22.1.2. From `lampOS/`,
sequentially:

```sh
cargo fmt --all --check
cargo clippy -p lamp-policy --all-targets --offline -- -D warnings
RUST_TEST_THREADS=1 cargo test -p lamp-policy --offline
cargo build --workspace --offline
```

Linux: the same Mac's existing Podman VM, named `lampos-arm64` but actually
**x86_64**, Linux 6.12.13-200.fc41.x86_64, six vCPUs, 8192 MiB RAM, same
Rust version. A read-only standalone copy with no parent sources, Git,
credentials or target artifacts ran with external networking disabled;
container-local HTTP/Unix-socket fixtures remained available. With
`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, `CARGO_TARGET_DIR=/target`:

```sh
cargo clean -p lamp-policy --target x86_64-unknown-linux-gnu
cargo clippy -p lamp-policy --all-targets --offline --locked --target x86_64-unknown-linux-gnu -- -D warnings
cargo test -p lamp-policy --offline --locked --target x86_64-unknown-linux-gnu
cargo build --workspace --offline --locked --target x86_64-unknown-linux-gnu
```

Final Mac Clippy/build elapsed times were 2.54/2.17 seconds; the final
Linux workspace build was 4.22 seconds, with existing caches. These are
Cargo elapsed times, not interaction or lamp measurements. Linux retained
two inherited Body build warnings (`inspection.rs:615`, `lamp-body.rs:592`);
Policy's strict lint was clean. Body remains Fifth's.

**Keep the failures:** early formatting/Clippy mistakes were corrected
without suppression. One Linux gate then failed the unchanged legacy
name-joining test: its fourth utterance had `name_heard: 0.97`, expected
`0.0`. One isolated task-source run and five untouched-baseline runs passed.
A separate diagnostic copy of baseline `574c2e5ed`, with only a 40 ms test
delay between the third `Thinking` state and transcript, reproduced the
same failure with unchanged production code. The source's 80/300 ms gather
wait depends on receipt order; this can leave an earlier name inside the
next utterance's one-second pre-roll. The original failure had no adjacent
timestamps, so the reproduced trigger does not prove its exact schedule.
No assertion, timeout, test inclusion, name window or gather behavior was
changed. The final complete gate passed after investigation. Switching
baseline/task copies at the same `/source` mount also exposed stale Cargo
artifacts; the explicit Policy-only clean above resolved missing-module
compiler errors without a source change. Both failures are retained.

**For First:** the legacy fixture's scheduling sensitivity and possible
cross-utterance name association remain. Establish that a later utterance
is actually outside the pre-roll before asserting no name; inspect source
timing/turn attribution on the lamp. Task 17 does not claim to fix that
existing path. The exact diagnostic patch and output are in
[the investigation](verification/local-judge-host-20261014/name-fixture-investigation.txt).

**Measured latency on this Mac only:** one fresh release test process,
OS cache not cleared, measured local `Decider::ask` (cue parsing and seven
answers included): first 9.072 µs; next 1,000 median 1.434 µs, p95 1.453 µs,
p99 1.466 µs, worst 2.379 µs. Weights loading separately took 124.678 µs.
This excludes cue gathering, combination, logs, bus delivery and sound.
The controlled internal shadow-choice fixture held one slow request and
measured eligibility to final internal choice just before publication.
Before capping its final wait by the remaining budget, the first choice
took 104.080 ms; after, 100.223 ms. The next 100 local-only choices while
that request remained held had median/p95/p99/max 16/19/29/36 µs before
and 16/23/37/47 µs after. Both runs were alone, with no concurrent build.
The after run still exceeds the nominal 100 ms completion target by
0.223 ms; strict slow-answer admission does not promise hard real-time
completion. This is one controlled pair, not lamp statistics. Reproduce:

```sh
cargo test -p lamp-policy --release --test local_timing --offline -- --ignored --nocapture
RUST_TEST_THREADS=1 cargo test -p lamp-policy --release --test judges --offline -- --ignored --nocapture
```

**Comparison:** eight authored snapshots, zero real lamp recordings and
zero captured Voice first-output snapshots. Five speech permissions differ:
against four authored negative and four positive intentions, the busy floor
permits two negatives and misses no calls; the guarded local seed permits
no negatives but misses three calls. This **does not beat the floor** and
is not a fitted accuracy claim. Policy's completed text/20-second context/
near-face receipt view cannot reproduce Voice's first-output prefix/
45-second conversation/any-face view without those actual snapshots.

**First on the lamp:** cross-build the Policy binaries, then stop the
existing Policy owner before this foreground run and restore exactly one
owner afterward. Use the lamp's existing decisions key and an absolute
private log directory; Fourth created neither. From the build directory:

```sh
./target/release/lamp-policy-judges --local-judge --show-judges \
  --key-file /ABSOLUTE/EXISTING/DECISIONS_KEY \
  --log-dir /ABSOLUTE/PRIVATE/JUDGE_LOG \
  --bus-root /run/lampos-bus --deadline-ms 100 --request-ms 3000 --seconds 60
```

Play a quiet short scrap with no call, busy talk containing the name, an
explicit call, a follow-up, and known-away/unknown/facing observations. The
terminal should show seven numeric probabilities from each completed judge,
one shadow choice per utterance, and late replies retaining the older number.
The quiet scrap with no timely slow answer must be `look_up`. Nothing acts
on these choices. The command exits after 60 seconds; check final log write,
drop, abandonment and unobserved-slow counts. Repeat with a slow/offline
service and check that local choices continue. Measure these same boundaries
on the lamp separately; the Mac numbers do not qualify it.

Keep independent directed labels, nullable seven-question labels, lawful
source/rights and session identities; capture the actual Voice first-output
snapshot for a real comparison. Run `lamp-policy-fit INPUT.jsonl
NEW-WEIGHTS.json NEW-REPORT.json` using the documented schema and fresh
output paths. Its held-out report evaluates the fitted local probability,
not fused runtime accuracy. Fit and compare held-out lamp sessions before
considering any action wiring.

**Most likely wrong on the lamp:** the unfitted weights, then name/face
association from receipt times rather than a stable speaker/turn identity.
Speech duration is unknown in live Policy input; ASR receipt span is not
substituted for it. No lamp access, deployment, paid service, account,
real key, ARM64 execution or real-service performance was used here.
Fetch the running list again for the next task.

## 2026-10-14: task 17, rebase and current busy-room floor

Fetched the running list and rebased Fourth onto
`e93b9684929d62248394d61c87ec57120db43229`. No new Fourth task is listed:
13, 14, 15 and 16 are integrated; 21 and 17 are complete in this log.
Git dropped task 21's already-integrated code patch. That does not prove
its physical installation or qualification. The only rebase conflict was
the append in `reuse-decisions.md`; both Expression reviews and the local
judge review remain. Original task 17 code is now `5ee7efc73`; the
`96b0c3c7c` evidence above still identifies the original tested build.

The fetch exposed a real comparison change: First's `e07449e13` added a
sure-name branch in Voice's `first_output_busy`, before `decide_busy`.
An eligible score ≥0.999 can permit output in a busy room without a facing
face. Follow-up **`39a45c173f1dff42d264e177ff794cd6ae568bdf`**, built on
`fourth/local-judge-floor` and fast-forwarded onto Fourth, mirrors that
branch. It requires `named_sure` in each supplied baseline snapshot and
records the audited source in model records/fitting reports. Missing/null
latch evidence is rejected, not converted to false. For an actual Voice
first-output comparison, capture its retained per-request bit; a later weak
hearing does not erase an earlier sure one. Policy's runtime comparison
remains a separately labelled score projection with different time joins.

The opt-in shadow guard now accepts a valid score ≥0.999 as call evidence;
0.993 alone in a busy room still triggers the no-call guard. This does not
force an answer or alter the model probabilities, coefficients or mixture.
All behavior remains behind `--local-judge` and always shadow. The updated
test enumerates the additional latch input and checks exact score boundaries,
invalid/unknown values and required JSON evidence. A read-only source audit
found no discrepancy with First's current gate.

**Verification:** same macOS 26.6.2 x86_64 i7-10700K/64 GiB development Mac
and same x86_64 Linux Podman VM (six vCPUs/8192 MiB), Rust/Cargo 1.96.0,
as the original record. **143 passed, zero failed, two manual measurements
ignored by the normal suite in each environment**; both measurements ran
separately on the Mac. From `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy -p lamp-policy --all-targets --offline -- -D warnings
RUST_TEST_THREADS=1 cargo test -p lamp-policy --offline
cargo build --workspace --offline
```

The independent source-only Linux copy, without parent files/Git/credentials
and with external networking disabled, passed Policy clean, strict lint,
tests and workspace build with `--offline --locked --target
x86_64-unknown-linux-gnu`, `CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`.
The [follow-up evidence](verification/local-judge-floor-20261014/README.md)
lists each exact command and all 32 source hashes, which matched both
trees. Mac Clippy/workspace elapsed times were 5.06/24.08 seconds; Linux
3.63/21.50 seconds, with existing caches. The same inherited Body warnings
remain. No gate/test failed in this follow-up; earlier failures stay in the
original record. No previously integrated Fourth crate differs in source
from the pinned fetched upstream; Policy is the only changed crate and
no crate depends on it.

Fresh release measurements on this Mac, alone with OS cache not cleared:
local `ask` first 8.427 µs; next 1,000 median/p95/p99/max
1.425/1.442/1.454/2.149 µs, with the same parsing/allocation boundary.
Separate weights loading was 102.271 µs. The held-slow shadow fixture's
eligibility-to-internal-choice boundary measured 100.244 ms first, then
15/19/30/34 µs for 100 local-only choices. The first still exceeds the
nominal completion budget by 0.244 ms. Both manual commands and exclusions
are in the evidence; no acoustic or lamp timing is inferred.

**Comparison remains insufficient:** the current nine authored rows add a
0.999 busy-room call without a pose. That row is floor `play`, seed
`look_up`, probability 0.4067034, with no guard override. Six of nine
permissions differ. On the authored intentions, the floor permits two of
four negatives and misses none of five calls; the local seed permits no
negatives but misses four calls. No recorded Voice first-output row or
independent lamp corpus was supplied. The original eight-row evidence is
retained under its earlier rule, not rewritten as a result of this one.

**First on the lamp:** the same 60-second command above still applies,
after stopping the existing Policy owner; never run two. In addition to
the earlier played scenes, compare a sub-0.999 meeting detection and a
≥0.999 call with no facing face. Capture the actual `named_sure` latch
along with Voice's prefix, conversation/own-turn state, room and face view.
Use the updated fitting schema; old snapshots lacking this bit cannot
reproduce the new rule and must not be filled with an invented false value.
Fit independent labelled sessions and evaluate held-out lamp records before
action wiring. Most likely wrong remains the unfitted weights, then the
name/face receipt-time association. No lamp access, spending, account,
real key, deployment or ARM64 execution occurred.

The branch is ready for First to read; monitoring continues on the required
half-hour cycle when the fetched Fourth queue remains empty.

## 2026-10-14, 15:05 ICT: integration synced; no Fourth task left

The task list read at `d96b15f42d798bf833c57b889b007ebd226a8261`
has no untaken Fourth assignment. It now marks task 17 integrated as a
played-scene tool, with fitting still waiting for labelled records. First
also reports memory across days enabled on lamp-4ace at 14:50 after a
backup; Voice does not yet use its additions. These are First's reported
lamp actions, not measurements or actions by Fourth.

The tracking ref advanced during the preceding sync. The actual initial
rebase base was `3ee769e63`, which also includes Keeper and Body/Expression
work. The original task 17 patch dropped as already integrated upstream
(`aab4fe1d5`). A final rebase to the explicit `d96b15f42` adds only the
Expression documentation update. The current sure-name follow-up is
`2af979f7c92319f24773812cd2550fd9802c9f3c` (the same implementation as
`39a45c173` above). No runtime code was edited during these rebases.

Policy, its bus/message/IPC sources and the seed are byte-identical to
the passing follow-up. On the same development Mac, the expanded workspace
passed `cargo fmt --all --check` and `cargo build --workspace --offline`;
the build reported 21.60 seconds with the existing cache and no warnings.
Logs are `/private/tmp/lamp-fourth-empty-integration-{fmt,build}.log`.
The 143-test and Linux-copy evidence above belongs to its stated integration
snapshot; it was not rerun to claim testing of the newly integrated Keeper
or Body. Fourth's only remaining crate difference from this upstream is
the verified Policy comparison follow-up. Protected files, workspace
manifests and deployment files match upstream.

For First's evening scenes, the installed tool can sit beside the service's
binary, but a foreground run must still stop the existing Policy owner:
both subscribe and publish as `policy`. Restore one owner afterward. The
required actual-latch snapshot and unfitted-weight limits remain as above.
Continue fetching and reading the running list every half hour while no
new Fourth task is assigned.

## 2026-10-14, 15:36 ICT: task 28, memory-health — taken

Taken from the list at `17d312147908c04c97a9d433775844387ff89a5d`, after
rebasing Fourth and pushing the sync (`6ec55f39d`). First has integrated
the task 17 sure-name follow-up; no Fourth runtime difference remained at
this point. The new explicit order is 28, then 29, then task 8 (transferred
from Third despite its earlier table position).

Task branch: `fourth/memory-health`. Scope is Memory and its tests,
`docs/memory.md`, `docs/health.md`, and dependency metadata needed to use
the existing bus/message contracts. No new message type, Health/Actions
runtime edit or deployment change is planned. Read Actions' current report
as the established protocol and inspect Memory's actual server/store paths
before choosing the implementation.

Add an explicit `--bus-root` opt-in for one `memory` Diagnostic per second,
with actual exchange/kept counts and recent answer cost. Report warnings
for Memory's own storage failures and the specified note/call budgets
(300 ms / 1.5 s), not for ordinary empty results or invalid requests.
Keep bus initialization, publication and retries away from the socket loop,
with bounded state and one publisher worker. Disabled reporting must retain
the existing runtime behavior.

Latency target: reporting contributes no wait on bus I/O to an accepted
socket request; measure first and repeated local response time and the
health-update overhead separately, with an explicitly held publisher.
Use 1 ms p99 as the host target for the in-loop health update, not a lamp
claim. Preserve the Voice-facing 300 ms/1.5 s budgets and identify precisely
which portion Memory measures. Test real socket/bus reports, counts, errors,
recovery, opt-in absence, bounded shutdown and a stalled publication without
a lamp. Finish with the complete affected-crate gate and standalone source
verification. First's handoff must include `--expect memory` and the exact
Memory option, with no device action performed here.

## 2026-10-14, 16:33 ICT: task 28, memory-health — done

Built on `fourth/memory-health`, implementation
`11166b396d41ac2931e96ff7d4b3c159357a28c6`, fast-forwarded onto
`lamp-fourth`. The integration base remains the pinned `17d312147` above.
Memory now optionally publishes one `memory` Diagnostic per second when
given `--bus-root ABSOLUTE_DIR`. Without that argument, even an existing
`LAMP_BUS_ROOT` does not enable reporting or its worker/counters. The
existing Memory socket and response formats remain unchanged.

The report contains retained exchange/kept counts, their age, total and
recent answers, recent slowest note/call/answer and any in-flight answer.
Completed timings cover the current monotonic second and preceding 59;
an active slow handler also warns. The boundary is the complete request
entering the handler through its answer being computed, before logging
and delivery. Notes (`kept` and `session_open`) warn above 300 ms; other
complete requests above 1500 ms. These are Memory's service budgets, not
a claim that client queueing or transport fits a Voice deadline.
Ordinary invalid requests, empty answers and an unconfigured model are
not storage faults. An observed storage fault stays warned through reads
and clears after a successful append/remember/append_linked write.

All bus setup, publication, retries, cleanup and legacy file scans stay
on one worker, with fixed-size state and no event queue. A held publisher
cannot hold the socket loop. Legacy counts stream bounded lines and
discard a scan that overlaps any request; Days uses optional TEMP counters
and transactional triggers on its existing connection, with constant-time
reads and no durable schema change. Rollback, expiry, kind updates and
deletion cascades are tested. Shutdown waits at most 100 ms, including
publisher destruction, without joining a filesystem-blocked worker or
spawning its replacement. Terminal reporting is best effort; Health must
still detect a stale or missing process.

**Verification:** on this development Mac (macOS 26.6.2, Intel i7-10700K,
x86_64, 16 logical CPUs, 64 GiB, Rust/Cargo 1.96.0), from `lampOS/`, in order:

```sh
cargo fmt --all --check
cargo clippy -p lamp-memory --all-targets --offline --locked -- -D warnings
RUST_TEST_THREADS=1 cargo test -p lamp-memory --offline --locked
cargo build --workspace --offline --locked
```

All passed: 135 tests, zero failures, four manual timing tests ignored by
the normal gate. No crate depends on Memory. A standalone source-only
copy outside the repository, mounted read-only with network disabled,
also passed strict Memory Clippy, the same 135 tests and the workspace
build under Linux 6.12.13 x86_64 in the existing six-vCPU/8192-MiB Podman
VM. Its name `lampos-arm64` does not make these ARM64 tests. The Linux
workspace build retains Body's unrelated existing `unused_mut` warning;
Memory's strict Clippy is clean. The two new manual measurements were
explicitly run in release on the Mac, one pass each:

```sh
RUST_TEST_THREADS=1 cargo test -p lamp-memory --release --offline --locked --lib health::tests::measure_health_update_and_socket_with_a_held_publisher -- --ignored --exact --nocapture
RUST_TEST_THREADS=1 cargo test -p lamp-memory --release --offline --locked --test health_timing -- --ignored --nocapture
```

The two unchanged legacy timing tests were not repeated. Final functional
tests also cover absent/recovering real bus paths, duplicate JSON operation
keys, spurious worker wakes, a held real handler, and held publication and
publisher destruction. No test run failed. Development Clippy and format
failures, plus a staged whitespace-check failure in captured output, were
fixed and retained in the evidence.

[Full host evidence](verification/memory-health-host-20261014/README.md)
contains commands, the immutable Linux image, source hashes, raw output,
all runs and exact boundaries. On 6000 synthetic exchanges over 30 days
and 200 kept entries, each off/on socket case had one first request and
50 repeats. In-loop health bookkeeping p99 was 0.562 µs for JSONL notes,
4.689 µs for Days notes, and 12.373/15.493 µs for a 60 KiB append on
JSONL/Days, all below the 1 ms host target. Those exclude the store answer
and all bus I/O. The full Days append with reporting on took 61.227 ms
first, 60.199 ms median and 79.706 ms maximum over the repeats, measured
from connect through full response/close. A separately held publisher
allowed all 101 empty-store socket requests to finish (0.281 ms first,
0.036 ms repeated median). None of these numbers describes lamp flash
or acoustic response time.

**First on the lamp:** build with the usual lamp build flow. Preserve the
live store's `--memory-days` setting: First reports that store enabled at
14:50, and removing its flag is not a rollback. First run the new binary
with the reporting flag absent for the required minute. Then add
`--bus-root /run/lampos-bus` to Memory's existing command and
`--expect memory` to Health's existing serve command, retaining its other
expected parts. Run:

```sh
lamp-bus --root /run/lampos-bus tap diagnostics --seconds 6
lamp-health --bus-root /run/lampos-bus --json
```

Expect about one `memory` report per second, actual retained counts and
recent timing values (`none` before a relevant answer), with Memory OK
when healthy. The Health aggregate retains level/message/age, not the
Diagnostic's values; inspect the tap for counts and timings. Other parts
can still make the whole lamp warn. For flash timing, the compiled ARM64
`health_timing` test binary can run with `--ignored --nocapture`; it makes
its own temporary synthetic store and does not need the live records.

**Most likely wrong on the lamp:** flash contention and scheduling can
stretch the legacy scan/report cadence and Days transactions. Real Voice
queueing, transfer and answer delivery remain outside this report's
handler boundary. Pre-ownership CLI/bind failures cannot publish; an
open failure after ownership is best effort. Counts assume one writer;
external edits are outside the contract. Existing internal Days errors
classified as `bad_request` are not promoted to storage faults. One
successful write proves that write, not every storage path. No lamp,
ARM64, physical power-loss, real provider, live unit or deployment was
used. No unanswered question blocks task 29.

## 2026-10-14, 16:38 ICT: task 29, memory-voice — taken

Fetched and read the list at `ed4bccd6a0dd5c033983c21a2840835451dc5ddb`,
read this log, rebased Fourth onto that pinned integration and pushed
with a lease (`7cad55439`). Task 28's implementation is now `a7583dc14`;
its Memory source is unchanged by the rebase. The explicit next order
remains 29, then 8. First's 16:04 answer about task 17's labelled rows is
received; inspect the failed-to-answer fit, the copied busy-room rule
and the requested `voice_first_output` contract after those tasks, as
the answer orders. Do not silently treat those new lamp rows as host
evidence or a qualified model.

Task branch: `fourth/memory-voice`. Allowed implementation scope is
Memory, `crates/voice/src/remember.rs`, and the session-note call site in
`crates/voice/src/thread.rs`, with `docs/memory-days.md` and
`docs/memory.md` updated. No other Voice runtime, deployment or shared
message change. Inspect the current requests first: the existing recall
already sends query/day, so establish what actually changes for ranked
Days retrieval before replacing anything.

One explicit Voice environment switch will opt into frozen session notes
with provenance and conversation-end submission, retaining the old path
when absent. Targets: session-note acquisition within 300 ms total, tool
calls within 1.5 s total; no capture, playout or other foreground path
waits for Memory. End work is bounded and asynchronous, including when
Memory has no extraction model. Test the real temporary store, earlier-day
ranked recall, frozen notes and provenance, conversation-end receipt,
missing/slow Memory and truthful failure answers. Measure first/repeated
host latency and deadlines, keeping machine and boundary explicit.
Finish both affected-crate gates and a standalone source check; hand back
played scenes for First's `~/lamp-voice-data/tools/memory.sh`, expected
results and the most likely lamp-only failures. No device action here.

### Task 29 question: passing the one opt-in to the provider worker

The allowed files cannot deliver a new environment switch to the real
Voice provider: `crates/voice/src/process.rs:208` calls `.env_clear()` for
workers, and the coordinator's existing `tools::arguments_from_environment`
passes only the Memory socket, not a Days choice. Reading a new variable
only in `remember.rs` would appear to work in a direct test but stay off
in the actual lamp service. No path encoding, parent-process environment
inspection or side file will be used to bypass this boundary.

**Fourth-Q29:** may task 29 also change the provider spawn in `process.rs`
to pass only a validated `LAMP_MEMORY_DAYS=on` to the `provider` role,
leaving its absence and all other workers unchanged? The exact proposed
behavior is one `Command::env` conditional after `env_clear`, with a
test proving absent/invalid values and other roles get no variable.
Alternatively, First can add that narrow wiring when taking this task.
The remembered data, deadlines, opt-in client and real-store tests can be
built inside the assigned files meanwhile; that work continues. The final
handoff will distinguish a wired service switch from a direct-client test.

## 2026-10-14, 17:04 ICT: issue workflow adopted; task 30 — taken

Ready for lamp-voice: no

Read the new `joining.md`, `builders/fourth.md`, Queue #705 and Fourth's
issue comments after fetching. The current integration is pinned at
`462c18b924cd397f04df45f72503362b6a607b60`. Rebased Fourth and pushed with
a lease (`e484d862a`); task 28's code was already integrated. A rebase
conflict in reuse-decisions retained the upstream Schedule section and
its already-present Memory health section; no runtime source was changed.

First reports task 28 on lamp-4ace at 16:54 in closed
[#697](https://github.com/autonomous-ai/lampOS/issues/697): built for the
lamp, tests passed under emulation, Memory and Health flags enabled, and
Memory OK among twelve reports. That is First's lamp evidence, not a run
by Fourth. The finished task is the previously handed-off implementation,
now integrated as `7e858065b`.

Task 29 remains in progress on `fourth/memory-voice`; its pre-migration
claim is now recorded in [#698](https://github.com/autonomous-ai/lampOS/issues/698).
Fourth-Q29 was moved there once, as the adopted workflow requires. The
new client and real-store tests are implemented in the permitted files;
strict Clippy for Memory, Voice and its dependent Voice Eval passes on the
Mac, as do 22 focused Voice tests and 12 real-store/protocol tests. Full
gates and timings remain to do. No unfinished runtime code is on
`lamp-fourth`. The connector could read issues but returned HTTP 403 for
labels/comments; the already authenticated repository `gh` CLI completed
the authorized claims and question without a new account or credential.

Queue #705 now puts [#707](https://github.com/autonomous-ai/lampOS/issues/707),
task 30, first. Claimed it with `status:taken`, posted this session's claim,
and reread the comments: the first claim is this session's. Task branch
`fourth/records-exit`, in an isolated checkout while task 29's scope answer
is pending. Scope: `crates/records`, `docs/records.md` and test evidence.

Fix the scan's handling of a process that disappears or becomes a zombie
while its arguments are read. Keep refusing live but uninspectable
processes, including a live kernel thread with empty arguments; do not
weaken protection for a session held by a live process. Report which
processes were safely skipped. Use bounded rechecks, not a retry loop;
measure synthetic scan cost on this machine and keep it separate from
the lamp's hourly timer. Existing apply/enabled deletion opt-ins remain.
Run fixtures for disappearance, ESRCH, zombie, empty live arguments and
protected live sessions, the full Records gate, standalone verification
and the newly required Linux ARM64 build if the available toolchain can
provide it. First runs the finished build on lamp-4ace's timer.

## Not shown

No lamp access or deployment, Linux ARM64 execution, physical power-loss test,
real extraction model, speaker recognition, natural multi-day spoken test,
provider-context deletion, or end-to-end lamp latency. Memory remains behind
`--memory-days`; record deletion requires explicit apply and its disabled-by-
default configuration switch. Lean is not wired to Body and all supplied
geometry remains synthetic. Hand Stop and software mute are off unless their
independent Voice flags are supplied; no physical hand-to-silence or Body stop
was qualified. The local judge remains shadow-only behind `--local-judge`,
with unfitted weights and no proof that it improves the busy-room floor.
The log contains no unanswered question requiring
the owner to relay a message.
