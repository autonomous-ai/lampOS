# The lamp memory benchmark

31 October 2026. Test plan and draft arm contract for the
[tiered memory design](memory-design.md), adapted from the supplied evaluation
plan. This document pre-registers the method; it does not claim that a runner,
fixture set, scorer, or baseline result already exists.

The question is whether a memory system remembers and uses the right things
from a fictional owner's month, stays silent about what it should not know,
and earns its cost. All arms receive identical observed inputs and use the
same pinned answer model and generation settings. Only the memory mechanism
changes. The primary comparison is on a held-back persona; development scores
are never called held-back results.

## Scope and reuse

The implementation belongs in `crates/memory-bench` (binary
`lamp-memory-bench`) and `fixtures/memory-bench`. Rust only. Existing product
memory, voice, and deployment are read-only.

Reuse or adapt the existing memory store, server, and socket contract, the
test infrastructure described in [fixture clock](memory-fixture-clock.md), the retrieval evaluation in
[memory-evaluation.md](memory-evaluation.md), and the voice-eval append-only
ledger pattern. Inspect the current second-route memory note, tools, prompts,
and probes before implementing the current-product adapter. Copying only its
keyword search would understate that baseline. Borrow the supplied research's
source-cited trait scoring idea without importing another product's runtime
or real personal data.

The existing child fixture clock controls reporter scheduling, not store
aging. The month runner therefore uses a **bench-owned child** around the
real memory store and server, passing an injected logical clock through the
existing library interface. This keeps production code read-only. Explicit
event timestamps and the store clock advance together. It exercises real
storage and socket behavior, but not the unchanged production executable's
wall clock, production startup, or acoustic timing. Verify the adapter's
clock boundaries before claiming a complete accelerated month.

## A fictional month and a hidden answer key

The development persona contains roughly 40 facts and 12 traits, expressed
in different words on different days. All people, relationships, locations,
work details, and events in the fixtures are fabricated. No participant's
real name or handle enters the repository.

Generate 30 chronological days, normally 50–150 directed lines per day, with
session boundaries, visitors, arrivals and departures, breaks, plans that
change, corrections, uncertainty, and quiet intervals. Script the lamp's
replies consistently so one arm's early answer cannot change another arm's
later input. Use a recorded generation seed and stable record ids.
Each persona declares an IANA timezone. The development fixture uses
`Asia/Ho_Chi_Minh`; the held-back persona may use a daylight-saving zone.
Resolve relative dates, appointment expiry, and night/week boundaries in that
zone, while retaining UTC milliseconds for event ordering. Test a local
midnight and a daylight-saving transition without adding fixed 24-hour days.

The evaluator's answer key is separate from the observed event stream. Each
claim or trait has supporting record ids, validity times, acceptable answers,
and negative examples. An arm never sees that key, future events, the clean
script behind a misheard line, true speaker labels, question categories,
scoring rubrics, or other arms' outputs.

Decoys include a planted instruction, a guess, a joke, a reversed preference,
a colleague's private matter, and a visitor's own statement. Include both
uncertain speaker attribution and confident-but-wrong observed attribution.
Measure their consequences separately rather than assuming identity is solved.

A second persona is an empty held-back slot until an independent author fills
and seals it. The benchmark builders and candidate builders do not tune to
it. Its answer key and final questions stay outside arm inputs. Human checks
of the generated month and held-back material are separate acceptance steps;
synthetic generation alone is not human approval.

## Real speech-to-text errors, cached once

On a Mac, synthesize the scripted speech to files with `say`; do not play it
in the room. Feed those files through the real Deepgram route used by the
probes. Store a manifest of script hash, audio hash, voice and synthesis
settings, speech-to-text configuration, model/version when reported, request
identity, transcript, confidence when available, and response hash.

All arms and repetitions use the same frozen transcript cache. Do not
retranscribe for each arm or repair an error after seeing an answer. A missing
cache entry, changed model configuration, or corrupt hash is a failure, not
permission to fall back to the clean text. Keep audio outside Git; commit
text manifests and hashes, subject to the repository's artifact rules.

Report results on both clean text and observed transcripts when useful, but
keep their labels separate. The primary spoken-input experiment uses real
cached transcripts. Deterministic recorded-provider tests prove software
behavior only; they are not real-STT or live-model baseline scores.

## Freeze questions and scoring first

