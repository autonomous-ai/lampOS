# Tasks

Tasks is an optional process that keeps what the lamp proposed to an agent,
the person's confirmation, the one dispatch attempt, the output observed
later, and what Voice reports as played. Its private records survive process
restart. It starts no work without `--enable` and a fresh trusted confirmation.
The lamp's existing Voice, Actions, deployment and account configuration are
unchanged by this crate.

## What exists and what we take

The chosen route is the installed Harness CLI, as described in
[brain-build](brain-build.md#step-5-in-more-detail-tasks-through-harness-on-the-lamp).
The [contract review](tasks-contract.md) records the inspected owner revision,
V1 behavior, command shapes and unresolved boundaries. The review compared
the CLI, its MCP wrapper, direct local WebSocket RPCs and the older Device
protocol. The CLI retains Harness's caller policy, account, lineage, rate
limits and ownership checks. MCP adds no stronger completion receipt; raw
mutation RPCs would omit part of that policy; Device is the route explicitly
not taken. Tasks implements no pairing, login, credential reader or transport.

Keep V1's write-before-send reservation, explicit uncertainty, identity checks,
durable speech claims and admission-versus-playback distinction. Reuse Rust's
process/socket facilities, `rustix` for bounded polling and process groups,
`serde` for strict local requests, SHA-256 for immutable local fingerprints,
and bundled `rusqlite` for the journal. The private Store follows the reviewed
[Schedule](schedule.md) persistence implementation without a Tasks-to-Schedule
runtime dependency. All source remains inside the standalone `lampOS/` tree.

The current CLI cannot prove completion of the particular lamp request. Every
successful local response therefore carries `completion: "unverified"`.
`idle`, text saying “done,” and a successful process exit never change that.
There is no automatic completion announcement in this implementation.

## Ownership and switches

`lamp-tasks serve` returns `enabled:false` before opening a store, reading a
target file, constructing an adapter, accessing a clock, or creating workers.
`--enable` starts the private service. It does not confirm a task. There is no
automatic deployment, service unit, startup wiring or Voice tool registration.

The operator supplies an existing Harness executable and a target allowlist.
The model can choose an alias and task text; it cannot choose an executable,
environment, permission mode, machine prefix, transport frame or arbitrary
CLI verb. The first scope starts a new child through `new`; focused-agent
selection and following up an existing child require an owner-supported
contract. Tasks never substitutes a guessed agent or another engine.

The target file must be an absolute, service-user-owned, single-link 0600
regular file, at most 16 KiB, with version 1 and 1–8 unique aliases. For example
(the machine ID below is a fixture, not an actual account destination):

```json
{"version":1,"targets":{"coding":{"machine_id":"0123456789abcdef0123456789abcdef","project":"desk-work","engine":"codex"}}}
```

After the real-account qualification below, First can provision the parent
directories and run the explicitly enabled service:

```sh
./lamp-tasks serve --enable --dir /var/lib/lampOS/tasks \
  --socket /run/lampOS/tasks/request.sock --voice-socket /run/lampOS/tasks/voice.sock \
  --targets /etc/lampOS/tasks.json --harness /absolute/provisioned/harness
```

`--harness-as-person` (off unless set; added 2026-10-14 by the first
builder) calls Harness without `AI_AGENT` and `HARNESS_AGENT_SESSION`, so
Harness takes the call as the person's own. Marked as an agent, `harness new`
is refused until the person turns on "Agents can start other agents", an
account-wide setting (see "First real Harness check" below). The owner chose
the plain CLI on 2026-10-14 ("just use the harness cli to create a new
claude code session"); the person's spoken yes to each exact task is still
what lets a task start.

`--progress` (off unless set; #732, the second builder, 2026-10-15) reads
how each task handed over is going with one `harness ps` a machine and puts
it on the bus as `tasks/status`, without the agent's words: ["Progress on
the bus"](#progress-on-the-bus-serve---progress-732) below.

`--push-wait` (off unless set; #560, nine, 2026-10-16) holds Harness's own
`wait` on the agents the watch looks at and reads an agent as soon as it
ends, so a result is known within seconds rather than at the watch's next
look: ["Watching"](#the-lamp-as-chief-of-staff-watching-and-telling-2026-10-15)
below. It reads with the watch, so it refuses to go with `--no-watch`.

`--bus-root ABS` selects an isolated bus for qualification. `--run-for-ms N`
bounds an operator run to 1–86,400,000 ms; it does not cancel remote work.
Linux reads its boot identity itself; development hosts require `--boot-id ID`.
Configuration is loaded once. Prepared records retain their exact target and
must be prepared/confirmed again after restart, even if an alias was remapped.

The request socket permits preparation, inspection, cancellation before an
attempt, and an explicit observation request. Only the separate Voice socket
permits `confirm`, `claim_speech`, `ack_speech` and `told`. Both sockets and their
parent are private to the service user. This separation is an integration
boundary, not authentication against another process with that same UID.
Actions must expose only the public operations to the model. First must wire
the trusted route exclusively to Voice's real confirmation and output owner.

“Spoken yes” means the **person's** confirmation of the exact prepared task,
not the lamp saying “on it.” The private caller attests that fact; Tasks does
not recognize speech or identify a speaker. It also cannot establish who is
allowed to act on a nearby person's account. Those remain Voice/product rules.

## Local state transitions

1. `prepare` commits the exact text, operator target, unique task ID and label,
   payload digest, current boot/service session, and a 120-second expiry.
   No Harness command runs. Voice obtains that immutable content with
   `get_task` before requesting confirmation.
2. Trusted `confirm` must name that ID, digest and current service session.
   It commits `queued` and returns promptly without waiting for Harness.
   The storage owner commits `attempting` before giving one job to the CLI
   worker. That durable transition is the only launch grant.
3. The worker preflights the full machine ID, invokes `new` at most once, and
   the owner commits its outcome. `accepted` records a validated agent ID;
   `pending` retains an unresolved creation ID; `not_sent` records a proven
   pre-launch failure; `uncertain` records a possible launch without proof.
4. `observe` is explicit and only available for an accepted exact agent ID.
   One bounded `show` supplies an observation. The owner commits its state,
   ask/answer, truncation flag, tool count, fingerprint and local audit time.
   Repeated identical content updates seen metadata without a second record
   or speech claim. Different same-length answers remain different content.
5. Trusted `claim_speech` commits one exact text and digest against one stored
   observation before returning a playback grant. A duplicate claim returns
   `claim_uncertain` without another grant, even if only Voice restarted.
   Only an exact qualified `ack_speech` records `reported_spoken:true`.

The launch label is `lamp-<store-id>-<task-id>`; it is a diagnostic hint, not
a Harness idempotency key. A receipt with an already-bound agent or creation
ID on a different task becomes uncertain. The adapter validates the expected
machine, label, engine and ask-mode receipt before accepting a new binding.

At startup, old prepared/queued tasks become `invalidated`; they are never
automatically dispatched. An interrupted `attempting` record becomes
`uncertain`. An interrupted observation becomes a failed read, without a new
launch. An outstanding speech claim becomes uncertain. Accepted, pending,
uncertain and reported-spoken history remains. Old confirmations and speech
attestations cannot cross the newly generated service session.

`cancel` only cancels a prepared or queued local task before its attempt.
It returns `remote_cancelled:false`. There is no remote cancel, close, resend,
follow-up, permission reply, automatic observation retry, or automatic polling.
A person can explicitly request a new observation of an accepted agent.

## Wire interface

Each Unix connection carries one UTF-8 JSON object followed by a newline and
one JSON response followed by a newline. Requests reject unknown and duplicate
fields. Success includes `ok`, `store_id`, `revision` and `completion`;
failure includes `ok:false`, a stable `error` code and a bounded explanation.
Prompts, transcripts, raw CLI stderr and account data are excluded from logs
and bus diagnostics.

Every mutation carries a compare-and-set guard:

```json
{"store_id":"0123456789abcdef0123456789abcdef","revision":3,"request_id":"voice-turn-42-prepare"}
```

Read `status` to obtain the actual store identity, revision and
`summary.session_id`. Generate a new request ID for each intended mutation.
The exact most recent guarded request has a durable receipt; ordinary exact
retries return that receipt without executing again. Claim retries are the
exception above. Internal dispatch/read transitions can advance the current
revision while the original mutation receipt keeps its original revision.
Inspect state after a conflict; never substitute a new guard just to force an
ambiguous mutation through. `status.last_receipt` strips speech text and is
inspection evidence, not a playback grant.

| Operation | Fields besides `op` | Route / meaning |
|---|---|---|
| `status` | none | Counts, bounds, current identities, last receipt |
| `targets` | none | Each target's `alias` and `engine`, in alias order; never its machine or project (since task 41, for the voice) |
| `list` | optional `after_id` (default 0), `limit` (1–8, default 8) | Metadata page; follow `next_after_id` |
| `get_task` | `id` | Exact task, configured target, dispatch and observation metadata |
| `get_observation` | `id`, `observation_id` | One stored observation plus its speech record |
| `get_progress` | `id` | With `serve --progress` only: the agent's progress line as last read, and what was read with it (["Progress on the bus"](#progress-on-the-bus-serve---progress-732)) |
| `prepare` | `guard`, `text`, `target` alias | Immutable proposal; no CLI call |
| `confirm` | `guard`, `id`, `prepared_digest`, `session_id` | Trusted person's spoken confirmation |
| `cancel` | `guard`, `id` | Only before dispatch attempt |
| `observe` | `guard`, `id` | Explicit `show` of the bound accepted agent |
| `claim_speech` | `guard`, `id`, `observation_id`, `text`, `session_id` | Trusted one-time playback grant |
| `ack_speech` | `guard`, `id`, `observation_id`, `claim_id`, `text_digest`, `session_id`, `played:true` | Trusted completed-playout attestation |
| `told` | `guard`, `id`, `observation_id`, `session_id` | Trusted: the voice's news of this observation was heard whole, in its model's own words ([#1390](#closing-a-finished-harness-1390-serve---close-told-off-unless-set)); once only, `already_told` after. Without `--close-told`, `told:false` and nothing written |

The [protocol](../crates/tasks/src/protocol.rs) and
[engine](../crates/tasks/src/engine.rs) define the serialized fields and error
codes. Query the local service without a separate scripting runtime:

```sh
printf '%s\n' '{"op":"status"}' | ./lamp-tasks request --socket /run/lampOS/tasks/request.sock
```

The reference client has a 1.5-second round-trip deadline. Timeout is uncertain
for a mutation. It is not evidence that no commit or dispatch occurred.

## Harness adapter

Only six argv forms are constructed, without a shell; `ps` only with
`serve --progress` and `wait` only with `serve --push-wait`:

```text
harness machines --json
harness new codex --machine <full-machine-id> <project> --name <unique-label> --mode auto --json --task <exact-text>
harness show --machine <full-machine-id> <full-agent-id> --chars 4000 --json
harness ps --machine <full-machine-id> --json
harness wait --machine <full-machine-id> <agent-id>[,<agent-id>…] --any --timeout 600s --json
harness close --machine <full-machine-id> <full-agent-id> --json
```

`--mode auto` (Harness's default) since 2026-10-15, approved by the owner: the
agent works in its own project folder without asking and still stops for anything
risky. With `--mode ask` Claude Code asked before every file it wrote, with nobody
at the computer to answer: on lamp-4ace a two-line poem waited on "Do you want to
create coffee_poem.txt?", which Harness reports as state `unknown`, while the lamp
said "still working" (#770 makes the lamp say when an agent waits for the person).

A launch counts as started only when Harness's reply names the agent with
`permissionMode: "auto"` and a `bypassPermission` of true, false or null;
Harness 0.4.6 reports auto mode as `"auto"` with `true`. Any other mode, `full`
or `ask` among them, leaves the task `uncertain` with `invalid_reply`. Before
this fix (2026-10-15) the check still demanded `ask`, so every auto-mode launch on
lamp-4ace was refused after Harness had started it, and the lamp said it had
not gone through.

The machine is named with `--machine` since Harness 0.4.2 (openharness #1333,
2026-10-14), on `new` and on `show`, and `show` reads the agent on the machine
the task was started on. The `@<id>` form before it is refused ("Name the
machine with --machine <id>."), so a `lamp-tasks` build from before that
change fails every launch with `new_failed`.

The engine allowlist is `codex`, `claude` or `opencode` (OpenCode since task
41: Harness 0.4.2's `harness new --help` names it, and its ask mode passes
OpenCode no flag; see "Who gets the work"). A project is an explicit name
(at most 128 UTF-8 bytes) or an absolute/`~/` folder (at most 512 bytes).
Leading option/target syntax, ambiguous relative folders, control characters
and whitespace around a project are refused. Text is at most 1,000 UTF-8
bytes, nonempty, without NUL and not beginning with `--`, which the inspected
CLI parser treats as another option. The confirmed text is never rewritten.

The executable is resolved once to an absolute canonical regular file, owned
by the service user or root, executable, without group/other write or set-ID
bits. Its identity, size, modification time, mode and owner are checked again
before each invocation. Replacing it requires a service restart. This is not
a signature check or a sandbox for an untrusted executable.

The child gets `AI_AGENT=lampos` and `HARNESS_AGENT_SESSION=<persistent store
ID>`. It receives only explicitly allowed ordinary provisioning variables:
`HOME`, `PATH`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TMPDIR`, `TMP`, `TEMP`, `USER`,
`LOGNAME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`,
`XDG_RUNTIME_DIR`, `ADAPTER_DATA_DIR`, `HARNESS_AUTH_DIR`, `PORT`. `HOME` is
absolute and every PATH entry is absolute. No inherited pane, Claude or Codex
identity, provider key or shell startup environment is forwarded. Tasks never
opens Harness credential/ledger files; Harness itself owns their use.
Each forwarded value is at most 4 KiB, the environment totals at most 32 KiB,
and missing HOME/PATH is refused rather than filled from a different account.
Child stdin is null. Machine preflight accepts at most 256 unique full IDs
with typed `current` values. Oversized observed ask/answer fields are refused;
accepted text retains Harness's `truncated` flag without silent local trimming.

Machine preflight is NDJSON and must contain the exact configured full ID.
The current CLI still has a strict-target gap: if that ID disappears before
`new` re-fetches machines, its resolver can fall back to a matching label.
Returned-ID checking detects the mistake after possible launch. This client
cannot claim atomic exact-machine selection; see the owner qualification list.

Once a `new` process was spawned, timeout, nonzero exit, malformed output,
wrong identity, same-name `existing` heuristic or incomplete cleanup is
uncertain. Human error text is never parsed as proof of non-dispatch. A pending
creation is retained without guessing an agent from its name. The CLI's
creation-ID lookup gap prevents reliable automatic reconciliation.

## Speech and questions

An observation can be read at any later time; it can receive a new speech
claim only within ten minutes of its original observation and within the same
Tasks service session. Re-observing identical old content does not manufacture
freshness or reset its once-only speech history. A claim is not a statement
that audio was played. `ack_speech` records the trusted caller's report of
qualified completed playout, not independent acoustic proof or human hearing.

First must bind a claim to Voice's own durable output identity and check
privacy, active turn, current output authority, interruption, neither listening
nor speaking, and at least 1.5 seconds of quiet at the final output boundary.
No socket receipt, transcript, model admission, generated text or queued PCM
can stand in for that boundary. A lost or interrupted claim remains uncertain;
the component deliberately permits omission rather than automatic repeat.

`needs-input` is only an observed state. The CLI does not supply a stable
permission-dialog ID or type, so this component cannot implement V1's
once-per-dialog permission announcement. If requested, Voice can truthfully
say the agent needs attention in Harness. It must not label every such state
a permission request or approve it by voice. Task output is untrusted data,
not authority to call another tool or override these rules.

Probed 2026-10-15 (#770) on a Harness machine (CLI and daemon 0.4.6,
`--mode auto`, Claude Code): risky actions are not prompted but denied
(Claude Code's auto-mode classifier, e.g. writing in `~/.ssh`) or refused by
the model, so a permission prompt no longer stops a task. What still stops is
a question: `show --json` then says `state: "needs-input"` (about 14 s, until
answered) with no question text (`answer` empty or the previous answer), and
`wait --timeout 1s` exited 124 throughout, never 3. So `needs-input` is taken
from `show`, and the voice says the agent is waiting on the person's computer
rather than voicing a question it does not have.

## Closing a finished harness (#1390, `serve --close-told`, off unless set)

Each hand-over starts a Harness session (a Claude Code process) on the
person's computer, and until 17 Oct nothing closed it. The office Mac had 52
idle `lamp-*` harnesses from three days of tests, holding part of 17 GB of
Claude processes. A customer's computer would keep one for every job the lamp
ever handed over. With `serve --close-told`, the lamp closes the harness it
started once its answer is told:

- **When:** the task was made by this version, and its newest kept answer is
  from an agent `idle` with an answer, and that answer was told:
  - **Heard from the voice's news** (`told`): the voice's task news tells it
    as a turn its model says in its own words, so it claims no exact text.
    The turn carries its news keys (the task's, and those told with it).
    As it ends heard whole, on the proof a skill's say takes for `spoken`
    (settled and complete, its audio played with no gap, the mic allowed),
    the voice sends `told` for each on the trusted socket. A turn cut short
    (a Stop, the person speaking over it, muting) or ended with no audio or
    a gap sends nothing, and the harness stays open. The news is still kept
    as told when its turn begins, so it is never told twice.
    - **Sent off the coordinator's loop,** on a thread of the news's own.
      If the tasks part can't be reached (restarting, say), it's sent again
      every 10 s, for an hour.
    - **Not sent:** hand-over and progress news, of no observation; with
      `--close-told` off, `told` is answered `told:false` and writes nothing,
      so the revision a tool's guard read still holds.
    - **Not covered yet:** a finished answer the person only asked about
      (`check_agent_task`). Its words go to the model from the provider
      worker, another process, and its turn isn't known there. Such a task
      is kept as told, so it isn't told unasked either, and its harness
      stays open.
    - Before this, on lamp-4ace on 18 Oct, task 147 was told and never
      closed: the news path never said so.
  - **Or by a speech claim:** its telling played, or isn't known to have
    (`spoken` or `uncertain`). One still `claimed` is being played: not yet.
  - **`ask` doesn't count:** Harness's `ask` is what the agent was last
    asked, the lamp's own task text, so it plays no part here.
  - **Left to the person:** an agent that waits for them (`needs-input`).
    The lamp can't answer it, and Harness would refuse to close it.
  - **Old tasks:** only a task made while `--close-told` is on is marked
    `closable`. A task from a store written before this version, or made
    while the switch was off, is never closed, so turning it on later closes
    nothing older. Its harness may be one the person has gone on with since.
  - **The lamp's own turn only:** the told turn's asked text must still be
    the task's own, the `ask` of its first kept answer. If the person asked
    the agent more at the computer ("Now make it longer.") and that newer
    turn was kept and told, it's theirs, and never closed.
  - **No "go on" from the lamp:** no request lets the person ask the lamp to
    go on with a task, so a task told is a task done, from the lamp's side.
- **Read first, every time:** the close job reads the agent (`show`) just
  before closing.
  - **Still idle on the told turn** (the same answer to the same asked
    text): closed with `harness close --machine <id> <agent-id> --json`, by
    the agent's own id, never `--now`. Harness saves its screen and stops it,
    and keeps its conversation, so the person can still open and resume it.
  - **Any other turn or state:** the person went on with it at the computer,
    so it's theirs. It's left for good (`left`), never asked again.
  - **Stopped or failed already:** nothing to close (`closed`, code `gone`).
- **Only the lamp's own:** as an agent caller, Harness refuses one the caller
  didn't start (exit 5). Called as the person (`--harness-as-person`, as the
  product unit and lamp-4ace run it), Harness doesn't check that, so the
  agent id the lamp kept from its own `new` is the only guard. The lamp never
  closes by name, and never anything `ps` lists that isn't one of its tasks.
- **Recorded first:** the attempt is saved before the call, like a launch and
  a read.
- **Exit codes:** Harness 0.4.18's `close` exits 0 (closed), 5 (refused:
  `NOT_YOURS`, `SPAWN_LIMIT` or `TURNED_OFF`, never asked again) or 1. Exit 1
  covers working or waiting for the person (`WORKING`), changed while closing
  (`AGENT_CHANGED`) and gone alike, with only a message in the `--json`
  reply. So exit 1 is `failed`: tried again a minute later (`CLOSE_RETRY_MS`),
  three times in all (`CLOSE_TRIES`), each try reading the agent first.
- **Restarts:** a restart during a close leaves it `failed`, code `restart`,
  to be tried again the same way.
- **The journal:** each close prints one line, `tasks: closing task N's
  harness: …`, and `get_task` shows the task's `close`.
- **Reverting:** a `Task` is `deny_unknown_fields`, so a `lamp-tasks` from
  before this can't load a store with `close` or `closable` in it, nor one
  from before `told` a store with an observation `told` (written only with
  `--close-told` on). Going back
  on a lamp means resetting its tasks store, as with `watch_idle`.
- **Not done:** the sessions already on a computer from before this are the
  person's to close. This touches none of them.

**Tests:**
- **`engine::tests`:**
  - `a_harness_done_and_told_is_closed_once`;
  - `news_the_voice_told_in_its_own_words_closes_the_harness` (`told` on
    the trusted socket only, hours after the read, once; with the switch
    off, nothing written);
  - `a_harness_the_person_has_is_left_to_them` (`needs-input`, `left`, and a
    task from an older store);
  - `a_turn_the_person_asked_or_a_task_from_before_the_switch_is_never_closed`;
  - `a_failed_close_is_tried_three_times_and_a_refused_one_never`;
  - `a_close_under_way_at_a_restart_is_tried_again`.
- **`tests/runtime.rs`, through the service:**
  - `a_harness_done_and_told_is_closed_once_by_the_service` (the person's
    own idle harness listed by `ps` beside it is never closed);
  - `without_close_told_no_harness_is_closed`;
  - `a_harness_the_person_went_on_with_or_that_waits_for_them_is_left`.
- **`tests/harness.rs`, on the fixture CLI:**
  `close_names_the_agent_on_its_machine_never_now_and_reads_the_exit_code`.
- **The voice's `coordinator::tests::say`, the turn's proof:**
  `news_heard_whole_is_handed_on_as_the_turn_ends`,
  `news_cut_short_is_never_handed_on`,
  `news_not_heard_whole_is_never_handed_on` (no audio, a playback gap).
- **The voice's `tests/news_told.rs`, with a real `lamp-tasks` on its
  sockets:**
  - `news_heard_whole_closes_the_harness_it_was_of` (news taken to tell
    closes nothing, with the clock past three close retries);
  - `news_heard_while_the_tasks_part_is_down_is_told_once_it_is_back`.

**Against a real Harness (0.4.18), 18 Oct:**
- **lamp-4ace, by voice:** task 147 was told, and nothing closed: the news
  path never said it was told. `told` is that fix.
- **A laptop, as the person:** a task told and read idle on its turn wasn't
  closed. `harness close` exited 1 (`WORKING`) twice, six minutes apart,
  while `show`, `ps` and `wait` all said `idle`. Harness's close asks its
  daemon's own activity check (the session's busy flag and screen), not the
  transcript `wait` reads. Until that's settled with the Harness team, such
  a close fails three times and stops there.

## Bounds, durability and health

| Resource | Bound / behavior |
|---|---|
| Retained tasks | 16. When full, the oldest task that is over (never sent, expired, cancelled, or its agent finished, stopped or failed) gives way to the new one; only when all 16 are still going (waiting for a yes, being handed over, working, or asking the person) is preparation refused. Until 2026-10-15 a full store refused every new task for good: lamp-4ace stopped handing work over at its sixteenth |
| Observations | 4 distinct records per task; no silent eviction of delivery evidence |
| Text | Task and selected speech: 1,000 bytes each; observed ask: 4 KiB; answer: 16 KiB |
| Local transport | 8 KiB request; 128 KiB response, enough for escaped bounded observation text |
| Clients | 4 workers, 4 queued sockets, 16 queued owner requests |
| Client deadlines | 300 ms complete request, 1,100 ms owner receipt, 1,200 ms total server service |
| CLI | One worker, one job/result slot; preflight/read 10 s each, new 60 s |
| CLI capture | 64 KiB stdout, 16 KiB stderr; individual poll waits capped at 10 ms |
| Progress (`--progress`) | One `ps` per machine per watch period, 10 s each; 256 KiB stdout; 256 harnesses a reply; a progress line cut to 2 KiB; memory only, at most 16 tasks |
| Push (`--push-wait`) | One `wait` held per machine with watched tasks, on at most 64 agents, 600 s each (killed at 630 s), beside the CLI worker; the next on a machine no sooner than 3 s after one came back with an end, 30 s after one failed; a pushed task left out until the watch reads it again; 64 KiB stdout |
| Private persistence | 8 MiB snapshot, 32 MiB database, 40 MiB journal; 4,096-byte pages |
| SQLite | DELETE journal, FULL sync, fullfsync requested, 2 MiB cache, 50 ms busy wait, no mmap/WAL |
| Diagnostics | Once a second, with early readiness changes; owner stale after 1,500 ms |

There are seven service threads including main: four clients, the storage
owner and the CLI worker. Clients never run Harness. The CLI worker never
holds the ledger. Main publishes the standard `diagnostics` type as `tasks`,
with readiness, CLI-busy, counts, queue refusals and publication failures.
It publishes no task text and no automatic result delivery topic. A stalled
owner loses readiness even while its process still exists. Missing Tasks
does not inhibit healthy Voice; First owns the “cannot reach agents” fallback.

Since #830 the report also says whether the newest hand-over in this run
reached Harness (`last_handover` in the summary: `handed` when created or
pending there, `failed` when not sent or uncertain, absent before the
first): with `failed`, `tasks` is a warning, "Tasks could not hand the last
task over", where before it stayed well while every hand-over failed (with
no internet, or with Harness not running: lamp-4ace after a power cut on
2026-10-15). The next hand-over that reaches Harness clears it.

The Store locks its private 0700 directory for the owner's lifetime and uses
0600 single-link regular files owned by the effective UID. It rejects linked
final directories/files, unexpected schema/versions, malformed snapshots,
oversized files and WAL sidecars. Initialization publishes without replacing
an existing database and synchronizes the directory. A commit installs memory
only after SQLite commit and directory sync succeed. Any uncertain write
poisons the owner until reopen; unreadable data is never replaced with empty
history. Cooperating owners are excluded; another process with the same UID
is outside this protection boundary.

Linux confirmation/speech ages use BOOTTIME, including suspend. The host
fallback uses MONOTONIC and an explicit test boot identity. Every stored audit
includes boot, service session, milliseconds and revision; these are not UTC
timestamps. Clock reversal or a boot change inside one owner fails closed.
Physical power-loss durability and the lamp's storage latency remain unproven.

Shutdown cancels the CLI, attempts to kill its local process group and reap
the direct child, and waits a bounded interval for workers. Killing the CLI
does not cancel work already held by a daemon or remote machine. Detached or
privileged descendants are not contained by this wrapper. A blocked storage
system call cannot be forcibly ended by a Rust thread. Executable metadata
validation and synchronous process spawning can also block in the kernel:
the subprocess deadline starts after metadata validation, before spawn, and
is checked once capture can run. It is not an interruptible OS-call deadline.
Post-exit pipe draining has a 100 ms allowance within capture; cleanup has a
separate 200 ms budget. Incomplete cleanup poisons that adapter so subsequent
calls refuse without spawning until the process is replaced. If shutdown reports an
unfinished worker, the supervisor must exit that process and verify its death
and child cleanup before replacement. Do not restart an owner inside the same
surviving process. Terminal error reporting waits at most 10 ms for its one
writer before process exit; a blocked log sink cannot hold that error path.

No history garbage collector or remote close/archive operation is implemented.
Capacity stops admission rather than forgetting ambiguous work or resetting
speech history. Account, local Harness identity, operator target, store reset
and personal-data erasure need First's explicit reconciliation/reset wiring.
The private database contains task and answer text; it belongs in that reset
manifest. Deleting the database is not an ordinary retry strategy.

## Host qualification and first lamp check

The crate's tests need the host's `install` command. They copy the scripted
Harness with `install -m 0700`, so that no other test thread's child inherits a
descriptor still open for writing (`ETXTBSY`, #898). They also clear the group
and other write bits on the Cargo-built fixture, which a umask of 0002 leaves
set, before the fixture is used. Both are for the tests only: production still
refuses a group- or world-writable executable, unchanged.

Build the process and the standalone check from the `lampOS/` root:

```sh
cargo build --release --locked -p lamp-tasks --bin lamp-tasks --example tasks_check
```

Copy the resulting executables through First's authorized workflow. First's
first check on a persistent, short, private scratch parent is:

```sh
findmnt -T "$HOME"
./tasks_check --scratch-parent "$HOME" --samples 8
```

`tasks_check` launches its **own executable** as the scripted Harness. It
uses a fresh private store per cycle, no installed Harness, account, network,
credentials, Voice, microphones, light or motors. Each cycle checks public
confirmation refusal, one creation, exact observation, one speech claim,
duplicate-claim refusal, an authored playout receipt, restart preservation,
stable caller identity and no resend. Extra scenarios retain uncertainty after
malformed scripted JSON and unresolved pending creation across restart. They
do not simulate a real network losing a remote receipt. The final JSON
must say `all_assertions_passed:true` and `scratch_removed:true`; failures
retain their private scratch evidence.

Reported milliseconds measure the executing machine's real local SQLite,
socket and scripted-process work. Each boundary includes exactly the steps
named in the output. The first sample and subsequent-sample median/maximum
are separate, with every raw value retained. Stores are fresh per cycle;
“warm” means subsequent process/OS samples, not a claim about a warm production
database. Confirmations, boot identity and played receipts are authored.
These timings do not measure remote execution, speech or physical restart.

The [host evidence ledger](verification/tasks-host-20261014/README.md) retains
every failed run and the final gate: formatting, strict all-target Clippy,
**101 tests passed / 0 failed / 0 ignored**, full workspace build, and a fresh
source-only workspace build plus the same 101 passing tests. One release run
on the Intel i7-10700K Mac (macOS 26.6.2, Rust 1.96.0, APFS) completed eight
cycles and the two uncertainty scenarios in 13.998 seconds. Confirmation
receipts were 132.785 ms first, 82.716 ms median of the next seven; status
beside a silent client was at most 11.671 ms across eight samples. Raw values,
exact boundaries, source hashes and all nine timing rows are in that ledger.

The first likely lamp failures are installed CLI JSON/agent-policy
compatibility, its ordinary environment and executable wrapper, storage sync
latency relative to the 1.1-second receipt deadline, and Linux process-group
cleanup. No host result substitutes for those checks.

## First real Harness check (lamp-4ace, 2026-10-14, 21:36)

By hand, not through `lamp-tasks`, with the owner's leave to use their account
and their office computer. Harness 0.3.99 on the lamp (`~/.local/bin/harness`)
and on the office machine (`office`, `m0.local`); `harness machines --json` on
the lamp listed ten machines, the lamp itself among them ("current").

- **Starting works.** On the lamp, `harness new claude @<office's full id>
  lamp-first-task --mode ask --name lamp-test-1 --json --task "<a text-only
  request>"` returned `ok` with the agent's id, `permissionMode: ask`, and a
  new project `~/harnesses/lamp-first-task` on the office machine, 6.2 s after
  it was run. Claude Code answered with one sentence and used no tools.
- **Reading it back from the lamp does not.** `harness show` and `harness wait`
  read only the agents on the machine they run on: on the lamp, `show` by id,
  by name and with the machine named in every form tried said
  `UNKNOWN_HARNESS` ("on this machine"); on the office machine `show
  lamp-test-1` gave the asked text, the answer, `state: idle` and `tools: 0`.
  So `lamp-tasks` can start work on another machine and cannot read its
  answer there. Asked of the owner, for the Harness team: `show`/`wait` for an
  agent on another machine of the same account. The alternative is to run the
  agents on the lamp itself, which needs Claude Code installed and signed in
  there.
- **Reading it back works since Harness 0.4.2** (installed on the lamp by
  its own update, 2026-10-14 evening). On the lamp, `harness show --machine
  office lamp-8b18c0ad6a1fe15fc84148fb81446b16 --json` gave the 22:39 task's
  ask, its answer (three names, 331 characters), `state: idle`, `tools: 0`
  and `startedBy: person`: 2.8 s once, 4.4 s at 23:51:39. The machine's full
  id and the agent's id in place of the two names gave the same. `harness ps --machine office` lists
  that machine's sessions. The old `@<id>` form is refused.
- An agent may close only what it started: `harness close lamp-test-1` from the
  office machine was refused (`NOT_YOURS`); it stays for the person to close.
- **Through `lamp-tasks`, 21:40, a trial by hand** (the CI build `377d975b…`
  in a private folder on the lamp, `serve --enable --run-for-ms 900000`, not a
  service; target `office`, engine `claude`, project `lamp-tasks`): `prepare`
  and the voice route's `confirm` both answered `ok`; 8 ms after the confirm
  it ran `harness new`, which failed in 3 s, and the task was kept
  `uncertain` with `new_failed`, as designed. The same call by hand with the
  adapter's environment says why: `"Agents may not start harnesses: the
  person turns on \"Agents can start other agents\" in Harness Settings ▸
  Experimental, or runs harness settings agents-start-agents on."` The
  adapter marks itself as an agent (`AI_AGENT=lampos`), so that setting is
  the owner's to turn on; the earlier call worked because it carried no such
  mark. Not worked around.

## The voice's side (first builder, 2026-10-14 evening)

`crates/voice/src/agent_tasks.rs`, off unless the voice's unit sets
`LAMP_TASKS_DIR` (the directory holding `request.sock` and `voice.sock`;
`LAMP_TASKS_TARGET` names the alias, `office` unless set). On a home lamp
without Harness the tasks part is not started (its unit's
`ConditionPathExists`, #811). With `LAMP_TASKS_NEEDS=/usr/local/bin/harness`
(the product voice unit; `--tasks-needs` to the provider worker), a call that
cannot reach the part while that file is missing is answered in light words
the model can say, "I can't hand work on to another computer yet: that isn't
set up on this lamp." (`agent_tasks::NOT_SET_UP`). In every other case it is
told as a part not running: a missing socket alone proves nothing, since
systemd removes the part's runtime folder whenever it stops, a crash
included (the reviewer on #1063). Without the setting, always the latter. The model is told
of two functions:

- `start_agent_task(task)`: for computer work the person asks to have done
  on the office computer. The voice reads `status` and sends `prepare` on the
  request socket with the target alias. Nothing runs. The model is told to
  say the task back in one short sentence and ask whether to start it.
- `confirm_agent_task()`: the voice, not the model, decides whether it may.
  Only when a task is waiting, in a request after the one that prepared it,
  which the lamp heard rather than wrote itself (`attention::own_request()`), within two
  minutes of preparing it. Then `status` again and `confirm` on the voice
  socket with that store's guard and session. The model is told it has
  started and that the answer will be on that computer.

A request whose answer is held back (the busy-room rule) has neither run, as
for the other functions. Harness can now be read on the other machine
(above, since 0.4.2); the voice does not yet ask `lamp-tasks` to read the
answer and say it.

**On lamp-4ace since 2026-10-14, 22:24** (voice `c2ac775f5`,
`LAMP_TASKS_DIR=/run/lampos-tasks`; `lamp-tasks.service` with the build of
`b73dfe6fa`, `--harness-as-person`, target `office`, Claude Code, project
`lamp-tasks`; its target list is in `/var/lib/lampos-tasks/targets.json`, as
the service user cannot enter `/etc/lampOS`, which at first left it
restarting with "Permission denied"). Played at 22:39: "Hey Lamp, can you
ask Claude Code on my office computer to suggest three friendly names for a
desk lamp?" got "I'll ask the code to find three friendly names for a desk
lamp. Shall I start that?" (`prepare` answered in 35 ms); "Yes, go ahead."
got "I've started that task on your office computer." (`confirm` in 38 ms),
and the tasks part started a Claude Code session on the office machine,
which answered there with three names. The answer was not heard on the lamp.

One try before it failed: "Yes, please start it." was ended after "Yes." and
its rest heard as a new utterance; the model's `confirm_agent_task` for the
first arrived after the second had taken over and was dropped
(`UnownedToolCall`), and the model then said it could not start the task. A
yes said in one breath worked.

### Three ways a hand-over failed on lamp-4ace (first builder, 2026-10-15 evening)

Played between 20:07 and 20:29 with progress news on. The lamp faced the
office iMac's speaker about 60 cm away.

- **The tasks part was not sure, and the voice gave up.** The tasks part
  waits 1.1 s for its store (`REQUEST_DEADLINE`). If the store takes longer,
  the tasks part answers `{"ok":false,"error":"uncertain"}`, asking for the
  exact request again because it may have been done. Twice, within two
  minutes of a restart on the lamp (the tasks part's at 20:06, the Harness
  daemon's at 20:19), `start_agent_task` was answered so after 1,107 and
  1,110 ms. The lamp said the work "couldn't be prepared", and the yes then
  found nothing waiting, though the store held both tasks as prepared. Every
  other hand-over that day was answered in 36 to 106 ms. Now `prepare` and
  `confirm` are sent again, the very same line, while the answer is
  `uncertain`, `busy` or `timeout` and there is time left. Calls to the
  tasks part may take 3.5 s in all (`agent_tasks::DEADLINE`), where the
  other parts get 1.5 s. A refusal of any other kind is told to the model
  as before, and a journal line gives its code, never the task's words.
- **A call made before the request was whole.** "…learns to sing, in five
  chapters…" was ended at its comma after 400 ms. The call the model made
  for the first piece was answered as not done (`UnownedToolCall`, above).
  The model called again for the whole and the task was prepared, but it
  spoke of the first answer: "That's strange, the work couldn't be prepared
  right now." That answer is now sent under `not_done`, not `error`, and
  says that nothing went wrong and to say nothing about it
  ([gemini-session-reliability.md](gemini-session-reliability.md)). Played
  at 20:43 on a build with both changes (voice `64c62377…`), the request
  with a 600 ms pause after its comma: cut there both times. In the first
  the call for the piece was answered `not_done`; the model called again
  for the whole and said "Claude Code will write a two-line poem about the
  sea and review it. Shall I start it?", nothing about a failure. In the
  second it made no call for the piece. On the same build at 20:36 a
  hand-over was prepared in 37 ms and confirmed in 42 ms, and its news was
  told unasked 80 s later.
- **Harness was not running after a power cut.** The tasks part hands work
  over through the Harness daemon on the lamp. Nothing started it after the
  power cut at 18:15, so a confirmed hand-over at 20:10 ended `new_failed`
  ("Harness is not running here") after the lamp had said it was handing
  it over. The failed-hand-over news then told the person. Since 20:19 the
  daemon runs as a systemd user unit (`harness service install`), with
  lingering on for `orangepi` so that it starts at boot; the install is
  asked to do the same (#811).

Also seen: the service's transcript of "Claude Code" in the synthetic voices
was often wrong ("Claude Poe", "your go", "George Jones"). Once the lamp
answered as if asked for Codex, and once it took the request as the person's
own and told the story itself. Twice after that, in the same session, it went
on telling the story when asked again.

## The lamp as chief of staff: watching and telling (2026-10-15)

The owner, just after midnight: the lamp is the person's chief of staff at
work and their best friend. It hands work to Claude Code, Codex and the
others like an assistant hands work to staff ("I gave it to Claude Code to
work on it"), and later, unasked and at a good moment, says how it went ("oh
Claude Code just finished the task on xyz. the result is xyz"), "but must be
natural". Watching and deciding cost no model tokens; the realtime model is
used only to say it.

- **Handing over.** After the yes, the model is told to say in a few natural
  words that Claude Code has it and that the lamp will say when it is done.
- **Watching** (`Engine::watch`; on unless `serve --no-watch`). Each accepted task is read
  on its own, with the same `show --machine` as an asked read: every 30 s for
  its first 10 minutes, then every 2 minutes, for at most a day in one boot.
  A read made by the watch keeps an observation only when the agent has
  finished (idle with an answer), stopped, failed, or stopped to ask
  (`needs-input`); otherwise it ends as a failed read with code `working`,
  so the four observations a task may keep are not spent on work in
  progress. An idle answer is kept only when the next look finds the same
  one, as Harness's own `wait` takes two looks at one turn for done: the
  first ends as a failed read with code `idle_once`, the task remembers it
  (`watch_idle`, kept across a restart), and the next look comes 10 s later
  (`WATCH_CONFIRM_MS`) instead of on the usual schedule. A look that finds
  it at work again, or a different answer, starts over. An agent can end a
  turn while it waits on its own background work and then go on: on
  lamp-75c9 on 2026-10-16, a task that ran `sleep 25` in the background read
  idle → working → idle on one line before it was done. So "done" is told
  about 10 s after the agent goes idle. A question (`needs-input`), a stop
  or a failure is kept on the first look. A wait longer than the second
  look still reads idle twice, and neither `show` nor Harness's own `wait`
  (two looks about 3 s apart) can tell, so after an idle answer is kept the
  watch goes on looking every 2 minutes for 10 (`WATCH_FOLLOW_MS`). The same
  answer is only seen again; a different one, confirmed the same way, is
  kept as a new observation, and the voice tells it as news of its own:
  Harness gives that answer with the earlier one at its front ("…once it
  finishes.done" on lamp-75c9), so when it begins with the task's earlier
  idle answer, the voice is told only what comes after (`went_on` in
  `task_news.rs`). A
  question, a stop or a failure kept, or a task's four observations, end
  it; what progress reads (`tasks/status`) is unchanged. Otherwise, once
  such an observation is kept, the watch leaves that task.
  An asked read's receipt that a watch read replaces is dropped: a retry of
  that request finds the revision moved on and is refused, never run twice.
  Nothing is read while a launch or another read is in hand.
- **The push** (`serve --push-wait`, #560: the first builder's bound is
  that a person at the lamp hears a result within 20 s of the agent
  settling, and a look every 30 s, then every 2 minutes, cannot meet it).
  For each machine with tasks the watch looks at, one `harness wait
  --machine M <agents> --any --json` is held on a lane of its own: it never
  takes the CLI worker's one-at-a-time lane, so hand-overs and reads go on
  beside it. Harness looks every 3 s and calls an agent done on two looks
  at one turn (Harness 0.4.17), so it comes back within about 6 s of the
  agent ending (exit 0 idle, 3 waiting for the person, 4 stopped or gone;
  124 when its 600 s pass, and it is started again). The task of an agent
  it names is read at once (`Engine::push`), and that read keeps an idle
  answer on the spot: `wait`'s two looks stand for the watch's second
  look. A task whose idle answer is kept is no longer watched, so it is
  not waited on again; its follow-up stays on the watch's 2-minute looks,
  since `wait` on an idle agent comes back at once. When the set of watched
  agents on a machine changes, its `wait` is cancelled and started on the
  new set; each `wait` is numbered, so a cancelled one that answers late is
  never taken for the one that replaced it. A failed `wait` is tried again
  after 30 s, and the watch's own looks go on meanwhile as the backstop. A
  task pushed and not yet read, or whose latest read was a push
  (`Engine::pushed`), is left out of `wait` until the watch's own next read
  of it: an agent Harness keeps calling done while `show` finds it at work
  (a task never typed, or an agent removed) costs two reads a watch period
  (the watch's own and the push it brings back) and one `wait`, and never
  holds up the other tasks on its machine. An agent a push read finds at
  work after Harness called it idle is left out the same way, so its real
  end is then known at the watch's pace (up to 2 minutes after its first
  10); Harness's two-look rule makes that rare (the reviewer on #1157). A push read leaves
  `"confirmed": true` in the store, which a `lamp-tasks` built before
  `--push-wait` refuses, as it refused `watch` when that came: going back
  needs the tasks store removed. The product unit
  (`deploy/product/lamp-tasks.service`) runs `serve --enable --watch
  --progress --push-wait …` (lamp-4ace runs the same by its own record, since
  stack38) since #1157 was shown on lamp-75c9 (Ten, 2026-10-16, with the real
  Harness and the lamp's own Harness daemon running): from Harness's `idle`
  to `tasks/status` saying `settled`, 8.3 to 10.2 s over five tasks, against
  38.9 to 41.2 s without; the whole lamp 12.3 % of its cores busy against
  11.2 %, `lamp-tasks` about 4 % of one core, and the held `wait` one Node
  process of about 130 MB while a task is watched. One line in the journal
  for each `wait` that comes back or fails.
- **Telling** (`crates/voice/src/task_news.rs`). A thread of the voice reads
  `list`, `get_task` and `get_observation` every 10 s, and at once when
  `tasks/status` lists a task newly settled (#560: the tasks part publishes
  it the moment it keeps the answer, so the news is not up to 10 s late; a
  task stays listed for 2 minutes and wakes the reader once, by store, task
  and when it settled). That needs the bus and `lamp-tasks serve
  --progress`, which publishes the topic; without either it reads every
  10 s as before, and says so in the journal if the bus cannot be had. It
  offers the newest
  settled observation of each task, if it was seen in this boot within the
  last 12 hours and has not been told. The coordinator tells it as a turn the
  lamp begins itself (basis `lamp_tells_news`, record `task_news`) when the
  eyes say someone is at the lamp, or something was said to or by it within
  two minutes; nothing has been said for 15 s; nothing is owed or being
  heard; and the room is not busy. With `LAMP_TASK_NEWS_AT_PAUSE=on` (#560,
  off unless set) the 15 s is the first pause of a second instead: nobody
  heard speaking for 1 s and the lamp's last turn over for as long, as a
  due reminder waits (`task_news::quiet_for_news`, `PAUSE_US`). After the
  lamp's own words it waits 3 s with nobody starting (`AFTER_LAMP_US`): 1 s
  after the lamp stops is the person's turn to answer it, and someone who
  starts then has their line, the news waiting for its end and 1 s more
  (the first builder on #1161). Speech heard in the first 0.3 s after the
  lamp stops is not someone starting: the detector fires on the lamp's own
  voice while the microphone falls back to its floor, and the duplex mask
  waits as long before it hears anyone in the clear (`LAMP_TAIL_US`, the
  reviewer on #1161). A reply that ends on 0.2 s of its own echo still gets
  its 3 s, not 1.2 s; a "yes" that runs past the 0.3 s is the person's. The
  first
  builder's bound on #560 is that a person at the lamp hears the result
  within 20 s of the agent settling, at their first pause of at least 1 s if
  they are talking; someone who talks with the lamp until 18 s after it
  hears it at about 19 s, not 33. With the switch, tasks that finished
  together are told in one turn, at most three, one sentence each from one
  note (`TaskNews::tell_together`, the reviewer on #1161). Told one by one,
  each 3 s after the lamp's words before it, three were told at about 1, 9
  and 17 s, each "Oh, Claude Code just finished…", and a fourth missed the
  20 s bound. One note holds 1,024 bytes, so fewer go together when each
  answer would keep under 120 characters: three long answers go as two,
  about 180 characters of each, then one. How many go together is settled
  before any is taken from the line, so the rest keep their places: one
  left out never goes ahead of a task waiting for the person that came
  before it (the reviewer on #1223). A cut answer keeps #757's guard told
  together too: its gist in one plain line from its beginning, then that
  the rest is on their computer, never only that. A task waiting for the person,
  one that stopped, a hand-over that failed and progress are told on their
  own, and so is all news while it waits for a skill (`LAMP_TASK_NEWS=skill`):
  the skill tells each task in its own run (#1042). The record's `task_news` says `at_pause`, `since_turn_ms`, how long
  after the last turn it was told, and `also`, the news told with it. The note gives the model the task's words
  and up to 1,200 characters of the answer, and asks for one or two natural
  sentences starting like "Oh, Claude Code just finished…" ("earlier" after
  ten minutes), the gist in plain words, no code, lists, paths or links, and
  the rest left on the computer. When the answer is too long for the note,
  the note asks for its gist in one plain line first, from the beginning
  kept, and only then that the rest is on their computer, never only that
  (#757). An agent waiting for the person
  (`needs-input`) is told once, by name, as waiting for them on their
  computer, to be answered there; the model is never asked to voice a
  question the observation does not contain. With no answer text it is told
  not to ask or guess one; with text (which may be the previous answer) it
  may pass it on only if it is a question to the person. The same words
  (`task_news::waiting`) answer `check_agent_task`. A stop is said plainly. Each piece is told once: its key
  (store, task, observation) goes into `/var/lib/lampOS/voice/tasks-told`
  (`LAMP_TASKS_TOLD`), read again on each pass. With `--close-told`,
  once its turn is heard whole, a task's observation is also marked `told`
  in the tasks part, so that its harness can be [closed](#closing-a-finished-harness-1390-serve---close-told-off-unless-set).
- **Asked** (`check_agent_task`). "How's it going?" or "what did Claude Code
  say?" is answered at once from what was last seen, with no new read; a
  finished answer heard this way is kept as told, so it is not told again
  unasked. One the person was already told, unasked or asked, is answered
  with `told_before: true` and asks the model to say it is the one it
  mentioned, giving a gist only if it gave none then, not to tell it again
  as new (#757). A long one asks for the gist in one line first, then where
  the rest is.
- **Progress** is #732: `show` is empty while an agent works (measured
  2026-10-15, 00:05), so the progress signal is `ps --machine`, put on the
  bus as typed state, with Jev deciding whether a step is worth telling
  now, and the model used only to say it: ["Progress on the
  bus"](#progress-on-the-bus-serve---progress-732) below, and the policy's
  [progress question](policy.md#progress-news-say-this-progress-now-732).

The voice's side is on wherever the tasks part is (`LAMP_TASKS_DIR`). The
watch was `--watch` for its first run on lamp-4ace (00:49 on 2026-10-15) and
is the default since; `--no-watch` turns it off (`docs/joining.md`).

### The first run end to end on lamp-4ace (2026-10-15, 00:49 to 00:56)

`lamp-tasks` `45a1e4d2c` with `--watch` and the voice `55e92e3d8`, both
installed at 00:49. Played at 00:50:48: "Hey Lamp, can you ask Claude Code on
my office computer to suggest three names for a cat?" got "Shall I ask Claude
Code to suggest three names for a cat?" (1.75 s after the end of speech);
"Yes, go ahead." got "Done, Claude Code's on it. I'll let you know when it's
finished." (1.82 s). Three faults, all mended the same night:

- **The hand-over failed and the lamp said it had not.** `harness new` ended
  `new_failed`. By hand: since 0.4.2 a project *name* that already exists
  is refused ("A project with that name already exists. Name its folder
  instead."), and the 22:39 task had made `lamp-tasks` on the office Mac.
  lamp-4ace's target is now the folder, `~/harnesses/lamp-tasks` (started by
  hand from the lamp with it at 00:55: `ok` in about 2 s). The voice is told
  that it is handing the work over now, not that it is done, and a hand-over
  the person said yes to that fails is told as news within ten minutes, in
  plain words, with an offer to try again (`task_news::failed_hand_over`).
- **News told to an empty office.** At 00:49:48, 11 s after the restart, the
  lamp told the 22:39 task's names with nobody there: the presence check took
  the start of the service for someone speaking. Now only something a person
  said (an utterance of at least the gate's 30 blocks) in the last two
  minutes, or the eyes, count as someone there.
- The model, given that news, called `check_agent_task` by itself to read
  the answer (the tasks part answered in 19 ms); both kept it as told.

### The whole loop in a played day (2026-10-15, 01:25:42)

With the three faults mended (voice `fd1a273ee` from 01:24:30; the target a
folder), in `~/lamp-voice-data/tools/day.sh`, ten lines played in one voice
at the lamp, nobody in view, the empty office at night:

- "Hey Lamp, can you ask Claude Code on my office computer to write a
  two-line poem about a desk lamp?" → "I can ask Claude to write a two-line
  poem about a desk lamp. Should I go ahead and start?"
- "Yes, go ahead." → "I'm passing that over to Claude Code and will let you
  know when it's finished." (1.84 s after the end of speech)
- 39 s later, unasked: "Oh, Claude Code just finished that poem for you. It
  describes a small bent sun leaning over the page, pouring gold where worries
  disappear." (Claude Code on the office Mac; the watch saw it finished; the
  news was told at a quiet moment because a person had spoken within two
  minutes.)
- "How is that task going?" → "It's finished!", and the gist again.

Every other reply in the day began 1.17 to 1.96 s after the end of speech.
Two things to improve after the MVP: a short piece of writing (a two-line
poem) is described instead of read, and asking right after the news repeats
it.

### The second played day, and a page too long (2026-10-15, 02:54)

The same ten lines on the `lamp-voice` head of 02:53 (`f60d6d6f9`, with
memory across days on). The hand-over went as before ("I'm passing that over
to Claude Code now and will let you know when it's ready."), and Claude Code
answered and the watch kept it, but nothing was told in the 100 s after, and
"How is that task going?" got "I'm sorry, I can't check on that right now."
(`check_agent_task`: "the tasks part did not answer in 3 ms"). The night's
tests had left ten tasks in the store, and a page of eight with their
records is 9,333 bytes, over the voice's 8 KiB bound on a socket answer
(`tools::exchange`, set for the executive); both the news reader and the
check read that page first. Now the tasks part is read with a 64 KiB bound
(`tools::MAX_TASKS_ANSWER_BYTES`), which also covers an agent's longest
answer (16 KiB) with what it was asked (4 KiB). The test of "how is it
going?" now serves a page of over 9,000 bytes.

### Told when a skill asks (#939, (c), off unless `LAMP_TASK_NEWS=skill`)

The Task news skill (Jarvis, #1021) triggers on `tasks/status` and moves and
lights with the news; the voice keeps the words and tells each piece once,
as above. With `LAMP_TASK_NEWS=skill`, news of work finished, stopped or
asking waits for the skill's `from: tasks` ask on the say service, at most
10 s, and is then told with that ask's ticket, or by the voice itself
([speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939)).
An ask that names its task (`"task":10`) is given that task's news only,
so the skill's look and light go with its own task (the reviewer on #1042).
Progress is the Policy's to time already and is told as before.

### News too long for one written turn (2026-10-15, 02:31)

During a two-breath test on lamp-4ace (#740's first try), the voice ended
three times in three, 4 ms after committing news (`telling news from the
work handed over`), with `Gemini Live InvalidConfiguration`, and systemd
restarted it. The likely cause, which the first builder took for the
switch under test: the news is one written turn (`clientContent`), and the
transport refuses a written turn over 1,024 bytes (`wire::MAX_NOTE_BYTES`)
as `InvalidConfiguration`, a refusal the provider worker ends on. The
note's own words are 545 to 585 bytes, and it quoted up to 300
characters of the task and 1,200 of the answer, so any answer of more than
about 400 bytes was over. The poem told at 01:27 was short enough.

Now the note is cut to fit (`task_news::fitted`, the second builder):
the answer first, then the task's words, by whole characters, the answer
then marked "…(it goes on)". Since #757 the task's words are cut to 100
characters before the answer is cut, so that enough of a long answer is
kept for its gist. A note that fits is as it was, byte for byte
(a test); every note, for long answers and long tasks in English and
Vietnamese, in every state, fits and is taken by the transport (a test).
The same bound holds for any written turn: a note made from other text
(progress news, #732) has to fit in it too.

### A lead-in before a call (#799, off unless `LAMP_TOOL_LEAD_IN=on`)

A turn in which the model calls `start_agent_task` or `confirm_agent_task`
began 1.8 to 2.9 s after the end of speech on lamp-4ace on 2026-10-15, against
0.87 to 1.45 s for a plain answer: the model makes the call in one generation
and speaks in another. With `LAMP_TOOL_LEAD_IN=on` the read-back, as a
statement, comes before `start_agent_task`, "giving that to …" before
`confirm_agent_task` and "let me see" before `check_agent_task`, in the same
turn, and each answer's `next` says the person has heard it; the question
whether to start comes from `start_agent_task`'s answer
([tools.md](tools.md#where-a-turn-with-a-function-call-spends-its-time-799),
with what went wrong on lamp-4ace on 2026-10-16: a description too long
for the link left those sessions without any function).
What a prepared or handed-over task is, and what reaches the tasks part, is
unchanged.

### Asked of the staff, done by the lamp (#854, off unless `LAMP_TASKS_ASKED=on`)

On lamp-4ace on 2026-10-15 at 20:15 and 20:16 (one session, synthetic
voice), "Hey Lamp, can you ask Claude Code to write a short story about a lamp
who learns to sing, in five chapters, and to read each chapter over before
starting the next one?" was not handed over: the lamp told the story itself,
and asked again plainly it went on to chapters three and four. In a fresh
session the same words were read back as a hand-over. Once, with "your go"
in the service's transcript, the work was prepared for Codex.

What the model was given: the persona asks for stories to be told gladly
and never mentions the staff; `start_agent_task` was for "computer work …
code, documents or files there", to be called "only when the person asks for
such work to be done there", which a story is easy to think is not; and with
`LAMP_TASKS_WHO=on` its `who` was an enum of the staff's names, so a name not
made out could only be left out or guessed: `chosen()` refuses a name that is
none of the staff, but the enum never let the model send one.

With `LAMP_TASKS_ASKED=on` (the provider worker's `--tasks-asked`, after
`--tasks-lead-in`):

- `start_agent_task` is to be called whenever the person asks to have one of
  the staff do something ("ask Claude Code to …", "get Codex to …"),
  whatever the work, a story, a poem or a plan as much as code, even one the
  lamp could do itself, and even in the middle of a story it is telling;
  never to do the work instead, begin it, or offer to. Work on the office
  computer still counts, as before.
- A new session is told the same after the persona, which asks for stories
  to be told gladly (`agent_tasks::session_note`), on the Live route and on
  the second voice alike.
- With two or more staff, a name the model could not quite make out is
  given as the one it thinks was meant, with no question about it first:
  the task said back names who will do it and waits for a yes, so the person
  can put it right ("No, Codex."). A "not sure" choice that asked "Claude
  Code or Codex?" was tried first: on lamp-4ace at 21:33 the model never took
  it, guessed, and asked about its guess before the read-back as well, so
  twice. The model hears the sound, so a garbled name in the transcript is
  often a name said clearly, and a question every time would cost a turn for
  nothing. A name that is none of the staff is refused as before.
- What is prepared, confirmed and sent to the tasks part is unchanged.

Without the switch every function and session is as before
(`with_work_asked_of_the_staff_every_ask_is_a_hand_over_and_without_it_nothing_changes`);
with it, `who` takes the staff's names as without it, and its description
asks for the name meant, said in the read-back, with no question first
(`a_name_not_made_out_is_named_in_the_read_back_and_asked_about_once`).

**Shown on lamp-4ace** (the first builder, 2026-10-15, `LAMP_TASKS_WHO=on`
and `LAMP_TASKS_ASKED=on`, the voice restarted before each scene; on #854):
at 21:22 to 21:33 the five-chapter story, the comma version, a poem for
Codex and a request in the middle of a story the lamp was telling were each
read back as a hand-over and asked about, while a story asked of the lamp
itself was told and a plain question answered. With the name in the
read-back, at 21:58 and 21:59 a name the lamp could not place ("George
Jones") got one question naming who, and nothing started without a yes.
lamp-4ace keeps the switch on from 21:30.

**Measured without a sound:** `lamp-voice provider-check CONFIG --hand-overs
ROUNDS` writes #854's requests to the model, each in a session of its own,
and counts how often each is handed over (`as_expected`), with the switch
(`LAMP_TASKS_ASKED=on`, the note included) or without it
([tools.md](tools.md#where-a-turn-with-a-function-call-spends-its-time-799)).
The model reads the words there; on the lamp it hears them.

**Reopened: a story asked of the lamp was offered to Claude Code (2026-10-16).**
On lamp-4ace at 02:00 to 02:12, with the switch, "Hey Lamp, tell me a story
about a clever fox, at least eight sentences long." got "Claude Code can write
that story about the clever fox. Should I start it?", 3 of 3, and five-sentence
ones too. It was neither the length nor the build: some sessions offered
every story (9 of 10 in one), others none, and a session that began offering
went on offering. The wording above says to hand over "a story … even one
the lamp could do itself", with no limit on who was asked.

Changed (the second builder, pull request #903): **only the session note.**
After its hand-over sentence, kept word for word, it says:
"A name that comes to you garbled, such as "Claude Co" for Claude Code,
still names them. Work on their office computer is theirs too, named or
not. But a story, a poem or anything else they ask of you that names
neither Claude Code nor work on their office computer, however long, is
yours: tell it yourself, gladly, and never offer it to Claude Code or ask
whether Claude Code should, even after you handed something over earlier in
this conversation." (The exception for work on the office computer, named or
not, is the reviewer's on #903, so that the note never contradicts
`start_agent_task`'s own "call it too when they ask for work to be done on
their office computer".)
`start_agent_task`, its description and what it takes are as they were.

Two heads of #903 that changed `start_agent_task` were played on lamp-4ace
by the first builder, and both lost the hand-overs while telling the stories
4 of 4:
- one began its description "call it **only** when the person names …":
  "Hey Lamp, can you ask Claude Code to write a short poem about the sea?"
  (heard "… Claude Co …") got "I can't write code or use your office
  computer", 2 of 2 (02:43);
- the next gave it two more required arguments, `named` and
  `on_the_computer`, and prepared nothing without either: the model then
  never called it, 0 of 2 (a third, the Codex line, was played while the
  story before it was still being told, so it was never admitted, and does
  not count), and twice said "I've passed that along to them"
  or "I've got that request handled for you" with no call (03:10). A lamp
  that says it handed work over when it did not is the worst of the three.
So the function the hand-overs worked with is left alone, and the stories
are told by the note.

Tests: `with_work_asked_of_the_staff_every_ask_is_a_hand_over_and_without_it_nothing_changes`
(`crates/voice/src/agent_tasks.rs`: the function and what it takes as
before, the note's hand-over sentence as before and the exception after it).
`provider-check --hand-overs` has the two named poems as they were heard
(`named_poem_sea`, `named_poem_rain`), work on the office computer with
nobody named (`computer_unnamed`, to be handed over), and three more of the
lamp's own: the
eight-sentence story (`own_story_long`), the same after a hand-over and its
yes in one session (`own_story_after_hand_over`), and three stories in a row
(`own_stories_in_a_row`). Each request counts `offered`, the times what the
lamp said named Claude Code. Only the last line of a request is recorded
(the lines before it set the scene), so `own_stories_in_a_row` shows whether
a third story is told after two in the same session, not that all three
were; that is `handovers.sh`'s. **The acceptance is the first builder's
`handovers.sh`** on lamp-4ace: one session, three stories and a poem asked
of the lamp interleaved with two "ask Claude Code to …" and one "have Codex
…", each declined; it passes at told 4 of 4, offered 0, handed over 3 of 3
with the read-back. **Not shown yet.** Most likely wrong: the note alone may
not hold the stories in a session that has already handed over, which only
`handovers.sh` shows.

### Named in the words, told in time (off unless `LAMP_STAFF_NOTE=on`)

On lamp-4ace on 2026-10-16 (stack36, `LAMP_TASKS_WHO=on`,
`LAMP_TASKS_ASKED=on`) "Hey Lamp, ask Codex to write a haiku about coffee."
was heard right at 22:51:02, and the lamp said a haiku of its own with no
`start_agent_task` call; at about 21:05 "ask Claude Code to write a short
poem about the sea" got a poem of its own. The session note, read at 22:35:20
before the session opened at 22:35:22, named both of the staff. The model
had what it needed and did the work itself all the same.

With `LAMP_STAFF_NOTE=on` (the provider worker's `--staff-note`, after
`--policy-holds-actions`; only with the tasks part), `crate::staff_note`:

- **What is matched:** the service's transcript of the request whose input
  began last, all its pieces so far each time one comes
  (`staff_note::Heard`), against the staff's close forms, the ones `who`
  already takes (`agent_tasks::named_in_words`):
  - two words said together, anywhere, across a space or a hyphen but
    not a stop or comma: "Claude Code", "code X", "cloud code", "open
    code" ("Save it to the cloud. Code it later." names nobody);
  - one word ("Codex", "Claude", "codecs", "OpenCode") only right after
    "ask", "get" or "have", or after "tell" with "to" after the name: alone,
    "Claude" or "codec" is as likely someone or something else ("tell Claude
    I'm late", "which codec?"; the first builder on #1173);
  - "cloud" alone never ("save it to the cloud").

  Without `LAMP_TASKS_WHO` the staff are not read and only Claude Code is
  matched, since all work goes there.
- **What the model is told** beside the request's sound: "(For you to know,
  not to answer: the person named Codex, one of the staff. If this is work
  for Codex, hand it over with start_agent_task; never do it yourself.)", or
  for more than one, "the person named Claude Code and Codex, of the staff.
  If this is work for them, …". It is conditional where the first builder's
  wording ("this is for Codex; hand it over") was not, because a name alone
  is not always a request: "what does Codex cost?" names it too.
- **A name said later in the request** ("ask Claude Code and get Codex …")
  changes the note before it is sent: the staff's part takes the place of
  the one given (`staff_note_text`), since the newest note for a request is
  the one sent.
- **A request that goes on from the one before** (its Start's `continues`
  or `continues_cut_off`) keeps the words heard so far, and is matched again
  the moment its input begins. "Hey Lamp, ask Codex, … to write a haiku":
  the early end goes at the pause and "ask Codex" is written after it, too
  late for that request. The rest of it, which the model answers as one
  (`provider_worker::CUT_OFF_NOTE`), is told, beside the note that it is
  the rest (`a_request_that_goes_on_from_one_cut_off_is_told_what_the_words_before_named`;
  the reviewer on #1173). After a cut-off the one before was given up and
  took its note with it, so a note given there is given again; after a plain
  `continues` what it was told stands and is not given twice (the first
  builder on #1173).
- **A request begun over the lamp's own answer** (`over_the_lamp`) gets no
  note: its words may be the lamp's read-back ("… to Codex. Shall I start
  it?") with a "yes" said over it, and a note would invite a second start
  instead of the confirm (`a_request_begun_over_the_lamp_gets_no_staff_note`).
- **Before the answer:** the note goes in as the lamp's name note does
  (`crate::called`): `Session::try_note`, sent inside the request's activity,
  after its audio and just before its end. The model answers only after the
  end, so a note given while the request's input is open always reaches it
  first (`a_request_that_names_one_of_the_staff_is_told_so_before_its_end`).
- **Words that come too late:** the service may write its transcript only
  after the end. By `attention.md` the heard text comes 0.32 to 0.44 s before
  the first sound, and the first sound 0.86 to 0.99 s after the end, so the
  words likely come after it (the first builder on #1173). Then nothing is
  sent (`words_that_name_the_staff_after_the_end_send_nothing`).
- **The journal** says which each match was, for counting on a lamp:
  "request N: named Codex in time; the model is told" or "request N: named
  Codex after its input was closed; the model is not told". A request told
  in time whose later words name more says that apart ("named Codex once its
  input was closed, after its note; the model keeps the note it had"), so a
  count sees each request once. Nothing of what
  was said is written: only which of the staff. **Decided** (the first
  builder, #1173): they are counted on lamp-4ace through one battery with
  the switch on. If late is common, the next step is what `second_look`
  does: a first answer that is not a hand-over, for words naming the staff,
  is held and asked again with the note.
- **`start_agent_task` says it too,** with work asked of the staff
  (`LAMP_TASKS_ASKED=on`): its "The person chose who does it: never do the
  work yourself instead, …" becomes "The person chose who does it (a note may
  name them): never do the work yourself, never begin it, nor offer to."
  That is 9 bytes more. The longest description, three of the staff with the
  lead-in, is then 1,014 of the 1,024 the service takes
  (`every_description_fits_the_service_whatever_the_switches`, now with the
  note too). Without the switch it is as it was, word for word
  (`the_description_says_the_staffs_note_names_who_only_with_it`). #903
  showed this description is where a change loses the hand-overs, so the
  change is one clause.
- **The Live route only.** The second voice (`crate::second_provider`)
  parses the same flags and so has the clause in its `start_agent_task`, but
  it never sends the note. The clause says "may", so that is harmless.
- **The switch:** `--staff-note` is taken after `--policy-holds-actions` and
  also sets the tasks part's `Place::staff_note`, which the description reads
  (`the_staff_note_is_read_after_the_policy_holds_and_reaches_the_tasks_part`).

Without the switch nothing is matched or sent, a name in the words or not
(`no_note_without_a_name_or_without_the_switch`); the matching is
`the_words_of_a_request_name_the_staff`. **Not shown on a lamp yet.** Most
likely wrong: the words coming after the end, which the journal's "after its
input was closed" lines count.

### Told twice, and no gist (#757, 2026-10-15)

In the day played on lamp-4ace on 2026-10-15 at 08:02 (review page, Brain
tab, log "08:10"), the first builder found two faults in how news is told:

1. **Told twice.** At 193 s, unasked: "Oh, Claude Code just finished that
   story about the singing desk lamp, and the rest is waiting on your
   computer." At 205 s the person asked "How is that task going?" and heard
   the news again, as if new. `check_agent_task` read the observation and
   kept it as told, but said nothing of it having been told.
2. **No gist.** The story was over a thousand characters. After a task of
   about 300 characters and the note's own 545 bytes, the note kept about
   163 bytes of it, and asked for the gist only in passing ("if there is
   more than fits, say the rest is on their computer"). The model said only
   where the rest was.

Changed (the second builder, `second/news-polish`):

- **Asked about news already told.** Before keeping it, `check` looks for
  the key in the told file. If it is there, the answer carries
  `told_before: true` and its `next` says: "You already told them this. Do
  not tell it again as if it were new: say in a few words that it is done,
  the one you mentioned. If you gave no gist of it then, give it now in one
  plain line; otherwise add nothing unless they ask for more." An agent
  already told to be waiting is said to be still waiting on their computer,
  with no question voiced for it (#770); a stop already told is said again
  without alarm. The answer's words are still given, for a gist. Asked
  twice, the second answer is the short one too. The key is kept once.
- **A long result's note.** When the answer is cut, the idle note is
  another, of the same length: "Its answer is long; it begins: "…" … then
  its gist in one plain line from that beginning (a story: who it is about
  and what happens; code: what it does now), then that the rest is on their
  computer. Never only that." A note that fits is byte for byte as before
  (the pinned test).
- **More of the answer kept.** `fitted` cuts the task's words to 100
  characters before it cuts the answer, then the answer, then the rest of
  the task's words. With a 273-character task and a long story the note
  keeps 364 characters of the story when it has just finished and 324 when
  it finished a while ago, where it kept 190 and 150 before; with a short
  task, 438 and 398 (measured by the test on the Mac). The same order holds
  for a question or a stop.
- **Asked about a long result**, `check`'s `next` asks for the gist in one
  plain spoken line first, then where the rest is, never only that.

Tests: `a_long_result_keeps_enough_for_a_gist_and_asks_for_it_first`
(`crates/voice/src/task_news.rs`: at least 300 characters of the story kept
after a long or a short task, just or a while ago, the gist asked for
before where the rest is, the task's words cut to 100 and no fewer, and the
note taken by the transport), and
`asked_about_news_already_told_the_lamp_says_it_is_the_one_it_mentioned` and
`asked_about_a_long_result_the_gist_comes_before_the_rest_is_on_the_computer`
(`crates/voice/src/agent_tasks.rs`, against a stand-in tasks part).

**Not shown.** What the model says from these notes: no model was asked on
the Mac. The first builder plays the day again on lamp-4ace: a story handed
over by voice, told unasked, then "How is that task going?" a few seconds
later. Expected: the news with a one-line gist and where the rest is, then
a short "it's the one I mentioned". Most likely wrong there: the model still
giving only where the rest is, or announcing the news again despite
`told_before`; both would show in the session record's `function_response`
and the transcript.

## Who gets the work (task 41, #733, 2026-10-15)

The owner: "you are a boss with many employees (claude code, codex, etc) and
lamp assists you in delegating tasks to them". So the person names who gets
the work ("ask Codex to …", "have Claude Code …"), and nobody named is Claude
Code. Off unless the voice's unit sets `LAMP_TASKS_WHO=on` (`--tasks-who`
for the provider worker), for its first minute on lamp-4ace; once that has
passed, it is to become the default, as `docs/joining.md` asks and the issue
says.

**What exists and what was taken.** `lamp-tasks` already took one target per
alias, each with its engine (`claude` or `codex`), 1 to 8 of them; the voice
sent every task to one alias (`office`, or `LAMP_TASKS_TARGET`), and
`task_news` already named Codex from a task's engine. Harness 0.4.2 (the
lamp's version, also on the second builder's Mac, read there with `harness
new --help`, nothing started) takes `claude (default), codex, opencode, …`;
its own launch table (read in the installed `cli.js`, not run) gives
OpenCode `--auto` for auto and no flag for ask, and `--prompt` for the first
message. So OpenCode is now a third engine `lamp-tasks` takes. Whether a real
OpenCode receipt says `permissionMode: auto`, which `lamp-tasks` requires
before it binds a task, is not seen: a receipt that does not leaves the task
`uncertain` (`invalid_reply`) and is never sent again.

**What was built.**

- `lamp-tasks` answers `{"op":"targets"}` on the request socket with each
  target's alias and engine, in alias order, and nothing of its machine or
  project. A read: it runs nothing and writes nothing.
- The voice's provider worker, with the switch, reads that once as it starts
  (waiting at most 300 ms, as for the memory) and again every 30 s on a
  thread of its own, so a tasks part started after the voice, or restarted
  with other targets, is seen by the next session, and opening a session
  waits for nobody. A failed read keeps what was known. The journal says
  `lamp-voice provider: work can go to Claude Code (office-claude), Codex
  (office-codex)` when it changes.
- The staff are the first target of each engine, in alias order (one per
  engine: a second machine with the same engine is not offered yet). With
  two or more, `start_agent_task` takes `who`, an enum of their names, and
  `check_agent_task` takes the same to ask about one of them. The model is
  told the names from the targets, not from a list in the prompt:

  ```text
  start_agent_task: Prepare a piece of computer work for one of the person's agents on their office
    computer (Claude Code and Codex): … then say the task back in one short sentence, with who will
    do it, and ask whether to start it.
  who (enum "Claude Code", "Codex"): Who does it, if the person named one: Claude Code (they may say
    "Claude" or "Claude Code", which can sound like "cloud" or "cloud code"); Codex (they may say
    "Codex", which can sound like "codecs" or "code X"). Leave it out if they named nobody: then
    Claude Code does it.
  ```

- Nobody named is the target the voice was given (`LAMP_TASKS_TARGET`) if
  it is one of the staff, else Claude Code, else the first.
- **"Codex" heard as "codecs", "Claude" as "cloud"** (the issue's most likely
  failure). The enum and its description are the first guard. The second is
  in the voice: a name the model gives is taken by its letters and digits in
  lower case (so "Code X" is "codex"), and only when it is one member's
  name, a close form of it, or its alias, and nobody else's: `claudecode`,
  `claude`, `cloudcode`, `cloud` for Claude Code; `codex`, `codecs`, `codec`
  for Codex; `opencode` for OpenCode. None of these is close to another
  engine's, and a form of an engine that is not among the staff names
  nobody. Anything else ("code", "Cursor") prepares nothing and sends
  nothing to the tasks part; the model is told "Nothing was done: nobody by
  that name works here. Ask the person who should do it: Claude Code or
  Codex."
- The hand-off names the right one: after `prepare` the model is told
  `"where": "Codex on the office computer"` and to say the task back
  "saying Codex will do it"; after the yes, `"who": "Codex"` and "Sure, I'm
  giving that to Codex". The news, a failed hand-over and
  `check_agent_task` name it from the task's engine (`task_news::who`, now
  with OpenCode).

**Without the switch, nothing changes.** Tests show that with it off, or on
with the staff not yet read, or on with one target `office` for Claude Code,
the three functions declared are exactly the ones before (compared with a
copy of the old declaration), and the lines sent to the tasks part (`status`,
`prepare` with target `office`, `status`, `confirm`) and what the model is
told back are exactly the same, even if a `who` is given. The voice sends no
`targets` request without the switch.

**Tests** (host only, the second builder's Mac, Apple M2 Max, macOS 26.6.2,
Rust 1.96.0): in the voice, `agent_tasks` has 14 tests, 9 of them new: the
old declaration without the switch, the same lines and answers without it,
naming Codex, "codecs", "cloud code", nobody and a blank name, names that
are none of the staff, close forms never crossing engines, the staff read
from `targets` (one per engine, unknown engines left out, an older tasks
part that refuses `targets`), the switch's flags, and asking about one of
them. In `lamp-tasks`: `targets` in the protocol, the engine (alias and
engine only, nothing written) and over the socket, and an OpenCode launch's
argv and receipt checks. The gate, from `lampOS/`: `cargo fmt --all
--check`, `cargo clippy -p lamp-tasks -p lamp-voice -p lamp-voice-eval
-p lamp-memory --all-targets -- -D warnings` (`lamp-memory` now tests
against the voice), `cargo test -p <crate> -- --test-threads=1` for each,
and `cargo build --workspace`, on this branch rebased on `lamp-voice`
`1337e7e1d`: `lamp-tasks` 107 passed; `lamp-voice` 1,065 passed, 6 ignored;
`lamp-voice-eval` 284 passed; `lamp-memory` 147 passed, 5 ignored. Three
ring start-up tests far from this change failed in the whole run with the
Mac loaded and passed run alone three times out of three; they are counted
above as passed. Earlier runs failed other ring start-up tests, one
reply-buffer test and one head-motor test the same way, each passing alone.
The copy of the old declaration in the tests is token for token the one
`lamp-voice` declares there.

**lamp-4ace's `targets.json`** (`/var/lib/lampos-tasks/targets.json`, owned
by `orangepi`, mode 0600). `OFFICE_ID` is the office machine's full id, the
one today's `office` target already holds; it is an account's identifier,
so it is not written here:

```json
{"version":1,"targets":{"office-claude":{"machine_id":"OFFICE_ID","project":"~/harnesses/lamp-tasks","engine":"claude"},"office-codex":{"machine_id":"OFFICE_ID","project":"~/harnesses/lamp-tasks-codex","engine":"codex"}}}
```

Each is a folder, not a project name: since Harness 0.4.2 a project name
that already exists is refused (above, "The first run end to end"). Claude
Code keeps today's `~/harnesses/lamp-tasks`. Codex gets a folder of its own,
which step 1 below has Harness make for it (`harness new codex …
lamp-tasks-codex`), so that it is a folder Harness made for Codex, which
Codex should not ask to trust, and the two agents never work in one folder
at once.

**The played run on lamp-4ace** (first builder), smallest step first:

1. **Codex by hand.** On the lamp, as `orangepi`, one text-only task to Codex
   in auto mode, which also makes Codex's folder `~/harnesses/lamp-tasks-codex`
   on the office computer (a new project name, so Harness makes it):

   ```sh
   harness new codex --machine office lamp-tasks-codex --name lamp-codex-check --mode auto --json \
     --task "Reply with one sentence: what is a good name for a desk lamp?"
   ```

   The receipt must say `"engine":"codex"` and `"permissionMode":"auto"`
   (anything else: `lamp-tasks` would keep the task `uncertain`; stop here and
   write down the receipt). After about 30 s, `harness show --machine office
   lamp-codex-check --json` must say `state: idle` with an answer and not
   `needs-input` (the issue's second most likely failure). Then the same once
   more with the folder, as `lamp-tasks` will start it: `harness new codex
   --machine office ~/harnesses/lamp-tasks-codex --name lamp-codex-check-2
   --mode auto --json --task "…"` must also answer `ok` and not ask to trust
   the folder. (With `--machine`, a `~/` path is read under the office
   computer's home.) Codex must be installed and signed in on the office
   computer for this.
2. **The minute with the switch off.** Install this branch's `lamp-tasks` and
   `lamp-voice` from its green ARM64 run, change nothing else, restart both.
   `printf '%s\n' '{"op":"targets"}' | lamp-tasks request --socket
   /run/lampos-tasks/request.sock` answers `office` / `claude`; a task by
   voice goes to `office` as before.
3. **Room in the store.** `printf '%s\n' '{"op":"status"}' | lamp-tasks
   request --socket /run/lampos-tasks/request.sock`: `summary.retained` must
   be 13 or less. The store keeps 16 tasks, every prepared one included, and
   has no clean-up; this run prepares three.
4. **Switch on.** Keep the old file, write the new one with the id it holds,
   and restart the tasks part (its watch is on unless `--no-watch`, so the
   news comes unasked):

   ```sh
   cd /var/lib/lampos-tasks
   sudo cp -p targets.json targets.office.json
   ID=$(sudo grep -o '[0-9a-f]\{32\}' targets.json | head -1)
   printf '{"version":1,"targets":{"office-claude":{"machine_id":"%s","project":"~/harnesses/lamp-tasks","engine":"claude"},"office-codex":{"machine_id":"%s","project":"~/harnesses/lamp-tasks-codex","engine":"codex"}}}\n' "$ID" "$ID" \
     | sudo -u orangepi tee targets.json.new >/dev/null
   sudo chmod 600 targets.json.new && sudo mv targets.json.new targets.json
   sudo systemctl restart lamp-tasks
   printf '%s\n' '{"op":"targets"}' | lamp-tasks request --socket /run/lampos-tasks/request.sock
   ```

   The last line must list `office-claude` / `claude` and `office-codex` /
   `codex`. Then add to `lamp-voice.service`, beside `LAMP_TASKS_DIR`:

   ```text
   Environment=LAMP_TASKS_WHO=on
   Environment=LAMP_TASKS_TARGET=office-claude
   ```

   (`LAMP_TASKS_TARGET` keeps tasks going to Claude Code in the moment before
   the staff are first read), `sudo systemctl daemon-reload && sudo systemctl
   restart lamp-voice`, and look for `work can go to Claude Code
   (office-claude), Codex (office-codex)` in its journal.
5. **One task to each, said in one breath each:**
   - "Hey Lamp, ask Codex to suggest three friendly names for a desk lamp."
     The read-back names Codex; "Yes, go ahead." gets "I'm giving that to
     Codex".
   - "Hey Lamp, have Claude Code write two short lines on why a desk needs a
     lamp." The read-back names Claude Code; "Yes, go ahead."
   - "Hey Lamp, can you get someone to suggest a name for a plant?" (nobody
     named): the read-back names Claude Code; say "No." Nothing starts.
6. **Read what went where:** `printf '%s\n' '{"op":"list"}' | lamp-tasks
   request --socket /run/lampos-tasks/request.sock` (eight a page; then
   `{"op":"list","after_id":N}` with its `next_after_id`) gives each task's
   `target_alias` and dispatch state: `office-codex` accepted, `office-claude`
   accepted, `office-claude` prepared (`invalidated` once its two minutes
   pass). The voice logs function names only, never arguments, so this is
   where `who` shows.
7. **Both answered, both told back.** Within a few minutes, sitting at the
   lamp and saying nothing, it should say unasked "Oh, Codex just finished…"
   and "Oh, Claude Code just finished…" (`task_news`, record `task_news`).
   Then "How's Codex doing?" should be answered about Codex's task
   (`check_agent_task` with `who`).
8. **Going back:** `sudo cp -p targets.office.json targets.json`, take the two
   lines out of `lamp-voice.service`, daemon-reload, restart both.

Keep the read-back sentences, the `list` output and the two journals
(`journalctl -u lamp-voice -u lamp-tasks --since …`) in the verification
folder, with no task text that is private.

**Where it is most likely wrong on the lamp.** The model choosing `who` from
speech: the enum, its hints and the close forms are tested only as text on
the host, and the model hears sound; a mishearing outside the close forms
("clawed", "code") prepares nothing and has the model ask, which costs a
turn. The read-back may still garble the name (on 2026-10-14 it said "the
code" for Claude Code). Codex in ask mode may stop to ask permission even
for a text-only task, and then the news says Codex needs the person instead
of an answer. A Codex receipt or `show` reply shaped otherwise than the
fixtures leaves the task `uncertain` or unread. Codex may ask to trust its
folder when `lamp-tasks` names it (step 1 checks this by hand). OpenCode is
not in lamp-4ace's file: it needs OpenCode installed and signed in on the
office computer, and its receipt checked by hand first, as step 1 does for
Codex.

**Not shown.** No task was started anywhere for this; no Harness session,
receipt or answer from Codex or OpenCode was seen. Nothing was run on a lamp.
How often the model names the right one from speech is not measured.

## Progress on the bus (`serve --progress`, #732)

Built by the second builder on 2026-10-15 (`second-P2`). **Off unless
`lamp-tasks serve --progress` is given**; with the watch on or off
(`--no-watch`).

**What exists and what was taken.** `harness show` is empty while an agent
works (above). `harness ps --machine <id> --json` lists every harness on one
machine. Its shape was read from the installed Harness 0.4.2 CLI's own
source (`~/.harness/cli/cli.js` on the development Mac), not from any
session: one JSON object `{you, machine, harnesses}`, printed with one space
of indent. Each harness has `name`, `id` (the agent's id; one still being
made is listed by its creation id), `agent` (the engine's display name),
`machine` (its label), `state` (`working`, `idle`, `unknown` or `offline`;
`starting`, `installing` or `failed` for one being made; a stopped one is
left out unless `--all`), `folder`, `branch`, `startedBy`, `yours`, `self`,
`lastActive` (ISO 8601 in UTC, or null) and, for at most 40 that are not
stopped, `asks` (the last two things it was asked, 200 characters each) and
`about`: the summaries of its last three events, 140 characters each, joined
by ` / `. For those, `ps` asks the agent's machine (`agent_recent`, 3 s
each, in parallel). So one `ps` reads every task on a machine at once.
Nothing new is taken in: the existing adapter, process bounds and caller
identity run it, and `serde_json` reads it.

**What was built** (`crates/tasks/src/progress.rs`, `harness.rs`,
`runtime.rs`):

- **Which tasks.** The ones the watch looks at (`Engine::watched`):
  accepted, nothing settled kept, handed over at most a day ago in this boot
  (or in an earlier one, which cannot be dated). Listing them reads and
  changes nothing in the store.
- **When.** One `ps` for each machine with such tasks, on the watch's
  schedule: every 30 s while any of that machine's tasks is in its first ten
  minutes, then every two minutes, counted from when that machine was last
  asked (a failed read waits its turn too). A task new to the watch (just
  handed over, or there when the service starts) makes its machine due at
  once, so it is on the bus within one read and its first line, where
  counting begins, is read as early as it can be. Launches and `show` reads
  go first: a `ps` takes the one CLI worker only when they have nothing to
  do.
- **Bounds.** 10 s a read (the read deadline), 256 KiB of standard output
  (four times a `show`'s, since forty agents' lines can come back), 16 KiB of
  standard error, at most 256 harnesses in a reply, a progress line cut to
  2 KiB at a whole character with control characters made spaces, and at
  most the store's 16 tasks. A reply that breaks a bound or is not of the
  shape is a failed read: counted, and nothing changes. A `lastActive` in
  another form is not known, rather than a reason to lose the whole reply.
  The read is never retried.
- **What is kept.** The latest progress line of each task, in memory only:
  never in the store's files and never on the bus. A restart begins again
  from the next read, which is at once for every task in the watch, as
  another run: it keeps the store and the task numbers and counts changes
  from nothing, so every status says its `run` (when that run began, in
  milliseconds since 1970), and a change is named by its run and its count
  together (#765 review, 2026-10-15).
- **What is put on the bus.** `tasks/status` (`lamp_msgs/TaskStatus`,
  [msgs.md](msgs.md#the-tasks-contract)), a kept (`latest`) value with no
  period. It is published at start, so the kept topic always has a value
  and a reader can tell "nothing being worked on" from "the tasks part is
  not there"; after every read; at once when the set of tasks watched
  changes, a task joining or leaving the watch (settled, or a day old); and
  when a task joins or leaves `settled`, below. A
  task watched whose progress has not been read yet in this run (just handed
  over, there when the run began, or every read of it failed so far) is in
  `unread`, not in `tasks`, so `tasks` and `unread` both empty is nothing
  being worked on, and `tasks` empty alone is not (the reviewer on #793,
  16:46). Its publisher says once a second that it is alive. One message holds every
  task being read, so the one kept value is always all of them:

  | Field | What it says |
  |---|---|
  | `store_id` | The store the task numbers belong to |
  | `unread` | The tasks watched whose progress has not been read yet in this run, in order; left out when none |
  | `run` | Which run of the tasks part counted these changes: when it began to read progress, in milliseconds since 1970. Counts begin again at each run |
  | `task` | The task's number (`get_task`, `get_progress`) |
  | `engine` | Who has it: `claude` or `codex`, as its target names it |
  | `state` | `working`, `idle`, `needs_input`, `starting`, `installing`, `stopped`, `offline`, `failed`, `unknown`, or `missing` when the agent is not in the list (stopped, or gone). A state Harness may add later is `unknown` |
  | `working_s` | Seconds since it was handed over, if that was in this boot |
  | `last_active_unix_ms` | When Harness last saw the agent do anything |
  | `read_unix_ms` | When it was read |
  | `changes` | How many times its progress line changed since this run began reading it. The first line read is where counting begins, not a change |
  | `changed` | Whether the line changed at the read this message was made from. A read of one machine sets it only for that machine's tasks |
  | `new_share` | At the latest change, the share of the line's distinct words that were not in the line before, 0 to 1 |
  | `signs` | Signs in the line at its latest change that were not in the line before |
  | `signs_seen` | Every sign seen since reading began |
  | `settled` | Tasks the watch let go because their agent settled, for two minutes after (`progress::SETTLED_FOR_MS`), in order, each with `task`, `engine`, `state` (`idle` for finished, `needs_input`, `stopped` or `failed`), `settled: true` and `at_unix_ms`, when the tasks part found it so; left out when none (#1021) |

  No word of the agent's, the task's text or the asks is in it: a test reads
  every message and looks for them.

  **Settled (#1021).** The watch stops as soon as its own look finds the
  agent settled (finished with an answer, stopped to ask, stopped or
  failed), and the task leaves `tasks` at once, so its last `ps` state may
  still be `working` and `idle` is often never published (Jarvis, for Task
  news). Such a task is said in `settled` instead, for two minutes from
  when its settled answer was first kept (the newest observation's
  `observed`, by this boot's clock; `Engine::settled_within`), and only
  while the watch's read is the task's latest and did not find the agent
  back at work: a task the person asked to have read, or one read with the
  watch off (`--no-watch`) or after it was let go for age, is not listed,
  as the person has heard it or it is not news (Fourth on #1048). A later
  look of the watch (#1145's follow looks) neither takes it off while under
  way nor gives it a new time when it finds the same answer, so it is
  never listed twice for one answer (Ten on #1145). The read is kept in the store, so a
  restart of the tasks part inside the two minutes lists it again in its
  first value. Two minutes: long enough for a skill that triggers on it
  (Task news, per task, as it appears in `settled`, #1042), short enough to
  say what has just happened. The list is looked at each turn of the
  service, so a value is published when a task joins it and when it leaves,
  with nothing else happening. A task let go for age just leaves.
- **Signs**, cheap and without a model (`progress::SIGNS`): phrases matched
  as whole words, regardless of case, `#` for any number. A phrase with one
  of `not`, `no`, `never`, `cannot`, `can't`, `don't`, `doesn't`, `didn't`,
  `isn't`, `aren't`, `wasn't`, `weren't`, `won't`, `hasn't`, `haven't`,
  `couldn't`, `shouldn't`, `without`, `until`, `unless`, `if`, `once`,
  `when`, `whether`, `to`, `will`, `would`, `should`, `might`, `may`, `must`,
  `need`, `needs`, `try`, `trying`, `next`, `I'll`, `we'll`, `let's`,
  `going` or `before` among the three words before it is not taken (denied,
  hoped for, still to come), nor one followed by a question mark.

  | Sign | Phrases |
  |---|---|
  | `tests_pass` | tests pass, tests passed, tests passing, tests are passing, tests now pass, tests all pass, all tests pass(ed), test suite passes, suite passes, tests (are) green, # passed, # passing, # tests pass(ed), 0 failed |
  | `build_works` | build succeeds/succeeded/passes/passed/works, build is green, builds cleanly/successfully, compiles, compiles cleanly, compiled successfully, clippy (is) clean |
  | `fixed` | fixed, resolved, (the) fix works |
  | `found` | found, root cause, the cause, the culprit, identified, tracked down, figured out |
  | `done` | done, finished, completed, implemented, all set, wrapped up |
  | `pull_request` | pull request, opened (a) pr, created a pr, pr opened, pr is up, pr #, and any link with `/pull/` in it |

  Chosen from how coding agents describe their own work; not measured on
  the lamp's own tasks yet (the hand-back below).
- **The words, when the lamp will speak.** `get_progress` on the request
  socket: `{"op":"get_progress","id":3}` answers `{"ok":true,"store_id":…,
  "revision":…,"completion":"unverified","id":3,"progress":{"engine","state",
  "about","run","changes","signs_seen","last_active_unix_ms","read_unix_ms"}}`;
  `not_read` for a task watched and not read yet, `not_found` for one not watched, and `progress_off` without the
  switch. That is the only way the line leaves the tasks part. A task the
  watch let go because its agent settled answers, while it is in
  `settled`, `{"ok":true,…,"id":3,"settled":{"task","engine","state",
  "settled":true,"at_unix_ms"}}`: what it ended in, not `not_found` (Ten and
  Nine on #705, problem 3 (b)); its answer's words are `get_observation`'s.
  Task news reads only `progress`, so it says nothing of one.
- **The journal.** One line a read, so a minute on the lamp can be judged:
  `tasks: progress read of <machine> in 412 ms: 2 listed; task 7 working,
  new line, tests_pass fixed; task 8 needs_input, same line` (`first read`
  for a task's first), and for a failed read `tasks: progress read of
  <machine> failed after 10000 ms (timeout); tasks 7, 8 as before`. States
  and signs only, as the bus spells them; never an agent's line.
- **Health.** With the switch, the `tasks` diagnostics add `status_reads`,
  `status_failures`, `status_last_ms` and `status_most_ms` (the last and
  the longest read, from handing it to the CLI worker to its outcome) and
  `status_published`.

**Without the switch** nothing of this runs: no `ps`, the tasks part never
makes the `tasks/status` topic (no declaration or value on the bus; its
directory and `subs/` are there only for the voice's own socket, since the
voice subscribes wherever the bus is, for a task newly settled, #560), the
diagnostics carry exactly the keys they did, and `get_progress` is refused
with `progress_off`, which no client sent before. The store's files are
untouched either way: progress is memory only. Tests:
`without_progress_nothing_is_listed_the_topic_is_untouched_and_the_report_is_as_before`
and, with it,
`with_progress_a_value_from_the_start_each_task_read_at_once_then_on_schedule_and_the_words_stay_here`
(`crates/tasks/tests/runtime.rs`).

**After its first minutes on lamp-4ace** (the first builder on #751,
2026-10-15 07:52 and 13:50). Seven tasks handed over by voice from 07:22
to 07:44 each finished within about 70 s, and `lamp-bus latest
tasks/status` said "keeps no value" every 20 s from 07:45 to 07:51: the
topic had no value until a first read, and a machine's first read waited
30 s after the hand-over. At 13:43 a 50 s task did leave a value. Changed
since, as asked (second builder, `second/progress-reads`): the value at
start, the read at once for a task new to the watch, a task leaving the
bus at once when it leaves the watch, not at the next read, and the
journal line. Tests: `a_task_new_to_the_watch_is_read_at_once_then_on_the_watchs_schedule`,
`a_value_at_start_and_whenever_a_task_on_the_bus_leaves_the_watch` and
`each_read_is_one_journal_line_in_typed_words_only`
(`crates/tasks/src/progress.rs`), and the runtime test above, which now
reads the start value with `latest`, sees the first read without the
clock moving, and settles the task to see it leave. Not tried on a lamp.

**For the voice.** Proposed here on 2026-10-15 and agreed by the first
builder on #752; built as ["The voice's side of progress
news"](#the-voices-side-of-progress-news-752) below.

**Measured on the development Mac** (Apple M2 Max, macOS 26.6.2, Rust
1.96.0 release build, other sessions running: load average 14 to 20), with
`cargo run --release -p lamp-tasks --example progress_cost -- --samples 200`
on listings made up in Harness 0.4.2's shape, each line three events of
about 130 characters. Medians of 200 (first, largest):

| Harnesses in the reply | Bytes | `parse_ps` | Signs over every line | Tracking 16 tasks |
|---|---|---|---|---|
| 1 | 856 | 1.5 µs (79, 79) | 9.4 µs | 1.0 µs |
| 16 | 12,878 | 22 µs (28, 64) | 158 µs | 3.0 µs |
| 40 | 32,138 | 55 µs (61, 133) | 398 µs | 4.0 µs |
| 256 | 205,634 | 342 µs (370, 743) | 2.6 ms | 11.7 µs |

Signs are found only for the watched tasks' own lines at a change, at most
16, not for every harness listed. One whole read through the adapter, the
example standing in for `harness` with a 40-harness listing (spawn, capture,
parse; no network, no daemon): 4.2 ms median, 13.7 ms first, 18.4 ms the
largest of 50. The raw output is in
[the verification folder](verification/progress-news-host-20261015/README.md).
**This is the Mac's, not the lamp's**: the real `ps` starts Node and asks
another machine, and its cost on the lamp is the hand-back below.

**On the lamp** (the first builder; the smallest step first; nothing here
starts, shows or closes a session):

1. The cost of one `ps` there, as the service user, ten times:

   ```sh
   TIMEFORMAT='%R s real, %U s user, %S s sys'
   for i in $(seq 10); do time ~/.local/bin/harness ps --machine office --json >/dev/null; done
   ```

2. Install the ARM64 run's `lamp-tasks`, add `--progress` to
   `lamp-tasks.service`'s `ExecStart`, `sudo systemctl daemon-reload`,
   restart it, and see the switch off first: `lamp-bus info tasks/status`
   says `declaration none` before the switch and `type=lamp_msgs/TaskStatus`
   after.
3. Hand over one real task to Claude Code on `office` by voice, as on
   2026-10-14, and keep the statuses for its life:

   ```sh
   lamp-bus tap tasks/status --seconds 7200 > /var/tmp/progress-status.txt
   ```

   Each line is a header and the message, its fields in the order of the
   alphabet. About changes per hour: `grep -o '"changed":true'
   /var/tmp/progress-status.txt | wc -l` over the hours it ran (each change
   is flagged at one read only). Signs firing: `grep -o '"signs":\[[^]]*\]'
   /var/tmp/progress-status.txt | sort | uniq -c`. Whether each sign was
   right: the line it came from, kept by hand and not committed (it is the
   person's work), with
   `printf '{"op":"get_progress","id":N}\n' | ./lamp-tasks request --socket /run/lampos-tasks/request.sock`.
4. The read's own cost inside the tasks part: `lamp-bus tap diagnostics
   --seconds 5 | grep '"name":"tasks"'` shows `status_last_ms` and
   `status_most_ms`; `top -b -n 1 -p "$(pgrep -f 'lamp-tasks serve')"` its
   CPU between reads.

**Most likely wrong on the lamp.** That `about` says what the agent did
rather than the person's own last words: when an agent has had no event
since it was asked, its recent events may be the ask itself. That a `ps`
of a machine every two minutes is cheap on the lamp: it starts Node and
asks the office machine about up to 40 agents each time. That the English
phrases carry over to how Claude Code and Codex word their own summaries
(a `found` in "found no problem" fires; a summary in another language never
does). That `lastActive` and the lamp's clock agree: `idle_s` is the
difference of two machines' wall clocks.

**Not shown.** No real `ps` was run: the shape is from the CLI's source and
the replies are made up. No lamp, no Harness account, no real agent's
lines. The bus permissions between the tasks part (as `orangepi`) and the
policy (a dynamic user) on lamp-4ace were not looked at.

## The voice's side of progress news (#752)

Built by the second builder on 2026-10-15, in `crates/voice/src/task_news.rs`
(the voice is the first builder's; the change stays in that file). **Off
unless the voice starts with `LAMP_PROGRESS_NEWS=on`** (and the tasks part
is set, `LAMP_TASKS_DIR`, as for all task news).

- **What is listened to.** `policy/decisions` and `tasks/status`, on the
  lamp's bus (`LAMP_BUS_ROOT`, or `/run/lampos-bus`), by a thread of its own,
  subscribed before the voice goes on. Nothing else in the voice changes.
- **What is taken.** A decision about `progress`, `say_now`, acting
  (`shadow: false`, [`lamp-policy --act progress`](policy.md#acting---act-progress-752)),
  whose `task` names `<store>:<task>:<change>@<run>`. It is let go, with
  one line in the journal saying why (`lamp-voice: progress to say let go
  (stale)`), when it was made more than 60 s before (the bus header's
  time); when `tasks/status` is of another store or another run of the
  tasks part (`other_run`), does not hold the task, shows a later change
  than the one decided about, or shows the agent no longer working.
  The minute is counted from the decision: the time it may be told until
  is fixed when the decision comes, before anything is fetched.
- **The words, now.** `get_progress` and `get_task` on the request socket,
  each within 1.5 s: the progress line, the signs seen, the task's words and
  whom it went to. They are let go, and nothing is offered, when they are
  of another store or another run, when the agent is no longer working,
  when they are of
  any change but exactly the one decided about (the task moved on, or the
  tasks part began counting again), or when they come back at or past the
  decision's minute (#765 review, 2026-10-15).
- **The note**, one written turn, through `task_news::fitted` so it fits the
  transport's 1,024 bytes (the progress line cut first, then the task's
  words): who (Claude Code or Codex), the task (300 characters), the
  progress line (400), the signs not yet put in a note for this task in
  words ("the tests pass", "a pull request was opened"), and about how many
  minutes it has been at it. Asked for: one short natural sentence, the way
  a chief of staff mentions it in passing, starting like "Oh, Claude Code
  has…", nothing read out word for word, no code, paths or links, no
  question at the end.
- **Telling.** It is offered with the other news and told under the same
  gates (someone at the lamp, 15 s quiet, nothing owed, the room not busy),
  and let go if not told within the 60 s of its decision. While it waits
  for its moment it is checked against the newest `tasks/status` the
  reader has, every tick the coordinator looks (`waiting`) and once more as
  it is taken to be told (`tell`): once the task has left the watch,
  finished, moved on to a later change, or the tasks part restarted as
  another run, the note is withdrawn and never said. The coordinator never waits for this: the status is looked at only
  when it is free that tick, and a `tell` that finds it being written tells
  nothing and tries again on the next. Told, its key
  `<store>:<task>:progress-<run>-<change>` goes into the file of what was
  told, as every piece of news does. (`progress-` keeps it apart from an
  observation's number; the proposal had `<store>:<task>:<change>`.) The
  run is in it because a restart of the tasks part keeps the store and the
  task numbers and counts changes from nothing: without it, the new run's
  change 1, another line, was taken for the change 1 told before and let go
  unfetched (#765 review, 2026-10-15, 16:02;
  `after_a_restart_another_line_at_the_same_count_is_fetched_and_offered`).

**Without the switch** no progress thread is made, `policy/decisions` is
not subscribed to (no socket of the voice's there), the tasks part is asked
nothing but what it was asked before, and nothing more is kept. Since #560
the voice listens on `tasks/status` all the same, with one socket of its
own, only for a task newly settled, which has the settled news read at once
("Telling", above):
`without_the_switch_no_progress_is_listened_for_asked_or_kept`
(`crates/voice/tests/progress_news.rs`), which also shows that only `on`
turns it on.

**Shown on the development Mac**, in `crates/voice/tests/progress_news.rs`
on a bus of the test's own, with a stand-in for the tasks socket and the
policy's own loop (`run_with_progress`, acting, with a stand-in judge that
says yes): a change on `tasks/status`, then the policy's `say_now`, then one
note offered to tell, which fits and is taken by the transport's encoder;
told once, kept as told, nothing more offered. Decisions in shadow, a
`wait`, another store, an older change and no task are let go, and the
newest change is then taken; a stale one, one about a task not watched or
not working, and one not told within 60 s are let go in the unit tests.
After the review of #765: words that say the agent is idle, or are of an
older change than the decision, are let go
(`a_task_that_finished_before_its_words_came_is_not_told_as_working`,
`words_of_an_older_change_are_not_told_under_a_newer_decision`); a queued
note is withdrawn when its task leaves the watch, finishes or moves on
(`queued_progress_is_withdrawn_when_its_task_goes_finishes_or_moves_on`)
and when it finishes while the lamp waits for its moment, at `tell`
(`work_that_finished_while_the_lamp_waited_for_its_moment_is_not_told`);
with a tasks socket that takes 400 ms a read, the note's time still ends
60 s after its decision (`the_minute_counts_from_the_decision_not_from_the_fetching`),
and words that come back past it are let go
(`task_news::tests::words_that_come_back_past_the_decisions_time_are_let_go`).
"Told" here is offered and taken by `TaskNews::tell`, as the coordinator
takes it: the coordinator, the model and the sound are not in these tests.

### Playing progress news on lamp-4ace

For the first builder; nothing here was run on a lamp.

1. Install the ARM64 run's `lamp-tasks`, `lamp-policy` and `lamp-voice`
   with the switches as they are, start them, and see the minute out:
   `journalctl -u lamp-policy -n 5` says `progress=true` and no `act`.
2. Turn it on: add `--act progress` to `lamp-policy`'s `ExecStart` (beside
   `--progress --utc-offset-minutes 420`) and `Environment=LAMP_PROGRESS_NEWS=on`
   to `lamp-voice.service`; `sudo systemctl daemon-reload`, restart both.
   `journalctl -u lamp-policy -n 5` ends `progress=true act=progress`;
   `lamp-bus info policy/decisions` lists a `voice.` subscriber, and
   `lamp-bus info tasks/status` two: progress's, and the one that listens
   for a task newly settled (#560).
3. In three terminals:

   ```sh
   lamp-bus tap tasks/status --seconds 1200 > /var/tmp/progress-status.txt
   lamp-bus tap policy/decisions --seconds 1200 | grep '"about":"progress"'
   journalctl -u lamp-voice -f | grep -E 'progress to say|telling news'
   ```

4. Sit in front of the lamp and ask, in one breath: "Hey Lamp, can you ask
   Claude Code on my office computer to write a small Python script that
   prints the first twenty primes, with tests, and run the tests?", then
   "Yes, go ahead." Stay in view and quiet.
5. What should happen: within a few minutes, a change on `tasks/status`
   with a sign (`"signs":["tests_pass"]` or the like); a decision with
   `"choice":"say_now"` and `"shadow":false`; in the voice's journal
   `progress to say offered (<store>:<task>:progress-<n>)` then `telling news
   from the work handed over (…)`; and the lamp says one sentence about how
   the work is going. Once the agent finishes, the finished news follows as
   before.
6. Hand back: the sentence the lamp said, the times of the decision and of
   the first sound, any `let go (…)` line with its reason, and the
   policy's `progress decision` lines (`journalctl -u lamp-policy | grep
   'progress decision'`).

**Most likely wrong on the lamp.** That Jev says `say_now` to a real
step: with the hard rules and a product of three answers it may say `wait`
to everything, and nothing is told. That the 60 s window and the voice's
gates meet: someone talking, or nobody seen, for the whole minute after a
decision lets the note go. That a progress line read out of context makes a
natural sentence: the note gives the model the agent's own summary.

## Real Harness and Voice qualification still required

After the account owner authorizes a disposable task and confirms its exact
target, First must check the following against the installed versions:

1. Record CLI and daemon versions on the lamp and target, chosen engine and
   existing permission configuration. Verify generic outside-agent identity,
   agent-start policy and the provisioned absolute executable/environment.
   Preserve a real ask-mode refusal; do not escalate mode to make it pass.
2. Obtain owner support for strict full-machine-ID dispatch, or explicitly
   resolve the documented resolver race before enabling real task delivery.
   An exact preflight alone is not that proof.
3. Use a harmless, reversible task in an explicitly selected project. Confirm
   the person's exact spoken yes precedes the attempt, the immediate local
   feedback leaves Voice available, and the returned child/target match.
4. Interrupt local transport/processes before and after launch receipt. Verify
   the remote task may continue while the lamp retains uncertainty and issues
   no second `new`. Exercise installation-pending and the creation-ID lookup
   gap without guessing a name or reading Harness's private ledger.
5. Read an old answer, a changed same-length answer, needs-input, stopped state,
   missing/deleted-agent read failures,
   missing/rotated Harness ownership ledger and engine restart. None may become
   completed work merely from text or activity state. Completion/group replay
   and focused-agent/follow-up support need an owner-supported extension.
6. Wire Voice's private attestations, output cancellation and once-only claim
   handling. Test privacy, busy/listening/speaking state, quiet interval,
   ten-minute staleness, interrupted output, lost acknowledgements and separate
   Voice/Tasks crashes. Qualify actual played speech independently.
7. Measure cold/warm real CLI, durable receipt and output boundaries on the
   lamp. Check suspend, reboot, flash failures and user-performed power loss;
   reconcile reset/account changes and retained private history.

## What has not been shown

No live Harness task, account request, credential access, remote execution,
permission dialog, focused-agent resolution or Voice output is part of Third's
qualification. Source inspection and scripted tests do not establish installed
0.3.94 compatibility, atomic target selection, pending recovery, correlated
completion, a new permission sandbox, exactly-once human hearing, real flash
power-loss safety or lamp latency. Those gaps remain explicit; nothing here
changes the running lamp until First opts into and qualifies the integration.
