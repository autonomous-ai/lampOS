# Recovery-input accessor: scoped Mac gate

Exact source: foundation fa8ac496353c4679d2ab43e70e37b80f1924296f plus the three owned uncommitted continuation files in identity.json. This is not a current-Main qualification. The source branch was not rebased, committed or pushed by this worker.

Sequential commands in results.json: `cargo +1.96.0 fmt --all --check`; `cargo +1.96.0 clippy --offline --locked -q -p lamp-update --all-targets --message-format=short -- -D warnings`; and one `cargo +1.96.0 test --offline --locked -p lamp-update --lib --message-format=json -- --test-threads=1`.

All three stages pass. The full library invocation reports **163 passed / 0 failed / 2 ignored / 0 filtered**. All **18 writer_refresh controls passed once**; those controls are a subset, not an additional test total. Test harness runtime 22.29 seconds; Cargo library-stage wall time 54.182 seconds. Complete actual case census is in census.json.

Compiler-artifact JSON identifies the test-profile library executable from this same Cargo invocation. Actual and preserved post-run executable SHA-256 both equal `a449e0a10ad582f80c0dbb0d142c49c1b702fd5d993195cb3349eb8f5baa6baf`; size 14950976 bytes. Profile/freshness fields are the actual compiler record, not an independent rebuild claim. Source closure checks all 7285 source hashes and modes unchanged. No new gate failure, negative experiment or test rerun occurred in this continuation.

The four added controls exercise reopened prefix/restoration inputs, qualified and terminal product-compatible inputs, normal/poisoned session refusal, and retained future metadata. They do not prove signature authentication, artifact bytes on disk, private ancestor confinement, privileged recovery execution, root units, process cuts or physical fallback. No full install/round/workspace/ARM or device run was performed. The accessor remains unwired.

The branch's doc paragraph still says the added tests are unrun because it was frozen for this gate. Update that statement while carrying the change; this packet retains the actual tested doc hash. No source edit followed the source-closure check.

Root later updated only the documentation count, committed 63300fca27 locally and folded the exact tested source into the same foundation PR. New combined-head qualification remains separate; there is no stacked accessor PR.
