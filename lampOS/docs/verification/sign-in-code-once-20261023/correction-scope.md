# Proposed product scope for #2839 — not implemented

The smallest coherent correction is to arbitrate the initial code handoff
in Tasks, the existing state owner shared by the two Voice processes. Ask
for permission to change that handoff only. Preserve an explicit new heard
START as the way to request the same open code again.

## Observed path

1. `tasks/src/runtime.rs:1088` handles `SignInStart` in the serialized owner.
   It tends the session, calls `Session::code_given`, then returns its view.
   `code_given` records nothing if the session is still `Starting`.
2. `voice/src/sign_in.rs:419` continues the same tool call by issuing another
   indistinguishable `sign_in_start` every 200 ms within its existing budget.
3. Between those calls, Harness's code enters `Session`. The news reader
   obtains a code with `code_told=false` from `sign_in_state`; it is a
   read-only operation. `sign_in::news` emits the note and `TaskNews` queues
   it (`task_news.rs:1100–1119`).
4. The pending START poll receives the code and marks it given. Future news
   reads suppress it, but `TaskNews::waiting`/`tell` at 837/948 retain the
   already queued note. Their expiration/progress checks do not revoke this
   sign-in code. The frozen regression observed both payloads.

## Why an ordinary Voice-local reservation is not shared

The production producers are different processes. `coordinator.rs:5515`
constructs TaskNews in the coordinator. `spawn_rig` at 3845 calls
`Worker::spawn_with_boot_and_tick`; `process.rs:411` calls `.spawn()` on the
command built at 916, `lamp-voice worker <role>`. `main.rs:690–700` enters
the Live provider child; 705–717 enters the second-route child. The tool
dispatchers and START run there. They do not share a Rust Mutex or Arc with
the coordinator's news producer.

A provider can die and be replaced while its coordinator and queued TaskNews
remain alive. A local reservation in the replacement provider would be
empty while the parent's queued code remains. The present host regression
runs both producers in one process only to use the real Tasks socket
boundary; that fixture topology must not be mistaken for the product's.

A Voice-only implementation is possible in principle, but needs a **new
cross-process** authority: coordinator IPC or an atomic shared file/lock with
session lifetime, writable-path and restart rules. An ordinary local
reservation cannot provide it. Such a mechanism would duplicate ownership
that Tasks already holds and would need its own mixed-build protocol or
persistence contract. This is why the existing Tasks owner is recommended,
not because two Rust threads cannot reserve atomically without holding a
mutex over an RPC.

## Both route boundaries

The coordinator has one news admission path at `coordinator.rs:11193–11276`:
quiet/presence/room/owner/readiness gates, `TaskNews::tell`, then
`commit_written_turn` of the note. No delivery ownership is checked there.

Live accepts `ProviderInput::Written` at `provider_worker.rs:6074`, marks
sign-in privacy at 6085 and submits text through `input.written` at 6121.
The second route marks privacy before keeping the note and passing it to
`engine.submit` (`second_provider/worker.rs:7701`, 7798–7823).
`sign_in::written` starts a background state read for scrubbing; it does not
wait for it or refuse speech. Transcript scrubbing deliberately leaves audio
untouched, so changing scrubbing cannot fix duplicate speech admission.

Tool calls on both routes use the shared tools dispatcher
(`tools.rs:1478` → `agent_tasks.rs:1732` → `sign_in::run`). Fixing the shared
handoff reaches both without adding socket waits to the coordinator or either
worker's audio/control loop. Their privacy and cancellation paths stay intact.

## Concrete initial-handoff contract to request

Add a separate trusted operation, provisionally
`{"op":"sign_in_take_code","id":<current sign-in id>}`. Its name and the
wire spelling below are proposed, not an implemented protocol. Tasks handles
it in its existing serialized owner; neither Voice process holds a mutex
across a socket call.

For the requested id, tend the session, then make one atomic decision:

| Current owner state | Result | Code in reply | Ownership change |
|---|---|---|---|
| Starting | `waiting` | None | None; an initial continuation may poll within its existing budget. |
| Current, unexpired Code not yet given | `granted` | Current code and remaining lifetime | Mark that code given before replying. |
| Current Code already given | `already_given` | None | None; the initial continuation stops polling. |
| Missing/stale id, spent/expired code, Asking, Answered or Done | `not_available` | None | No grant; never substitute a different session's code. |
| Switch off or untrusted socket | Existing refusal | None | None. |

