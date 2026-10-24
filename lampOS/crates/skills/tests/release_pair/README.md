# Published Skills daily and on/off continuity

Four ignored integration cases execute the actual published Linux ARM64
`lamp-skills` artifacts, in both 6408 → 6498 → 6408 and
6498 → 6408 → 6498 directions. Today's Skills library is not used as either
persisted-state reader or writer. The harness supplies synthetic manifests,
local bus input, a clock-drain control, and private output traps.

These cases do not cover `rested`, updater installation, package permissions,
the full skill loader, physical output, clock synchronization, or power-loss
durability. `rested` remains a separate follow-up. No passing result is claimed
until the pinned native cases have executed.

## Inputs and isolated execution

| Release | Source | Bytes | SHA-256 |
|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |

Set `LAMP_COMPAT_SKILLS_6408` and `LAMP_COMPAT_SKILLS_6498` to those artifacts.
Set `LAMP_COMPAT_OUTPUT` to an existing short absolute directory, such as
`/tmp/pair-output`, created with mode 0700. When omitted, the system temporary
directory is used. Each case retains a unique `kr-*` directory on success or
failure; its mode, and its state directory's mode, are explicitly set to 0700.
Use an isolated native Linux ARM64 runner with no network, credentials, device
mounts, running lamp services, or capture/playback access:

```sh
cargo test -p lamp-skills --test release_pair -- --ignored --test-threads=1
```

The fixture source typechecks on macOS; only executing these ignored cases
requires Linux ARM64. Every artifact is checked for exact length, SHA-256,
ELF64 little-endian format and AArch64 machine type, copied privately as
`lamp-skills-6408` or `lamp-skills-6498`, and made mode 0555. Every child is owned
by cleanup before its `/proc/<pid>/exe` bytes are verified. Copied artifact
bytes and modes are checked again after the case. The caller must exclude
those executable copies from any published evidence archive.

Each case has a 90-second overall bound, each process/readiness phase at most
15 seconds, each local stream operation at most two seconds, and failure
cleanup at most five seconds. Daemons are actually SIGKILLed and reaped between
readers, after their acknowledged state boundary. Helpers exit normally and
are reaped. A cut after a completed write is not a cut inside rename/fsync.

## What the cases establish

Daily cases use a synthetic skill with three stable named daily triggers.
A fixed, permitted UTC offset places the actual wall clock near local noon;
all three processes use it and the same 11:00–13:00 window. The fixture fails
if the local date/window changes. It neither sets wall time nor treats the
`--clock-control` wait-clock as wall time.

`LAMP_SKILLS_ASK=policy` makes the actual daemon claim a day durably before
asking for output. No Policy subscriber or approving decision exists. The
first trigger must produce exactly Triggered, Asked and Answered(false),
with the explicit missing-Policy refusal. The opposite release must reject
that consumed day, keep its bytes unchanged, and successfully claim a second
trigger. The original release then rejects both prior claims without changing
their bytes and claims the third. Raw schema-2 bitmaps must contain exactly
one expected local day per key. A separate non-daily positive control proves
the daemon remains responsive. Every input is delivered to the real subscriber
and followed by `{"skip_us":0}`: the real main loop drains preceding inputs
before acknowledging it. Duplicate suppression is checked with that barrier,
the actual daily-refusal diagnostic, exact persisted bytes and fresh-claim
positive controls, not an arbitrary silence interval.

On/off cases use the actual `asked --dir skills --state-dir state` helper,
one request/reply over a private socket pair. The explicit existing test
allowance `LAMP_SKILLS_ANY_ASKER=on` bypasses only peer/cgroup admission; this
does not qualify production caller authentication. No Store switch is set.
The first release enables a shipped-disabled synthetic core skill, writing
`on` and changing its manifest. It also disables a separate enabled skill by
writing `off`, leaving that manifest enabled. The opposite daemon must
explicitly suppress the latter from `off` while the enabled control reaches
the refusing Policy boundary; its read leaves both state files unchanged.
The opposite helper reads and reverses both records, including restoring the
shipped-disabled manifest. The returning original daemon and helper consume
those changes and restore the initial record bytes.

There are no providers, accounts, microphone inputs or actuator processes.
The daemon inherits no environment: only the recorded fixture settings are
passed. Gaze/mood datagram sockets and Actions/Say stream listeners are private
traps; any attempted output fails the case. All optional feature switches
remain absent. `daily.json` must be mode 0600. Neither case creates `rested`.

## Retained evidence and limits

Each case retains the release/source pins, fixed clock settings, exact
argv and explicit environment, running executable identities, input and
event transcripts, exact helper responses, raw child stdout/stderr, exit or
failure-cleanup receipts, and state/manifest snapshots with file hashes.
The final receipt records unchanged executable copies. No artifact is
downloaded by this harness and no audio or personal fixture content is used.
Raw logs can contain the runner's private temporary path; publication must
review/redact that path and preserve an explicit redaction receipt.

The tested daily writer uses schema 2; both releases also contain a schema-1
reader, but these cases do not claim an actual schema-1 writer comparison.
The daily/on/off implementation files are unchanged across these sources,
while the released executables differ (6498 changes presence-duration
bookkeeping). These equality-trigger fixtures avoid that unrelated behavior.
They prove compatibility of these exact released artifacts and states, not a
guarantee for arbitrary future schemas. The unversioned `on` record is consumed
here by the helper alongside `enabled`; updater preservation of it is outside
this fixture.
