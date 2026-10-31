# Records within a bound

`lamp-records` keeps **recognized, closed voice service diagnostics** within
configured age and allocated-size limits. It is an independent timer job,
off in its built-in default and its example, and on in the product's file
("On a product lamp", below). It does not own conversation memory,
read transcripts, stop voice, or wait on the spoken path.

## What exists and what we take

V1 already bounds individual writers. At the task's source base
`96e5b83a7`, `system/lib/logger/logger.go` configures lumberjack with a
2 MB file and ten backups, but no age limit. HAL's
`server_support/log_setup.py` uses a 20 MiB server log with three backups
and 5 MiB provider usage logs with three backups. Those are the values in
the code, even where an older comment differs. `system/lib/flow/flow.go`
removes dated flow JSONL older than seven days at startup, without a total
byte budget. Keep writer-owned rotation and finite retention; add a byte
budget across completed service invocations and report failures.

For the process-exit repair in [task 30](https://github.com/autonomous-ai/lampOS/issues/707),
V1's same writer-owned rotation and dated-flow cleanup were inspected again;
they contain no equivalent cross-process session guard to port. The relevant
Linux 5.15 sources are [`fs/proc/base.c`](https://github.com/torvalds/linux/blob/v5.15/fs/proc/base.c)
(`proc_pid_cmdline_read`, `get_task_cmdline`),
[`kernel/exit.c`](https://github.com/torvalds/linux/blob/v5.15/kernel/exit.c)
(`do_exit`), [`fs/proc/array.c`](https://github.com/torvalds/linux/blob/v5.15/fs/proc/array.c)
(`do_task_stat`) and [`include/linux/sched.h`](https://github.com/torvalds/linux/blob/v5.15/include/linux/sched.h).
They distinguish a vanished task (`ESRCH`), a released address space (empty
command line), `PF_EXITING` (`0x00000004`) and `PF_KTHREAD` (`0x00200000`).
Keep the existing descriptor-pinned procfs guard and known kernel-thread
exclusion; add one bounded exit observation after an incomplete inspection.
An empty command line by itself is never proof of exit.

Maintained alternatives were inspected on 2026-10-14:

- [systemd-tmpfiles](https://github.com/systemd/systemd/blob/v257/man/systemd-tmpfiles.xml)
  supplies age cleanup, dry-run facilities and locking conventions. Existing
  voice writers do not hold its locks. A general pathname/age rule cannot
  establish this lamp's live output ownership or recognize its record layout.
- [logrotate](https://github.com/logrotate/logrotate/blob/3.22.0/logrotate.8.in)
  is suitable for cooperating file writers. Voice already rotates four
  retained descriptors. Do not move or truncate those live descriptors, or
  substitute file rotation for a bound across invocation directories.
- [journald](https://github.com/systemd/systemd/blob/v257/man/journald.conf.xml)
  already owns journal rotation and size limits. Keep that owner; this tool
  never unlinks a journal file. See the coverage limits below.
- Reuse the workspace's **rustix 1.1.5**, the existing bus and
  `lamp_msgs/Diagnostic`. No new C library, shell cleanup script, daemon
  dependency, model, account or network service is introduced. Linux uses
  [openat2](https://man7.org/linux/man-pages/man2/openat2.2.html), descriptor
  relative operations and no-replace rename. The selection rule is small,
  deterministic Rust with injected time, sizes and protected session IDs.

## Ownership and recognized layout

The adapter kind is `voice_service_metadata`. The root must be an explicit,
absolute, trusted directory named `sessions`; it is never created by a scan.
The known layout is:

```text
sessions/
  <32 lowercase hexadecimal invocation characters>/
    metadata/
      events-0.jsonl
      events-1.jsonl
      events-2.jsonl
      events-3.jsonl
      health.json
```

A partial set of these files is possible after an interrupted start. An
unexpected child, link, hard-linked file, special file, wrong owner or wrong
permission makes that candidate ineligible. Record contents are not read.
Unknown folders are preserved and listed as refused, so a successful cleanup
is not a claim that every byte beneath the root was understood. Current
source does not establish the format of all 612 historical lamp directories.
The first dry run must expose those differences rather than erase them.

Accepted directories are private (0700); files are private (0600) and have
one link. The configured root and ancestors must have trusted ownership and
permissions. Ancestor links, traversal components and overlapping roots are
refused. A protected sticky temporary ancestor is allowed for private test
fixtures. Descendant mount crossings, including bind mounts, are refused on
Linux. Missing or blocked `openat2` is a refusal, not a weaker fallback.

### Optional session-reader group (#2789)

An office installation can give a dedicated reader access to service events
without giving it the voice's account or other private state. Set
`session_read_group` on the particular `voice_service_metadata` root in
`records.json`, matching Voice's `LAMP_SESSION_READ_GROUP`. Omission or JSON
`null` keeps the private policy. This setting does not enable retention or
change its limits. Its name participates in the configuration fingerprint;
an omitted policy preserves the historical private-policy fingerprint.

The name is 1–128 ASCII characters, starts with a letter and otherwise uses
letters, digits, `_` or `-`. Records resolves one unique exact entry from the
local `/etc/group` once before opening that root. The read is capped at 1 MiB
and refuses a link, special file, duplicate name, invalid ID or read failure.
There is no NSS/network lookup or numeric-ID setting. A failed lookup refuses
that root visibly; it never falls back to another group.

| Object | Private policy/history | Explicit reader policy |
|---|---|---|
| Sessions root, invocation directory, `metadata/` | 0700 | 0750 with the pinned reader GID, or private historical 0700 |
| `events-0.jsonl` through `events-3.jsonl` | 0600 | 0640 with the pinned reader GID, or private historical 0600 |
| `health.json` | 0600 | 0600 |
| Records quarantine | 0700 | 0700 |

Owner, file link count, filesystem, exact mode and captured identity checks
still apply. With a reader policy, any access or default ACL is refused,
including one on private historical entries or quarantine. The ACL check
uses retained descriptors and is repeated with the existing identity and
layout checks before deletion. A platform without the Linux ACL verifier
refuses the opt-in policy. With no reader policy, the existing private-only
behavior is unchanged. Records neither reads event contents nor changes
permissions or ownership.

Shared modes alone do not establish that the contents are safe for a reader.
Voice's successful-grant path writes the fixed, flat `shared_metadata: 1`
projection described in [Peek](peek.md#session-reader-access): approved turn
kinds/statuses and unsigned counters/timings, with other records reduced to
a fixed generic event. Transcripts, nested payloads, paths and identities are
excluded at serialization, not merely ignored by Peek. Records does not
parse, certify or redact those contents; its retention decisions remain
based on the same pinned filesystem identities. Private and historical
trace formats and the private health schema are unchanged. Previously
shared files are not rewritten by this change.

Install compatible Records before enabling Voice's producer grant. Older
readers can ignore this additive field but still reject shared modes; they
must not be described as maintaining the new layout. Voice grants access
only to a new service invocation and its retained event descriptors. Existing
sessions are not widened, and rotation does not reopen or re-grant files.
The dedicated account, narrowly prepared parent traversal, a bus view that
excludes raw speech before delivery, and the authorized install/restart test
remain separate from this retention policy. Subscriber-only access alone
cannot protect payloads already delivered; see [Peek](peek.md#session-reader-access).

### Which session is live

A directory's age, name, `writer_finished` field or an absent main PID is
not a proof that its producer stopped. The deployed unit is root-owned
`Type=exec` and passes `sessions/${INVOCATION_ID}`. systemd can retain older
processes through a failed stop or restart; the manager's current invocation
alone is insufficient. See the pinned
[service implementation](https://github.com/systemd/systemd/blob/v255/src/core/service.c)
and [execution environment contract](https://github.com/systemd/systemd/blob/v255/man/systemd.exec.xml).

The cleanup pass first fixes a snapshot of existing, recognized directories.
Only then does it establish live-process protection. The Linux guard queries
the explicitly named system unit, validates its loaded command and output
mapping, protects its invocation, and inspects host processes for current,
leftover and manually launched voice writers. PID identities and bounded
manager/process observations are checked for change. Ambiguity, missing
visibility, a changed unit, unknown output arguments or an unavailable manager
refuses the entire root. No producer or service process is signaled or frozen.
The loaded command's binary is resolved through its links before it is
compared with each process's `exe`, which the kernel gives resolved. A
product unit names `/opt/lampOS/voice/current/lamp-voice`, and its main
process's `exe` is the build that link points to. Compared as written, they
disagreed, and every run on a product lamp refused ("main process disagrees
with manager invocation mapping"; Twelve on lamp-cb94, 17 October). A link
switched to another build while the old one still runs disagrees again and
refuses (`a_voice_started_through_a_current_link_is_its_own_main_process`).
The guard kills and reaps its own query child on a timeout or query failure.
The voice's known processes, its commands and worker roles, come from the
voice's own list, `lamp-voice-processes` ("The voice's own list" below). The
guard keeps no copy: a copy missing `second` refused every hourly pass on
lamp-4ace from 15 Oct 14:01.

This relies on the **current trusted producer contract**:
`crates/voice/src/coordinator.rs::run_source` exclusively creates `NEW_OUTPUT`
before it starts logging or workers. It cannot resume an existing candidate.
A later start therefore cannot acquire a directory in the fixed inventory.
New directories created during a pass wait until the next pass. Active IDs
are protected even if older or larger than every configured limit. A future
writer that reopens existing session directories needs a new ownership adapter.

The real guard requires root, systemd and the host's complete procfs in the
host PID namespace. It is not a container escape detector: an installation
must give it the actual host view, not a systemd container's private world.
The shipped unit does not hide processes or create a private PID namespace.
On macOS, the real guard refuses; tests supply made-up ownership evidence.

### Processes that end during inspection

An unrelated process ending during a scan need not refuse the entire cleanup.
The guard distinguishes these positive observations from a live process whose
metadata cannot be inspected:

- A PID directory cannot be opened because it is gone (`ENOENT` or `ESRCH`),
  or the **pinned process's `stat`** returns either error: record `gone`.
  A missing `exe`, `cwd`, `cmdline` or `fd` entry alone is insufficient.
- `stat` says `Z` (zombie) or `X` (dead), with exactly one thread: record
  `zombie` before attempting to read arguments. A terminated group leader
  with other threads still present refuses; its workers may own live records.
- An inspection fails: read the pinned process's `stat` once more. The PID,
  start time and process kind must still match. It may now be gone or a
  single-thread zombie, or have `PF_EXITING` set, exactly one thread and a
  newly read **empty** `cmdline`. The last case records `exiting`; an `ESRCH`
  from that final command-line read records `gone`. A flag alone, stopped or
  blocked state, unreadable metadata or a changed identity still refuses.
- Linux may report zero threads when reaping races with `stat`'s signal-state
  lock. Zero is ambiguous, so it uses that same single follow-up observation;
  only positive gone/zombie/exiting evidence passes. Persistent zero, a
  malformed follow-up or a live follow-up still refuses.
- The existing positive `PF_KTHREAD` check remains a separate
  `kernel_threads` observation. Kernel threads are not reported as dead, and
  an empty command line cannot establish that flag. This explicit exclusion
  is checked before thread count or zombie state, so even a dying kernel
  thread stays out of the exit examples. A manager main PID
  classified as gone, dead, exiting or a kernel thread always refuses the root.

- **A process still live and not inspectable after that** is looked at
  again, from its `stat`, up to three more times, 20 ms apart
  (`SETTLE_LOOKS`, `SETTLE_WAIT`):
  - gone, a zombie or exiting by then, it is skipped, as on a first look;
  - readable by then, it is inspected as any live process, and what it
    holds is protected;
  - still live and not inspectable after the last look, it refuses the
    root, as before.

  A process ending with more than one thread, whose leader has let go of
  its memory, reads an empty `cmdline` for a few milliseconds, as does one
  between `execve` and its new arguments. On lamp-4ace on 17 October such a
  process failed every run (`live process has no inspectable arguments`,
  beside `gone: 1, zombie: 2`), so the voice's sessions were never trimmed
  and would have grown past the limit (the first builder on #705, 15:10).

  Tests, in `guard.rs`, with each wait scripted:
  - `unreadable_at_first_and_gone_a_zombie_or_exiting_on_a_later_look_is_skipped`;
  - `unreadable_at_first_and_readable_on_a_later_look_is_inspected_as_live`;
  - `still_live_and_unreadable_after_every_look_refuses`.

  The first two fail with a single look.

Each failed inspection costs at most one extra `stat` and `cmdline` read per
look, and at most 60 ms of waiting, under the same ten-second work budget.
Kernel I/O still cannot be preempted by that budget.
Paths already protected before an exit remain protected, and both complete
process passes retain their union. A live file holder remains protected.

Each root's JSON result may now include `processes`, with `gone`, `zombie`,
`exiting` and `kernel_threads` counts, plus at most 16 `examples` of
`{"pid":42,"reason":"gone"}` (also `zombie` or `exiting`). Examples exclude
kernel threads so they cannot displace exit evidence. These are observations,
not unique-process totals: the same zombie can appear in both passes. Only
the first 16 exit observations are sampled; counts cover all observations,
including those made before a later root failure. A live unreadable process
still appears in `error` with its PID, and no deletion plan is made.

The empty `processes` object is omitted, and old receipts without this field
remain readable. The total observation bound is 131,072 (two passes of at
most 65,536 processes); receipt reading bounds the example list before
allocating extra entries. These details stay in stdout and the private
receipt, not the one-second bus diagnostic. Report readers tolerate added
fields, including inside these observations; unknown reason values are still
refused. The supplied `enabled:false` default and the requirement for explicit
`--apply` are unchanged.

### Removal and recovery

Only the fixed inventory can be selected. Before mutation, the store reopens
and validates all identities, allocated sizes, modification/change times and
the complete shallow layout. A changed or disappeared candidate is refused;
it is never recreated or chased under a replacement name.

Apply serializes cleaners and moves a verified candidate into private
`.lamp-records-trash/<original-id>` **under that same root**, using no-replace
rename. It then removes only the known entries through directory descriptors.
A crash can leave a recognized remainder in that directory; the next pass
checks and finishes that remainder with the same live protection. Unknown
trash, duplicate identities, failed sync and partial removal are reported.
Creating quarantine, renaming or syncing can fail on a full filesystem. There
is no fallback that bypasses the checks.

This is confinement under a trusted namespace, not protection against a
malicious process with the same UID/root privileges or a mount administrator.
An open directory descriptor follows its object if a privileged process moves
it elsewhere; Unix rename does not compare an expected inode atomically.
Normal producers must exclusively create their own new directories and never
rename, reuse or repopulate closed sessions. The private quarantine separates
cleanup from normal creation. The tool refuses observed replacement and never
follows links; it cannot revoke root's ability to mutate its namespace.

## Configuration, bounds and defaults

[records.example.json](../crates/records/records.example.json) is the complete
customer proposal. `lamp-records defaults` prints the same configuration.

**On a product lamp,** `lamp-update install` places
`/etc/lampOS/records.json` from
[deploy/product/settings/records.json](../deploy/product/settings/records.json),
as lamp-4ace has run since 14 October: on, the voice's sessions bounded at
2 GB and 14 days. Before 17 October install placed none, so the timer ran
nothing (`ConditionPathExists`), and a customer's sessions would have grown
without end (Twelve on lamp-cb94). A file already on the lamp that isn't
install's own is kept, as any setting is. The built-in default below stays
off and at 500 MB; the product file is what turns it on.
There are no environment overrides for retention, no implicit roots and no
CLI option to bypass live protection. An unknown `kind` is an error, so it
can never become a path.

**A newer build's `records.json`** (#1680, `docs/updates.md`, "Every
part's saved data, one version ahead"), since install writes it and a
put-back may leave one behind:
- At this build's `schema_version`, a field it doesn't know, at any depth,
  is read past and named once a run, by its path, never its value:
  `lamp-records: /etc/lampOS/records.json: fields this build does not
  read, ignored: roots[0].min_free_bytes`. The cleanup runs as before, and
  the fingerprint is this build's fields'.
- A greater `schema_version` is told apart before anything else is read.
  `check` prunes nothing, writes no result, says so in one line and
  succeeds, so the timer's run doesn't fail: what a bump means isn't this
  build's to guess, and a cleanup deletes. The reporter keeps reporting,
  at warn, on the bus the newer file names: lamp-health shows "records: a
  newer schema, cleanup paused" (`report::paused`, with `config_schema`), so
  a lamp left rolled back shows why its disk grows. It reports as before
  once this build's schema is back
  (`a_newer_schema_is_reported_as_the_cleanup_paused_until_this_builds_is_back`).
  A newer file naming no bus it can take stops the reporter at start, with
  nowhere to say it. Where the reporter isn't running (it is still a
  template, not a product unit, below), the journal line is where this
  shows.
- Still refused: a file that doesn't parse, a known field's bad value (a
  `kind` it doesn't know), and a field named like a key's, by its name.
  serde's reason is told only as its category, line and column, never its
  message.
- Tests: `config::tests::the_products_config_one_release_ahead_is_read_or_told_newer`,
  `real_cli_runs_on_a_newer_builds_config_one_release_ahead`.

```json
{
  "schema_version": 1,
  "enabled": false,
  "bus_root": "/run/lampos-bus",
  "roots": [{
    "kind": "voice_service_metadata",
    "path": "/var/lib/lampOS/voice/sessions",
    "unit": "lamp-voice.service",
    "max_bytes": 500000000,
    "max_age_seconds": 1209600
  }]
}
```

**500,000,000 bytes and 14 days** bound troubleshooting history without
promising a two-week archive. At the first builder's reported 0.4 GB/day,
500 MB represents roughly 1.25 days: size wins well before age. That is a
calculation from the reported rate, not a new lamp measurement. Oldest means
the newest mtime among a session's owned directories and records, with the
ID as a deterministic tie-breaker. A session expires when its age is strictly
greater than the limit. After age removals, further oldest closed sessions
are removed until the recognized total fits. Future timestamps are refused.

Size is allocated `st_blocks * 512`, including the owned directories. It is
neither logical file length nor a promise of bytes physically reclaimed.
Open descriptors, snapshots, compression and concurrent writers can affect
actual free space. The report separately measures `fstatvfs` total capacity
and space available to ordinary writers, excluding reserves.

Live records count toward the budget but cannot be removed. Unrecognized
records are not silently counted as zero total disk use: refusals make the
result degraded and filesystem fullness remains separately visible. An
hourly timer is a periodic bound, not an instantaneous global quota. Growth
between runs, one large live session, unsupported layouts and failures can
leave the budget exceeded. The cleaner reports those limits instead of
relaxing live-session protection.

At most four distinct roots and a fixed shallow layout are accepted. The
16,384-entry limit per root includes unknown/refused names and entries inside
quarantine, not just recognized sessions. Retention limits must be positive;
a root cap is at most 1 TB and an age at most 366 days. The live guard has a
ten-second budget and each manager query a two-second deadline. A cleanup
pass has a 45-second work budget checked between operations; the unit has a
60-second start timeout. Kernel filesystem calls are not preemptible deadlines.

The configuration file is limited to 65,536 bytes (64 KiB); the stored result
is limited to 8 MiB. Result deserialization accepts schema version 1 and modes
`apply` or `dry_run`, with at most four roots and 16,384 entries in each
`active`, `planned`, `removed` or `refused` list. Each list entry is at most
1,024 decoded UTF-8 bytes; paths and error strings are at most 4,096 bytes,
and unit names at most 128 bytes. Arrays are checked as they are read, before
an extra element is allocated. An oversized or invalid result cannot be
stored as a normal receipt. The CLI preserves detailed outcomes on stdout
and attempts a small failure receipt instead. If that also fails, the prior
complete receipt and its original timestamp remain; failure is still printed
and returned. A regression verifies that the next cleanup can replace the
small failure receipt.

### Persisted reports after a rollback

Within schema version 1, readers ignore fields they do not know on `RunReport`,
`RootReport`, `Disk`, `ProcessObservations` and `ProcessSkip`. Existing reports,
including roots without `processes`, still read the same way. A save writes only
the current build's fields, so fields added by a later build are not retained
in its replacement. Required fields, known-field types, bounds and configuration
fingerprint matching remain enforced. `ProcessSkipReason` still accepts only
`gone`, `zombie` and `exiting`; unknown reason strings and unknown tagged values
are refused, as is a `mode` that isn't a plain word. A plain new `mode` at
this schema is a newer build's, and is set aside (below). Fixture and wire
readers stay strict; the configuration reads one release forward (above).

A future change that adds a required field, enum value or changes a field's
meaning must bump `REPORT_SCHEMA_VERSION`; an optional field can keep it,
unless it makes the result worse than its known fields show, such as a skipped
root or a partial count. That field needs a bump too: an older reporter ignores
it and would show that report as ok.
When `check --apply --report` or an enabled reporter reads a greater schema
version, it retains the whole file as `last.json.newer-<schema>-<unix s>` beside
`last.json` (using the actual result filename). The journal records that once
after a successful rename. Recognition still requires valid JSON within 8 MiB
and a structurally recognizable envelope: an unsigned 64-bit schema, valid
fingerprint, unsigned timestamp, a mode string of at most 128 decoded UTF-8
bytes, boolean `enabled`, and at most four roots. Future mode values and the
contents of future roots are not interpreted or trusted as cleanup evidence.
An absent or invalid envelope is refused and remains in place.

At the supported schema, a `mode` other than `apply` or `dry_run` that is a
plain word (lowercase letters and `_`, at most 32), in a report whose every
other field reads as this build's, is a newer build's (#1680): it is
retained whole the same way, as `last.json.newer-mode-<unix s>`. The
journal says "cleanup result has a mode this build does not know, at its
schema 1", never the mode, and the cleanup runs. Before, every run was
refused until someone removed the file. Any other mode, or a plain new one
beside a field that doesn't read, is still refused and left in place
(`a_newer_builds_mode_at_this_schema_is_set_aside_whole_on_read_and_write`).

Archival uses the writer's private-directory lock, checks the source identity
again, refuses to overwrite an existing archive and syncs the parent directory.
A changed source, unsafe file, unavailable lock or archival failure does not
permit the report to be used or overwritten. The writer repeats this check
before saving, so a newer report installed after cleanup preflight is retained
too. Unknown values in the supported schema and malformed files remain intact;
the failure-receipt fallback cannot replace them. An archive collision refuses
that attempt and leaves both files intact.

The reporter discards cached evidence after archival and reports the ordinary
missing-result error. It creates no replacement file. Only a subsequent cleanup
can produce a current receipt; `check` archives before cleanup, then follows
the usual enablement and live-process safeguards. Disabled reporting still
needs no receipt and does not archive one. This policy helps the next release
roll back to this reader; it cannot change already-installed older readers.

## Commands and diagnostics

```text
lamp-records defaults
lamp-records check /etc/lampOS/records.json
lamp-records check /etc/lampOS/records.json --apply --report /run/lamp-records/last.json
lamp-records report /etc/lampOS/records.json /run/lamp-records/last.json
```

`check` is a dry run unless `--apply` is explicit. Actual removal additionally
requires `enabled: true`; a mistakenly enabled timer with the sample file
cannot remove anything. Dry runs scan even when the switch is false so they
can show the proposed result. They write one JSON object to stdout and do
not create a quarantine, report file, publisher or persistent lock file.
Linux directory reads use `O_NOATIME`; macOS testing does not establish Linux
atime behavior. Shell redirection, if the operator chooses it, is the caller's
write. `--report` is rejected on a dry run.

The JSON distinguishes `active`, `planned`, fully `removed`, refused records,
allocated bytes actually unlinked, bytes remaining from the fixed inventory,
and measured filesystem space. `remaining_bytes` accounts for completed or
partial removals against that inventory; it is not a fresh tree measurement
and does not include later live growth. In a dry run it remains `before_bytes`;
projected recognized size is `before_bytes - planned_bytes`. A partial removal
is reported as partial and is not counted as a fully removed session. A
vanished candidate is a refusal, not a fabricated success. Errors preserve
available disk information.

Every result carries `config_fingerprint`: 64 lowercase hexadecimal characters
containing SHA-256 of the serialized parsed configuration. The reporter compares
it with the current configuration, so changing enablement, roots, limits, age
policy or bus path cannot turn an old result into current success. JSON
whitespace is not part of this identity. The fingerprint identifies settings;
it does not authenticate a file against a privileged writer.

Exit codes: 0 is a completed check/apply (or disabled apply), 1 is an
unresolved refusal/budget/full-disk condition, 2 is a failed cleanup root or
report write, and 64 is invalid arguments/configuration or command startup.
When no earlier failure applies, exit 1 detects an apply still over its
recognized byte limit, zero available bytes, or missing/zero disk capacity.
The 3% and 10% health thresholds below do not change the cleanup exit code.
No exit code establishes that all historical formats or the whole disk are
covered.

The cleanup job does not publish a one-shot health event. `lamp-health`
marks every previously seen part stale after five seconds, even if it is not
in `--expect`. A separate `report` process publishes `diagnostics` as
`name=records` every second, using the bounded atomic result under `/run`.
It does not rescan the record tree. It reports the last cleanup's age,
counts and byte totals, and explicitly labels disk space as measured at that
cleanup. An existing valid result older than two hours becomes stale; existing
worse warnings/errors survive staleness. A missing, unreadable, malformed or
wrong-configuration result is an error immediately, as is a future result
timestamp. Refusals/over-budget are warnings; unknown disk or less than 3%
available is an error; less than 10% available is a warning. When the current
configuration disables cleanup, the reporter immediately says it is disabled
and makes no disk measurement claim, without needing a saved result.
IDs, paths, error details and transcript contents are excluded from the
one-second diagnostic; the private result and stdout retain session IDs.
One detail is the exception: after two cleanups in a row refused for a
voice worker the guard does not know, the message names it, as
`records refused: unknown voice worker `second``, and only when the role is
a plain word (see "The voice's own list" below).
A stopped reporter itself becomes stale normally. A missing bus does not
block cleanup or any other component.

### Explicit reporter cadence for local fixtures

The ordinary executable and the explicit fixture share the private
application body. Ordinary startup still supplies the real reporting function;
the parser, initial config read/validation, cleanup commands, file reloads,
publication, error handling and exit codes retain their existing order. The
original main-binary tests remain at the same paths. Ordinary execution has no
fixture environment switch or control protocol.

The reporting loop accepts an explicit schedule with pure, nondecreasing
`Instant` reads and a wait callback, plus a publisher factory selected only at
the existing construction/reconstruction site. The system schedule still reads
`Instant::now` at each cycle start and residual calculation, then calls
`thread::sleep` once with the original one-second period minus work time,
including a zero residual. A supplied wait sees the actual combined publisher
construction/publication result after error handling and the host lifetime
check. Construction failure does not claim a publication attempt; a returned
success does not prove that a subscriber received an Event, or that a child
exited. A fixture's wait must preserve full periods and separately enforce the
supplied real host lifetime; zero residual is not a parked boundary.

`Reporter::new_with_cadence_clock` installs an explicit pure Bus cadence source
before its first declared presence refresh. It retains the Bus's 200 ms
subscriber rescan and one-second presence cadence. `Reporter::new` keeps its
ordinary clock and declaration-before-start ordering. The reporter's own
monotonic header timestamp remains real, as do receipt/diagnostic wall time,
Bus UTC and filesystem lease timestamps, actual atomic files and datagrams,
the 60-second error-log throttle, and the independent `--seconds` host lifetime.
The application schedule and publisher cadence are explicit separate inputs;
a coordinated fixture must supply the same clock domain to both. Clock reads
never advance it. No clock replacement reaches retention guards, cleanup,
subscriber liveness or the operating system's child-status evidence.

`lamp-records-fixture` is a separate local test executable. It accepts the same
report arguments with an explicit bounded `--seconds`, and takes versioned,
nonce/round-checked control frames only through private inherited stdin/stdout
pipes. Frames are at most 4096 bytes; nonblocking reads and writes share an
absolute host deadline. Its pure clock is shared with the Bus cadence source.
The first settle at logical zero is acknowledged only after the real first
iteration reaches a positive wait. Same-time and pre-due steps hold that wait;
an exact due step releases it but receives no acknowledgement until the next
actual reporting iteration reaches its positive wait. Periods remain one
second. Invalid frames, time or receipts permanently fail the fixture.

The reload integration case retains its six actual Event diagnostics, original
file/fingerprint transitions and all original assertions. It drains the real
subscriber queue before each due step and requires an actual matching datagram;
the control reply reports the combined construction/publication outcome only.
Each status search keeps its original four-second absolute host deadline and
one four-second logical window. Its child keeps `--seconds 30` in real host
time, including when blocked on control. A blocked-control lifetime timeout
uses the existing error exit 64; it does not fabricate a normal successful
exit. Real `try_wait`, kill and wait remain authoritative on success, setup
failure and panic. Kernel filesystem operations, inherited stderr and actual
reaping do not acquire a universal hard return guarantee from these deadlines.
No background reader, observer worker, stderr marker or synthetic Event is
involved, and no runtime speedup is claimed without paired execution evidence.

The saved result under `/run` is lost at reboot. With cleanup enabled, the
reporter can therefore show a missing-result error until the next hourly
cleanup completes. The timer's `Persistent=true` only catches missed calendar
ticks; it does not guarantee a cleanup on every boot. The timer also has a
60-second randomized delay and one-minute accuracy window. See the
[systemd timer contract](https://raw.githubusercontent.com/systemd/systemd/v257/man/systemd.timer.xml).

## Coverage beyond voice

| Writer | Current ownership | This handback |
|---|---|---|
| Voice `serve` metadata | Four 4 MiB segments plus health per invocation | Recognized closed invocation directories, 500 MB / 14 days by default proposal |
| `lamp-bus record` | Caller chooses any directory and topics; recorder may append; 64 MiB default per-topic file cap | Do not infer that a quiet directory is closed. Use finite capture duration and explicit per-file bounds; unmanaged capture directories are not eligible roots |
| system journal | journald owns shared active/archived files | Keep native rotation. Do not vacuum the whole system on behalf of one lamp unit |
| Policy daily JSONL | Policy retains a dated file descriptor until another write changes the day | Yesterday is not proof it is closed; no policy adapter is claimed |
| Memory, conversation thread, model/config files, finite experiments | Their respective owners and retention/privacy rules | Always outside this tool's supported kind |

Upstream journald defaults already impose a size policy: maximum use 10% and
keep-free 15%, each capped at 4 GiB; active files can exceed the archived-file
vacuum target. The installed lamp's settings have not been inspected here.
For a customer image, propose a **dedicated lamp journal namespace** with
250 MiB persistent use, 64 MiB volatile use, 14-day retention and a 500 MiB
keep-free target, installed by the image/service owner. That requires an
explicit namespace on participating units; changing the shared system journal
or pretending a per-unit query isolates deletion would be wrong. These are
proposed settings, not installed or enforced by this crate. The native
[journal configuration contract](https://github.com/systemd/systemd/blob/v257/man/journald.conf.xml)
explains the active-file exception.

There is no known production root or completion marker for bus captures.
First must assign one before a safe automatic directory adapter can cover
them. A file cap is not a lifetime cap across repeated captures. This crate
solves the known accumulating voice-session root and exposes its exceptions;
it does not claim a global years-long bound for every current diagnostic
writer. Memory retention remains entirely the memory component's business.

## Unit templates and first lamp check

The three templates are beside the crate under
[systemd/](../crates/records/systemd/): an hourly timer, a one-shot cleanup
service, and the persistent diagnostic reporter. The timer and the cleanup
service are now product units ([deploy/product/](../deploy/product/README.md):
`lamp-records.service` and `lamp-records.timer`, hourly, outside the target),
run as `/opt/lampOS/records/current/lamp-records` with
`/etc/lampOS/records.json`, which the product's settings set to
`enabled: true` and `max_bytes` 2,000,000,000
(`deploy/product/settings/records.json`). The reporter is still only a
template (below). The rest of this section is the first lamp check as it was
written before those units, kept as its procedure.

For that check: build the crate for the qualified Linux ARM64 release. The templates use
`/opt/lampOS/bin/lamp-records`; First chooses and installs the versioned binary
behind that path, installs the sample as `/etc/lampOS/records.json`, and
keeps `enabled` false. Adapt `ReadWritePaths` if a root or bus path changes.
Do not add `ProtectProc`, a private PID namespace, or settings that hide
another process's descriptors: unknown protection must stop deletion.

The one command to run on the lamp, with that binary/config installed, is:

```sh
sudo /opt/lampOS/bin/lamp-records check /etc/lampOS/records.json
```

Expect JSON with `mode:"dry_run"`, `enabled:false`, one voice root, the
running invocation in `active`, zero `removed_bytes`, empty `removed`, and
oldest eligible IDs in `planned` if the recognized budget/age is exceeded.
The current invocation must never occur in `planned`, even if it is very
old. `disk` must contain nonzero `total_bytes` and a plausible
`available_bytes`; no exact current percentage or removal count is asserted
from this Mac. Historical layouts or a guard incompatibility must appear in
`refused`/`error`, and an incomplete guard must leave `planned` empty. Those
are actionable refusals, not permission to weaken protection.

After First verifies the live IDs, loaded command format and recognized
history, opt-in installation creates private `/run/lamp-records` and a private
`.lamp-records-trash` under each root (preparation avoids requiring a first
mkdir at complete disk exhaustion), installs the three units, and enables
the reporter and timer. Actual cleanup starts only after setting `enabled`
true. No voice restart is needed to enable retention. Check the first apply's
JSON, the untouched active folder, and the subsequent `records` diagnostic.
Interrupt an apply on a disposable fixture and verify recovery before using
real archives. Never test disk exhaustion by filling the lamp's real disk.

## Verification and latency

This process has no synchronous path from microphone, button or touch to
feedback. The design target is zero added waiting in those paths, and a
1,000-session inventory under one second on the development Mac. The fixture
measurement below meets the inventory target on that machine. The first
builder must still compare conversation latency/drop counters with and without
an active cleanup, because filesystem and CPU contention are not proven
harmless by process separation.

### Measured development-Mac cost, 2026-10-14

One release test run used macOS 26.6.2 (25G83), x86_64, an Intel Core
i7-10700K at 3.80 GHz, 16 logical CPUs and 64 GiB RAM. Run the same fixture
from `lampOS/` with:

```sh
cargo test -p lamp-records --release --test records records_cost -- --ignored --nocapture
```

The [fixture](../crates/records/tests/records.rs) creates 1,000 disposable
sessions, each with five files containing 1,024 logical bytes. Cache was not
evicted, including before the first inventory. Fixture creation and opening
the store are outside the timed intervals. The first inventory took
**153.317235 ms**; the subsequent measured distributions were:

| Operation | Samples | Median (ms) | p95 (ms) | Worst (ms) |
|---|---:|---:|---:|---:|
| Inventory of all 1,000 sessions | 30 | 148.485177 | 158.1607 | 170.073146 |
| Pure retention plan for those sessions | 30 | 0.172239 | 0.192564 | 0.286491 |
| Removal of one session, including filesystem syncs | 30 | 2.045649 | 2.15702 | 2.188294 |

For these 30-sample distributions, the fixture reports the upper middle
sample as the median and the nearest-rank p95 (sample 29). The 30 removals
unlinked **614,400 allocated bytes**; that is unlink accounting, not a measured
increase in free capacity. The live set was injected and the real guard was
not timed. These measurements do not cover a full systemd/procfs-protected
cleanup, Linux ARM64 execution, the lamp's storage or concurrent conversation
latency. Only disposable fixture records were removed.

### Functional gate, 2026-10-14

On the same Mac with Rust/Cargo 1.96.0, the final gate passed:

```sh
cargo fmt --all --check
cargo clippy -p lamp-records --all-targets --offline -- -D warnings
cargo test -p lamp-records --offline
cargo build --workspace --offline
```

There were **96 passing tests** (87 library, five CLI, four integration),
zero failures and one normally ignored release timing test, run separately
above. No workspace crate depended on `lamp-records` then; `lamp-settings`
does now (`crates/settings/Cargo.toml`). The workspace also built
before this task. Socket tests required the local sandbox's bind restriction
to be lifted; they did not contact a device.

A 704-file source-only copy outside the parent repository built and passed
**97 tests** under `cargo test -p lamp-records --offline --locked --target
x86_64-unknown-linux-gnu`, inside a disposable offline container. The local
VM runs Linux 6.12.13-200.fc41.x86_64 with six configured CPUs and 8 GiB RAM;
its name `lampos-arm64` is not its processor architecture. Only source and
public dependency caches were mounted, read-only. Linux adds FIFO and atime
checks and omits the Mac unsupported-platform test. The two normal ignores
were the separately run timing test and a mount test. The latter passed
separately in an isolated mount namespace with `CAP_SYS_ADMIN`: both a file
and directory bind mount on the same filesystem were refused without
touching the outside sentinel.

The Linux ARM64 source check also passed:

```sh
cargo +stable clippy -p lamp-records --all-targets --target aarch64-unknown-linux-gnu --offline -- -D warnings
```

Here `stable` and named `1.96.0` resolve to the same compiler commit; only
`stable` had the target standard library installed. Initial checks without
that selector failed with E0463, then the command above passed. This is
cross-target checking, not ARM execution or a linked release build.

Under ARM64 user-mode emulation (`qemu-aarch64` 7.2, as in a local podman
gate), `openat2` is not implemented (ENOSYS), and the store refuses to run
without it. Every test that opens a store (those in `store.rs`,
`main.rs`'s `cleanup_on_sets_aside_a_newer_report_and_still_removes_an_expired_session`
and `public_tree_pipeline_dry_then_apply_keeps_live_memory_and_outside_link`)
probes for it once, and on ENOSYS prints why and stops (#1210). With `CI` set
they fail instead, so the real ARM64 runner, whose kernel has `openat2`,
always runs them.

All three unchanged unit templates passed `systemd-analyze verify` under
Debian Bookworm's systemd 252.39-1~deb12u2, in another disposable local Linux
x86_64 container. The initial check reported the missing installed executable
for both services. A container-only `/opt/lampOS/bin/lamp-records` symlink to
`/bin/true` satisfied executable existence; the repeat emitted no diagnostics
and exited zero. No service started and no security directive was suppressed.

Tests inject time, sizes, process state and failures and use disposable trees.
They include authoritative-guard ambiguity/restarts/surviving workers,
unexpected `serve` processes, disappearing/replaced entries, links, full-disk
errors, interrupted removals and recovery, receipt bounds, real bus delivery,
and live reporter reloads through enable/disable and changed retention policy.
No lamp records were available to this builder. Initial development failures
and the exact container commands are retained in
[Fourth's work log](https://github.com/autonomous-ai/lampOS/blob/lamp-fourth/lampOS/docs/work-log-fourth.md).

## Switched on, lamp-4ace (first builder, 2026-10-14, 14:03)

`/etc/lampOS/records.json` set to `enabled: true` with a 2 GB bound and
the 14-day age (it was 500 MB and off; the old file is kept beside it),
the service and its hourly timer installed from `crates/records/systemd/`
with the binary's path on this lamp (`/opt/lampOS/records/lamp-records`).
The first dry run refused: since 13:21 the voice runs the fast end-of-turn
worker, `lamp-turn-end`, inside its own group, and the guard took it for
an unknown process sharing the voice's ownership. It now knows that one
helper by name and still checks its open files (`136e23c09`, with a test;
the guard's 19 Linux tests pass under emulation). Then the dry run and the
first real run: 1.18 GB of sessions, nothing planned and nothing removed,
no error. The timer runs every hour.

## The reminders' consumer, lamp-4ace (first builder, 2026-10-15, 20:05)

From the first hourly run after the schedule tools arrived
(`20bf8cabe`, 02:05) until 20:01, every run on lamp-4ace refused with
"process … protection is incomplete: unknown voice process command". With
`LAMP_SCHEDULE_TOOLS=on` the voice starts `lamp-voice schedule-consumer`
beside itself, in its own group, and the guard knew no such command.
Nothing was removed in that time; the sessions stood at 1.52 GB, under the
2 GB bound. The guard now reads `schedule-consumer`, with no other
argument, as naming no output, and its open files are still protected
like any voice process's (a test for each, and one that an extra argument
is still refused). On lamp-4ace (ARM64 build `2fb1eb99…`, the previous
binary kept beside it as `lamp-records.before-20261015`), the dry run and a
real run of the service at 20:05 both passed: 1.52 GB, nothing planned,
nothing removed, no error. Under emulation the guard's 34 tests pass, and 22
of the store's fail with "Function not implemented" (code 38), which is
the emulator, not the change.

## The voice's own list (thirteen, 2026-10-16, #1071)

From 2026-10-15 14:01 every hourly run on lamp-4ace refused with "process …
protection is incomplete: unknown voice worker command" (22 in 26 h; the
sessions reached 1.69 GB of the 2 GB bound). The second voice's worker,
`lamp-voice worker second`, was on no list the guard had. It was the third
time: `lamp-turn-end` on 2026-10-14 and `schedule-consumer` on 2026-10-15
went the same way, each noticed only in the journal.

The voice's processes are now named once, in
[`lamp-voice-processes`](../crates/voice-processes/src/lib.rs): its
commands (`serve`, `directed`, `directed-fixture`, `camera-inspect`,
`provider-check`, `schedule-consumer`, `worker`, and the read-only reports
run by hand, `short-turns`, `stt-check`, `marked`, `turn-times`, `stalls`,
`probe-cascade`, `probe-cache`, `probe-tools`, `probe-reminders`), its worker
roles
(`capture`, `speaker`, `provider`, `second`, `camera`, `ring`, `privacy`,
`probe`) and its helpers (`lamp-turn-end`). Both crates use that list:

- **The voice** runs only as a listed command: `lamp-voice WORD` refuses
  any other first word before anything runs, and a test reads `main.rs`'s
  dispatch and fails on a word not on the list
  (`every_command_the_voice_dispatches_is_on_its_list_and_no_other_runs`).
  It starts a worker only in a listed role. `Worker::spawn…`
  refuses any other before it runs anything, and `lamp-voice worker ROLE`
  refuses an unlisted role before its arguments are read. A role added to
  the voice and not to the list fails the voice's own tests on the
  builder's machine (`a_worker_role_not_on_the_voices_list_is_never_started`
  in `crates/voice/tests/it/process.rs`), never first a cleanup on a lamp.
- **The guard** knows exactly the listed commands and roles. Its
  role-specific reading stays in the guard: `capture` and `speaker` name a
  `--diagnostics` folder, `schedule-consumer` takes no other argument, and
  each command's output argument is where it was. A test runs every listed
  role and command through it, and one runs `worker second` with the
  arguments lamp-4ace ran it with.
- **When it still refuses**, the refusal names the role, when it is a plain
  word (lower-case letters, digits and dashes, at most 32):
  `unknown voice worker `X``, in the journal and in the private result. The
  reporter counts the cleanups in a row refused for that, from the results
  themselves, and from the second on its `records` diagnostic says
  `records refused: unknown voice worker `X``, at error, in lamp-health. One
  refused cleanup can be a worker caught as it starts; it keeps the usual
  "the last cleanup failed for one or more roots". The unit's failure stays
  in `systemctl --failed` as before.

The reporter is still one of the templates beside the crate, not a product
unit. Where it is not running, lamp-health shows nothing of this, and the
journal line and `systemctl --failed` are where a refusal shows.

## Process-exit repair, development host, 2026-10-14

Task 30 follows the first builder's failed 16:00:28 hourly run: PID 1738165
had no inspectable arguments, while a 16:57 dry run passed. No state snapshot
for that PID survived, so exit remains an inferred cause of that particular
failure. The repair above establishes the safe cases explicitly and keeps
live ambiguous processes as errors.

On the same development Mac and Rust/Cargo 1.96.0, the final formatting and
strict Records Clippy checks passed. `RUST_TEST_THREADS=1 cargo test -p
lamp-records --offline --locked` passed **113 tests**, with zero failures and
one ignored existing manual inventory timing. The unchanged baseline passed
97 tests; the pre-review implementation passed 111. The additional cases
cover exit races, zero thread counts, positive kernel-thread identification,
bounded report examples and continued live-file protection. A Linux-only
owned-child test also distinguishes a live fd holder, a zombie and a reaped
process; macOS does not execute it. Commands, verbatim outputs and source
hashes are in the [host verification folder](verification/records-exit-host-20261014/).
The standalone Debian 12 Linux x86_64 run passed strict Clippy and **115
tests**, including the live fd-holder/zombie/reaped test; zero failed, two
manual tests were ignored (cost measurement and privileged bind mounts).
The Linux ARM64 release cross-build and Mac workspace build passed without
warnings. The required native ARM64 GitHub gate is still outstanding after
an inherited Controls test could not find `/bin/kill` in CI; the task remains
unready for integration. No guard timing or lamp measurement is claimed by
these checks.

## Not shown

The lamp's loaded systemd command serialization, host procfs visibility,
historical session layouts, actual reclaimed capacity, full-filesystem
recovery on its storage, boot/timer operation, or effect on conversation
latency. No real lamp record has been removed, unit installed or device accessed.
The highest device risk is incomplete or differently rendered evidence of
which processes still use a session. A malicious privileged namespace writer
is outside the safety contract. Journal, policy and unmanaged bus archives
remain outside this adapter's enforced retention, and their absence is not
hidden by a successful voice cleanup.
