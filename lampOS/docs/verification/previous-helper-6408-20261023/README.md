# Current writer with the actual previous fixed helper

Provisional compatibility evidence for #2809 and #3269, 23 October 2026.
First has not yet selected this as the supported release pair. This receipt
does not qualify a release or replace the official ARM64 PR gate.

Current product source is `46f5f384ad94f9c2dcc5f7050991ef4325621384`, based on
`810e05109`. The current coordinator and writer run on the Mac with Rust
1.96.0. The consumer is the actual published Linux ARM64 `lamp-update` from
[release 6408](https://github.com/autonomous-ai/lampOS/releases/tag/lampos-lamp-voice-6408),
source `ee53765c8d014eb00e41c1a5a7054d2546c18bdb`, asset 668607981.
Its 4,690,504 bytes hash to
`f731555d6b783a69eb00bc2ff774e8ad922988284c9f3a9114010fa75fa51778`.
The earlier asset receipt checked the downloaded bytes against the manifest
and published checksum. Independent release-signature verification still
needs Migrate's trusted public key; these tests use newly generated fixture
keys, not the release signing key.

## Results

The final fixture source passes **6 tests, 0 failures** in two commands:

| Target | Passed / failed | What was exercised |
| --- | --- | --- |
| Standalone helper | 4 / 0, 2.23 seconds | Current request to old Apply to current merge; old Apply/Back restores bytes, modes and state hashes; current-produced backup consumed by old Back; unknown operation refuses without file-tree changes or a done receipt |
| Current round with old helper | 2 / 0, 7.86 seconds | Real old Apply writes incompatible pins, real old Back restores them before any reader restart; logical interruption after old Apply, then a fresh current coordinator restores pins and reloads unchanged Controls |

Output is in [standalone.txt](standalone.txt) and [round.txt](round.txt), with
terminal blank lines removed; the receipt retains the original raw hashes.
The round output includes an intentional typed panic at the cut point; the
test catches that exact payload and checks recovery. It is not process death.
Both commands exited zero. Formatting and strict Clippy for both test targets
also exited zero, with empty diagnostic files. [receipt.json](receipt.json)
binds source blobs, fixture hashes, commands and output hashes.

The mismatch fixture deliberately changes the signed legacy pins' interval
from 5 to 6 while carrying the new reader pair. This forces the old helper
to save a real backup and write its unprojected bytes. The current
coordinator detects the mismatch and asks that same old executable to put
the saved bytes back before restarting Voice, Expression or Controls.

The cut fixture leaves the old units, binaries and current links unchanged.
After the old helper completes Apply, it interrupts the current coordinator
at a typed logical cut. A fresh current `World` reads the persisted journal,
invokes actual old Back, restores the prior files and settings hashes, and
restarts unchanged Controls after Back. The old helper does not read that
journal: this is current-coordinator/old-helper evidence, not old-coordinator
journal compatibility.

## Isolation and reproduction

The manual tests are ignored by default. In a disposable checkout of the
current source above, copy [helper_release_pair.rs.txt](helper_release_pair.rs.txt)
to `lampOS/crates/update/tests/helper_release_pair.rs` and apply
[round-additions.patch](round-additions.patch) from the repository root.
The fixture SHA-checks the old executable and its AArch64 ELF header before
any Docker call. Set `LAMP_COMPAT_OLD_HELPER` to that downloaded artifact.

The fixture uses the existing `colima-lampos` Docker context and pinned
Linux ARM64 image
`sha256:7dc3bb4b3b88bb00e16e37eeb5c98a74f9d926f956584eb8c009c9ca86023cd2`.
It does not pull images. Each invocation creates a new network-disabled,
labelled container with no host mounts, copies only the verified helper and
fixture-owned settings/update records, and runs `lamp-update apply`.
Back is a staged `op: back` request to that same apply command; the CLI's
separate `back` command is not the settings rollback operation.

Docker operations have deadlines, capture output in files and confirm the
container's actual terminal exit status. Copy-back affects only a test-owned
temporary root. Cleanup verifies the ownership label before removing the
container. The final labelled-container listing was empty; [cleanup.json](cleanup.json)
retains the command, exit code and observed output.

From `lampOS/`, the executed commands were:

```sh
cargo +1.96.0 test -q --locked --offline -p lamp-update --test helper_release_pair -- --ignored --test-threads=1 --nocapture
cargo +1.96.0 test -q --locked --offline -p lamp-update --test round actual_old -- --ignored --test-threads=1 --nocapture
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -q --locked --offline -p lamp-update --test helper_release_pair --test round --message-format=short -- -D warnings
```

The host used two build jobs, incremental compilation off, zero dev/test
debug info and an existing cache. No local ARM64 build was run. The fixture
source is retained here as evidence rather than added to the ordinary PR
test suite, which must not require this machine's Docker context or image.

## Limits and remaining acceptance

The coordinator uses the existing fake-systemd round harness. These six
tests do not run an ARM64 coordinator, real systemd, an installed helper
refresh, a daemon exec handover, a process kill, a device reboot or a physical
unplug. They do not prove flash durability or the supported release pair.
The fixed old helper safely rolls back the incompatible head-pins update;
successful new-mode projection still needs the fixed helper refreshed through
install. The official PR's native gate and its product assertions remain
separate. First owns pair selection and merge; the assigned lamp holder and
attending person own the physical trial; Migrate owns the public-key handoff.
