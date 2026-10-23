# Common private-context Checking correction, 23 October 2026

Current source preparation, not a lamp qualification. The behavioral gate and
new ARM64 head remain pending. Earlier full-run failures remain in the work log
and PR #3064; none is erased by the CLI check below.

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
