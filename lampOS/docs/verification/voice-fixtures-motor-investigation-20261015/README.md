# Motor fixture investigation on the Intel development Mac

Fourth's Intel Core i7-10700K, x86_64 macOS 26.6.2 (25G83), 64 GiB RAM,
Rust/Cargo 1.96.0. This evidence predates the full loaded qualification;
it is not a lamp result. Raw files are copied without editing. Scripts
are retained as non-executable text, and no binary is committed.

At 19:26 UTC on 2026-10-14 a scratch orchestrator compared whole
spawn-to-exit intervals for one existing Voice test binary. Its SHA-256 was
`b2dc0816f07cf7f21d59be69fff78b19705c39292876e6f493a1a2ee4b741efa`.
Every invocation selected `coordinator::motor::tests::inert_child` with
no motor environment, so it returned before device or IPC setup. Three
fresh executable script inodes were compared with direct binary execution,
then repeated execution of the first wrapper and three fresh scripts
passed to explicit `/bin/sh`. Exact commands and nanosecond measurements
are in `launch/results.json` and `launch/followup.json`.

| Invocation | Spawn-to-exit milliseconds, three observations |
|---|---|
| direct | 91.149, 33.005, 28.657 |
| wrapper | 675.516, 296.821, 295.996 |
| same-wrapper | 33.426, 34.284, 32.841 |
| explicit-shell-fresh-wrapper | 33.412, 32.718, 32.346 |

These observations localize substantial cost to fresh executable-script
invocation in this experiment. They do not identify the OS subsystem,
prove cold filesystem caches, reproduce a historical two-second timeout,
or measure motor readiness. No Podman stress load was deliberately started
for this experiment; it is separate from the later loaded gate.

The task's inert preflight executes the exact frozen wrapper and requires
its R marker, successful exit and actual reap before real supervisor timing
begins. It starts no worker, socket or device. Production startup and safety
bounds and the existing two-second fixture predicates are unchanged. Three
new tests cover identity/no sockets, invalid marker/status and actual reap,
and marker-without-exit timeout/reap. The crash-preserves-voice scenario also
creates its reply after unrelated startup and refreshes that same owner's
500 ms lease while servicing the motor, without replacing the owner.

The first focused development module at base
`1e5beebf42910ec043cc6ad76552af51165e5b01` plus task edits passed 31 / 0 / 0,
19.17 test seconds, 28.862 seconds including Cargo. Exact source hashes,
command, binary identity and raw output are under `focused-module/`.
Its raw output hash is
`f08af61995eb22a157c03bafde93548aa45644386bd74d1b375e1760300b5e17`.
The task's motor source remains byte-identical in frozen v4:
`9d2a40f46093c7603c907f7d1cf7a88f05e9b311581c3d79f75560a275b3d7f3`.
An isolated passing module is not a complete task gate.

`host-disk-preflight.json` separately retains the earlier reclamation of
Fourth's inactive generated Judge target cache after an empty open-file
check. Source, measurements and other builders' caches were preserved.
That filesystem operation is not a motor timing measurement.

Not shown: the cause of every historical startup or missing-receipt failure,
arbitrary-load determinism, cold production startup, physical output or
stop latency, or a native ARM64 task-head gate. The real 20 ms parent inhibit
clock can still expire while the file-gated fixture child is held. The
injected Body-clock seam remains an unanswered ownership question on #739;
no Body source was changed and forced termination is never clean-exit proof.