Commit approximately 200 scheduled questions, their expected answers and
rubrics, the scorer implementation, and tests before any arm is scored. The
run manifest records that commit and content hashes. A correction after a run
creates a new experiment version and retains all earlier results. Never
rewrite a failed run out of the ledger.

| Kind | What it tests | Scoring |
|---|---|---|
| Recall | Names, values, objects, and exact past statements | Registered exact answers and aliases |
| When | When an event was discussed or happened | Parsed date with the registered one-day tolerance |
| Changed facts | Latest value versus retired history | Exact value valid at the question time |
| Across days | A grounded account of a changing situation | Required points, exclusions, and citations |
| Understanding | Correct answers to the same trait questions across all arms | Trait recall and false claims separately; candidate owner-model inspection is an additional diagnostic |
| Unasked preferences | Appropriate suggestion or answer length | Explicit positive and negative rubric |
| What is ahead | A timely follow-up, asked only once | Scripted multi-turn probe with a time window and count |
| Not knowing | A fact never supplied | Registered abstention forms; reject an invented answer |
| Visitors | Disclosure and wrong-person retention | Leak checks plus denied-access and attribution probes |
| Forgetting | Asked-for deletion survives time and sleep | Probe after deletion and again after consolidation |
| Decoys | An instruction or statement that must not become a fact | Forbidden claims/actions, not merely answer similarity |

Prefer deterministic scoring. Exact matching must not accept a correct token
inside a contradictory answer (for example, two incompatible dates). Normalize
only what the question registers; do not use broad substring or fuzzy matches
that excuse wrong answers. Date scoring uses the fixture timezone and question
clock, never the machine's current date.

A rubric judge sees the question, candidate answer, and fixed rubric, without
the arm's identity. Keep its model, prompt, output, token use, and cost. Test
it against deliberately wrong answers and contradictions. A person audits a
registered sample, all judge/scorer disagreements, and all suspected privacy
or forgetting failures. A judge-only score is provisional until audited.

Questions must not teach the main month. Each independent question starts
from the same pre-question state. A multi-turn follow-up probe may mutate its
own isolated state, then that state is discarded. Store neither its question,
answer, nor model-generated facts into the month's main state.

## Arms and fairness

| Arm | Required behavior |
|---|---|
| `none` | Current question and the common immediate conversation only; no prior-day memory |
| `full_history` | All eligible observed history up to the question, without hidden truncation |
| `lampos_today` | Real memory store/server in a bench-owned process with an injected clock, at a pinned revision; preserve second-route observations, talk notes, running summary, note, tools, and prompts |
| `mem0` | Out-of-tree adapter pinned to a revision and configuration |
| `memoryos` | Out-of-tree adapter pinned to a revision and configuration |
| `tiers` | The candidate adapter from `crates/memory-tiers` |
| `tiers_minus_*` | One registered component removed at a time |

Full history is a reference, not a guaranteed accuracy ceiling. If the frozen
month exceeds the pinned model's context window, record an unsupported or
failed run; do not silently truncate and retain that label. Privacy and
forgetting constraints also apply to full history. The independent scorer
must distinguish no access, absent evidence, and a failed service.

The current-product arm must record its revision, switches, person mapping,
session lifecycle, note construction, tools, observations, talk notes,
running summary, consolidation path, and clock. Keep the original product memory instructions with that arm, and
report this necessary prompt difference. All arms otherwise use one common
answer prompt, question context, model, sampling settings, output limits, and
tool-round bounds. Arm-specific extraction/sleep calls are permitted and fully
counted. No arm bypasses the benchmark's model client.

Run each arm three times with fresh state. Record order, seeds, model identity,
all failed attempts, and spread by question kind. Candidate ablations use the
same inputs and budget; do not improve an ablation's prompt after seeing its
score. Treat an unavailable outside arm as unmeasured, never as zero accuracy.

## JSON-lines arm contract, draft v1

This is the boundary for an in-tree or external memory adapter. It deliberately
uses no Rust crate dependency across implementations. The first fixture and
contract tests will freeze it before baseline runs. Additive fields require a
versioned compatibility test; breaking changes require another protocol version.

The benchmark launches one adapter process with private fresh state. stdin
and stdout are newline-delimited UTF-8 JSON objects; stderr is diagnostic only.
Exactly one request is active. Every frame carries `v: 1` and an `id` echoed
by its final response. The envelope is:

