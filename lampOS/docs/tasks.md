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
A finished agent's news is told by the voice once the watch sees it idle
(below, "The lamp as chief of staff"), still as unverified.

## Ownership and switches

`lamp-tasks serve` returns `enabled:false` before opening a store, reading a
target file, constructing an adapter, accessing a clock, or creating workers.
`--enable` starts the private service. It does not confirm a task. The
product's `lamp-tasks.service` starts it with `--enable`, and its
`lamp-voice.service` starts after it.

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

`--route` (off unless set; the person, 29 Oct 23:46) asks Harness's
router first, after the yes and before `new`: `harness route --machine <id>
--json --send -- <task>`, within 20 s (`harness::ROUTE_TIMEOUT`,
`Cli::route_first`). That time counts inside the launch's one 60 s bound:
a `new` after it gets what is left. The router (Jev, as
Harness's ⌘B, openharness `docs/design/2026-10-09-auto-router.md`) reads
that computer's sessions and either picks one, typing the words into it
when it is idle as `harness reply` does (openharness#2157, the person's
`programs-reply` switch), or says the task is new work.
- **A session picked and typed into** is the task's agent (`joined` in the
  dispatch line). The watch, `wait` and the news follow it as any task's.
  It is the person's own session, so the task is stored not closable and
  `--close-told` never closes it; an older build reads that the same way.
  A follow-up typed into a session an earlier task of the lamp's had
  ("now make it rhyme", after the lamp's own poem task) takes it over once
  that task is spent: its newest answer settled (idle with an answer,
  stopped or failed, not waiting on the person) and no close of it under
  way. The earlier task leaves the store, so the store keeps one task a
  session, and the follow-up watches the session and tells its answer. An
  earlier answer not yet told is then not told on its own. A session the
  store still sees working, though the router saw it idle (a race: a
  working one is `NOT_IDLE` at the router, so `agent_busy`) keeps its task
  (a close under way holds every launch until it ends), and the words typed into it are `uncertain`
  (`reused_remote_id`), never sent again.
- **`new` as before:** the router says new work, is off or can't be
  reached (`ROUTER_UNAVAILABLE`), refuses (`REPLY_OFF`, `NOT_YOURS`), or its
  pick is no longer running or its computer refused the words (`FAILED`).
  Nothing was
  typed in any of these.
- **A busy pick** (`NOT_IDLE`: working, or maybe waiting on the person)
  ends `not_sent` with `agent_busy`, and never starts a new session for
  the same task. Task news says so with its `failed_busy` words.
- **A send that ended unconfirmed** (`UNCONFIRMED`, `DISCONNECTED`,
  `INVALID_RESPONSE`, `CANCELLED`: openharness#2252 says so under `sent`
  with `uncertain`), an answer it can't read after it may have typed, or a
  route stopped before it answered, is `uncertain`, never sent again and
  never new: the words may be in the session already.
- **Not routed:** Devin Cloud, whose sessions are in Devin's cloud.
- **Which session:** the router picks among every session on the target's
  computer, so the read-back's agent may not be the one that takes it.
- **Tests:** `tests/harness.rs`
  `with_route_the_router_is_asked_first_and_its_pick_is_the_tasks_agent`;
  `engine::tests::a_task_that_joined_the_persons_session_is_told_and_never_closed`,
  `engine::tests::a_follow_up_takes_over_a_spent_tasks_session_and_never_a_working_one`;
  the voice's `skill_words` (with `failed_busy`).
- **Not yet shown:** any run with a real `harness route`, which
  harness-reviewer is building (29 Oct).

`--reply-named` (off unless set; #5535, the owner, 9 Nov) hands the work
to a session the person names ("go into the docs session and also do
this"), never a new one. The tasks request `prepare` takes an optional
`session`, the session as the person said it (its name, or what it worked on), and it goes into the launch, so the
yes is to that session (`Launch::session`, absent when none was named;
a store holding such a task is schema 5). After the yes and the machines
preflight, `Cli::reply_named` lists that computer's sessions with `harness
ps --machine <id> --all --json`, the ones stopped this week too, within
the launch's bound (60 s), not the 10 s read bound: on lamp-c0a4 on 9 Nov
that list of the person's Mac took 4.5 to 10 s, and two hand-overs said
yes to twice ended `sessions_timeout` at 10 s. A row fits when its name or its folder's last part is
the words said, compared on letters and digits only ("Docs-Update",
"docs update"); when no row is that, a row where each word said begins
one of the name's or folder's words, in order (three letters or more in
all: "docs" fits "Docs-Update", "api" never fits "rapid-prototype", the
Reviewer-lead's P2 on #5779). When neither fits, the session is found by
what it worked on (the owner, 9 Nov: a person remembers the work, never a
session's name, which for one the lamp started is `lamp-<hex>`, or its
id): the words said, less the ones that only say which one ("the", "one",
"session", "earlier", "that", … `NOT_THE_WORK`), against each row's
`asks` and `about` (never its name or folder, which only the rule above
reads, so "client api" never finds "Fix the API client"). A word said is
held when a row's word is the same, or the same with one of the endings
s, es, ed, ing, er or ers (`ENDINGS`: "book" for "books.py", "test" for
"tests", never "testament" or "notebook", the Reviewer-lead's P2 on #5784
at de9bebdb41). A word of one or two letters takes no ending, so "the US
tax session" never finds "Update user tax settings" (`SHORTEST_STEM`,
Reviewer-three's P2 on #5784 at 71bf964baa). A row fits only when it holds every one of those words,
so "the payroll dashboard one" never finds a weather dashboard on
"dashboard" alone (Reviewer-three's P2 on #5784 at 3d90e56736). So "the
book program one" finds the session whose `about` is "I wrote books.py, a
short command-line program for keeping a reading list". Two that fit are
`session_ambiguous`, a live one beside a stopped one too, since the one
meant may be the stopped one (the Reviewer-lead's P2 on #5784 at
71bf964baa). Harness fills `asks` (the last two
prompts) and `about` (the last recaps) only for a live session with a
finished turn, within 3 s, for the first 40 rows, and never for a stopped
one (openharness cli `commands.ts` 602-605, as the Harness team means
it). So with `--close-told`, which closes a session once its done line
is told, a follow-up by its work ("in the moon poem, add a line about
the stars") would never find it (lamp-c0a4, 10 Nov). When no row fits by
name, folder or work and the words name some work, the newest stopped
rows of that machine (at most 4, `MOST_STOPPED_READ`, by `lastActive`;
failed or offline ones are not read) are read together, one thread
each, with `harness show --machine <id> <agent> --json` (its last turn
only: `ask`, `answer`, `tools`, commands.ts 746-790), each within what
was left of the launch bound less 1 s when they began (none with under
2 s left), and their last `ask` is matched by work the same way, newest
first. On lamp-c0a4 on 10 Nov each show took about 2.3 s, so four read
one after another cost about 9.2 s of a 22.8 s follow-up; together they
cost about one show.
One that fits is that stopped session; two are `session_ambiguous`; a
show that fails or can't be read is passed over. A live row that fits by
its work is taken without reading the stopped ones, so a stopped one
that also holds the words is not weighed against it. A row with no turn
ended yet is found by its name or folder alone. A session
the lamp started keeps the `lamp-<hex>` it was given with `--name`, which
Harness never replaces with the agent's title. `harness route` was tried for this
and not used: on lamp-c0a4 on 9 Nov it took 6.6–18.6 s a call and called
"add a search feature to the reading list book program" new work. The words are typed in with `harness reply
--machine <id> <agent> --json -- <task>`, which Harness takes for a live,
idle session behind the person's `programs-reply` switch. Nothing is
routed, the engine is not asked about (`--engines-ready`, a known
refusal), and nothing is started new in any case:
- **One live session fits and takes it:** the task's agent, `joined`, as a
  routed one: watched, told, and never closed by the lamp. How the person
  knows that session is kept on the task (`dispatch.joined`, listed): its
  `name`, or its folder's last part when it has none, never a lamp-made
  `lamp-<hex>` one, and its first `ask`, on one line, cut at a word to 80
  characters. Task news says it at once, so a wrong join is heard (the
  Reviewer-lead's P2 on #5784 at 71bf964baa). A routed join keeps none.
- **Not sent, with why:** `session_not_found`; `session_ambiguous` (two or
  more live ones fit, or, found by its work, a live one and a stopped one,
  or two stopped ones by their last asks; never guessed);
  `session_stopped` (only a stopped one fits, `LAMP_TASKS_RESUME_CLOSED`
  off; two stopped ones by their name too); `session_busy` (working, asking the person something, or
  still starting); `session_closed` (the reply's `NOT_RUNNING`: Harness
  says it was closed or stopped; nothing is started on its own, the first
  builder's ruling on #5784's follow-up; with `LAMP_TASKS_RESUME_CLOSED`
  it is reopened first, below); the reply's other refusals
  (`reply_off`, `reply_not_idle`, `reply_failed`); `sessions_failed` or
  `sessions_<code>` when the list can't be read, `sessions_timeout` too
  when it leaves the reply under 1 s of the launch bound (`REPLY_FLOOR`),
  and `sessions_offline` when nothing fits while a computer was left out
  as offline (below).
- **Closed, reopened** (`LAMP_TASKS_RESUME_CLOSED=on` in lamp-tasks' own
  unit, off unless set). It needs Harness CLI 0.5.98 or newer, the first
  released CLI that lists `resume` in `harness --help` (openharness#3606,
  PR #3808); the switch can be on once the lamp's pin reaches it (#5957).
  One stopped session that fits, by its name or folder or by its last
  ask (above), is reopened with `harness resume --machine <id> <agent>
  --json` before any reply (Harness CLI 0.6.2 `commands.ts` 983: the same
  id, name and folder, `hermes --resume <sessionId>` for Hermes), and a
  reply that says `NOT_RUNNING` is followed by it once, both inside the
  same launch bound. Then `harness reply` types the words as into an idle
  one, which needs the person's `programs-reply` switch (else
  `REPLY_OFF`, exit 5, `reply_off`). Before the first
  resume of a start, lamp-tasks asks whether the installed CLI has it
  (`harness --help` lists `resume`, or else `harness resume --help` exits
  0) and keeps the answer for the rest of the run (a look that can't
  finish is not kept, and says no that time); a CLI without it is one
  journal line, `Harness <version> has no resume` (from `harness
  --version`), and `session_closed`. Only `ok`,
  `id`, `resumed` and `state` are read, never `effect` (its words may
  change); an answer for another session, or with no `resumed`, is a
  failure. `idle` is typed into with `harness reply` again; `working` is
  waited for first (`harness wait --machine <id> <agent> --timeout <s>s
  --json`, leaving `REPLY_FLOOR`); anything else, or still working at the
  bound, is `session_busy`. Typed into, it is `joined`, with
  `dispatch.joined.resumed` (`true` with its earlier conversation, `false`
  open again in a new one), and Task news adds `joined_reopened` ("I
  reopened that session") or `joined_fresh` (it starts fresh, without the
  earlier conversation). `RESUME_UNCONFIRMED` (Harness could not confirm
  it yet; a second run joins the first) is `session_resume_unconfirmed`,
  told with `failed_closed_soon`, the closed-session line and "try again
  in a moment"; any other end (`NOT_YOURS`, `REPLY_OFF`, `UNKNOWN_HARNESS`,
  `AGENT_BUSY`, a timeout, an unknown code or an answer it can't read) is
  `session_closed`. Never resumed twice in one hand-over (a second
  `NOT_RUNNING` is `session_closed`), never started new; under 1 s left
  before the resume or the reply is `sessions_timeout`, nothing typed.
  Off, a closed session is `session_closed` as above, and a stopped one
  that fits `session_stopped`, nothing typed. Tests:
  `tests/harness.rs`
  `with_resume_closed_a_closed_named_session_is_reopened_once_and_typed_into`,
  `a_stopped_session_is_found_by_its_last_ask_and_reopened_with_the_switch`,
  `words_that_name_no_work_read_no_stopped_session`,
  `with_resume_closed_a_stopped_session_found_by_name_is_reopened`,
  `with_resume_closed_a_resume_that_leaves_no_time_types_nothing`,
  `with_resume_closed_an_older_harness_without_resume_is_told_closed`;
  `task_news::tests::a_reopened_named_session_is_said_reopened_and_an_unconfirmed_one_closed`.
- **Uncertain:** a reply that started and gave no answer, or one Harness
  took (exit 0) whose answer can't be read or names another session
  (`reply_uncertain`): the words may be there, so never sent again.
- **Switch off:** a launch that names a session is `not_sent` with
  `reply_named_off` before anything runs; with no session named, the
  hand-over is as before either way. Session words that are blank, over
  128 bytes or hold a control character are refused (`bad_session` at
  `prepare`, `invalid_session` at launch).
- **Tests:** `tests/harness.rs`
  `with_reply_named_a_named_session_is_typed_into_and_never_started_new`,
  `with_reply_named_a_session_is_found_by_what_it_worked_on`,
  `with_reply_named_the_joined_session_is_kept_by_its_name_and_first_ask`,
  `with_reply_named_a_closed_session_is_told_closed_and_nothing_new_starts`;
  `engine::tests::a_store_with_a_named_session_is_written_as_schema_five`,
  `engine::tests::a_joined_sessions_name_and_first_ask_are_kept_and_listed`.
- **The voice's side** (off unless `LAMP_TASKS_REPLY_NAMED=on`, the
  provider worker's `--tasks-reply-named`, last of the tasks flags; it
  needs only the tasks part): `start_agent_task` takes an optional
  `session`, described as "Only if the person means earlier work, not a
  new session: how they said it, its name or what it worked on ("the book
  program one"). Never guessed." It is trimmed and sent as `prepare`'s
  `session`; blank is none, and words over 128 bytes or with a control
  character prepare nothing and the model is told to ask which session.
  The answer's `where`, which the read-back says, is "<who>, in the
  session "…" on <computer>, not a new one", and its `next` is
  `session_next`, never only "Shall I start it?": say who will do it and
  where, as `where` says it, and ask whether to start it there, so the
  person hears the session before their yes (the Reviewer-lead's P2 on
  #5784 at de9bebdb41). The session is kept with the
  task waiting for its yes. With `LAMP_TASKS_ONE_YES`, a yes in the turn
  that prepared it never starts a task for a named session, whatever the
  read-back said: which session a line said can't be told from its words
  ("docs" inside "the docs archive session" is not the "docs" session,
  and Harness may hold both), so it always waits for a second yes, to the
  read-back of `start_agent_task`'s answer, whose `where` says the session
  as prepared (the Reviewer-lead's P2 on #5784, Reviewer-three's P2); a session of only filler words ("the
  that") prepares nothing and the model is told to ask; with `LAMP_TASKS_CORRECT`, an ask that
  changes only the session is a correction and replaces the one waiting. A
  session named for Devin's cloud is refused before anything is asked of
  the tasks part. Without the switch the parameter isn't offered, and one
  the model sends anyway is dropped. Task news tells
  `session_busy` (and `reply_not_idle`) with the `failed_busy` words, as a
  busy routed pick; `session_closed` and `session_stopped` with the
  `failed_closed` words, which have the lamp say "That session is closed.
  Open it in the Harness app and ask me again, or I can start a new one in
  the same project." (the first builder's ruling on #5784's follow-up);
  the other codes above are told as a plain failure.
  Work said yes to that joined a session is told once, at once, within
  10 minutes of the hand-over on this boot (`<store>:<task>:joined`, beside
  its later finish): with the `joined` words, naming the session by its
  name, or with `joined_asked`, by what it was first asked, and that the
  person can say if it's the wrong one. A new session, or the router's
  pick, is not told so.
  Tests: `agent_tasks::tests::a_named_session_is_sent_with_the_task_only_with_the_switch`,
  `agent_tasks::tests::the_reply_named_switch_is_on_only_with_the_tasks_part_and_adds_session`,
  `agent_tasks::tests::a_task_for_a_named_session_never_starts_on_one_yes`,
  `agent_tasks::tests::another_session_is_a_correction_and_devins_cloud_takes_none`,
  `task_news::tests::a_busy_named_session_is_told_as_busy_and_its_other_ends_as_a_failure`,
  `task_news::tests::a_closed_named_session_is_told_how_to_open_it_again`,
  `task_news::tests::work_that_joined_an_earlier_session_names_that_session_once`.

`--progress` (off unless set; #732, the second builder, 2026-10-15) reads
how each task handed over is going with one `harness ps` a machine and puts
it on the bus as `tasks/status`, without the agent's words: ["Progress on
the bus"](#progress-on-the-bus-serve---progress-732) below.

`--push-wait` (off unless set; #560, nine, 2026-10-16) holds Harness's own
`wait` on the agents the watch looks at and reads an agent as soon as it
ends, so a result is known within seconds rather than at the watch's next
look: ["Watching"](#the-lamp-as-chief-of-staff-watching-and-telling-2026-10-15)
below. It reads with the watch, so it refuses to go with `--no-watch`.

`--push-watch` (off unless set; #5230) holds one Harness `watch --json` on
each task the watch looks at, at most four, and reads a task as soon as
its agent settles: ["Following by watch"](#following-by-watch-serve---push-watch-5230)
below. It reads with the watch, so it refuses to go with `--no-watch`.

`--bus-root ABS` selects an isolated bus for qualification. `--run-for-ms N`
bounds an operator run to 1–86,400,000 ms; it does not cancel remote work.
Linux reads its boot identity itself; development hosts require `--boot-id ID`.
Configuration is loaded once. Prepared records retain their exact target and
must be prepared/confirmed again after restart, even if an alias was remapped.

The request socket permits preparation, inspection, cancellation before an
attempt, and an explicit observation request. Only the separate Voice socket
permits `confirm`, `claim_speech`, `ack_speech`, `told` and the five
`sign_in_*` ops (`Request::trusted`). Both sockets and their
parent are private to the service user. This separation is an integration
boundary, not authentication against another process with that same UID.
Actions must expose only the public operations to the model. First must wire
the trusted route exclusively to Voice's real confirmation and output owner.

“Spoken yes” means the **person's** confirmation of the exact prepared task,
not the lamp saying “on it.” The private caller attests that fact; Tasks does
not recognize speech or identify a speaker. It also cannot establish who is
allowed to act on a nearby person's account. Those remain Voice/product rules.
With `LAMP_MESSAGES=on` (#1673, [messages.md](messages.md)), a task asked for in
a text from the Autonomous app is confirmed the same way, by a "yes" typed in
that same chat within the same 120 s (`agent_tasks::may_confirm`): never by a
spoken yes, and a typed yes never confirms a spoken task.

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
It returns `remote_cancelled:false`. There is no remote cancel. The product
runs the watch (`--watch`, the default) and closes a told harness
(`--close-told`); with `--route` (not in the product's unit), follow-ups
are routed. Each is below.
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
| `status` | none | Counts, bounds, current identities, last receipt; with `serve --set-up-later` and the sign-in read, `signed_in` (`yes`, `offline` or `no`, #5488) |
| `targets` | none | Each target's `alias` and `engine`, in alias order; never its machine or project (since task 41, for the voice) |
| `list` | optional `after_id` (default 0), `limit` (1–8, default 8) | Metadata page; follow `next_after_id` |
| `get_task` | `id` | Exact task, configured target, dispatch and observation metadata |
| `get_observation` | `id`, `observation_id` | One stored observation plus its speech record |
| `get_progress` | `id` | With `serve --progress` only: the agent's progress line as last read, and what was read with it (["Progress on the bus"](#progress-on-the-bus-serve---progress-732)) |
| `tasks_now` | none | With `serve --progress` only: every task being worked on or lately settled, newest first, with its last step in the agent's words marked untrusted, in under 2 KiB (["Tasks now"](#tasks-now-4342-serve---progress)) |
| `prepare` | `guard`, `text`, `target` alias | Immutable proposal; no CLI call |
| `confirm` | `guard`, `id`, `prepared_digest`, `session_id` | Trusted person's spoken confirmation |
| `cancel` | `guard`, `id` | Only before dispatch attempt |
| `observe` | `guard`, `id` | Explicit `show` of the bound accepted agent |
| `claim_speech` | `guard`, `id`, `observation_id`, `text`, `session_id` | Trusted one-time playback grant |
| `ack_speech` | `guard`, `id`, `observation_id`, `claim_id`, `text_digest`, `session_id`, `played:true` | Trusted completed-playout attestation |
| `sign_in_start` | none | With `serve --sign-in-by-code` only, trusted: start Harness's sign-in by a spoken code, or give back the one open (["Signing in by a spoken code"](#signing-in-by-a-spoken-code---sign-in-by-code-1676)) |
| `sign_in_state` | none | With `serve --sign-in-by-code` only, trusted: the sign-in's code, the email it asks about, or how it ended; `none` before any |
| `sign_in_asked` | `id`, `request` | With `serve --sign-in-by-code` only, trusted: the voice heard the question "Sign in as …?" said whole for sign-in `id`, in its request `request` (`not_asking`, `not_this_sign_in` otherwise) |
| `sign_in_answer` | `answer`: `yes` or `no`; `request` | With `serve --sign-in-by-code` only, trusted: the person's spoken answer, in the voice's request `request`, passed on only while Harness asks (`not_asking`), once the question was told in an earlier request (`not_told`) and within two minutes of it (`too_late`) |
| `told` | `guard`, `id`, `observation_id`, `session_id` | Trusted: the voice's news of this observation was heard whole, in its model's own words ([#1390](#closing-a-finished-harness-1390-serve---close-told-on-in-the-product-unit)); once only, `already_told` after. Without `--close-told`, `told:false` and nothing written |

The [protocol](../crates/tasks/src/protocol.rs) and
[engine](../crates/tasks/src/engine.rs) define the serialized fields and error
codes. Query the local service without a separate scripting runtime:

```sh
printf '%s\n' '{"op":"status"}' | ./lamp-tasks request --socket /run/lampOS/tasks/request.sock
```

The reference client has a 1.5-second round-trip deadline. Timeout is uncertain
for a mutation. It is not evidence that no commit or dispatch occurred.

## Harness adapter

These argv forms are constructed, without a shell; `ps` only with
`serve --progress`, `wait` only with `serve --push-wait` and `watch` only
with `serve --push-watch`. The adapter
also builds `route`, `machines engines`, `reply`, `auth status`, `start`
and `login --code` (`crates/tasks/src/harness.rs`), each in its own
section below:

```text
harness machines --json
harness new codex --machine <full-machine-id> <project> --name <unique-label> --mode auto --json --task <exact-text>
harness show --machine <full-machine-id> <full-agent-id> --chars 4000 --json
harness ps --machine <full-machine-id> --json
harness wait --machine <full-machine-id> <agent-id>[,<agent-id>…] --any --timeout 600s --json
harness watch --machine <full-machine-id> <full-agent-id> --chars 200 --timeout 10m --json
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

A target's `project` is a folder or a name, and Harness treats the two
oppositely (openharness `cli/src/lib/newCommand.ts`, `projectExists`). A
**name** (`lamp-tasks`) asks Harness to make a new project folder
(`~/harnesses/lamp-tasks`) and is refused with `PROJECT_EXISTS` ("“lamp-tasks”
already exists on …") once that folder is there. So a named target takes its
first hand-over, and every later one fails `new_failed`: lamp-4ace's first
night (below), and again on lamp-0c89 on 23 October (#2813), whose folder had
been made by hand. A target used again and again names its **folder**
(`~/harnesses/lamp-tasks` or an absolute path), and that folder must already
be on the computer: Harness never makes one it is given.
`new` into a folder that isn't there is refused before anything is made
(`{"ok":false,"error":"CWD_NOT_FOUND"}`, openharness `core/agents/create.ts`;
`INVALID_CWD`'s "That folder does not exist on the machine." alike). Found
on lamp-75c9 on 22 October, when a new target's folder was missing from the
laptop. **`serve --folder-missing`** (#2840, off unless set): such a task
ends `not_sent` with `folder_missing`, and the voice tells it with Task
news' `failed_folder` words: the work didn't start, as the folder it works
in isn't on their computer (by its name with `LAMP_TASKS_MACHINE_NAMES`),
and has to be made there first. Without it, and for every other failed
`new`, the task stays `uncertain` with `new_failed`, and the lamp says the
work may not have started.

**`serve --absolute-folder`** (P4 step 1, off unless set): a `~/…` folder
is sent as that computer's absolute folder once its home is known. Harness's
CLI (0.5.28, `harness new`) resolves a `~/…` folder for another machine by
asking that machine for its home (`fs_list_dir` over the machine link)
before it asks for the agent; an absolute folder is sent as it is, so it
saves that round trip through the relay. Harness gives no home in
`machines --json`, so the lamp learns it from the hand-overs themselves:
a `new` that started in a `~/REST` folder whose `--json` reply says the
agent works in `HOME/REST` (`agent.project.cwd`), for that machine and
`ok`, teaches `HOME` for that computer, kept while the part runs (no file;
the first hand-over after a restart goes as `~/…`). Only a plain absolute
home is taken: `/` first, no empty, `.` or `..` part, no `~` or control
character, the whole folder within 512 bytes. Each later `~/…` target on
that computer is then sent as `HOME/REST`. A `new` sent in a learned home
that fails forgets it, so the next goes as `~/…` again; nothing is asked
twice. The target, the stored launch and the person's yes keep the `~/…`
words: only the argument to `new` changes, as with a taken name above. An
absolute target and a project name are sent as they are, switch or not.
Without the switch, every folder goes as the target names it, as before.
For measuring it, each `new` says how long it took and how its folder was
sent, never the folder: `tasks: lamp-task-N: new answered in MS ms, the
folder sent KIND`, KIND one of `home-relative (~/), so Harness asks the
computer for its home`, `absolute, from the computer's home learned
earlier`, `absolute, as the target names it`, `as a project name` or `none
(a cloud agent)`.

**`serve --engines-ready`** (openharness#1788, off unless set): before
`new`, right after the `machines --json` preflight, the computer is asked
about the named engine alone, `harness machines engines MACHINE --engine
ENGINE --json` (Harness's `engines_probe`, over the machine link;
openharness#1888). An answer that refuses nothing is kept 5 minutes for
each computer and engine (`ENGINES_KEPT`), so later hand-overs ask nothing;
a refusal is never kept, so a person who signs in and asks again is asked
of afresh. Each ask is given at
most 8 s (`ENGINES_PROBE_TIMEOUT`; Harness reads the engine in a login shell
of 5 s at most there). Only a known no refuses:
- `firstPrompt: false` ends the task `not_sent` with `engine_no_prompt`,
  told with Task news' `failed_no_prompt` words: that computer can't start
  it with work yet, so trying again won't help until its software is
  updated;
- `signedIn: false` (read for Claude Code and Codex only) ends it `not_sent`
  with `engine_signed_out`, and the voice tells it with Task news'
  `failed_signed_out` words: the work didn't start, as the engine isn't
  signed in on their computer, and it has to be signed in there first.

`null`, no row for that computer and engine, a failed exit, a timeout, or an
older Harness without the command is no refusal, and is kept as long, so a
computer that can't say costs one ask. Hermes' `signedIn` is always `null`,
so a hand-over to Hermes is never refused by it. Devin Cloud is never asked:
it isn't one of Harness's engines there. The ask runs inside the launch,
under the part's one Harness lane, so a read for another task waits for it,
8 s at most. Each ask says how it went
in one journal line, never naming the computer: `tasks: asked the computer
about ENGINE: engine_signed_out | engine_no_prompt | no refusal | it could
not say | CODE, T ms`.

The engine allowlist is `codex`, `claude`, `opencode`, `devin`,
`devin-cloud` or `hermes` (Hermes since #3504, 27 Oct: Harness launches it
with its first prompt as `hermes chat -q`, with no permission modes, so it
is sent no `--mode`, as Devin is; Harness's other agents, Cursor, Copilot,
Grok, Amp, Kilo and the rest, take no first prompt there, so none can be
handed work; the store's `schema` is 4 only while it holds a Hermes task,
by Devin's rule, so a build from before sets it aside; OpenCode since task 41: Harness 0.4.2's `harness new --help`
names it, and its ask mode passes OpenCode no flag; see "Who gets the work";
Devin since #1854, see "Devin" below; Devin Cloud, see "Devin Cloud"). A
`devin-cloud` target's project is empty: it works in no folder. Otherwise a
project is an explicit name
(at most 128 UTF-8 bytes) or an absolute/`~/` folder (at most 512 bytes).
Leading option/target syntax, ambiguous relative folders, control characters
and whitespace around a project are refused. Text is at most 2,000 UTF-8
bytes (1,000 until #2813's made-skill repair needed room for a brief, the
request and the check's lines; Harness 0.4.57 took a 1,999-character
`--task` on a Linux laptop on 23 Oct), nonempty, without NUL and not beginning with `--`, which the inspected
CLI parser treats as another option. The confirmed text is never rewritten.

A name's folder is made by Harness on its first `new` (`~/harnesses/<name>`),
and every later `new` on that name is refused for it before anything starts:
"“NAME” already exists on COMPUTER: FOLDER. Choose another name …", FOLDER as
that computer reports it: `~/harnesses/NAME` from Harness 0.4.55 (Mobile on
lamp-0c89, 23 October), or its absolute path (openharness `newCommand.ts`,
for another machine). On that refusal alone, with a FOLDER that ends in that
name, `lamp-tasks` asks the same `new` once more in exactly that FOLDER, in what
is left of `new`'s one 60 s bound, and its journal says so (`tasks:
lamp-task-N: the name 'NAME' is taken by a folder of that name on that
computer, so it starts there`: never the folder itself, which may hold a
user name, and no ask's words). A name Harness turns into a
folder of another spelling (capitals, spaces) gets no retry, as before:
give such a target its folder. A folder target,
any other refusal, a timeout or a second refusal is never asked again,
so nothing can start twice, and a named target's hand-overs all start.

**`serve --new-folder`** (subdarren-P3 part 1, off unless set;
`--targets-from-machines` turns it on for the folder its targets name,
`~/harnesses/lamp-tasks`, alone, #5541): Claude Code
and Codex ask the person to trust a folder Harness didn't make ("Do you
trust this folder?"), and Harness pre-trusts only a folder it made itself
(`harness new --help`: "Claude Code and Codex ask the person to trust a
folder Harness did not make"). On lamp-c0a4 on 3 November the first task to
a new, empty `~/harnesses/lamp-tasks` waited at that question, read as
`needs-input`, until someone pressed it. With the switch, a folder target
of exactly `~/harnesses/<name>` (`<name>` of ASCII letters, digits, `.`,
`_` and `-`, no `.` or `-` at either end, at most 64 bytes: a name Harness
keeps as it is, so it makes exactly that folder) that this lamp hasn't
seen on that computer is sent as `--new <name>` in place of the folder
(`harness new claude --machine <id> --new lamp-tasks --name … --mode auto
--json --task …`), so Harness makes the folder and trusts it. Then:
- **made** (`new` succeeded, started or pending): the folder is marked
  seen;
- **there already** (Harness's `PROJECT_EXISTS`, made earlier by Harness or
  by hand: exit 1 and `{"ok":false,"error":"“NAME” already exists on
  COMPUTER: FOLDER. Choose another name …"}`, a sentence, read by " already
  exists on " alone): it is marked seen, and the same `new` is asked once more with the
  target's own folder in place of `--new <name>`, as without the switch, in
  what is left of `new`'s one 60 s bound (journal: `tasks: lamp-task-N: the
  folder for 'NAME' is on that computer already, so it starts there`). A
  folder already there and trusted, as on the test Mac, starts as before; one
  made by hand and never trusted still asks, as Harness never trusts it;
- **anything else** (another refusal, another exit, a timeout): nothing is
  marked and nothing asked again, so the next hand-over asks `--new` again.

Each first ask says so in the journal (`tasks: lamp-task-N: the folder for
'NAME' is new to this lamp on that computer, so Harness is asked to make
it`), after any `--route` has said new work. A marked folder's later
hand-overs name the folder, as before. The marks, machine id and folder,
are kept in `folders-seen.json` in the tasks folder (`--dir`), mode 0600,
written whole beside it and renamed over it, at most 256 (past that, the first
in sort order is dropped), so a restart doesn't ask `--new` again. They are only a
shortcut: a missing, unreadable or malformed file is read as none seen
(with a journal line), which costs one `--new` refused as there already. Any
other folder (`~/code/x`, an absolute path), a bare name (which Harness
makes on its own, above), a cloud target, or the switch off: the hand-over
goes exactly as before.

With `serve --absolute-folder` as well: a `--new <name>` names no folder,
so it goes as `--new <name>` whether or not the computer's home is known,
and its failure forgets no home; an `ok` one teaches the home as any `new`
does, its agent working in `HOME/harnesses/<name>`. The ask again after
`PROJECT_EXISTS`, and every later hand-over to a `~/…` folder on that
computer, send the absolute folder once the home is known, and a failed
one sent so forgets it.

Every hand-over says how it went in one journal line, whatever the outcome
(on everywhere, no switch): `tasks: lamp-task-N: dispatch STATE [CODE] (exit
N, T s, stderr: LAST)`. STATE is `accepted`, `pending`, `not_sent` or
`uncertain`, with its code (`new_failed`, `folder_missing`,
`engine_signed_out`, `timeout` …);
`exit` is the last Harness call's status, `no exit` when none ended (a
timeout, or refused before any call); LAST is stderr's last line with words,
at most 160 characters, with only what can't be the person's kept: quoted
spans go whole, and a word stays only if it is plain lowercase ASCII
(`error:`, `refused`) or an upper-case code (`SPAWN_FAILED`), is none of the
ask's words at any length, has no `/` or `@`, doesn't look secret, and is
none of the machines' names, hostnames or ids that the preflight's `machines`
listed, nor any part of one of two letters or more (split at spaces, `-`,
`_`, `.` and apostrophes, and again without a trailing `s`: `lan` of "Lan’s
MacBook Air" and of `lans-macbook-air`), nor any run of its dashed parts
(`lan-mbp` of `dev-lan-mbp`), nor a long run with digits (an id). `local`
is kept, since it names nobody. A name, a computer's label
(`lan-macbook` too), a folder or a word with an accent is `…`, one per run
(Fourteen's privacy read; their and the Reviewer-lead's P3 on #3234). The ask is never in it. Before (lamp-0c89, 23 October), an `uncertain` hand-over said
nothing in the journal, only in the task store.

The executable is resolved once to an absolute canonical regular file, owned
by the service user or root, executable, without group/other write or set-ID
bits. Its identity, size, modification time, mode and owner are checked again
before each invocation. Replacing it requires a service restart. This is not
a signature check or a sandbox for an untrusted executable.

The child gets `AI_AGENT=lampos` and `HARNESS_AGENT_SESSION=<persistent store
ID>`. It receives only explicitly allowed ordinary provisioning variables:
`HOME`, `PATH`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TMPDIR`, `TMP`, `TEMP`, `USER`,
`LOGNAME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`,
`XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`, `ADAPTER_DATA_DIR`,
`HARNESS_AUTH_DIR`, `PORT`. `HOME` is absolute and every PATH entry is
absolute. A system unit gives its process no `XDG_RUNTIME_DIR` or user bus,
yet Harness 0.5.42's `harness start` runs `systemctl --user start
harnessd.service`, which then fails ("Failed to connect to bus: No medium
found"; after a sign-in from the app on lamp-c0a4, 5 Nov, #5385). So each one
not already set is taken from the service user's own runtime folder,
`/run/user/<uid>`, and its `bus`, only where they exist
(`harness::with_user_bus`), looked at again for each call
(`Cli::child_environment`): after a reboot lamp-tasks can start before
logind has made the lingering `lampos` user's folder, and the next call
then finds it with no restart (#5393's reads). No inherited pane, Claude or Codex
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

### What the journal says about Harness calls

Every Harness CLI call the part makes says one journal line too, whatever
the outcome (on everywhere, no switch): the preflight `machines`, the
engine probe, `route`, `new`, Devin's `show` and `reply`, the `show` of a
read, `ps` for progress, the held `wait`, `close`, `auth status` and
`start`. So a slow or failed call can be found on a lamp, and matched to
Harness's own log on the computer (proposal L1 and L2). The hand-over's
`dispatch` line above stays as it is. The line is

`tasks: harness VERB: [machine=M] [agent=A | agents=N] exit=CODE ms=T
[out=B] [state=S] [error=E] [tools=K] [listed=L] [states=S:n,…] [ids=…]
[id=I] [creation=C] [message=G]`

- VERB is `new`, `show`, `ps`, `wait`, `reply`, `close`, `route`,
  `machines`, `engines` (`machines engines`), `auth-status` or `start`;
  any other is `other`.
- `machine=` and `agent=` are the ids the call was asked about, their first
  8 characters only (a cloud agent's keeps `cloud_` and the 8 after it);
  `agents=` is how many a `wait` waited on. They are read only from where
  the part put them (the slot after `--machine` and the one after it), never
  from the ask (`--task` for a `new`, after `--` for a `reply`).
- `exit=` is the CLI's exit status, `killed` when it ended on a signal, or
  the part's own code when it never answered: `timeout`, `cancelled`,
  `stdout_limit`, `spawn_failed`, `executable_changed` and the like. `ms=`
  is how long the call took; `out=` stdout's size in bytes, when it exited.
- From stdout only typed fields: `state=` the agent's or the send's state,
  a route's decision (`new`, `session`), `pending` for a `new` still being
  made, `signed-in`, `signed-out` or `offline` for `auth status`, else
  `ok` or `failed`; `error=` Harness's code (`SPAWN_FAILED`), or `?` for
  an error in words; `tools=` the agent's tool count (`show`); `listed=`
  how many harnesses (`ps`, `wait`) or machines (`machines`) it listed,
  with `states=` how many in each state; `ids=` the agents a `wait` came
  back for, four at most.
- The ids Harness gave, cut the same way: `id=` the agent's (`agent.id` of
  `new`, `id` of `show`, `reply` and `route`); and, only for a `new` or
  `reply` that exited 0 with `ok: true`, no error and no failed state,
  `creation=` a pending `new`'s and `message=` the message a `reply` queued.
  In any other reply `message` is Harness's own words, never journaled.

A state or code that isn't plain lowercase or upper-case ASCII is `?`. The
ask, a question, an answer, a progress line (`about`), a task or machine
name, a folder and Harness's words are never in it. For example:

```
tasks: harness new: machine=01234567 exit=0 ms=1840 out=212 state=ok id=11111111
tasks: harness reply: machine=01234567 agent=11111111 exit=0 ms=410 out=98 state=queued id=11111111 message=m1
tasks: harness show: machine=01234567 agent=11111111 exit=timeout ms=10002
tasks: harness ps: machine=01234567 exit=0 ms=380 out=1544 listed=3 states=idle:1,starting:1,working:1
tasks: harness wait: machine=01234567 agents=2 exit=0 ms=93120 out=88 listed=1 states=idle:1 ids=11111111
tasks: harness close: machine=01234567 agent=11111111 exit=1 ms=520 out=40 state=failed error=?
```

A `ps` is one line each 30 to 120 s per watched machine, and the held
`wait` one line each time it comes back (10 min at most).

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

## Closing a finished harness (#1390, `serve --close-told`, on in the product unit)

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
    - **Asked about** (`check_agent_task`): its answer carries the news's
      key once the agent has settled (`idle`, `needs-input`, `stopped` or
      `failed`; never `working`, nor `idle` with no answer). Only an `idle`
      answer that was told is ever closed: lamp-tasks' `close_due` leaves
      the others open. The provider worker takes the key out before the model is
      told and, once the model has the answer, hands to the coordinator
      (`NewsAnswered`) for that request. It rides on that turn like news
      told unasked, on the same proof. Told before or not: a telling unasked
      may have been cut short. Only the tasks part's own answer can carry a
      key.
      The second voice does the same (the first builder's 20:15 on #1722):
      its spoken turn's runner leaves the key in the answer
      (`tools::answer_keeping_news`), and the turn takes it out before its
      model reads the answer and sends `NewsAnswered` for that request
      (`second_provider`'s
      `news_asked_about_rides_on_its_turn_and_its_key_is_never_the_models`).
      Before this it put no news on its turns, so news asked about there left
      its harness open. A text from the app (`tools::answer_typed`) still
      takes the key out and keeps nothing.
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
  before closing (`Cli::observe_to_close`).
  - **Its own bound:** that read is given `CLOSE_READ_TIMEOUT`, 45 s (no
    more than one `launch_timeout`), not the 10 s `read_timeout` every other
    read keeps. On a lamp on 3 Nov, `show` against the person's Mac took 16
    to 39 s: every read before a close ran out at 10 s, and no harness was
    closed, so they stayed open on the Mac.
  - **One budget:** the read and `harness close` share `CLOSE_BUDGET`, 60 s;
    the close is given what the read left (`Cli::close_within`), never more
    than one `launch_timeout`. With nothing left it starts no process and
    fails, code `close_no_time`. So the job lane is held no longer than one
    launch, and a hand-over the person just said yes to waits behind a
    close no longer than behind another launch.
  - **Never skipped:** an earlier idle read (a held `wait`, `ps` or the read
    that was told) never stands in for it. The person may have typed into
    the harness since; only this read says it is still the told turn.
  - **A read that fails is no idle agent:** its time run out, or any other
    failure, closes nothing. The close fails with the read's code prefixed
    `observe_` (`observe_timeout`), and is tried again like a failed close
    (below).
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
- **Not followed after:** once closed (`closed`), the watch's follow looks
  (`WATCH_FOLLOW_MS`) stop for that task. Before, a look two minutes later
  found the stop the lamp itself had made, kept it as a `stopped` answer,
  and the voice told the task's news a second time (the #4342 lamp run on
  lamp-c0a4, 3 Nov: news at 13:26:45, closed at 13:27:15, news again at
  13:28:51). A close refused, left to the person or failed keeps the task
  followed. Test: `a_harness_closed_after_its_news_is_not_followed_or_told_again`
  (`engine.rs`).
- **Recorded first:** the attempt is saved before the call, like a launch and
  a read.
- **Exit codes:** Harness 0.4.18's `close` exits 0 (closed), 5 (refused:
  `NOT_YOURS`, `SPAWN_LIMIT` or `TURNED_OFF`, never asked again) or 1. Exit 1
  covers working or waiting for the person (`WORKING`), changed while closing
  (`AGENT_CHANGED`) and gone alike, with only a message in the `--json`
  reply. So exit 1 is `failed`: tried again a minute later (`CLOSE_RETRY_MS`),
  three times in all (`CLOSE_TRIES`), each try reading the agent first. A
  read that fails or runs out (`observe_<code>`), a close that runs out
  (`timeout`) and `close_no_time` are `failed` the same way. After the third
  failed try the harness is left open, never asked again.
- **Restarts:** a restart during a close leaves it `failed`, code `restart`,
  to be tried again the same way.
- **The journal:** each close prints one line, and `get_task` shows the
  task's `close` (its `state`, `tries` and `code`):
  - `tasks: closing task N's harness: try T: Closed` (or `Gone`, `Left`,
    `Refused`);
  - `tasks: closing task N's harness: try T of 3 failed (CODE); tried again
    in a minute, read first`;
  - on the third: `… try 3 of 3 failed (CODE); giving up, the harness is
    left open`.
- **Reverting:** a `lamp-tasks` from before #1505 is `deny_unknown_fields`
  on its stored state, so it can't load a store with `close` or `closable` in
  it, nor a store with an observation `told` (written only with
  `--close-told` on). Going back that far on a lamp means resetting its
  tasks store, as with `watch_idle`. From #1505 on, a build reads one
  release forward (below).
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
- **`runtime::tests`:**
  - `a_close_reads_first_and_closes_only_an_idle_agent_on_the_told_turn`
    (the read before a close is its own, never the 10 s one; a failed read
    closes nothing, `observe_<code>`; only idle on the told turn closes,
    within what is left of `CLOSE_BUDGET`);
  - `a_close_journal_line_names_its_try_and_the_give_up`.
- **`tests/runtime.rs`, through the service:**
  - `a_harness_done_and_told_is_closed_once_by_the_service` (the person's
    own idle harness listed by `ps` beside it is never closed);
  - `without_close_told_no_harness_is_closed`;
  - `a_harness_the_person_went_on_with_or_that_waits_for_them_is_left`;
  - `a_show_slower_than_the_read_bound_still_ends_in_a_close` (every
    ordinary read runs out and the harness is still closed; one the slow
    read finds waiting for the person is left);
  - `a_close_whose_read_runs_out_is_tried_again_read_first`;
  - `a_close_whose_read_always_runs_out_gives_up_after_its_tries` (three
    reads, a minute apart, nothing closed, then never again).
- **`tests/harness.rs`, on the fixture CLI:**
  `close_names_the_agent_on_its_machine_never_now_and_reads_the_exit_code`;
  `the_read_before_a_close_is_given_longer_than_any_other_read` (scaled
  down: a `show` slower than `read_timeout` reads through before a close,
  and one slower than the close's own bound still runs out);
  `a_close_is_given_only_what_is_left_of_its_budget`.
- **The voice's `coordinator::tests::say`, the turn's proof:**
  `news_heard_whole_is_handed_on_as_the_turn_ends`,
  `news_cut_short_is_never_handed_on`,
  `news_not_heard_whole_is_never_handed_on` (no audio, a playback gap).
- **The asked path:** `agent_tasks`'
  `an_answer_with_finished_news_carries_its_key_for_the_worker_alone`;
  `the_key_is_never_the_models_on_either_voice` (the Live worker's asking
  thread and the second voice, through `Route::Tasks`); the
  provider worker's `an_answer_with_news_hands_its_key_on_once_it_is_sent`;
  `coordinator::tests::say::news_answered_to_a_question_rides_on_its_own_turn`.
- **The voice's `tests/it/news_told.rs`, with a real `lamp-tasks` on its
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
| Text | Task: 2,000 bytes; selected speech: 1,000 bytes; observed ask: 4 KiB; answer: 16 KiB |
| Local transport | 8 KiB request; 128 KiB response, enough for escaped bounded observation text |
| Clients | 4 workers, 4 queued sockets, 16 queued owner requests |
| Client deadlines | 300 ms complete request, 1,100 ms owner receipt, 1,200 ms total server service |
| CLI | One worker, one job/result slot; preflight/read 10 s each, new 60 s (a name-taken retry included) |
| CLI capture | 64 KiB stdout, 16 KiB stderr; individual poll waits capped at 10 ms |
| Progress (`--progress`) | One `ps` per machine per watch period, 10 s each; 256 KiB stdout; 256 harnesses a reply; a progress line cut to 2 KiB; memory only, at most 16 tasks |
| Tasks now (`tasks_now`, with `--progress`) | Memory only; at most 8 tasks, newest first; the reply under 2 KiB (body at most 1,900 bytes); a last step at most 160 characters, cut at a whole character, control characters removed; a settled task kept 30 min after it was told and at most 2 h after it settled; no Harness call of its own |
| Follow (`--push-watch`) | One `watch` per watched task, at most 4 at once (`MOST_FOLLOWED`), each on its own thread and its own adapter beside the CLI worker, 600 s each (killed at 630 s) and started again at once; 64 KiB a line, each line dropped once read; killed on `done`/`failed`, when its task leaves the watch, and when the part stops (the owner joins every one before it ends); a failed one started again after 3, then 10 s, given up for its task after 3 in a row; a task settled by it not followed again for 3 s |
| Push (`--push-wait`) | One `wait` held per machine with watched tasks, on at most 64 agents, 600 s each (killed at 630 s), beside the CLI worker; the next on a machine no sooner than 3 s after one came back with an end, 30 s after one failed; a pushed task left out until the watch reads it again; 64 KiB stdout |
| Private persistence | 8 MiB snapshot, 32 MiB database, 40 MiB journal; 4,096-byte pages |
| SQLite | DELETE journal, FULL sync, fullfsync requested, 2 MiB cache, 50 ms busy wait, no mmap/WAL |
| Diagnostics | Once a second (`report_every`; tests pass less), with early readiness changes; owner stale after 1,500 ms |

There are seven service threads including main: four clients, the storage
owner and the CLI worker; with `--push-wait` (the product's), one
`tasks-wait` thread more for each machine; with `--push-watch`, one
`tasks-follow` thread more for each followed task, four at most, and sign-in's reader threads
while one runs. Clients never run Harness. The CLI worker never
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

**Reading one release forward (#1505).** On lamp-4ace on 18 October at
13:51 every `lamp-tasks` build before #1425 refused the store #1425's build
had written: its observations had a `told`, and its last receipt was a
`told` request, an `op` the older builds don't have. An install, and the
hand rollback after it, stopped there, and hand-overs with them. A build
now reads the state of the release after it:

- **A field it doesn't know is passed over**, on the stored types: the
  state, a task, its dispatch, read, observations, speech, close and audits.
  The state is decoded from its stored bytes, never through a JSON value, so
  a field written twice is still refused, not read with the last of the two.
  What a build writes is only what
  it knows, so if it saves that state, the newer fields are dropped, as if
  that release had never run. For #1425's `told`, a task told under it is
  then not closed by the newer build after a roll forward; its harness stays
  open, which is the safe side. **Strict still:** a task's target, being
  the targets file's type too; its launch, under `prepared_digest`, the
  person's yes to that exact task, so a later field that narrows a launch is
  never passed over to run a wider one; an observation's data, under its
  `digest`; and the requests (Jarvis on #1512).
- **A last receipt it can't read is dropped**, never refused, with one
  journal line naming the request's `op` only, if it's a plain name, and
  `unknown` otherwise: `tasks: the last receipt is not one this build reads
  (op a_later_op); dropped`. So is a receipt it reads but that doesn't hold
  for it, as a later release's reply with a field added to it would be (the
  reviewer on #1512). It only answers an exact retry of that very request,
  which a restart refuses anyway (`stale_session` for the voice's trusted
  requests, `conflict` for the rest).
- **A newer `schema`** is a change no older build may read (the rule
  below says which changes those are). A build
  that finds a `schema` above its own (4) keeps the store as it is,
  renamed `tasks.sqlite3.newer-<schema>-<unix seconds>` in the same folder
  (Harness's `HOME` beside it is not touched), and starts with an empty
  store and a new store id, with one line: `tasks: storage was written by a
  newer build (schema N, this build reads 4); it is set aside as …, and
  this build starts with an empty store`. The tasks it was following are
  forgotten by the lamp; their harnesses go on in Harness. The kept file is
  whole, so the newer build can be put back on it by hand.
- **The rule for a change to the stored state:** these bump `SCHEMA` in
  `engine.rs`, in the same PR:
  - a new value in a stored enum, or a new required field;
  - a new field on a target, a launch or an observation's data;
  - a larger `MAX_STATE_BYTES`: an older build refuses a state over its own
    bound before it reads `schema`, so it can only set aside what it can
    read;
  - an optional field whose absence would let an older build launch, close
    or speak more than the newer one would, such as "never close this one".

  Any other new optional field (`#[serde(default)]`) needs nothing more.
- **Said until someone looks** (#1505, the first builder's addition). While
  a kept `tasks.sqlite3.newer-*` file is in the store's folder, the `tasks`
  diagnostic is a warning, "Tasks set aside a store from a newer build;
  its tasks are not followed", with `set_aside` naming the newest. It's
  read at open and again once a minute, so it holds across restarts and
  ends within a minute of the file being removed or put back. Health and
  the self-test show it as any part's warning. Nothing clears it on its
  own.
- **Task targets written for a newer build** (`targets.json` with a
  `version` above 1, or a field this build doesn't know) stay strict: a
  later target field may narrow what a task may do, so it's never passed
  over. But `lamp-tasks` no longer stops on them, since a stopped part says
  nothing: it runs with **hand-overs off**. Every `prepare` is refused,
  code `targets_newer`, and nothing reaches Harness. The voice tells the
  model `agent_tasks::TARGETS_NEWER`, that hand-overs work again once the
  lamp is updated, not "just now" (Eighth's P3 on #3131). The `tasks` warning
  says "The task targets were written for a newer build (version N);
  hand-overs are off until they match this build", every second, with
  `targets_newer`. A version-1 file with a field or an engine this build
  doesn't know says so instead ("from a newer build, or mistyped"), not a
  version. Nothing is set aside or rewritten, since a person or Harness's
  pairing wrote the file. A file that isn't a targets file at all still
  stops the part, as before. Hand-overs off isn't the part off: tasks
  handed over before are still watched, told and, with `--close-told`,
  closed, each by the target it was stored with.
- **The warnings' order:** not ready, then bus failures, then targets for
  a newer build, then (with `--set-up-later`, below) why hand-overs are
  off and a targets file that isn't valid, then a store set aside, then
  the last hand-over failed.

Tests: `engine::tests::the_next_releases_state_opens_and_keeps_its_tasks`
(a field this build doesn't know on every stored type, a close under way,
and a receipt for a later request; what this build knows of the task kept
when it saves again; the target refused),
`engine::tests::a_receipt_that_does_not_hold_is_dropped_and_the_store_opens`,
`engine::tests::a_store_kept_aside_is_in_the_summary_until_it_is_gone`,
`engine::tests::with_targets_for_a_newer_build_a_task_offered_is_refused`,
`runtime::tests::targets_for_a_newer_build_are_told_apart_and_never_read`,
`tests/runtime.rs`: `targets_for_a_newer_build_keep_the_part_running_with_hand_overs_off`,
`a_store_set_aside_is_said_until_its_file_is_gone`,
`engine::tests::a_dropped_receipt_names_only_a_plain_op`,
`engine::tests::a_newer_schema_is_set_aside_whole_and_a_new_store_begins`,
`engine::tests::a_request_from_a_later_release_is_still_refused`,
`store::tests::a_store_set_aside_is_kept_whole_and_a_new_one_starts_empty`.

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

No history garbage collector or archive operation is implemented; a told
harness is closed with `--close-told` (above).
Capacity stops admission rather than forgetting ambiguous work or resetting
speech history. Account, local Harness identity, operator target, store reset
and personal-data erasure need First's explicit reconciliation/reset wiring.
The private database contains task and answer text; it belongs in that reset
manifest. Deleting the database is not an ordinary retry strategy.

## Before the lamp is set up for hand-overs (`--set-up-later`, #1676)

A home lamp gets Harness, its sign-in and the person's computers after the
tasks part is installed. With lamp-update's `LAMP_HARNESS_AT_ONBOARDING=on`
(#5197, off unless set; on a product lamp with it unset, on by the build's
default), Harness itself is put there with no hand after the
first update round that reaches the network, installed but not running, and
lamp-tasks is started then; the app reads that install on `update.status`
(`docs/install.md`, "Harness before the sign-in"). Without the switch, the part stops at start with
no `targets.json` or no program at `--harness`, and systemd restarts it
again and again, saying nothing useful. `lamp-tasks serve --set-up-later`
(off unless set; set in the product unit, `deploy/product/lamp-tasks.service`,
with `--targets-from-machines claude`, since 3 Nov) lets it run, and says why
hand-overs are off. On lamp-c0a4 on 3 Nov, without it, `lamp-update
harness-on` left the part failing at every start (`tasks: No such file or
directory (os error 2)`) until systemd stopped restarting it. Every file
error at start names its path, e.g. `tasks:
/var/lib/lampos-tasks/targets.json: No such file or directory (os error 2)`.
The product unit's test is `the_tasks_part_starts_with_no_targets_and_learns_them_from_the_machines`
(`crates/update/tests/install.rs`):

- **No targets file:** the part starts with no target. Every `prepare` is
  refused with `target`, as for an alias it doesn't have, and
  `{"op":"targets"}` lists none.
- **The targets file is read again when it changes.** Once a minute
  (`SET_UP_LOOK_EVERY`) the part compares the file's device, inode, size
  and modification time with the last read. When they differ, it reads
  the file with the same strictness as at start:
  - **valid:** the next hand-over uses it. A task handed over before keeps
    the target it was stored with;
  - **for a newer build:** hand-overs off, as above (`targets_newer`);
  - **gone:** no target again;
  - **not valid:** the targets before it are kept, and the `tasks`
    diagnostic warns "The task targets file changed and is not valid; the
    targets before it are kept", with `targets_invalid`.

  The journal has one line a change, with how many targets and never their
  names, since an alias may be a computer's name. A computer added after
  sign-in is used within a minute, with no restart.
- **No Harness program** at `--harness` (not there at all; one there and
  not fit to run is refused as before): the part runs with a stand-in that
  sends nothing. Every `prepare` is refused, code `no_harness`, and the voice
  tells the model `agent_tasks::NO_HARNESS`, that hand-overs aren't set up
  on this lamp and can be set up later. Any other refusal is "could not be
  prepared just now", which the model said as a retry to offer on lamp-75c9
  (#2842). Harness put on later is used once the part starts again.
- **The sign-in** is read once a minute with `harness auth status --json`
  (one line, `loggedIn`, and `offline` when signed in but its sign-in
  couldn't be renewed just now). It goes on the Harness worker after
  launches, reads, closes and progress, so it never sits on a request's
  path. One failed read keeps what was known.
- **Health:** the `tasks` diagnostic gives `handovers_off` and a message:

  | `handovers_off` | Message |
  |---|---|
  | `no_harness` | Harness is not installed on this lamp; hand-overs are off until it is |
  | `no_computer` | No computer to hand tasks to yet: the task targets are missing or empty |
  | `computers_need_sign_in` | Harness is not signed in: tasks go to the lamp's own agent; sign in to hand tasks to your computers |
  | `not_signed_in` | Harness is not signed in to the person's account; hand-overs fail until it is |

  Their level is ok: a lamp not yet set up isn't unwell. With the sign-in
  read, `signed_in` is `yes`, `offline` or `no`. `computers_need_sign_in`
  is said in place of `not_signed_in` while the lamp's own target names
  this computer's own id (signed out, #6407, "The lamp itself, signed
  out" below): work goes to the lamp, and only the computers need the
  sign-in. Targets for a newer build
  keep their own warning. Being signed out doesn't refuse a `prepare`: the
  sign-in read is up to a minute old, and the hand-over then fails as
  before ("could not hand the last task over").
- **The sign-in on `status`** (#5488): once read, the `status` answer
  carries the same `signed_in` (`yes`, `offline` or `no`), on any route.
  The voice's onboarding reads it for its one sentence about Harness in the
  walk's closing (`LAMP_ONBOARDING_HARNESS_LINE`, [onboarding.md](onboarding.md)).
  Not read yet, the answer has no `signed_in`, as before; every other
  answer is unchanged (`runtime::tests::status_says_harness_signed_in_once_read`).
  One failed read (`harness auth status` not answering) keeps the last
  answer; three failed reads in a row (`SIGN_IN_FAILED_READS`, three looks,
  about three minutes on a lamp) let it go, journal line "Harness's sign-in
  not read 3 times in a row: not known until it is read again", so `status`,
  the app's `app_sign_in_state` (#5524) and the report carry no `signed_in`
  (not known) and the machines are not
  asked, until a read answers again; before, a sign-out followed by failing
  reads kept `yes` until a restart. Harness's daemon is still started only
  after the first read of a run that answers (#5379)
  (`runtime::tests::a_sign_in_that_cannot_be_read_for_three_looks_is_let_go`,
  and through the part's own loop
  `tests/runtime.rs`: `a_sign_in_read_failing_three_times_in_a_row_is_let_go_by_the_part`).
- **Without the switch** nothing of this is said or read, and the part
  starts, stops and reports as before.

- **Targets written from the person's machines** (`--targets-from-machines
  ENGINE`, only with `--set-up-later`; off unless set, #1676 B6 step 3; the
  product unit sets `--targets-from-machines claude`, so each of the
  person's computers is a target for Claude Code).
  While Harness is signed in (`signed_in` `yes`, not `offline`), every five
  looks (five minutes on a lamp, after everything else on the Harness
  worker), the part runs `harness machines --json` and writes
  `targets.json` from it:
  - **which:** every machine but the lamp itself (Harness's `current`), at
    most 8, newest first as Harness lists them;
  - **aliases:** the first is `office`, the alias the voice asks for unless
    `LAMP_TASKS_TARGET` names another, so today's voice finds it with no
    change. Each other is its Harness name (else its hostname) made into an
    alias: lower case letters and digits, `-` between words, at most 32
    bytes (`MacBook Air` is `macbook-air`), `computer` with nothing left,
    and `-2`, `-3` … where two would be the same;
  - **each target:** the machine's id, the folder `~/harnesses/lamp-tasks`
    (lamp-4ace's today; a folder, not the name `lamp-tasks`, which Harness
    would refuse from the second hand-over on. Nothing else makes it, so
    the first hand-over to each computer asks Harness to make it, as
    `--new lamp-tasks`, as `serve --new-folder` does but for this folder
    alone unless `--new-folder` is set (#5541; before it, every first
    hand-over to a fresh computer failed `CWD_NOT_FOUND`, lamp-a63e on 6
    Nov). A refusal for it is said with `--folder-missing`) and
    `ENGINE`
    (`claude`, `codex` or `opencode`); with `--engines-from-machines`, one
    that computer has (openharness#1888 answers #1410's question 2; see
    "Which agents each computer has" below).

  **Never over someone else's file.** The part writes only where there's
  no file, or the file is one it wrote: `targets.json.from-machines`
  beside it holds the sha256 of what it wrote. A file a person or a
  pairing wrote is left as it is, and the journal says so once. The mark
  is written before the file and names both the old and the new, so a stop
  between the two never makes the part's own file look like someone
  else's. Each write goes beside the file, synced, then renamed over it,
  0600. With no machine but the lamp, the part removes its own file: no
  computer again. The file is read back by the look above, at once, with
  the same strictness, and a task already handed over keeps its target.
  One journal line a change, with how many targets and never their names.
- **The computers' own names** (#1676, the first builder at 14:12). Beside
  the targets it writes, the part writes `targets.names.json`:
  `{"version":1,"names":{ALIAS:NAME}}`, each name the machine's Harness
  name (else hostname), control characters out, trimmed, at most 64 bytes.
  None where nothing is left. A computer renamed keeps its alias and gets
  its new name. With no computer the file goes with the targets, and
  beside a file someone else wrote nothing is written.
  - **Its own file,** not a field of `targets.json`: an older build never
    reads it, so a rollback keeps its hand-overs, where a field it didn't
    know would turn them off (#1505). Every store reads one release back
    (El's #1680 rule), and a label shouldn't cost a lamp its hand-overs.
  - **Read at each look,** as strictly as the targets: an owned 0600
    file, version 1, nothing else in it, each alias of a target alias's
    shape and once, each name as above. A missing or bad file is no names.
    The journal says how many, never which.
  - **Told:** `{"op":"targets"}` gives `name` beside `alias` and `engine`
    for a target that has one.
  - **Said, with `LAMP_TASKS_MACHINE_NAMES=on`** (off unless set): the
    voice reads the targets (as with `LAMP_TASKS_WHO`) and keeps the names
    by alias. The read-back's `where` names the computer the work was
    prepared for, "Claude Code on your MacBook Air", and a single task's
    news says the rest of its answer, or that its agent waits, is "on
    their MacBook Air". With no name for that target, or without the
    switch, both read as today ("the office computer", "their computer").
    The descriptions and the session note are not changed: the longest
    description is at 1,014 of the service's 1,024 bytes, and the computer
    there isn't yet chosen. Nor are news of several tasks at once, which
    may be on several computers, and `check_agent_task`'s.
  - **Tests:** `runtime::tests::names_from_machines_are_clean_labels_by_alias`,
    `runtime::tests::the_names_file_is_read_strictly`,
    `runtime::tests::names_are_written_beside_the_parts_own_targets`,
    `tests/runtime.rs`: `targets_from_machines_are_written_and_used_while_signed_in`
    (the names told, and a rename), the voice's
    `agent_tasks::tests::with_machine_names_each_computer_is_said_by_its_own_name`
    (two computers, one with none, the switch off; a read-back that
    ignores the names fails it) and
    `agent_tasks::tests::the_machine_names_switch_is_taken_on_its_own`.
- **Which agents each computer has** (`--engines-from-machines`, only with
  `--targets-from-machines`; off unless set, #5199). Without it every
  target written from the machines gets `ENGINE`, and a hand-over to an
  agent the computer lacks, one not signed in there, or a computer that is
  asleep fails only after the person's yes, on the computer.
  - **Asked:** after each look at the machines, each computer listed (the
    lamp left out, at most 8) and each a target names is asked `harness
    machines engines <id> --json` (openharness#1888), which probes it live
    over its link (Harness's own 30 s bound; the part allows 35 s). One
    computer at a time, on a thread and a Harness lane of their own: a
    computer that takes its 30 s never holds up a hand-over or a read.
    Exit 1 is `unreachable`, 2 `usage` (an older Harness). One journal line
    a look, each engine and what it said, never the computer's name.
  - **Kept**, by machine: its `status` from `harness machines --json`
    (`running`, which is online, `offline`, `unknown` or
    `payment_pending`; any other word is unknown; its `engine` field is
    legacy and never read), and each engine's `installed`, `installable` and
    `signedIn`, each with when it was read. Harness's CLI turns the
    computer's `installed: false` into `null` and passes `installable`
    through (the computer's `!installed && command && recipe`); `signedIn`
    is read for Claude Code and Codex only. A look that failed keeps what
    was known. `null` is unknown, never missing: it forgets a value that
    would refuse (that may no longer hold) and keeps one that wouldn't, so
    one shell that timed out doesn't move a target to another engine. A
    reading more than 15 minutes old is unknown.
  - **Targets from what is there:** a target written from the machines
    gets `ENGINE` where it is known installed on that computer, else the
    first known installed of claude, codex and opencode, else `ENGINE` as
    before. A file someone else wrote is still never written over.
  - **Its own file,** `targets.engines.json` beside the targets:
    `{"version":1,"computers":{ALIAS:{"status":{"value":WORD,"read_ms":MS},
    "engines":{ENGINE:{"installed":{"value":BOOL,"read_ms":MS},
    "installable":{…},"signed_in":{…}}}}}}`, each field only when known, for the targets the
    file names now, whoever wrote it. An older build never reads it, as with
    the names. Read at each look as strictly as the names (owned, 0600,
    version 1, nothing else in it); a missing or bad file is nothing known,
    and refuses nothing. A file there that isn't this part's is left as it
    is. Nothing known: the file is removed.
  - **Refused at `prepare`,** at once, with nothing made: the computer known
    `offline` is `computer_offline`; the engine not found there
    (`installed` null with `installable` true, as Harness prints an engine
    a computer lacks) is `engine_missing`; known `signedIn: false` (Claude
    Code and Codex only) is `engine_not_signed_in`, in that order. Unknown
    always proceeds, as today: `installed` null with `installable` false
    goes, and Harness's own `ENGINE_NOT_INSTALLED` decides. A computer
    whose shell took over 5 s reads as not found too, so it is said as
    "not found", never as "not installed". Devin Cloud, which works in
    Devin's cloud, is never refused. A refusal has the machines and their
    engines looked at again at once (at most once in 30 s), so a computer
    woken or an agent signed in since is known by the next ask.
  - **Said:** the voice tells the model one plain line a code, naming the
    agent and the computer as the read-back does ("your MacBook" with
    `LAMP_TASKS_MACHINE_NAMES`, else "the office computer"): "Codex wasn't
    found on your MacBook.", "Your MacBook seems to be asleep or offline.",
    "Claude Code isn't signed in on your MacBook.", each with "Nothing was
    handed over." and what would let it go, never "just now".
  - **The staff:** `{"op":"targets"}` gives `installed: false` beside a
    target whose engine wasn't found on that computer, and the voice leaves
    it out of the staff, so the descriptions and the staff's note name only
    agents some computer has. With the engine chosen above, a target
    written from the machines names one its computer has.
  - **Without the switch** nothing is asked, kept, read or refused: an
    engines file beside the targets is never read.
  - **Tests:** `readiness::tests` (a null is unknown, each refusal and
    unknown proceeds, the engine chosen, the file written, read back and
    read strictly), `tests/harness.rs`:
    `a_computers_engines_are_one_call_read_as_harness_gives_them`,
    `runtime::tests::targets_from_machines_take_the_engine_each_computer_has`,
    `tests/runtime.rs`: `engines_from_machines_choose_an_engine_there_and_refuse_early`
    (a failed look keeps the old), `an_engine_known_not_there_is_refused_and_told_and_unknown_proceeds`,
    `without_engines_from_machines_nothing_is_asked_or_refused`,
    `targets_from_machines_needs_an_engine_and_set_up_later`, the voice's
    `agent_tasks::tests::an_early_refusal_is_one_plain_line_a_code`,
    `agent_tasks::tests::an_agent_no_computer_has_is_not_one_of_the_staff`
    and `agent_tasks::tests::a_target_the_tasks_part_lacks_is_answered_from_who_is_there`.
  - **Not yet shown on a lamp:** a played before/after count on lamp-c0a4,
    hand-overs to a missing, a signed-out and an offline agent each refused
    at once with a plain line, and a good one unchanged.
- **The lamp itself, with Hermes** (`LAMP_HERMES_ON_LAMP=on`, only with
  `--engines-from-machines`; off unless set, #5978 part 3; the product's
  `lamp-tasks.service` sets both). Off, the
  machine Harness marks `current` (the lamp) is left out, as above.
  - **Kept as a target** only while its own engines look says `hermes` is
    `installed: true` (fresh, as above): with the switch the lamp's machine
    is looked at too. The target is `lamp`, engine Hermes only, folder
    `~/harnesses/lamp-tasks/lamp` under `lampos`'s home (the part makes it,
    0700, since Harness makes only a `~/harnesses/<name>` folder), with no
    name. With it, at most 7
    computers are kept, so the file keeps its 8; a computer whose name
    makes `lamp` is `lamp-2`. With no computer, the file holds the lamp
    alone instead of being removed. Hermes missing, not looked at, or the
    switch off: left out, as before. Without `--engines-from-machines` the
    part says once that the lamp is no target while signed in.
  - **The lamp itself, signed out** (#6407). A product lamp never signed
    in, Hermes 0.22.2 installed, answered "I can't reach Hermes": the
    target above came only from `harness machines`, which works only signed
    in, and the part said `Harness signed in: no` and `no engines file`.
    Harness 0.6 signed out refuses `machines` and every `--machine`, while
    `start` and `new` with none work on this computer, under its own id
    (`computerId`, a UUID, which `auth status --json` names signed out too;
    an account machine's id is 32 hex digits). So, with a lamp engine's
    switch on (`LAMP_OPENCLAW_ON_LAMP` or `LAMP_HERMES_ON_LAMP`) and
    `--targets-from-machines`:
    - **The target from the lamp itself.** At each sign-in read that says
      signed out and names this computer's id, the lamp's engine is
      installed here when its program is where Harness finds it,
      `/usr/local/bin/<engine>` (`LAMP_ENGINES_BIN`; `hermes` and
      `openclaw` are lamp-update's wrappers), executable. Then the file is written, as
      from the machines and never over a file the part didn't write, with
      the lamp alone: `lamp`, that engine, `~/harnesses/lamp-tasks/lamp`
      (made, with its guidance), under this computer's id. A computer's
      targets written before go: none can be reached signed out. Not
      installed here, a target written so before is removed; any other
      file is left as it is. The engine is the first in `lamp_order` (the
      lamp's engines switched on, in `LAMP_LAMP_ENGINE`'s order, OpenClaw
      first unless set, OpenClaw left out while the Harness here is too old
      for it) that is installed here; neither engine is named anywhere else.
      One journal line a change.
    - **No machines look and no `--machine`.** A target naming this
      computer's id is started with no `machines --json` preflight (and no
      `machines engines` with `--engines-ready`), and every call for its
      work (`new`, `show`, `ps`, `wait`, `watch`, `reply`, `resume`,
      `close`, `route`) leaves `--machine` out; the journal's call line
      still names the machine. `new`'s `machineId` must still be that id.
    - **The daemon at boot.** A part whose first sign-in read says signed
      out starts Harness's daemon too, while a lamp engine's switch is on
      (below).
    - **Signed in again**, the machines write the targets, as above, and the
      lamp's target names the account's machine. Work started under the
      account's machine id stays readable only signed in (its calls name
      it with `--machine`); the computers keep needing the sign-in.
    - **What is said.** The health line says `computers_need_sign_in`
      (above), and the voice's `harness_status` says only the computers
      need the sign-in (below).
    - **Tests:** `a_lamp_never_signed_in_hands_work_to_its_own_engine`
      (`tests/runtime.rs`),
      `the_lamps_own_target_signed_out_is_asked_with_no_machines_look_and_no_machine`
      and `signed_out_the_computers_own_id_is_read_with_the_sign_in`
      (`tests/harness.rs`).
    - **Not yet shown on a lamp:** signed out, "ask Hermes …" answered and
      a follow-up reaching the same session.
  - **While Hermes is still installing.** With the switch on and no lamp
    target yet (Hermes not installed; lamp-update installs it later in a
    quiet window), the `targets` answer adds `"lamp":"installing"`;
    otherwise it is as before. A hand-over the voice asks for then finds no
    target of its engine and the model is told the lamp's own helper is
    still being installed (`LAMP_INSTALLING`), never that it needs setting
    up on a computer; nothing is prepared or kept waiting. A computer target
    of that engine still takes the work, as before. Tests:
    `engine::tests::the_targets_say_the_lamp_is_installing_only_with_the_switch_on_and_no_lamp_hermes`
    and
    `agent_tasks::tests::a_lamp_whose_hermes_is_installing_says_so_instead_of_asking_for_a_computer`.
  - **Hermes' home.** Hermes keeps its settings in
    `HERMES_HOME=/var/lib/lampos-hermes/.hermes`, outside `lampos`'s home,
    apart from Harness' sign-in and lamp-cloud's credentials. Hermes saves
    its shell environment there (`cache/scratch`), so nothing lamp-tasks
    gives it carries a key: only the forwarder's local token. Which user
    Hermes runs as is part 1's (install) and still being settled. At
    start, with the switch on, the part (as `lampos`) writes Hermes'
    settings there for the lamp's DeepSeek forwarder (port 7741) with the
    token from `/var/lib/lampos-cloud/hermes-token`, and says one journal
    line: written, unchanged, or "not written: no forwarder token yet",
    when the token is missing (then nothing is written).
  - **The lamp's skills for Hermes.** With the switch on, Hermes acts for
    the lamp's voice, so it sees the lamp's own skills and can run them:
    - **Read in place.** The same settings merge adds
      `/var/lib/lampOS/skills` (the folder lamp-skills reads) to Hermes'
      `skills.external_dirs`, so Hermes lists the lamp's skills as
      external, read-only skills. They are never copied into
      `HERMES_HOME`. The merge keeps every other key under `skills:` and
      every dir a person added there, adds the lamp's only when it is not
      there already (a trailing `/` or quotes count as there), and leaves
      Harness's `hooks:` block as it is. A flow list (`[a, b]`) or a single
      path becomes a block list of the same items and the lamp's; a value a
      reset left (`{}`, `[]`, `null`) takes the block. Anything else there
      (a nested item, an anchor, a block scalar) refuses the whole merge,
      as above, and the file is left as it is.
    - **The guidance.** Each time the part keeps the lamp as a target, it
      writes `AGENTS.md` in the lamp's work folder
      (`~/harnesses/lamp-tasks/lamp`), which Hermes reads from its working
      folder as the project's context; never a file under `HERMES_HOME`.
      It says where the skills are and how to run one, and the owner's
      rules: run a skill only when the person's task asks for it, never
      move, light up or play sounds unprompted; if the lamp's skills
      refuse, say why and don't retry around it; never edit or copy the
      skills folder; a new skill goes in the work folder, and installing
      it is the person's step. The file is lampOS's: it is written, 0600,
      only when what is there differs (a hand edit is written over at the
      next look), never through a link, and a work folder that is a link
      is refused. One journal line when it is written or not written.
    - **Calling a skill's tool.** `lamp-tasks skill list` prints the tools
      in `/run/lampos-skills/tools.json` as one JSON line
      (`{"ok":true,"tools":[...]}`). `lamp-tasks skill call NAME
      ['{"key":"value"}']` sends the voice's own line,
      `{"op":"call","name":NAME,"args":{...}}`, on
      `/run/lampos-skills/call.sock` and prints the one line back as it
      came (`{"ok":true,"result":{...}}` or `{"ok":false,"error":"..."}`).
      The guidance names it by its path on the lamp,
      `/opt/lampOS/tasks/current/lamp-tasks`. It waits at most 3 s,
      connecting included. It refuses, without connecting, a name the
      tools file doesn't list, arguments that are not a JSON object, and a
      line over 8192 bytes; the folder is fixed (no option or variable
      names another), and a socket that is a link is never followed.
      Exit status: 0 for `ok:true`, 1 for the skills' `ok:false`, 2 for its
      own refusal or no answer (also printed as `{"ok":false,"error":...}`).
      The lamp's skills service applies its own checks to the call, as to
      the voice's (cooldowns, the lamp's pause, its policy).
    - **Without the switch** nothing of this is written: no
      `external_dirs` line, no `AGENTS.md`. `lamp-tasks skill` is there
      either way, and does only what is asked of it by hand.
  - **`HERMES_HOME` for Harness.** With the switch on, the env file the
    part writes for Harness (`HARNESS_ENV_FILE`, as for the hold above)
    names `HERMES_HOME` and `SHELL=/bin/sh`, and every Harness command the
    part runs has both. `lampos`'s own shell is `/usr/sbin/nologin`, and
    Harness reads an engine's launch environment through `SHELL` first:
    without it `harness new` times out "Waiting to read the engine's
    launch environment" (seen on a lamp).
    Harness's daemon reads the file only when it starts: on lamp-c0a4 on
    10 Nov it was written with `SHELL` and `HERMES_HOME` while harnessd
    ran, and `harness new hermes` waited on "Waiting to read the engine's
    launch environment" until harnessd was restarted by hand. So when
    lamp-tasks' write gives the file other words (or it was not there)
    and a daemon answers on Harness's port (`/api/health`), lamp-tasks
    starts it again once with `harness start --restart` (Harness CLI
    0.6.2: stops the daemon running for this data folder and starts it
    over), on its job lane as the daemon start after a sign-in is
    (openharness#1463), before the next hand-over; never while a task is
    busy (watched), then it waits until none is. One journal line says it
    (`Harness's daemon started again: its env file changed while it ran`,
    or `not started again: <code>`). Tests:
    `hold::tests::the_env_file_is_said_changed_only_when_its_words_change`,
    `tests/runtime.rs`
    `a_changed_env_file_restarts_harnesss_daemon_once_and_never_while_a_task_is_busy`,
    `tests/harness.rs` `the_daemon_is_started_again_with_harness_start_restart`.
    The file is written with the hold off too, then without the hold's
    lines: the launcher isn't pinned then, so only the Harness commands the
    part runs, and the daemon they start, have `HERMES_HOME`. The hooks
    block is kept as it is: Harness writes its hooks into Hermes'
    `config.yaml` only while that file is owned by Harness's user
    (`lampos`).
  - **Which session, which machine.** A hand-over naming a session looks
    for it as #5784 does (exact name, then a word-start prefix, then by its
    work, then by a stopped session's last ask), on the lamp first, then
    on each computer target's machine. A fit on the lamp is the answer,
    and the computers' lists are then not read at all; one fit on a
    computer alone is replied into
    there (`harness reply --machine <its id>`), the task's dispatch says
    `joined_on` (alias, machine, engine; schema 6), and it is shown,
    watched, closed and told as that machine's, like any computer task.
    Two computers fitting and none on the lamp is `session_ambiguous`; a
    computer that can't be read is left out rather than said not found.
    A computer the last `harness machines` look said is `offline` is not
    listed at all (kept by lamp-tasks from that look, no call of its own):
    nothing could be typed there, and its list only answers DISCONNECTED.
    A hand-over naming a machine looks there only. Latency: on lamp-c0a4
    on 10 Nov the lookup took 15.2 s, reading every machine in turn,
    including two offline computers that each answered DISCONNECTED after
    about 2.2 s and the person's Mac, whose list takes about 4 s. Now a
    session on the lamp costs the lamp's own list (and, found by its last
    ask, up to 4 `harness show` reads and a `harness resume`); the
    computers' 8.4 s or so are gone. A session only on the Mac costs the
    lamp's list plus the Mac's (about 4 s), the offline ones' 4.4 s gone.
  - **New work goes to the lamp.** A new task naming nothing goes to the
    lamp's Hermes, and so does one naming Hermes or the lamp as `who`;
    one naming their computer (`where` `"computer"`) or an agent there
    goes to the computer, and a computer known `offline` is
    refused `computer_offline`, never moved to the lamp quietly. The task
    is followed on the lamp's machine id exactly like a computer task:
    show, wait, watch, task news and the status and progress tool. Its
    session shows in Harness Desktop like any other.
  - **The voice:** the `lamp` target is never one of the staff.
    `start_agent_task` gets `where` (`"lamp"`, `"computer"`) while the
    tasks part has it; the description says long work is handed over, an
    earlier session of theirs first (the lamp's first), else a new one on
    the lamp unless they name their computer. The read-back and the yes
    are as before (one yes for a named session too): "Hermes on the lamp".
    Asked for with no `lamp` target, it says that isn't set up and nothing
    goes to a computer instead. A call naming no machine from a tool
    declared with the lamp's words goes to the lamp even when the `lamp`
    target went away since (the lamp's reading went stale): it is said not
    set up, never sent to a computer. A finished task's news adds that it was
    done here on the lamp ("I did it here on the lamp"); a session joined
    on a computer is told as that computer's.
  - **Tests:** `readiness::tests::the_lamp_is_kept_only_where_hermes_is_known_installed`,
    `runtime::tests::the_lamp_is_kept_only_with_hermes_chosen_for_it`,
    `runtime::tests::hermes_settings_are_written_at_start_only_with_a_token`,
    `runtime::tests::the_lamp_folder_gets_the_guidance_once_and_again_when_changed`,
    `hermes_config::tests`: `the_lamps_skills_dir_is_added_once_and_hooks_are_kept`,
    `a_persons_own_skills_dirs_and_keys_are_kept`,
    `the_dir_already_there_in_any_spelling_is_left_as_it_is`,
    `a_flow_list_a_single_path_or_a_reset_becomes_a_block_list`,
    `skills_that_cannot_be_merged_safely_are_refused`,
    `the_guidance_is_written_once_never_through_a_link`,
    `the_guidance_says_where_the_skills_are_how_to_run_one_and_the_rules`,
    `skill_call::tests` (the list, the voice's line and its answer, the
    skills' refusal, a name not listed or bad arguments never reaching the
    socket, a linked socket never followed, the deadline),
    `hold::tests::the_env_file_names_hermes_home_and_a_shell_with_or_without_the_hold`,
    `engine::tests::a_session_joined_on_another_machine_is_followed_there`,
    `tests/harness.rs`: `a_named_session_is_looked_for_on_the_lamp_first_then_the_computers`,
    `a_computer_said_offline_is_not_listed_for_a_named_session`,
    `a_lamp_with_no_computer_finds_and_reopens_its_closed_session`,
    `the_session_machines_come_from_the_targets`,
    `tests/runtime.rs`: `a_lamp_with_no_computer_hands_work_to_its_own_hermes`
    (the owner's rule: a fresh lamp with no computer paired is one device,
    and its targets are only `lamp` with Hermes, a hand-over to it started
    on the lamp's machine; the lamp's work folder is made whichever look,
    the machines' or an engines look coming back, finds Hermes there first),
    the voice's `agent_tasks::tests::a_lamp_with_no_computer_still_hands_work_to_its_own_hermes`,
    `the_lamp_is_a_target_with_hermes_only_when_on_and_installed_there`
    (off, Hermes missing, installed; a computer offline refused; the answer
    shown on the lamp's machine),
    `with_hermes_on_lamp_off_work_naming_no_machine_goes_to_the_computer`
    (the PM's: the switch unset, `off`, empty or `ON`, the lamp listed as a
    machine with Hermes installed, yet the targets are the office computer
    alone, `lamp` is refused as no target, and work naming no machine is
    started on the computer with Claude Code, as on stable), the voice's
    `agent_tasks::tests::new_work_goes_to_the_lamp_unless_a_computer_is_named`
    and `task_news::tests::work_done_on_the_lamp_is_told_as_done_here`.
  - **Not yet shown on a lamp:** Hermes found by the lamp's Harness, a task
    run there with DeepSeek, its done line, the hooks block after a
    restart, the session in Harness Desktop, and the
    voice staying responsive while it runs (#5978 part 4); the lamp's
    skills in Hermes' list through `external_dirs` with the hooks block
    kept, `AGENTS.md` written in the work folder, and a skill's tool
    (such as `dance`) called by `lamp-tasks skill call` from Hermes'
    own namespace.
- **The lamp itself, with OpenClaw** (`LAMP_OPENCLAW_ON_LAMP=on`, only
  with `--engines-from-machines`; off unless set, #6226). OpenClaw is the
  lamp's default engine (no memory floor: the owner, #6226, every lamp has 6 GB): it takes Hermes's place as the lamp's own target
  in exactly Hermes's role (above), and Hermes stays selectable.
  `lamp_order` (`crates/tasks/src/runtime.rs`) leaves OpenClaw out only
  while the Harness on the lamp is older than 0.6.11.
  - **Which engine:** `LAMP_LAMP_ENGINE` is `openclaw` or `hermes`, unset
    `openclaw`, the default (no memory floor, #6226). The lamp's target is still `lamp`, with that engine; when
    the one chosen is not installed but the other is, the other.
  - **Started** as Harness starts OpenClaw:
    `openclaw tui --session harness-<agentId> --message "<prompt>"`, with
    no permission modes and no model routing (OpenClaw picks its own
    model, from its settings below).
  - **The voice** (`agent_tasks`, `task_news`): reads the lamp's target's
    engine from `{"op":"targets"}` and names it, so a hand-over naming no
    machine is said as "OpenClaw on the lamp" and the tool descriptions
    say "OpenClaw here on the lamp"; with `hermes` there, "Hermes" as
    before. Until the targets are read it says OpenClaw, the default.
    `who` "OpenClaw" or "Hermes", while the lamp has its target, is the
    lamp's own, never a computer's. OpenClaw is also one of the agents the
    voice can name on a computer, heard as "OpenClaw", "open claw", "open
    clause", "open close", "open clow" or "open klaw". News of work done by
    either engine on the lamp is told as done here on the lamp.
  - **Shown only in tests:** `agent_tasks::tests::new_work_goes_to_openclaw_on_the_lamp`,
    `agent_tasks::tests::with_the_lamps_target_the_hand_over_says_what_goes_where`,
    `agent_tasks::tests::with_named_only_the_words_name_an_agent_not_set_up`,
    `task_news::tests::work_done_by_openclaw_on_the_lamp_is_told_as_done_here`.
    Not shown on a lamp.

Signing in a lamp with no screen is by a spoken code since Harness 0.4.23
(openharness#1410; ["Signing in by a spoken code"](#signing-in-by-a-spoken-code---sign-in-by-code-1676)).
The reset plan doesn't list `targets.json.from-machines`. A mark left after
a reset changes nothing: a missing file is the part's to write anyway, and
a file a person writes has another sha256.

Tests: `engine::tests::targets_read_again_are_used_from_then_on_and_a_task_keeps_its_own`,
`engine::tests::targets_for_a_newer_build_read_again_are_off_until_valid_ones_come`,
`engine::tests::with_no_harness_a_task_offered_is_refused_with_why`,
`tests/harness.rs`: `sign_in_is_one_auth_status_read_as_harness_gives_it`,
`tests/runtime.rs`: `set_up_later_reads_the_targets_again_and_says_why_hand_overs_are_off`
(a mutation that reads the file only once fails it),
`without_set_up_later_the_report_is_as_before`,
`tests/runtime_failures.rs`: `set_up_later_runs_with_no_harness_and_no_targets_and_refuses_with_why`
(the real binary; without the switch the same start fails),
`tests/harness.rs`: `the_machines_are_one_call_read_as_harness_lists_them`,
`runtime::tests::targets_from_machines_name_the_first_office_and_the_rest_by_name`,
`runtime::tests::targets_are_written_from_machines_only_where_the_file_is_the_parts_own`
(writing over a file without the part's mark fails it),
`tests/runtime.rs`: `targets_from_machines_are_written_and_used_while_signed_in`
(asking while signed out fails it), `targets_from_machines_needs_an_engine_and_set_up_later`.

The signed-out machines test observes diagnostics for its original 600 ms,
then sends a targets request and waits for an idle, still-signed-out report
that includes that request. The owner writes this report after scheduling,
so the test checks both that no machines read remains pending and that the
read count and targets stayed unchanged. It also checks the read count on
each diagnostic while waiting: a wrong machines job that keeps the worker
busy must fail this assertion, not hide behind the idle barrier. Sign-in
call counts alone do not establish that the machines timer was due or that
the owner finished its pass. Both diagnostic waits are bounded by the test
helper.

### Harness held at its pinned version (`LAMP_HARNESS_HOLD_UPDATES`, #5229)

On in the product since #5379: the product's `lamp-tasks.service` sets
`LAMP_HARNESS_HOLD_UPDATES=on`, so a home lamp keeps the pinned Harness and a
newer one comes only through a new pin in a build. Off unless set elsewhere
(`crates/tasks/src/hold.rs`). A home lamp installs the Harness
its build pins (install.md, "The build's own pin"). Without a hold, Harness's
daemon updates itself to whatever Harness publishes next. From Harness's
source (openharness main 577954332):

- The daemon holds its self-update when `ADAPTER_UPDATE_DISABLE=true` is in
  its own environment (`config/env.ts`, `masterProcess.ts:278`). A **Team
  account ignores the hold** on purpose, and a team setup deletes it. That
  is for the Harness team to change; this switch can't.
- `harness start` either spawns the daemon with the caller's environment, or,
  when `harness service install` wrote a user unit, rewrites that unit from
  the caller's values of a fixed list (`lib/platformDaemon.ts:42`). The list
  includes `ADAPTER_UPDATE_DISABLE` and `HARNESS_ENV_FILE`, so a later start
  from a shell without them drops them.
- The one file Harness loads is `HARNESS_ENV_FILE` (dotenv, not overriding
  the environment). Nothing under `HOME` is loaded by itself.

So, with the switch on, each time lamp-tasks builds its Harness client it
writes `harness-hold.env` in Harness's home (`/var/lib/lampos-tasks`, 0600,
whole: `ADAPTER_UPDATE_DISABLE=true`). It then adds `ADAPTER_UPDATE_DISABLE=true`
and `HARNESS_ENV_FILE=/var/lib/lampos-tasks/harness-hold.env` to the
environment every Harness command gets (both are now on the client's
environment list), `harness start` among them. Its journal says `tasks:
Harness's own updates held (ADAPTER_UPDATE_DISABLE=true,
HARNESS_ENV_FILE=…)`, or `not held: <why>` when the file can't be written.

**Dev and beta lamps: the hold lifted (#6300, (B)).** With the switch on,
a lamp whose updater follows the lamp-voice feed is the canary for Harness's
releases, and every writer of the hold lifts it there instead:
`harness-hold.env` is written without `ADAPTER_UPDATE_DISABLE=true` (said
changed on a lamp that held before, so a running daemon is started again once
no task is busy, and `harness start` rewrites its unit without the hold), no
Harness command is given `ADAPTER_UPDATE_DISABLE`, and the launcher's three
lines below are taken out, at lamp-tasks' start and after `lamp-update
harness-install`. The journal says `tasks: Harness's own updates not held:
this lamp follows the lamp-voice feed, …`, and the health line after each
start is kept. The feed is the `--channel-url` of `systemctl show
lamp-update.service --property=ExecStart --value`, exactly
`https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/`.
The stable feed, any other, and a line not read (systemctl failed) keep the
hold exactly as below. One predicate decides it for both lamp-tasks and
lamp-update (`lamp_msgs::harness_hold`, `crates/msgs/src/harness_hold.rs`).

**The pinned launcher (#5493).** The environment above reaches only the
Harness commands lamp-tasks runs. A `harness logout` run by hand over ssh,
without them, restarted the held daemon from its own environment, and the
daemon updated itself within 10 s (lamp-c0a4, 6 Nov). So with the same
switch, Harness's launcher in its home's bin folder
(`/var/lib/lampos-tasks/.local/bin/harness`, normally `#!/bin/sh` and one
`exec … cli.js` line) is written in Harness's own pinned shape, the one its
installer writes with `--no-updates`:

```sh
#!/bin/sh
export ADAPTER_UPDATE_DISABLE=true
export HARNESS_ENV_FILE=/var/lib/lampos-tasks/harness-hold.env
case "${1-}" in update) echo "harness: this lamp keeps Harness at its pinned version and updates it itself; nothing was changed" >&2; exit 1 ;; esac
exec … cli.js "$@"
```

Every `harness …` run through the launcher (or through `/usr/local/bin/harness`,
its link) then holds the updates, whoever runs it. Harness's own updates
rewrite only the `exec` line, so the preamble stays.

**`harness update` refused (#5957).** The daemon's hold stops only its own
self-update. `harness update`, run from a shell as the tasks user (by hand,
or by an agent's terminal tool), installs the newest Harness whatever the
hold says. It put lamp-c0a4 above its build's pin on 11 Nov, where
lamp-update never moves it back down. So the launcher's third line refuses
that one command before Harness runs: one line on stderr, exit 1, nothing
changed. Harness takes its command from its first argument (openharness
`cli/src/cli.ts`), so only that is read: `harness update --force` is
refused, `harness reply <id> update` and every other command run as before.
lamp-update moves Harness to its pin itself (`harness-install`) and never
calls `harness update`. The line is written and taken out with the hold by
both writers, from one text (`lamp_msgs::harness_hold::REFUSE_UPDATE`).

- **Where:** lamp-tasks at its start (`hold::pin_at_start`), for lamps
  installed before this; and `lamp-update harness-install` right after it
  installs, before it starts lamp-tasks again
  (`crates/update/src/harness_install/launcher.rs`). lamp-update reads the
  switch from lamp-tasks' unit as systemd has it loaded (`systemctl show
  lamp-tasks.service --property=Environment`), so both follow the one switch
  (drop-ins included). A failed read is taken as off; either way it says one
  line (`LAMP_HARNESS_HOLD_UPDATES is not on in lamp-tasks.service: …` or
  `lamp-tasks.service's environment not read (<why>): … taken as off, …`).
- **What is written:** any `export ADAPTER_UPDATE_DISABLE=…` or `export
  HARNESS_ENV_FILE=…` line, and the refusing line, before the first `exec`
  line is taken out, and the three lines above are put just before it. The `exec` line and every other
  line are kept as they are. A launcher already in that shape is not written
  (byte for byte the same, no second preamble). Otherwise it is written whole,
  through `.harness.lampos-pin` beside it, with its own mode (executable) and
  owner, then renamed over it. lamp-update (root) does this without following
  links: `.local`, `bin` and the launcher are opened `O_NOFOLLOW`, the launcher
  is checked, read and its mode and owner taken through that one open file, and
  `.harness.lampos-pin` is unlinked, made new (`O_EXCL`) and renamed in that
  same folder.
- **Not written:** with the switch off; with no launcher (none is made, one
  line said); with a launcher that is a link (`a link, left as it is`; its
  target is not read or written), not a plain file, or has no `exec` line;
  with one that has a second hard link (`hard-linked (N names), left as it
  is`) or is owned by another uid than its `bin` folder's owner (`owned by
  uid U, not the bin folder's owner B, left as it is`), both checked on the
  one `O_NOFOLLOW` open file (two security reads' P3 on #5624).
  Nor with one over 64 KiB (Harness's is a few hundred bytes): its size is
  checked on that open file and the read stops past the cap, so a launcher
  that grows meanwhile is still bounded (`N bytes, over the 64 KiB cap, left
  as it is`; a P3 on #5647).
- **Journal:** lamp-tasks says `tasks: Harness's launcher pinned (…): <path>`,
  `tasks: Harness's launcher already pinned: <path>`, `tasks: no Harness
  launcher at <path>: left alone` or `tasks: Harness's launcher not pinned:
  <why>`; lamp-update says the same without `tasks: `.
- lamp-tasks still gives both variables to each Harness command.
- **Limits:** a direct `node …/cli.js` call skips any launcher; a Team
  account ignores the hold.

**Health, no credential.** With the same switch, after each `harness start`
that succeeded (after a sign-in, or at the part's own start), a thread of its own asks `GET /api/health`
on `127.0.0.1` at `PORT` (18473 unless set, `config/env.ts:124`). It sends
no token or cookie, and nothing waits on it. Harness answers 200
`{"ok":true,"version":"x.y.z"}` once its server is up, signed in or not
(`hookServer.ts:527`). A chunked answer
(`Transfer-Encoding: chunked`) is joined before it is read (#5379's
follow-up: on lamp-c0a4 the line said `not_json` while the daemon was up). It is asked every 0.5 s, each ask bounded to 2 s,
for at most 20 s, and one line is journaled:

- `tasks: Harness health ok on 127.0.0.1:18473, version 0.5.39, <ms> ms after its start`
- `tasks: Harness health failed on 127.0.0.1:18473: <reason>`, the reason
  one of `not_200`, `not_json`, `not_ok`, `no_answer` or `no_answer within 20 s`.

The version is the one to compare with the build's pin (install.md, "The
build's own pin"), which `lamp-update harness-install` also compares right
after the install.

### Signing in by a spoken code (`--sign-in-by-code`, #1676)

`lamp-tasks serve --sign-in-by-code`, off unless set; the product's
`lamp-tasks.service` sets it (with `--app-socket`) since 5 Nov (#5198). A lamp has no screen
to show Harness's QR code. Harness 0.4.23 signs a device in by a code it
says aloud instead (openharness#1410, the Harness team on 20 October):
`harness login --code --json`, as the part's own user and `HOME`. The part
runs Harness's own sign-in and passes the person's answer on; it keeps no
credential of its own.

- **The voice starts it** with `sign_in_start`, on the trusted route only.
  The part starts the program on a lane of its own (`Cli::sign_in_by_code`):
  the same program check, environment and marks as every call, but its
  stdin kept for the answer, its lines read as they come, and its life
  [`sign_in::MOST_LIFE`](../crates/tasks/src/sign_in.rs) (11 minutes; Harness
  keeps a code alive for 10). It never takes the operation lane, so tasks
  and reads go on beside a sign-in waiting on its person. A start while one
  is open gives that one back. Each sign-in has an `id`, in every view, for the
  voice's news to key on: counted from the clock (unix ms, then one more a
  sign-in), so one after a restart isn't taken for one before it.
- **The code.** `{"sign_in":"code","code":"12345678","expires_in":…,"code_told":…}`:
  `code_told` is true once a `sign_in_start` answer or an atomic
  `sign_in_take_code` claim gave that code. The provider's continuation and
  coordinator news share this Tasks-owned claim; a state read alone gives
  neither ownership to say it. The
  code given is forgotten once the state leaves `code`. The
  voice says the digits, and where to type them (Harness on the computer,
  Machines ▸ Approve a device, or harness.autonomous.ai). The same code
  kept alive is no change.
- **The question.** Once the person approves in Harness,
  `{"sign_in":"asking","email":"…"}`: the voice asks "Sign in as …?", the
  email read back clearly, as Harness asks. No session exists before the
  answer, so a stranger who heard the code gains nothing.
- **The question told.** The voice sends `sign_in_asked` with the
  sign-in's `id` and the number of the request whose turn said the question,
  once that turn was heard whole. A question cut off, refused or never said
  opens nothing; the first telling counts, except that one in a lower
  request is a restarted voice's, whose numbers began again, and replaces
  it. The voice's news runs in its
  coordinator and its tools in a worker, two processes: this state is kept
  here, the one place both reach.
- **The answer.** `sign_in_answer` with `yes` or `no` and the voice's
  request number, written to Harness's stdin only while it asks
  (`not_asking`), in a request after the one the question was told in
  (`not_told`: a yes said before the lamp asked is for something else), and
  within [`sign_in::ANSWER_WINDOW`](../crates/tasks/src/sign_in.rs) of the
  first telling (two minutes; `too_late`). An answer too soon leaves the
  question open; past the window the sign-in ends as `too_late`, by a late
  answer or by the clock, so the person can start again and isn't left
  with one open that can't be answered until its 11 minutes are up. One answer only: a question after it is passed over. The
  sign-in's `Debug` hides the digits and the email.
- **The end.** `{"sign_in":"done","result":"success"}` (the email is kept
  only for the question), or
  `"error"` with Harness's code (`DENIED`, `EXPIRED`, `CANCELLED`,
  `BACKEND_ERROR`) or the part's own: `harness_too_old` (a Harness before
  0.4.23, which doesn't know `--code`), `ended` (it stopped with no result),
  `unreadable`, `timeout`, `too_late` (its question went unanswered for two
  minutes) or `no_harness`. Kept for the voice until another
  starts. Signed in, the part looks at the sign-in and the machines at once
  (with `--set-up-later`), not a minute later.
- **Harness's daemon, started again.** Harness's own sign-in stops its
  running daemon, and the user service `harness service install` set up
  doesn't start it again (openharness#1463). So every hand-over after a
  sign-in failed in under 3 s, `new_failed`, with Harness saying "Harness is
  not running here" (lamp-75c9, 20 October, two voice hand-overs to Devin
  Cloud). Once a sign-in by code succeeds, the part runs `harness start`
  (`Cli::start_daemon`, through systemd where the service is installed).
  It's the next job on the lane, ahead of a hand-over already waiting,
  which would otherwise go to the stopped daemon. It's given 75 s
  (`DAEMON_START_TIMEOUT`), past Harness's own 60 s wait for the daemon,
  and the part journals `Harness's daemon started` or why
  not. A sign-in that fails or is cancelled starts nothing.
- **Started at the part's own start, when already signed in (#5379).**
  Harness put on by the onboarding install (`harness-install`, install.md)
  has no daemon at boot: no linger and no `harness service install`. So
  after a reboot a signed-in lamp had no daemon, and no hand-over could go
  (lamp-c0a4, 5 November: `Harness signed in: yes`, then
  `machines_failed`). With `--sign-in-by-code` and `--set-up-later`, when
  the part's first sign-in read says signed in (or offline), it runs the
  same `harness start`, with the same tries below, and journals
  `Harness read yes at this start: its daemon is started (#5379, #6407)`
  (`offline`, or `no` below). A part that starts signed out starts nothing
  until a sign-in, unless the lamp's own engine may be its target
  (`LAMP_OPENCLAW_ON_LAMP` or `LAMP_HERMES_ON_LAMP` on, #6407): then it starts the daemon signed out
  too, since the lamp's own work runs on it.
- **A failed start is tried again.** On lamp-75c9 (21 October) the start
  right after a spoken sign-in said `start_failed`, and one by hand about a
  minute later worked. So a failed start is tried again 10 s later, then
  20 s and 40 s after each next failure (`DAEMON_RETRY_AFTER`,
  `DAEMON_START_TRIES`: four tries in all, about 70 s of waits). While a try
  is due no hand-over goes, since it would fail on the stopped daemon; reads
  of tasks already handed over go on. Each failure is journalled with when
  it's tried again. If every try runs to its 75 s, a waiting hand-over
  waits about six minutes. **Known limit:** after the fourth failure it
  isn't tried again until the next sign-in, and hand-overs fail as before,
  until openharness#1463 is answered.

The journal says `tasks: sign-in 1: started`, `code given (good for 120 s)`,
`approved in Harness: asking whether to sign in`, `question told in request
3`, `answered yes` (or `yes not taken: not_told`), and `signed in` or `not
signed in: CODE`: never the digits or the email.

Tests: `sign_in::tests` (Harness's lines read strictly; a sign-in from its
code to its end; ended, too old, timed out;
`an_answer_is_taken_only_in_a_later_request_soon_after_the_question_was_told`,
`the_code_given_is_forgotten_once_the_code_is_spent`),
the CLI's
`the_sign_in_by_code_reads_harness_lines_and_writes_the_answer` and
`a_harness_without_the_code_sign_in_is_told_apart_and_a_dropped_one_stops`
against a scripted program, and the service's
`the_sign_in_by_code_is_the_trusted_routes_from_its_code_to_its_end` (the
daemon started once after the sign-in succeeded, not before),
`after_a_sign_in_the_daemon_is_started_before_a_waiting_hand_over` (ahead
of a hand-over waiting, and never after a sign-in that failed),
`a_failed_daemon_start_is_tried_again_before_a_waiting_hand_over` (a
waiting hand-over goes after the try that works, or after the last) and
`the_daemon_is_started_with_harness_start`. The
voice's side (`sign_in_computer`, `sign_in_answer`) is its own PR.

## Hermes' settings on the lamp (#5978, `hermes_config`)

`lamp_tasks::hermes_config::write(HERMES_HOME, port, token)` merges what
Hermes on the lamp needs into its own two files, and nothing else. Nothing
calls it yet: the lamp as a target (#5978, part 3) runs it before a Hermes
task, with lamp-cloud's forwarder port (7741) and its local token
(`/var/lib/lampos-cloud/hermes-token`, docs/cloud.md, "Hermes' DeepSeek,
forwarded on the lamp").

- `config.yaml`: `model.provider: "custom:lamp"` and
  `model.default: "deepseek-flash"` (the voice's DeepSeek model), with any
  `model.base_url`, `model.api_key` or `model.api_mode` from another provider
  removed; `providers.lamp` with `base_url:
  "http://127.0.0.1:7741/v1/deepseek"`, `api_mode: "chat_completions"`
  (DeepSeek's OpenAI-compatible chat), `key_env: "LAMP_HERMES_TOKEN"` and
  `default_model: "deepseek-flash"`; and `approvals.mode: "manual"`, so
  Hermes asks before a dangerous command (lamp-tasks never says yes to it
  by itself). The values are quoted: PyYAML would read a bare `off` or `on`
  as a boolean.
- `.env`: one `LAMP_HERMES_TOKEN=<token>` line, the forwarder's local
  token, never the relay's key. Provider keys are taken out
  (`DEEPSEEK_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
  `AUTONOMOUS_API_KEY`, `OPENROUTER_API_KEY`); every other line kept.
- **Who:** it must run as Hermes' own user (`lampos`), so both files stay
  Hermes'. lamp-tasks (also `lampos`) reads the token from
  `/var/lib/lampos-cloud/hermes-token`. Neither file is read through a link
  (`O_NOFOLLOW`), and one that isn't a regular file is refused.
- **Kept:** every other key and line, Harness's `hooks:` block above all
  (openharness `engines/hermes/contract.ts`), comments, and other providers.
  Running it again changes nothing, and a file already right is not
  written. A written file is 0600, written beside the old one and renamed
  over it.
- **Refused:** the workspace has no YAML crate, so the merge is line-based
  over block-style YAML, as Hermes and Harness write it. A file it can't
  merge safely (a tab, a document marker, a key twice, a list or anchor
  where a mapping is set, a top level that isn't a mapping) is left as it
  is, and nothing is written, `.env` included.

## OpenClaw's settings on the lamp (#6226)

With `LAMP_OPENCLAW_ON_LAMP` on, lamp-tasks at its start merges what
OpenClaw on the lamp needs into `<state>/openclaw.json`
(`/var/lib/lampos-openclaw/state`, JSON5), exactly as `hermes_config` does
for Hermes, and keeps every other key (lamp-update's `gateway` block among
them) (`crates/tasks/src/openclaw_config.rs`):

- `models`: `mode: "merge"`, with the provider `lampos`: `baseUrl`
  `http://127.0.0.1:7741/v1/deepseek` (lamp-cloud's forwarder, docs/cloud.md),
  `apiKey` `"${LAMP_HERMES_TOKEN}"` (the wrapper exports it from the
  forwarder's token file), `api` `"openai-completions"`, and one model,
  `deepseek-flash` (the forwarder's only model). A loopback target may
  need `request.allowPrivateNetwork: true` on the provider.
- `agents.defaults.model.primary`: `"lampos/deepseek-flash"`.
- `agents.defaults.workspace`: the lamp's own work folder
  (`/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp`, `LAMP_PROJECT` under
  `lampos`'s home), where the lamp's skills keys point and the guidance
  (`AGENTS.md`) is written, as for Hermes.

## Devin (#1854)

`devin` is Devin's own terminal program on the person's computer, which
Harness starts in a pane like Claude Code and Codex, with its hooks and its
transcript readers (openharness `cli/src/engines/devin/`). So `ps`, `show` and
`wait` read it as they read the others. Devin Cloud is a separate engine,
`devin-cloud` ("Devin Cloud" below).

- **No `--mode`.** Devin has no permission modes in Harness: its `new`
  leaves the mode out for it and refuses one it is sent
  (`INVALID_PERMISSION_MODE`, `cli/src/lib/newCommand.ts`). So `lamp-tasks`
  sends none for `devin` (`EngineKind::has_modes`), and takes a receipt that
  claims no mode that approves on its own (`auto`, `full`) and no bypass. A
  missing or `null` mode, or another, is taken; Devin's real receipt hasn't
  been seen. The other engines still need `auto` with a bypass that is
  there.
- **Its task is typed in after it starts.** Harness starts Devin with no
  first message ("Devin cannot start with a first message", Harness 0.5.6
  on lamp-75c9 and the laptop, 29 Oct), so a `--task` fails the `new`
  (`new_failed`). For `devin` alone, `lamp-tasks` sends `new` with no
  `--task`. It then reads the harness with `show --machine <id> <agent>
  --json`, every 0.5 s, until Harness says it is `idle`, or `needs-input`
  with nothing asked, no answer and no tool used (`fresh`: how Harness 0.5.6
  reads a Devin that has done nothing yet). Then it types the
  task in with `harness reply --machine <id> <agent> --json -- <task>`
  (openharness#2157, merged 29 Oct), as the person would. All of it fits
  inside the one `launch_timeout` (60 s) the `new` has
  (`EngineKind::typed_after`, `Cli::first_message`).
  - **The person's switch.** Harness takes the reply only with the
    person's `programs-reply` switch on (`harness settings`, off by
    default).
  - **Not typed in.** The harness is started either way, so a task not
    typed in is uncertain, never sent again: `reply_off` (the switch is
    off), `reply_not_idle`, `reply_not_running` or `reply_failed` (the
    reply's refusal), `stopped_before_message`, or `never_idle` (no idle
    `show` inside the bound), or a `show` that couldn't run, under its own
    code (`cancelled`, `cleanup_incomplete`, a timeout), never retried. The
    empty harness is left for the person: the dispatch keeps no agent id
    when it is uncertain, so the lamp can't close it.
  - **A fresh Devin on an older Harness.** Harness 0.5.6 (Devin CLI
    v3000.11.228) reads a Devin just started, showing only its empty input
    box, as `needs-input` (`needsPerson: true`). It is fresh, so the reply
    is tried at once and that Harness refuses it (`reply_not_idle`) in
    seconds, rather than a minute of looks holding the tasks worker (the
    Reviewer-lead's P2 on #4374). A Devin that has asked something, or
    worked, isn't fresh, and nothing is typed into it.
  - **Shown on lamp-75c9 at 18:32 on 29 Oct** (#4374's first head, d0ba23d0cc), with Harness
    0.5.6-dev.9ffe3054f (main with openharness#2157 and its fresh-Devin
    fix) on the lamp and the laptop, and the person's `programs-reply` on.
    A typed question was handed over: `dispatch accepted (exit 0, 14.1 s)`
    for `new`, `show` until idle and `reply`. Devin answered in 39 s, and
    the lamp told its news at 18:33:16, then closed the harness.
- **It may ask before it writes.** With no auto mode, Devin works in its own
  default. An agent waiting for the person is told as now: "Devin is
  waiting for you on your computer".
- **The store's `schema` is 2 only while it holds a Devin task** and no
  newer engine's (it is written as the highest any task needs, 4 with a
  Hermes task and 3 with a Devin Cloud one, `schema_needed`): a
  target's engine may be `devin`, a new value in a stored enum. A store
  with only Codex, Claude or OpenCode work stays schema 1 and is written as
  1, so a rollback to a build from before keeps the person's hand-overs. A
  store that holds a Devin task is written as 2, which a build from before
  sets aside (above). A schema-2 store whose Devin work is gone is written
  as 1 again, and a schema-1 store that names Devin is refused (the first
  builder on #1872).
- **The voice** knows Devin as one of the staff (`agent_tasks::KINDS`). It's
  heard as "Devin" or "Devon", and the model is told it can sound like
  "Kevin", which is never taken as Devin's own name, since it is a
  person's. News of its work names it (`task_news::who`). With all four
  agents the start description gives examples for two of them, since all
  four are named in its list and the description must stay inside the
  service's 1,024 bytes.
- **Tests:**
  - `tests/harness.rs` `devin_is_asked_for_by_name_with_no_mode_and_its_receipt_claims_none` (sending `--mode` or `--task` to Devin fails it), `devins_task_is_typed_in_with_reply_once_it_is_idle`, `a_devin_whose_task_is_not_typed_in_is_uncertain_with_the_reason` and `a_fresh_devin_read_as_needs_input_is_replied_to_at_once`;
  - `engine::tests::a_store_with_no_devin_task_is_written_as_schema_one` (writing 2 always fails it) and `a_store_with_a_devin_task_is_written_as_schema_two`;
  - `runtime::tests::targets_for_a_newer_build_are_told_apart_and_never_read` (Devin read as this build's);
  - the voice's `agent_tasks::tests::the_person_may_hand_work_to_devin_by_name`, `every_description_fits_the_service_whatever_the_switches` (now with four), and `task_news::tests::news_names_the_agent_each_engine_is`.
- **Shown:** a real hand-over to Devin on lamp-75c9 (above), on a Harness
  built from main. **Not yet shown:** the same on a released Harness, or
  on lamp-a0ae.

## Devin Cloud (#1854, openharness#1444)

`devin-cloud` is Devin in Devin's own cloud, billed in ACUs. It needs
Harness 0.4.27 or later on the lamp and on the person's computer: an older
`new` refuses it, and the task ends `new_failed`. Harness 0.4.27
starts it with `harness new devin-cloud` (openharness#1444), under the
computer that holds the person's Devin connection: the target's
`machine_id` is that computer's. The lamp holds no Devin key and no other
secret; the secrets Devin uses (a Slack bot token, say) are kept in Devin,
by the person. Shown from the person's laptop on 20 October, through
Harness: "reply hello" in 10 s; a post in a private Slack channel with a bot
token kept as a Devin secret, its permalink in the answer, in 49 s; and a
reply in a thread, after reading the channel, in 37 s.

- **No folder, no `--mode`.** The target's `project` is empty (any other is
  `invalid_project`, and an empty one for another engine still is), and
  `new` is sent `devin-cloud --machine <id> --name <name>` with neither a
  folder nor a mode (`EngineKind::cloud`, `has_modes`). Its receipt is taken
  as Devin's is: no mode and no bypass (Harness 0.4.27 sends both `null`).
  No repository is sent (Devin Cloud needs none) and no `--acu-limit`, so
  Harness's 5 applies.
- **What leaves the lamp, and where it stays.** Only the confirmed task's
  text, as for any engine: the same prepare and the person's yes, after a
  read-back that says "Devin, in Devin's cloud". Nothing of the thread,
  memory or context goes with it. **That text goes to a third party,
  Cognition (Devin's maker), and stays in that Devin session**, which the
  lamp never closes. As for every engine, a typed request with a typed yes in
  the same chat starts it too (#1673); neither the model on its own nor a
  skill can start one without a yes.
- **Its id** is Harness's `cloud_` and 16 hex digits, taken wherever a
  computer agent's UUID is (`harness::agent_id`): `show`, `wait`, `ps` and
  the stored task.
- **Its answer is told as done.** A Devin Cloud agent that has answered is
  `needs-input` in `show` and `wait` (exit 3) whether or not it asked
  anything (openharness#1449), so lamp-tasks reads a cloud agent's
  `needs-input` as `idle` in `show`, `wait` and `ps` (so progress doesn't
  say it waits), and its answer is told. **The lamp can't tell a real
  question from a finished task until openharness#1449 is answered:** the
  news asks the model to say a question it finds in Devin's answer, and that
  the person answers it in the Devin session in Harness, but it may be told
  as a result, and the person can't answer Devin from the lamp. A computer
  agent's `needs-input` is unchanged.
- **A finished session ends `stopped`.** On lamp-75c9 on 20 October, Devin
  Cloud's session that had replied in Slack ended `stopped`, not
  `needs-input`. So a cloud agent `stopped` with an answer is read as `idle`
  too, and told as finished, not "stopped without finishing"; `wait` and
  `ps`, which carry no answer, read it so, and `show` decides. One `stopped`
  with no answer stays `stopped`, so it is told and never waited on.
  `working` and `failed` are as Harness says. **Known limits** (the
  reviewer's P3 1 on #2307):
  - Harness says `stopped` both for a session Devin put to sleep
    (`inactivity`, as here) and for one someone stopped by hand
    (`user_request`). So a session stopped mid-task whose last words were
    progress ("I'm reading the channel now") is told as finished, and so is
    a question left long enough for Devin to sleep. Running out of ACUs is
    `failed`, and told as such. Asked of Harness on openharness#1449
    (21 Oct): keep a finished session `idle`, or give the stop's reason.
  - `ps` and `wait` carry no answer, so for a cloud `stopped` with nothing,
    `tasks/status` says `idle` until `show` is read, while the news says it
    stopped.
- **Never closed:** Harness closes no cloud harness ("stop it in Harness"),
  so `close` is never run for one and its task's close is `refused`, once,
  never tried again. **So each hand-over leaves a Devin session open on the
  person's account,** spending up to Harness's 5 ACUs of their Devin
  account (no `--acu-limit` is sent; a per-target limit would be the place
  for one), which the read-back doesn't say; the person ends it in Harness
  or in Devin.
- **On the bus:** `tasks/status` names the engine `devin_cloud`
  (`EngineKind::bus_name`), since its check (`msgs::tasks`, `engine_named`)
  takes letters, digits and `_` only; Harness is still asked for
  `devin-cloud`. With the dash, one Devin Cloud task made every status fail
  its check, and progress and the settled wake were lost for every task
  (Jarvis's P1 on #2240). The Policy tells the model `devin_cloud` as Devin.
- **The store's `schema` is 3 only while it holds a Devin Cloud task,** by
  the same rule as Devin's 2: a build from before sets it aside, and a
  schema-2 store that names Devin Cloud is refused.
- **The voice** knows it as Devin (`agent_tasks::KINDS`), heard as Devin is;
  "Devin" is one name among the ones speech to text listens for. The
  read-back says "Devin, in Devin's cloud", news names it Devin, and the rest
  of its answer is "in the Devin session in Harness", not on their
  computer. A lamp should have one Devin target or the other. With both, the
  voice keeps the first in the tasks part's order as the one "Devin" names
  (`staff_from` keeps one member a name), and the other gets no work by
  name.
- **Tests:**
  - `tests/harness.rs` `devin_cloud_is_asked_for_with_no_folder_or_mode_and_its_cloud_receipt_binds` (its real receipt; the folder and target rules) and `a_devin_cloud_answer_is_read_as_idle_and_it_is_never_asked_to_close` (`show`, `wait` beside a computer agent, `close`, and ids not Harness's form);
  - `engine::tests::a_store_with_a_devin_cloud_task_is_written_as_schema_three`;
  - the voice's `agent_tasks::tests::the_person_may_hand_work_to_devin_cloud_by_name` and `task_news::tests::news_names_the_agent_each_engine_is` (with a question in Devin Cloud's answer);
  - `tests/harness.rs` `every_engines_bus_name_passes_the_status_check`, and the Policy's `devin_cloud_is_told_as_devin`;
  - in `a_devin_cloud_answer_is_read_as_idle_and_it_is_never_asked_to_close`, a cloud agent `working` or `failed` stays so (reading every cloud state as done fails it).
- **Not yet shown:** the lamp itself handing work to Devin Cloud, by voice
  (lamp-75c9, with its person).

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

## Recording a hand-over for a test

`tools/harness-record.sh` keeps, for a set time, what Harness reported while
a hand-over ran, so a test can show when the agent started, worked and
answered next to the lamp's journal. It only reads (`watch`, `show`, `ps`):

```text
tools/harness-record.sh record NAME SECS [--machine ID] [--agent ID] [--out DIR]
tools/harness-record.sh timeline DIR/NAME
```

It writes `DIR/NAME/` (DIR is `--out`, else `$HARNESS_RECORD_DIR`, else
`./harness-records`). Each line is `END<TAB>START<TAB>MS<TAB>BODY`, local
times in ms since 1970. For `show` and `ps`, START is when the call was made,
END when it answered and MS how long it took, because slow calls are what a
hand-over test usually has to explain (on lamp-c0a4, with the office
computer loaded, `ps --machine` took 5 to 19 s and `show` 7 to 20 s). For
`watch`, START is when that watch run began and END when the line came.

- `watch.jsonl`: `harness watch [--machine ID] AGENT --json`, every line
  whole; only with `--agent`, because `watch` follows one agent. Each run
  opens with `!watch run N started`. When it ends (the agent not there yet,
  a Harness `TIMEOUT`, or stopped) the end is kept as `!rc=N watch ended`
  and it starts again 2 s after that run began, or at once if it ran longer.
- `show.jsonl`: `harness show [--machine ID] AGENT --json` every 2 s, with
  `--agent`; its pretty JSON joined onto one line.
- `ps.jsonl`: `harness ps [--machine ID] --json` every 5 s.
- `events.jsonl`: only if `harness --help` names an `events` verb (0.5.27 has
  none).
- `timeline.txt`: every line merged in time order, with a short summary
  (state, tool count; for `ps` the count of harnesses by state; `took N ms`
  for `show` and `ps`). A Harness error reply reads `error=TIMEOUT`, or
  `!rc=1 error=TIMEOUT` when the call also failed. The summaries come from
  `jq`; without it the same fields are picked out of the line. `timeline`
  builds it again.

A failed call is kept as `!rc=N` and its output. One reader never runs two of
its calls at once: a `show` slower than 2 s is followed by the next, not
stacked. When the time is up no new call starts; a call still running gets
up to `HARNESS_RECORD_GRACE` seconds (30 by default) to answer, and one that
does not is killed and kept as `!killed at end` with how long it ran. A
`watch` run is stopped at once (`!stopped at end`). On INT or TERM every call
is killed at once. Either way nothing is left running and the timeline is
built. `HARNESS_BIN` names the binary; no `$HOME` path is assumed, so on the
lamp it runs as `lampos` with `HOME=/var/lib/lampos-tasks`. Keep the
recordings out of the repository; a verification README names what was kept
from them. `tools/harness-record-test.sh` tests it with a stand-in Harness,
with no network or account.

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
- **A target the tasks part doesn't have** (#1390). On lamp-4ace on 18 Oct
  an install rewrote `targets.json` and restarted the tasks part (which reads
  it only as it starts) under a running voice. The voice went on asking for
  `office-claude` while only `office` was there: with `LAMP_TASKS_WHO` until
  its next staff read, at most 30 s later; without it, until the voice
  restarts. The person heard
  "I can't reach Claude Code at the moment", and the journal had only
  `(target, 18 ms in)`. Now a `target` refusal has the voice read
  `{"op":"targets"}` once. With the switch below it also keeps that as the
  staff, so the next hand-over and session name who is there now. Then:
  - **The journal, always:** `lamp-voice provider: request N: no target
    'office-claude' in the tasks part; it has 'office' (claude)`, with
    aliases and engines only.
  - **Exactly one target of the engine asked for** (the named member's, or
    Claude Code's with nobody named): the work is prepared there, and the
    journal says `using 'office', the only claude target`. It's the only
    place it could go, and the read-back and the person's yes still come
    first. A named member is never swapped for another engine.
  - **None of that engine, or two or more:** nothing more is prepared, and
    the model is told "Claude Code isn't set up for the lamp yet, so this
    can't be handed over. Say so plainly in a few words: it needs setting up
    on the computer first", with the engine's own name. It names no place:
    a home has no "office". It's never told "can't reach it now".
  - **If the targets can't be read,** it's answered as any other refusal.
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
  question kept (`needs-input`) is followed the same way, every 2 minutes
  for up to an hour (`WATCH_QUESTION_MS`): the person answers it at their
  computer, maybe much later, and what the agent then ends with is kept and
  told. The same question seen again is only counted, and work in progress
  is passed over. On lamp-75c9 on 21 October, Claude Code first asked to
  trust its new folder. The lamp told the question, the person trusted it at
  the computer, and the agent finished, but the lamp never said so, since a
  question then ended the watch. A stop or a failure kept, or a task's four
  observations, end it; what progress reads (`tasks/status`) is unchanged. Otherwise, once
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
- **Following by watch** (`serve --push-watch`, #5230,
  `crates/tasks/src/follow.rs`). <a id="following-by-watch-serve---push-watch-5230"></a>
  Each task the watch looks at gets its own `harness watch --machine M
  <agent> --chars 200 --timeout 10m --json`, held on a lane of its own (a fresh
  adapter per watch, so a child that can't be cleaned up stops only its
  own), at most four at once, the tasks already followed first, then the
  watch's order. `watch` prints one JSON line when the event or the turn
  changes (Harness looks every 3 s); only `event` and `id` are read, never
  the summary, ask or question, and a line for another id is passed over.
  `started` and `working` read nothing. `done`, `failed`, `needs_person`,
  `idle` and `stopped` read the task at once (`Engine::push`, the same read
  as `--push-wait`'s, which keeps an idle answer on the spot: `done` comes
  after Harness's two idle looks). `watch` exits by itself only on
  `stopped` (exit 4); on `done` and `failed` the part kills it itself,
  since a pane harness stays idle and its watch would go on. A watch is
  also killed when its task leaves the watch (settled, closed with
  `--close-told`, or let go), and every one is killed and joined when the
  part stops. Exit 124 (its 10 minutes passed) starts it again at once; a start
  again is safe, its first line being how the harness stands then. Exit 1
  (`GONE`, `DISCONNECTED`, `MACHINE_UNAVAILABLE` …; only the code is
  journaled) starts it again after 3, then 10 s, and after three failures
  in a row the task is given up to the fallback. Exit 2, a usage error, is
  taken for a Harness without `watch`: no watch is started again while the
  part runs. A task settled by its watch is not followed again for 3 s,
  and not while its latest read was a push (`Engine::pushed`), so a watch
  started again on an agent still idle never reads it in a loop.
  - **Fallback:** a task once heard from by its watch is left out of
    `--push-wait`'s `wait`; one past the cap, given up, or under a Harness
    without `watch` stays on `wait` when that is on. The watch's own looks
    go on for every task whatever this does, so a result is never later
    than without it.
  - **Journal:** one call line for each `watch` (`tasks: harness watch:
    machine=… agent=… exit=… ms=…`, `exit=ended` when killed on `done`),
    and one line for each event (`tasks: watch of task N: done after N ms`).
  - **Cost:** each `watch` is one Node process, about as big as the held
    `wait` (~130 MB on lamp-75c9), hence the cap of four.
  - **Listing:** Harness has no SDK `get tasks`; the list is `harness ps
    --machine <id> --json` (`{you, machine, context, harnesses, next}`),
    which `--progress` already reads ([Progress on the
    bus](#progress-on-the-bus-serve---progress-732)). `ps` filters only by
    `--machine` and `--all`, so the part keeps only its own tasks' rows.
  - **Not yet shown:** any lamp run. The 5-minute played check on
    lamp-c0a4 (a voice task that stays quiet while it works and says its
    result once) is left for #5230.
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
  in the tasks part, so that its harness can be [closed](#closing-a-finished-harness-1390-serve---close-told-on-in-the-product-unit).
- **Told once more when not heard** (#4342, off unless
  `LAMP_TASK_NEWS_RETELL=on`). News is kept as told the moment it is taken
  to tell (`TaskNews::tell`) or answered (`check_agent_task`), before
  anyone hears it. On lamp-c0a4 on 3 November a task's done news rode on a
  `check_agent_task` answer ("request 109 was answered the news (…); it
  rides on the turn"), the person cut the answer short ("was cut short
  (user_interrupted): its harness stays open"), the Task news skill's ask
  expired, the tasks part's `told` stayed null, and the news was never said
  again. With the switch, settled news (done, failed, stopped, or an agent
  waiting for the person, and a hand-over that failed) whose turn was cut
  short or ended not heard whole is told once more. That includes news
  played through with gaps, whose harness stays open (#4035): on lamp-c0a4
  on 3 November such a news, played under someone's words that made no
  request (`audio_written_with_playback_gaps_unscored`), was otherwise
  neither told again nor closed. It is told once more
  (`TaskNews::not_heard`): the reader offers it again though it is kept as
  told, with the note's words worked out afresh, and the voice tells it
  itself, alone, at the next quiet pause: someone there and nothing said by
  or to the lamp for 15 s (`QUIET_US`), whatever `LAMP_TASK_NEWS_AT_PAUSE`
  says, so the person speaking always wins; it never waits for a skill's
  ask, which is long gone. Only within 30 minutes of the work settling
  (`RETELL_WITHIN_MS`, the note's `until`). Taken the second time
  (`task_news` record `retold: true`), it is never told again unasked: not
  heard whole again, it is let go, two tries in all, the first one
  included. Heard whole in the meantime (asked and answered), it is not
  told again. Heard whole, the tasks part is told as for any news, so its
  `told` is set. Progress and a sign-in's question are never told again
  this way (progress is the Policy's to time; a sign-in asks again its own
  way). Off, news not heard whole stays as before: kept as told, said only
  when asked.
  A task waiting for the person (`needs-input`, as the push wait ends it) is
  already told once by its own words above ("Claude Code is waiting for
  you on your computer"), the same question never twice; the switch only
  gives it its one more try when that telling was cut short.
- **Asked** (`check_agent_task`). "How's it going?" or "what did Claude Code
  say?" is answered at once from what was last seen, with no new read; a
  finished answer heard this way is kept as told, so it is not told again
  unasked. One the person was already told, unasked or asked, is answered
  with `told_before: true` and asks the model to say it is the one it
  mentioned, giving a gist only if it gave none then, not to tell it again
  as new (#757). A long one asks for the gist in one line first, then where
  the rest is. One still working is answered from its latest step (#4342):
  the tasks part's `get_progress` line (`ps`'s `about`, read on the watch's
  schedule with `serve --progress`), cut to 160 characters as `step`,
  quoted from the agent and marked untrusted, and the model says how it is
  going from it, never word for word, and that the agent has been quiet
  when it was last active 3 or more minutes ago. With no line yet (not
  read, or no `--progress`) it says only that the agent is still working.
- **Asked about any of them** (`tasks_now`, off unless `LAMP_TASKS_NOW=on`,
  #4342): "how's it going?", "what is Claude doing?" or "did the poem one
  finish?" answered for every recent piece of work, not only the latest
  (["Every piece of work now"](#every-piece-of-work-now-4342-off-unless-lamp_tasks_nowon)).
- **Progress** is #732: `show` is empty while an agent works (measured
  2026-10-15, 00:05), so the progress signal is `ps --machine`, put on the
  bus as typed state, with Jev deciding whether a step is worth telling
  now, and the model used only to say it: ["Progress on the
  bus"](#progress-on-the-bus-serve---progress-732) below, and the policy's
  [progress question](policy.md#progress-news-say-this-progress-now-732).

The voice's side is on wherever the tasks part is (`LAMP_TASKS_DIR`). The
watch was `--watch` for its first run on lamp-4ace (00:49 on 2026-10-15) and
is the default since; `--no-watch` turns it off
([switches.md](switches.md)).

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

### Told when a skill asks (#939, (c), `LAMP_TASK_NEWS=skill`, on in the product)

The Task news skill (Jarvis, #1021) triggers on `tasks/status` and moves and
lights with the news; the voice tells each piece once, as above, in the
skill's own words (`[news.words]` in its `skill.toml`, #940). With `LAMP_TASK_NEWS=skill`, news of work finished, stopped or
asking waits for the skill's `from: tasks` ask on the say service until
10 s after the voice had it, and is then told with that ask's ticket, or by
the voice itself (news that settled while nobody was there is told when
someone is, at once and without the skill's sway: its ask, made as the work
settled, is gone 20 s later; the reviewer's P2 on #1667)
([speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939)).
An ask that names its task (`"task":10`) is given that task's news only,
so the skill's look and light go with its own task (the reviewer on #1042).
Progress is the Policy's to time already and is told as before.

### Given with a text (#1673, off unless `LAMP_MESSAGES=on`)

News of work asked for by text that the lamp hasn't told aloud is given
with the answer to the person's next text from the Autonomous app, once,
and then kept in the same file of what was told, so the voice doesn't tell
it aloud after. Work asked for at the lamp is told at the lamp, never by
text: a task started by a typed "yes" is kept as a text's (`tasks-typed`)
([messages.md](messages.md#news-when-youre-away)).

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

### The read-back names what the task names (#2602 finding 2)

After `start_agent_task` prepares the work, its answer's `next`
(`agent_tasks::prepared_next`) asks the model to say the task back in one
short sentence, **naming each name the task has as it was written for the
agent**: a channel, place, program, file, project or person. That way a
word misheard shows before the person says yes. With the lead-in (below), a
read-back already said counts only if it named them. On lamp-75c9 on 21
October, "the lamp devin demo Slack channel" went to Devin as "the #dead
Slack channel", and "Claude Code" to Claude Code as "the blog code", while
the read-backs said only "the Slack channel". This isn't behind a switch:
it changes the words of `next` only, on every route, and
`every_read_back_names_what_the_task_names` holds all four variants.

### A corrected ask after the read-back (#2838, off unless `LAMP_TASKS_CORRECT=on`)

On lamp-75c9 on 21 October all three read-backs named a misheard ask, and
the person could only say no and start again. With `LAMP_TASKS_CORRECT=on`
(the provider worker's `--tasks-correct`, after `--tasks-named-only`; it
needs only the tasks part) a "no, I said …" after a read-back gives the
corrected ask, read back again, and started only on a yes:

- `start_agent_task`'s `next` ends "If they say no and give a corrected ask,
  call start_agent_task again with it: it replaces this one. Only their yes
  starts it: the same ask again, in the same words or others, is not a yes."
- `confirm_agent_task`'s description ends "The same ask again, in the same
  words or others, is not a yes: call start_agent_task with it again
  instead." **A repeat is not a yes** (the first builder on #2838, 23 Oct
  05:08): hand-over work runs on the person's computer, so only their yes to
  the read-back starts it. On lamp-75c9 at 04:59 the same ask in other
  words, while a task waited, was taken as the yes and the task started.
- the second route's note beside a later request while a task waits
  (`agent_tasks::pending_note`) says: on a yes, call `confirm_agent_task`; on
  a no with what they meant instead ("no, I said …", "no, ask Codex …"),
  call `start_agent_task` again with the corrected task, and who if named,
  which replaces the one waiting, "which you will not start now"; on a
  plain no, call nothing; "Only their yes starts it": the same work asked
  again, in the same words or others, is not a yes, so `start_agent_task`
  again and read it back, never `confirm_agent_task`.
- a `start_agent_task` that prepares while another task still waits for its
  yes replaces it, but only from a request that could say yes to it
  (`may_confirm`): a later request than the one that prepared it, heard (or
  in the same text chat), within `CONFIRM_WITHIN`. So one line asking two
  agents (two starts in one request) replaces nothing, nor does a request
  the lamp wrote itself (the reviewers' P2 and P3s on #3009), nor the same
  work for the same agent asked again (a yes taken as a new hand-over,
  #1712; El's P3 on #3028). "The same work" is the same text, byte for
  byte: a reworded repeat of the same ask still counts as a correction and
  cancels the one waiting (to be heard in #2838's run on lamp-75c9). Then the
  answer's `next` begins "This one replaces the earlier task, which you will
  not start now.", the journal says `lamp-voice: tasks: request N: a
  corrected ask replaces task <id>, which this voice no longer confirms`,
  and the voice sends the tasks part a `cancel` for the old id, best-effort,
  at the revision the new prepare left. After that this voice confirms only
  the new task, by its own id and digest. A refused or unanswered cancel is
  journalled (`could not be cancelled; this voice no longer confirms it`).
  Not ruled out: a yes being confirmed at the same moment as the corrected
  start (CONFIRM and START are not made exclusive) can still start the old
  task. That's rare, and nothing here says it was not started.

Without the switch the note, `next` and the lines sent are byte-for-byte
as before (a second prepare already replaced the one waiting, unsaid).
Tests: `a_corrected_ask_replaces_the_one_waiting_and_only_it_starts`,
`a_plain_no_starts_nothing_and_a_failed_cancel_keeps_the_correction`,
`two_starts_in_one_request_or_a_written_one_replace_nothing`,
`without_the_correct_switch_the_note_and_next_are_as_before`,
`with_the_correct_switch_a_repeat_of_the_waiting_ask_is_not_a_yes`,
`a_repeat_after_a_no_harness_refusal_is_refused_again_with_nothing_waiting`
(when nothing was waiting, a refused ask leaves nothing waiting, so no
note follows it; a refusal doesn't clear a task already waiting). The
words only steer the model: the voice doesn't yet refuse a confirm on a line
with no yes in it, which comes only if a run shows the words aren't enough.

**Played on lamp-75c9** (23 Oct, lamp-voice `b8a9148bd`, lines from a laptop
in Piper's voice, `LAMP_SPOKEN_TO=off` as lamp-name didn't take Piper's
"Lamp"; #2838):
- **Passed:**
  - a correction then yes (task 24 replaced, only 25 reached the computer);
  - a plain no (nothing called);
  - two corrections in a row then yes (31 and 32 replaced, only 33 sent).
- **Found:**
  - a reworded repeat was taken as the yes (before the words above);
  - within the 120 s after a plain no, new asks were answered in words, as
    the declined task's note still stood beside them;
  - "yes, and also …" isn't shown cleanly yet.

### One yes for a read-back said in words (#5169, off unless `LAMP_TASKS_ONE_YES=on`)

On lamp-c0a4 on 4 November the product voice often said the task back in
words ("Claude Code will write … Shall I start it?") without calling
`start_agent_task`. Then the person's yes either prepared the task and
confirmed it in the same turn, which `may_confirm` refuses (the request that
prepares can't confirm), or confirmed with nothing prepared. Both times the
lamp asked again, so the hand-over needed two yeses. With
`LAMP_TASKS_ONE_YES=on` (the provider worker's `--tasks-one-yes`, last of the
tasks flags; it needs only the tasks part):

- `start_agent_task`'s description, with the lead-in or without it, asks for
  the call first: "Call it first: say the task back only from its answer, in
  one short sentence, and ask whether to start it." With the staff, it says
  ", with who will do it," as before. Every description still fits in 1,024
  bytes, with the switch on too (`every_description_fits_the_service_whatever_the_switches`).
- `start_agent_task`'s `task` parameter reads "The work, in the person's
  own words. A short ask is enough: the agent works out the details." in
  place of "… complete enough to do without asking." (folded in from #5119).
  With the old words the model asked what a short ask should have instead
  of calling: on lamp-c0a4 on 3 November, "ask Claude Code to build a
  website that promotes tourism in Vietnam" called it 1 time in 3, and the
  same ask with its sections named was called at once. The 4 November count
  (`docs/verification/p2-handover-wording-lamp-c0a4-20261104/`) found the
  words alone changed little (0 of 9 before, 1 of 9 after), so they sit
  under this switch beside the call-first read-back. The read-back still
  guards a short ask: nothing starts before the yes
  (`a_short_ask_is_enough_only_with_the_one_yes_switch`).
- `confirm_agent_task` with nothing prepared answers "Nothing is prepared
  yet: call start_agent_task now with the task you said back, then call
  confirm_agent_task again." Without the switch it answers "There is no task
  waiting to be started." as before.
- A yes in the request that prepared the task may start it
  (`agent_tasks::yes_in_the_same_turn`) only when all of these hold. If any
  fails, `may_confirm`'s refusal stands:
  - the request was heard, not typed in the app or written by the lamp;
  - the task was prepared in this very request, within `CONFIRM_WITHIN`
    (120 s);
  - the person's words in it (`thread::heard_for`) are a plain yes: at most
    six words, only yes, yeah, yep, yup, sure, ok, okay, "go ahead", "do
    it" and "please do", with "please", "lamp" and "hey" beside them, and
    no question mark. "Yes and also make it longer" is not a yes. "Okay."
    and "ok ok" count as a yes too: each is only yes words;
  - the lamp's line just before it (`thread::answered_last_but_cut`) was
    said within `CONFIRM_WITHIN`, ends with a question, names who the task
    was prepared for (the name, or a form the voice hears for it), and
    holds every one of the task's content words, none missing. Content
    words are lower case, with common short words ("did" and "does" among
    them) left out, and for a question task they come from its question
    alone. A task word counts as read back only when a spoken word of
    the read-back has the same stem (a plural or verb "s", "es", "ing" or
    "ed" and a final "e" set aside, so "files" matches "file" and "fixing"
    matches "fix"), and each spoken word stands for one task word at most:
    "Test the tests" read back as "Claude Code will test it" is refused.
    No near form counts: "remote" never stands for "remove", "kill" for
    "killall", "wrote" for "write" or "reverse" for "revert". A stem this
    leaves apart costs a second yes, never work started unheard. A
    read-back that drops any word never starts the task: for "Delete all
    the files in my Downloads folder and empty the trash", "Claude Code
    will delete the files in your Downloads folder. Shall I start it?"
    misses "all", "empty" and "trash"; "Reboot" read back as "Claude Code
    will do it", "delete Downloads" as "Claude Code will delete that",
    "Force push to main" without "force", and "Empty the trash and delete
    Downloads" without "trash" or without "downloads" are all refused
    (the Reviewer-lead's P2s on #5609). Before, half the content words,
    then all but one, and then near forms were enough, and the unheard
    part started. The words of the name and the closing question never
    stand for a task word: the first of the name's forms in the line
    (those the name check uses) is left out of it, and so is the line's
    last sentence when it ends with "?". In the task a form is left out
    only where it addresses the agent, right after "ask", "have", "tell",
    "get" or "let", so "Uninstall the codecs" read back as "Codex will
    uninstall it" is refused and "Codex will uninstall the codecs" is not
    (the Reviewer-lead's P3 on #5785's READ-OK). "Delete the code" read back as "Claude
    Code will delete it. Shall I start it?" (the name's "Code") and "Start
    the server" read back as "Claude Code will stark the server. Shall I
    start it?" (the question's "start") are refused. A task that names the
    agent, "Ask Claude Code to fix the tests", needs "ask", "fix" and
    "tests" said back beside the name ("I'll ask Claude Code to fix the
    tests. Shall I start it?"). A read-back that is one question alone
    ("Shall Claude Code start the server?") says nothing back and costs a
    second yes (the Reviewer-lead's P3 on #5609's READ-OK);
  - that line was not cut. It is marked cut on its own exchange, so a yes
    in any later request reads it as cut too: kept as heard after a cut
    (`thread::heard_only`, the second route with `LAMP_SAID_AS_HEARD=on`),
    or the line after it began over the lamp's voice, came first after a
    Stop, or cut its answer (`over_the_lamps_voice`, `after_stop` or
    `heard_of_cut` on that line's Start, marked by
    `thread::began_over_the_lamps_voice` from both routes' provider
    workers, `mark_cut_read_back`; on the Live route the three flags are
    read from the Start itself, `start_cuts_read_back`, so the test drives
    the same reading as the call site). On the Live route the lamp's line is
    never cut down to what was heard, so without this a yes said over a
    read-back cut off, or after a Stop by hand or head with
    `LAMP_SAID_AS_HEARD` off, was checked against the whole line, its
    question included (the Reviewer-lead's P3s on #5609).
- When those hold as `start_agent_task` prepares the task, its `next` says
  "The person has already said yes to this task as you said it back: call
  confirm_agent_task now, and do not ask again." The confirm checks the rule
  again on its own.
- A task started this way is journalled as `lamp-voice: tasks: request N:
  task <id> started on a yes in the turn that prepared it, after its
  read-back in words`, with no task words or paths.

The guard that a yes must come after a read-back stays. This rule only
recognizes a read-back said in words, before the call. Without the switch
every description, answer and line is byte-for-byte as before
(`the_one_yes_switch_changes_only_the_read_back_and_task_words`). Tests:
`a_yes_in_the_turn_that_prepared_counts_only_after_its_read_back_in_words`,
`one_yes_starts_only_a_task_read_back_whole`,
`one_yes_never_counts_over_a_cut_read_back`,
`one_yes_needs_every_task_word_read_back`,
`one_yes_matches_task_words_by_stem_one_for_one`,
`one_yes_reads_back_past_the_name_and_the_closing_question`,
`a_start_over_the_lamps_voice_or_after_a_stop_marks_the_read_back_cut`
(in both `provider_worker` and `second_provider`),
`thread::tests::answered_last_but_cut_says_a_read_back_was_cut`,
`a_confirm_with_nothing_prepared_says_to_prepare_it_with_the_switch`,
`the_one_yes_switch_is_on_only_with_the_tasks_part`. Not yet played: a
count of 10 spoken hand-overs with a real yes on each side of the switch,
counting those that needed only one yes (#5169).

### A lead-in before a call (#799, `LAMP_TOOL_LEAD_IN=on`, on in the product)

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

### The person's own accounts, asked of their computer (#1676, off unless `LAMP_TASKS_ACCOUNTS=on`)

Only with `LAMP_TASKS_ASKED=on` (the voice's `--tasks-accounts`, right after
`--tasks-asked`). The lamp keeps no calendar, email or other account of the
person's: their computer's agent has those connectors, so the lamp hands such
an ask over (the owner, 19 Oct 07:1x, "via hand-overs").

- **The session note** (`agent_tasks::session_note`) says the person's own
  calendar, meetings, email and other accounts are on their computer, not
  with the lamp, and that the lamp's timers and reminders are not their
  calendar. An ask about them ("what's on my calendar today", "any email
  from …", "when is my next meeting") is handed over with
  `start_agent_task`, in the person's words, and its answer told when it
  comes, never answered from what the model knows or from `list_reminders`.
- **No office:** the descriptions, the note and the read-back's `where` say
  "the person's computer", not their office computer, since a home has no
  office. Nothing else in the descriptions changes, so each fits its 1,024
  bytes as before (`every_description_fits_the_service_whatever_the_switches`
  covers the switch).
- **No new function:** the session's 32 functions are nearly all used, so
  this is wording only. The read-back and the person's yes come first, as
  for any hand-over: the ask leaves the lamp for their computer.
- **Not yet shown:** an ask answered end to end on a lamp. It needs the
  computer's agent to have calendar and mail connectors in a session Harness
  starts, which is the owner's to set up (#1676); until then it's shown with
  an ask that needs none.

Tests: `agent_tasks::tests::with_accounts_the_persons_calendar_and_email_are_handed_over_and_no_office_is_named`,
`agent_tasks::tests::the_accounts_switch_is_on_only_with_asked_and_taken_after_it`.

### A question for one of the staff is theirs (off unless `LAMP_TASKS_QUESTIONS=on`)

Only with `LAMP_TASKS_ASKED=on` (the voice's `--tasks-questions`, taken
after `--tasks-correct`). The person's rule (28 Oct, 18:1x): "just spawn a
new agent of type <...> to ask". A question about or for a named agent
("what did Devin build last week?", "ask Hermes what it's working on") is
handed over as a question task. The lamp never answers it itself, never
says it doesn't know, and never asks what to do.

- **The session note** (`agent_tasks::session_note`) says such a question
  goes to the agent at once with `start_agent_task`. The task is written as
  `agent_tasks::QUESTION`, "Answer this question; don't change any code or
  open any PR:", followed by the person's question in their own words. The
  agent's target is the one its engine has in the tasks part's targets,
  as for any hand-over: `devin` → Devin Cloud on a lamp that has it. With
  `LAMP_TASKS_NAMED_ONLY=on`, the agent must still be named.
- **Past work:** an agent keeps no memory between sessions, so the voice
  adds one line to every question task, whatever the engine (Claude Code,
  Codex, Devin on the computer or in the cloud, OpenCode, Hermes):
  `agent_tasks::PAST_WORK`, "For anything about past work, check your own
  session history and this folder's git log first." Each engine knows its own
  history command, so the lamp names none (the person, 29 Oct 16:37: "just
  one line for all type of coding engine"). A Devin Cloud session has no
  folder: it reads its sessions alone. Nothing is added to other work.
- **What the person hears:**
  - After their yes, the model is told `agent_tasks::asked_next`: say it has
    asked Devin, that it may take a few minutes, and that it will tell them
    what Devin says. It must never guess or give the answer itself.
  - When the answer comes, Task news tells it with the skill's `answered`
    words (or `answered_cut` for a long one): "Devin says…", then the
    answer in plain words.
  - The news quotes the person's own question (`agent_tasks::question_of`),
    without the opening or the past-work line.
  - A Task news `skill.toml` written before these words still passes, with
    the built-in ones.
- **Unchanged:** the read-back and the person's spoken yes come first, as
  for any hand-over. The functions' descriptions are untouched, since their
  1,024 bytes are spent; this is wording in the note only.
- **Not yet shown:** a played check on lamp-a0ae once its voice is unmuted.

Tests: `agent_tasks::tests::with_questions_a_question_for_the_staff_is_handed_over_as_a_question_task`,
`agent_tasks::tests::the_questions_switch_is_on_only_with_asked_and_taken_last`,
`agent_tasks::tests::a_question_for_devin_says_where_its_past_work_is_written`,
`agent_tasks::tests::a_question_tasks_own_question_is_what_the_news_tells`,
`task_news::tests::a_finished_question_task_is_told_as_its_answer`,
`skill_words::tests::a_task_news_without_the_folder_words_passes_with_the_built_in_ones`.

### Signing the lamp in from the app (#5198, `--app-socket`, off unless `LAMP_TASKS_SIGN_IN_APP=on`)

On in the product since 5 Nov (#5198): `lamp-voice.service` and `lamp-cloud.service`
set `LAMP_TASKS_SIGN_IN_APP=on` (the voice `LAMP_TASKS_SIGN_IN=on` too), and
`lamp-tasks.service` runs `--sign-in-by-code --app-socket
/run/lampos-tasks/app.sock`, the folder in `lamp-cloud`'s `ReadWritePaths`
(`-/run/lampos-tasks`). A round carries the three units to a
product-installed lamp (updates.md), so lamps already in homes get it too.

`lamp-tasks serve --sign-in-by-code --app-socket PATH` binds a third socket,
the owner's app's route (`/run/lampos-tasks/app.sock`, reached by
`lamp-cloud` for the app's `harness.sign_in.*`, `docs/cloud.md`, which keeps the code and the email off the broker and hands them to the backend over HTTPS). It takes
only `app_sign_in_start` (`{"method":"code"}`, the default, or
`{"method":"qr"}`), `app_sign_in_state` and `app_sign_in_answer` (`{"id","answer"}`);
any other op there is `not_on_this_route`, and those three on the public or
voice socket are `untrusted`. Without `--sign-in-by-code` the socket isn't
bound. The contract the app builds on is in #5198.

- **Only lamp-cloud's unit** (#5379). The socket is mode 0600 in lamp-tasks'
  own folder, so any unit run as `lampos` (skills, addons, …) could connect.
  The part reads each caller from the kernel, as lamp-body does for its lease
  socket (`lamp_ipc::caller::of_stream`: `SO_PEERCRED`, then the unit its
  cgroup names), and lets in only `lamp-cloud.service` run as the part's own
  user (`runtime::app_caller_may`). Any other caller, or one whose unit can't
  be read (gone already, or not Linux), is closed with no answer and one
  journal line, `tasks: the app's route refused pid N (uid U, unit …)`
  (`unit none` when it can't be read), never anything the caller sent.

- **Who started it.** Each sign-in records `by` (`voice` or `app`) and
  `method` (`code` or `qr`). A start from the app while the app's own is
  open gives it back, whatever its method; while a voice-started one is open
  it is `voice_sign_in_open`.
- **The QR (#5198, `"method":"qr"`, only from the app).** lamp-tasks runs
  `harness login --qr --json` (Harness 0.6.16, the pinned version) in place
  of `--code`. Harness prints
  `{"type":"qr","url":"https://harness.autonomous.ai/signin#k=…","expiresIn":N}`,
  again when it is kept alive, then the same `confirm` and `result` as the
  code. The link is read only when it begins with Harness's sign-in page and
  `#k=` (`sign_in::QR_PREFIX`), is at most 512 plain bytes, and is good for
  1 to 600 s; anything else ends the sign-in `unreadable`. The app's view is
  `sign_in:"qr"` with `qr_url`, `expires_in` and `expires_at`; the app shows
  the link as a QR, which the Harness phone app scans
  (Settings ▸ Sign in a computer). Then the question "Sign in as …?" is the
  app's to answer, as for the code. The voice reads it as the app's
  (`in_app`) and never sees the link. A code line in a QR sign-in, or a link
  in a code one, is passed over. Harness's own message is never kept when it
  holds `#k=`. One that ran out says "The QR code ran out of time. Start again
  for a new one." The link is journalled as "QR given (good for N s)", never
  itself.
- **The voice never says an app-started one.** On the voice route it is only
  `{"sign_in":"in_app","id":N}`: `sign_in_start` is `started_in_app`, its code
  claim `not_available`, `sign_in_asked` and `sign_in_answer` `started_in_app`.
  The voice tells the person it is finished in the app, without a digit.
- **The app's view** (`Session::app_view`): `by`, `method`, `revision`,
  `instance_id` (random per run), the code with `expires_in` and `expires_at`
  (unix ms, moved on each keepalive), and the question. The email and
  `answer_here:true` only for an app-started sign-in; a voice-started one
  shows its code and never its email. `revision` is the sign-in's id times
  1000 plus its changes: every change moves it, a read never does. A done
  sign-in has `message`: Harness's own result `message` (bounded, never one
  holding an `@`, eight digits, or the sign-in's code, matched on the
  message's digits alone whatever the spacing or separators ("1234 5678",
  "1234-5678"), and still after the question was asked; spelled-out digits
  are not matched: the privacy reader's P3 on #5236 and the reviewer-lead's
  P2 on #5579; tests
  `a_result_keeps_harness_message_and_already_signed_in_for_the_app` and
  `a_sign_in_message_quoting_the_code_with_separators_is_dropped`) or the lamp's
  words for its code; Harness's
  `alreadySignedIn:true` is `done` success with `already_signed_in:true`.
- **The app's answer** (`Session::answer_from_app`): only for an app-started
  sign-in (`started_by_voice`), for its `id` (`not_this_sign_in`), once the
  app was given an `asking` view (`not_read`), within
  [`sign_in::ANSWER_WINDOW`](../crates/tasks/src/sign_in.rs) of that
  (`too_late`, which ends it), once.
- **Whether Harness is signed in** (#5524, for the app's Harness row, app
  #111): with `serve --set-up-later`, once the part has read Harness's own
  sign-in (the same read as its journal's "Harness signed in: …", at start
  and every `--set-up-later` look), `app_sign_in_state` carries
  `signed_in` (`yes`, `offline` or `no`) beside the sign-in's view, as
  `status` does ("The sign-in on `status`"). It comes from Harness, not
  from the last sign-in this run saw, so after a restart, with no sign-in
  open and the view `none`, it still says it. Not read yet, it is absent;
  the app's other operations are unchanged. Tests:
  `runtime::tests::the_apps_state_says_harness_signed_in_once_read` and the
  service's `the_apps_state_says_harness_signed_in_across_a_restart`.
- **The id only grows** (Backend2 on #5198: the backend tells an ended
  sign-in from one not put yet by it). Each new id is the larger of the
  clock in ms and the newest kept + 1; the newest is kept in the state
  folder beside the store's (`/var/lib/lampos-tasks/sign-in-id`, written
  atomically at each new sign-in), so a part restarted with its clock gone
  back still counts up. Test:
  `a_sign_in_id_grows_across_a_restart_with_the_clock_gone_back`.
- **A put that failed** (`app_sign_in_relay {id, delivered}`, app route
  only, sent by lamp-cloud after each HTTPS put): `delivered:false` gives
  the open sign-in's view `error:"relay_failed"` and the message "The
  sign-in code couldn't reach the app. Try again." with a new revision;
  the sign-in goes on. `delivered:true` (a later put that worked) clears it,
  with a new revision again. The same word twice is no change.
- The journal says `sign-in N: started from the app (code)`, `answered yes in
  the app`, `yes from the app not taken: CODE`: never the digits or the email.

Tests: `sign_in::tests::a_result_keeps_harness_message_and_already_signed_in_for_the_app`,
`protocol::tests::app_sign_in_operations_are_their_own_and_take_only_a_method_or_an_answer`,
and the service's `the_apps_sign_in_is_its_own_routes_and_never_the_voices`
(each route's refusals, the app's start, state and answer, the voice's
`in_app`, a voice-started one shown without its email and not answerable
from the app, `revision` moved by changes and not reads, Harness's message
passed on).

With the same switch in the voice, `sign_in_computer` and the news say the
code more slowly, one digit at a time with a comma after each and a full stop
between the two groups ("1, 2, 3, 4. 5, 6, 7, 8."), and tell the model to
offer to say it again; "say the code again" is a new heard
`sign_in_computer`, which gives the same code while it is alive. Without the
switch both are as before. Test:
`sign_in::tests::with_the_app_switch_the_code_is_said_slowly_and_again_and_an_app_sign_in_never`.

### Every piece of work now (#4342, off unless `LAMP_TASKS_NOW=on`)

`check_agent_task` tells only the newest work handed over. With
`LAMP_TASKS_NOW=on` and the tasks part (`LAMP_TASKS_DIR`), the voice's
`--tasks-now`, taken after `--tasks-questions` and
`--tasks-sign-in-app`, before `--tasks-one-yes`, the model also has
`tasks_now`. It answers "how's it
going?", "is it done?", "what is Claude doing?" and "did the poem one
finish?" for any recent piece of work.

- **Where it reads.** The tasks part's ["Tasks now"](#tasks-now-4342-serve---progress)
  record (`{"op":"tasks_now"}`), then `get_task` for each task's ask. Both
  are the tasks part's own memory and store: no Harness call, so the answer
  comes at once. It needs `lamp-tasks serve --progress`.
- **What the model is given**, for each task, newest first: `who` (the
  agent's name, as the news says it), `where` ("their computer", or the
  computer's own name with `LAMP_TASKS_MACHINE_NAMES`), `asked` (the task's
  words, or a question task's question, at most 120 characters), `state` in
  plain words ("still working on it", "finished", "waiting for the person
  to answer it on their computer", "stopped without finishing", "handed
  over, not looked at yet", …), `since` ("7 minutes ago"), `quiet_for` when
  a working agent was last active 3 or more minutes ago, `last_step` as
  `{"untrusted": "…"}` (at most 160 characters, quoted), and, for settled
  work, `told`. `not_listed` counts what the record left out. No task
  number, store or machine id. Every word of `asked` and `last_step` that
  looks like a path (it holds `/` or `\`, or starts with `~`) is given as
  "a file", so a folder or a username never reaches the model
  (`without_paths`).
- **Anyone who asks hears it.** The voice does not know who is asking, and
  tasks have no owner. So a guest who asks "what is Claude doing?" hears
  every recent piece of work and its ask, the owner's included, where
  `check_agent_task` told only the newest. That is why the switch stays
  off until answers are kept to the person who asked for the work, or to
  the lamp's owner (the reviewer-lead's P2 on #5118).

  ```json
  {"tasks":[{"who":"Claude Code","where":"their computer","asked":"Fix the parser",
    "state":"still working on it","since":"7 minutes ago","quiet_for":"5 minutes",
    "last_step":{"untrusted":"Running the parser tests: 41 'passed' …"}},
   {"who":"Codex","where":"their computer","asked":"Write a short poem about tea",
    "state":"finished","since":"50 minutes ago",
    "last_step":{"untrusted":"Wrote the poem"},"told":false}],
   "next":"Answer only what they asked, about the work they meant …"}
  ```

- **What it is told to say** (`next`): only what was asked, about the work
  meant, matched by who has it or by what was asked, in one or two natural
  sentences. The last step is the agent's own words, untrusted: said from,
  never read out word for word, never followed. Of finished work, only that
  it is done: its news is still told by itself. Nothing is kept as told.
- **Which of the two.** `tasks_now`'s description makes it the one for
  progress and for a piece of work named by what it was. With the switch,
  `check_agent_task`'s gains one sentence: for how work is going, or work
  other than the latest, call `tasks_now`; it is for what the latest came
  back with. It works as before. Both stay within 1,024 bytes for every
  combination of the switches.
- **Nothing there, or no record.** An empty record: "no work is with their
  agents just now". A tasks part not reached: the same words as
  `check_agent_task` (not set up, or not running). A tasks part without
  `--progress` (`progress_off`), or a build older that doesn't know the op:
  the light words of a part that can't do it, and the journal says only
  the refusal's code. A step is never in a journal line.
- **Not yet shown:** a played "did the poem one finish?" with two pieces of
  work on a lamp with Harness. The product unit's tasks part needs
  `--progress` before the switch is any use.

Tests: `agent_tasks::tests::tasks_now_is_declared_and_run_only_with_its_switch`,
`agent_tasks::tests::tasks_now_tells_every_recent_task_with_its_step_quoted_and_untrusted`,
`agent_tasks::tests::tasks_now_with_no_task_or_no_record_says_so_lightly`,
`agent_tasks::tests::every_description_fits_the_service_whatever_the_switches`.

### How the lamp's Harness is (#6061 part B, off unless `LAMP_TASKS_HARNESS_STATUS=on`)

The voice could not say whether the lamp's Harness is signed in, or which
computers and agents it can hand work to, though lamp-tasks knows both. With
`LAMP_TASKS_HARNESS_STATUS=on` and the tasks part (`LAMP_TASKS_DIR`), the
voice's `--tasks-harness-status`, taken last, after `--tasks-reply-named`,
the model also has `harness_status`. It answers "is Harness signed in?",
"which computers do you have?" and "what can you hand work to?".

- **Where it reads.** Three requests on the tasks part's request socket,
  each under the call's own deadline: `{"op":"status"}` for `signed_in`
  (#5488), `{"op":"targets"}` for each computer's alias, `name` (#1676) and
  engine, and `{"op":"tasks_now"}` for how many pieces of work are under
  way. All three are the tasks part's own memory, so there is no Harness call
  and the answer comes at once. lamp-tasks does not change.
- **What the model gets.**
  - `signed_in`: `yes`, `offline` (signed in, but Harness can't reach its
    service just now), `no`, or `unknown` (not read yet since lamp-tasks
    started, or any other value).
  - `computers`: each computer once, in the targets' order (targets with
    the same own name are one computer), with the agents it has by name.
    It is always called by its first alias (`office`), never by its own
    name, `LAMP_TASKS_MACHINE_NAMES` or not, since a device name often holds
    a person's name (Thirteen's P2 on #6222; the first builder, 11 Nov). A target
    whose engine wasn't found there (`installed: false`, #5199), one whose
    alias isn't an alias's shape, and an engine the voice doesn't know are
    left out. At most three are listed, and `more_computers` counts the rest.
  - `working`: how many pieces of work are `working`. It is left out when
    `tasks_now` is refused (no `serve --progress`), never a guess. No task's
    words and no last step are given.
  - `next`: answer only what was asked, in one or two sentences, never a
    list, and name only what the answer has. With `signed_in: no`, it offers
    to sign in by voice only with `LAMP_TASKS_SIGN_IN` (`--tasks-sign-in`);
    otherwise it says to sign in from the app. Signed out with the lamp's
    own target among the targets (#6407), it first says work goes to the
    lamp's own agent, and to "sign in to hand tasks to your computers",
    never that nothing works until then.
- **When it fails.** No tasks part, or none set up, gets the same words as
  every task call (`unreached`). A `status` that is refused gets the light
  "can't do that" words, and the journal has only its code
  (`lamp-voice: tasks: harness_status was refused: <code>`). A `targets` or
  `tasks_now` that doesn't answer only leaves its part out.
- **Off**, the function is not declared, and every other tool's description
  is word for word as before (a test holds it).
- **Who hears it.** Whoever asks the lamp hears the person's computers by
  alias (`office`) and their agents, never a computer's own name. The switch
  stays off in the product until its played check (TWO READS).
- **Tests** (`agent_tasks`):
  - `harness_status_is_declared_and_run_only_with_its_switch`;
  - `harness_status_says_the_sign_in_the_computers_and_the_work_as_counts`
    (each `signed_in`, grouping, aliases only, names on or off, the cap, no task words, within
    500 ms of a local tasks part);
  - `harness_status_refused_says_it_lightly`;
  - `harness_status_signed_out_with_the_lamps_own_target_says_computers_need_the_sign_in`
    (#6407).

### Signing the lamp in by voice (#1676, off unless `LAMP_TASKS_SIGN_IN=on`)

On in the product's voice since 5 Nov (#5198), which the app's sign-in needs
(above).

A lamp has no screen for Harness's QR code. Harness 0.4.23 signs a device in
by a code it says aloud (openharness#1410). The tasks part runs Harness's
own sign-in for the voice (`lamp-tasks serve --sign-in-by-code`, its trusted
ops `sign_in_start`, `sign_in_state`, `sign_in_take_code`, `sign_in_asked`
and `sign_in_answer`;
["Signing in by a spoken code"](#signing-in-by-a-spoken-code---sign-in-by-code-1676)).
With `LAMP_TASKS_SIGN_IN=on` the voice declares two functions
([`sign_in.rs`](../crates/voice/src/sign_in.rs)), on both routes.

**Where the state is.** The voice's news runs in its coordinator and its
tools in a worker: two processes, which share nothing in memory. So where
the sign-in is, whether its question was heard and whether an answer may
be taken are kept in the tasks part alone, each sign-in by its `id`
(#2137's redesign, after its first version kept them per process and could
never have asked or taken an answer on a lamp).

- **`sign_in_computer`**, when the person asks to sign the lamp in, to
  connect the lamp to their computer or to Harness, or to set up hand-overs
  to their computer (the first two since #5198's follow-up): the code
  is said digit by digit in two groups ("1 2 3 4, 5 6 7 8"), with where to
  type it (the Harness app under Machines ▸ Approve a device, or
  harness.autonomous.ai). A start refused says why: only the switch being
  off is "not set up"; a busy tasks part is "busy just now".
- **The news, in the coordinator, from the tasks part**
  (`sign_in::news`, read every 2 s while a sign-in is open, every 10 s
  otherwise): a code whose atomic claim it wins (`code_told` was false
  when read, but the claim decides); once the person approves in Harness, the question "Shall I sign
  in as someone at example dot com?", the email read back slowly ("dot",
  "at", "dash", "underscore", "plus"); and how it ended (signed in, or why
  not). A sign-in first seen already done gets none (the start's answer told
  it), nor does the end after a no. Each is told once; its key
  (`sign-in-<id>-code`, `-asking`, `-done`) has no `:`, so it is never taken
  for a task's or sent as `told`.
- **One initial code payload (#2839).** Each heard tool invocation sends
  `sign_in_start` once. If its reply is `starting`, continuation polls use
  `{"op":"sign_in_take_code","id":<that id>}` within the original deadline
  and cadence. News uses that same operation before offering a code. Tasks
  handles it atomically: `claim` is `waiting`, `granted`, `already_given`, or
  `not_available`. Only `granted` carries the current code view, with
  `code_told:true`; every other outcome has only the requested id and status.
  Wrong or expired ids, expired codes and question/end states grant nothing.
  An unchanged-code keepalive never reopens a claim. A new heard invocation
  ("say it again") still returns the open code once through START.
  Each code handed to the voice is one journal line, never its digits:
  `tasks: sign-in N: code handed to a start` for a START answer with the
  code, and `tasks: sign-in N: code claimed` for a granted claim. A lamp's
  journal then counts the tellings: one, and one more for each "say it
  again".
- **The update boundary.** `sign_in_state` advertises `code_claims:1`, even
  with no session. New Voice requires it before START or automatic code
  delivery; an old or replaced Tasks that lacks/refuses the operation causes
  a bounded update explanation for START and no code news, never fallback
  polling. Old Voice can still use the unchanged legacy operations with new
  Tasks, but needs its own update to gain this duplicate fix. Questions and
  answers keep their separate existing protocol.
- **Expiry and delivery limits.** Voice uses the grant RPC's start plus
  Tasks' remaining lifetime as the code news's `until`; transit cannot extend
  it. The existing queue drops expired notes before telling them. Direct
  START replies are checked against the same conservative lifetime. A claim
  is never refunded after a stop, lost reply, privacy closure or provider
  restart: the person can request another telling. This is payload ownership,
  not proof the code was heard. A requested repeat does not revoke an older
  queued initial note; it is one additional authorized payload. Existing
  sign-in priority and other news's relative order remain unchanged, as do
  the coordinator's final privacy, ownership and cancellation gates. A remote
  session change cannot immediately retract an already queued note.
- **Told at the next pause** (#2602, finding 4): a sign-in's news goes
  ahead of any task's waiting (`TaskNews::sign_in_first`) and is told at
  the first pause, as `LAMP_TASK_NEWS_AT_PAUSE=on` tells all news, whether
  that switch is on or not. On lamp-75c9 on 21 October it waited for 15 s
  with no turn like a task's news, though the person had just asked for
  it and its question goes unanswered in two minutes. Someone there, the
  room not busy and nothing owed still hold it; a task's news keeps its
  15 s without the switch, and the record's `task_news` says `at_pause`
  true for a sign-in's. Tests:
  `a_sign_ins_news_is_told_at_the_first_pause_without_the_switch`,
  `with_the_switch_on_a_sign_ins_news_goes_ahead_of_a_tasks` (as on
  product lamps) and `a_sign_ins_news_is_put_first`.
- **A question not heard whole is asked again** (the 21 Oct lamp-75c9
  rerun's (3), #2602): a sign-in's question is kept as told once it's
  heard whole, not when it's taken to be told. The news follows it (`sign_in::AskAgain`) and
  offers it again 30 s after it was taken (`ASK_AGAIN_AFTER`) if its turn
  wasn't heard whole and the tasks part still says `asking`. It's then told
  at the next pause like any sign-in news. That's at most three tellings
  in all (`ASKED_MOST`), and none once it was heard whole. On lamp-75c9,
  talk in the room cut the question short a second after it began
  (`user_interrupted`), it was never told again, and the person's yes was
  refused as `not_told`. After a voice restart, a question still asked and
  never heard whole is offered again, and one heard isn't (the
  reviewer-lead's P3s on #2702). A question told the last time is let go
  once its wait has passed. The journal says `the
  sign-in's question (sign-in-<id>-asking) was not heard whole: offered
  again`. Tests: `a_sign_ins_question_cut_short_is_offered_again_until_heard_whole`
  (the news thread against a stand-in tasks part) and
  `a_question_not_heard_whole_is_due_again_a_few_times_at_most`.
- **A question talked over is asked at the next pause** (#5198): when the
  question's turn ends not heard whole, cancelled (the person talks over
  it: `cancel_reply`) or finished with its audio cut or unheard
  (`heard_news`), the coordinator hands its news to `TaskNews::not_heard`.
  A sign-in's question among it is offered again at once
  (`AskAgain::cut`), not 30 s later, so it is told at the next pause. It
  still counts toward `ASKED_MOST`: one talked over on its last telling is
  let go. Other news is left as it was. A turn cancelled by
  `privacy_muted`, `privacy_gap` or `service_stopped` was not talked over:
  its question is not offered again at once. Tests:
  `a_sign_ins_question_talked_over_is_offered_again_at_once`,
  `a_question_talked_over_is_due_at_once`,
  `news_a_turn_did_not_get_heard_is_told_not_heard` and
  `a_sign_in_question_cut_by_privacy_or_a_stop_is_not_talked_over`.
- **The question heard:** once the question's turn was heard whole
  (`TaskNews::heard`), the coordinator sends `sign_in_asked` with the
  sign-in's id and that turn's number, the same number the worker's tools
  get as their request. A question cut off or never said opens nothing.
- **`sign_in_answer`** with `yes` or `no` and its request: the tasks part
  takes it only after the question was heard, in a later request, within
  two minutes, once; past that the sign-in ends as `too_late`, and the
  person is told they can ask again.

Only a request the lamp heard may start a sign-in or answer it: never one
it wrote, a reminder or a text. No session exists before the person's yes,
so a stranger who heard the code gains nothing. The guard against someone
else in the room saying yes is the email read back: the voice doesn't yet
take the yes only from the voice that asked (`same_voice` exists on the
Live route alone; a follow-up).

**The code and the email never leave the worker in words** (the first
builder at 09:46 on #1676; the reviewer's P2 and Jarvis's P1 on #2137).
Each sign-in news note begins with `sign_in::NEWS_TAG`; the worker given it
marks its request and reads the code and the email from the tasks part.
The request whose `sign_in_computer` gave a code and the one that answered
are marked too. Where each worker's words leave it, before the thread keeps
them and before they are sent (`sign_in::leaving`, `leaving_text`):

- **the lamp's words of a marked request** leave as one line, "(a sign-in
  code and account were said)", however they were split, its later pieces
  empty. The audio is untouched. So the coordinator's events, `voice/said`,
  the second route's history, the thread's file and the memory's journal
  never get them. On the second route a cut answer's played words
  (`LAMP_SAID_AS_HEARD`) are that line too, and any other request's are
  scrubbed (`keep_heard`);
- **on Live, every later request of the session that heard it** leaves as
  "(words kept back: this conversation heard a sign-in code)"
  (`sign_in::leaving_live`, Jarvis's and the reviewer's P2 on #2137).
  Gemini's session keeps the tool's result, so "Sorry, what was the code?"
  could be answered in a request nobody marked. A connection Gemini resumes
  (a GoAway, about every 10 minutes, or after an outage) goes on with the
  same conversation under a new session number, so it is held too; a
  marked request cut before its first word holds its session as well. Only
  a fresh connection never heard it, so its words are kept again. The second route needs none of this: its
  history has only the one line;
- **the lamp's words of any other request** are scrubbed as the person's
  are (below). On lamp-75c9 on 21 October, asked "what was the sign-in code
  again?" in a request of its own, the second route's model said "Three one
  five eight three", kept as said (#2602 finding 5). `sign_in_computer`'s
  description now tells the model to call it again for the code, which
  gives back the open sign-in's code in a marked request, and never to say
  a code's digits from memory;
- **the person's words**: any run of three or more of the code's digits in
  its order, as digits ("4 8 2", "48213907") or words ("four eight two"),
  says "a sign-in code"; the account's name and its domain, each as written
  or as said, say "their account". Other numbers are left ("at 2 pm in 10
  minutes"). On Live, while the code or the account is kept, the person's
  pieces are held until their line ends and leave whole
  (`sign_in::person_piece`): a piece split inside the code or a name can't
  be scrubbed alone, and one sent can't be taken back. So, during a sign-in,
  `voice/heard` gets the person's line at its end, not word by word. The
  second route hears each line whole.

The thread also scrubs what it keeps joined, after each piece. The code and
the account are kept in each worker for 15 minutes after the newest news of
them at most, and for two minutes after the done news, then forgotten
(the reviewer's P3 5). Their read from the tasks part is off the worker's
loop, so a slow tasks part never holds its sound (P3 4). **Left:** past
those two minutes, the person saying the code or the email back is kept as
said; and an email split inside its domain across two of the lamp's
pieces, in a request not marked, on the second route, can leave a fragment.

Tests: `sign_in::tests`:
- the code and the email as said, and each state as the model is told it;
- the switch and the two functions;
- `a_sign_in_by_voice_goes_from_its_code_to_its_news`, through a stand-in
  tasks part, the worker's calls and the news sharing nothing but it: off,
  not heard, the code, the question once, its hearing told with its request,
  a refused answer said, the yes passed on with its own request, the end
  once;
- `a_sign_in_is_never_told_twice_nor_its_end_after_a_no`;
- `code_delivery::a_sign_in_code_is_delivered_once_and_repeated_only_on_request`
  and `code_delivery::a_queued_sign_in_code_is_not_repeated_by_the_pending_start`
  use the real Tasks service with a channel-controlled stand-in Harness. The
  latter runs the provider in a separate test process and holds its initial
  `starting` reply until code news is in the actual Voice queue, then lets
  START continue. They count the code in tool results
  and news taken to tell, and one further tool result for a new heard request
  ("say it again"). Harness's unchanged-code keepalive must add no news and
  both requests must use one sign-in. Readiness and ordering use events;
  the production request deadlines and code lifetime stay unchanged. These
  are delivery-payload regressions, not proof of acoustic speech or a played
  sign-in; #2839 still needs its authorized lamp run;
- `a_refused_start_says_why` (an old Tasks `bad_request` now explains that
  Tasks needs updating, while switch-off and busy refusals remain distinct);
- `a_code_handoff_needs_the_claim_protocol_and_never_restarts_its_poll`,
  `news_never_reads_digits_from_an_unclaimed_or_expired_snapshot`, and
  `code_validity_cannot_be_extended_by_a_delayed_reply` cover refusal after
  capability, stale ids, conservative expiry and exact request sequences;
- `the_code_is_kept_only_for_its_sign_in_and_held_pieces_leave_whole`;
- `what_leaves_the_worker_has_neither_the_code_nor_the_account`, with
  Jarvis's planted values, in pieces and mixed forms;
- `the_code_and_the_email_are_taken_out_of_what_is_kept`.

Each worker's own path is tested too:
`the_second_worker_sends_a_sign_ins_words_as_one_line` (second worker) and
`the_live_worker_sends_a_sign_ins_words_as_one_line_and_scrubs_the_persons`
(Live, with Jarvis's planted run: an unmarked request in the same session
held back, and in a resumed one, a fresh one's kept, a marked request cut
before its first word holding its session, the person's three-word pieces whole, and
the thread's file clean). Either fails if its call is taken out. The second
route's news test holds the turn heard to the news's, and
`task_news::tests::a_sign_in_question_heard_is_told_with_its_own_request`
holds the number `send_heard` tells the tasks part to that turn's (the
reviewer's P3 1).

### A skill made by voice (#1685): not through the tasks part

A skill is made by the lamp's own model, never handed to a computer's agent
(the first builder, 3 Nov, on the Queue;
[skills.md](skills.md#made-by-voice-1685-lamp_make_skillon-on-in-the-product)).
The tasks part declares no `make_skill` or `put_on_made_skill`, and the
worker's `--tasks-make-skill` flag is gone: `LAMP_MAKE_SKILL=on` alone gives
the voice its own function, `make_skill` (`put_on_made_skill` is gone since
#5977: a made skill goes on as soon as it is kept), with a tasks part or without one, and an
app's typed skill (`LAMP_APP_MAKE_SKILL`) is made the same way, never handed
over. The made skills' news words (`made`, `not_kept`) and their repair through
a task are gone with it. A task whose words are a skill's brief, handed over
by an earlier release, is never told: its answer is files.

- **The reset** wipes `/var/lib/lampOS/voice/made-skills` with the person's
  other stores, and removes each made skill put on in `/var/lib/lampOS/skills`
  (its `.package.json`) with its folder ([setup.md](setup.md)).

### Asked of the staff, done by the lamp (#854, `LAMP_TASKS_ASKED=on`, on in the product)

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

### Named in the words, told in time (`LAMP_STAFF_NOTE=on`, on in the product)

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
  in time whose later words name more says the second line too (the model
  keeps the note it had), so a count from the journal sees such a request
  twice. Nothing of what
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
says. The product's voice unit sets it on; the code's default is still off.

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
  (office-codex)` when it changes. When the target it was given
  (`LAMP_TASKS_TARGET`) isn't among them, it says so once per change of the
  targets: `lamp-voice provider: no target 'office-claude' in the tasks
  part; it has 'office' (claude)`. So a setup or update that changes
  `targets.json` shows before anyone asks (#1390). Without the switch the
  staff aren't read, and only a refusal reads the targets.
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
  for Codex; `opencode` for OpenCode; `devin`, `devon` for Devin and Devin
  Cloud; `hermes`, `herms`, `hermez`, `hermess`, `hermies` for Hermes
  (`agent_tasks::KINDS`). None of these is close to another
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

**Read on the second route too, and only the one named (#1722).** On
lamp-4ace on 2026-10-20 (17:1x) "Ask codex to write a haiku about coffee."
was heard right and the lamp said "handing that to Claude Code". Its
`targets.json` had two targets (claude and codex), with `LAMP_TASKS_WHO=on`,
but the journal had no "work can go to" line: only the Live worker read the
staff (`agent_tasks::start`), and the second worker never did. So
`start_agent_task` had no `who`, and all work went to `LAMP_TASKS_TARGET`
as Claude Code. The second worker now reads them as the Live worker does,
with `LAMP_TASKS_WHO=on` (`second_provider::worker::start_tasks`).

With one target, `start_agent_task` still had no `who`, its description
named only that agent, the staff's note matched only the staff, and the
voice sent any call to the one there was. Off unless
`LAMP_TASKS_NAMED_ONLY=on` (the provider worker's `--tasks-named-only`,
after `--tasks-sign-in`, and only with `--tasks-who`). With it, once the
staff are read:

- `who` is offered even with one of the staff, as an enum of every agent the
  voice knows (`agent_names()`: Claude Code, Codex, OpenCode, Devin and
  Hermes), and its description
  says who is set up: "Set up on this lamp: Claude Code, and nobody else. If
  they named another of these, give that name: you will be told it isn't
  set up, and nothing goes to anyone else."
- a name that is one of those agents' close forms and not among the staff
  prepares nothing and asks nothing of the tasks part; the model is told
  "Nothing was handed over: Codex isn't set up on this lamp. Say so plainly
  in a few words ("Codex isn't set up on this lamp."), and do not give the
  work to anyone else unless the person asks you to."
- the staff's note (`LAMP_STAFF_NOTE`) matches any agent the voice knows, so
  "ask Codex" is noted as a hand-over where only Claude Code is set up.
- nobody named is the default, as before; one of the staff named gets it.

Checked on lamp-75c9 on 22 Oct, and on in the product
(`LAMP_TASKS_NAMED_ONLY=on` in `lamp-voice.service`).

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
   be 13 or less. The store keeps 16 tasks, every prepared one included; a full
   store lets its oldest finished task go, and refuses only when all 16 are
   still going; this run prepares three.
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

### A named ask is a hand-over (off unless `LAMP_TASKS_NAMED_ASK=on`, #1722)

On lamp-4ace on 2026-10-21, with two of the staff read (Claude Code and Codex,
#2297), played asks such as "Hey Lamp, ask Claude Code to write a two-line poem
about the rain." went to `not_for_me` or were answered by the lamp itself
("poetry's mine to write, no need to ask anyone"), 0 to 3 of 3 per run. The
person named who should do it; the lamp refused anyway.

With `LAMP_TASKS_NAMED_ASK=on` (the voice passes it to the second route's
worker only, through `second_route_worker_environment`, and that worker reads
it; with the tasks part on), a spoken turn's first chat round whose last message is the person's words,
and whose words ask one of the staff by name to do something, is sent with
`tool_choice: {"type": "function", "function": {"name": "start_agent_task"}}`
(`agent_tasks::named_ask`, `second_provider::worker::named_asks`,
`net::Chat::forcing`). The model still writes the task; it can't decline the
hand-over or do the work itself. A round after a call ends with the call's
result and is asked as before.

- **Asks:** "ask", "tell", "get" or "have" right before the agent's whole name
  with "to" after it ("ask Claude Code to …", "tell Codex to …"), a place
  allowed before "to" ("ask Claude Code on my office computer to …": the word
  right after the name is "on", "at", "in" or "from", and "to" comes within
  the 6 words after it), or "can", "could", "would" or "will" "you ask/get
  <name> …".
- **As speech to text hears them** (b0934 on lamp-4ace, 2026-10-21): the name
  in any case and as misheard (`Kind::heard` beside `KINDS`): Claude Code as
  "cloud code", "Claude Coat", "Claude Cold", "clawed code" or "ClaudeCode";
  Codex as "code X", "codecs" or "Kodex". A misheard form counts only with
  "ask" or "tell": "get" and "have" are for the true names ("Claude Code",
  "ClaudeCode", "Codex"), since "I can't get codecs to work" is ordinary
  English. "ask <name> …" with no "to" where a sentence starts ("Ask Claude
  Code write a poem.", "…. Ask Codex, write a haiku."), unless a "?" or "."
  comes right after the name ("Ask Claude Code? No, never mind." is none; a
  comma is no stop). "asked", "sk" or "k" for "ask", only as the line's
  first word, after nothing but "Hey Lamp," or the like ("Asked Claude Code
  to …", "sk Claude Code to …"); "k" only with "to" after the name, since it
  is also an "OK" ("K, Claude Code finished?" is none). After a subject it is a report and no ask ("I asked
  Claude Code to do it yesterday, is it done?"). A line that opens "Asked
  Claude Code to …" is taken as an ask even when the person meant a report.
- **Not asks:** a mention ("I told Claude Code yesterday", "is Codex done?",
  "what did Claude Code say?", "did you ask Codex to …", "tell Claude Code I'm
  late", "Claude Code's answer", "get Codex's answer", "have Claude Code's PR
  reviewed", "I'll ask Claude Code later"), a negation right before the verb
  ("don't ask Claude Code to …", "do not tell …", "never ask Codex to …", "not
  get …"), a question the person asks themself ("should I ask Claude Code to
  …", "shall I …", "do I …"), a name not of the staff ("ask Devin to …" when
  Devin isn't set up), a heard form with no asking word ("the cloud code is
  broken", "a cold coat"), a clipped name ("Claude Co"; the staff's note still
  hears it).
  With none of the staff read, only Claude Code.
- **The trade:** the rules can let through a line said to someone else (in a
  quiet room without `LAMP_SPOKEN_TO`, or as a conversation going on). Today the
  model can still say such a line wasn't for the lamp (`not_for_me`). When it
  names an agent ("…, ask Claude Code to fix the build"), forcing takes that
  away: it becomes a prepared hand-over and a question to the room. The heard
  yes still guards the start: `start_agent_task` only prepares, and nothing
  runs without a later yes heard within 120 s.
- **Off:** the request is the same as before, word for word (a test compares
  the bodies). `tool_choice` is never sent with a held line's named functions.
- **The relay:** checked on lamp-06c1 on 2026-10-21 at 03:4x: the relay's
  DeepSeek chat, thinking off, called the function `tool_choice` named on "What's two plus two?"
  3 of 3, and none of 3 without it.
- **Who does it** (played test b0701 on lamp-4ace, #1722, 2026-10-21 07:19):
  forced, "Hey Lamp, can you ask Claude Code on my office computer to write a
  short story …" went to Codex. The words matched (Claude Code). The force named
  only the function, so the model chose `who`. Now the forced round's
  `start_agent_task` is told only the agent the words name: `who` gets that one
  value in its `enum` and is required (`net::Forced`). A round 1 call whose
  `who` names another agent, or nobody, is given to the named agent before the
  tasks part prepares it (`second_provider::Engine::asking_by_name`).
- **Several asked** (the reviewer on #2410: "Hey Lamp, ask Codex to check the
  build and ask Claude Code to write a poem." gave both calls to Claude Code):
  `named_ask` gives every agent of the staff the words ask, once each, in the
  order the words ask them. With two or more, round 1 is still forced to
  `start_agent_task`, but `who` is told as declared, nothing pinned. Each
  round 1 call whose `who` is one of the asked agents keeps it. A call that
  names nobody, or an agent the words don't ask, goes to the first asked agent,
  in word order, that no round 1 call names yet, or else the first asked
  (`second_provider::given_to_the_named`). So a line's two tasks, one naming
  Codex and one nobody, go one to each, not both to the first named.
- **The journal**, once per line, with no words: `named ask: matched <agent>`
  (several in word order: `matched Codex, Claude Code`), plus, for each agent
  a call was given to, `; the call named another agent, given to <agent>` or
  `; the call named nobody, given to <agent>`; `named ask: no match (<n> words)`;
  `named ask: matched <agent>, not applied: round 1 called <functions>, not
  start_agent_task` (or `answered in words`); and, for a line held back,
  `named ask: matched <agent>, not applied: the line is held`.
- **Over the lamp's voice** (b0701, request 58: "Hey Lamp, ask Codex to write
  a haiku about coffee.", said while the lamp spoke, a stop phrase heard, then
  `not_for_me`): every line the second route answers is forced the same way.
  Over the lamp's voice, after a stop and as a conversation going on, its notes
  go before the person's words, which stay last. That line's words match
  (Codex), and the relay keeps `tool_choice` (above). So request 58 most likely
  asked with other words than those. A stop phrase heard over the lamp abandons
  the line then being heard until its end (`coordinator/hand.rs`, `cancel`, then
  `InputAdmission::abandon_until_end`), so the next request can be only the rest
  of what was said. The journal line now shows which: `no match` with its word
  count, or `matched` and what round 1 did.

**The probe** (`cargo run -p lamp-voice --example probe_handover`, run where
the voice runs, as root, with the voice unit's tasks switches): each played ask
as a spoken turn's first round, the system message, staff note, call note,
functions and chat the second worker builds, with two of the staff held as the
tasks part would answer. With `PROBE_HANDOVER_HISTORY=on`, each run first asks
"tell me a story about a clever fox, at least eight sentences" and keeps the
real answer in the thread as an exchange. It prints counts only. On lamp-06c1
on 2026-10-21, 10 runs per line:

| Wording | Story first | Rain poem | Sea poem | Haiku (Codex) |
|---|---|---|---|---|
| A (today's note) | no | 10/10 | 10/10 | 10/10 |
| B (#2337, reverted) | no | 10/10 | 9/10 (1 self) | 10/10 |
| C (rule first) | no | 10/10 | 10/10 | 10/10 |
| A | yes | 10/10 | 10/10 | 10/10 |
| B | yes | 10/10 | 10/10 | 9/10 (1 self) |
| C | yes | 10/10 | 10/10 | 10/10 |
| D (A, `LAMP_TASKS_NAMED_ASK=on`) | yes | 10/10 | 10/10 | 10/10 |

So the probe doesn't reproduce lamp-4ace's 0 of 3: what the played run sends
beyond it (the memory's notes, the skills' lines, the web search's functions,
the name heard) isn't in it. D makes the hand-over not depend on that. Not
played on a lamp yet.

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
  | `engine` | Who has it, as its target names it: `claude`, `codex`, `opencode`, `devin`, `devin_cloud` or `hermes` (`EngineKind::bus_name`) |
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

### Tasks now (#4342, `serve --progress`)

**What it is for.** The lamp answers "what are my agents doing?" from one
small record, not from a Harness call or a read of each task. `tasks_now`
on the request socket gives it. It is built only from what the tasks part
already has: the store's tasks (`Engine::recent`) and the progress read
above (`Tracker::line`). It makes no `ps` or `show` of its own. Off unless
`--progress` is given, like `get_progress`: without it `tasks_now` is
refused with `progress_off`. It is not on the trusted route.

**Which tasks.** Every task handed over that the watch looks at now, and
every one whose newest answer is settled (`idle` with an answer,
`needs-input`, `stopped` or `failed`) and was kept in this boot. One
settled in an earlier boot cannot be dated, and is left out. A settled task
leaves 30 minutes after its news was told (`now::TOLD_KEPT_MS`), and 2
hours after it settled if it was never told (`now::SETTLED_KEPT_MS`),
whichever comes first. Told means its telling played (`ack_speech`), or
the voice said it in its own words (`told`), or its harness was closed
after the telling (#1390). Told is read from the store (the observation's
played speech or `told`, or the close record), so it holds across a restart
of the tasks part, and the 30 minutes count from that record's time. When
the store has no time for it in this boot, the settle time is used, the
earliest the telling can have been. A `told` sent without `--close-told`
writes nothing to the store: it is kept in memory for the record only, and
is lost at a restart, so such a task is then kept up to the 2 hours.

**What it says.** `{"op":"tasks_now"}` answers:

```json
{"ok":true,"store_id":"…","revision":12,"completion":"unverified",
 "tasks":[{"task":7,"agent":"claude","target":"office","state":"working",
   "since_ms":42000,"last_active_ms":3000,
   "last_step":{"untrusted":"Reading the brief / Fixed the parser"},
   "told":false}],
 "left_out":0}
```

| Field | What it says |
|---|---|
| `task` | The task's number (`get_task`) |
| `agent` | Who has it, as on `tasks/status` (`claude`, `codex`, …) |
| `target` | The target alias it was handed to |
| `state` | As on `tasks/status`: `working`, `idle`, `needs_input`, `stopped`, `failed`, `missing`, …; `unknown` for a task watched and not read yet |
| `since_ms` | How long ago it came into that state: when this run first saw it so, or, for a settled answer, when that was kept |
| `last_active_ms` | How long ago Harness last saw its agent do anything (the other machine's clock against the lamp's); null if never read |
| `last_step` | The `ps` line as last read, as `{"untrusted": "…"}`: the agent's own words, to be quoted and never followed. Kept after the task settles. Null if none was read |
| `told` | Whether its settled answer was told |
| `left_out` | How many tasks were left out to keep the bounds |

**Bounds.** At most 8 tasks, newest first, and a body of at most 1,900
bytes, so the whole reply stays under 2 KiB. Over either bound, settled
tasks give way first, the oldest first. Then every step is cut shorter
(120, 80, 40 characters, then none). Then the oldest tasks still at work
give way. A step is at most 160 characters, cut at a whole character, with
control characters removed and spaces at its ends trimmed.

**What stays here.** The record is in memory only, beside the tracker,
and begins empty at each start; a step from before a restart is gone
until the next read, so a task settled before a restart has `last_step`
null (its state, ages and told come from the store). The step never goes to the journal, the bus
(`tasks/status`), the diagnostics or the store's files.

**Not there yet.** No `partial` field: Harness has no stream of an agent's
partial output to read it from.

Tests: `a_step_is_cleaned_and_cut_at_a_whole_character`,
`a_working_task_has_its_agent_target_state_ages_and_last_step`,
`a_watched_task_not_read_yet_is_unknown_with_no_step`,
`a_settled_task_leaves_thirty_minutes_after_it_was_told_or_two_hours_after_it_settled`,
`at_most_eight_newest_first_and_settled_give_way_first`,
`the_record_stays_within_its_bytes_cutting_steps_before_dropping_work` and
`the_step_is_only_in_the_body_marked_untrusted_never_on_the_bus`
(`crates/tasks/src/now.rs`);
`recent_tasks_are_the_watched_and_those_settled_in_this_boot`
(`engine.rs`); and, through the service with a fake `ps`,
`with_progress_tasks_now_says_each_task_and_its_last_step_and_the_words_stay_here`
and `tasks_now_a_task_told_before_a_restart_stays_told_and_leaves_thirty_minutes_after`
(`crates/tasks/tests/runtime.rs`). Not tried on a lamp.

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
  transport's 1,024 bytes (the task's words cut to 100 characters first,
  then the progress line, then the rest of the task's words): who (the
  agent's name), the task (300 characters), the
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
(`crates/voice/tests/it/progress_news.rs`), which also shows that only `on`
turns it on.

**Shown on the development Mac**, in `crates/voice/tests/it/progress_news.rs`
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
