Exact-source Mac query qualification
Head: 23d0c3fbbfe050dae1979c0de0b8127e8ed4de60
Base: 3d8c75558e235c125b9cf953a66a6c193888fd9e

setup-query-wake:
  test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.50s
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.85s
Aggregate nonnested summaries: {'passed': 30, 'failed': 0, 'ignored': 0}

Commands:
  cargo +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  cargo +1.96.0 fmt --all --check
  cargo +1.96.0 clippy --locked --offline -p lamp-setup -p lamp-cloud --all-targets --message-format=short -- -D warnings
  cargo +1.96.0 test --locked --offline -p lamp-setup --lib --test wake --no-run --message-format=json
  cargo +1.96.0 test --locked --offline -p lamp-setup --lib --test wake --no-fail-fast -- --test-threads=1

Source, committed head/status, configuration and compiler-artifact guards passed. Actual and separately preserved outputs, depfiles and executable hashes were verified before target reuse and around runtime. The prior target boundary is recorded in the owned-target receipt. Only the final scope actual paths remain asserted after compilation; historical copied artifacts retain their own identities. Original log bytes and executable/depfile payloads remain local with receipt hashes. The sanitized packet contains no binary or raw logs.
No local ARM, device, provider, key, signing, publication, promotion or full-workspace gate. Prior original ARM results retain exact old heads and are not represented as execution of this head. No negative control or historical failed runtime was replayed.
Reset: the original ff03/737 sixteen-thread 388/1/5 wake failure at 163.063834 ms versus unchanged 150 ms remains separately retained and unexplained; this serial qualification is no cause or waiver. Cloud: original 129 environment failures and 553 Voice LEAK remain separately retained; no new-source pass erases them.
Cloud extra tests, if present: two endless writers are subsets of the full suite, not extra added counts. The first malformed maintenance invocation failed its argument-shape guard before any Cargo, output directory or test execution; corrected invocation used exact committed head/query digest.

Current runtime scope is Setup library 24 plus wake 6, all 30 passed, including eight query controls. Clippy covers Setup and Cloud all targets. Full Setup+Cloud 390/0/5 was executed once on 7a67/755 before the final foreign cfg(test) and documentation-only base delta; that result remains separately identified and is not a 3d8 full run. The exact committed owned blobs/modes and production input path comparison support that clean carry.