```json
{"v":1,"id":"r1","op":"init","data":{}}
{"v":1,"id":"r1","ok":true,"data":{}}
{"v":1,"id":"r1","ok":false,"error":{"code":"unsupported","message":"short explanation"}}
```

Unknown versions and operations are errors. A nonzero exit, malformed frame,
wrong id, duplicate terminal response, or missing response invalidates the run.
The runner enforces maximum 4 MiB per frame, 30 s for a local operation and
120 s for an operation that explicitly permits a model call. One call may be
outstanding, with at most 8 model calls in an operation. Model and wall-clock
budgets may impose stricter bounds. Exceeding a bound records a failure and
ends the owned process; there is no silent retry.

Recall has a **200 ms turn budget**, measured from sending the request until
its complete response is available. A separate session-start `snapshot` has
**300 ms**, matching the product note budget. The arm returns empty memory
when its budget expires; an answer never waits for late recall or a late
snapshot. The runner also enforces these deadlines, ignores late content,
and records a miss. At most one request stays in flight while its terminal
response is drained under the outer watchdog; no late response can become
the next question's memory. These are host contract boundaries, not proof of
first-audio latency on a lamp.

`init` runs exactly once, before any other operation. There is at most one
open probe; `probe_end` must match it, and `close` follows its restoration.
Only `ingest`, `sleep`, `recall`, and `tool` permit model calls. `ingest`,
`sleep`, and `tool` receive the 120-second operation deadline; calls made by
`recall` remain within its 200 ms total budget, with empty memory on expiry.
`snapshot` is local and bounded by 300 ms. Other operations are local, with the
30-second deadline, and cannot extend it by asking for a model. Request ids
and model call ids are unique across the whole run. Event and operation time
never moves backward within the main state or an isolated probe. Restoring
the main state also restores its logical clock; global run accounting never
moves backward. Invalid order is an error without a state transition.

All times below are unsigned Unix milliseconds on the **fixture clock**.
Ids are stable strings. `person` is the **observed** scope (`owner`, `known`,
`visitor`, or `unsure`), and `person_id` is a fabricated identifier or null.
`presence` is `owner_alone`, `others`, or `unknown`. Neither scope nor presence
is an evaluator's hidden correction of an observed mistake.

| Operation | Request `data` | Successful response `data` |
|---|---|---|
| `init` | `run_id`, `arm`, `state_dir`, `owner_id`, `timezone`, `at_unix_ms`, `config` | `arm`, `implementation_revision`, `capabilities` |
| `ingest` | `event` (shape below) | `accepted`, `source_ids` |
| `sleep` | `at_unix_ms`, `cadence` (`nightly` or `weekly`) | `applied`, `source_ids`, `reason` (nullable), `pending`, `next_cursor` (nullable opaque string) |
| `forget` | `at_unix_ms`, `person_id`, `record_ids`, `text` | `applied`, `removed_count` |
| `probe_begin` | `probe_id`, `at_unix_ms` | `isolated: true` |
| `snapshot` | `query` (shape below, with empty `heard`) | `owner_model`, `source_ids`, `memory_instructions` |
| `recall` | `query` (shape below) | `context`, `source_ids`, `owner_model`, `tools`, `memory_instructions` |
| `tool` | `probe_id`, `name`, `arguments` | `result`, `source_ids` |
| `inspect` | `person_id` | `owner_model`, `retained_record_ids`, `stats` |
| `probe_end` | `probe_id` | `restored: true` |
| `close` | empty object | empty object, followed by clean exit |

`config` is frozen arm-specific configuration, without credentials. The
runner resolves and validates `state_dir`; an adapter writes only there.
`capabilities` lists supported operations. Missing a required operation makes
a run unsupported. A no-op sleep for a baseline reports `applied: false` and
`reason: "not_applicable"`; it is not a failed night. A rejected candidate
update instead reports its actual reason. `accepted` means eligible input was
handled, not that an owner fact was proven. `source_ids` always refer to input
record ids, never to the answer key.

Sleep reports `pending: true` while eligible records remain unprocessed;
`next_cursor` identifies its resume point without containing record text.
The runner repeats bounded batches at the same fixture time up to the
manifest's batch and cost limits. Exhaustion with work pending is an
incomplete night, never a successful pass. A no-op baseline returns
`pending: false` and `next_cursor: null`. A candidate cannot silently skip
records when a batch fills. Its cursor advances only after a successful apply.

