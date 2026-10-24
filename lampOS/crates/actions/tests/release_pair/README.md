# Published Actions cap-state continuity

The two ignored cases execute the published Linux ARM64 `lamp-actions`
artifacts in both 6408 → 6498 → 6408 and 6498 → 6408 → 6498 directions.
Only those released processes write or consume the live cap store. The driver
uses the private JSON socket, observes the private bus and inspects copied
SQLite files with generic read-only SQL. It never substitutes today's cap
Controller for a released reader.

Both published Actions executables are **byte-identical**: 4,517,592 bytes,
SHA-256 `b5359557611b4fa3aa09709b659c7243f77c9ae945c326937f55adbc928903d6`.
The cap implementation is also unchanged between the two release sources.
These cases qualify the artifacts that shipped, not an implementation change.

| Release | Source |
|---|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` |

## Explicit native execution

Set `LAMP_COMPAT_ACTIONS_6408` and `LAMP_COMPAT_ACTIONS_6498` to the pinned
artifacts. Set `LAMP_COMPAT_OUTPUT` to an existing short absolute directory,
such as `/tmp/pair-output`, with mode 0700. The system temporary directory is
used if it is omitted. Each case retains a unique private `cp-*` directory;
its full path must be at most 45 bytes so Unix socket paths remain bounded.
Use a disposable native Linux ARM64 runner without lamp services, accounts,
providers, device mounts, capture or playback:

```sh
cargo test --locked --release -p lamp-actions --test release_pair -- --ignored --nocapture --test-threads=1
```

The target has exactly two cases:

- `release_6498_keeps_6408_caps_and_6408_reads_its_changes`
- `release_6408_keeps_6498_caps_and_6498_reads_its_changes`

Ordinary crate tests compile the driver on macOS too, but skip these cases.
No native pass is established by that skip. The dedicated
`lampos-caps-state-pair.yml` workflow downloads and checks the pinned artifacts
before executing both cases. The harness itself downloads nothing.

## Actual writer and reader sequence

For initial executable A, opposite executable B and returning A:

1. A writes two source caps and cancels one. Revision 3 must contain exactly
   one active entry and one cancellation tombstone. A is SIGKILLed and reaped
   after the acknowledged commit, before B starts.
2. B reads that exact state. The cancelled token cannot restart; changed
   same-token data, a replacement with an equal issue time, a wrong remove
   token and an extra RPC field are refused without changing the singleton JSON BLOB, revision,
   file mode or file identity. A fresh source succeeds at revision 4 while
   the original active cap survives. B cancels the original cap at revision 5
   and is killed and reaped.
3. Returning A reads B's state, rejects revival of both cancelled tokens and
   accepts the surviving token's exact retry without a write. A wrong remove
   token is refused. A fresh, strictly later put with a new token replaces the
   original source's tombstone at revision 6, retaining the unrelated cap.
   The displaced old token is stale. Another new cap reaches revision 7;
   explicit removals reach revisions 8, 9 and 10, leaving all four sources as
   cancellation tombstones and no active entries. The final child is killed
   and reaped, and the final stored JSON is checked again.

Every expected snapshot checks exact active sources, tokens, authored times,
ceilings, revision and the effective minimum. Generic inspection also checks
the exact stored source set and statuses; an empty-store fallback cannot pass.
The schema-1 JSON lives in the singleton BLOB of a version-1 SQLite database
using DELETE journal mode. Retained database copies pass `quick_check` and
are unchanged by read-only inspection. Continuity equality uses the exact
JSON BLOB plus original file mode/inode/device, rather than relying on SQLite
container bytes being invariant. Raw database bytes and their hashes remain
available separately. No WAL behavior is claimed.

Fresh puts sample the real wall clock immediately before their RPC. A
replacement waits boundedly for a strictly later sample; it never invents a
future issue time. Each cap expires one hour after its own issue time, and
the fixture fails if execution reaches any authored expiry. Thus this is
continuity before expiry, not expiration or clock-loss qualification.

## Ownership, output and evidence limits

Each child directly executes a private mode-0555 copy named
`lamp-actions-6408` or `lamp-actions-6498`. Input and copied bytes must match
the size, SHA-256, ELF64 little-endian and AArch64 pins. The live
`/proc/<pid>/exe` bytes must match too; originals and copies are checked again
at the end. Cleanup owns the child before post-spawn checks. Each case has
a 90-second bound, each stage or wait ten seconds, socket I/O two seconds,
and failure cleanup five seconds. The normal path records actual SIGKILL
and reaped signal-9 exits for all three children; no replacement starts
before its predecessor is reaped. The fixture verifies no helper descendants
are present before the cut.

Children inherit no environment. Only `LAMP_OUTPUT_CAPS=on` is supplied,
with private runtime, bus and cap-store paths. No mixer arguments, output
owner, provider or device is supplied. A fresh private bus per stage proves
the new process publishes its own exact snapshot. A volume restriction is
durably accepted but explicitly unconfirmed because no mixer exists;
`ready` is withdrawn while it is required. A ring publication is also
explicitly unconfirmed. Releasing all restrictions can restore software
readiness without proving physical output. These are process/RPC bounds,
not measured lamp response times.

The retained directory contains exact argv/environment, artifact identities,
RPC transcripts, bus envelopes, private stdout/stderr, snapshots, and exit or
failure-cleanup receipts. It is preserved after a failure. Evidence archives
must exclude the two executable copies and include only regular files, never
socket or FIFO contents. Synthetic identifiers are the only input content;
review private runner paths before public log publication.

The released stored structs tolerate additive fields, but their plain
`status` enum refuses unknown values. The private tagged `op` request and
nested put remain strict; unknown operations are not cap commands. This pair
executes the extra-field refusal, not synthetic new schemas or enum variants.
Newer-schema handling, unknown-clock retention, expiry, source-budget
exhaustion, signed release acceptance, updater installation, power loss,
flash durability and physical output remain outside its claims. See the
[verification record](../../../../docs/verification/caps-state-release-pair-20261024/README.md).

RPC replies now use one absolute deadline bounded by the original step/case
and two-second I/O budgets. Each read uses the remaining time; partial bytes
cannot renew it. Ordinary controls cover a complete response and a dribbling
peer. Dedicated `--ignored` execution still requires two actual artifact
cases, with the two ordinary controls filtered out. Earlier fast native
passes did not exercise this timeout boundary.
