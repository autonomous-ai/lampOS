# Actual updater release-pair recovery

Assigned to Second under #2809. First selected release 6408 as the previous
reader; the current published release is 6449, source `be16c3327`.

## Artifact identity

| Role | Release | SHA-256 | Bytes |
| --- | --- | --- | ---: |
| Previous | 6408 | `f731555d6b783a69eb00bc2ff774e8ad922988284c9f3a9114010fa75fa51778` | 4,690,504 |
| Current | 6449 | `c953a32e802f41dc1bc741bef1fe910faa286f27c6cb1a1988aad435c5331434` | 4,739,096 |

Both downloaded files match their immutable GitHub asset digests and identify
as little-endian AArch64 ELF executables. Current bytes also match the current
manifest's updater entry. The release metadata and successful official ARM64
build/publisher run are retained beside this file. `artifact-files.json`
hashes those metadata files. No executable is committed.

The publisher's envelope is not independently trusted here: the actual public
key handoff is still pending. The manifest comparison establishes consistency
with the downloaded release, not independent cryptographic authenticity.

## Cases and scope

The two named cases live in `crates/update/tests/release_pair.rs`:

- `release_6408_recovers_a_cut_written_by_release_6449`
- `release_6449_recovers_a_cut_written_by_release_6408`

They must use the actual opposite-version coordinator and helper executables,
retain the journal and request prefix produced by the writer, prove that the
writer was killed after the selected boundary, then check exact recovery.
The signed channel uses a fixture key generated for the case, not a release
signing key. The service command boundary is fake; no actual lamp service or
actuator is started. Containers have no network and no host mounts.

Execution result: **2 passed / 0 failed / 0 ignored**, exit 0, 4.85 seconds,
on the first execution. `attempt-1.txt` is the Cargo output with only local
temporary directory prefixes replaced by relative evidence labels; both
original and published hashes are retained. `execution.json`
records each direction. Each case folder retains raw command streams, actual
journal/request/back/done files, snapshots and their source modes/hashes.
The checked writers and helpers are unmodified release executables. Each
writer received SIGKILL at the verified owned process group and its direct
parent observed shell wait status 137. This is not a `WIFSIGNALED` observation.
Recovery returned the expected rolled-back status 1, restored the old active
restart order, preserved the inactive member and unknown state canary, and
refused the same failed candidate on the next process.

Both containers were removed successfully after copying their output. The
final fixture container listing is empty. Docker `--init` handles orphaned
shim reaping. These are one pre-first-restart cut per direction, with fake
service health; no additional cut positions are implied.

The fixture source hashes are in `source-before.sha256`; they remain unchanged
after execution and the package gate. Formatting, both shell syntax checks
and strict Update Clippy pass. The Mac Update package gate is **371 passed /
0 failed / 6 ignored**: four existing platform skips and these two explicit
manual cases. `local-gates.json`, `clippy.txt` and `package-tests.txt` retain
the exact commands and results. The explicit manual result above is separate
from those skips. Required PR native gates are recorded when complete.

The earlier six helper-format checks are separate provisional observations.
They do not substitute for opposite-version coordinator journal recovery or
actual process death. Neither these tests nor those earlier checks establish
physical power-loss durability, a production systemd sandbox, attended body
motion or signed-release acceptance on a lamp.
