# Memory and Records product contract, 24 October 2026

Static evidence for release checklist item 6 and Second's #2809 worksheet. Fourth has no assigned lamp. No device, install, service start, socket request, database read, signature-trust check or payload-binary execution was performed. This report does not establish the effective units on a running lamp or justify changing a default.

## Binding and observed counts

- Selected published release: **6592**, published 09:26:46 UTC, source `c6dfe980979e0088cd94f1dab1fe4a70ddbcbe46`. Current source sampled: `5238a6ecdbeb3e63315479f31c541bebd596eaf5`.
- **16/16 sampled source files are byte-identical** between those commits. `receipt.json` lists every path and both SHA-256 values; this is a scoped comparison, not whole-tree equality.
- **4/4 downloaded assets** match the release API's digest and size. The envelope decodes byte-for-byte to the downloaded manifest. The two published Memory units also match their manifest descriptors and source files. Public checksum consistency is not trusted-signature verification.
- The manifest contains **32 file descriptors and 28 unit descriptors**, for lamp-4ace and lamp-52e6. It carries Memory and Records binaries, but **no Records unit/timer and no product-lamp unit entry**. Binary bytes were not downloaded or independently hashed here; their published descriptors are retained in the receipt.
- No Cargo/test suite was rerun for this documentation-only audit. Existing #3333 Memory and #3329 Records qualification remains separately bound in its own evidence; these static comparisons add no test passes.

## Product contracts

| Boundary | Product definition / source evidence | Live observation still owed |
|---|---|---|
| Memory start | `lampos.target`, setup-done condition; Voice starts after Memory. `lamp-memory --dir /var/lib/lampos-memory --socket /run/lampos-memory/memory.sock --keep-days 30 --zone system --memory-days --bus-root /run/lampos-bus`. | Effective argv, current binary identity, ready socket and no restart loop. |
| Memory ownership | `lampos-memory:lampos-memory`, private state directory 0700, runtime directory 0755; primary socket created 0600 by the server. Product Voice is root, so no group-readable socket or ACL is needed for this caller. | Actual ownership, modes and effective Voice user. Do not widen modes to make a check pass. |
| Memory isolation | Private network, strict system protection, private temp, protected home; bus writable; systemd StateDirectory/RuntimeDirectory provide its own writable directories. Bus reporting is optional and cannot hold the request loop. | Effective sandbox/drop-ins and bus health freshness. |
| Memory persistence | SQLite file `memory-days.sqlite3` in the private state directory; `--memory-days` is on. `--keep-days 30` bounds journal retention, not all kept personal facts. `--zone system` follows the configured local zone. | Counts/times before update, after update and after put-back; logical survivor/forget observations without personal text. No raw database publication. |
| App's narrow forget | Optional `--forget-socket` and `--forget-group` are not passed by this product unit. The primary Voice socket still supports forget. | Do not claim the optional app-only socket is deployed from its CLI support alone. |
| Records schedule | `lamp-records.timer`: hourly, randomized delay 60 seconds, accuracy one minute, persistent; enabled outside `lampos.target`. The service itself is timer-started, not directly enabled by install. | Timer enabled/next trigger and installed service identity. |
| Records retention | `/etc/lampOS/records.json`: schema 1, explicitly enabled, Voice metadata root `/var/lib/lampOS/voice/sessions`, 2,000,000,000 bytes and 1,209,600 seconds (14 days). No `session_read_group` opt-in. | Effective configuration matches these bounds; retain the private layout. |
| Records cleanup | Root oneshot, UMask 0077, 60-second start / 5-second stop bounds, memory 256M, idle I/O. Writes limited to the sessions root and private `/run/lamp-records`; report is `last.json`. No private PID namespace, ProtectProc or PrivateUsers: live-output guard needs host process visibility. | Host process visibility, effective write paths, bounded completion and no deletion of live output. Do not start cleanup merely to collect this read-only audit. |
| Persistent boundaries | Product tmpfiles creates Voice sessions root as root:root 0700. Normal update switches build links; Memory store lies outside its normal updater write set. Factory reset explicitly lists Memory state and Voice sessions for removal. | An update/put-back must retain stores; a factory reset is a different destructive operation and is not part of #2809's survivor check. |

The published per-lamp Memory units are historical deployment definitions, not proof of current hardware state. Both use `orangepi`, whereas the product uses its dedicated Memory user and a `current` link. The 4ace definition uses `--zone system`; 52e6 uses fixed `--utc-offset-minutes 420`. Both opt into Memory Days, 30-day journal retention and the bus. The updater understands both explicit build paths and product `current` links; an update's binary switch does not by itself prove product unit settings were installed or refreshed. This is a coverage boundary, not a demonstrated running-lamp fault.

## Holder read-only checklist

The holder retains device control and chooses an attended window with Second. These observations have **not run**. Record monotonic/UTC times, digests, modes and counts; keep personal words, identifiers, environment values and raw database/session contents private.

1. Read effective `FragmentPath`, `DropInPaths`, `User`, `Group`, `ExecStart`, state/runtime directories, sandbox and write paths for Memory and Records; read Voice's effective user and output-directory shape. Compare against the bound product source, including all drop-ins. Do not publish the Voice environment or provider files.
2. Read the Memory and Records `current` link targets and executable SHA-256 values. Bind installed A and chosen B to actual signed eligible releases with the device's trusted public key; release 6592 is a static reference, not a requirement to install an older build.
3. Use `stat` for the Memory state directory, socket and ready marker, Voice sessions root and Records runtime report directory. Report owner/mode equality and readiness age, with no store contents. A stale ready file alone is not liveness.
4. Inspect the Records timer's enabled state and next trigger, and the service's last result/exit status/timestamps. Inspect existing structured health/report counts privately. Do not invoke the `--apply` command just for this check: it deletes eligible closed sessions.
5. At Second's A/B/put-back checkpoints, record Memory aggregate counts and times, Records retained/protected/deleted/refused counts and timestamps, and the current binary identities. Keep journal output to counts/times only. Preserve all live-session protection checks; a green static checklist is not a native-systemd cleanup or real-lamp update result.
6. Report each row PASS, FAIL or NOT RUN. If a running unit differs, return its sanitized argv/flag/mode difference and holder evidence to Fourth; then a narrowly scoped own-branch PR can carry a proven product improvement. Until then no drift/default change is asserted.

Commands used off-device: `gh release view lampos-lamp-voice-6592 --json targetCommitish,assets,publishedAt`; `gh release download` for the four metadata/unit assets; `git show <bound commit>:<path>` for all 16 paths; local SHA-256/size and strict JSON/base64 comparisons. The scratch validator and binary caches are not committed. `release.json`, `receipt.json` and the exact four downloaded assets are retained here.
