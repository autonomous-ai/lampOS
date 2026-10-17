# The lamp's memory

`lamp-memory` (`crates/memory`) keeps what the lamp remembers from one day to
the next: a journal of every exchange it took part in (what it was asked, as
the provider wrote it down, what it answered, and when) and the things it was
asked to keep ("remember that I take my coffee black"). It is decision 12 of
[brain-build.md](brain-build.md). Memory is one person's: what nobody known
says is under the person id `everyone` ("said in this room"), what someone
the eyes know says is under theirs (since #764, "Each person's own" below),
and no request gives one person's memory in answer to another's.

**What exists and what we take.** Keep the raw journal as evidence, use
SQLite FTS5 for ranked keyword retrieval, and let a separate model propose
small observations with explicit source links. The studies behind that
choice are recorded below. The implementation and public retrieval
comparison are in [memory-days.md](memory-days.md) and
[memory-evaluation.md](memory-evaluation.md).

Health reporting reuses the existing `Diagnostic` message and its one-second
cadence, following Actions and ROS diagnostics. It keeps V1 telemetry's
separation of delivery failures from component failures, with one worker
instead of synchronous publication in the request loop. The source review is
in [reuse-decisions.md](reuse-decisions.md#memory-health-task-28-2026-10-14).

**Two modes.** The JSONL mode described first below remains the default.
`--memory-days` enables the new store and request extensions. Its current
host results and First's lamp runs are in [memory-days.md](memory-days.md).
The optional health report below changes no Voice caller or deployment,
and configures no model.

Task 29 adds an optional Voice client for ranked results, frozen notes with
provenance, linked journalling and a background conversation-end receipt.
It requires `LAMP_MEMORY_DAYS=on` in the provider worker as well as the
existing Memory socket. The coordinator forwards only that exact switch
value to the provider role; the complete gate remains pending. Its Mac client
timings are in [the task 29 evidence](verification/memory-voice-host-20261014/README.md).
See [the client contract and played scenes](memory-days.md#opt-in-voice-client-task-29).

## The process

Its own process, so that the lamp talks on, without recalling earlier days,
when this one has stopped, hung or crashed. Others ask over a Unix stream
socket and never read its files.

    lamp-memory --dir ABSOLUTE_DIR --socket ABSOLUTE_SOCKET_PATH [--keep-days N] [--utc-offset-minutes M | --zone system|NAME] [--memory-days] [--bus-root ABSOLUTE_DIR]

`--dir` is made with mode 0700 if missing, `--socket` with mode 0600, and a
socket file nobody answers on is replaced. A relative path, an unknown option
or a `--dir` its group or everyone can enter exits with 2 and says why; an
answered socket or a `--dir` another `lamp-memory` has open exits with 1. Once
the store is open a file `ready`, holding `ready` and a newline, is kept
beside the socket.

One connection is served at a time. A client has two seconds to send a whole
request line and two to take its answer, or it is dropped; a silent open
connection makes the next client wait, so close when you have your answer.
Each request is logged in one line (op, person, outcome, counts), never with a
word of what was said or kept. The process registers no signal handler:
SIGTERM ends it where it stands. A successful write is synced before its
request is answered, but the socket and `ready` stay until the next start
clears them or systemd removes the runtime directory.

### Shared application startup

The binary includes one private application body from `src/application.rs`.
Its entrypoint accepts a reporter factory; ordinary `lamp-memory` supplies
`Reporter::start`. The factory is called only for an explicit `--bus-root`,
after the actual socket bind and before opening the store, with the same
bus root and Legacy/Days source as before. With reporting absent, or if
socket ownership fails, it is never called.

The shared body keeps log-relay installation before argument parsing,
private-directory validation, readiness, request serving, error handling
and bounded reporter shutdown in their original order. This internal seam
lets a later explicit fixture reuse startup without duplicating Memory's
application logic; this step adds no fixture executable or clock mode.

## Optional health reports

`--bus-root ABSOLUTE_DIR` enables one `lamp_msgs/Diagnostic` on `diagnostics`
each second, named `memory`. Without this flag, even `LAMP_BUS_ROOT` does
nothing: there is no reporting thread, counter initialization or bus I/O,
and the journal says so once at start, after the socket is taken and no
reporter was made: `lamp-memory: health reports off: no --bus-root`.
Both store modes support the flag. Add `--expect memory` to Health's existing
command when enabling it; [health.md](health.md#memory-reports) explains what
the aggregate can show.

The report is `ok` when serving without an observed fault. It is `warn` after
a typed storage failure, a count-read failure, an answer over its budget, or
a server/store-open failure after taking the socket. Invalid requests, empty
results, a full kept list, an unconfigured model and clients disconnecting
are not storage failures. A storage warning persists until a successful
`append`, `append_linked` or `remember`; a read or a no-op deletion cannot
prove writing works. A count warning clears on a successful count read.
Messages contain fixed descriptions only, with no paths, people or memory
text. Internal Days corruption currently classified as `bad_request` is not
automatically promoted to a storage warning.

| Value | Meaning |
|---|---|
| `exchanges`, `kept` | Retained records across all people; observations, jobs and session snapshots are excluded. `unknown` means no trustworthy count is available |
| `counts_age_ms` | Age of the accepted count snapshot, or `unknown`. A busy legacy store can retain an older count |
| `answered` | Complete request lines whose handler returned, including refusals; excludes oversized lines rejected by the framing layer |
| `recent_answers`, `recent_seconds` | Answers completed in the current monotonic second and the previous 59 seconds; a fixed 60-bucket window |
| `slowest_answer_ms`, `slowest_note_ms`, `slowest_call_ms` | Largest completed handler duration in that window, with three decimal places; `none` if that class has no observation |
| `active_answer_ms` | Elapsed duration of a request still being handled, or `none` |
| `note_budget_ms`, `call_budget_ms` | 300 and 1500. Equality is allowed; greater than the budget warns |
| `timing` | The boundary described below, also carried in the report |

The note class is `kept` or `session_open`; other complete lines use the
1.5-second service budget. Voice's explicit calls use 1.5 seconds, but its
background append writer has a different deadline: this service rule does
not claim every caller waits 1.5 seconds. Timing starts after a complete
request line arrives, before classifying/parsing it, and ends when its
serialized answer exists. It includes store maintenance and durable writes.
It excludes connection queueing, request transfer, the health counter read,
logging and response delivery. It therefore does not prove Voice's total
deadline was met. An active handler over budget also warns, even if it never
finishes. Completed slow-answer warnings age out with their bucket.

One worker owns publisher construction, retries, publication and legacy
count scans. The socket owner writes fixed atomic state; it never locks or
waits for that worker. A legacy count is accepted only when no request
overlapped its scan. The scan streams one line at a time, at most 64 KiB;
larger lines make counts unknown and warn, without changing the store's
existing reader or files. This assumes the store's single owner; external
file edits are not fenced. Scanning a large store can delay reports and
compete for disk bandwidth, but never creates another worker or queues
requests for it.

Days enables a TEMP count table and TEMP triggers on its owning connection,
initialized once before readiness. They follow the same transactions,
rollbacks and deletions as the records; reading the two totals after an
answer takes constant work. No second database reader holds a lock against
the writer. No durable schema changes, second database or persistent health
records are created. Reopening without `--bus-root` creates none of these
counters.
Both counting methods assume the service is the only writer; editing its
files or database from another process bypasses that ownership contract.

Nothing is reported before the ready file is written. A store-open failure
after socket ownership gets one best-effort final warning. Invalid arguments,
private-directory validation and refusal of an already owned socket still
exit before reporting; this avoids a rejected second process speaking for
the real owner. A missing/unusable bus does not stop Memory. The worker
retries once per reporting cycle and logs delivery failure/recovery separately;
Health detects missing or stale reports. There is no catch-up queue.
Shutdown waits at most 100 ms for the worker. Filesystem calls cannot be
cancelled, so a held worker can outlive that wait until its call returns or
the process exits; it is never replaced. SIGTERM behavior is unchanged.
An already blocked publication can finish with its earlier snapshot; a final
warning is not guaranteed, and Health's missing/stale detection remains
necessary.

### Reporter scheduling in local fixtures

`Reporter::start_with_schedule` accepts an explicit `ReportSchedule` with a
pure, nondecreasing `Instant` source and an interruptible wait. It uses that
same source for the publisher's cadence, installed before the first presence
write. Ordinary `Reporter::start` keeps the host clock and `park_timeout`;
both paths keep the one-second period, original clock sample positions and
100 ms shutdown wait. State's request latency and count-age measurements
remain on their real clock. No process-wide clock or CLI mode is installed.

The schedule's wait must use the worker's thread park token so the existing
shutdown `unpark` wakes it, including a wake before parking. Observations must
remain local and nonblocking. `CycleComplete` follows the real count scan,
snapshot, optional publication and failure/recovery log decision; its
publication outcome records only whether that call returned success or
failure, or was not attempted. `WaitChecked` reports a positive remaining
wait after the deadline check. It does not prove that the thread has parked.
Neither observation acknowledges Event delivery, a flushed downstream log,
publisher cleanup or worker exit. A controller must account for those real
participants separately before advancing time or claiming evidence.

## On the disk

This layout is the default. Enabling `--memory-days` imports it into one
private SQLite database, then removes the imported files before serving.
The conversion is one way: later starts on that directory require the flag.
Read the [migration and durability contract](memory-days.md#storage-and-migration)
before using an existing store.

    <dir>/<person>/journal-YYYY-MM-DD.jsonl   {"id":1,"at_unix_ms":1791892800000,"heard":"...","said":"..."}
    <dir>/<person>/kept.jsonl                 {"id":2,"at_unix_ms":1791892800000,"text":"..."}

Directories are 0700, files 0600. A person id is `[a-z0-9_-]{1,32}`; anything
else is refused before it is near a path. The day is the local day of
`at_unix_ms`, `--utc-offset-minutes` east of UTC (default 0; 420 for UTC+7),
the same all year. `--zone system` (or `--zone America/New_York`) takes the
zone's offset at each moment instead, so a day under summer time turns at
local midnight too (#820). `system` follows `/etc/localtime`, which setup
points at the home's zone, and takes up a new zone at the next request, with
no restart; a change that cannot be read keeps the last zone. The two flags
together, or an unknown zone, end the start; never a silent UTC. The start line
says `zone=` in place of `utc_offset_minutes=`.
Ids rise for each person across both files, and after a restart go on from the
highest on the disk. A line is added with one `write` and `sync_data`; a
removal writes the file anew beside the old one and renames it into place. A
line that does not read as an entry, such as one a power cut stopped in, is
skipped and counted (`skipped_lines`), and the rest of the file is used.

## Requests

One JSON object on a line, answered by one on a line: `"ok":true` and the
result, or `"ok":false`, an `"error"` (`bad_json`, `bad_request`, `unknown_op`,
`too_long`, `bad_person`, `bad_day`, `bad_time`, `empty`, `storage`) and a
`"message"`. A refusal never ends the connection. Each takes an optional
`"person"`, `everyone` if left out.

    {"op":"append","at_unix_ms":1791892800000,"heard":"How do I take my coffee?","said":"Black."}
    {"op":"remember","person":"mai","at_unix_ms":1791892800000,"text":"takes her coffee black"}
    {"op":"kept","max_bytes":2000}
    {"op":"recall","query":"coffee","day":null,"limit":8,"max_bytes":4096}
    {"op":"forget_last","kind":"kept"}
    {"op":"forget_all","person":null}
    {"op":"status"}

- `append`, `remember`: `{"ok":true,"id":3}`. `heard` or `said` may be left out.
- `kept`: `entries` of `{id, at_unix_ms, text}`, the newest that fit in
  `max_bytes` of text, oldest first.
- `recall`: `entries` of `{"kind":"exchange", id, at_unix_ms, heard, said}` or
  `{"kind":"kept", id, at_unix_ms, text}`, oldest first. With `day`
  (`YYYY-MM-DD`): that local day's exchanges. With `query`: what in the kept
  list and the journal has any of its words as a whole word, whatever the
  case; more of the words rank first, then the newer. With both: that day's
  exchanges that match. With neither: the newest exchanges.
- `forget_last` removes the entry with the person's highest id, of either kind:
  `{"ok":true,"removed":{"kind":"kept","id":2,"at_unix_ms":1791892800000}}` or
  `"removed":null`. Which entry, never its text. With `"kind"` (`kept` or
  `exchange`) it is the highest of that kind, whatever came after it:
  "forget what I asked you to remember" means the last kept thing. Any
  other `kind` is `bad_request`.
- `forget_all` empties one person's directory, unreadable lines included:
  `{"ok":true,"removed":14}`. With `"person":null`, everyone's; every other
  request but `status` refuses `null`.
- `status`: `people`, each `{person, exchanges, kept, bytes, oldest_day,
  newest_day, skipped_lines}`, everyone's unless one is named.

### Additional requests with `--memory-days`

The old request and response shapes remain accepted. `recall` now selects
by FTS5 rank and still returns the selected raw entries in chronological
order; it does not return observations. New operations are:

```json
{"op":"search","person":"everyone","query":"cà phê","limit":8,"max_bytes":4096,"history":false}
{"op":"append_linked","at_unix_ms":1791892800000,"heard":"My coffee?","said":"Black.","source_ids":[2]}
{"op":"append_linked","person":"p-be9688998f085098","at_unix_ms":1791892800000,"heard":"Who am I?","said":"Minh.","source_ids":[],"rule_ids":[1]}
{"op":"forget","id":2}
{"op":"session_open","session":"voice_1","max_bytes":1200}
{"op":"session_close","session":"voice_1"}
{"op":"conversation_end","conversation":"turn_1","source_ids":[1],"previous_ids":[]}
{"op":"conversation_end","conversation":"voice_1","source_ids":[1],"submit":true}
{"op":"tidy_apply","job":"j1","observations":[{"text":"Prefers black coffee","category":"preference","source_ids":[1],"supersedes":null}]}
```

These are shapes, not an ordered script: IDs must come from successful
responses in the same person's store. `search` returns ranked records with
their provenance; `forget` returns a count after removal and index checks.
`session_open` returns a frozen `snapshot`; default `conversation_end`
returns bounded model `input`, without invoking a model. With `submit:true`,
the current build validates references and returns
`{ok:true,submitted:false,reason:"model_unconfigured"}`, allocating no job.
`tidy_apply` returns the new observation `ids`.
See [the full wire contract](memory-days.md#request-contract)
for fields, errors, retries, invalidation and caller responsibilities.

## Limits, and what happens at each

- A request line over 64 KiB: `too_long`; the next line is read as usual.
- `heard` 600 bytes, `said` 800, `text` 300: cut at a character, never refused.
  Whitespace and control characters become single spaces first; an entry with
  nothing left is refused (`empty`).
- 200 kept things for a person: the oldest is dropped as the next arrives.
  With `--memory-days`, the next write is refused as `full` instead;
  explicit memories are never silently evicted.
- `--keep-days N` (default 30): a journal day is deleted once it is more than
  N days before today by the lamp's clock, at start and at the first request
  of a new local day. Kept things have no age. A clock set far ahead empties
  the journal.
- `recall`: `limit` 8 unless given, at most 50; `max_bytes` 4096 unless given,
  at most 16384; more is lowered, not refused. Bytes are of text; the first
  entry that does not fit whole ends the answer. A query's words are those of
  two characters or more, the first 16. `kept`: 4096 bytes unless given.
- `at_unix_ms` outside the years 1970 to 9999: `bad_time`.

## How the voice uses it

The first five minutes ([onboarding.md](onboarding.md)) has the model ask a
new person their name and their preferences, and keep them with these
functions.

`crates/voice/src/remember.rs`, switched on by naming the socket in the
voice unit (`LAMP_MEMORY_SOCKET`, the provider worker's `--memory-socket`).
Without it the voice knows nothing of this process. The following describes
the unchanged legacy client, used unless the new Days client is selected.

- **Three functions the model may call** ([tools.md](tools.md)):
  `remember {text}`, `recall {about?, days_ago?}` and `forget {}`. Each is
  one request here, on a thread of its own, with a 1.5 s budget. With
  no answer the model is told "My memory is not answering right now, so I
  could not do that." and the conversation goes on.
- **The journal.** When a request is over, the exchange (what the provider
  wrote down of the person, and of the lamp) is queued for one writer
  thread, which sends `append`. The queue holds 32; a full queue or a
  memory that does not answer loses that exchange and nothing else. An
  answer with no word in it is not journalled. Talk the lamp did not
  answer never reaches here.
- **Asking it to keep or forget something is not itself kept.** The
  exchange in which `remember` or `forget` was called stays out of the
  journal: the kept list is the record of that.
- **A note for each new session.** Every session is told that it has a
  memory and when to consult it ("when someone asks about an earlier
  conversation, or about something they told you before that you do not
  see here, call recall ... If it gives nothing, say you do not
  remember"), and then the newest kept things (1,200 bytes of text), with
  the rule to use one only when the person brings its subject up, never to
  list them, and never to say "you told me": the lamp cannot tell who is
  speaking. The kept things are read once when the voice starts, waiting
  300 ms at most, and after that by the writer thread: after each
  `remember` and `forget`, and every five minutes. Opening a session
  never waits for this process, unless someone known is at the lamp (below).
- **What the model is given from a recall** is how long ago in words
  ("yesterday", "2 weeks ago"), never a date or a time of day: a person
  does not say "on 3 October at 14:02".
- **`forget` is not yet all of forgetting.** It removes the last kept
  thing, clears the note and the conversation thread, and that is what the
  lamp says ("it is off my list"). A later conversation in which the thing
  came up is still in the journal until it is 30 days old. Forgetting that
  reaches everything derived from a thing needs each entry to say what it
  came from. The opt-in store supports those links and transitive deletion;
  the legacy client does not send them or use frozen snapshots.

### Each person's own (#764)

When the voice has eyes that remember faces (`LAMP_EYES_WHO_SOCKET`, the
worker's `--who-socket`), it reads who of those people is at the lamp from
the eyes' known file beside that socket, counted only while it is under
3 s old ([eyes.md](eyes.md#who-is-there)). Both clients, legacy and Days,
then use that person's scope:

- **The person id** is `p-` and the first 16 hex digits of SHA-256 of the
  lower-cased name (`remember::person_id`): "Minh" is `p-be9688998f085098`.
  It fits `[a-z0-9_-]{1,32}`, so no name is ever in a path or a log line.
  The name itself stays between the eyes and the voice.
- **`remember`** and **`forget`** go to their scope: `forget` removes the
  last thing *they* asked to keep, and nothing of the room's.
- **`recall`** (and Days' `search`) asks their scope first and then the
  room's, one after the other inside the same 1.5 s. The room is asked for
  the entries and bytes of text their answer left of the usual six and
  2,400, and not at all when theirs filled them, so an answer is no longer
  than before. Their entries come first and carry `"theirs":true`. Either
  part not answering is a memory that did not answer.
- **Each exchange** goes to the journal of whoever was known at the lamp when
  the request was over, read then, not when the writer sends it. With Days,
  an exchange links only that person's sources, because the store numbers
  and links records within one person; a different person at the lamp ends
  the conversation being collected, which is sent to `conversation_end` in
  its own scope.
- **The session note**, for a session opened while someone is known: the
  room's note, then what they asked to keep. Legacy reads theirs then, in
  300 ms at most; Days opens a second frozen snapshot in their scope within
  the same 300 ms, and the session is told the first is the room's and the
  second theirs. Theirs missing leaves the room's note as it is.
- **What the model is told.** The functions' descriptions are fixed at
  session setup, while people come and go within a session, so they still
  say "You cannot tell who is speaking". The note told when someone known
  is recognised ("The person at the lamp now is Minh: you know their face.")
  now goes on, when the lamp has a memory: what they ask to keep and say is
  theirs, and what recall marks "theirs" they told it themselves, so with
  them it may say "you told me" about that and never about the rest.
- **"Forget my face"** forgets the face only: what they said stays theirs.
  Forgetting everything (`forget_all`) is as before.
- **With nobody known**, or without eyes, every line to the memory is
  exactly what it was: no `person` field, the same limits, the same note
  (`remember::tests::with_nobody_known_every_line_to_the_memory_is_as_before`
  pins the bytes).

### A session for each person (#764 follow-up)

Scopes alone did not reach what the open provider session had already
heard: on lamp-52e6 with a real face, after the known person had gone,
"what is my locker code?" was answered "It's 417" without a recall, from the
session in which they had kept it four minutes earlier
([verification](verification/memory-per-person-real-face-lamp-52e6-20261015/README.md)).
So, with the same switch (a memory and `LAMP_EYES_WHO_SOCKET`), each
provider session is one person's:

- **Who is at the lamp** is read from the known file every 500 ms
  (`known_faces::Presence`), and once before the first session opens, so
  that session is theirs and the count starts from that reading (#1388:
  opened for the room, it was renewed the moment it was ready, 4 times on
  lamp-52e6 on 15 October). Someone known counts at once when nobody was.
  A different known name counts only once the file has given it for 10 s
  with the person at the lamp unseen (`SOMEONE_ELSE_AFTER`): on lamp-4ace
  on 16 October from 16:06 the eyes flipped the owner between two names
  kept for one face, each flip opened a new session, and the lamp was deaf
  to them. Nobody counts only after 30 s without a fresh file
  (`GONE_AFTER`): the eyes renew it within 3 s while they see the face, so
  looking away for a few seconds is not a change. And a different known
  name counts at once when the one counted was last seen 30 s ago or more,
  read between or not.
- **Each exchange is the person's the eyes name as it is made**
  (`Thread::seen_now`), as the memory files it; the counted person's only
  when no name is fresh. The counted person decides only whose session it
  is. So for up to those 10 s a newcomer in a known person's place is
  answered in that person's session, which holds their thread and kept
  things, but what the newcomer says is theirs, whether or not they come
  to count: it is in their own recall and never told to the one before
  (the reviewer on #1100;
  `thread::tests::what_a_newcomer_says_is_theirs_whether_or_not_they_count`).
  The cost: a face kept under two names splits its thread between them, as
  its memory already is.
- **A face kept under two names still restarts the session**, far less
  often, not never. In the reviewer's probe on #1100, with the eyes' one
  recognition per 2 s naming either record at random: 14 restarts an hour
  when they match equally (23 at worst), against about 900 before; 2 an
  hour at 70/30. The full fix is one record per face (#1057). The session
  is told the name of the one counted, so such a face is told one name.
- **The second route** reads the file only when a turn opens, so "unseen"
  there means not named at the last few turns' openings, and someone
  else known counts at once when the one counted was last named 30 s ago
  or more (`second_provider::worker::tests::a_known_person_long_after_another_has_their_own_turns_at_once`).
  With a face that matches its two records equally and a turn every 5 s,
  it changes thread about 50 times an hour. It opens no session, so the
  lamp is not deaf, but a turn can be given the other record's thread.
- **Nobody ends a person's session only when it has gone quiet**: after the
  30 s unseen, and also two minutes with no exchange in the thread
  (`QUIET_BEFORE_ROOM`) and no turn open. Recognition needs the face, and
  lamp-4ace stands sideways to its desk: a known person who keeps talking
  while turned to a monitor, a book or someone else would otherwise be
  given a room session told the room's thread, and lose what they were
  just talking about. Until then the session stays theirs and what is said
  is kept as theirs. So someone new who speaks within two minutes of a
  known person leaving, without being recognised, is still in that
  person's session, as before #775; that is the narrow case left.
- **When that is not whom the session was opened for**, the worker asks for
  a new session (`renew_when_idle`), only between turns: no request open
  and nothing of an answer still to be handed on; a change during a turn
  waits for its end, and the session itself still waits until nothing is
  owed on it, so an answer is never cut off. The log says "the person at
  the lamp has changed", never who.
- **The new session** is told only that person's kept things (the room's,
  then theirs) and their thread, or the room's when nobody is known
  (`remember::told_for_session` for the person the thread says the session
  is for). No resumption handle is presented after the first change, since
  one could restore the earlier person's conversation.
- **Each person's thread** is kept apart ([conversation-thread.md](conversation-thread.md#each-persons-own-thread)),
  so someone who comes back within ten minutes is told theirs again.
- **With nobody ever known** nothing of this acts: the thread is the room's,
  no session is renewed, and the session note is what it was
  (`thread::tests::with_nobody_ever_known_nothing_changes`).

Cost: one session open per change, as for any renewal. Building its
instruction takes about 5 µs on the development Mac (release build, four
people's full threads, 8,000-byte character); the open itself is a network
round trip not measured on the host. During it what is said is not heard,
which is why it happens only between turns. Not shown: a lamp run.

Not shown: anything on a lamp. The host test
`crates/memory/tests/voice_people.rs` plays two known people and the room
against the real store in both modes: each is answered from their own and
the room's, never from the other's, their exchanges are journalled and
found in their own scope only, and one forgetting leaves the other's.
Known limits: who is at the lamp is the eyes' word, not proven (the store
calls a person's snapshot "caller-supplied person; identity not verified");
someone the eyes do not see, beside someone known, is journalled as them;
and with Days a room fact used in a known person's exchange cannot be
linked from it, so forgetting that room fact does not reach that exchange.

The [Days client](memory-days.md#opt-in-voice-client-task-29) uses `search`
for keyword recalls and `session_open` for a fresh session's bounded note.
It preserves quoted provenance, sends linked journal writes from one
background worker, and requests tidy-up after 45 seconds without a finished
exchange. No model is configured, so that request is a skip with no pending
job. Its 300 ms note and 1.5 s call budgets cover nonblocking socket I/O and
reply validation; they do not measure provider connection or audible
response time. Source overflow, uncertain mutations and confirmed forgetting
inhibit new journal writes until a fresh worker; a restart also discards the
old thread-file recap because it carries no provenance. These new paths
have no task 29 lamp measurements.

### What a person teaches and corrects (#813)

Off unless set: the provider worker needs `LAMP_MEMORY_RULES=on` beside
`LAMP_MEMORY_DAYS=on` (the coordinator passes both exact values through
its cleared worker environment, to the provider only). With the legacy
memory, or without the switch, the model is told of no new function, the
memory is asked nothing new and every session note is the same bytes
(`remember::tests::without_the_rules_switch_nothing_changes`).

With it, a correction or a standing preference about how the lamp behaves
with a person ("don't call me sir", "you don't need to tell me the weather
every morning", "shorter answers") is a **rule**: the model calls
`keep_rule` ([tools.md](tools.md#the-memorys)) with one short plain sentence
of at most 200 bytes, never the person's whole words. Tasks, reminders and
facts stay `remember`.

- **Where it is kept.** A rule is a `kept` entry of its own scope beside
  the person's memory: `r-<hex>` for the known person `p-<hex>`, and
  `r-everyone` for the room when nobody is known (`rules::scope`). The
  memory needed no new kind or schema: a rule is never in a person's own
  scope, so a recall or a search never returns one, and a person's rules
  are reached only through their own `r-` scope. A rule about speaking
  first is marked in its text (`[quiet] ` or `[quiet_mornings] `), and one
  about the greeting or the news said unprompted by a typed mark after it,
  `[unprompted <greeting|news|both> never not_before=<minute>
  not_from=<minute> not_while_talking] ` with only the limits asked; the
  marks are taken off before anything is told to the model.
- **Typed when kept (#813).** `keep_rule` takes, beside `rule`, the
  optional `about` (`greeting`, `news` or `both`) with `never`,
  `not_before` / `not_from` (`HH:MM`, 24 hours, local) and
  `not_while_talking`, filled by the model. A limit without `about`, a time
  that is not `HH:MM`, or `not_before` equal to `not_from` is refused with
  words the model can say, so nothing is kept half-typed. A rule without
  a limit stays text only: told to sessions, enforced by nothing. A word in
  a mark this voice does not know is left out, not misread.
- **Bounded when kept, never cut when told.** A scope keeps at most six
  rules (`rules::MOST_RULES`, so at most 1,200 bytes of rule text). A
  seventh is refused, with words for the model to say: ask which rule to
  drop. No older rule is ever dropped to make room, and every read asks
  for the whole scope, so the note, `list_rules` and the policy's fact all
  see every rule.
- **Used.** Each new session is told, after its memory note, every rule of
  the room's and then of the known person's, as "How you were asked to
  behave", within the note's 300 ms. Another person's rules are never read
  for it. The rules `forget_rule` acts on are numbered; the model is told
  never to say the numbers.
- **Undone, by number only.** `forget_rule {number}` sends `forget` with
  that id in the person's own `r-` scope, so nobody can take back
  another's rule. The number is required: on lamp-52e6 a default to the
  last rule kept took back the wrong one ([verification](verification/rules-lamp-52e6-20261015/README.md)).
  No number, one that is not a positive integer, or one that is not theirs
  removes nothing and tells the model to call `list_rules` and ask which.
  `list_rules` gives each rule with its number. A forgotten rule is erased
  by the store and never told again, and the conversation recap is
  cleared as for `forget`.
- **What is said under a rule is journalled, linked to it.** Journalling
  stays on whatever rules exist. Each exchange's `append_linked` also
  carries `rule_ids`: the rules of the person's own rules scope that the
  provider's context may hold (those the session was told, kept or listed
  in it). The store accepts only kept entries of that one scope beside the
  person's (`Person::rules`), so another person's rules are never linked.
  Forgetting a rule removes every exchange linked to it, as forgetting a
  kept thing removes what was linked to that; the links are per session,
  so they may remove more than the one answer that used it. As after
  `forget`, the conversation recap is cleared and the worker journals
  nothing more until a fresh one, since the provider's context still holds
  the rule. Limit: a room rule told in a known person's session is in the
  room's scope, so it is not linked from their exchanges, as for room facts.
- **The policy's typed fact.** At each session open, and after each rule
  kept or forgotten, the voice publishes `lamp_msgs::Rules` on the bus's
  kept topic `memory/rules` ([msgs.md](msgs.md)): the person id, `quiet`
  (asked never to speak first), `quiet_mornings` (asked not to speak first
  in the mornings, or never), the count of rules, and `hello` and `news`
  (`lamp_msgs::Unprompted`: `never`, `not_before_minute`,
  `not_from_minute`, `not_while_talking`), every typed rule of the room's
  and theirs merged, the strictest winning: the latest `not_before`, the
  earliest `not_from`. `quiet` and `quiet_mornings` are not copied into
  them; they stand for both kinds. Never a rule's words.
  It is said from a thread of its own, newest fact only, so a bus that
  hangs never holds up a session's note or a call. The policy reads it
  with `latest`, and obeys it with `lamp-policy --rules`: progress news
  with `--progress`, the hello on arrival with `--hello`, and a skill's
  asks with `--gate` ([policy.md](policy.md#a-persons-rules-about-speaking-first-813)).
- The exchange in which a rule was kept, forgotten or listed stays out of
  the journal, like `remember` and `forget`.

Host test: `crates/memory/tests/voice_people.rs`
(`days_a_rule_is_told_in_the_next_session_to_that_person_only_until_it_is_forgotten`)
keeps Minh's rule through the real Days store, tells it in Minh's next
session and never in Sam's or the room's, lists it, does not return it
from a recall, refuses Sam's attempt to forget it and forgets it for Minh.
Beside it: two rules, the first taken back by its number and the second
left, with no number, 0, -1, a word and a number not theirs removing
nothing; six rules kept and all told and listed, a seventh refused, also
with eight keep_rule calls at once; an answer under a rule journalled with
its rule_ids and recalled the next day, gone when the rule is forgotten
while an answer from before the rule and another person's stay, nothing
journalled after the forget in that worker, and a fresh worker journalling
again; and the store refusing another person's rule IDs.
Known limits: whether the model calls `keep_rule` for a spoken correction
is not shown on the host; `memory/rules` holds the person of the last
session opened or rule changed, not whoever is at the lamp now; a `forget_all`
for one person must also name their `r-` scope (no caller sends one today).

### Played on the lamp

On lamp-4ace on 2026-10-13, late in the evening with nobody in the office,
a synthetic voice through a loudspeaker, once each
(`lamp-memory.service`, its own unit and user; the voice at `7da2571cf`):

| Said to the lamp | What it did | First sound after the end of speech was decided |
|---|---|---|
| "Hey Lamp, please remember that I take my coffee black." | Called `remember` (the memory answered in 10 ms) and said "Got it. Black coffee." | 1.59 s |
| "Lamp, how do I take my coffee?", in the same conversation | Called `recall` and said "You take it black." | 1.95 s |
| The same question to a new session that had never heard it said, the conversation thread removed | "Black, just the way I like it too.", from the note, with no call | 1.32 s |
| "Lamp, what did we talk about earlier today?" | Called `recall` (six entries) and said "We chatted about a math question, and you also asked about how you take your coffee." | 2.09 s |
| "Hey Lamp, forget what I asked you to remember." | Called `forget` (6 ms) and said "It's off my list." | 2.25 s |
| The coffee question to a new session after that | Nothing: the service heard a statement ("I don't want to take my coffee.") and did not answer | |

On the lamp the memory answered `append` in about 10 ms and `recall`,
`kept` in under 1 ms, with a journal of a few dozen lines. A reply that
uses a function begins roughly a second later than one that does not.

The run before it, on the first build (`bfa8f57a9`), is why two things
are as they are. A new session did not use the kept thing: the voice had
opened its first session a moment before the writer thread had read what
was kept, so what is kept is now read before the first session, and a
question about the coffee to that session was declined as not for the
lamp. And "what did we talk about earlier today?" got "I don't remember
earlier conversations. We're just starting fresh!" with no call, which is
why every session is now told that it has a memory.

Not shown by this: whether a thing that was forgotten still comes back
from the journal (the last row was not answered at all), anything with a
person and not a loudspeaker, a second language, more than one thing
kept, a journal of more than a day, or what happens to the lamp's
ordinary answers with three more functions declared.

## What is taken from how others do memory

The owner's question on 2026-10-13: is rolling our own right, or should
the lamp use what the field has worked out? Three studies were made that
evening and are kept outside the repository, each claim with its source:
how agent products do memory (Claude Code, Codex, ChatGPT and others),
the open-source memory systems (mem0, Letta, Zep and Graphiti and a dozen
more) with their benchmarks and the disputes over them, and what a spoken
companion in a shared room needs that a chat agent does not. Their
findings are the studies', read from the sources named there and not
re-measured here.

The answer they support: **adopt none of them whole, and build on the
journal.** No open-source system fits as it is: each is a Python service
with a vector or graph store and a model call on every write, and in
independent tests the well-known ones do not beat plain keyword search
over the raw record. The products people use every day converged on
something simpler: readable text, a small part always in the prompt under
a hard cap, the rest searched on demand, and a pass in the background
that tidies up. The journal here is already the layer the evidence
supports most. **Except where the last column says so, this is a decision
record and not what the crate does.**

| What the best of them do, and the evidence | What the lamp's memory does about it | In the crate |
|---|---|---|
| Keep the raw record and search it; treat extracted facts as an index over it, never a replacement (the best-supported finding: replacing sessions with summaries lowered accuracy in LongMemEval) | The journal is the truth. Anything derived points back to the lines it came from and can be rebuilt | Opt-in raw records plus validated observation source links; scripted tests |
| A fast path for "remember this" | The kept list, written at once | Yes |
| A small part always in the prompt, under a hard cap, as it stood when the session opened | Open promises, recent threads, lasting preferences | Opt-in frozen snapshot, at most 1,200 UTF-8 bytes including its JSON framing; Voice client and service forwarding implemented, final host gate pending |
| Tidy up in the background, never during a reply (Codex, ChatGPT, Letta, OpenClaw) | A pass after a conversation has ended, with a model off the voice path: dated observations, each with its source lines | Bounded prepare/apply protocol and cancellable model interface; optional Voice handoff explicitly skips without an adapter and retains no job |
| Stamp time on everything; a newer fact supersedes an older one by a link, and both are kept (overwriting and model-decided deletion were wrong often enough that mem0 gave them up) | Supersession as a link. Nothing derived is destroyed by the pass that made it | Opt-in source-dated observations; correction links preserve history; stale results refused |
| Keyword search first; meaning-based search only where it is measured to help | SQLite FTS5 ranking; no embeddings without evidence | Opt-in Unicode-normalized English/Vietnamese retrieval; LongMemEval user-session recall-all@5: 69.69% whole-word, 81.86% FTS5, 419 questions on the development Mac |
| Forgetting that is real: it reaches every summary and index (regulators acted against a product that kept transcripts of "deleted" recordings) | Remove the thing and linked derivatives, and verify the index before acknowledging | Opt-in transaction covers records, source/replacement links, FTS, pending jobs and stored notes; callers must drop already supplied model context |
| Age out what goes unused (Copilot 28 days, Codex 30) | Derived facts lapse when unused; the journal goes at 30 days; what a person asked to be kept has no age | Opt-in observations lapse after 30 unused days or when their evidence expires; explicit kept facts do not age |
| Scope by person, and never tell one person what another said (rated the most damaging failure for a shared room) | Until someone asks to be recognised, everything is "said in this room": used when its subject comes up, never attributed, never volunteered | All records, jobs, links and snapshots scoped by person; default is everyone; identity remains caller-supplied. The voice uses a known person's scope since #764 |
| Treat what is heard as untrusted, and check a fact before leaning on it (the recorded errors of a deployed voice agent's memory came from mishearing) | The lamp says back what it kept; names, times and promises need confirmation | Structured untrusted snapshot and strict output/provenance validation; no semantic truth check or new voice confirmation flow |
| Reminders are not memories: their own store, and a clock outside the model | Not part of this crate: a part of its own | No |
| A page where people see what is kept and can delete it | With the operator pages | No |

Not taken: a knowledge graph (it added 1.6 points for mem0 and costs a
model call or several for each write), a vector database as the store,
scores of importance and forgetting curves set by hand (no evidence they
help anyone), and any hosted memory service (every conversation in the
office would go to one more company).

**How it will be known whether this is good.** Three numbers, reported
together: accuracy, how long a read takes on the lamp, and how much text
leaves the lamp for a third party in a day. Accuracy on LongMemEval-S
(public, five abilities scored apart, and its retrieval score needs no
model) beside three baselines on the same harness: the whole
conversation in the prompt, today's whole-word match, and ranked keyword
search. And a set of the lamp's own: scripted conversations over several
days played to it, with questions whose right answer is "I don't
remember" or "that was someone else". The public retrieval comparison and
scripted multi-day store tests now run on the development Mac; see
[memory-evaluation.md](memory-evaluation.md). Full context is a retrieval
ceiling only, not a measured answer score. No daily provider traffic or
new lamp performance has been measured.

## What it deliberately does not do

No audio, no pictures. Nothing is sent anywhere from here, though a line
leaves the lamp, to the voice provider, whenever the voice uses one in an
answer. The default has whole-word matching; the opt-in mode normalizes
Unicode and ranks with FTS5, with no embeddings or semantic answer model.
Its observation pass has only a scripted adapter here. It does not know who speaks: the person is
whoever the caller says, and the voice says whom the eyes know at the lamp. Files are not encrypted; forgotten text is unlinked,
not scrubbed from the flash. The offset from UTC is fixed: no summer time.

## Not shown

Task 29's new Voice client still awaits its complete gate; its first and
repeated Mac client timings are retained in
[the host evidence](verification/memory-voice-host-20261014/README.md).
The historical results below do not validate its new reply order, snapshot
context, linked journal or background handoff on the lamp.

Host tests on a development Mac: store and protocol in unit tests, the real
server on a socket in a thread, the built binary killed with SIGKILL and
started again. On 2026-10-13 the same 34 tests passed once as a Linux ARM64
build under emulation in a container; `forget_last` with a kind was added
after that run (36 tests on the host). The voice's side is host tests: the
three functions against a scripted memory, the journal's queue and writer,
and the provider worker with the scripted provider and a memory on a
socket. On a lamp: the played run above, once. A power cut was imitated by cutting
a file short. Moving the socket to its path after binding it has run on macOS and in
that emulated Linux run. Times, on that Mac through the socket with thirty days of 200 exchanges
(`tests/timing.rs`): `append` 19 ms, `recall` by words 15 ms, by day or newest
under 1 ms, `forget_last` 50 ms. None is known for the lamp.

The opt-in implementation's host results and current limitations are in
[memory-evaluation.md](memory-evaluation.md) and
[memory-days.md](memory-days.md#not-shown). The historical lamp and emulation
runs above do not validate SQLite, its native ARM64 build, the new protocol,
a real extraction model, or any changed voice behavior.

Task 28's health report has no lamp measurements. Its host gate, failure
fixtures and exact timing boundaries are in
[the host evidence](verification/memory-health-host-20261014/README.md) and
[Fourth's log](work-log-fourth.md). Flash contention during count scans,
long-lived bus filesystem stalls and the real Health unit remain for First.
