# Common private-context Checking correction, 23 October 2026

## Current local qualification, 24 October

The corrected Common source is based on `9872cb69288b9e113bb95e6b25140a60a1e91e19`.
Frozen tested tree `665d44694def1048095deb236a27d9677dc084fa` passes all eight
required stages on Mac and offline Linux AMD64, with full source and index
stability plus process/container cleanup and target release confirmed.

| Host | Eyes | Voice | Update | Skills | Memory |
| --- | --- | --- | --- | --- | --- |
| Mac | 357 / 0 / 5 | 4,120 / 0 / 21 | 386 / 0 / 6 | 516 / 0 / 7 | 324 / 0 / 8 |
| Linux AMD64 | 360 / 0 / 5 | 4,181 / 0 / 24 | 388 / 0 / 4 | 519 / 0 / 7 | 324 / 0 / 8 |

Counts are passed / failed / ignored, selected once per final Cargo target.
Workspace formatting, strict workspace/all-target Clippy and workspace build
also exit 0 on both hosts. Each run validates and invalidates exactly the 46
local workspace packages, then binds all 46 to `fresh: false` compiler artifacts
across Clippy/build. Registry/git dependency outputs stay cached. Source-map
parity alone is not compiler provenance.

All three new missing-read/frame-dedup regressions and the existing
private-disclosure/enrollment composition control pass exactly once in each
Voice library target. Linux also observes the guarded-capture/enrollment
composition control once/pass. The correction preserves the publisher's
frame key across missing reads and the original 500 ms empty-look rate;
it adds no wait or I/O. The isolated exact-helper probe changed two expected
failures into four passing controls without relaxing an assertion.

Raw source/compiler/test/cleanup receipts are in
`/private/tmp/fourth-common-distinct-frame-gates-20261024/{mac,linux}/attempt1/`.
The final documentation update changes this README and the Vietnamese
private-context note only after those terminals; all tested runtime/test
blobs and other files remain unchanged. Earlier failures are retained, and
these fresh passes do not establish their cause.

This is local source qualification. Current PR ARM64/Clippy, independent
privacy reads, compatible Voice/Eyes artifact pins, installation and the
known-person lamp acceptance remain separate gates. The voice-freeze merge
hold remains; no lamp was accessed or installed.

## Historical carry preparation

> Carry preparation, 24 October: this draft now composes the preserved Common
> delta with main `9ed44f88c573837c50451c94f4d88f010e8a6721`, including its
> guarded enrollment and multi-face speaking changes. The new source and two
> composition controls are uncompiled and unrun; direct file formatting is
> syntax evidence only. The preceding frozen `7b5df82fd` source's Mac and Linux
> gates both ended with exit101; those failures and their original receipts
> remain retained. Nothing below promotes historical passes to this carry or
> supplies native, physical, review or installation acceptance.


The retained CLI probe below is separate syntax evidence. Earlier failed,
interrupted and timed-out runs remain in the work log and PR #3064.

## Exact retained Eyes CLI

The old retained Eyes executable is SHA-256
`bee4cb54e005008073cbcb1bd575e63674d6286120e55368f3aa0fd917d69658`,
25,173,000 bytes. Its unmodified `--help` exited 0 in 1.346 s under QEMU
7.2.22 in the cached Linux container, returning 3,809 stdout bytes. The raw
help is [eyes-pin-help.txt](eyes-pin-help.txt), SHA-256
`53918b521ce682c82928c845ddcdda5367a2e4cde580b4e14a0d33c09abc5283`.
All 16 flags from the existing lamp-4ace Eyes ExecStart are present;
`--steady-rate on|off` explicitly accepts the prepared off value. The
[flag receipt](eyes-pin-flags.json) binds the unit before and after that edit.

The initial 30 s attempt returned no help and timed out. A separate guest
loader control exited normally, so another 12 s attempt traced guest system
calls. It stopped during the process's initial log-relay thread setup before
argument parsing; its bounded cleanup exited 137. This identifies the observed
boundary, not a proven general cause in QEMU or a lamp fault.

A controlled fallback then set RLIMIT_NOFILE to 6 **inside** the running
container before launching the same executable. The relay's duplicate-fd
operation returned EMFILE, and its existing documented error path left stderr
alone. The unmodified program printed the logger diagnostic and its help,
then exited 0 naturally. Applying that low limit at container creation first
failed with OCI exit 126 before the guest ran; that attempt is retained too.
No limits were changed in the product, source executable or lamp unit.

The successful argv after container startup was equivalent to:

```sh
ulimit -n 6
exec /usr/bin/qemu-aarch64-static -strace -L /usr/aarch64-linux-gnu /probe/lamp-eyes --help
```

The cached image was
`44442b1b138ace391dbeab57edfb2673bec55cb39f2a287e92a3dfa100f452fc`;
network disabled, root filesystem and exact executable read-only, UID/GID
65534, no capabilities/new privileges, 128 MiB, one CPU and 16 processes.
A guest 12 s timeout with 2 s kill fallback and 25 s host bound applied.
The disposable container was absent after completion. Complete retained
local receipts are under `/private/tmp/fourth-old-eyes-pin-cli-20261023/`.

This qualifies syntax only. It is not a normal-start, camera, classifier,
atomic-source protocol, thermal, acoustic or known-person memory result.
Compatible qualified Voice/Eyes artifact pins, installation, and the new
known-person lamp run remain First's. The source units prepare scoped
context, camera-following and speaking in Voice, speaking in Eyes, and
regular Eyes cadence. No lamp was accessed or installed.