Reuse the session's `code_given` identity so an old `sign_in_start` that
already returned the code also excludes an initial claim. A same-code Harness
keepalive may extend its lifetime but does not reopen ownership. A different
code follows the existing newer-code behavior; it must never receive a grant
using a stale earlier code snapshot. Return the code from the atomic grant,
not from a preceding `sign_in_state` read. Keep state observational.

A new Voice checks an additive capability on `sign_in_state` before its START
or automatic code handoff. For example, `code_claims: 1` is present even when
there is no session. A capability check, START, and any continuation all use
the original overall tool deadline; individual RPC and polling limits do not
grow. This check is also how an old Tasks refusal is handled safely below.

For each new heard START tool invocation, issue `sign_in_start` **once**. If
that first reply contains an open code, it remains the explicitly requested
answer or repeat. If it says Starting, remember its id and continue only with
`sign_in_take_code`; do not issue another indistinguishable START as a poll.
News also uses `sign_in_take_code` before offering its code note. Only a
`granted` reply supplies digits. If news won, the pending tool returns a
non-code result that the code is being provided, not an error and not another
copy. If START won, a reader holding older state cannot obtain another grant.
The existing heard-only gate stays ahead of these operations.

This is **at most one initial payload handoff per current code**, plus an
answer to each genuinely new heard START. It is not exactly-once acoustic
speech. The frozen host test counts payloads; both route output and lamp
speech need their own qualification.

## Cancellation, expiry and restart semantics

- **Cancel, Stop, privacy suppression, lost reply or a worker dying after a
  grant:** consumption is not refunded. Do not automatically retry the code
  or reset its ownership because no hearing acknowledgment arrived. Preserve
  the existing final-output ownership/privacy/cancellation gates. If the
  person did not hear the code, a later heard START can request it again.
  An initial grant is not a permission to bypass those gates.
- **Provider child restart:** the Tasks session and the coordinator's queued
  note can outlive the child. The replacement child cannot get a second
  initial grant. A genuinely new heard START remains the explicit recovery;
  a restarted internal continuation is not reclassified as a new request.
- **Whole Voice restart:** an initial claim remains consumed in Tasks. Losing
  the in-memory news queue must not reopen it. Recovery is again an explicit
  heard START. This deliberately prefers a missed automatic telling over an
  automatic duplicate after an ambiguous failure.
- **Tasks restart or session replacement:** old continuations must fail closed
  for their old id rather than silently starting a new Harness session. The
  existing id seed is wall-clock milliseconds (`runtime.rs:735`), retained
  here; this proposal does not introduce new persistence or strengthen that
  existing identity scheme. A fresh heard START may open the next session.
- **Expiry before claim:** the new operation refuses a Code whose own `until`
  has passed, even if its session remains open. This needs an explicit check:
  current `Session::tend` enforces the session/question deadlines, while
  `view` can still return a Code with `expires_in: 0`. Do not claim the old
  START operation already enforces this or change its old wire behavior
  implicitly.
- **Expiry after a news grant:** propagate a conservative remaining lifetime
  into TaskNews's existing `News.until`; `waiting` and `tell` already drop
  expired notes without waiting. Derive the local deadline from the RPC's
  start plus the owner's remaining duration, not receipt time, so transit
  cannot extend validity. Discard a reply already past that deadline. This
  small mapping addition belongs in the requested scope because today's
  sign-in mapping sets `until: None`. It does not promise to retract audio
  already admitted before expiry.
- **Question and answer:** code ownership never acknowledges the question.
  Preserve `sign_in_asked`, the heard-whole requirement, cut-short question
  reoffer, later-request yes/no checks and the original answer deadline.
  A code after Asking remains ineligible. No new cancel-Harness operation is
  proposed; existing operations do not include one.

One important limit is explicit rather than hidden in this request: if a new
heard START asks for the code while an earlier granted news note is still
queued, this minimal contract authorizes one initial news payload and that
one requested payload. The old note can arrive later under the unchanged
coordinator gates. Automatically replacing or cancelling that outstanding
note when the repeat occurs would require a separate cross-process final-
admission/revocation design. An eventual `code_told` reread is not an atomic
solution. First should decide whether #2839 requires that stronger ordering
before implementation; it is not demonstrated by the current failing case
and must not be promised by the initial-claim fix. Likewise, the claim alone
cannot revoke an already queued note immediately on a remote session change.
These are concrete output-boundary limits, not reasons to weaken the current
count assertion or remove its deterministic overlap.

## Exact product scope to request

