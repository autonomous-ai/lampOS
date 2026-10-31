# Schedule: timers and reminders that survive a restart

## What exists and what we take

`lamp-schedule` owns a private, durable list of timers and reminders. It resolves
times, retains due occurrences, and serves guarded local requests. The executable
does nothing unless started with `--enable`. Actions exposes its six tools only
with `LAMP_SCHEDULE_TOOLS=on`; Voice declares/routes them with the same exact
switch. Voice's separate `LAMP_SCHEDULE_SPEAK=on` consumer (or `skill`;
`reminders::speaks`) may speak due work under the final Voice owner. Both
additions are off unless explicitly set; the product's units set both
(`LAMP_SCHEDULE_TOOLS=on` in lamp-actions' and lamp-voice's,
`LAMP_SCHEDULE_SPEAK=skill` in lamp-voice's).

The product references were reviewed on 2026-10-14. Apple's
[HomePod guide](https://support.apple.com/en-nz/guide/homepod/apd82db5cb0d/homepod)
and Google's [timer guide](https://support.google.com/googlehome/answer/7028899?hl=en)
establish familiar named duration timers and remaining-time questions. Google's
[voice Tasks guide](https://support.google.com/googlehome/answer/16722329)
provides examples of tomorrow and weekly reminders. Amazon's
[reminder guide](https://digprjsurvey.amazon.com/csad/help/node/GVMW5E8YCNR6AC6E)
explicitly describes snoozing and retrieving missed reminders. We take those
plain user requests, separate cancellation from postponement, and keep missed
work visible. These guides do not establish how those assistants handle power
loss, DST ambiguity, or exactly-once audible delivery. Their pause/resume,
household sharing, and follow-up features are not implemented here.

The V1 review covers `system/schedule/spec.go`, its time and timezone tests, and
[GO-3.19](v1-verdicts.md). We retain strictly future initial calendar selection
and calendar recurrence. We do not copy V1's one-minute polling, five-minute
interval floor, device jitter, 30-minute catch-up cutoff, or global timezone
re-anchoring. The current service retains each missed recurrence until an
explicit receipt advances it. V1's distinction between handing off work and
someone hearing it remains essential.

[Jiff](https://docs.rs/jiff/0.2.38/jiff/) supplies civil calendars and bundled
IANA rules; its
[disambiguation API](https://docs.rs/jiff/0.2.38/jiff/tz/enum.Disambiguation.html)
supplies the explicit gap/fold policies below. `rusqlite` uses bundled SQLite;
transactions use `synchronous=FULL`, DELETE journals, and an additional
directory sync. SQLite's [atomic commit](https://www.sqlite.org/atomiccommit.html)
and [synchronization documentation](https://www.sqlite.org/pragma.html#pragma_synchronous)
explain the storage assumptions. A successful sync still depends on the
filesystem, operating system, and storage honoring it.

Implementation: [time.rs](../crates/schedule/src/time.rs),
[engine.rs](../crates/schedule/src/engine.rs),
[store.rs](../crates/schedule/src/store.rs),
[protocol.rs](../crates/schedule/src/protocol.rs),
[runtime.rs](../crates/schedule/src/runtime.rs), and
[the message contract](../crates/msgs/src/schedule.rs).

## Starting the owner

The disabled command returns an `enabled:false` JSON line and exits without
opening a store, socket, bus publisher, or worker:

```sh
lamp-schedule serve
```

An enabled Linux example is below, with example paths for a service user. The
product's unit (`deploy/product/lamp-schedule.service`) runs it as `lampos`
with `--dir /var/lib/lampos-schedule/store --socket
/run/lampos-schedule/call.sock --zone ${LAMP_ZONE} --bus-root /run/lampos-bus
--kernel-clock`, both folders made 0700 by systemd (`StateDirectory`,
`RuntimeDirectory`). All paths supplied here are absolute.

```sh
lamp-schedule serve --enable \
  --dir /home/lamp/.local/state/lampOS/schedule \
  --socket /home/lamp/.local/state/lampOS/schedule-socket/call.sock \
  --bus-root /run/user/1000/lamp-bus \
  --zone Asia/Ho_Chi_Minh
```

`--zone` takes an IANA name or `system` (#820): the zone the lamp's setup
last pointed `/etc/localtime` at, read through `lamp-local-time` as the other
parts read it, and named again at each request, so a re-setup to another zone
is followed without a restart (one line says the change). A later system zone
that names none keeps the last that did.

A zone this build's bundled time zone rules don't know (a newer tzdb's name in
`zone.env`) never stops the part (#1680). A named one falls back to the
system's zone when it names one it knows, followed as with `--zone system`.
Setup links `/etc/localtime` to the same name it writes to `zone.env`, so the
system's zone is usually unknown too, and the fallback is then the fixed
offset `zone.env` also holds, `LAMP_UTC_OFFSET_MINUTES` (read from the unit's
environment): `Etc/GMT+3` for -180, so "remind me at 7" rings at 07:00 at the
lamp. It is the offset when setup ran, so it can be an hour off across summer
time, never more. With no offset stored, or one that is not whole hours (no
`Etc/GMT` zone has it), the fallback is `Etc/UTC`, and a new reminder at a
clock time with no zone of its own ("at 7 tomorrow", daily, weekly) is
refused with `bad_time` and a reason, while one in minutes from now, or at a
time in a named zone, is set. One journal line says which, and health warns
("Schedule doesn't know the lamp's zone; a new reminder is set in the fallback
zone's time") with `zone_asked` and `zone`. A reminder already set rings at
the time it was set for, in its own zone. A stored reminder whose own zone
this build doesn't know, or whose offset newer zone rules gave, is kept unread
(below). Tests:
`an_unknown_zone_falls_back_to_the_systems_and_reminders_still_ring`,
`an_unknown_zone_falls_back_to_the_stored_utc_offset`,
`with_no_known_zone_or_offset_a_clock_time_is_refused_and_minutes_still_work`
(`crates/schedule/tests/it/engine.rs`), `an_unknown_zone_is_a_health_warning_not_a_stop`
(`tests/it/runtime.rs`).

`--dir`, `--socket`, and `--zone` are required when enabled. `--bus-root` defaults
to `/run/lampos-bus` if omitted; supplying it explicitly
makes the intended bus clear. The store and socket each need an existing parent;
the service can create their final private directory. The socket path is at
most 103 bytes. The store directory must be owned by the effective service user
and exactly mode `0700`; its regular files must be mode `0600`, singly linked,
and owned by that user. The socket parent must be private and owned by the same
user; the socket is mode `0600`. A live socket or another directory owner is
refused. This is a local service, with no HTTP endpoint or network listener.

On Linux, the default boot identity comes from
`/proc/sys/kernel/random/boot_id`. Wall time is considered trusted when
`/run/systemd/timesync/synchronized` is a file, or when the operator explicitly
passes `--trust-clock`. With `--kernel-clock` it is also trusted when the
kernel says a time client has set it (`adjtimex`, as `timedatectl`'s
`NTPSynchronized` and `lamp-health --clock kernel`): the real board keeps its
clock with chrony and has no timesyncd marker (lamp-75c9, 2026-10-16), so
without it a lamp needs `--trust-clock`, which also trusts a clock never set
(#830, [offline.md](offline.md)). This is evidence compatible with the existing Health
boundary, not proof of NTP authenticity or clock accuracy. Removing that marker
withdraws automatic trust on the next sample. The service never sets the clock.

Other development hosts require `--boot-id`. A host-only example, using fresh
test directories, is:

```sh
lamp-schedule serve --enable \
  --dir /tmp/lamp-schedule-host-state \
  --socket /tmp/lamp-schedule-host-socket/call.sock \
  --bus-root /tmp/lamp-schedule-host-bus \
  --zone Asia/Ho_Chi_Minh \
  --boot-id schedule-host-20261014 --trust-clock --run-for-ms 10000
```

An explicit boot identity must stay the same across process restarts on one
boot and change after a real reboot. It is 1–128 ASCII letters, digits, `_`,
`-`, or `.`. `--trust-clock` is the operator's assertion that wall time is usable;
it does not synchronize it. `--run-for-ms` accepts 1–86,400,000. Duplicate and
unknown options are refused. SIGINT and SIGTERM request shutdown; they do not
delete timers.

## What is committed

`<dir>/schedule.sqlite3` contains one `state` table with a singleton BLOB, under
SQLite `user_version=1`. That BLOB is the engine's JSON `schema:1`: persistent
`store_id`, revision, next item ID, active items, and the most recent mutation's
request and receipt. The store ID is 32 lowercase hexadecimal characters.
Reminder text and the most recent request are private plaintext in this store;
they are not encrypted. File permissions do not defend against another hostile
process with the same UID.

The product's reset plan lists this store (`/var/lib/lampos-schedule/store`)
and Voice's delivery folder (`/var/lib/lampOS/voice/schedule-delivery`)
(`crates/setup/fixtures/reset-plan-product.json`, embedded by install), so a
factory reset wipes them. The store contains personal reminder text, each
person's own: every item keeps who set it, and a voice line hears only its
own, and the room's short timers ("Whose reminders a line hears" below).

A new database is prepared as `.schedule.sqlite3.init`, synced, and published
without replacing an existing database. An interrupted, unpublished empty
initialization can be recovered. An existing zero-byte database, corrupt
database, unexpected SQLite schema or `user_version` 0, invalid engine snapshot, unsafe file, or
WAL/SHM sidecar is refused rather than replaced with a blank schedule. A valid
newly initialized schema with no snapshot receives its first engine state.
Checked rollback journals can be recovered by SQLite. Do not remove a database
or its journal to make a startup failure disappear; preserve them for recovery.

The JSON snapshot is read one release forward, following the Tasks boundary in
#1512. Unknown additive fields in the state, items, pending metadata, receipt
envelope, deadlines, elapsed-clock descriptors, and known daily/weekly repeat
variants are ignored. Existing identities, revisions, item and delivery IDs,
text, bounds and deadline consistency are still checked. Opening a current
snapshot does not rewrite it; ignored fields remain on disk until an ordinary
mutation writes this build's known fields. Current files and exact readable
mutation receipts continue to work unchanged.

The cached last receipt is the exception to active-state refusal. Its stored
request uses the strict live `Request`, `Guard` and `When` types. An unknown
operation/tag, extra request field, unreadable receipt, or response that does
not match this build's expected committed effect drops that cache in memory,
with one journal line saying the last mutation receipt was dropped. No request
fields are stripped to manufacture an exact match. Items and the committed
revision are retained: retrying the original mutation with its old revision
receives `conflict`, not another write. Read status and list/get to inspect the
committed work before making a new guarded request. The next mutation replaces
the discarded cache on disk; reopening before then can report its drop again.
An invalid active item still refuses the store even if its receipt is also bad.

Unknown enum/tag values in active persisted work are never guessed or mapped
to a default. A reminder or timer this build can't read at `schema` 1 (an
unknown repeat kind or weekday, a value of another type, a field twice, a
zone its time zone rules don't know, or a newer tzdb's rules for a zone they
do, whose stored offset is not the one they give) is **unread** (#1680): passed over with
one journal line naming only its id, never sounded, snoozed or acknowledged
here, kept in the store byte for byte and written back in id order at the next
save, so the build that wrote it finds it again. It counts toward the 128-item
bound, and health warns ("Schedule kept stored reminders from a newer build
that this build can't read; they will not sound") with `unread`. The part
starts and serves. An item that reads but doesn't hold together (an incoherent
`local`, id 0, empty text) still refuses the store. A new stored value still
needs a `schema` bump, pinned by `a_new_stored_value_comes_with_a_schema_bump`
(`src/engine/stored.rs`). Tests:
`a_reminder_this_build_cannot_read_is_kept_as_it_is_and_the_part_serves`,
`a_reminder_by_newer_rules_for_a_known_zone_is_kept_unread` and two beside
it (`tests/forward.rs`),
`a_stored_reminder_this_build_cannot_read_is_kept_and_the_part_serves`
(`tests/it/runtime.rs`). Live
requests, clocks, public time descriptors and delivery-ledger inputs retain
their strict deserializers. The tolerant deadline readers are private to this
engine's stored items; they do not change a shared digest or wire shape.

A numeric JSON `schema` newer than `1` is recognized before decoding its items.
The owner closes SQLite, renames the complete database without replacing any
existing file to `schedule.sqlite3.newer-<schema>-<unix seconds>` (as
lamp-tasks names its own, so a reader can tell the newest), and syncs the
directory before creating the replacement database. It holds the directory
lock throughout. It then commits an empty schema-1 state and writes one journal
line naming the newer schema and retained file. The empty state **keeps the
retained store's identity** and goes on from its revision (one past it) and its
next id, read from the newer snapshot's top level (`store_id`, `revision`,
`next_id`) where they are of this build's shape. The voice's delivery ledger
is fenced to that identity, so it follows: a reminder set after the set-aside
is said, and one in the retained store is gone to it as a cancelled one is
(the reviewer's P2 on #1562). Where the identity can't be read, the empty state
gets a fresh one, and the ledger then refuses new sound (`store_changed`), as
for any changed store. A crash after the rename and before the empty state is
committed leaves an empty store with the retained one beside it: the next
start finds the store empty, reads the newest retained one (read-only), and
carries its identity in the same way, with one journal line (the reviewer on
#1616). The new store accepts normal work; none of the retained schedule is
delivered, merged or silently deleted. A later restart uses the new store.
Keep the retained database for an appropriate newer build; this build does
not restore it. **Never put it back by hand once the lamp has taken new
work:** the ledger would meet a revision that went back (`stale_revision`)
and ids it already holds. Put it back only before any new work, or not at
all.

**Who is told about the retained reminders.** The journal says it once at
start, and a crash between the rename and that line leaves none. So health
says it too, from the file itself: while a `schedule.sqlite3.newer-*` file is
in Schedule's folder, its `schedule` diagnostic is a warning, "Schedule set
aside a store from a newer build; its reminders will not sound", with
`set_aside` naming the newest one (by its trailing seconds), as lamp-tasks'
health does for its own (#1554). The folder is listed once a second, at most,
with the diagnostic, and a folder that can't be listed says nothing rather
than stopping the part. Removing the file, once someone has looked, ends the
warning. Test: `a_store_set_aside_is_said_in_health_until_it_is_gone`
(`crates/schedule/tests/it/runtime.rs`).

An existing archive name is never clobbered. Rename, sync or replacement failure
refuses startup and poisons the owner; any already-renamed database remains
retained. A backend without durable set-aside support refuses a newer snapshot.
Invalid JSON, nonnumeric/old unsupported schema values and snapshots beyond the
existing 1 MiB bound are still refused. A SQLite `user_version` above 1 (a newer
build's container) is set aside the same way, whole, as
`schedule.sqlite3.newer-db<version>-<unix seconds>`, never deleted, with one
journal line, and the part starts with an empty store (#1680). The version is
read from the file's header before any SQLite connection, so a newer build's
WAL database, or one with `-wal`/`-shm` beside it, is set aside with its
sidecars (named after it) rather than refused by the journal-mode and sidecar
checks. The sidecars are renamed first; a start after a crash between their
renames and the database's keeps the database under their stamp, so they stay
paired. It does so only when the database's mtime is no later than their
ctime and none of its own sidecars would land on a name already there; a stale
leftover from an older crash, or a complete earlier set-aside at the same
version, gets a new stamp. Health names it
as `set_aside`, as above. `user_version` 0 with tables, or a schema it doesn't
expect at 1, is still refused (corruption, not a newer build). Tests:
`a_newer_database_version_is_set_aside_whole_and_a_new_store_begins`,
`a_newer_wal_database_is_set_aside_with_its_sidecars`,
`a_set_aside_cut_before_the_database_rename_joins_its_sidecars_again`,
`a_stale_leftover_sidecar_is_not_taken_as_the_newer_databases`,
`each_set_aside_at_the_same_version_keeps_its_own_stamp`
(`tests/it/storage.rs`), `a_newer_schedule_database_is_set_aside_and_the_part_serves`
(`tests/it/runtime.rs`). A future writer
must bump the JSON schema for new enum values, required fields, raised bounds,
or optional fields whose omission could change scheduling or delivery behavior.

There is one storage owner, protected by a lifetime advisory lock. Each change
is prepared on a copy, committed with `synchronous=FULL`, and followed by a
directory sync before a successful response or new due notice is exposed.
`fullfsync` is requested where supported. Database pages are capped at 16 MiB;
the snapshot is 1–1,048,576 bytes. The active list is capped at 128, including
scheduled, pending, and waiting-for-clock items. A full store refuses another
item; it does not evict an old or overdue one. Cancellation or completed
one-off delivery frees capacity. IDs are increasing and are not reused.

A failed write or sync can occur after the transaction reached disk. The owner
stops accepting work after such a failure. Restart and inspect the committed
state before deciding whether to retry; neither a transport timeout nor a sync
error proves that the mutation did not happen. Files changed or replaced while
owned stop further storage work. Setting a newer snapshot aside is not a
migration of its reminders. There is no automatic migration, remote
backup, factory-reset action, or expiry-based deletion in this component.

## Time rules

The owner supplies `unix_ms`, `boot_ms`, `boot_id`, and `wall_trusted` to the pure
calendar engine. The model supplies a duration or a civil-time request. It must
not calculate an epoch timestamp from a conversation's opening time.

| Request | Meaning |
|---|---|
| `{"kind":"after","seconds":480}` | Eight elapsed minutes, from the current owner sample. Durations are 1–31,536,000 whole seconds. |
| `{"kind":"at","time":"09:00"}` | The next strictly future 09:00 in the configured zone. Omitted `day` and `day:"next"` mean the same thing. |
| `{"kind":"at","time":"09:00","day":"today"}` | Today at 09:00; past or exactly current time is refused. |
| `{"kind":"at","time":"09:00","day":"tomorrow"}` | The next local calendar date, not now plus 24 hours. |
| `{"kind":"at","time":"09:00","day":"2028-02-29","zone":"Asia/Ho_Chi_Minh"}` | An exact valid calendar date and explicit IANA zone. |
| `{"kind":"daily","time":"09:00"}` | Every local date at 09:00, initially strictly after now. |
| `{"kind":"weekly","weekday":"monday","time":"09:00"}` | Every Monday at 09:00, initially strictly after now. Weekdays are full lowercase English names. |

Times are exact ASCII `HH:MM` on a 24-hour clock. `9:00`, `09:0`, AM/PM strings,
seconds, unknown fields, and invented relative-day words are refused. Dates are
exact `YYYY-MM-DD`. A zone is a name in the bundled IANA database, such as
`Asia/Ho_Chi_Minh`, `America/New_York`, or `UTC`; a numeric offset is not a zone.
Omitting the zone selects the configured zone for that new request, with
`--zone system` the system's at that moment. Changing the service's default
later, or the system's zone under `--zone system`, does not move existing
reminders: each keeps the zone it was set in.

Every new timer, reminder, and snooze requires trusted wall time, including an
elapsed timer: it needs a durable wall deadline for a later boot. On the same
boot, an elapsed timer uses its stored boot deadline even if wall time moves or
becomes untrusted. Linux uses `CLOCK_BOOTTIME`, so suspend counts toward elapsed
timers. Development hosts use `CLOCK_MONOTONIC`; suspend behavior on those hosts
is not a Linux qualification. Bus stamps separately use shared
`CLOCK_MONOTONIC` microseconds and the host boot identity, matching other lamp
topics. Do not compare a BOOTTIME deadline directly with a bus stamp.

After reboot, an elapsed timer falls back to its persisted Unix deadline. With
untrusted wall time it waits, rather than guessing how much time passed.
That fallback was calculated when the timer was created; a later wall-clock
correction during the original boot does not rewrite it. The same-boot elapsed
deadline stays correct, but the post-reboot estimate can inherit that earlier
wall-clock error. Qualifying clock trust and reboot recovery is part of the
lamp handoff.
Calendar reminders require trusted wall time even on the original boot. A
pending occurrence stays pending across clock corrections and trust loss; it
does not become future work again. A changed boot identity or backward boot
clock within a running owner is refused and requires a restart.

Jiff 0.2.38 with bundled timezone data is used directly; the calendar module
does not consult `/etc/localtime`, the environment, or a runtime clock. One-off
nonexistent local times are refused. Ambiguous one-off times default to
rejection; `fold:"earlier"` or `fold:"later"` chooses an instant explicitly.
For example, New York's `2026-11-01 01:30` needs that choice. An already-past
gap or wholly past fold can be passed when finding the next day's occurrence;
an upcoming ambiguity is not silently resolved.

Daily and weekly reminders deliberately use Jiff's `Compatible` policy:
shift forward through a gap and choose the earlier instant in a fold. The
requested date and `HH:MM` remain in `repeat`, so a shifted 02:30 occurrence
does not turn the entire series into 03:30. Advancement is from the previous
requested civil occurrence, not from delivery time. A whole-day zone jump can
map two distinct civil dates to the same instant, as in Apia in 2011; both
occurrences are retained, with different delivery tokens. Advancement is
receipt-driven, so equal instants do not create a timer loop.

Supported Unix milliseconds are 0 through **253,402,207,200,999**, inclusive:
1970-01-01T00:00:00Z through 9999-12-30T22:00:00.999Z. Jiff reserves its maximum
legal zone offset near year 9999. This runtime bound is narrower than the
message schema's end-of-year-9999 bound. Arithmetic, boot deadlines, and signed
remaining-time conversion are checked for overflow.

Persisted deadlines are checked against the bundled rules on reopen. A future
software update that changes a rule affecting a stored occurrence can therefore
fail validation. There is no implicit timezone-database migration or silent
re-anchoring; that upgrade case needs explicit recovery design and testing.

## Due, late, snoozed, and completed

At due time the owner commits a pending occurrence. It remains there until
cancelled, snoozed, or acknowledged. With Voice absent, disabled, interrupted,
or busy, nothing consumes it. The oldest pending occurrence is selected by
effective due instant, then item ID.

There is at most one pending cursor per active series. The pending count is
therefore not the total number of missed recurrence dates. One acknowledgement
advances one civil occurrence, and the next scan may make the following date
pending. No historical queue is allocated and no intervening date is skipped.
First can say the original date and let the person cancel the series, rather
than presenting this count as all the reminders missed during an outage.

A previously scheduled item found already overdue on reopening is marked late,
even if only one millisecond overdue. Ordinary polling lateness is reported in
`late_by_ms`; the `late` flag is set once it exceeds 1,000 ms. Where the current
clock cannot establish lateness, it is unknown rather than zero. First's spoken
wording should use the returned late flag and current receipt, for example
“This reminder is late: take the laundry out,” rather than inventing how long
the lamp was offline.

Snooze applies only to the current pending `(id, delivery)`. It increments the
delivery token, clears pending, and adds a new elapsed deadline. The original
recurrence anchor remains unchanged. Acknowledging the snoozed occurrence
advances from that original anchor, to its next occurrence still ahead.

**After time off, a repeat is said once** (#811, the first builder's decision
of 2026-10-16 13:21: a person who was away does not hear "late" N times).
A pending repeat whose later occurrences have also passed is shown as its
newest missed one: `due_unix_ms` and `late_by_ms` are that occurrence's, and
it is marked late, and it is ordered among the pending by that occurrence.
Its acknowledgement advances to the next occurrence still
ahead, so the ones missed in between are stepped over, not delivered. The
stored deadline is not rewritten by a scan (its receipt names it); only an
acknowledgement moves it. With an untrusted wall clock nothing is stepped.

Acknowledging a one-off removes it. Acknowledging a repeat advances to its
next occurrence still ahead and increments its delivery token. Cancellation removes
the whole item, including its recurrence. There is no separate “delete just
one recurrence,” edit, pause, resume, monthly, yearly, or interval operation.

## Private socket requests

Open a Unix stream connection, send one UTF-8 JSON object followed by a newline,
read one JSON line, and close. There is no authentication token; access comes
from the private socket's filesystem permissions. Unknown operations or fields
are rejected. Read operations need no guard:

```json
{"op":"status"}
{"op":"get","id":1}
{"op":"list","after_id":0,"limit":8,"pending_only":false}
{"op":"list_set","after_id":0,"limit":8,"pending_only":false}
```

`list_set` is `list` whose items also say when each was set (`set_local`,
`set_day`; see the model tools below).

Each line above represents a separate connection. `status` returns the current
store identity/revision, item and pending counts, waiting-for-clock count,
default zone, wall trust, capacity, and `last_receipt`. The latter is either
`null` or `{"request_id":"...","op":"set","response":{...}}`; its response
is the original committed result, and may carry an older revision than current
status. It does not include the original reminder text.

`get` returns an `item` with its `id`, current `delivery`, text, `kind`
(`timer` for a duration from now, set as `after`; `reminder` for a clock time,
daily or weekly; taken from the stored deadline, so a snoozed timer stays a
timer and no store format changed, #1595), creating `request_id`, state (`scheduled`, `pending`, or `waiting_for_clock`), effective
and original due instants, canonical local time, recurrence, snoozed flag,
remaining milliseconds, and lateness. `list` returns the same item views and
`next_after_id`. Use that cursor until it is null. A page contains at most eight
items and may contain fewer to fit the 8,192-byte reply budget. A list is not a
cross-request snapshot: concurrent changes can change revisions between pages.
Remaining time is clamped to zero once due, and is null when the required clock
comparison is unavailable.

For a manual probe, an existing `nc` client that supports Unix sockets can send
one status line. This is an operator convenience, not a runtime dependency:

```sh
printf '%s\n' '{"op":"status"}' | \
  nc -w 2 -U /home/lamp/.local/state/lampOS/schedule-socket/call.sock
```

Every mutation needs a `Guard` containing the current `store_id`, its current
`revision`, and a caller-generated `request_id`. The ID is 1–64 ASCII letters,
digits, `_`, `-`, `.`, or `:`. The Actions/Voice adapter supplies this guard; the
model never sees or invents it. The following are independent shape examples,
not a sequence to replay with the illustrated revision:

```json
{"op":"set","guard":{"store_id":"0123456789abcdef0123456789abcdef","revision":7,"request_id":"request-42"},"text":"Check the pasta","when":{"kind":"after","seconds":480}}
{"op":"set","guard":{"store_id":"0123456789abcdef0123456789abcdef","revision":7,"request_id":"request-43"},"text":"Water the plant","when":{"kind":"at","time":"09:00","day":"tomorrow"}}
{"op":"cancel","guard":{"store_id":"0123456789abcdef0123456789abcdef","revision":7,"request_id":"request-44"},"id":1}
{"op":"snooze","guard":{"store_id":"0123456789abcdef0123456789abcdef","revision":7,"request_id":"request-45"},"id":1,"delivery":2,"seconds":600}
{"op":"ack","guard":{"store_id":"0123456789abcdef0123456789abcdef","revision":7,"request_id":"playout-46"},"id":1,"delivery":2}
```

Successful mutations return `ok:true`, `store_id`, and the committed revision,
plus the item/delivery and operation result. Creation and snooze also return
the resolved `due_unix_ms` and canonical `local` value. Text must contain
1–300 UTF-8 bytes, contain no control characters, and not be whitespace only.
The byte bound is checked before surrounding whitespace is trimmed. Unicode is
preserved and oversized text is not truncated. Confirm the resolved local time
and recurrence to the person only after a successful durable response.

The most recent mutation's exact request and response survive a lost socket
reply, process restart, and automatic due scans. Repeating that same request
with the same guard and arguments returns its original receipt without another
mutation. This is one retained receipt, not an unlimited request-ID history.
After another mutation replaces it, an old compare-and-set revision conflicts.
An automatic due scan can also advance the revision between a status read and
a new mutation. Read status and the relevant items before choosing further
work; do not turn an uncertain request into a second command by silently
retrying it with a new guard. Use `status.last_receipt.request_id` and the
creating `request_id` in list/get to inspect what is known. A lost older receipt
is not reconstructed from a new request ID.

Refusals have `ok:false`, an `error` code, and a static `message`. Important
codes include `bad_request`, `bad_guard`, `conflict`, `bad_text`, `bad_time`,
`not_found`, `stale_delivery`, `full`, `clock`, and `storage`. The transport adds
`busy`, `unavailable`, `timeout`, `uncertain`, `incomplete`, and `too_long`.
`uncertain` means no receipt arrived in time and the operation may have committed.
A full connection queue can close a connection without a JSON refusal. A closed
connection is not evidence that a started mutation was undone.

## Bounds and responsiveness

| Boundary | Implemented limit |
|---|---|
| Request and response JSON | 8,192 bytes each, excluding the newline |
| Text | 300 UTF-8 bytes, no control characters |
| Active items | 128, with no eviction |
| List page | At most 8 items and within the response byte bound |
| Socket clients | 4 workers plus 4 queued connections |
| Storage-owner request queue | 16 entries |
| Complete request read | 300 ms from connection acceptance, including queued time |
| Owner response wait | 1,100 ms from connection acceptance |
| Entire client read/write budget | 1,200 ms from connection acceptance |
| Idle owner due scan | 50 ms tick; requests also run a due scan |
| Observer considers owner stale | After 1,500 ms without an updated owner report |

Queued work that has expired before the owner starts it is refused. Once a
storage call starts, a socket deadline cannot cancel its transaction or bound
`fsync`. Client readers and the main bus observer run separately from the one
storage worker. A silent or trickling client cannot hold that owner while it
reads a request. An actual disk stall can hold due detection and every queued
request; the observer withdraws `ready` and the actionable notice when its last
owner report becomes stale, provided the observer and bus can still progress.
Shutdown likewise cannot force a blocked filesystem call to return.

A requested stop is not a storage failure (#869). The storage worker watches
the same stop flag as the service's loop, so it may end within the loop's pass
that the stop arrives in. The loop looks at the worker first and the flag
after: a worker found ended while no stop had been asked for is a failure
(`Schedule store worker stopped`), and the last report before it says it is
not progressing. A worker that ended after a stop has stopped as asked, and no
such report is published. Either way its own result is read when it is joined,
so an error of its own is still returned. Tests:
`a_stop_asked_for_while_the_store_worker_ends_within_a_pass_is_a_clean_stop`
(fails with that message without the fix) and
`a_store_worker_that_fails_with_no_stop_asked_for_is_still_a_failure`
(`crates/schedule/src/runtime.rs`).

The interactive targets, not measured lamp guarantees, are: a complete valid
request to durable response within **250 ms** on healthy storage; due detection
within **50 ms plus storage and queue time**; and the main observer beginning
publication within **5 ms after a committed owner report** becomes visible.
The main loop sleeps 5 ms, but its socket and bus operations and host scheduling
also cost time. No platform-independent deadline is claimed for disk sync,
end-to-end audio, or a person hearing the reminder.

## Bus and diagnostics

`schedule/due` is `lamp_msgs/ScheduleDue` version 1, **Latest**, with a 1-second
period and a 3-second lifespan. The owner also publishes on revision and
readiness changes. The period is `runtime::REPORT_EVERY`, given as
`Config::report_every`; tests give 50 ms, so a test that reads the report
does not wait a second for it (#1806). Its fields are `stamp`, `store_id`, `revision`, `enabled`,
`ready`, pending count, waiting-for-clock count, and optional `oldest` metadata:
`id`, `delivery`, `due_unix_ms`, `late`, and optional `late_by_ms`. There is no
reminder text in this topic. It is a hint to inspect durable work, not a queue
of speech commands. Multiple due items remain in the private store even though
the topic carries only the oldest one.

The consumer must validate the message shape, current host boot, shared
monotonic age, bus publisher identity, and ordering. A restarted publisher's
sequence belongs to its new bus identity. A renewed heartbeat does not create
another delivery. `ready` means the Schedule owner is progressing; it says
nothing about Voice's privacy state or ability to speak. Disabled or unready
metadata offers no actionable `oldest`. If publishing fails, the committed
pending work remains available through the socket and future publications.

`diagnostics` reports item/pending/waiting counts, requests, wall trust, storage
readiness, publication failures, and refused clients. It does not log reminder
text or model arguments. Failure diagnostics are not an acknowledgement. There
is no bus acknowledgement topic: completion is a guarded private `ack` request.

## Proposed model tools

The six proposals are now implemented in Actions' optional adapter and Voice's
declaration/routing gate. Enable `LAMP_SCHEDULE_TOOLS=on` in both processes and
provide Voice's existing `LAMP_ACTIONS_DIR`. The executable provider declarations
are in [`actions/src/schedule.rs`](../crates/actions/src/schedule.rs):

| Tool | Required arguments | Optional arguments | Schedule operation |
|---|---|---|---|
| `set_timer` | `text`, `seconds` | none | `Set` with `After` |
| `set_reminder` | `text`, `time` | `day`, `zone`, `fold` | `Set` with `At` |
| `repeat_reminder` | `text`, `time`, `every` | `weekday` (required for weekly), `zone` | `Set` with `Daily` or `Weekly` |
| `list_reminders` | none (empty object allowed) | `pending_only` (default false), `after_id` (default 0) | `ListSet` with the line's `asker`, at most eight source items per page, only the line's own, and the room's short timers |
| `cancel_reminder` | `id` | none | `Cancel` of the selected whole item/series, only the line's own |
| `snooze_reminder` | `id`, `seconds` | none | `Get` then `Snooze` of the exact pending delivery, only the line's own |

Text is 1–300 UTF-8 bytes, nonblank and without controls. Seconds are whole
integers from 1 through 31,536,000. `time` is exact `HH:MM`; `day` is `today`,
`tomorrow`, `next`, or `YYYY-MM-DD` in 1970–9999. An omitted day means the next
strictly future occurrence. Zone is a valid IANA name; an omitted zone uses
Schedule's configured zone. Fold is `earlier`, `later`, or `reject`. `every`
is `daily` or `weekly`; weekday uses its full lowercase English name. IDs
must come from a current list, never a guess. Omit optional values rather
than passing null; unknown or null fields are refused before IPC.

**`pending_only` means due, not set (#1595).** An item is pending only once
its time has come and it is not yet said (`engine.rs`, `tick`), so a list
asked `pending_only` leaves out every timer and reminder still to come. On
lamp-4ace on 18 October at 19:40, "cancel my timer" 15 s after a two-minute
timer was set got "I don't have any timers or reminders set right now", and
the timer rang (likely a `pending_only` list; the call's arguments are not
journalled). So the schema now says what it means ("true lists only items
that are due now and not yet said; leave it out to list everything that is
set"), `cancel_reminder` says to list without it, and a list asked
`pending_only` that left items out carries `not_yet_due` (their count, from
`Status`) and a `next` telling the model to list again before saying none
are set (`lamp_actions::schedule::NOT_YET_DUE`). The count is best effort: a
`Status` that can't be read leaves the list as it was. Test:
`a_just_set_timer_is_listed_counted_beside_a_due_only_list_and_cancelled_for_good`
(`crates/actions/tests/it/schedule.rs`, against the real Schedule service).
Each listed item also says its `kind`, and `cancel_reminder` says to cancel
only an item of the kind asked for, so "cancel my timer" with a reminder also
due cancels only the timer. The list stays oldest first. Test:
`cancel_my_timer_with_a_reminder_also_due_cancels_only_the_timer`.

**Questions about a reminder are answered from the list (the person's call,
30 Oct).** "What did I ask you to remind me about?", "when is my reminder?"
and "when did I set that reminder?" are the reminder list's, not memory's:
reminders are not memories (memory.md's decision record), and a fired
reminder's turn is kept out of the journal on purpose. The miss was the
model calling `recall`, or nothing, and saying it did not remember. So
`list_reminders`' description now ends (`LIST_ASKED`, after the earlier
text, kept as `LIST_BEFORE`): call this, not recall, for any question about
their reminders or timers, what is set, when one is due or when they set
it; never say you do not remember a reminder before calling this; and
never list reminders unasked (rule 8). Both routes declare the same
catalog, so the Live route and the second route are told the same.

Each listed item now says when it was set: `set_day` (`today`,
`yesterday`, or its `YYYY-MM-DD`) and `set_clock` (its local `HH:MM`), so
the lamp can say "you set it yesterday at nine". They come from the
`created_unix_ms` every stored item has always kept (a `Set` needs a trusted
wall clock, so it is a real time), said in the lamp's zone as it is when
listed (`time::set_at`); the store's format is unchanged, so old stores
load with their set times. They come from a request of their own,
`list_set` (`Request::ListSet`, the same arguments as `list`), whose items
add `set_local` (RFC 9557) and `set_day`; with now's wall clock untrusted,
the day is its date, never `today` on a guess. `list` and `get` are exactly
as they were, because the delivery ledger reads their items strictly (an
older voice would refuse a field it doesn't know). An older Schedule
service refuses `list_set` as `bad_request`; the adapter then asks the same
`list` once (a read, nothing to repeat), and every other refusal is not
asked again. Only Voice's `list_reminders` asks `list_set`: the app's
`schedule.list` (`crates/cloud`, `Adapter::call_plain`) asks plain `list`
and its items carry no set time, so its contract is unchanged. `set_day` and `set_clock` are null, told as "I don't know
when", when they can't be said: a zone this build can't name, or an older
Schedule service. Tests: `a_list_set_item_says_when_it_was_set_in_the_lamps_zone`
and `kept::each_kept_state_reads_with_its_items` (`crates/schedule/tests/it`),
`each_listed_item_says_when_it_was_set_and_a_day_on_it_is_yesterday`,
`an_item_without_a_readable_set_time_lists_with_it_unknown`,
`an_older_schedule_is_listed_without_set_times` and
`list_reminders_is_described_for_questions_about_reminders`
(`crates/actions/tests/it/schedule.rs`). The wording is measured with
`lamp-voice probe-reminders` ([second-voice.md](second-voice.md)).

**Whose reminders a line hears (Reviewer-three's after-merge P2 on #4527,
the first builder's (a), 31 Oct).** After #4527 sent every question about
"my" reminders to this list, a colleague at the desk asking "what did I ask
you to remind me about?" would have heard the owner's "take my pills". So
each reminder now keeps who set it, as the person's memory id (`p-` and 16
hex digits, as the hello does in #4625), never a name: the stored item's
optional `person`, written by `Set`'s optional `person`. It is optional
stored metadata, so there is no `schema` bump: a store from before loads
with every item set by no one, and an older build reads a newer store,
dropping the field (its items then are no one's, which only narrows).

- **The setter** is the person a keep in that line would be written for
  at that moment, under memory's scope rules (`memory_scope`): the line's
  own person; for a line read as the room's, the face the eyes named by
  its end (`keeper_within`) or the one-face keep (`kept_by_one_face`,
  #4584). A line held only by a face's gap, or carried as a conversation,
  keeps nothing at once, so what it sets is no one's. No sure person is
  none (`crates/voice/src/tools/schedule_whose.rs`).
- **The reader** is the line's person as memory's reads take it
  (`ScopeLease::scope`): a one-face line reads the room's, so it is not
  "theirs" for listing; a held gap reads nothing of theirs.
- **A line hears only its own.** `list_set` with an `asker`
  (`protocol::Asker`, `{person, owner}`) lists only the items it may see
  (`Asker::may_see`): a person's own; one set by no one only on the
  **owner's line**, the line whose reader is one of the owner's memory ids
  (`person::owner_ids`: the typed setup name's and the face who_you_are
  kept), as #4476 tells the typed name only there. An unknown face or the
  room hears none of anyone's reminders. Its `not_yet_due` is the line's
  own count, told beside a `pending_only` list in place of `Status`'
  whole-store one.
- **A timer set with nobody sure is the room's** (option 1 on #4656's
  after-merge P2). Every line hears, cancels and snoozes a timer set as
  no one's for two hours at most (`ROOM_TIMER_MS`). A timer is
  `set_timer`'s duration from now: an item whose deadline keeps an elapsed
  clock, the `kind` `list` says. Without it, a person at the stove with
  their back to the lamp set a timer as no one's, and then "how long's
  left?" found no timer and "cancel the timer" was `not_found`. When it's
  due it's said aloud to whoever is there. A timer a person set stays
  theirs, and so does a longer one: `set_timer` also sets relative
  reminders, up to a year out ("in three weeks, renew my prescription",
  Reviewer-three's P1 and Third's line on #4668).
- **`cancel_reminder` and `snooze_reminder` by id** act only on an item the
  line may see: `Cancel` and `Snooze` with an `asker` answer someone else's
  reminder as `not_found`, never as someone else's, and a snooze says that
  before "not due". A guest can't cancel the owner's reminder by its id,
  but can cancel the room's timer.
- **The voice sends its line's caller.** Beside each schedule call on the
  executive's socket the voice adds `caller`
  (`{"asker":{"person","owner"},"setter"}`, `lamp_actions::schedule::Caller`);
  a call without one is nobody's: it hears and changes none of anyone's,
  and what it sets is no one's.
- **The app is the owner's view.** `schedule.list` and `schedule.cancel`
  (`Adapter::call_plain`, `Adapter::call`) send no asker and see every
  item, as before: the app is the owner's, over the lamp's own account
  channel.
- **A forget takes them too.** A person's "forget everything about me"
  that emptied their memory cancels every reminder they set
  (`Request::Forget`, the executive's own `schedule_forget` op, which no
  model call can name), and the answer says `reminders: {"cancelled": n}`,
  or that they could not go. The app's `face.remove` with `entirely`
  cancels them for each of the memory ids it forgets, with the schedule
  given to lamp-cloud (`LAMP_APP_SCHEDULE=on`); without it they stay and
  the answer is as before, with no `reminders`. The reminders are cancelled, not kept as no
  one's: kept, they would turn up on the owner's line. A factory reset
  already wipes the store.
- **An older Schedule** refuses an `asker` or a `person` as `bad_request`:
  the list is then the owner's line's alone (every item there is no
  one's), and a set, cancel or snooze is unavailable. The voice, actions
  and schedule ship together.
- **A due reminder is said as before**, to whoever is there: it is due, so
  it is said. The delivery ledger's `list` and `get` are unchanged.
- **No switch:** it only narrows who hears what.

A room line's reminder (nobody known) is then heard and cancelled only on
the owner's line: on a lamp where nobody is known by face or typed name,
no line is the owner's, and such a reminder is said when due but can't be
listed or cancelled by voice, only from the app. Tests:
`each_line_lists_only_the_reminders_it_set_and_the_owner_also_hears_the_no_person_ones`,
`a_cancel_or_snooze_by_another_persons_id_is_not_found_and_changes_nothing`,
`a_persons_forget_cancels_every_reminder_they_set_and_survives_a_reopen`
and
`a_store_from_before_setters_loads_with_every_item_no_persons_and_keeps_the_setter_on_reopen`
(`crates/schedule/tests/it/engine.rs`);
`each_person_hears_and_changes_only_their_own_reminders_through_the_real_server`
and `a_persons_forget_cancels_their_reminders_only_through_the_voices_own_op`
(`crates/actions/tests/it/schedule.rs`); `tools::schedule_whose`'s three
(`crates/voice`); `face_remove_entirely_cancels_the_persons_reminders`
(`crates/cloud`).

The declarations use Gemini's small `parameters` schema subset, omitting
`additionalProperties`, `maxLength`, and regex patterns. Runtime validation
enforces the stricter contract above. First must qualify the declarations on
the actual model; host acceptance is not provider qualification.

The adapter enforces 00–23 hours, the service's UTF-8 byte limit, valid
day/zone values, and weekday presence only for weekly requests; the schemas
above do not replace service validation. It maps `set_timer` to `After`,
`set_reminder` to `At`, and `repeat_reminder` to `Daily` or `Weekly`. List output
remains bounded to 4,080 JSON bytes for the model's separate tool-result budget.
It returns complete item texts, an explicit next cursor and a truthful page
boundary, never a silently truncated reminder. The adapter
obtains current guards and the current pending delivery for snooze. Neither
guards, delivery acknowledgements, wall-clock trust, nor boot-clock arithmetic
belong in the model's arguments.

Relative requests use `set_timer` even when phrased as a reminder:
"remind me in two minutes to stretch my legs" becomes
`set_timer {"text":"Stretch my legs","seconds":120}`. "In ten minutes" uses
600 seconds and "in an hour" uses 3600 seconds, with the requested label.
Both tool descriptions distinguish these elapsed durations from clock times;
they must not be rounded into `set_reminder`'s minute-only `HH:MM` argument.
Only a requested clock time, such as "at nine tomorrow morning", uses
`set_reminder`. This is model guidance, not a deterministic natural-language
parser: the actual selected tool still needs lamp qualification.

Other example model arguments are `set_timer {"text":"Check the pasta","seconds":480}`,
`set_reminder {"text":"Water the plant","time":"09:00","day":"tomorrow"}`,
and `repeat_reminder {"text":"Take the bins out","time":"19:00","every":"weekly","weekday":"thursday"}`.
“Snooze that for ten minutes” can become
`snooze_reminder {"id":1,"seconds":600}` only after the caller selects the current
reminder ID from a list or context. The adapter validates that ID's exact pending
delivery. The owner resolves the date and returns the
actual local deadline. If Schedule cannot commit, the lamp must say it cannot
set the reminder just now; it must not give a success confirmation anyway.

Actions uses `LAMP_SCHEDULE_SOCKET` (default
`/run/lampos-schedule/call.sock` when enabled). One tool call has one absolute
1,250 ms deadline across every Status/Get/mutation hop. Voice gives its new
Schedule-only call 1,500 ms; ordinary tool transport keeps its old behavior.
The healthy local target is 250 ms, measured separately from model/audio delay.
A malformed or lost reply after a possible write means confirmation is unknown.
There is no automatic fresh-guard mutation retry and no exactly-once promise
for repeated model calls. List current state before deciding whether to ask
again. Item IDs are meaningful only in the current store: replacing Schedule's
store requires listing again before selecting an old remembered ID.

### Relative-time follow-up on the lamp

First reported on lamp-4ace that "in two minutes" fired about 73 seconds
after its request because the model used a rounded `set_reminder` clock time
([PR #747](https://github.com/autonomous-ai/lampOS/pull/747#issuecomment-6179199142)).
The follow-up changes the two enabled descriptions and tests the catalog
published by Actions. The schemas, service timing, gates and deadlines stay
the same. It adds no network or storage operation; the healthy local tool-call
target remains 250 ms, separate from provider and audio latency.

First installs the follow-up's native CI `lamp-actions` binary while retaining
the current lamp-owned settings, character, Schedule store and Voice delivery
ledger. Use the existing Task 31 off-minute procedure. After restoring the
enabled settings, restart Actions before Voice so its next provider session
reads the new `tools.json`; this follow-up requires no new Voice binary.

1. Start away from a whole-minute boundary. Say "remind me in one minute to
   stretch" and "remind me in two minutes to check the tea" in separate trials.
   Record the selected tool/arguments, accepted Schedule deadline, confirmation
   and audible due time. Expect `set_timer` with 60 and 120 seconds and the
   requested labels. Neither may fire before its elapsed deadline; delivery
   may wait for the existing local-silence, privacy and readiness conditions.
2. Ask for "in ten minutes" and "in an hour". Verify `set_timer` with 600 and
   3600 seconds and the requested labels, then list and cancel those items.
   These checks do not require waiting an hour or claim an hour-long timing run.
3. Ask for a reminder "at nine tomorrow morning". Verify `set_reminder` with
   `time:"09:00"` and `day:"tomorrow"`, then list and cancel it. Check a normal
   explicit timer, light/volume, interruption and privacy as before.

The likely remaining gap is the live model choosing or converting the wrong
tool despite its descriptions. Host catalog assertions cannot establish that
choice or acoustic delivery timing. Third has no lamp or live-provider result
for this follow-up.

### A plain ask must set it (`LAMP_SCHEDULE_ASKED`)

On lamp-4ace on 25 Oct, on the ship build 6819 and its product unit, "Lamp,
remind me in three minutes to call my sister." was answered "Three minutes,
call your sister." with no call: nothing was set, and nothing was said three
minutes later. The two asks before it had called `set_timer`. With
`LAMP_SCHEDULE_ASKED=on` (the second route; on in the product unit), a line
that plainly asks for a timer or a reminder at a sentence's start
(`tools::schedule_asked::asks_to_set`) makes its first chat round call the
function that sets it (`tool_choice`): `set_timer` for a duration from now,
`set_reminder` for a clock time, as their descriptions ask. The time must come
right after the ask ("remind me in three minutes …", "remind me tomorrow at
6:30 …", "wake me up at seven", "at seven in the morning"), a clock time must
stand as one, and after the time come only "to" and the task, or "please". A
task's length stands ("… to exercise for twenty minutes"), but a second time
or a day in the task ("… at ten", "… tomorrow"), a condition ("… when the oven
is ready", "… if it rains"), a clause after a comma ("…, translate that into
French"), another sentence's time or "at 180 degrees" forces nothing, and a
length runs from now, so "set a timer tomorrow for ten minutes" forces
nothing. It errs toward not forcing: the line must be one sentence, the ask
(beside a greeting), and a line with quote marks (single or double), brackets,
a dash, a `%`, or any word that may take the ask back, put it in doubt or make
it another operation ("don't", "no", "wait", "cancel", "actually", "or",
"leave it", …; a repeat or a span: "every day", "on Mondays", "twice a day",
"for a week"; a later start: "starting at noon"; a condition: "when", "if",
"once") and a
question that isn't an ask are asked as before, and the round after the call is asked with its
result, so the lamp says what the schedule did.

## First's Voice integration contract

Exactly one logical consumer must own delivery. There is no claim/lease API to
arbitrate multiple independent speakers. That consumer needs durable local
deduplication keyed by `(store_id, id, delivery)` and a stable request identity
for each acknowledgement. Bus sequence is not a delivery identity.

1. Validate a fresh enabled, ready notice, then call private
   `{"op":"get","id":1}` for the offered ID. Compare the reply's top-level
   `store_id` and its `item.id` and `item.delivery` with the offered tuple, and
   require `item.state:"pending"`. Do not add store or delivery fields to the
   `get` request; that operation accepts only `op` and `id`. A retained bus file
   alone is not authority to speak. Use the private text and current late
   metadata.
2. Let final Voice ownership decide whether privacy, interruption, local
   silence, owed follow-ups and current input/output readiness permit speech.
   A requested due reminder does not wait for a room classification: Unknown,
   Busy and Quiet are equivalent here. Schedule cannot grant microphone/camera
   permits or override the current owner. Do not copy greeting/“hello”
   heuristics into reminder eligibility.
3. Persist the consumer's delivery identity and its own in-flight correlation
   before starting playout. Recheck current work before a delayed start. A
   cancellation or snooze invalidates the old occurrence; integration must also
   cancel any matching queued or active audio and fence late callbacks.
4. Only a qualified receipt that the entire intended reminder audio completed
   playout permits `ack` of that exact delivery. A bus send, a provider accepting
   text, generated audio, queue insertion, or the first sound is insufficient.
   Interruption, cancellation, failure, and unknown completion leave it pending
   or require rereading current state; they are not fabricated completion.
5. Persist the completed-playout evidence, obtain a current mutation guard, and
   send one stable acknowledgement request. Resolve a lost acknowledgement
   reply through exact retry and durable receipt inspection. Do not replay the
   sound merely because its acknowledgement reply was lost, and do not let an
   old acknowledgement consume the next recurrence.

There is an unavoidable unproven boundary between sound finishing and durable
completion evidence. A process or power failure there leaves this consumer's
attempt uncertain and pending; the next voice offers it again (below), so it
may be heard twice. An explicit later snooze may also cause someone to hear it
again. The implementation does not promise
exactly-once speech or that anyone heard it. First must test that window with real playout
receipts, interruption, process restart, privacy changes, and the chosen
consumer's persistent deduplication. The implementation below adds Voice and
Actions only; installation and on-lamp qualification remain First's work.

## Optional Voice consumer

Voice requires exact `LAMP_SCHEDULE_SPEAK=on` (or `skill`, where a due
reminder also waits up to 10 s for the Reminders skill to ask for it through
the say service, then is said by Voice itself; the skill asks only while
`voice/state` rests, so a reminder due during a talk is asked for when the
talk ends, as Voice says it, #1667: [speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939))
(both values give the provider worker its `--schedule-speak`, by one rule,
`reminders::speaks`: until 17 Oct `skill` gave the worker none, so it
refused every reminder and none was said, the reviewer on #1038)
and an explicit absolute
`LAMP_SCHEDULE_DELIVERY_DIR` on persistent storage. Provision its parent for the
Voice service user; the final directory/database are private 0700/0600. This is
a separate tagged ledger, never Schedule's item database or an audio session
directory. It retains no reminder text. A changed store identity, wrong ledger
kind, or ambiguous storage failure refuses new sound. Never delete this ledger
to clear an operational fault.

The saved delivery ledger reads harmless added fields at schema 1, including
nested entries and proof metadata. Known fields and their proof bindings still
validate; missing required fields, duplicate known fields and unknown `kind`
or `op` values are refused with the stored bytes unchanged. Public Voice IPC
types remain strict. An exact saved Ack request and its guard also remain
strict: their fields must not be silently removed before retransmission. So a
field added to `Request::Ack` or `Guard` without a bump would make an older
build refuse the whole ledger, not set it aside; a test pins their saved keys.

- **The rule for a change to the saved ledger:** these bump `SCHEMA` in
  `delivery.rs`, in the same PR:
  - a new enum value, a new required field, or a change to the exact Ack
    request or its guard;
  - a field that changes admission, completion or acknowledgement meaning;
  - a field a newer build needs back after a rollback and a roll forward,
    not just one it can do without: an older build ignores it, and it is
    gone after that build's next save;
  - a larger `MAX_STATE_BYTES`: an older build refuses a state over its own
    bound before it reads `schema` (`DeliveryLedger::open` checks the size
    first), so it can only set aside what it can read. A ledger over the
    older build's bound is refused there, and its due reminders wait.

  Any other added field needs nothing more; it disappears on the next save.

On a newer `schema`, the consumer closes and durably renames the whole database
to `schedule.sqlite3.newer-<schema>-<unix-seconds>` under its existing directory
lock, without replacing an earlier archive. It then commits an empty schema-1
ledger, and after that commit writes one journal line naming that file. A crash
after the rename and before that commit leaves the kept `newer-*` file with no
journal line, and the next start begins empty, so the kept file is the record.
A preservation failure refuses startup. No entry or completion proof from the
newer ledger is interpreted.
Current pending reminders can be offered again after the normal fresh Schedule
observation, single-consumer claim and Voice admission checks. A reminder heard
before rollback but not acknowledged in Schedule may therefore be heard twice;
this follows the existing preference for a possible repeat over a lost reminder.
Already acknowledged reminders are absent from Schedule and are not replayed
from the preserved file. This work occurs at consumer startup; it adds no wait
or I/O to Voice's audio path.

One child, started by Voice's internal `schedule-consumer` entry, owns all Bus,
Schedule socket and ledger work. It survives mute/unmute audio cycles. A fixed
per-UID claim `/run/lock/lampos-schedule-consumer-<uid>.lock` is opened without
following links, validated as private and locked without waiting. Provision
permission to create that file for the service user. It is never unlinked. The
child transfers the same locked descriptor over an inherited private datagram
socket; Voice retains it even after child failure. This excludes a second
cooperating consumer with a different ledger on the same host and UID. Different
UIDs or hosts remain a deployment single-consumer constraint; Schedule has no
remote claim API.

Before accessing paths or taking the claim, the Linux child sets and reads back
normal `SCHED_OTHER`, static priority 0 and nice 19. It must not inherit Voice's
realtime audio priority. Failure leaves the optional consumer unavailable;
the parent and audio workers keep their policies. Nice 19 deliberately gives
storage work less CPU under load and may delay a reminder. macOS fixtures do
not perform or qualify this Linux transition.

The parent performs no ledger, Bus or Schedule filesystem/socket request on the
urgent path. Its cooperative polling target is 2 ms, with bounded 4,096-byte
datagrams and four protocol packets per poll. Process creation, OS scheduling,
kernel calls and physical audio have no claimed hard deadline. A stalled child
expires offers and cannot create a replacement worker. Optional failure leaves
ordinary Voice running and reminders pending.

The child checks one current Schedule publisher, the typed declaration, host
boot, monotonic age at most three seconds, and source/typed sequence ordering.
It fetches the exact pending occurrence and durably prepares its correlation
before offering text. Offers last at most 750 ms from the observation preceding
socket/storage work, bounded also by the original bus timestamp. Revalidation
starts no sooner than 200 ms after the previous tick completes. Each tick shares
a 250 ms socket request budget; storage and OS work can extend that interval,
and slow storage cannot renew an old observation. Cancellation, snooze, changed store, lost freshness or a
stalled consumer withdraws queued/current reminder audio through Voice's normal
owner revocation. These are host targets and bounds, not measured acoustic stops.

The core Reminders skill uses a bounded change trigger while Voice rests and
Schedule is ready and enabled. It asks once on first eligibility, after a
change to `(store_id, pending, oldest.id, oldest.delivery)`, or when eligibility
returns after talk, disabled/not-ready state or source expiry. A held occurrence
can keep the count positive across later work; no zero-to-positive edge is
required. Ordinary one-second heartbeats, revision, timestamps and lateness do
not repeat an unchanged held request. Several changes in one observation make
one opportunity. Voice's fallback, admission and five-second delivery-attempt
pacing remain unchanged; this is not an end-to-end timing guarantee.

An opportunity is not a delivery or permission to look. Voice's durable consumer
chooses the actual occurrence. Skills permits one executing reminder run,
rejects additional runs as busy, and moves only after Voice returns `begun`.
An unchanged held tuple produces one opportunity across sixty heartbeats, with
no look or speech without that response. Voice caps source-ask
eligibility at 20 seconds and expires before selecting. Socket reads use
the existing 180-second per-read timeout; connect has no explicit
deadline. Neither is an absolute execution-duration bound. Busy serialization
still permits only one executing reminder run. The policy is the existing
local reminder rule, with no model request. Pure skill replays count planned calls, not actual speech or
motion; physical time-to-first-audio still requires the lamp replay.

The child journals (stderr, so the voice's journal) one line,
`lamp-voice schedule-consumer: ...`, each time what it does with a due item
changes, and never one per tick. At start it says `started`, then
`due reminders available` at its first good read. Per item:
`due, offered to the voice (attempt N[, late])` (each attempt is its own
line, so a reminder deferred while people talk is said once per retry),
`said, waiting to be acknowledged` (a completion kept from before),
`withdrawn: <why>` or `not offered: <why>`, `said; its completion is kept, to
be acknowledged`, `not completed: this attempt can no longer speak, to be
offered again`, `held: an attempt may have been heard and was never proven
said, so it is not offered again`, and `resolved (<resolution>), not
offered`. When the due notice itself cannot be read or trusted it says
`due reminders unavailable: <why>` once, and `due reminders available` once
it can again. It remembers the last line of 64 items; past that, the item
written longest ago is forgotten first. The parent says
`lamp-voice: Schedule consumer failed (<why>); due reminders wait for a
restart` when the child is lost.

How it is said: the provider is asked for one short natural sentence with the
item's own words as data, never to count seconds, and not to call it a
reminder or a timer unless its words do. Late by under three minutes it is
said as on time (Schedule marks anything over one second `late`; a creature
does not). Later, the delay is named once, plainly and with a short apology,
in rounded words: `about 7 minutes`, `about 12 hours`, `about 3 days`
(`provider_worker/reminder_policy.rs`). With no delay known it is never said
as on time: it waits and is said as an overdue one is (below).

A reminder due an hour or more ago (`reminders::OVERDUE_MS`), or due by an
amount Schedule cannot give (no `late_by_ms` while the wall clock is not yet
trusted, as in the first minute after a boot), is not told to an empty room.
It begins only when someone is at the lamp, as for the news: the eyes'
presence file says so, or something speech-long was heard within the last
two minutes (a TV counts, as it does for the news). While it waits it holds
no skill's ask. It is then said once, gently, as something asked for
earlier ("You'd asked me to remind you to stretch, earlier."), with no delay
named, and acknowledged as any other. While it waits the voice says once
for each attempt (again after a withdraw and retry),
`lamp-voice: a reminder due over an hour ago, or by an unknown amount, waits for someone at the lamp`.
The consumer offers its oldest due item first, so newer due work waits
behind it until someone comes. A timer or reminder set by voice is not held
by this: asking for it is speech, which opens the two-minute window, so the
overdue one is said in the first quiet second after the lamp answers, and the
new one is not behind it when it comes due. Only an item set without
speaking (from an app, or an agent's `schedule` call) can wait behind an
overdue one while nobody is seen or heard (Nine and the reviewer on #1103).

**Due while the microphones are off (#1545).** A muted lamp opens no
session (`service_waiting_for_privacy`), so a reminder that comes due while
the privacy switch is closed, or the lamp is muted or asleep, is not said
then: saying it would open a session while the person has said "don't
listen" (the first builder's call on #1545). It waits, still offered by the
consumer, and the muted wait says it is owed. The journal says once for
each attempt
`lamp-voice: a reminder is due while the microphones are off: it waits until they open`,
the session's trace gets `reminder_owed_while_muted`, and the voice's
health while it waits adds "a reminder is owed, to be said when they open"
and `reminder` `owed` to why the microphones are off (`bus_out::OWED`).
Only while the wait holds them closed (`Waiting::closed`: the switch, a
mute, a hand, the restart guard, a switch that cannot be read): the same
wait runs at every start of the voice while the switch is first read and
once it has read open, and a reminder pending then is no mute's, so it gives
no line that would later hold back the real one (the reviewer on #1551).
When they open, it is the first turn the voice's loop may take, said as any
late reminder is, and resolved `said`; one that waited an hour or more
follows the one-hour rule above. With `LAMP_REMINDER_PULSE=on` (and
`LAMP_RING_ASLEEP=on`), the asleep face shows one soft pulse a minute while
it is owed and the switch or a mute holds the microphones (not asleep), the
first at its due time, for half an hour from then at most ([ring-choreography.md](ring-choreography.md), "The asleep face"). One cancelled meanwhile is withdrawn and
never said, and the health stops saying it at once. On lamp-cb94 on 17
October one came due 21 minutes into a mute, and nothing said it was owed.
Tests:
`coordinator::tests::reminders::a_reminder_due_while_muted_is_owed_once_and_no_turn_begins`,
`on_unmute_the_owed_reminder_is_offered_to_the_first_turn`,
`a_reminder_cancelled_while_muted_is_never_said`,
`a_mute_past_an_hour_leaves_the_reminder_to_the_one_hour_rule` (Schedule's
clock moved past an hour during the wait; the provider is given the
lateness) and `a_reminder_pending_at_a_start_is_owed_to_no_mute`; for the
pulse, `the_muted_wait_tells_the_asleep_face_a_reminder_is_owed_only_while_it_may_pulse`,
`a_pulse_held_back_keeps_the_time_it_was_owed_from`,
`an_owed_reminder_pulses_under_the_switch_or_a_mute_and_never_asleep`,
`a_pulse_switched_on_without_the_asleep_face_says_why_once`,
`ring_asleep::tests::an_owed_reminder_pulses_from_when_it_was_owed_each_minute_for_half_an_hour`,
`the_asleep_face_pulses_only_while_a_reminder_is_owed` and `lamp-ring`'s
`an_owed_arc_goes_around_warm_and_never_above_the_peak`.

Admission requires current microphone retention/privacy, ready provider, no
reply, queued input, speaker receipt or draining/prospective utterance, and one
second of fresh local silence. A promised camera look and the existing
unanswered-name second look take their owed turn before a due reminder. The
prepared reminder remains available afterward; waiting does not consume its
offer or acknowledge delivery. Room classification changes nothing for a due
reminder: Unknown, Busy and Quiet all permit the requested reminder at the
next local silence with normal readiness. A change of room classification
while the provider prepares or speaks it does not defer it. This follows
[First's answer on #709](https://github.com/autonomous-ai/lampOS/issues/709#issuecomment-6176285386):
a timer set after restart must not wait for RoomTold's five-minute quiet
classification. Actual input, privacy, output ownership and owed follow-ups
still take precedence; a Busy classification does not mean someone currently
owns the local input turn.

Reminder text and current lateness are JSON-quoted data in a fixed spoken
reminder instruction. Every tool call from that due request is denied before
the tool desk, including Memory, Actions and any future tool. Ordinary written
requests keep their old wire form and behavior. Reminder attempts do not invent
heard/unanswered person evidence in RoomTold; actual output speech is still
recorded there. A silent reminder also does not count toward renewing a
provider session for unanswered speech. The existing short-sound rule remains:
an unanswered sound is not talk, while an ordinary unanswered speech request
still counts. A deferral is silent and leaves intake usable.
Reminder completions stay out of automatic Memory journalling, including
`LAMP_MEMORY_DAYS=on`. The Days regression runs in an isolated process with
the real Voice memory queue and a local socket peer: a kept-out reminder is
followed by an ordinary exchange and a FIFO conversation-end barrier. The
peer must receive only the ordinary exchange, with its snapshot provenance.
This checks Voice's journal handoff; it does not call a real memory extractor
or establish behavior on a lamp.
Reminder output also cannot request a follow-up camera turn by beginning with
"Let me look". A per-conversation turn watermark rejects delayed reminder
transcripts as well as the current one; future ordinary turns keep their
existing look policy.

Only a successful provider terminal joined with ended input, empty PCM, no
speaker work, no playback occurrence, no gaps/reset, and actual final
provider-origin retirement produces a completion record. Multiple generations
must all retire. Zero audio, local failure notices, partial acceptance, first
audio, cancelled tails and late callbacks cannot acknowledge. The receipt is
the speaker's ALSA cursor evidence, not proof of acoustic fidelity or that a
person heard it. A failed reminder does not play a local failure recording.

When a new line reaches admission after that whole-output proof is already
available, the reminder first takes the ordinary completion path. The pending
line and its audio prefix stay in admission; completion updates their expected
owner, and the next capture opportunity can admit the same line, including on
a quiet frame. Only a currently owned, microphone-permitted reminder with
qualified final-retirement evidence takes this path. Partial, failed, reset or
otherwise unqualified delivery keeps the interruption path. No completion is
inferred from the successor, first audio, or accepted-but-unretired PCM.
This adds no timer, provider wait or ledger work to the coordinator. The
target is one additional capture opportunity (normally a 10 ms frame), not a
hardware or acoustic deadline. Privacy, lease and authority checks still run
before completion; without fresh capture, the pending input retains its normal
expiry rather than receiving new readiness evidence.

A skill's `from: schedule` say reads that same local completion record after
Voice tries the whole-reply join. It reports `spoken` only when the record is
`Complete` and the existing whole-playback checks still pass. A playback-clock
reset can invalidate the receipt without adding a playback gap; that case
reports `dropped` with `reminder_completion_unconfirmed`, never `spoken` beside
an abandoned reminder. Provider failures and playback gaps keep their existing
failure reasons. Ordinary says and skill sounds keep their existing checks.
This read adds no I/O or wait to the audio loop, and does not claim the worker
has already durably stored completion or acknowledged Schedule. The coordinator
fixtures pair the actual say ticket with clean, partially accepted, reset, gap,
failed, silent and privacy-revoked reminder turns; their PCM/cursor evidence is
synthetic, not a listening test.

Completion is durably stored before Ack. Exact Ack retries preserve the whole
guard and request; a newer still-pending observation may durably rebase its CAS
revision. Lost replies are checked against the actual receipt and exact current
item. Absence or advancement resolves an occurrence as gone/superseded, never
as a fabricated acknowledgement. Completed records recover after restart
without replaying sound. At most 128 unresolved entries are retained; only
proven resolved entries can be compacted, preserving a revision high-water mark.

An offer withdrawn before admission or a provider deferral before any PCM may
retry after explicit owner qualification, with at least five seconds between
attempts. A positively admitted user interruption also permits retry before
any qualified own audio generation has retired, including partly played audio
whose generation is still unretired, but only after the coordinator revokes its exact
owner, clears queued output and successfully publishes the replacement or
revoked authority. The receipt is retained until that boundary; any failure
still leaves the attempt held. An already failed provider or invalidated
playback clock does not qualify. An existing own-retirement cursor keeps the
attempt held even if no whole-provider completion arrived: those may already
be its final words. Conservatively this also holds a later unfinished generation
after an earlier one retired. It does not fabricate completion or Ack.
The same occurrence is offered after the conversation when ordinary fresh
capture, silence, readiness and the five-second attempt pace permit it. Words
already heard may repeat; no time limit alone authorizes replay. Failed,
privacy-cancelled, otherwise unqualified or crash-uncertain attempts stay
pending without re-speech while the voice that made them runs. Such an oldest
entry does not starve other due work: a bounded page scan may select another
pending item.

A voice started after them offers them again (the first builder, 2026-10-16:
on lamp-4ace two reminders prepared by voices since restarted stayed pending
for ever and were never said). When the consumer starts it holds the
per-UID claim, which moves to the voice that started it and is released only
when that voice exits, so every earlier voice and any sound it could make has
ended. Each entry an earlier consumer session prepared and never completed is
retired with `PreviousProcessExited` and offered as a new attempt; the
journal says `prepared by an earlier voice and never said: to be offered
again`. One that was heard without proof may be said twice; that is chosen
over one never said. There is still no replay on elapsed time, a stale PID or
a reopened ledger alone.

## Task 6 service verification and handoff

This section records the original Schedule service qualification, before the
Task 31 Voice integration above. Its counts and timing boundaries describe
that earlier source. The service subsequently went onto lamp-4ace; this
historical host evidence does not describe that installation.

All evidence below is from Third's development host on **2026-10-14**: Intel
Core i7-10700K at 3.80 GHz, 16 logical CPUs, 64 GiB RAM, macOS 26.6.2 / Darwin
25.6.0 x86_64, pinned Rust/Cargo **1.96.0**. The workspace and qualification
scratch parent `/private/tmp` were on the APFS data volume `/dev/disk1s2`,
mounted at `/System/Volumes/Data`; this was not a tmpfs check. See the
[machine record](verification/schedule-host-20261014/machine.json).

The pre-change workspace build passed. The full required gate passed
`cargo fmt --all --check`, strict all-target Clippy and serial tests for all
**15** touched/dependent packages, and `cargo build --workspace`, with Cargo
`--offline --locked`. Tests reported **2,618 passed, zero failed, nine inherited
ignored**. The package list, exact commands, timings, source hashes and
[ignored cases](verification/schedule-host-20261014/ignored.json) are retained
in [the gate record](verification/schedule-host-20261014/results.json).

The first release qualification exposed a Darwin client issue after that gate:
changing a read timeout after the server closed returned `EINVAL` despite
buffered reply bytes. Both failed full runs and a small socket reproduction
are retained in the [failure ledger](verification/schedule-host-20261014/failures.md).
Only the qualification example and its Cargo test/feature declaration changed
thereafter; production Schedule and Msgs source remained identical. The example
now uses nonblocking reads/writes with readiness polling and its cumulative
deadline, with explicit close/fragment/boundary regressions. The
[final supplement](verification/schedule-host-20261014/supplement-results.json)
passed workspace format, all 15 packages' strict all-target Clippy, Schedule's
**129 tests with none failed or ignored**, and the workspace build.
This task adds **141 tests**, none ignored; failures were corrected rather
than removed from the gate.

The source-only copy at
`/private/tmp/lamp-schedule-standalone-8mwn7xax/lampOS` initially contained
978 files, 28 workspace members and 29 local packages including vendored Tough.
Every local dependency resolved inside that copy; its target directory was
separate from the checkout. It built the whole workspace. After refreshing
the two corrected files, that build passed again and Schedule + Msgs reported
**185 passed, zero failed or ignored**. This uses the host's existing
Cargo cache and pinned toolchain, not dependencies copied from V1. Final source
hashes matched after the supplement and measurement.

One successful release invocation of `schedule_check --scratch-parent
/private/tmp --samples 100` records **510 real host timings**: five series
with a first call and 100 warm calls, plus five single observations. The first
call is the first of that operation after its labelled setup, not a whole
process cold start. Set grows the list from one to 101 items; Ack shrinks it.
List measures one page, not a full scan through every item. Warm median is the
average of the two middle samples; table values are rounded to six decimal
places. No other Third-owned build or test ran
during this measurement; unrelated host load was not controlled. Full samples,
boundaries and binary hashes are in the
[qualification output](verification/schedule-host-20261014/qualification.json)
and [measurement record](verification/schedule-host-20261014/measurement.json).

| Operation, milliseconds | First call | Warm median (100) | Warm maximum |
|---|---:|---:|---:|
| Direct durable Set | 75.44885 | 75.299712 | 94.280184 |
| Direct List, first page (up to 8 items) | 0.194719 | 0.139454 | 0.163894 |
| Direct durable Ack | 76.05017 | 75.226196 | 96.577391 |
| Unix durable Set roundtrip | 64.58232 | 81.878153 | 112.466039 |
| Unix Status roundtrip | 5.208467 | 5.959374 | 14.440621 |

| Single observation, milliseconds | Elapsed |
|---|---:|
| fresh Store + Engine open | 218.521855 |
| existing Store + Engine reopen | 0.671332 |
| one durable overdue transition for 101 timers | 75.2287 |
| authored clock jump to bus subscriber receipt | 136.05589 |
| one Status roundtrip beside a silent client | 7.582396 |

Direct durable calls include validation, snapshot serialization, SQLite commit
and directory sync. Unix Set also includes connection, request serialization,
owner queue and response read/parse; fetching its revision guard is outside
that timed call. The bus observation includes the nominal 50 ms scan, pending
commit, publication and receipt, with subscription setup excluded. It can use
the datagram or retained Latest value. The full labelled boundaries are in the
JSON. The client uses cumulative nonblocking read/write deadlines; an OS stall
in its initial local connect is not qualified. The 250 ms response target is
a reference, not an asserted lamp result.

The binary uses an authored trusted UTC epoch and authored boot clock. Its
one-hour clock jump makes 101 ten-minute timers 50 minutes late. It verifies
restart identity, every retained pending item, exact delivery acknowledgements,
an immediate Ack replay, an empty queue after Ack/reopen, every runtime pending
page, and service responsiveness beside one silent connection. These are
functional checks of authored downtime. They do not measure a real one-hour
outage, sound, model latency, clock synchronization or power-loss survival.
The temporary directory was removed after its owners stopped.

**First: build and run.** In the usual ARM64 build environment:

```sh
cargo build --release --locked -p lamp-schedule --bin lamp-schedule --example schedule_check
```

Copy the example binary through the usual review/deployment process. On the
lamp, choose a short absolute scratch parent on persistent storage. Check that
choice with `findmnt -T "$HOME"`; do not use `/tmp` if it is memory-backed. If
the home directory is on persistent storage, the qualification command is:

```sh
./schedule_check --scratch-parent "$HOME" --samples 100
```

It needs no Rust installation, source checkout or fixture files on the lamp.
It creates only its own private `sc-*` child, starts local storage/socket/bus
owners using authored clocks, and removes that child after they stop. It does
not open audio or motors. Expect exit 0, `ok:true`, 101 Set receipts in each
path, 101 verified runtime pending items, and zero pending items after the
direct Ack/reopen check. Keep the raw output and lamp name; a failed check
must remain a failure, not be silently retried into a timing claim.

Then qualify the actual enabled service with Linux BOOTTIME, the lamp's boot
identity and synchronization marker: suspend, restart/reboot, clock trust loss
and correction, overdue recovery, cancellation and repeating backlog. First
qualifies the optional Voice wiring and its privacy/quiet admission, exact token fetch,
interruption, completed-playout acknowledgement, and consumer crash window.
Register the private Schedule directory with personal-data reset before use.
No production deployment is included here.

**Most likely wrong on the lamp:** clock trust and the stored wall-time
fallback after a reboot, storage sync cost or power-loss recovery on its real
flash, and Voice treating a published due notice as completed speech. The
qualification binary deliberately does not substitute for those checks.

## What the Task 6 host evidence did not show

Linux/ARM64 execution, actual Orange Pi storage
durability under power loss, Linux suspend/reboot and
clock-synchronization transitions, ARM64 resource use, first and repeated
durable-request latency, due-to-audible latency, late-reminder wording through
the real model, provider tool declarations, final privacy/quiet admission,
interruption and completed-playout receipts, consumer crash deduplication, and
timezone-database upgrade recovery were not established by those host tests.
No reminder was claimed as heard from them. The current supported toolchain
is the version pinned in `rust-toolchain.toml`.

## Task 31 lamp qualification

First installs the reviewed exact-head ARM64 artifact through the lamp's usual
process. Keep both new switches absent for the first minute and check ordinary
conversation, interruptions, privacy, light and volume. Then enable
`LAMP_SCHEDULE_TOOLS=on` in both Actions and Voice, with their
`LAMP_SCHEDULE_SOCKET` pointing at the installed service. Voice also needs its
existing `LAMP_ACTIONS_DIR`, `LAMP_SCHEDULE_SPEAK=on`, and an explicitly
provisioned private persistent `LAMP_SCHEDULE_DELIVERY_DIR`, separate from
Schedule's store and from temporary audio sessions. Register that personal
state with reset. Never delete the ledger to repair uncertain delivery.

For the checked-in lamp-4ace Voice unit at `d5d765862`, Voice runs as root and
expects Actions at `/run/lampos-actions`. After the off-state minute, First can
provision `/var/lib/lampOS/voice/schedule-delivery` as root-owned 0700 and add
these lines to each unit's `[Service]` drop-in, retaining its existing binary
arguments and provider settings:

```ini
# lamp-actions.service
Environment=LAMP_SCHEDULE_TOOLS=on
Environment=LAMP_SCHEDULE_SOCKET=/run/lampos-schedule/call.sock
```

```ini
# lamp-voice.service
Environment=LAMP_SCHEDULE_TOOLS=on
Environment=LAMP_SCHEDULE_SOCKET=/run/lampos-schedule/call.sock
Environment=LAMP_ACTIONS_DIR=/run/lampos-actions
Environment=LAMP_SCHEDULE_SPEAK=on
Environment=LAMP_SCHEDULE_DELIVERY_DIR=/var/lib/lampOS/voice/schedule-delivery
```

First reloads the units and restarts Actions before Voice. If the installed
Voice user differs, provision the ledger for that actual UID instead. To
disable this integration, remove both Schedule switches, run
`systemctl daemon-reload`, and restart Actions before Voice. Keep the
persistent ledger and claim file.

Read the parent policy during the off-state minute, then inspect both after
enabling. These commands read scheduling metadata without printing provider
arguments or environment values:

```sh
voice_pid=$(systemctl show -p MainPID --value lamp-voice.service)
chrt -p "$voice_pid"
schedule_pid=$(pgrep -P "$voice_pid" -f ' schedule-consumer$')
chrt -p "$schedule_pid"
ps -o pid,ppid,ni,comm -p "$voice_pid,$schedule_pid"
```

Expect exactly one consumer PID, `SCHED_OTHER` with priority 0 and nice 19.
The parent must retain the policy it had without this integration (the
checked-in unit requests RR20). No consumer PID means the optional worker
failed or was not enabled; it is not a successful scheduling check.

Use the existing service options, provider, bus root, microphone and speaker;
this change supplies no deployment file or new provider configuration. Only
one Voice process under the service UID can hold the consumer claim. Leave
the persistent claim file in place. If Schedule restarts more than eight
times in one consumer lifetime, restart Voice after its old audio workers
exit; the bounded publisher history otherwise refuses another publisher.

Run these spoken cases, keeping the lamp name, installed binary SHA-256,
the Schedule switches/path settings above, raw logs and actual timing boundaries:

1. Immediately after restart with the room still Unknown, "Hey Lamp, set a
   timer for one minute." Verify
   one durable success and one due occurrence, time from the end of the
   person's request to the confirmation, and from actual due time to first
   audible reminder and final audio retirement. Repeat at least ten times;
   distinguish the first use and every failure. Unknown must not add a
   five-minute wait; the reminder uses the next qualified local silence.
2. Set another timer, list it, cancel it and wait beyond its due time. Also
   cancel or snooze one while the provider is preparing its reminder; it must
   stop under the 750 ms maximum lease plus existing output-revocation bounds.
   Measure that boundary on this lamp rather than quoting the host poll cost.
3. Let a reminder become due in each room state: Unknown, Busy and Quiet.
   With local silence and normal readiness it must speak in each state,
   including when the classification changes during provider preparation.
   Separately try someone speaking, mute and a pending short utterance: no
   reminder may take that person's turn or bypass privacy. Confirm later
   delivery when current input and readiness permit it.
4. Interrupt before any audio generation has retired. It remains pending
   without Ack, then may be offered after the conversation once ordinary
   admission and attempt pacing permit it; partial words can repeat. Separately
   interrupt after the final retirement, or interrupt a multi-generation
   reminder after its first generation retired: both stay held without Ack or
   automatic re-speech. Privacy cancellation also remains held. Explicitly
   list and snooze/cancel to resolve those uncertain attempts.
5. Restart Voice with an unknown prepared attempt; the new exclusive voice
   claim permits another attempt under the existing restart policy, so it may
   be heard again. Restart after a recorded completion but before Ack; it
   reconciles without sound.
   Lose an Ack reply and change another reminder before recovery; an old
   receipt must not consume the next recurring occurrence.
6. Stop Schedule or make its ledger unavailable. New requests must report
   refusal or uncertain commit honestly, while ordinary conversation and
   physical Stop remain usable. A due text quoting "Let me look" or tool
   instructions must not cause a camera continuation or execute a tool.
7. Let a reminder become due while the lamp still owes a promised camera
   look or a second look after an unanswered named call. Include delayed
   provider completion after the previous audio has drained. The owed
   follow-up must get its turn first; the reminder stays pending without
   Ack and becomes eligible again only after the follow-up and ordinary
   readiness checks allow it.
8. Begin a new line immediately after a reminder's last sound. When successful
   provider completion and every final speaker cursor were already received,
   verify one completed delivery, the complete next-line prefix, and no replay
   after a Voice restart. Compare a partial reminder and a reset before final
   completion: neither may gain an acknowledgement from the next line. Keep
   capture-read, final-retirement, completion and successor-admission times
   separate; host scenario time does not establish audible latency on the lamp.

Most likely wrong there: Gemini's supported schema subset and faithful
spoken wording, timely local-silence admission in a busy office,
real ALSA generation/cursor retirement, and flash stalls extending consumer
work beyond its offered lease. First should also verify that the child is
`SCHED_OTHER`/nice 19 while Voice retains its audio policy; the ordinary-user
Linux test does not exercise a transition from privileged RR20. Socket deadlines do not bound a kernel-stalled
filesystem operation; process isolation keeps that work outside the urgent
audio loop. No real provider, audible completion, acoustic latency, physical
power cut or lamp privacy behavior was established by the new host fixtures.