An event:

```json
{"record_id":"day01-001","at_unix_ms":1893492000000,"session_id":"day01-am","kind":"utterance","person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"I might go on Sunday.","said":"","stt_confidence":0.91,"cues":{"activity":"desk","objects":[],"topic":null}}
```

`kind` is `utterance`, `session_start`, `session_end`, `arrive`, or `leave`.
For non-utterances, `heard` and `said` are empty. `stt_confidence` is a finite
number in [0,1] or null when the provider gives none. `cues` contains observed
`activity` (nullable string), `objects` (strings), and `topic` (nullable string).
Do not supply gold importance, hidden truth, or expected extracted facts.
Explicit remember and correction requests arrive as ordinary observed text;
there is no oracle flag telling an arm which fact it should extract.

A query:

```json
{"probe_id":"q001","at_unix_ms":1893578400000,"person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"Which day did I say I might go?","cues":{"activity":"desk","objects":[],"topic":null},"max_context_bytes":16384}
```

`context`, `memory_instructions`, and each `owner_model` line are UTF-8 text.
`owner_model` is an array of `{text, source_ids}`. The requested context bound
counts all UTF-8 text memory adds to the prompt: context, owner-model text,
memory instructions, textual tool schemas, and subsequent tool results.
The base character prompt is outside this cap and identical across arms.
The snapshot is reused within its session, counted once; recall must not
duplicate it. Forgetting or a changed access scope invalidates that snapshot.
Report any selection
in `stats` when inspected. The example's 16,384-byte bound is illustrative;
the run manifest fixes one common bound for all arms, plus model token
accounting that reserves the common prompt, question, immediate conversation,
and maximum output. No field bypasses either limit. Full history must fit its
entire eligible history within those limits or return `unsupported`, without
selecting a subset and continuing as full history.
`tools` contains OpenAI-compatible function schemas for the adapter's memory
tools only. The runner uses the common answer model, returns its tool calls
through `tool`, and limits the tool loop to 8 rounds. The arm must not emit
its own final natural-language answer instead of recall context. The
`lampos_today` adapter supplies its actual memory instructions and schemas;
other arms may return empty tools/instructions. Their differences are recorded.

`probe_begin` creates a reversible isolated view of the current arm state.
`ingest`, `forget`, `sleep`, `recall`, and tools inside it affect only that
view. `probe_end` restores all durable and in-memory state, including caches,
follow-up marks, forget fingerprints, consolidation cursors, and counters
used by the memory policy. No nested probes.
A runner aborts rather than continue if isolation or restoration fails.
Probe restoration never restores or refunds global model cost, reservations,
request identities, or ledger history. Those remain monotonic across probes.
`inspect` is read-only and never feeds the answer model. It is evaluator
instrumentation for attribution, deletion, and candidate owner-model checks;
unsupported inspection is reported and cannot establish those safety checks.
Common trait questions measure understanding in every arm; candidate-only
owner-model provenance and false-line checks are separate diagnostics. An arm
without an owner-model representation is not assigned zero understanding.

Forgetting tests explicitly distinguish a direct deletion from recognizing a
natural-language request. The main experiment sends the request as ordinary
`ingest` text so the arm must recognize it and choose its own tool or action.
Direct `forget`, whether given ids or text, is a separate component test; it
cannot prove end-to-end natural-language forgetting. The runner never tags
an ordinary input with the scorer's hidden intent classification.

### Model calls through the runner

An arm needs no network key. During an operation it may emit a model request
instead of its terminal response:

```json
{"v":1,"id":"r2","model_request":{"call_id":"m1","kind":"chat","purpose":"sleep","messages":[{"role":"user","content":"bounded input"}],"max_output_tokens":1024,"response_format":{"type":"json_object"},"tools":[],"tool_choice":"none"}}
```

The runner replies on stdin, then the adapter resumes the original operation:

```json
{"v":1,"id":"r2","model_response":{"call_id":"m1","ok":true,"content":"{\"facts\":[]}","tool_calls":[],"usage":{"input_tokens":20,"output_tokens":4}}}
```

