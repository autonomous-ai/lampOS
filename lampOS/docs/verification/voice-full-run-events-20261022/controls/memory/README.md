# Memory fixture controls: executed

All three predeclared controls were compiled once and run once on native
x86_64 macOS. This completed-run record supersedes the historical preparation
state in the external scratch README/manifest. No control was retried.
[summary.json](summary.json) records the exact commands, original output
hashes, mutant and restored-source hashes, exits, durations and source audit.

| Patch and raw output | Required result | Observed result |
|---|---|---|
| [Wrong oldest text](oldest-text-changed.patch), [output](oldest-text-changed.txt) | Original Hue assertion rejects Da Nang | Exit 101; 0 passed, 1 failed; `remember.rs:9644:9` |
| [Reply held after readiness](reply-held-after-ready.patch), [output](reply-held-after-ready.txt) | Original first `hits_note` unwrap fails under the real 60 ms deadline | Exit 101; 0 passed, 1 failed; `remember.rs:9645:90` |
| [Setup held before readiness](setup-held-before-ready.patch), [output](setup-held-before-ready.txt) | Both original calls and all three assertions pass | Exit 0; 1 passed, 0 failed |

The fixture prebuilds the same 150-entry response, then uses a zero-capacity
channel before each request. The worker reaches that rendezvous with the
listener bound and response prepared; it still has to accept/read the request
and write the response. Readiness is not evidence that the client received a
response. In the tested candidate, both `hits_note` expressions, all three
original assertions, and every byte outside this selected `cfg(test)` function match base
`69f2228338a8df63a81d23087c3e55308fa125ca`. The production
`HITS_WITHIN = 60 ms` and its `bounded_exchange_up_to` call are unchanged.
No product clock is frozen. There was no fixed sleep in the original case,
and the submitted fixture adds none. The 160 ms sleeps exist only in the two
control patches. The 5 s readiness watchdog and 2 s fake I/O bounds do not
extend either real call deadline. Arbitrary descheduling during a real call
can still cause it to miss that deadline.

The controls precede the rebase to `e2232b8369fb125af6c79e8bf6927ec590a612e9`.
The selected fixture body is byte-identical after that rebase, and all bytes
outside it match the new base, including its `ALREADY_KEPT` product change.
The summary records both source audits. Historical control and restoration
hashes describe the earlier candidate; final-tree gates are recorded separately.

The wrong-text control also records the fake worker's subsequent `SendError`
after the original Hue assertion panics and drops the receiver. Its primary
failure is the unchanged Hue assertion. Test-reported times were 0.04 s,
0.06 s and 0.37 s; measured process wall times were 2.223769 s, 3.195832 s
and 9.245528 s. These are distinct boundaries, not device latency results.

The initial sandbox socket probe returned EPERM before any Cargo command.
The authorized socket-enabled run then passed that probe. After the first
control's intended failure, the runner rejected Rust's numeric thread-ID
suffix while parsing the panic. That runner stop and original output remain
recorded externally. Correcting the parser ran only the two remaining
controls; the first control was not repeated.

Before the first Cargo invocation, 1,378 tracked crate/Cargo/toolchain files
had their mtimes refreshed. The before/after content aggregate was identical;
its algorithm and digest are in the summary. Exact saved candidate bytes were
restored after every control and finally, with SHA-256
`53fbd84e3453221c6a77b41771391d64983526f8941e199d120bc11c672d223d`.
The exclusive target was `/private/tmp/tests-codex-small-native-20261022`,
with two build jobs, offline Cargo, debug info and incremental compilation
disabled, and `CARGO_BUILD_TARGET`/`RUST_TEST_THREADS` unset.

Each text file wraps the original stdout and stderr byte streams with
`BEGIN`/`END` lines and a final `END RAW CONTROL OUTPUT`. Original byte
lengths and hashes in the summary exclude those wrappers. Successful compile
logs, Cargo JSON, full compile-tree manifests and Python runners stay outside
the repository; their hashes do not make them portable evidence. No full
suite, final gate or ARM64 validation is claimed by these controls.

To replay, save the exact candidate file, apply one patch from the repository
root, compile using the summary's command from `lampOS`, then run the emitted
library-test executable with the recorded exact test and `--exact --nocapture`.
Capture the expected result, restore the saved bytes and check their recorded
SHA-256 before applying another patch. The unchanged comparison base lacks
this fixture change, so restoring it is insufficient. Apply one control at a
time.
