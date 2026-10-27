Exact-source Mac query qualification
Head: ad693e626823eeea856f6cb45b749be8082eb83f
Base: f7252d81dc2e72494afc6ea842bd8b0445f1991c

setup-cloud:
  test result: ok. 166 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 35.27s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.73s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.49s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 164 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 14.05s
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.83s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Aggregate nonnested summaries: {'passed': 390, 'failed': 0, 'ignored': 5}

Commands:
  cargo +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  cargo +1.96.0 fmt --all --check
  cargo +1.96.0 clippy --locked --offline -p lamp-setup -p lamp-cloud --all-targets --message-format=short -- -D warnings
  cargo +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-run --message-format=json
  cargo +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-fail-fast -- --test-threads=1

Source, committed head/status, configuration and compiler-artifact guards passed. Actual and separately preserved outputs, depfiles and executable hashes were verified before target reuse and around runtime. The prior target boundary is recorded in the owned-target receipt. Only the final scope actual paths remain asserted after compilation; historical copied artifacts retain their own identities. Original log bytes and executable/depfile payloads remain local with receipt hashes. The sanitized packet contains no binary or raw logs.
No local ARM, device, provider, key, signing, publication, promotion or full-workspace gate. Prior original ARM results retain exact old heads and are not represented as execution of this head. No negative control or historical failed runtime was replayed.
Reset: the original ff03/737 sixteen-thread 388/1/5 wake failure at 163.063834 ms versus unchanged 150 ms remains separately retained and unexplained; this serial qualification is no cause or waiver. Cloud: original 129 environment failures and 553 Voice LEAK remain separately retained; no new-source pass erases them.
Cloud extra tests, if present: two endless writers are subsets of the full suite, not extra added counts. The first malformed maintenance invocation failed its argument-shape guard before any Cargo, output directory or test execution; corrected invocation used exact committed head/query digest.
