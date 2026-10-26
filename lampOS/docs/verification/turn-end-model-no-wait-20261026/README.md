# Turn-end model-open fixture: frozen Mac proof

This packet qualifies the uncommitted test/documentation delta on `0c1f6437ee57eae3416e3281d91b813d1c3bec1d`. It does not qualify a later carry, including the newer `1c884` base. The fixture removes only the missing/wrong-model case's 100 ms upper bound and adds a real worker held at FIFO model open. Spawn must return Loading with unavailable submission before the parent opens any writer. Reader rendezvous, size refusal, no Ready or kill, worker reaping, completion witnesses and native child success are required. The five-second diagnostic watchdog precedes the unchanged ten-second startup deadline; product code and Group B deadlines are unchanged.

Mac formatting, strict Turn-end all-target Clippy and compilation pass. Full crate: **30 passed / 0 failed / 8 ignored** (15 library, 9 binary and 6 integration passes; eight model-dependent integration cases ignored). Separate focused checks pass **2/0**.

| One execution of each control | Result |
| --- | --- |
| Original fixture, 400 ms pause at the worker-return boundary | **0/1**, native 101 at the old 100 ms assertion |
| Corrected fixture, 400 ms pauses at both return boundaries | **2/0** |
| Actual ProcessClassifier::spawn changed to wait while Loading | **0/1**, native 101 at parent watchdog, reported 5.03 s |
| Restored exact source, freshly compiled | **2/0** |

The negative's inherited child stderr split the outer test header from its FAILED token. The original controller therefore rejected its anchored result-line parse and retains **complete:false**. [reconciliation.json](reconciliation.json) separately binds the native 101, exactly one failed test, parent line 359 and restored positives. The child Loading assertion after failure cleanup releases the FIFO is retained as a cleanup diagnostic, not another test case. No test was rerun to repair the parser, and no original receipt or controller was rewritten. At the completed restoration check, all 7,192 source entries and modes were restored; preserved artifact copies remain intact, actual binaries matched the restored copies at the completed restoration review, and original/restored worker hashes agree.

Before the compiler gate, source review caught Rustix's Apple-excluded mkfifoat API. The fixture switched to pinned Nix 0.29.0's portable test-only mkfifo; explicit Rustix fs remains for openat. An initial full offline metadata command encountered uncached curve25519-dalek-derive 0.1.1 after resolving the lockfile; its exact exit code is **unknown**, and no compile/test result is attributed to it. Retained locked no-deps metadata succeeds. The lockfile adds only the existing Nix dependency edge, with no new package or version.

[results.json](results.json) records normalized commands, frozen source and artifact identities, raw-log/receipt hashes and setup history. [selected-results.txt](selected-results.txt) keeps compact outcomes. Full raw records and binary copies remain private; no download or availability is promised. This is host ordering/failure evidence, not ARM64, model, provider, device, acoustic or physical-latency acceptance.