On refusal or failure it returns `ok: false` and an `error` object instead of
content. Chat messages use OpenAI-compatible roles and preserve structured
assistant tool calls and tool-result ids. `tools` contains function schemas;
`tool_choice` is `none`, `auto`, or `required`. A returned `tool_calls` array
contains `{id, type: "function", function: {name, arguments}}`, where
`arguments` is a JSON string. `content` can be null for a tool-only response.
The adapter executes memory tools inside its own current state, then sends
their structured results through the same broker for any next model round.
This supports the actual current-product ingestion and observation path;
no hidden direct model connection is allowed.

An embedding request uses `kind: "embedding"`, `input` (an array of strings),
and `purpose: "retrieve"`, rather than chat fields. Its response contains
`vectors` in input order and `usage: {input_tokens, output_tokens: 0}`.
The runner pins the embedding model and dimension in the run manifest,
bounds batch size and text bytes, and rejects non-finite or wrong-sized
vectors. Different embedding requirements are an explicit recorded arm
configuration, not permission for an unmetered external call. Recorded tests
cover this path too. Embedding and chat capabilities are declared separately;
an unavailable required capability makes the arm unsupported.

`purpose` is `extract`, `sleep`, `reflect`, or `retrieve`; the runner
owns answer and judge calls. Every provider request, including transcription,
passes through the single bounded client. It reserves a conservative maximum
charge before dispatch, reconciles reported usage after completion, and never
assumes an unknown charge was zero. Budget refusal makes no provider call.
The ledger retains cost reservations for uncertain failed calls.

Recorded model outputs use the same client interface and request-content
hashes. A fixture miss is an error. These runs are labelled `recorded` and
cannot be mixed with `live` scores. Record mode, provider/model revision,
pricing source/date, token/audio counts, dollars, and bytes sent. Never put a
credential or private configuration path into a ledger or process argument.

## Runs, ledger, and scorecard

A run starts with an immutable manifest: experiment commit, fixture hashes,
transcript-cache hash, arm and product revision, adapter configuration, model
settings, cost ceiling, deadlines, seeds, and run mode. Append events for
initialization, ingestion, consolidation, probes, scoring, costs, and terminal
status. Create exclusive run directories, append without rewriting, and fail
closed on duplicate ids or a broken/truncated record. Hash chaining can detect
accidental edits but is not a claim of tamper-proof storage.

Print counts, denominators, per-kind score, and the three-run spread. Report
unsupported cases, failures, judge disagreements, and pending human audits
beside scores. The scorecard also records visitor leaks, failed forgetting,
decoys accepted, correct abstentions, owner-model false claims, model cost,
text sent per owner-day, storage, and host recall time. A shorter answer is
not automatically a faster first spoken word.

Winning requires a held-back score above lampOS today and mem0, close to full
history, and strong trait accuracy without false owner-model lines. Privacy
and forgetting are hard gates: zero visitor leaks, zero accepted decoys, and
100% forgetting persistence in the registered cases. Removing a tier must
lower the relevant score, or that tier has not earned its place. The proposed
"within a few points" accuracy tolerance and cost target must become numeric
pre-registered thresholds before final evaluation; do not choose them after
seeing the scores.

## Outside systems and later validation

Run mem0 and MemoryOS in separate external environments, pinned to exact
revisions and dependencies. Their adapters speak the contract above and use
the runner's model broker. No Python implementation, virtual environment, or
service enters lampOS. Record extraction settings, embedding models, prompts,
and any unavoidable differences. An adapter that calls a provider directly
is not eligible for the budgeted comparison.

Public LongMemEval, LoCoMo with audited corrections, and PrefEval runs are
secondary comparisons, with their dataset version and scorer identified.
Retrieval evidence recall is not answer accuracy. Do not transplant a score
from an existing retrieval report into this month benchmark.

Later, an authorized lamp replay can measure real microphone attribution,
speech recognition, first-answer audio, CPU, storage, and cost using the
appropriate clock for each boundary; the current reporter fixture alone
cannot accelerate store aging. Register a first-audio comparison boundary and noise tolerance
before running, including cold use, repeated turns, and slow dependencies.
No additional first-audio delay is the design target. Finally, two weeks of
consenting real-owner trials and a human diary of useful or awkward memory
moments are separate evidence. Neither step is authorized by this host-only
benchmark task.

## What was not shown

The month, approximately 200 questions, scorers, executable contract, cached
speech-to-text pass, live baseline scores, independent persona, human audit,
and real-device latency remain to be implemented or measured. No baseline
number is claimed by this plan. Relay access and an explicit run cost ceiling
are needed before live calls; they do not block recorded-output tests.
