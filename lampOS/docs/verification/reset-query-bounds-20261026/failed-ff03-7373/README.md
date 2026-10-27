# Query 7373 carry: retained failed host gate

Exact PR head ff03ad11f3884e4ed09bb5538a6ba2a495891124 on public base 737331535d0cab813170d288110fc44ff5d41a68. Fmt, strict Setup/Cloud Clippy and compilation passed. The single full Setup/Cloud runtime returned 101; supervisor returned 1 after retaining source and artifact closure. **388 passed, 1 failed, 5 ignored**, plus two zero doctest summaries. All eight query controls passed once. This is a failed gate, not readiness.

The sole failure was `a_waking_face_that_never_answers_cannot_keep_the_privacy_ring_dark`, crates/setup/tests/wake.rs:279. Its measured interval was 163.063834ms; it requires at least AT_ONCE_WAIT 100ms and strictly less than 150ms. The observed failure is retained without retry, source fix or waiver. Whole command 21.939s. Other selected targets continued under --no-fail-fast.

The direct test and Setup wake implementation, Ring waking implementation and their manifests are byte/mode-identical to the previously passing 99 carry, as immutable Git entries show. That comparison does not establish causation or excuse the failure; inherited Memory/Voice dependency source changed and host scheduling differs. A separately requested exact public-baseline control may add evidence but must not erase this run or imply that an isolated case recreates full-suite contention.

Completed runtime closure checked unchanged source and 10 executable/63 local-output/445 depfile actual+preserved bindings. Raw-log hashes, exact commands and all case rows including the failure are retained. No complete-success marker exists. No binaries, scripts or private paths are published here.