- `crates/tasks/src/protocol.rs`: additive trusted operation parsing and
  classification; unchanged old request shapes.
- `crates/tasks/src/sign_in.rs`: atomic per-code grant and expiry check.
- `crates/tasks/src/runtime.rs`: serialized id/switch/trust checks, grant
  response and additive capability in state replies.
- `crates/voice/src/sign_in.rs`: capability/refusal handling, distinction
  between one new heard START and its continuation, and news claim using the
  granted payload and lifetime.
- `crates/voice/src/task_news.rs`: carry only the sign-in code's expiry into
  existing `News.until`; retain question retries and existing admission gates.
- Associated unit/integration tests and `docs/tasks.md` behavior/protocol
  documentation. No new switch, tool, topic, unit or msgs field is proposed.

Route source need not gain a blocking RPC: both tools already run through the
shared sign-in dispatcher, and the news reader is already a background task.
If implementation needs a new coordinator/worker IPC message or a final-output
revocation mechanism to meet the stronger ordering above, return for that
explicit scope rather than hiding it in this list.

Removing START's poll loop is a smaller correction for the single reproduced
ordering, but changes initial timing and leaves ownership/repeat races
unresolved. A local mutex cannot arbitrate the real two-process topology.

## One-build-apart compatibility is part of the requested scope

The subsequently read `AGENTS.md` blob `71ecf9471` requires communication to
work with the previous build on either side; a new reader refused by an old
server must act safely and explain it. The recommended behavior is definite:

- **Old Voice, new Tasks:** retain the request shapes and reply meanings of
  `sign_in_start`, `sign_in_state`, `sign_in_asked`, and `sign_in_answer`.
  Add a separate operation because the old request parser rejects unknown
  fields. The extra capability is in the JSON response and the previous
  Value-based reader ignores it. Test the old reader against the new owner.
  An old Voice still makes independent legacy START/state calls: it does
  **not** acquire the duplicate fix merely because Tasks was updated.
- **New Voice, old Tasks:** a missing capability or `bad_request` refusal
  disables the new code handoff for that call. Do not fall back to legacy
  START polling or emit code news from a state snapshot. Return a bounded,
  non-code tool explanation such as "Voice sign-in needs the Tasks update
  to finish; please try again after it updates." No automatic retry loop
  attempts the old duplicate-producing pair. For an existing Asking session,
  the unchanged question/answer protocol remains separate. A refused
  background news claim stays silent; the next heard START gives the clear
  explanation. This proposal does not allow an optional legacy code fallback.

Test refusal after a capability read as well as an old server from the start;
a Tasks replacement must not cause fallback or a longer deadline. The mixed-
build tests also need a true separated coordinator/provider-process case or
fixtures with distinct process-local state. A common static must not make the
host test green while production remains unfixed.

## Validation that distinguishes the decisions

Retain the present failed and passed samples without rerunning unchanged
source. With an approved implementation:

1. The existing frozen direct and queued cases must both pass without
   weakening their delivery counts, one-session requirement, refused
   off/unheard calls, or production budgets.
2. Force the opposite order with barriers: START continuation claims first,
   then a reader holding a prior state attempts to claim. Only START gets
   digits. In the present order, news claims first and only news gets them.
   Removing the grant check must fail the relevant original count assertion.
3. Keep an open code across a duplicate Harness keepalive; it gives no new
   automatic delivery. A genuinely new heard request returns that code once.
   Cover that request before and after queued news is taken, cancellation,
   lost replies, worker restart and expiry. Record the stronger queued-repeat
   and session-revocation limit above; do not claim the grant solves it or
   mistake a claim for proof of hearing.
4. Reject stale ids, wrong routes and switch-off claims; no code/account may
   reach public replies, journals or kept transcripts. Preserve question
   acknowledgment, cut-short reoffer and yes/no timing independently.
5. Drive both route stand-ins to their actual Written/tool-result boundary,
   retaining the existing coordinator quiet/presence/ownership checks and
   the Live/second privacy regressions. No slow Tasks request may block Stop
   or audio; keep existing latency bounds and use events, not longer sleeps.
6. Run fmt, strict Voice/Tasks Clippy, relevant focused tests and full serial
   Voice/Tasks with consumer/workspace gates as required, then Linux CI.
   A separate authorized lamp played run must count actual code speech;
   these host payload counts cannot replace it.

All above is source analysis and a proposed scope, not new runtime behavior,
new test execution, or proof that the lamp report has this sole cause.
